use anyhow::Result;
use std::time::Duration;
use tracing::{info, warn};

use crate::config::Config;
use crate::hotkey::ipc::{DaemonState, EventStream, IpcEvent};
use crate::hud;
use crate::tray::{self, TrayState};

/// Runs the OpenWhisper Graphical UI service (System Tray icon & Floating HUD overlay).
/// Connects to the headless daemon's IPC event stream and synchronizes desktop state.
pub async fn run_ui_service(config: Config) -> Result<()> {
    info!("Starting OpenWhisper UI service (System Tray & Floating HUD)...");

    // 1. Initialize Floating HUD Overlay (binds to WAYLAND_DISPLAY / DISPLAY if present)
    let hud_ctrl = hud::start_hud_service(config.hud_enabled, config.hud_position);

    // 2. Initialize StatusNotifierItem System Tray (auto-retrying background registration)
    let (tray_ctrl, _) =
        tray::start_tray_service(config.socket_path.clone(), config.server_url.clone()).await;

    println!("OpenWhisper UI service active.");
    println!("Connecting to background daemon at: {}", config.socket_path);

    let mut first_connect = true;

    loop {
        match EventStream::connect(&config.socket_path).await {
            Ok(mut stream) => {
                if first_connect {
                    info!(
                        "Connected to OpenWhisper daemon event stream on {}",
                        config.socket_path
                    );
                    first_connect = false;
                } else {
                    info!(
                        "Reconnected to OpenWhisper daemon on {}",
                        config.socket_path
                    );
                }

                tray_ctrl.set_state(TrayState::Idle);

                while let Ok(Some(event)) = stream.next_event().await {
                    match event {
                        IpcEvent::StateChanged { state } => match state {
                            DaemonState::Idle => {
                                tray_ctrl.set_state(TrayState::Idle);
                                // Completed and Error states have their own timed fadeout lifecycle;
                                // only reset to Idle if we were actively Recording or Transcribing.
                                let should_idle = {
                                    let lock = hud_ctrl.model();
                                    let model = lock.read().unwrap();
                                    matches!(
                                        model.state,
                                        crate::hud::HudState::Recording { .. }
                                            | crate::hud::HudState::Transcribing { .. }
                                    )
                                };
                                if should_idle {
                                    hud_ctrl.set_idle();
                                }
                            }
                            DaemonState::Recording { device } => {
                                tray_ctrl.set_state(TrayState::Recording);
                                if let Some(d) = device {
                                    tray_ctrl.set_active_device(Some(d));
                                }
                                hud_ctrl.set_recording();
                            }
                            DaemonState::Transcribing => {
                                tray_ctrl.set_state(TrayState::Transcribing);
                                hud_ctrl.set_transcribing();
                            }
                            DaemonState::Degraded { reason } => {
                                tray_ctrl.set_state(TrayState::Degraded);
                                warn!("Daemon reported degraded state: {reason}");
                            }
                            DaemonState::Error { message } => {
                                tray_ctrl.set_state(TrayState::Error);
                                hud_ctrl.set_error(&message);
                            }
                        },
                        IpcEvent::AudioLevel { rms } => {
                            hud_ctrl.update_audio_level(rms);
                        }
                        IpcEvent::TranscriptionCompleted {
                            text,
                            duration_secs,
                        } => {
                            hud_ctrl.set_completed(&text);
                            tray_ctrl.add_history(text);
                            tray_ctrl.set_diagnostics(Some(duration_secs), None);
                            tray_ctrl.set_state(TrayState::Idle);
                        }
                        IpcEvent::Diagnostics {
                            server_url,
                            active_device,
                            last_latency,
                            last_error,
                        } => {
                            tray_ctrl.set_server_url(server_url);
                            tray_ctrl.set_active_device(active_device);
                            tray_ctrl.set_diagnostics(last_latency, last_error);
                        }
                    }
                }

                warn!("Connection to OpenWhisper daemon lost. Reconnecting in background...");
                tray_ctrl.set_state(TrayState::Degraded);
            }
            Err(err) => {
                tracing::debug!(
                    "Waiting for OpenWhisper daemon at {}: {err}",
                    config.socket_path
                );
                tray_ctrl.set_state(TrayState::Degraded);
            }
        }

        tokio::time::sleep(Duration::from_millis(1500)).await;
    }
}
