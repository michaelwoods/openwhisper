use crate::dataset::{DEFAULT_MIN_DURATION_SECS, DEFAULT_SEED, DEFAULT_TRAIN_RATIO, ExportFormat};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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

        /// Run system tray and HUD overlay within the daemon process (unified mode)
        #[arg(long, default_value_t = false)]
        with_ui: bool,
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

    /// Stop the running OpenWhisper daemon cleanly
    #[command(alias = "quit", alias = "shutdown")]
    Stop,

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

    /// Run comprehensive pre-flight system diagnostics and health checks
    #[command(alias = "check-system", alias = "diag")]
    Doctor {
        /// Output diagnostic results formatted as JSON
        #[arg(long, default_value_t = false)]
        json: bool,

        /// CI-safe mode: skip the host-dependent audio-capture and STT-endpoint checks and
        /// run only the environment-independent checks (real logic, no stubs)
        #[arg(long, default_value_t = false)]
        ci: bool,
    },

    /// Run OpenWhisper graphical UI service (System Tray icon & Floating HUD overlay)
    #[command(alias = "tray")]
    Ui,

    /// Manage automatic launch at startup (systemd user services and XDG autostart)
    Autostart {
        /// Enable launch at startup
        #[arg(long, conflicts_with = "disable")]
        enable: bool,

        /// Disable launch at startup
        #[arg(long, conflicts_with = "enable")]
        disable: bool,

        /// Output autostart status formatted as JSON
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Export paired .wav/.txt recordings as a TTS training dataset (LJSpeech / Piper / XTTS)
    #[command(alias = "export-dataset")]
    ExportTtsDataset {
        /// Directory with paired .wav/.txt recordings (defaults to the configured save_audio_dir)
        #[arg(short, long)]
        source: Option<PathBuf>,

        /// Output directory for the dataset (wavs/ plus manifests)
        #[arg(short, long, default_value = "tts_dataset")]
        output: PathBuf,

        /// Drop clips shorter than this many seconds
        #[arg(long, default_value_t = DEFAULT_MIN_DURATION_SECS)]
        min_duration: f32,

        /// Fraction of clips assigned to the training split (rest is validation)
        #[arg(long, default_value_t = DEFAULT_TRAIN_RATIO)]
        train_ratio: f64,

        /// Manifest format to write
        #[arg(long, value_enum, default_value_t = ExportFormat::All)]
        format: ExportFormat,

        /// Seed for the deterministic train/validation split
        #[arg(long, default_value_t = DEFAULT_SEED)]
        seed: u64,

        /// Emit the machine-readable summary as JSON on stdout (human summary is stderr-only)
        #[arg(long, default_value_t = false)]
        json: bool,
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
    fn test_cli_stop_aliases() {
        let stop = Cli::try_parse_from(["openwhisper", "stop"]).unwrap();
        assert!(matches!(stop.command, Some(Commands::Stop)));

        let quit = Cli::try_parse_from(["openwhisper", "quit"]).unwrap();
        assert!(matches!(quit.command, Some(Commands::Stop)));

        let shutdown = Cli::try_parse_from(["openwhisper", "shutdown"]).unwrap();
        assert!(matches!(shutdown.command, Some(Commands::Stop)));
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

    #[test]
    fn test_cli_ui_command() {
        let ui = Cli::try_parse_from(["openwhisper", "ui"]).unwrap();
        assert!(matches!(ui.command, Some(Commands::Ui)));

        let tray = Cli::try_parse_from(["openwhisper", "tray"]).unwrap();
        assert!(matches!(tray.command, Some(Commands::Ui)));
    }

    #[test]
    fn test_cli_autostart_command() {
        let status = Cli::try_parse_from(["openwhisper", "autostart"]).unwrap();
        match status.command {
            Some(Commands::Autostart {
                enable,
                disable,
                json,
            }) => {
                assert!(!enable);
                assert!(!disable);
                assert!(!json);
            }
            _ => panic!("Expected Commands::Autostart"),
        }

        let enable = Cli::try_parse_from(["openwhisper", "autostart", "--enable"]).unwrap();
        match enable.command {
            Some(Commands::Autostart { enable, .. }) => assert!(enable),
            _ => panic!("Expected Commands::Autostart with enable"),
        }
    }

    #[test]
    fn test_cli_export_tts_dataset_defaults_and_flags() {
        let d = Cli::try_parse_from(["openwhisper", "export-tts-dataset"]).unwrap();
        match d.command {
            Some(Commands::ExportTtsDataset {
                source,
                output,
                min_duration,
                train_ratio,
                format,
                seed,
                json,
            }) => {
                assert!(source.is_none());
                assert_eq!(output, PathBuf::from("tts_dataset"));
                assert_eq!(min_duration, 0.5);
                assert_eq!(train_ratio, 0.95);
                assert_eq!(format, ExportFormat::All);
                assert_eq!(seed, DEFAULT_SEED);
                assert!(!json);
            }
            _ => panic!("Expected Commands::ExportTtsDataset"),
        }

        let f = Cli::try_parse_from([
            "openwhisper",
            "export-dataset",
            "--source",
            "/rec",
            "-o",
            "/out",
            "--min-duration",
            "1.25",
            "--train-ratio",
            "0.8",
            "--format",
            "ljspeech",
            "--json",
        ])
        .unwrap();
        match f.command {
            Some(Commands::ExportTtsDataset {
                source,
                min_duration,
                train_ratio,
                format,
                json,
                ..
            }) => {
                assert_eq!(source, Some(PathBuf::from("/rec")));
                assert_eq!(min_duration, 1.25);
                assert_eq!(train_ratio, 0.8);
                assert_eq!(format, ExportFormat::LjSpeech);
                assert!(json);
            }
            _ => panic!("Expected Commands::ExportTtsDataset"),
        }

        assert!(
            Cli::try_parse_from(["openwhisper", "export-tts-dataset", "--format", "bogus"])
                .is_err()
        );
    }
}
