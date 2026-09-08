use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use crate::audio::{SoundPlayer, VadConfig};
use crate::transcribe::FormattingMode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_server_url")]
    pub server_url: String,

    #[serde(default = "default_model")]
    pub model: String,

    #[serde(default)]
    pub language: Option<String>,

    #[serde(default)]
    pub prompt: Option<String>,

    #[serde(default)]
    pub temperature: Option<f32>,

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default = "default_ptt_threshold_ms")]
    pub ptt_threshold_ms: u64,

    #[serde(default = "default_output_mode")]
    pub output_mode: OutputMode,

    #[serde(default = "default_paste_delay_ms")]
    pub paste_delay_ms: u64,

    #[serde(
        default = "default_true",
        alias = "de_notifications",
        alias = "enable_notifications",
        alias = "notifications",
        alias = "notifications_enabled"
    )]
    pub show_notifications: bool,

    #[serde(default = "default_true")]
    pub sound_feedback: bool,

    #[serde(default = "default_sound_volume")]
    pub sound_volume: f32,

    #[serde(default = "default_vocabulary")]
    pub vocabulary: Vec<String>,

    #[serde(default)]
    pub formatting_mode: FormattingMode,

    #[serde(default = "default_true")]
    pub trailing_space: bool,

    #[serde(default = "default_false")]
    pub vad_enabled: bool,

    #[serde(default = "default_vad_silence_timeout_ms")]
    pub vad_silence_timeout_ms: u64,

    #[serde(default = "default_vad_energy_threshold")]
    pub vad_energy_threshold: f32,

    #[serde(default)]
    pub audio_device: Option<String>,

    #[serde(default = "default_socket_path")]
    pub socket_path: String,

    #[serde(
        default = "default_true",
        alias = "enable_hud",
        alias = "hud",
        alias = "floating_hud"
    )]
    pub hud_enabled: bool,

    #[serde(default = "default_hud_position")]
    pub hud_position: HudPosition,

    #[serde(
        default = "default_true",
        alias = "enable_evdev",
        alias = "evdev_enabled",
        alias = "hardware_hotkey_enabled"
    )]
    pub evdev_hotkey_enabled: bool,

    #[serde(
        default = "default_evdev_hotkey",
        alias = "hotkey",
        alias = "evdev_key"
    )]
    pub evdev_hotkey: String,

    #[serde(
        default = "default_false",
        alias = "restore_prev_clipboard",
        alias = "clipboard_restore"
    )]
    pub restore_clipboard: bool,

    #[serde(
        default = "default_false",
        alias = "rnnoise",
        alias = "enable_noise_suppression",
        alias = "denoise"
    )]
    pub noise_suppression: bool,

    #[serde(default, alias = "recordings_dir", alias = "save_audio_path")]
    pub save_audio_dir: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HudPosition {
    BottomCenter,
    TopCenter,
    BottomRight,
    TopRight,
}

fn default_hud_position() -> HudPosition {
    HudPosition::BottomCenter
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    /// Copies text to clipboard AND simulates Ctrl+V to paste immediately into the active cursor
    Paste,
    /// Only copies text to clipboard without simulating keypresses
    ClipboardOnly,
    /// Simulates direct keystrokes character-by-character
    Type,
}

fn default_server_url() -> String {
    "http://localhost:8000/v1/audio/transcriptions".to_string()
}

fn default_model() -> String {
    "whisper".to_string()
}

fn default_ptt_threshold_ms() -> u64 {
    350
}

fn default_paste_delay_ms() -> u64 {
    60
}

fn default_output_mode() -> OutputMode {
    OutputMode::Paste
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_sound_volume() -> f32 {
    0.5
}

fn default_vad_silence_timeout_ms() -> u64 {
    1800
}

fn default_vad_energy_threshold() -> f32 {
    0.015
}

fn default_vocabulary() -> Vec<String> {
    vec![
        "Rust".to_string(),
        "Wayland".to_string(),
        "KDE".to_string(),
        "OpenVINO".to_string(),
        "cpal".to_string(),
        "tokio".to_string(),
        "uinput".to_string(),
        "Fedora".to_string(),
    ]
}

fn default_socket_path() -> String {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        format!("{}/openwhisper.sock", runtime_dir)
    } else {
        std::env::temp_dir()
            .join("openwhisper.sock")
            .to_string_lossy()
            .to_string()
    }
}

fn default_evdev_hotkey() -> String {
    "KEY_RIGHTALT".to_string()
}

#[cfg(target_os = "linux")]
pub fn parse_evdev_key(name: &str) -> Option<evdev::Key> {
    let trimmed = name.trim();
    if let Ok(code) = trimmed.parse::<u16>() {
        return Some(evdev::Key::new(code));
    }

    let upper = trimmed.to_uppercase();
    let search = if upper.starts_with("KEY_") {
        upper
    } else {
        format!("KEY_{upper}")
    };

    // Scan all valid evdev keycodes
    for code in 0..768u16 {
        let key = evdev::Key::new(code);
        let debug_name = format!("{:?}", key);
        if debug_name == search {
            return Some(key);
        }
    }

    None
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server_url: default_server_url(),
            model: default_model(),
            language: None,
            prompt: None,
            temperature: None,
            api_key: None,
            ptt_threshold_ms: default_ptt_threshold_ms(),
            output_mode: default_output_mode(),
            paste_delay_ms: default_paste_delay_ms(),
            show_notifications: default_true(),
            sound_feedback: default_true(),
            sound_volume: default_sound_volume(),
            vocabulary: default_vocabulary(),
            formatting_mode: FormattingMode::Standard,
            trailing_space: default_true(),
            vad_enabled: default_false(),
            vad_silence_timeout_ms: default_vad_silence_timeout_ms(),
            vad_energy_threshold: default_vad_energy_threshold(),
            audio_device: None,
            socket_path: default_socket_path(),
            hud_enabled: default_true(),
            hud_position: default_hud_position(),
            evdev_hotkey_enabled: default_true(),
            evdev_hotkey: default_evdev_hotkey(),
            restore_clipboard: false,
            noise_suppression: false,
            save_audio_dir: None,
        }
    }
}

impl Config {
    pub fn config_path() -> Result<PathBuf> {
        if let Some(proj_dirs) = ProjectDirs::from("com", "openwhisper", "openwhisper") {
            let dir = proj_dirs.config_dir();
            Ok(dir.join("config.toml"))
        } else {
            let home = std::env::var("HOME").context("Unable to locate HOME directory")?;
            Ok(PathBuf::from(home).join(".config/openwhisper/config.toml"))
        }
    }

    pub fn load_or_default() -> Self {
        match Self::load() {
            Ok(cfg) => cfg,
            Err(err) => {
                tracing::warn!("Could not load config file: {err}. Using default configuration.");
                Self::default()
            }
        }
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file at {:?}", path))?;
        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse TOML from {:?}", path))?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config dir {:?}", parent))?;
        }
        let content = toml::to_string_pretty(self).context("Failed to serialize config to TOML")?;
        fs::write(&path, content)
            .with_context(|| format!("Failed to write config file to {:?}", path))?;
        Ok(())
    }

    pub fn sound_player(&self) -> SoundPlayer {
        SoundPlayer::new(self.sound_feedback, self.sound_volume)
    }

    pub fn vad_config(&self) -> VadConfig {
        VadConfig {
            enabled: self.vad_enabled,
            silence_timeout: Duration::from_millis(self.vad_silence_timeout_ms),
            energy_threshold: self.vad_energy_threshold,
            min_speech_duration: Duration::from_millis(300),
        }
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert_eq!(
            cfg.server_url,
            "http://localhost:8000/v1/audio/transcriptions"
        );
        assert_eq!(cfg.model, "whisper");
        assert_eq!(cfg.ptt_threshold_ms, 350);
        assert_eq!(cfg.output_mode, OutputMode::Paste);
        assert!(cfg.show_notifications);
        assert!(cfg.sound_feedback);
        assert_eq!(cfg.sound_volume, 0.5);
        assert!(!cfg.vocabulary.is_empty());
        assert_eq!(cfg.formatting_mode, FormattingMode::Standard);
        assert!(cfg.trailing_space);
        assert!(!cfg.vad_enabled);
        assert_eq!(cfg.vad_silence_timeout_ms, 1800);
        assert_eq!(cfg.vad_energy_threshold, 0.015);
    }

    #[test]
    fn test_toml_roundtrip() {
        let mut cfg = Config::default();
        cfg.server_url = "https://api.groq.com/openai/v1/audio/transcriptions".to_string();
        cfg.model = "whisper-large-v3".to_string();
        cfg.formatting_mode = FormattingMode::SnakeCase;
        cfg.trailing_space = false;
        cfg.sound_volume = 0.75;
        cfg.vad_enabled = true;

        let toml_str = toml::to_string_pretty(&cfg).expect("Serialization failed");
        let parsed: Config = toml::from_str(&toml_str).expect("Deserialization failed");

        assert_eq!(parsed.server_url, cfg.server_url);
        assert_eq!(parsed.model, cfg.model);
        assert_eq!(parsed.formatting_mode, FormattingMode::SnakeCase);
        assert!(!parsed.trailing_space);
        assert_eq!(parsed.sound_volume, 0.75);
        assert!(parsed.vad_enabled);
    }

    #[test]
    fn test_toml_partial_parsing() {
        let partial = r#"
            server_url = "http://127.0.0.1:9000/v1/audio/transcriptions"
            formatting_mode = "camel_case"
            vad_enabled = true
        "#;
        let parsed: Config = toml::from_str(partial).expect("Partial parsing failed");
        assert_eq!(
            parsed.server_url,
            "http://127.0.0.1:9000/v1/audio/transcriptions"
        );
        assert_eq!(parsed.model, "whisper"); // default
        assert_eq!(parsed.formatting_mode, FormattingMode::CamelCase);
        assert!(parsed.vad_enabled);
        assert_eq!(parsed.ptt_threshold_ms, 350); // default
        assert!(parsed.sound_feedback); // default
    }

    #[test]
    fn test_sound_player_helper() {
        let mut cfg = Config::default();
        cfg.sound_feedback = true;
        cfg.sound_volume = 0.4;
        let player = cfg.sound_player();
        assert!(player.is_enabled());
        assert_eq!(player.volume(), 0.4);
    }

    #[test]
    fn test_vad_config_helper() {
        let mut cfg = Config::default();
        cfg.vad_enabled = true;
        cfg.vad_silence_timeout_ms = 2500;
        cfg.vad_energy_threshold = 0.03;

        let vad_cfg = cfg.vad_config();
        assert!(vad_cfg.enabled);
        assert_eq!(vad_cfg.silence_timeout, Duration::from_millis(2500));
        assert_eq!(vad_cfg.energy_threshold, 0.03);
    }

    #[test]
    fn test_hud_config_defaults_and_parsing() {
        let default_cfg = Config::default();
        assert!(default_cfg.hud_enabled);
        assert_eq!(default_cfg.hud_position, HudPosition::BottomCenter);

        let custom = r#"
            hud_enabled = false
            hud_position = "top_center"
        "#;
        let parsed: Config = toml::from_str(custom).expect("Failed to parse HUD config");
        assert!(!parsed.hud_enabled);
        assert_eq!(parsed.hud_position, HudPosition::TopCenter);

        let parsed_pos: HudPosition = serde_json::from_str(r#""bottom_right""#).unwrap();
        assert_eq!(parsed_pos, HudPosition::BottomRight);
    }

    #[test]
    fn test_trailing_space_custom_config() {
        let toml_data = r#"
            trailing_space = false
        "#;
        let parsed: Config = toml::from_str(toml_data).expect("Failed to parse trailing space");
        assert!(!parsed.trailing_space);
    }

    #[test]
    fn test_notifications_and_hud_aliases() {
        let toml_data = r#"
            de_notifications = false
            enable_hud = false
        "#;
        let parsed: Config = toml::from_str(toml_data).expect("Failed to parse aliases");
        assert!(!parsed.show_notifications);
        assert!(!parsed.hud_enabled);

        let toml_data2 = r#"
            notifications = true
            hud = true
        "#;
        let parsed2: Config = toml::from_str(toml_data2).expect("Failed to parse aliases");
        assert!(parsed2.show_notifications);
        assert!(parsed2.hud_enabled);
    }

    #[test]
    fn test_evdev_config_and_key_parsing() {
        let default_cfg = Config::default();
        assert!(default_cfg.evdev_hotkey_enabled);
        assert_eq!(default_cfg.evdev_hotkey, "KEY_RIGHTALT");

        let toml_data = r#"
            evdev_hotkey_enabled = false
            evdev_hotkey = "KEY_MICMUTE"
        "#;
        let parsed: Config = toml::from_str(toml_data).expect("Failed to parse evdev config");
        assert!(!parsed.evdev_hotkey_enabled);
        assert_eq!(parsed.evdev_hotkey, "KEY_MICMUTE");

        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                parse_evdev_key("KEY_RIGHTALT"),
                Some(evdev::Key::KEY_RIGHTALT)
            );
            assert_eq!(parse_evdev_key("rightalt"), Some(evdev::Key::KEY_RIGHTALT));
            assert_eq!(parse_evdev_key("RightAlt"), Some(evdev::Key::KEY_RIGHTALT));
            assert_eq!(parse_evdev_key("100"), Some(evdev::Key::KEY_RIGHTALT));
            assert_eq!(parse_evdev_key("KEY_HELP"), Some(evdev::Key::KEY_HELP));
            assert_eq!(parse_evdev_key("help"), Some(evdev::Key::KEY_HELP));
            assert_eq!(parse_evdev_key("Help"), Some(evdev::Key::KEY_HELP));
            assert_eq!(parse_evdev_key("138"), Some(evdev::Key::KEY_HELP));
            assert_eq!(
                parse_evdev_key("KEY_MICMUTE"),
                Some(evdev::Key::KEY_MICMUTE)
            );
            assert_eq!(parse_evdev_key("micmute"), Some(evdev::Key::KEY_MICMUTE));
            assert_eq!(parse_evdev_key("nonexistent_key_12345"), None);
        }
    }
}
