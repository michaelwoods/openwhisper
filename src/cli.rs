use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "openwhisper")]
#[command(author, version, about = "Cross-platform speech-to-text assistant powered by Whisper", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run OpenWhisper background service (handles hotkeys, IPC, recording, and text pasting)
    Daemon {
        /// Optional path to config file
        #[arg(short, long)]
        config: Option<String>,
    },

    /// Toggle dictation on/off (sends toggle signal to running daemon)
    Toggle,

    /// Push-to-talk keydown event (sends keydown to running daemon)
    PttDown,

    /// Push-to-talk keyup event (sends keyup to running daemon)
    PttUp,

    /// Cancel active recording
    Cancel,

    /// Query the running daemon status
    Status,

    /// Standalone one-shot recording & transcription (press Enter to stop)
    Record {
        /// Optional recording duration in seconds (stops automatically after this duration)
        #[arg(short, long)]
        duration: Option<u64>,

        /// Do not paste into active window, only copy to clipboard and print
        #[arg(long)]
        no_paste: bool,
    },

    /// Test connection to local OpenVINO model server / OpenAI-compatible endpoint
    TestOvms {
        /// Override endpoint URL
        #[arg(long)]
        url: Option<String>,

        /// Override model name
        #[arg(long)]
        model: Option<String>,
    },

    /// List available audio input devices (microphones)
    ListDevices,

    /// Initialize default ~/.config/openwhisper/config.toml
    InitConfig,

    /// Open native graphical configuration panel
    #[command(alias = "gui", alias = "settings")]
    ConfigGui,

    /// Reload daemon configuration from config.toml via IPC
    #[command(alias = "reload-config")]
    Reload,

    /// Run automated setup: install binary, icons, desktop entries, and systemd service
    Setup,

    /// Launch interactive preview of the floating HUD overlay
    #[command(alias = "test-hud", alias = "hud")]
    HudDemo {
        /// Run only one preview cycle and automatically exit
        #[arg(long, default_value_t = false)]
        once: bool,
    },

    /// Sniff hardware key events via evdev to inspect scancodes, press/release, and hold duration
    #[command(alias = "test-key", alias = "sniff-keys")]
    TestHotkey,
}

