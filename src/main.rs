use openwhisper::audio;
use openwhisper::autostart;
use openwhisper::cli;
use openwhisper::config;
use openwhisper::doctor;
use openwhisper::gui;
use openwhisper::history;
use openwhisper::hotkey;
use openwhisper::hud;
use openwhisper::notification;
use openwhisper::output;
use openwhisper::setup;
use openwhisper::transcribe;
use openwhisper::tray;
use openwhisper::ui_service;

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
use tokio::sync::{Mutex, RwLock, mpsc};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use audio::{AudioRecorder, EarconType, VadDecision, VadDetector};
use hotkey::{
    Action, HotkeyEngine, IpcCommand, IpcServer, PortalShortcutListener, send_ipc_command,
};
use transcribe::TranscriptionClient;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "openwhisper=info,warn".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    let cli = Cli::parse();
    let config = Config::load_or_default();

    match cli.command.unwrap_or(Commands::Daemon {
        config: None,
        with_ui: false,
    }) {
        Commands::InitConfig => {
            let path = Config::config_path()?;
            if path.exists() {
                println!("Config file already exists at: {}", path.display());
            } else {
                config.save()?;
                println!("Default configuration written to: {}", path.display());
            }
            println!(
                "\nConfiguration contents:\n{}",
                toml::to_string_pretty(&config)?
            );
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
                    let sample = (0.1
                        * (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 16000.0).sin()
                        * 32767.0) as i16;
                    writer.write_sample(sample)?;
                }
                writer.finalize()?;
            }
            let wav_bytes = buf.into_inner();

            let client = TranscriptionClient::new(&test_config);
            let start = Instant::now();
            match client.transcribe(wav_bytes).await {
                Ok(text) => {
                    println!(
                        "SUCCESS! Server responded in {:.2}s",
                        start.elapsed().as_secs_f32()
                    );
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

        Commands::History {
            limit,
            search,
            copy,
            play,
            delete,
            clear,
            json,
            gui,
        } => {
            let mgr = Arc::new(history::HistoryManager::new(None)?);

            if let Some(ref dir_str) = config.save_audio_dir {
                let _ = mgr.backfill_audio_paths(std::path::Path::new(dir_str));
            }

            if gui {
                history::gui::run_history_gui(mgr)?;
                return Ok(());
            }

            if let Some(id) = play {
                if let Some(entry) = mgr.get_by_id(id)? {
                    if let Some(ref path_str) = entry.audio_path {
                        let path = std::path::Path::new(path_str);
                        if path.exists() {
                            println!("▶ Playing audio for transcription #{}: {}", id, path_str);
                            crate::audio::play_wav_file_blocking(path)?;
                            println!("Playback finished.");
                        } else {
                            eprintln!("Error: Audio file not found at {}", path_str);
                        }
                    } else {
                        eprintln!(
                            "Error: No audio recording associated with history entry #{}",
                            id
                        );
                    }
                } else {
                    eprintln!("Error: No history entry found with ID #{}", id);
                }
                return Ok(());
            }

            if let Some(id) = copy {
                if let Some(entry) = mgr.get_by_id(id)? {
                    output::set_clipboard(&entry.text)?;
                    println!("Copied transcription #{} to clipboard:\n{}", id, entry.text);
                } else {
                    eprintln!("Error: No history entry found with ID #{}", id);
                }
                return Ok(());
            }

            if let Some(id) = delete {
                if mgr.delete(id)? {
                    println!("Deleted history entry #{}", id);
                } else {
                    eprintln!("Error: No history entry found with ID #{}", id);
                }
                return Ok(());
            }

            if clear {
                mgr.clear_all()?;
                if let Some(ref dir) = config.save_audio_dir {
                    println!(
                        "All transcription history cleared. (Note: Saved audio recordings in '{}' were not deleted.)",
                        dir
                    );
                } else {
                    println!(
                        "All transcription history cleared. (Note: Saved audio recordings on disk were not deleted.)"
                    );
                }
                return Ok(());
            }

            let entries = if let Some(ref q) = search {
                mgr.search(q, limit)?
            } else {
                mgr.list(limit, 0)?
            };

            if json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
                return Ok(());
            }

            if entries.is_empty() {
                if let Some(ref q) = search {
                    println!("No transcriptions found matching query: {:?}", q);
                } else {
                    println!(
                        "No transcription history found. Run dictations with OpenWhisper to populate history."
                    );
                }
                return Ok(());
            }

            let total_in_db = mgr.count()?;
            println!("\n  ID  | Time (Local)        | Dur   | Chars | Audio | Transcription");
            println!(
                "------+---------------------+-------+-------+-------+--------------------------------------------------"
            );
            for e in &entries {
                let local_time = if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&e.timestamp)
                {
                    let local: chrono::DateTime<chrono::Local> = chrono::DateTime::from(dt);
                    local.format("%Y-%m-%d %H:%M:%S").to_string()
                } else {
                    e.timestamp.clone()
                };

                let display_text = if e.text.chars().count() > 46 {
                    let truncated: String = e.text.chars().take(43).collect();
                    format!("{}...", truncated)
                } else {
                    e.text.clone()
                };

                let audio_col = if let Some(ref p) = e.audio_path {
                    if std::path::Path::new(p).exists() {
                        " 🔊 Yes"
                    } else {
                        "   -   "
                    }
                } else {
                    "   -   "
                };

                println!(
                    "{:>5} | {:<19} | {:>4.1}s | {:>5} | {} | {}",
                    e.id.unwrap_or(0),
                    local_time,
                    e.duration_secs,
                    e.char_count,
                    audio_col,
                    display_text
                );
            }
            println!(
                "------+---------------------+-------+-------+-------+--------------------------------------------------"
            );
            println!(
                "Showing {} of {} entries in database. Use `openwhisper history --gui` for GUI, `--play <ID>` to play, or `--copy <ID>` to copy.\n",
                entries.len(),
                total_in_db
            );

            Ok(())
        }

        Commands::Doctor { json } => {
            doctor::run_doctor(&config, json).await?;
            Ok(())
        }

        Commands::Ui => {
            ui_service::run_ui_service(config).await?;
            Ok(())
        }

        Commands::Autostart {
            enable,
            disable,
            json,
        } => {
            if enable {
                autostart::enable()?;
                println!("✅ OpenWhisper launch at startup enabled successfully.");
            } else if disable {
                autostart::disable()?;
                println!("✅ OpenWhisper launch at startup disabled.");
            } else if json {
                let st = autostart::status();
                println!("{}", serde_json::to_string_pretty(&st)?);
            } else {
                let st = autostart::status();
                print!("{}", autostart::format_status_report(&st));
            }
            Ok(())
        }

        Commands::Daemon {
            config: cfg_path,
            with_ui,
        } => {
            let active_config = if let Some(p) = cfg_path {
                let content = std::fs::read_to_string(&p)
                    .with_context(|| format!("Failed to read config file at {}", p))?;
                toml::from_str(&content)?
            } else {
                config
            };

            run_daemon(active_config, with_ui).await
        }
    }
}

async fn run_standalone_record(
    config: Config,
    duration_secs: Option<u64>,
    no_paste: bool,
) -> Result<()> {
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
    let wav_bytes = active_rec.stop_with_options(config.noise_suppression)?;
    let start_t = Instant::now();
    let text = client.transcribe(wav_bytes.clone()).await?;
    let elapsed = start_t.elapsed().as_secs_f32();

    sound.play(EarconType::Transcribed);
    println!("\n✅ Transcribed in {:.2}s:", elapsed);
    println!("--------------------------------------------------");
    println!("{}", text);
    println!("--------------------------------------------------");

    let audio_path = if let Some(ref dir_str) = config.save_audio_dir {
        match crate::audio::save_recording_to_dir(std::path::Path::new(dir_str), &wav_bytes, &text)
        {
            Ok(p) => Some(p.to_string_lossy().to_string()),
            Err(e) => {
                tracing::warn!("Failed to save audio recording to {}: {}", dir_str, e);
                None
            }
        }
    } else {
        None
    };

    let hist_entry = history::HistoryEntry {
        id: None,
        timestamp: chrono::Utc::now().to_rfc3339(),
        text: text.clone(),
        raw_text: None,
        duration_secs: elapsed,
        char_count: text.chars().count(),
        model: config.model.clone(),
        output_mode: format!("{:?}", config.output_mode),
        audio_path,
    };
    if let Ok(mgr) = history::HistoryManager::new(None)
        && let Err(e) = mgr.record(&hist_entry)
    {
        tracing::warn!("Failed to persist transcription to history: {e}");
    }

    if !no_paste && config.output_mode == OutputMode::Paste {
        println!("Inserting text into active cursor & copying to clipboard...");
        output_mgr.output_text(&text)?;
    } else {
        println!("Copying text to clipboard...");
        output::set_clipboard(&text)?;
    }

    Ok(())
}

struct UiBridge {
    event_tx: tokio::sync::broadcast::Sender<hotkey::IpcEvent>,
    tray: Option<tray::TrayController>,
    hud: Option<hud::HudController>,
}

impl UiBridge {
    fn new(
        event_tx: tokio::sync::broadcast::Sender<hotkey::IpcEvent>,
        tray: Option<tray::TrayController>,
        hud: Option<hud::HudController>,
    ) -> Self {
        Self {
            event_tx,
            tray,
            hud,
        }
    }

    fn set_recording(&self, device: Option<String>) {
        let _ = self.event_tx.send(hotkey::IpcEvent::StateChanged {
            state: hotkey::DaemonState::Recording {
                device: device.clone(),
            },
        });
        if let Some(ref t) = self.tray {
            t.set_state(tray::TrayState::Recording);
            t.set_active_device(device);
        }
        if let Some(ref h) = self.hud {
            h.set_recording();
        }
    }

    fn set_degraded(&self, reason: &str, device: Option<String>) {
        let _ = self.event_tx.send(hotkey::IpcEvent::StateChanged {
            state: hotkey::DaemonState::Degraded {
                reason: reason.to_string(),
            },
        });
        if let Some(ref t) = self.tray {
            t.set_state(tray::TrayState::Degraded);
            t.set_active_device(device);
        }
        if let Some(ref h) = self.hud {
            h.set_recording();
        }
    }

    fn update_audio_level(&self, rms: f32) {
        let _ = self.event_tx.send(hotkey::IpcEvent::AudioLevel { rms });
        if let Some(ref h) = self.hud {
            h.update_audio_level(rms);
        }
    }

    fn set_transcribing(&self) {
        let _ = self.event_tx.send(hotkey::IpcEvent::StateChanged {
            state: hotkey::DaemonState::Transcribing,
        });
        if let Some(ref t) = self.tray {
            t.set_state(tray::TrayState::Transcribing);
        }
        if let Some(ref h) = self.hud {
            h.set_transcribing();
        }
    }

    fn set_completed(&self, text: &str, duration_secs: f32) {
        let _ = self
            .event_tx
            .send(hotkey::IpcEvent::TranscriptionCompleted {
                text: text.to_string(),
                duration_secs,
            });
        let _ = self.event_tx.send(hotkey::IpcEvent::StateChanged {
            state: hotkey::DaemonState::Idle,
        });
        if let Some(ref t) = self.tray {
            t.set_diagnostics(Some(duration_secs), None);
            t.set_state(tray::TrayState::Idle);
            t.add_history(text.to_string());
        }
        if let Some(ref h) = self.hud {
            h.set_completed(text);
        }
    }

    fn set_error(&self, message: &str) {
        let _ = self.event_tx.send(hotkey::IpcEvent::StateChanged {
            state: hotkey::DaemonState::Error {
                message: message.to_string(),
            },
        });
        if let Some(ref t) = self.tray {
            t.set_diagnostics(None, Some(message.to_string()));
            t.set_state(tray::TrayState::Error);
        }
        if let Some(ref h) = self.hud {
            h.set_error(message);
        }
    }

    fn set_idle(&self) {
        let _ = self.event_tx.send(hotkey::IpcEvent::StateChanged {
            state: hotkey::DaemonState::Idle,
        });
        if let Some(ref t) = self.tray {
            t.set_state(tray::TrayState::Idle);
        }
        if let Some(ref h) = self.hud {
            h.set_idle();
        }
    }

    fn update_config(&self, new_cfg: &Config) {
        let _ = self.event_tx.send(hotkey::IpcEvent::Diagnostics {
            server_url: new_cfg.server_url.clone(),
            active_device: new_cfg.audio_device.clone(),
            last_latency: None,
            last_error: None,
        });
        if let Some(ref t) = self.tray {
            t.set_server_url(new_cfg.server_url.clone());
        }
        if let Some(ref h) = self.hud {
            h.set_enabled(new_cfg.hud_enabled);
            h.set_position(new_cfg.hud_position);
        }
    }
}

async fn run_daemon(config: Config, with_ui: bool) -> Result<()> {
    // Single-instance guard: if another daemon is already running, exit cleanly
    if let Ok(resp) = hotkey::ipc::send_ipc_command_sync(&config.socket_path, IpcCommand::Status) {
        println!(
            "OpenWhisper daemon is already running (socket responsive: {}).",
            resp.status
        );
        println!("Use `openwhisper status` or `openwhisper toggle` to interact with it.");
        return Ok(());
    }

    tracing::info!("Starting OpenWhisper daemon...");
    tracing::info!("STT Endpoint: {}", config.server_url);
    tracing::info!("Model: {}", config.model);
    tracing::info!("PTT threshold: {} ms", config.ptt_threshold_ms);
    tracing::info!("Output mode: {:?}", config.output_mode);
    tracing::info!("Sound feedback: {}", config.sound_feedback);
    tracing::info!("VAD enabled: {}", config.vad_enabled);
    tracing::info!("Formatting mode: {:?}", config.formatting_mode);
    tracing::info!(
        "UI Mode: {}",
        if with_ui {
            "Unified (In-process Tray + HUD)"
        } else {
            "Headless (IPC Event Stream)"
        }
    );

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<IpcCommand>(32);
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel::<hotkey::IpcEvent>(128);

    let notifications = Arc::new(RwLock::new(NotificationManager::new(
        config.show_notifications,
    )));
    let sound = Arc::new(RwLock::new(config.sound_player()));
    let client = Arc::new(RwLock::new(TranscriptionClient::new(&config)));
    let output_mgr = Arc::new(Mutex::new(OutputManager::new(&config)));
    let engine = Arc::new(Mutex::new(HotkeyEngine::new(config.ptt_threshold_ms)));
    let active_recording: Arc<Mutex<Option<audio::ActiveRecording>>> = Arc::new(Mutex::new(None));
    let mut recorder = Arc::new(AudioRecorder::new(config.audio_device.clone()));
    let history_mgr = Arc::new(history::HistoryManager::new(None)?);
    if let Some(ref dir_str) = config.save_audio_dir
        && let Err(err) = history_mgr.backfill_audio_paths(std::path::Path::new(dir_str))
    {
        tracing::warn!("Failed to backfill audio paths from {}: {}", dir_str, err);
    }
    let mut active_config = config;

    // Spawn IPC Unix Socket Server with Event Stream broadcaster
    let ipc_server = IpcServer::new(
        active_config.socket_path.clone(),
        cmd_tx.clone(),
        Some(event_tx.clone()),
    );
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

    // Initialize in-process Tray & HUD only if with_ui is enabled
    let (tray_ctrl, hud_ctrl) = if with_ui {
        let (tray, _) = tray::start_tray_service(
            active_config.socket_path.clone(),
            active_config.server_url.clone(),
        )
        .await;
        let hud = hud::start_hud_service(active_config.hud_enabled, active_config.hud_position);
        (Some(tray), Some(hud))
    } else {
        (None, None)
    };
    let bridge = Arc::new(UiBridge::new(event_tx, tray_ctrl, hud_ctrl));

    tracing::info!("OpenWhisper daemon ready! Waiting for hotkey / IPC events...");
    println!("OpenWhisper daemon running in background.");
    println!("Trigger via:");
    if active_config.evdev_hotkey_enabled {
        println!(
            "  • Hardware hotkey (evdev): {} (Hold for PTT, tap to toggle)",
            active_config.evdev_hotkey
        );
    }
    println!("  • Global shortcut (if configured in KDE/portal)");
    println!("  • CLI: `openwhisper toggle`");
    println!("  • CLI: `openwhisper ptt-down` (press) & `openwhisper ptt-up` (release)");
    println!("  • CLI: `openwhisper reload` (reloads config without restarting)");
    if !with_ui {
        println!("  • UI service: run `openwhisper ui` to show system tray icon and HUD overlay.");
    }

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
                IpcCommand::Subscribe => Action::None,
                IpcCommand::ReloadConfig => {
                    tracing::info!(
                        "ReloadConfig IPC command received. Reloading configuration from disk..."
                    );
                    match Config::load() {
                        Ok(new_cfg) => {
                            tracing::info!("Successfully reloaded configuration from disk.");
                            *client.write().await = TranscriptionClient::new(&new_cfg);
                            *sound.write().await = new_cfg.sound_player();
                            notifications
                                .write()
                                .await
                                .set_enabled(new_cfg.show_notifications);
                            output_mgr.lock().await.update_config(&new_cfg);
                            eng.set_ptt_threshold_ms(new_cfg.ptt_threshold_ms);
                            bridge.update_config(&new_cfg);
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
                                    evdev_handle = hotkey::start_evdev_listener(
                                        &new_cfg.evdev_hotkey,
                                        cmd_tx.clone(),
                                    );
                                }
                            }

                            // Reload audio recorder if device configuration changed
                            if new_cfg.audio_device != active_config.audio_device {
                                recorder =
                                    Arc::new(AudioRecorder::new(new_cfg.audio_device.clone()));
                                tracing::info!(
                                    "Audio input device reconfigured to: {:?}",
                                    new_cfg.audio_device
                                );
                            }

                            if let Some(ref dir_str) = new_cfg.save_audio_dir {
                                let _ =
                                    history_mgr.backfill_audio_paths(std::path::Path::new(dir_str));
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
                    tracing::info!(
                        "PreviewHud IPC command received. Triggering 1-shot HUD demo preview..."
                    );
                    let bridge_preview = bridge.clone();
                    tokio::spawn(async move {
                        bridge_preview.set_recording(None);
                        for i in 0..50 {
                            let t = i as f32 * 0.05;
                            let rms = ((t * 4.0).sin() * 0.5 + 0.5) * 0.08 + 0.01;
                            bridge_preview.update_audio_level(rms);
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                        bridge_preview.set_transcribing();
                        tokio::time::sleep(Duration::from_millis(1400)).await;
                        bridge_preview
                            .set_completed("Preview: OpenWhisper HUD overlay active", 1.4);
                        tokio::time::sleep(Duration::from_millis(2200)).await;
                        bridge_preview.set_idle();
                    });
                    Action::None
                }
            }
        };

        match action {
            Action::StartRecording => {
                tracing::info!("Action: StartRecording");
                notifications.read().await.recording_started();
                sound.read().await.play(EarconType::RecordingStarted);

                match recorder.start_recording() {
                    Ok(rec) => {
                        if rec.is_fallback() {
                            tracing::warn!(
                                "Preferred microphone unavailable; recording using fallback microphone: {}",
                                rec.device_name()
                            );
                            bridge.set_degraded(
                                "Fallback microphone in use",
                                Some(format!("{} (Fallback)", rec.device_name())),
                            );
                        } else {
                            bridge.set_recording(Some(rec.device_name().to_string()));
                        }

                        let sample_buf = rec.sample_buffer();
                        let is_rec_handle = rec.is_recording_handle();

                        let mut guard = active_recording.lock().await;
                        *guard = Some(rec);

                        // Spawn concurrent monitor for audio RMS visualizer and VAD silence gating
                        let cmd_tx_vad = cmd_tx.clone();
                        let vad_cfg = active_config.vad_config();
                        let vad_enabled = active_config.vad_enabled;
                        let bridge_monitor = bridge.clone();

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
                                    bridge_monitor.update_audio_level(rms);

                                    if let Some(ref mut d) = detector
                                        && d.process_chunk(&chunk, Instant::now())
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
                        });
                    }
                    Err(err) => {
                        tracing::error!("Failed to start recording: {err}");
                        bridge.set_error(&format!("Mic error: {err}"));
                        notifications
                            .read()
                            .await
                            .error(&format!("Mic error: {err}"));
                        sound.read().await.play(EarconType::Error);
                        let mut eng = engine.lock().await;
                        eng.on_cancel();
                    }
                }
            }

            Action::StopAndTranscribe => {
                tracing::info!("Action: StopAndTranscribe");
                bridge.set_transcribing();
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
                    let bridge_clone = Arc::clone(&bridge);
                    let history_mgr_clone = Arc::clone(&history_mgr);

                    let noise_suppression = active_config.noise_suppression;
                    let save_audio_dir = active_config.save_audio_dir.clone();
                    let output_mode = active_config.output_mode;
                    if rec.has_stream_error() {
                        tracing::warn!(
                            "Audio capture stream encountered errors during recording on device: {}",
                            rec.device_name()
                        );
                    }
                    let wav_res = rec.stop_with_options(noise_suppression);
                    tokio::spawn(async move {
                        match wav_res {
                            Ok(wav_bytes) => {
                                let start = Instant::now();
                                let transcribe_res = {
                                    let cl = client_clone.read().await.clone();
                                    cl.transcribe(wav_bytes.clone()).await
                                };
                                match transcribe_res {
                                    Ok(text) => {
                                        let duration = start.elapsed().as_secs_f32();
                                        tracing::info!(
                                            "Transcribed in {:.2}s: {:?}",
                                            duration,
                                            text
                                        );

                                        let audio_path = if let Some(ref dir_str) = save_audio_dir {
                                            match crate::audio::save_recording_to_dir(
                                                std::path::Path::new(dir_str),
                                                &wav_bytes,
                                                &text,
                                            ) {
                                                Ok(p) => Some(p.to_string_lossy().to_string()),
                                                Err(e) => {
                                                    tracing::warn!(
                                                        "Failed to save audio recording to {}: {}",
                                                        dir_str,
                                                        e
                                                    );
                                                    None
                                                }
                                            }
                                        } else {
                                            None
                                        };

                                        let hist_entry = history::HistoryEntry {
                                            id: None,
                                            timestamp: chrono::Utc::now().to_rfc3339(),
                                            text: text.clone(),
                                            raw_text: None,
                                            duration_secs: duration,
                                            char_count: text.chars().count(),
                                            model: client_clone.read().await.model().to_string(),
                                            output_mode: format!("{:?}", output_mode),
                                            audio_path,
                                        };
                                        if let Err(err) = history_mgr_clone.record(&hist_entry) {
                                            tracing::warn!(
                                                "Failed to persist transcription history: {err}"
                                            );
                                        }

                                        bridge_clone.set_completed(&text, duration);
                                        notif_clone.read().await.transcribed(&text);
                                        sound_clone.read().await.play(EarconType::Transcribed);

                                        let mut out = output_mgr_clone.lock().await;
                                        if let Err(err) = out.output_text(&text) {
                                            tracing::error!("Output injection error: {err}");
                                            notif_clone
                                                .read()
                                                .await
                                                .error(&format!("Output error: {err}"));
                                            sound_clone.read().await.play(EarconType::Error);
                                        }
                                    }
                                    Err(err) => {
                                        let err_str = err.to_string();
                                        let descriptive_msg = if err_str
                                            .contains("Failed to connect")
                                            || err_str.contains("Connection refused")
                                        {
                                            let srv =
                                                client_clone.read().await.server_url().to_string();
                                            format!("Server unreachable ({srv})")
                                        } else if err_str.contains("401") || err_str.contains("403")
                                        {
                                            "Auth error: Invalid API key".to_string()
                                        } else if err_str.contains("404") {
                                            let mdl = client_clone.read().await.model().to_string();
                                            format!("Model not found or 404 ({mdl})")
                                        } else if err_str.contains("empty") {
                                            "No speech detected (empty audio)".to_string()
                                        } else {
                                            format!("STT failed: {err}")
                                        };

                                        tracing::error!("Transcription error: {err}");
                                        bridge_clone.set_error(&descriptive_msg);
                                        let reset_bridge = bridge_clone.clone();
                                        tokio::spawn(async move {
                                            tokio::time::sleep(Duration::from_secs(3)).await;
                                            reset_bridge.set_idle();
                                        });
                                        notif_clone.read().await.error(&descriptive_msg);
                                        sound_clone.read().await.play(EarconType::Error);
                                    }
                                }
                            }
                            Err(err) => {
                                let descriptive_msg = format!("Audio encoding error: {err}");
                                tracing::error!("Audio stop/encoding error: {err}");
                                bridge_clone.set_error(&descriptive_msg);
                                notif_clone.read().await.error(&descriptive_msg);
                                sound_clone.read().await.play(EarconType::Error);
                            }
                        }

                        let mut eng = engine_clone.lock().await;
                        eng.on_transcription_finished();
                    });
                } else {
                    bridge.set_idle();
                    let mut eng = engine.lock().await;
                    eng.on_transcription_finished();
                }
            }

            Action::CancelRecording => {
                tracing::info!("Action: CancelRecording");
                bridge.set_idle();
                let mut guard = active_recording.lock().await;
                *guard = None;
                notifications.read().await.error("Recording cancelled");
                sound.read().await.play(EarconType::Cancel);
            }

            Action::None => {}
        }
    }

    Ok(())
}
