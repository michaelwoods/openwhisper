use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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

    #[serde(default = "default_true")]
    pub show_notifications: bool,

    #[serde(default)]
    pub audio_device: Option<String>,

    #[serde(default = "default_socket_path")]
    pub socket_path: String,
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

fn default_socket_path() -> String {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        format!("{}/openwhisper.sock", runtime_dir)
    } else {
        "/tmp/openwhisper.sock".to_string()
    }
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
            audio_device: None,
            socket_path: default_socket_path(),
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
        let content = toml::to_string_pretty(self)
            .context("Failed to serialize config to TOML")?;
        fs::write(&path, content)
            .with_context(|| format!("Failed to write config file to {:?}", path))?;
        Ok(())
    }
}
