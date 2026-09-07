use std::io::Cursor;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Instant;
use eframe::egui;
use hound::{SampleFormat, WavSpec, WavWriter};

use crate::audio::{EarconType, SoundPlayer};
use crate::config::{Config, OutputMode};
use crate::transcribe::{FormattingMode, TranscriptionClient};

pub struct ConfigApp {
    config: Config,
    new_vocab_word: String,
    status_message: Option<(String, bool)>, // (message, is_error)
    test_receiver: Option<Receiver<Result<String, String>>>,
    test_pending: bool,
}

impl ConfigApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        Self {
            config,
            new_vocab_word: String::new(),
            status_message: None,
            test_receiver: None,
            test_pending: false,
        }
    }

    fn test_stt_connection(&mut self) {
        let (tx, rx): (Sender<Result<String, String>>, Receiver<Result<String, String>>) = channel();
        self.test_receiver = Some(rx);
        self.test_pending = true;

        let server_url = self.config.server_url.clone();
        let test_cfg = self.config.clone();


        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(Err(format!("Runtime error: {e}")));
                    return;
                }
            };

            rt.block_on(async move {
                // Generate 0.5s audio wav buffer in-memory
                let spec = WavSpec {
                    channels: 1,
                    sample_rate: 16000,
                    bits_per_sample: 16,
                    sample_format: SampleFormat::Int,
                };
                let mut buf = Cursor::new(Vec::new());
                if let Ok(mut writer) = WavWriter::new(&mut buf, spec) {
                    for i in 0..8000 {
                        let sample = (0.05 * (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 16000.0).sin() * 32767.0) as i16;
                        let _ = writer.write_sample(sample);
                    }
                    let _ = writer.finalize();
                }
                let wav_bytes = buf.into_inner();

                let client = TranscriptionClient::new(&test_cfg);
                let start = Instant::now();
                match client.transcribe(wav_bytes).await {
                    Ok(text) => {
                        let elapsed = start.elapsed().as_secs_f32();
                        let _ = tx.send(Ok(format!(
                            "Success! Connected to {} in {:.2}s. (Sample text: {:?})",
                            server_url, elapsed, text
                        )));
                    }
                    Err(err) => {
                        let _ = tx.send(Err(format!("Connection to {} failed: {}", server_url, err)));
                    }
                }
            });
        });
    }

    fn save_and_apply(&mut self) {
        match self.config.save() {
            Ok(_) => {
                // Dynamically reload running daemon via IPC
                let socket_path = self.config.socket_path.clone();
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                let mut reloaded = false;
                if let Ok(rt) = rt {
                    if let Ok(res) = rt.block_on(crate::hotkey::ipc::send_ipc_command(
                        &socket_path,
                        crate::hotkey::ipc::IpcCommand::ReloadConfig,
                    )) {
                        if res.status == "ok" {
                            reloaded = true;
                        }
                    }
                }

                if reloaded {
                    self.status_message = Some((
                        "Settings saved to config.toml and running daemon reloaded live via IPC!".into(),
                        false,
                    ));
                } else {
                    self.status_message = Some((
                        "Settings saved to config.toml. (Daemon not running; changes will apply on next startup)".into(),
                        false,
                    ));
                }
            }
            Err(e) => {
                self.status_message = Some((format!("Failed to save config: {e}"), true));
            }
        }
    }
}

impl eframe::App for ConfigApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Poll async test connection result if any
        if let Some(ref rx) = self.test_receiver {
            if let Ok(res) = rx.try_recv() {
                self.test_pending = false;
                match res {
                    Ok(msg) => self.status_message = Some((msg, false)),
                    Err(err) => self.status_message = Some((err, true)),
                }
                self.test_receiver = None;
            }
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(8.0);

                ui.heading("🎙️  OpenWhisper Configuration");
                ui.label("Manage Speech-to-Text inference, hotkey modes, audio feedback, and system integration.");
                ui.separator();

                // Status Banner
                if let Some((ref msg, is_error)) = self.status_message {
                    let color = if is_error {
                        egui::Color32::from_rgb(239, 68, 68)
                    } else {
                        egui::Color32::from_rgb(34, 197, 94)
                    };
                    egui::Frame::group(ui.style())
                        .fill(egui::Color32::from_rgba_premultiplied(color.r() / 8, color.g() / 8, color.b() / 8, 30))
                        .stroke(egui::Stroke::new(1.0, color))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(if is_error { "⚠️" } else { "✓" }).color(color).strong());
                                ui.label(egui::RichText::new(msg).color(color));
                            });
                        });
                    ui.add_space(4.0);
                }

                // Section 1: Server & Model Endpoint
                ui.collapsing("🌐 Inference Server & Model Endpoint", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Server Endpoint URL:");
                        ui.text_edit_singleline(&mut self.config.server_url);
                    });

                    ui.horizontal(|ui| {
                        if ui.button(if self.test_pending { "⏳ Testing..." } else { "⚡ Test Connection" }).clicked() && !self.test_pending {
                            self.test_stt_connection();
                        }
                        ui.label("OpenVINO Model Server or OpenAI-compatible endpoint");
                    });

                    ui.horizontal(|ui| {
                        ui.label("Model Name:");
                        ui.text_edit_singleline(&mut self.config.model);
                    });

                    ui.horizontal(|ui| {
                        ui.label("Language Code:");
                        let mut lang_str = self.config.language.clone().unwrap_or_default();
                        if ui.text_edit_singleline(&mut lang_str).changed() {
                            self.config.language = if lang_str.trim().is_empty() { None } else { Some(lang_str) };
                        }
                        ui.label("(optional, e.g. en, fr, de)");
                    });

                    ui.horizontal(|ui| {
                        ui.label("Prompt Bias Context:");
                        let mut prompt_str = self.config.prompt.clone().unwrap_or_default();
                        if ui.text_edit_singleline(&mut prompt_str).changed() {
                            self.config.prompt = if prompt_str.trim().is_empty() { None } else { Some(prompt_str) };
                        }
                    });
                });

                ui.add_space(6.0);

                // Section 2: Formatting & Output
                ui.collapsing("✍️ Dictation Formatting & Output Mode", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Formatting Mode:");
                        egui::ComboBox::from_id_salt("formatting_mode")
                            .selected_text(format!("{:?}", self.config.formatting_mode))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.config.formatting_mode, FormattingMode::Standard, "Standard (Default)");
                                ui.selectable_value(&mut self.config.formatting_mode, FormattingMode::SnakeCase, "snake_case");
                                ui.selectable_value(&mut self.config.formatting_mode, FormattingMode::CamelCase, "camelCase");
                                ui.selectable_value(&mut self.config.formatting_mode, FormattingMode::KebabCase, "kebab-case");
                                ui.selectable_value(&mut self.config.formatting_mode, FormattingMode::Raw, "Raw (Whisper verbatim)");
                            });
                    });

                    ui.horizontal(|ui| {
                        ui.label("Output Method:");
                        egui::ComboBox::from_id_salt("output_mode")
                            .selected_text(match self.config.output_mode {
                                OutputMode::Paste => "Paste (Wayland wl-copy + Ctrl+V via uinput)",
                                OutputMode::ClipboardOnly => "Clipboard Only (Copy to clipboard)",
                                OutputMode::Type => "Type (Direct keystrokes)",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.config.output_mode, OutputMode::Paste, "Paste (uinput Virtual Keyboard)");
                                ui.selectable_value(&mut self.config.output_mode, OutputMode::ClipboardOnly, "Clipboard Only");
                                ui.selectable_value(&mut self.config.output_mode, OutputMode::Type, "Type (Direct Input)");
                            });
                    });


                    ui.horizontal(|ui| {
                        ui.label("Paste Delay (ms):");
                        ui.add(egui::Slider::new(&mut self.config.paste_delay_ms, 10..=300).text("ms"));
                    });

                    ui.horizontal(|ui| {
                        ui.label("PTT vs Toggle Hold Threshold (ms):");
                        ui.add(egui::Slider::new(&mut self.config.ptt_threshold_ms, 100..=1000).text("ms"));
                    });
                });

                ui.add_space(6.0);

                // Section 3: Audio Feedback & Notifications
                ui.collapsing("🔊 Audio Feedback & Notifications", |ui| {
                    ui.checkbox(&mut self.config.sound_feedback, "Enable Audio Feedback (Earcons)");

                    if self.config.sound_feedback {
                        ui.horizontal(|ui| {
                            ui.label("Volume:");
                            ui.add(egui::Slider::new(&mut self.config.sound_volume, 0.05..=1.0).text(""));
                            if ui.button("🔊 Test Sound").clicked() {
                                let player = SoundPlayer::new(true, self.config.sound_volume);
                                player.play(EarconType::RecordingStarted);
                            }
                        });
                    }

                    ui.checkbox(&mut self.config.show_notifications, "Desktop Notifications (via Desktop Notification Service)");
                });

                ui.add_space(6.0);

                // Section: Floating Status Overlay (HUD)
                ui.collapsing("🖥️ Floating Status Overlay (HUD)", |ui| {
                    ui.checkbox(&mut self.config.hud_enabled, "Enable Floating HUD Overlay");

                    if self.config.hud_enabled {
                        ui.horizontal(|ui| {
                            ui.label("Screen Position:");
                            egui::ComboBox::from_id_salt("hud_position")
                                .selected_text(match self.config.hud_position {
                                    crate::config::HudPosition::BottomCenter => "Bottom Center (Default)",
                                    crate::config::HudPosition::TopCenter => "Top Center",
                                    crate::config::HudPosition::BottomRight => "Bottom Right",
                                    crate::config::HudPosition::TopRight => "Top Right",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut self.config.hud_position, crate::config::HudPosition::BottomCenter, "Bottom Center");
                                    ui.selectable_value(&mut self.config.hud_position, crate::config::HudPosition::TopCenter, "Top Center");
                                    ui.selectable_value(&mut self.config.hud_position, crate::config::HudPosition::BottomRight, "Bottom Right");
                                    ui.selectable_value(&mut self.config.hud_position, crate::config::HudPosition::TopRight, "Top Right");
                                });
                        });
                    }
                });

                ui.add_space(6.0);

                // Section 4: Voice Activity Detection (VAD)
                ui.collapsing("🎙️ Voice Activity Detection (VAD Auto-Stop)", |ui| {
                    ui.checkbox(&mut self.config.vad_enabled, "Enable Silence Auto-Stop");

                    if self.config.vad_enabled {
                        ui.horizontal(|ui| {
                            ui.label("Trailing Silence Timeout:");
                            ui.add(egui::Slider::new(&mut self.config.vad_silence_timeout_ms, 500..=5000).text("ms"));
                        });

                        ui.horizontal(|ui| {
                            ui.label("Energy Silence Threshold:");
                            ui.add(egui::Slider::new(&mut self.config.vad_energy_threshold, 0.005..=0.080));
                        });
                    }
                });

                ui.add_space(6.0);

                // Section 5: Custom Vocabulary Biasing
                ui.collapsing("📚 Custom Vocabulary & Domain Biasing", |ui| {
                    ui.label("Boost transcription accuracy for technical terms, frameworks, and proper nouns:");

                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.new_vocab_word);
                        if ui.button("➕ Add Word").clicked() && !self.new_vocab_word.trim().is_empty() {
                            let term = self.new_vocab_word.trim().to_string();
                            if !self.config.vocabulary.contains(&term) {
                                self.config.vocabulary.push(term);
                            }
                            self.new_vocab_word.clear();
                        }
                    });

                    let mut to_remove = None;
                    for (i, word) in self.config.vocabulary.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(format!("• {}", word));
                            if ui.button("✕").clicked() {
                                to_remove = Some(i);
                            }
                        });
                    }
                    if let Some(idx) = to_remove {
                        self.config.vocabulary.remove(idx);
                    }
                });

                ui.separator();
                ui.add_space(8.0);

                // Footer Actions
                ui.horizontal(|ui| {
                    if ui.button(egui::RichText::new("💾 Save & Apply").strong().size(14.0)).clicked() {
                        self.save_and_apply();
                    }

                    if ui.button("🔄 Reset to Defaults").clicked() {
                        self.config = Config::default();
                        self.status_message = Some(("Reset to default configuration values. Click 'Save & Apply' to persist.".into(), false));
                    }
                });
                ui.add_space(8.0);
            });
    }
}

