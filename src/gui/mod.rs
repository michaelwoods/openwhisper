use std::cell::RefCell;
use std::io::Cursor;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use anyhow::Result;
use hound::{SampleFormat, WavSpec, WavWriter};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::audio::{EarconType, SoundPlayer};
use crate::config::{Config, HudPosition, OutputMode};
use crate::transcribe::{FormattingMode, TranscriptionClient};

slint::include_modules!();

/// Preserved for HUD overlay font fallback in src/hud/app.rs
pub fn configure_fallback_fonts(ctx: &eframe::egui::Context) {
    let mut fonts = eframe::egui::FontDefinitions::default();
    let candidate_paths = [
        // Fedora / RHEL
        "/usr/share/fonts/google-noto-emoji-fonts/NotoEmoji-Regular.ttf",
        "/usr/share/fonts/gdouros-symbola/Symbola.ttf",
        "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
        "/usr/share/fonts/google-noto/NotoSansSymbols-Regular.ttf",
        "/usr/share/fonts/google-noto/NotoSansSymbols2-Regular.ttf",
        // Debian / Ubuntu
        "/usr/share/fonts/truetype/noto/NotoEmoji-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/ancient-scripts/Symbola.ttf",
        // Arch Linux
        "/usr/share/fonts/noto/NotoEmoji-Regular.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
    ];

    let mut loaded_any = false;
    for path in candidate_paths {
        if let Ok(data) = std::fs::read(path) {
            let name = std::path::Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("fallback_font")
                .to_string();

            if fonts.font_data.contains_key(&name) {
                continue;
            }

            fonts.font_data.insert(
                name.clone(),
                std::sync::Arc::new(eframe::egui::FontData::from_owned(data)),
            );

            fonts
                .families
                .entry(eframe::egui::FontFamily::Proportional)
                .or_default()
                .push(name.clone());

            fonts
                .families
                .entry(eframe::egui::FontFamily::Monospace)
                .or_default()
                .push(name);

            loaded_any = true;
        }
    }

    if loaded_any {
        ctx.set_fonts(fonts);
    }
}

/// Populates Slint SettingsWindow properties and vocabulary list from a Config struct.
pub fn config_to_ui(
    config: &Config,
    ui: &SettingsWindow,
    device_items: &[String],
    vocab_model: &Rc<VecModel<SharedString>>,
) {
    // 1. Endpoint Properties
    ui.set_server_url(SharedString::from(&config.server_url));
    ui.set_model_name(SharedString::from(&config.model));
    ui.set_api_key(SharedString::from(config.api_key.as_deref().unwrap_or("")));
    ui.set_language(SharedString::from(config.language.as_deref().unwrap_or("")));
    ui.set_prompt_text(SharedString::from(config.prompt.as_deref().unwrap_or("")));

    // 2. Hotkey & Text Properties
    ui.set_evdev_enabled(config.evdev_hotkey_enabled);
    ui.set_evdev_key(SharedString::from(&config.evdev_hotkey));
    ui.set_ptt_threshold(config.ptt_threshold_ms as i32);
    let output_mode_idx = match config.output_mode {
        OutputMode::Paste => 0,
        OutputMode::ClipboardOnly => 1,
        OutputMode::Type => 2,
    };
    ui.set_output_mode_index(output_mode_idx);
    ui.set_paste_delay(config.paste_delay_ms as i32);
    ui.set_restore_clipboard(config.restore_clipboard);

    // 3. Audio & Recording Properties
    let mut active_device_idx = 0;
    if let Some(ref current) = config.audio_device {
        for (i, dev) in device_items.iter().enumerate() {
            if dev == current || (i > 0 && dev.contains(current)) {
                active_device_idx = i;
                break;
            }
        }
    }
    ui.set_audio_device_index(active_device_idx as i32);
    ui.set_noise_suppression(config.noise_suppression);
    ui.set_save_audio_dir(SharedString::from(
        config.save_audio_dir.as_deref().unwrap_or(""),
    ));

    // 4. Feedback & HUD Properties
    ui.set_sound_enabled(config.sound_feedback);
    ui.set_sound_vol(config.sound_volume);
    ui.set_notifications_enabled(config.show_notifications);
    ui.set_hud_enabled(config.hud_enabled);
    let hud_pos_idx = match config.hud_position {
        HudPosition::BottomCenter => 0,
        HudPosition::TopCenter => 1,
        HudPosition::BottomRight => 2,
        HudPosition::TopRight => 3,
    };
    ui.set_hud_position_index(hud_pos_idx);

    // 5. VAD & Formatting Properties
    ui.set_vad_enabled(config.vad_enabled);
    ui.set_vad_silence_timeout(config.vad_silence_timeout_ms as i32);
    ui.set_vad_threshold(config.vad_energy_threshold);
    let formatting_idx = match config.formatting_mode {
        FormattingMode::Standard => 0,
        FormattingMode::SnakeCase => 1,
        FormattingMode::CamelCase => 2,
        FormattingMode::KebabCase => 3,
        FormattingMode::Raw => 4,
    };
    ui.set_formatting_mode_index(formatting_idx);
    ui.set_trailing_space(config.trailing_space);

    // 6. Vocabulary
    while vocab_model.row_count() > 0 {
        vocab_model.remove(0);
    }
    for word in &config.vocabulary {
        vocab_model.push(SharedString::from(word.as_str()));
    }
}

/// Reads the current Slint UI state and vocabulary model and returns a validated Config struct.
pub fn ui_to_config(
    ui: &SettingsWindow,
    vocab_model: &Rc<VecModel<SharedString>>,
    device_items: &[String],
    base_config: &Config,
) -> Config {
    let mut cfg = base_config.clone();

    cfg.server_url = ui.get_server_url().trim().to_string();
    cfg.model = ui.get_model_name().trim().to_string();

    let api_key_str = ui.get_api_key().trim().to_string();
    cfg.api_key = if api_key_str.is_empty() {
        None
    } else {
        Some(api_key_str)
    };

    let lang_str = ui.get_language().trim().to_string();
    cfg.language = if lang_str.is_empty() {
        None
    } else {
        Some(lang_str)
    };

    let prompt_str = ui.get_prompt_text().trim().to_string();
    cfg.prompt = if prompt_str.is_empty() {
        None
    } else {
        Some(prompt_str)
    };

    let dev_idx = ui.get_audio_device_index() as usize;
    cfg.audio_device = if dev_idx > 0 && dev_idx < device_items.len() {
        Some(device_items[dev_idx].clone())
    } else {
        None
    };
    cfg.noise_suppression = ui.get_noise_suppression();

    let save_dir = ui.get_save_audio_dir().trim().to_string();
    cfg.save_audio_dir = if save_dir.is_empty() {
        None
    } else {
        Some(save_dir)
    };

    cfg.evdev_hotkey_enabled = ui.get_evdev_enabled();
    cfg.evdev_hotkey = ui.get_evdev_key().trim().to_string();
    cfg.ptt_threshold_ms = (ui.get_ptt_threshold() as u64).max(50);
    cfg.output_mode = match ui.get_output_mode_index() {
        1 => OutputMode::ClipboardOnly,
        2 => OutputMode::Type,
        _ => OutputMode::Paste,
    };
    cfg.paste_delay_ms = (ui.get_paste_delay() as u64).max(5);
    cfg.restore_clipboard = ui.get_restore_clipboard();

    cfg.sound_feedback = ui.get_sound_enabled();
    cfg.sound_volume = ui.get_sound_vol().clamp(0.0, 1.0);
    cfg.show_notifications = ui.get_notifications_enabled();

    cfg.hud_enabled = ui.get_hud_enabled();
    cfg.hud_position = match ui.get_hud_position_index() {
        1 => HudPosition::TopCenter,
        2 => HudPosition::BottomRight,
        3 => HudPosition::TopRight,
        _ => HudPosition::BottomCenter,
    };

    cfg.vad_enabled = ui.get_vad_enabled();
    cfg.vad_silence_timeout_ms = (ui.get_vad_silence_timeout() as u64).max(100);
    cfg.vad_energy_threshold = ui.get_vad_threshold().max(0.001);

    cfg.formatting_mode = match ui.get_formatting_mode_index() {
        1 => FormattingMode::SnakeCase,
        2 => FormattingMode::CamelCase,
        3 => FormattingMode::KebabCase,
        4 => FormattingMode::Raw,
        _ => FormattingMode::Standard,
    };
    cfg.trailing_space = ui.get_trailing_space();

    let mut words = Vec::new();
    for i in 0..vocab_model.row_count() {
        if let Some(w) = vocab_model.row_data(i) {
            let s = w.trim().to_string();
            if !s.is_empty() && !words.contains(&s) {
                words.push(s);
            }
        }
    }
    cfg.vocabulary = words;

    cfg
}

pub fn run_gui(config: Config) -> Result<()> {
    let ui = SettingsWindow::new()
        .map_err(|e| anyhow::anyhow!("Failed to initialize Slint SettingsWindow: {e}"))?;

    let detected_devices = crate::audio::AudioRecorder::list_input_devices().unwrap_or_default();
    let mut device_items = vec!["(System Default)".to_string()];
    device_items.extend(detected_devices);
    let dev_shared: Vec<SharedString> = device_items
        .iter()
        .map(|s| SharedString::from(s.as_str()))
        .collect();
    ui.set_audio_devices(ModelRc::from(Rc::new(VecModel::from(dev_shared))));

    let vocab_model = Rc::new(VecModel::default());
    ui.set_vocab_words(ModelRc::from(vocab_model.clone()));

    config_to_ui(&config, &ui, &device_items, &vocab_model);

    // Callback: Add Word
    {
        let vocab_model = vocab_model.clone();
        ui.on_add_word(move |word| {
            let trimmed = word.trim();
            if !trimmed.is_empty() {
                let mut exists = false;
                for i in 0..vocab_model.row_count() {
                    if let Some(existing) = vocab_model.row_data(i)
                        && existing.as_str().eq_ignore_ascii_case(trimmed)
                    {
                        exists = true;
                        break;
                    }
                }
                if !exists {
                    vocab_model.push(SharedString::from(trimmed));
                }
            }
        });
    }

    // Callback: Remove Word
    {
        let vocab_model = vocab_model.clone();
        ui.on_remove_word(move |idx| {
            if idx >= 0 && (idx as usize) < vocab_model.row_count() {
                vocab_model.remove(idx as usize);
            }
        });
    }

    // Callback: Test STT Connection
    {
        let ui_weak = ui.as_weak();
        ui.on_test_connection(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            ui.set_test_in_progress(true);
            ui.set_status_text(SharedString::from("Testing connection to endpoint..."));
            ui.set_status_is_error(false);

            let server_url = ui.get_server_url().to_string();
            let model_name = ui.get_model_name().to_string();
            let api_key = {
                let k = ui.get_api_key().to_string();
                if k.trim().is_empty() { None } else { Some(k) }
            };
            let language = {
                let l = ui.get_language().to_string();
                if l.trim().is_empty() { None } else { Some(l) }
            };
            let prompt = {
                let p = ui.get_prompt_text().to_string();
                if p.trim().is_empty() { None } else { Some(p) }
            };

            let ui_weak_bg = ui_weak.clone();
            std::thread::spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = ui_weak_bg.upgrade_in_event_loop(move |ui| {
                            ui.set_test_in_progress(false);
                            ui.set_status_text(SharedString::from(format!("Runtime error: {e}")));
                            ui.set_status_is_error(true);
                        });
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
                            let sample = (0.05
                                * (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 16000.0).sin()
                                * 32767.0) as i16;
                            let _ = writer.write_sample(sample);
                        }
                        let _ = writer.finalize();
                    }
                    let wav_bytes = buf.into_inner();

                    let test_cfg = Config {
                        server_url: server_url.clone(),
                        model: model_name,
                        api_key,
                        language,
                        prompt,
                        ..Default::default()
                    };

                    let client = TranscriptionClient::new(&test_cfg);
                    let start = Instant::now();
                    match client.transcribe(wav_bytes).await {
                        Ok(text) => {
                            let elapsed = start.elapsed().as_secs_f32();
                            let msg = format!(
                                "Success! Connected to {} in {:.2}s. (Sample text: {:?})",
                                server_url, elapsed, text
                            );
                            let _ = ui_weak_bg.upgrade_in_event_loop(move |ui| {
                                ui.set_test_in_progress(false);
                                ui.set_status_text(SharedString::from(msg));
                                ui.set_status_is_error(false);
                                let ui_clr = ui.as_weak();
                                slint::Timer::single_shot(
                                    std::time::Duration::from_secs(6),
                                    move || {
                                        if let Some(ui) = ui_clr.upgrade() {
                                            ui.set_status_text(SharedString::default());
                                        }
                                    },
                                );
                            });
                        }
                        Err(err) => {
                            let msg = format!("Connection to {} failed: {}", server_url, err);
                            let _ = ui_weak_bg.upgrade_in_event_loop(move |ui| {
                                ui.set_test_in_progress(false);
                                ui.set_status_text(SharedString::from(msg));
                                ui.set_status_is_error(true);
                                let ui_clr = ui.as_weak();
                                slint::Timer::single_shot(
                                    std::time::Duration::from_secs(8),
                                    move || {
                                        if let Some(ui) = ui_clr.upgrade() {
                                            ui.set_status_text(SharedString::default());
                                        }
                                    },
                                );
                            });
                        }
                    }
                });
            });
        });
    }

    // Callback: Test Earcon Sound
    {
        let ui_weak = ui.as_weak();
        ui.on_test_sound(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let vol = ui.get_sound_vol();
            let player = SoundPlayer::new(true, vol);
            player.play(EarconType::RecordingStarted);
        });
    }

    // Callback: Preview HUD Overlay
    let preview_child: Rc<RefCell<Option<std::process::Child>>> = Rc::new(RefCell::new(None));
    {
        let socket_path = config.socket_path;
        let child_handle = preview_child.clone();
        ui.on_preview_hud(move || {
            // 1. If daemon is running, trigger seamless preview on daemon's existing HUD
            let mut sent_ipc = false;
            if let Ok(res) = crate::hotkey::ipc::send_ipc_command_sync(
                &socket_path,
                crate::hotkey::ipc::IpcCommand::PreviewHud,
            ) && res.status == "ok"
            {
                sent_ipc = true;
            }

            // 2. If daemon not running, launch tracked 1-shot preview process
            if !sent_ipc {
                let mut lock = child_handle.borrow_mut();
                if let Some(mut child) = lock.take() {
                    let _ = child.kill();
                }
                let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("openwhisper"));
                if let Ok(child) = std::process::Command::new(exe)
                    .args(["hud-demo", "--once"])
                    .spawn()
                {
                    *lock = Some(child);
                }
            }
        });
    }

    // Callback: Detect Hardware Hotkey
    {
        let ui_weak = ui.as_weak();
        ui.on_detect_hotkey(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            ui.set_detecting_hotkey(true);
            ui.set_status_text(SharedString::from(
                "Listening for hardware keypress (10s timeout)...",
            ));
            ui.set_status_is_error(false);

            let ui_bg = ui_weak.clone();
            std::thread::spawn(move || {
                let key_opt = crate::hotkey::sniff_single_key(std::time::Duration::from_secs(10));
                let _ = ui_bg.upgrade_in_event_loop(move |ui| {
                    ui.set_detecting_hotkey(false);
                    if let Some(key) = key_opt {
                        ui.set_evdev_key(SharedString::from(&key));
                        ui.set_status_text(SharedString::from(format!(
                            "Detected hardware key: {}",
                            key
                        )));
                        ui.set_status_is_error(false);
                    } else {
                        ui.set_status_text(SharedString::from("Key detection timed out."));
                        ui.set_status_is_error(true);
                    }
                    let ui_clr = ui.as_weak();
                    slint::Timer::single_shot(std::time::Duration::from_secs(6), move || {
                        if let Some(ui) = ui_clr.upgrade() {
                            ui.set_status_text(SharedString::default());
                        }
                    });
                });
            });
        });
    }

    // Callback: Test Microphone VU Level Meter
    {
        let ui_weak = ui.as_weak();
        let device_items = device_items.clone();
        ui.on_test_mic(move || {
            let Some(ui) = ui_weak.upgrade() else { return; };
            let dev_idx = ui.get_audio_device_index() as usize;
            let device_name = if dev_idx > 0 && dev_idx < device_items.len() {
                Some(device_items[dev_idx].clone())
            } else {
                None
            };

            ui.set_testing_mic(true);
            ui.set_mic_clipping(false);
            ui.set_mic_test_level(0.0);
            ui.set_mic_level_text(SharedString::from("Listening... Speak into your microphone"));

            let ui_bg = ui_weak.clone();
            std::thread::spawn(move || {
                let ui_level = ui_bg.clone();
                let res = crate::audio::AudioRecorder::test_input_levels(
                    device_name,
                    std::time::Duration::from_millis(3500),
                    None,
                    move |level, clipping| {
                        let ui_cb = ui_level.clone();
                        let _ = ui_cb.upgrade_in_event_loop(move |ui| {
                            ui.set_mic_test_level(level);
                            ui.set_mic_clipping(clipping);
                            let percent = (level * 100.0).round() as u32;
                            if clipping {
                                ui.set_mic_level_text(SharedString::from(format!(
                                    "Level: {}% — ⚠️ Clipping! Lower microphone input gain",
                                    percent
                                )));
                            } else if level > 0.65 {
                                ui.set_mic_level_text(SharedString::from(format!(
                                    "Level: {}% (Strong Signal)",
                                    percent
                                )));
                            } else if level > 0.15 {
                                ui.set_mic_level_text(SharedString::from(format!(
                                    "Level: {}% (Good Signal)",
                                    percent
                                )));
                            } else {
                                ui.set_mic_level_text(SharedString::from(format!(
                                    "Level: {}% (Quiet / Background)",
                                    percent
                                )));
                            }
                        });
                    },
                );

                let _ = ui_bg.upgrade_in_event_loop(move |ui| {
                    ui.set_testing_mic(false);
                    ui.set_mic_test_level(0.0);
                    ui.set_mic_clipping(false);
                    match res {
                        Ok(peak) => {
                            let peak_percent = (peak * 100.0).round() as u32;
                            if peak >= 0.80 {
                                ui.set_mic_level_text(SharedString::from(format!(
                                    "Test complete — Peak level: {}% (⚠️ Caution: Audio clipped)",
                                    peak_percent
                                )));
                            } else if peak >= 0.20 {
                                ui.set_mic_level_text(SharedString::from(format!(
                                    "Test complete — Peak level: {}% (Optimal speech level)",
                                    peak_percent
                                )));
                            } else {
                                ui.set_mic_level_text(SharedString::from(format!(
                                    "Test complete — Peak level: {}% (Very quiet — speak closer to mic)",
                                    peak_percent
                                )));
                            }
                        }
                        Err(err) => {
                            ui.set_mic_level_text(SharedString::from(format!(
                                "Failed to access microphone: {err}"
                            )));
                            ui.set_mic_clipping(true);
                        }
                    }
                });
            });
        });
    }

    // Callback: Save & Apply
    {
        let ui_weak = ui.as_weak();
        let vocab_model = vocab_model.clone();
        let device_items = device_items.clone();
        ui.on_save_and_apply(move || {
            let Some(ui) = ui_weak.upgrade() else { return; };

            let base_cfg = Config::load().unwrap_or_default();
            let cfg = ui_to_config(&ui, &vocab_model, &device_items, &base_cfg);

            match cfg.save() {
                Ok(_) => {
                    let socket_path = cfg.socket_path.clone();
                    let mut reloaded = false;
                    if let Ok(res) = crate::hotkey::ipc::send_ipc_command_sync(
                        &socket_path,
                        crate::hotkey::ipc::IpcCommand::ReloadConfig,
                    )
                        && res.status == "ok" {
                            reloaded = true;
                        }

                    if reloaded {
                        ui.set_status_text(SharedString::from(
                            "Settings saved to config.toml and running daemon reloaded live via IPC!",
                        ));
                        ui.set_status_is_error(false);
                    } else {
                        ui.set_status_text(SharedString::from(
                            "Settings saved to config.toml. (Daemon not running; changes will apply on next startup)",
                        ));
                        ui.set_status_is_error(false);
                    }

                    #[cfg(target_os = "linux")]
                    crate::setup::sync_kwin_hud_position(cfg.hud_position);

                    let ui_clr = ui.as_weak();
                    slint::Timer::single_shot(std::time::Duration::from_secs(6), move || {
                        if let Some(ui) = ui_clr.upgrade() {
                            ui.set_status_text(SharedString::default());
                        }
                    });
                }
                Err(e) => {
                    ui.set_status_text(SharedString::from(format!("Failed to save config: {e}")));
                    ui.set_status_is_error(true);
                    let ui_clr = ui.as_weak();
                    slint::Timer::single_shot(std::time::Duration::from_secs(8), move || {
                        if let Some(ui) = ui_clr.upgrade() {
                            ui.set_status_text(SharedString::default());
                        }
                    });
                }
            }
        });
    }

    // Callback: Reset to Defaults
    {
        let ui_weak = ui.as_weak();
        let vocab_model = vocab_model;
        ui.on_reset_defaults(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let def = Config::default();

            config_to_ui(&def, &ui, &device_items, &vocab_model);

            ui.set_status_text(SharedString::from(
                "Reset to default configuration values. Click 'Save & Apply' to persist.",
            ));
            ui.set_status_is_error(false);

            let ui_clr = ui.as_weak();
            slint::Timer::single_shot(std::time::Duration::from_secs(6), move || {
                if let Some(ui) = ui_clr.upgrade() {
                    ui.set_status_text(SharedString::default());
                }
            });
        });
    }

    let res = ui
        .run()
        .map_err(|e| anyhow::anyhow!("Slint GUI error: {e}"));

    if let Some(mut child) = preview_child.borrow_mut().take() {
        let _ = child.kill();
    }

    res?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    #[test]
    fn test_vocabulary_vec_model_operations() {
        let initial_words = vec![
            SharedString::from("Kubernetes"),
            SharedString::from("OpenVINO"),
        ];
        let vocab_model = Rc::new(VecModel::from(initial_words));
        assert_eq!(vocab_model.row_count(), 2);

        // Test push
        vocab_model.push(SharedString::from("Wayland"));
        assert_eq!(vocab_model.row_count(), 3);
        assert_eq!(vocab_model.row_data(2).unwrap().as_str(), "Wayland");

        // Test remove
        vocab_model.remove(1); // removes OpenVINO
        assert_eq!(vocab_model.row_count(), 2);
        assert_eq!(vocab_model.row_data(0).unwrap().as_str(), "Kubernetes");
        assert_eq!(vocab_model.row_data(1).unwrap().as_str(), "Wayland");
    }

    #[test]
    fn test_settings_window_comprehensive_suite() {
        let ui = SettingsWindow::new().expect("Failed to create SettingsWindow");

        // --- 1. Direct Property Setters and Getters ---
        ui.set_server_url(SharedString::from(
            "http://custom-host:8000/v1/audio/transcriptions",
        ));
        assert_eq!(
            ui.get_server_url().as_str(),
            "http://custom-host:8000/v1/audio/transcriptions"
        );

        ui.set_model_name(SharedString::from("whisper-large-v3"));
        assert_eq!(ui.get_model_name().as_str(), "whisper-large-v3");

        ui.set_evdev_enabled(true);
        assert!(ui.get_evdev_enabled());
        ui.set_evdev_key(SharedString::from("KEY_RIGHTALT"));
        assert_eq!(ui.get_evdev_key().as_str(), "KEY_RIGHTALT");

        ui.set_ptt_threshold(450);
        assert_eq!(ui.get_ptt_threshold(), 450);

        ui.set_output_mode_index(1);
        assert_eq!(ui.get_output_mode_index(), 1);

        ui.set_restore_clipboard(true);
        assert!(ui.get_restore_clipboard());

        ui.set_noise_suppression(true);
        assert!(ui.get_noise_suppression());

        ui.set_save_audio_dir(SharedString::from("/tmp/recordings"));
        assert_eq!(ui.get_save_audio_dir().as_str(), "/tmp/recordings");

        ui.set_sound_enabled(true);
        ui.set_sound_vol(0.85);
        assert!(ui.get_sound_enabled());
        assert!((ui.get_sound_vol() - 0.85).abs() < 1e-4);

        ui.set_hud_enabled(true);
        ui.set_hud_position_index(2);
        assert!(ui.get_hud_enabled());
        assert_eq!(ui.get_hud_position_index(), 2);

        ui.set_vad_enabled(true);
        ui.set_vad_silence_timeout(2500);
        assert!(ui.get_vad_enabled());
        assert_eq!(ui.get_vad_silence_timeout(), 2500);

        ui.set_formatting_mode_index(3);
        assert_eq!(ui.get_formatting_mode_index(), 3);
        ui.set_trailing_space(false);
        assert!(!ui.get_trailing_space());

        // Test Microphone VU Level properties
        ui.set_mic_test_level(0.65);
        assert!((ui.get_mic_test_level() - 0.65).abs() < 1e-4);
        ui.set_testing_mic(true);
        assert!(ui.get_testing_mic());
        ui.set_mic_clipping(true);
        assert!(ui.get_mic_clipping());
        ui.set_mic_level_text(SharedString::from("Level: 65% (Strong Signal)"));
        assert_eq!(
            ui.get_mic_level_text().as_str(),
            "Level: 65% (Strong Signal)"
        );

        // --- 2. Full Roundtrip: Config -> UI -> Config ---
        let vocab_model = Rc::new(VecModel::default());
        let device_items = vec![
            "(System Default)".to_string(),
            "USB Audio Interface".to_string(),
            "Builtin Microphone".to_string(),
        ];

        let mut original = Config::default();
        original.server_url = "http://frigg:8000/v1/audio/transcriptions".to_string();
        original.model = "whisper-medium".to_string();
        original.api_key = Some("test-api-token".to_string());
        original.language = Some("es".to_string());
        original.prompt = Some("Biasing prompt".to_string());
        original.audio_device = Some("USB Audio Interface".to_string());
        original.noise_suppression = true;
        original.save_audio_dir = Some("/home/mike/TTS_Recordings".to_string());
        original.evdev_hotkey_enabled = true;
        original.evdev_hotkey = "KEY_F12".to_string();
        original.ptt_threshold_ms = 400;
        original.output_mode = OutputMode::Type;
        original.paste_delay_ms = 65;
        original.restore_clipboard = true;
        original.sound_feedback = true;
        original.sound_volume = 0.70;
        original.show_notifications = true;
        original.hud_enabled = true;
        original.hud_position = HudPosition::TopRight;
        original.vad_enabled = true;
        original.vad_silence_timeout_ms = 1800;
        original.vad_energy_threshold = 0.045;
        original.formatting_mode = FormattingMode::CamelCase;
        original.trailing_space = false;
        original.vocabulary = vec![
            "Slint".to_string(),
            "Wayland".to_string(),
            "Rust".to_string(),
        ];

        config_to_ui(&original, &ui, &device_items, &vocab_model);
        let extracted = ui_to_config(&ui, &vocab_model, &device_items, &Config::default());

        assert_eq!(extracted.server_url, original.server_url);
        assert_eq!(extracted.model, original.model);
        assert_eq!(extracted.api_key, original.api_key);
        assert_eq!(extracted.language, original.language);
        assert_eq!(extracted.prompt, original.prompt);
        assert_eq!(extracted.audio_device, original.audio_device);
        assert_eq!(extracted.noise_suppression, original.noise_suppression);
        assert_eq!(extracted.save_audio_dir, original.save_audio_dir);
        assert_eq!(
            extracted.evdev_hotkey_enabled,
            original.evdev_hotkey_enabled
        );
        assert_eq!(extracted.evdev_hotkey, original.evdev_hotkey);
        assert_eq!(extracted.ptt_threshold_ms, original.ptt_threshold_ms);
        assert_eq!(extracted.output_mode, original.output_mode);
        assert_eq!(extracted.paste_delay_ms, original.paste_delay_ms);
        assert_eq!(extracted.restore_clipboard, original.restore_clipboard);
        assert_eq!(extracted.sound_feedback, original.sound_feedback);
        assert!((extracted.sound_volume - original.sound_volume).abs() < 1e-4);
        assert_eq!(extracted.show_notifications, original.show_notifications);
        assert_eq!(extracted.hud_enabled, original.hud_enabled);
        assert_eq!(extracted.hud_position, original.hud_position);
        assert_eq!(extracted.vad_enabled, original.vad_enabled);
        assert_eq!(
            extracted.vad_silence_timeout_ms,
            original.vad_silence_timeout_ms
        );
        assert!((extracted.vad_energy_threshold - original.vad_energy_threshold).abs() < 1e-4);
        assert_eq!(extracted.formatting_mode, original.formatting_mode);
        assert_eq!(extracted.trailing_space, original.trailing_space);
        assert_eq!(extracted.vocabulary, original.vocabulary);

        // --- 3. Sanitization, Trimming, and Boundary Clamping ---
        ui.set_server_url(SharedString::from("  http://localhost:8000/v1  "));
        ui.set_model_name(SharedString::from("  whisper-1  "));
        ui.set_api_key(SharedString::from("   "));
        ui.set_language(SharedString::from("   "));
        ui.set_prompt_text(SharedString::from("   "));
        ui.set_save_audio_dir(SharedString::from("   "));
        ui.set_ptt_threshold(10); // Below min 50ms
        ui.set_paste_delay(1); // Below min 5ms
        ui.set_sound_vol(1.8); // Above max 1.0
        ui.set_vad_silence_timeout(20); // Below min 100ms
        ui.set_vad_threshold(0.00001); // Below min 0.001

        while vocab_model.row_count() > 0 {
            vocab_model.remove(0);
        }
        vocab_model.push(SharedString::from("   "));
        vocab_model.push(SharedString::from("Fedora"));
        vocab_model.push(SharedString::from("Fedora")); // Duplicate
        vocab_model.push(SharedString::from("  Plasma  "));

        let cfg = ui_to_config(&ui, &vocab_model, &device_items, &Config::default());

        assert_eq!(cfg.server_url, "http://localhost:8000/v1");
        assert_eq!(cfg.model, "whisper-1");
        assert_eq!(cfg.api_key, None);
        assert_eq!(cfg.language, None);
        assert_eq!(cfg.prompt, None);
        assert_eq!(cfg.save_audio_dir, None);
        assert_eq!(cfg.ptt_threshold_ms, 50); // Clamped
        assert_eq!(cfg.paste_delay_ms, 5); // Clamped
        assert!((cfg.sound_volume - 1.0).abs() < 1e-4); // Clamped
        assert_eq!(cfg.vad_silence_timeout_ms, 100); // Clamped
        assert!((cfg.vad_energy_threshold - 0.001).abs() < 1e-4); // Clamped
        assert_eq!(
            cfg.vocabulary,
            vec!["Fedora".to_string(), "Plasma".to_string()]
        );

        // --- 4. Audio Device Selection Matching ---
        let mut dev_cfg = Config::default();

        // None selects System Default (index 0)
        dev_cfg.audio_device = None;
        config_to_ui(&dev_cfg, &ui, &device_items, &vocab_model);
        assert_eq!(ui.get_audio_device_index(), 0);

        // Exact match selects index 1
        dev_cfg.audio_device = Some("USB Audio Interface".to_string());
        config_to_ui(&dev_cfg, &ui, &device_items, &vocab_model);
        assert_eq!(ui.get_audio_device_index(), 1);

        // Substring match selects index 2
        dev_cfg.audio_device = Some("Builtin".to_string());
        config_to_ui(&dev_cfg, &ui, &device_items, &vocab_model);
        assert_eq!(ui.get_audio_device_index(), 2);

        // Missing device defaults to index 0
        dev_cfg.audio_device = Some("Nonexistent Bluetooth Mic".to_string());
        config_to_ui(&dev_cfg, &ui, &device_items, &vocab_model);
        assert_eq!(ui.get_audio_device_index(), 0);
    }
}
