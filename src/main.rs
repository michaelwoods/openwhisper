mod audio;
mod cli;
mod config;
mod gui;
mod hotkey;
mod hud;
mod notification;
mod output;
mod setup;
mod transcribe;
mod tray;


use anyhow::{Context, Result};
use clap::Parser;
use cli::{Cli, Commands};
use config::{Config, OutputMode};
use hound::{SampleFormat, WavSpec, WavWriter};
use notification::NotificationManager;
use output::OutputManager;
use std::io::{self, Cursor};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use audio::{AudioRecorder, EarconType, VadDecision, VadDetector};
use hotkey::{send_ipc_command, Action, HotkeyEngine, IpcCommand, IpcServer, PortalShortcutListener};
use transcribe::TranscriptionClient;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "openwhisper=info,warn".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();
    let config = Config::load_or_default();

    match cli.command.unwrap_or(Commands::Daemon { config: None }) {
        Commands::InitConfig => {
            let path = Config::config_path()?;
            if path.exists() {
                println!("Config file already exists at: {}", path.display());
            } else {
                config.save()?;
                println!("Default configuration written to: {}", path.display());
            }
            println!("\nConfiguration contents:\n{}", toml::to_string_pretty(&config)?);
            Ok(())
        }

        Commands::ListDevices => {
            println!("Available audio input devices:");
            let devices = AudioRecorder::list_input_devices()?;
            if devices.is_empty() {
                println!("  (None found)");
            } else {
                for (i, dev) in devices.iter().enumerate() {
                    println!("  [{}] {}", i + 1, dev);
                }
            }
            Ok(())
        }

        Commands::TestOvms { url, model } => {
            let mut test_config = config.clone();
            if let Some(u) = url {
                test_config.server_url = u;
            }
            if let Some(m) = model {
                test_config.model = m;
            }

            println!("Testing STT connection to: {}", test_config.server_url);
            println!("Model: {}", test_config.model);

            // Generate 1-second 16kHz silence/test tone WAV
            let spec = WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 16,
                sample_format: SampleFormat::Int,
            };
            let mut buf = Cursor::new(Vec::new());
            {
                let mut writer = WavWriter::new(&mut buf, spec)?;
                for i in 0..16000 {
                    let sample = (0.1 * (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 16000.0).sin() * 32767.0) as i16;
                    writer.write_sample(sample)?;
                }
                writer.finalize()?;
            }
            let wav_bytes = buf.into_inner();

            let client = TranscriptionClient::new(&test_config);
            let start = Instant::now();
            match client.transcribe(wav_bytes).await {
                Ok(text) => {
                    println!("SUCCESS! Server responded in {:.2}s", start.elapsed().as_secs_f32());
                    println!("Transcribed text: {:?}", text);
                }
                Err(err) => {
                    eprintln!("ERROR: Failed to transcribe audio: {err}");
                }
            }
            Ok(())
        }

        Commands::Toggle => {
            let res = send_ipc_command(&config.socket_path, IpcCommand::Toggle).await?;
            println!("{}: {}", res.status, res.message);
            Ok(())
        }

        Commands::PttDown => {
            let res = send_ipc_command(&config.socket_path, IpcCommand::PttDown).await?;
            println!("{}: {}", res.status, res.message);
            Ok(())
        }

        Commands::PttUp => {
            let res = send_ipc_command(&config.socket_path, IpcCommand::PttUp).await?;
            println!("{}: {}", res.status, res.message);
            Ok(())
        }

        Commands::Cancel => {
            let res = send_ipc_command(&config.socket_path, IpcCommand::Cancel).await?;
            println!("{}: {}", res.status, res.message);
            Ok(())
        }

        Commands::Status => {
            let res = send_ipc_command(&config.socket_path, IpcCommand::Status).await?;
            println!("{}: {}", res.status, res.message);
            Ok(())
        }

        Commands::Record { duration, no_paste } => {
            run_standalone_record(config, duration, no_paste).await
        }

        Commands::ConfigGui => {
            gui::run_gui(config)?;
            Ok(())
        }

        Commands::Reload => {
            let res = send_ipc_command(&config.socket_path, IpcCommand::ReloadConfig).await?;
            println!("{}: {}", res.status, res.message);
            Ok(())
        }

        Commands::Setup => {
            setup::run_setup()?;
            Ok(())
        }

        Commands::HudDemo { once } => {
            hud::run_hud_demo(once)?;
            Ok(())
        }

        Commands::TestHotkey => {
            hotkey::run_test_hotkey()?;
            Ok(())
        }

        Commands::Daemon { config: cfg_path } => {
            let active_config = if let Some(p) = cfg_path {
                let content = std::fs::read_to_string(&p)
                    .with_context(|| format!("Failed to read config file at {}", p))?;
                toml::from_str(&content)?
            } else {
                config
            };

            run_daemon(active_config).await
        }
    }

}

async fn run_standalone_record(config: Config, duration_secs: Option<u64>, no_paste: bool) -> Result<()> {
    let recorder = AudioRecorder::new(config.audio_device.clone());
    let client = TranscriptionClient::new(&config);
    let mut output_mgr = OutputManager::new(&config);
    let sound = config.sound_player();

    println!("🎙️  Recording audio... Speak into your microphone.");
    sound.play(EarconType::RecordingStarted);
    let active_rec = recorder.start_recording()?;

    if let Some(secs) = duration_secs {
        println!("Recording for {} seconds...", secs);
        tokio::time::sleep(Duration::from_secs(secs)).await;
    } else {
        println!("Press ENTER to stop recording...");
        let mut input = String::new();
        let _ = io::stdin().read_line(&mut input);
    }

    println!("⏳ Processing and transcribing...");
    sound.play(EarconType::RecordingStopped);
    let wav_bytes = active_rec.stop()?;
    let start_t = Instant::now();
    let text = client.transcribe(wav_bytes).await?;
    let elapsed = start_t.elapsed().as_secs_f32();

    sound.play(EarconType::Transcribed);
    println!("\n✅ Transcribed in {:.2}s:", elapsed);
    println!("--------------------------------------------------");
    println!("{}", text);
    println!("--------------------------------------------------");

    if !no_paste && config.output_mode == OutputMode::Paste {
        println!("Inserting text into active cursor & copying to clipboard...");
        output_mgr.output_text(&text)?;
    } else {
        println!("Copying text to clipboard...");
        output::set_clipboard(&text)?;
    }

    Ok(())
}

async fn run_daemon(config: Config) -> Result<()> {
    tracing::info!("Starting OpenWhisper daemon...");
    tracing::info!("STT Endpoint: {}", config.server_url);
    tracing::info!("Model: {}", config.model);
    tracing::info!("PTT threshold: {} ms", config.ptt_threshold_ms);
    tracing::info!("Output mode: {:?}", config.output_mode);
    tracing::info!("Sound feedback: {}", config.sound_feedback);
    tracing::info!("VAD enabled: {}", config.vad_enabled);
    tracing::info!("Formatting mode: {:?}", config.formatting_mode);

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<IpcCommand>(32);
    let notifications = Arc::new(RwLock::new(NotificationManager::new(config.show_notifications)));
    let sound = Arc::new(RwLock::new(config.sound_player()));
    let client = Arc::new(RwLock::new(TranscriptionClient::new(&config)));
    let output_mgr = Arc::new(Mutex::new(OutputManager::new(&config)));
    let engine = Arc::new(Mutex::new(HotkeyEngine::new(config.ptt_threshold_ms)));
    let active_recording: Arc<Mutex<Option<audio::ActiveRecording>>> = Arc::new(Mutex::new(None));
    let recorder = Arc::new(AudioRecorder::new(config.audio_device.clone()));
    let mut active_config = config;

    // Spawn IPC Unix Socket Server
    let ipc_server = IpcServer::new(active_config.socket_path.clone(), cmd_tx.clone());
    tokio::spawn(async move {
        if let Err(err) = ipc_server.run().await {
            tracing::error!("IPC server error: {err}");
        }
    });

    // Spawn XDG Portal Shortcut Listener (Wayland/KDE)
    let portal_listener = PortalShortcutListener::new(cmd_tx.clone());
    tokio::spawn(async move {
        if let Err(err) = portal_listener.run().await {
            tracing::warn!("Portal shortcut listener ended: {err}");
        }
    });

    // Spawn Linux evdev Hardware Hotkey Listener (Push-to-Talk)
    let mut evdev_handle = if active_config.evdev_hotkey_enabled {
        hotkey::start_evdev_listener(&active_config.evdev_hotkey, cmd_tx.clone())
    } else {
        None
    };

    tracing::info!("OpenWhisper daemon ready! Waiting for hotkey / IPC events...");
    println!("OpenWhisper daemon running in background.");
    println!("Trigger via:");
    if active_config.evdev_hotkey_enabled {
        println!("  • Hardware hotkey (evdev): {} (Hold for PTT, tap to toggle)", active_config.evdev_hotkey);
    }
    println!("  • Global shortcut (if configured in KDE/portal)");
    println!("  • CLI: `openwhisper toggle`");
    println!("  • CLI: `openwhisper ptt-down` (press) & `openwhisper ptt-up` (release)");
    println!("  • CLI: `openwhisper reload` (reloads config without restarting)");

    // Initialize StatusNotifierItem System Tray
    let (tray_ctrl, _) = tray::start_tray_service(active_config.socket_path.clone()).await;

    // Initialize Floating HUD Overlay
    let hud_ctrl = hud::start_hud_service(active_config.hud_enabled, active_config.hud_position);

    while let Some(cmd) = cmd_rx.recv().await {
        tracing::info!("Daemon event loop received: {:?}", cmd);
        let action = {
            let mut eng = engine.lock().await;
            match cmd {
                IpcCommand::PttDown => eng.on_press(),
                IpcCommand::PttUp => eng.on_release(),
                IpcCommand::Toggle => eng.on_toggle(),
                IpcCommand::Cancel => eng.on_cancel(),
                IpcCommand::Status => {
                    let state = eng.current_state();
                    tracing::info!("Status requested: current state is {:?}", state);
                    Action::None
                }
                IpcCommand::ReloadConfig => {
                    tracing::info!("ReloadConfig IPC command received. Reloading configuration from disk...");
                    match Config::load() {
                        Ok(new_cfg) => {
                            tracing::info!("Successfully reloaded configuration from disk.");
                            *client.write().await = TranscriptionClient::new(&new_cfg);
                            *sound.write().await = new_cfg.sound_player();
                            notifications.write().await.set_enabled(new_cfg.show_notifications);
                            output_mgr.lock().await.update_config(&new_cfg);
                            eng.set_ptt_threshold_ms(new_cfg.ptt_threshold_ms);
                            hud_ctrl.set_enabled(new_cfg.hud_enabled);
                            hud_ctrl.set_position(new_cfg.hud_position);
                            #[cfg(target_os = "linux")]
                            crate::setup::sync_kwin_hud_position(new_cfg.hud_position);

                            // Reload evdev hardware listener if hotkey settings changed
                            if new_cfg.evdev_hotkey_enabled != active_config.evdev_hotkey_enabled
                                || new_cfg.evdev_hotkey != active_config.evdev_hotkey
                            {
                                if let Some(h) = evdev_handle.take() {
                                    h.stop();
                                }
                                if new_cfg.evdev_hotkey_enabled {
                                    evdev_handle = hotkey::start_evdev_listener(&new_cfg.evdev_hotkey, cmd_tx.clone());
                                }
                            }

                            active_config = new_cfg;
                            tracing::info!("All daemon components updated with reloaded config.");
                        }
                        Err(err) => {
                            tracing::error!("Failed to reload config from disk: {err}");
                        }
                    }
                    Action::None
                }
                IpcCommand::PreviewHud => {
                    tracing::info!("PreviewHud IPC command received. Triggering 1-shot HUD demo preview...");
                    let ctrl = hud_ctrl.clone();
                    tokio::spawn(async move {
                        ctrl.set_recording();
                        for i in 0..50 {
                            let t = i as f32 * 0.05;
                            let rms = ((t * 4.0).sin() * 0.5 + 0.5) * 0.08 + 0.01;
                            ctrl.update_audio_level(rms);
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                        ctrl.set_transcribing();
                        tokio::time::sleep(Duration::from_millis(1400)).await;
                        ctrl.set_completed("Preview: OpenWhisper HUD overlay active");
                        tokio::time::sleep(Duration::from_millis(2200)).await;
                        ctrl.set_idle();
                    });
                    Action::None
                }
            }
        };

        match action {
            Action::StartRecording => {
                tracing::info!("Action: StartRecording");
                tray_ctrl.set_state(tray::TrayState::Recording);
                hud_ctrl.set_recording();
                notifications.read().await.recording_started();
                sound.read().await.play(EarconType::RecordingStarted);

                match recorder.start_recording() {
                    Ok(rec) => {
                        let sample_buf = rec.sample_buffer();
                        let is_rec_handle = rec.is_recording_handle();

                        let mut guard = active_recording.lock().await;
                        *guard = Some(rec);

                        // Spawn concurrent monitor for audio RMS visualizer and VAD silence gating
                        let cmd_tx_vad = cmd_tx.clone();
                        let vad_cfg = active_config.vad_config();
                        let vad_enabled = active_config.vad_enabled;
                        let hud_ctrl_monitor = hud_ctrl.clone();

                        tokio::spawn(async move {
                            let mut detector = if vad_enabled {
                                Some(VadDetector::new(vad_cfg))
                            } else {
                                None
                            };
                            let mut read_idx = 0;
                            while is_rec_handle.load(std::sync::atomic::Ordering::Relaxed) {
                                tokio::time::sleep(Duration::from_millis(60)).await;
                                if !is_rec_handle.load(std::sync::atomic::Ordering::Relaxed) {
                                    break;
                                }
                                let chunk = {
                                    if let Ok(guard) = sample_buf.lock() {
                                        let total = guard.len();
                                        if read_idx < total {
                                            let slice = guard[read_idx..].to_vec();
                                            read_idx = total;
                                            slice
                                        } else {
                                            Vec::new()
                                        }
                                    } else {
                                        Vec::new()
                                    }
                                };

                                if !chunk.is_empty() {
                                    let rms = VadDetector::calculate_rms(&chunk);
                                    hud_ctrl_monitor.update_audio_level(rms);

                                    if let Some(ref mut d) = detector {
                                        if d.process_chunk(&chunk, Instant::now())
                                            == VadDecision::SilenceTimeout
                                        {
                                            tracing::info!(
                                                "VAD silence threshold reached: automatically stopping recording"
                                            );
                                            let _ = cmd_tx_vad.send(IpcCommand::Toggle).await;
                                            break;
                                        }
                                    }
                                }
                            }
                        });
                    }
                    Err(err) => {
                        tracing::error!("Failed to start recording: {err}");
                        tray_ctrl.set_state(tray::TrayState::Error);
                        hud_ctrl.set_error(&format!("Mic error: {err}"));
                        notifications.read().await.error(&format!("Mic error: {err}"));
                        sound.read().await.play(EarconType::Error);
                        let mut eng = engine.lock().await;
                        eng.on_cancel();
                    }
                }
            }

            Action::StopAndTranscribe => {
                tracing::info!("Action: StopAndTranscribe");
                tray_ctrl.set_state(tray::TrayState::Transcribing);
                hud_ctrl.set_transcribing();
                notifications.read().await.transcribing();
                sound.read().await.play(EarconType::RecordingStopped);

                let rec_opt = {
                    let mut guard = active_recording.lock().await;
                    guard.take()
                };

                if let Some(rec) = rec_opt {
                    let client_clone = Arc::clone(&client);
                    let output_mgr_clone = Arc::clone(&output_mgr);
                    let notif_clone = Arc::clone(&notifications);
                    let sound_clone = Arc::clone(&sound);
                    let engine_clone = Arc::clone(&engine);
                    let tray_ctrl_clone = tray_ctrl.clone();
                    let hud_ctrl_clone = hud_ctrl.clone();

                    let wav_res = rec.stop();
                    tokio::spawn(async move {
                        match wav_res {
                            Ok(wav_bytes) => {
                                let start = Instant::now();
                                let transcribe_res = {
                                    let cl = client_clone.read().await.clone();
                                    cl.transcribe(wav_bytes).await
                                };
                                match transcribe_res {
                                    Ok(text) => {
                                        let duration = start.elapsed().as_secs_f32();
                                        tracing::info!("Transcribed in {:.2}s: {:?}", duration, text);
                                        tray_ctrl_clone.set_state(tray::TrayState::Idle);
                                        hud_ctrl_clone.set_completed(&text);
                                        notif_clone.read().await.transcribed(&text);
                                        sound_clone.read().await.play(EarconType::Transcribed);

                                        let mut out = output_mgr_clone.lock().await;
                                        if let Err(err) = out.output_text(&text) {
                                            tracing::error!("Output injection error: {err}");
                                            notif_clone.read().await.error(&format!("Output error: {err}"));
                                            sound_clone.read().await.play(EarconType::Error);
                                        }
                                    }
                                    Err(err) => {
                                        tracing::error!("Transcription error: {err}");
                                        tray_ctrl_clone.set_state(tray::TrayState::Error);
                                        hud_ctrl_clone.set_error(&format!("STT failed: {err}"));
                                        let reset_ctrl = tray_ctrl_clone.clone();
                                        tokio::spawn(async move {
                                            tokio::time::sleep(Duration::from_secs(3)).await;
                                            reset_ctrl.set_state(tray::TrayState::Idle);
                                        });
                                        notif_clone.read().await.error(&format!("STT failed: {err}"));
                                        sound_clone.read().await.play(EarconType::Error);
                                    }
                                }
                            }
                            Err(err) => {
                                tracing::error!("Audio stop/encoding error: {err}");
                                tray_ctrl_clone.set_state(tray::TrayState::Error);
                                hud_ctrl_clone.set_error("Audio encoding error");
                                notif_clone.read().await.error(&format!("Audio encoding error: {err}"));
                                sound_clone.read().await.play(EarconType::Error);
                            }
                        }

                        let mut eng = engine_clone.lock().await;
                        eng.on_transcription_finished();
                    });
                } else {
                    tray_ctrl.set_state(tray::TrayState::Idle);
                    hud_ctrl.set_idle();
                    let mut eng = engine.lock().await;
                    eng.on_transcription_finished();
                }
            }

            Action::CancelRecording => {
                tracing::info!("Action: CancelRecording");
                tray_ctrl.set_state(tray::TrayState::Idle);
                hud_ctrl.set_idle();
                let mut guard = active_recording.lock().await;
                *guard = None;
                notifications.read().await.error("Recording cancelled");
                sound.read().await.play(EarconType::Error);
            }

            Action::None => {}
        }
    }

    Ok(())
}
