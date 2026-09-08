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

    /// Persistent transcription history (search, view, copy, delete, or launch GUI)
    History {
        /// Number of entries to display
        #[arg(short, long, default_value_t = 15)]
        limit: usize,

        /// Search query to filter history entries
        #[arg(short, long)]
        search: Option<String>,

        /// Copy entry with the given ID to system clipboard
        #[arg(long)]
        copy: Option<i64>,

        /// Delete entry with the given ID
        #[arg(long)]
        delete: Option<i64>,

        /// Clear all history entries
        #[arg(long, default_value_t = false)]
        clear: bool,

        /// Output results formatted as JSON
        #[arg(long, default_value_t = false)]
        json: bool,

        /// Play recorded audio for entry with the given ID
        #[arg(long)]
        play: Option<i64>,

        /// Launch dedicated standalone History graphical window
        #[arg(long, default_value_t = false)]
        gui: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_default_no_subcommand() {
        let cli = Cli::try_parse_from(["openwhisper"]).unwrap();
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_simple_subcommands() {
        let toggle = Cli::try_parse_from(["openwhisper", "toggle"]).unwrap();
        assert!(matches!(toggle.command, Some(Commands::Toggle)));

        let status = Cli::try_parse_from(["openwhisper", "status"]).unwrap();
        assert!(matches!(status.command, Some(Commands::Status)));

        let cancel = Cli::try_parse_from(["openwhisper", "cancel"]).unwrap();
        assert!(matches!(cancel.command, Some(Commands::Cancel)));
    }

    #[test]
    fn test_cli_aliases() {
        let gui = Cli::try_parse_from(["openwhisper", "gui"]).unwrap();
        assert!(matches!(gui.command, Some(Commands::ConfigGui)));

        let settings = Cli::try_parse_from(["openwhisper", "settings"]).unwrap();
        assert!(matches!(settings.command, Some(Commands::ConfigGui)));

        let reload = Cli::try_parse_from(["openwhisper", "reload-config"]).unwrap();
        assert!(matches!(reload.command, Some(Commands::Reload)));

        let sniff = Cli::try_parse_from(["openwhisper", "sniff-keys"]).unwrap();
        assert!(matches!(sniff.command, Some(Commands::TestHotkey)));
    }

    #[test]
    fn test_cli_record_arguments() {
        let rec = Cli::try_parse_from(["openwhisper", "record", "--duration", "5", "--no-paste"])
            .unwrap();
        match rec.command {
            Some(Commands::Record { duration, no_paste }) => {
                assert_eq!(duration, Some(5));
                assert!(no_paste);
            }
            _ => panic!("Expected Commands::Record"),
        }
    }

    #[test]
    fn test_cli_hud_demo_once() {
        let hud = Cli::try_parse_from(["openwhisper", "hud-demo", "--once"]).unwrap();
        match hud.command {
            Some(Commands::HudDemo { once }) => assert!(once),
            _ => panic!("Expected Commands::HudDemo"),
        }
    }

    #[test]
    fn test_cli_history_arguments() {
        let hist = Cli::try_parse_from([
            "openwhisper",
            "history",
            "--limit",
            "25",
            "--search",
            "meeting",
            "--gui",
        ])
        .unwrap();
        match hist.command {
            Some(Commands::History {
                limit,
                search,
                copy,
                play,
                delete,
                clear,
                json,
                gui,
            }) => {
                assert_eq!(limit, 25);
                assert_eq!(search, Some("meeting".to_string()));
                assert!(copy.is_none());
                assert!(play.is_none());
                assert!(delete.is_none());
                assert!(!clear);
                assert!(!json);
                assert!(gui);
            }
            _ => panic!("Expected Commands::History"),
        }

        let hist_play = Cli::try_parse_from(["openwhisper", "history", "--play", "42"]).unwrap();
        match hist_play.command {
            Some(Commands::History { play, .. }) => {
                assert_eq!(play, Some(42));
            }
            _ => panic!("Expected Commands::History with play"),
        }
    }
}
