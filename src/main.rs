mod audio;
mod cli;
mod config;
mod hotkey;
mod notification;
mod output;
mod transcribe;

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
use tokio::sync::{mpsc, Mutex};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use audio::AudioRecorder;
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

    println!("🎙️  Recording audio... Speak into your microphone.");
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
    let wav_bytes = active_rec.stop()?;
    let start_t = Instant::now();
    let text = client.transcribe(wav_bytes).await?;
    let elapsed = start_t.elapsed().as_secs_f32();

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

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<IpcCommand>(32);
    let notifications = Arc::new(NotificationManager::new(config.show_notifications));
    let client = Arc::new(TranscriptionClient::new(&config));
    let output_mgr = Arc::new(Mutex::new(OutputManager::new(&config)));
    let engine = Arc::new(Mutex::new(HotkeyEngine::new(config.ptt_threshold_ms)));
    let active_recording: Arc<Mutex<Option<audio::ActiveRecording>>> = Arc::new(Mutex::new(None));
    let recorder = Arc::new(AudioRecorder::new(config.audio_device.clone()));

    // Spawn IPC Unix Socket Server
    let ipc_server = IpcServer::new(config.socket_path.clone(), cmd_tx.clone());
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

    tracing::info!("OpenWhisper daemon ready! Waiting for hotkey / IPC events...");
    println!("OpenWhisper daemon running in background.");
    println!("Trigger via:");
    println!("  • Global shortcut (if configured in KDE/portal)");
    println!("  • CLI: `openwhisper toggle`");
    println!("  • CLI: `openwhisper ptt-down` (press) & `openwhisper ptt-up` (release)");

    while let Some(cmd) = cmd_rx.recv().await {
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
            }
        };

        match action {
            Action::StartRecording => {
                tracing::info!("Action: StartRecording");
                notifications.recording_started();
                match recorder.start_recording() {
                    Ok(rec) => {
                        let mut guard = active_recording.lock().await;
                        *guard = Some(rec);
                    }
                    Err(err) => {
                        tracing::error!("Failed to start recording: {err}");
                        notifications.error(&format!("Mic error: {err}"));
                        let mut eng = engine.lock().await;
                        eng.on_cancel();
                    }
                }
            }

            Action::StopAndTranscribe => {
                tracing::info!("Action: StopAndTranscribe");
                notifications.transcribing();

                let rec_opt = {
                    let mut guard = active_recording.lock().await;
                    guard.take()
                };

                if let Some(rec) = rec_opt {
                    let client_clone = Arc::clone(&client);
                    let output_mgr_clone = Arc::clone(&output_mgr);
                    let notif_clone = Arc::clone(&notifications);
                    let engine_clone = Arc::clone(&engine);

                    let wav_res = rec.stop();
                    tokio::spawn(async move {
                        match wav_res {
                            Ok(wav_bytes) => {
                                let start = Instant::now();
                                match client_clone.transcribe(wav_bytes).await {
                                    Ok(text) => {
                                        let duration = start.elapsed().as_secs_f32();
                                        tracing::info!("Transcribed in {:.2}s: {:?}", duration, text);
                                        notif_clone.transcribed(&text);

                                        let mut out = output_mgr_clone.lock().await;
                                        if let Err(err) = out.output_text(&text) {
                                            tracing::error!("Output injection error: {err}");
                                            notif_clone.error(&format!("Output error: {err}"));
                                        }
                                    }
                                    Err(err) => {
                                        tracing::error!("Transcription error: {err}");
                                        notif_clone.error(&format!("STT failed: {err}"));
                                    }
                                }
                            }
                            Err(err) => {
                                tracing::error!("Audio stop/encoding error: {err}");
                                notif_clone.error(&format!("Audio encoding error: {err}"));
                            }
                        }

                        let mut eng = engine_clone.lock().await;
                        eng.on_transcription_finished();
                    });
                } else {
                    let mut eng = engine.lock().await;
                    eng.on_transcription_finished();
                }
            }

            Action::CancelRecording => {
                tracing::info!("Action: CancelRecording");
                let mut guard = active_recording.lock().await;
                *guard = None;
                notifications.error("Recording cancelled");
            }

            Action::None => {}
        }
    }

    Ok(())
}
