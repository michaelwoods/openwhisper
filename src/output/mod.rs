pub mod clipboard;
pub mod injector;

pub use clipboard::set_clipboard;
pub use injector::TextInjector;

use crate::config::{Config, OutputMode};
use anyhow::Result;

pub struct OutputManager {
    injector: TextInjector,
    mode: OutputMode,
    paste_delay_ms: u64,
    restore_clipboard: bool,
}

impl OutputManager {
    pub fn new(config: &Config) -> Self {
        Self {
            injector: TextInjector::new(),
            mode: config.output_mode,
            paste_delay_ms: config.paste_delay_ms,
            restore_clipboard: config.restore_clipboard,
        }
    }

    pub fn update_config(&mut self, config: &Config) {
        self.mode = config.output_mode;
        self.paste_delay_ms = config.paste_delay_ms;
        self.restore_clipboard = config.restore_clipboard;
    }

    pub fn output_text(&mut self, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            return Ok(());
        }

        match self.mode {
            OutputMode::Paste => {
                let prev_clip = if self.restore_clipboard {
                    clipboard::get_clipboard()
                } else {
                    None
                };

                // Set clipboard first, then trigger Ctrl+V paste
                set_clipboard(text)?;
                self.injector.paste_clipboard(self.paste_delay_ms)?;

                // Asynchronously restore previous clipboard contents after target app consumed paste
                if let Some(prev) = prev_clip {
                    if prev != text {
                        std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_millis(350));
                            let _ = set_clipboard(&prev);
                            tracing::debug!("Restored previous clipboard contents");
                        });
                    }
                }
            }
            OutputMode::ClipboardOnly => {
                set_clipboard(text)?;
            }
            OutputMode::Type => {
                let prev_clip = if self.restore_clipboard {
                    clipboard::get_clipboard()
                } else {
                    None
                };

                // Ensure text is stored in clipboard as a fallback for manual pasting if target window loses focus
                set_clipboard(text)?;
                self.injector.type_text(text)?;

                if let Some(prev) = prev_clip {
                    if prev != text {
                        std::thread::spawn(move || {
                            // In type mode, keep transcription on clipboard for 5 seconds so user has time
                            // to manually paste if target lost focus, then restore previous clipboard.
                            std::thread::sleep(std::time::Duration::from_millis(5000));
                            let _ = set_clipboard(&prev);
                            tracing::debug!("Restored previous clipboard contents after type fallback window");
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_manager_lifecycle_and_config() {
        let mut cfg = Config::default();
        cfg.output_mode = OutputMode::Paste;
        cfg.paste_delay_ms = 45;
        cfg.restore_clipboard = true;

        let mut manager = OutputManager::new(&cfg);
        assert_eq!(manager.mode, OutputMode::Paste);
        assert_eq!(manager.paste_delay_ms, 45);
        assert!(manager.restore_clipboard);

        cfg.output_mode = OutputMode::Type;
        cfg.paste_delay_ms = 100;
        cfg.restore_clipboard = false;

        manager.update_config(&cfg);
        assert_eq!(manager.mode, OutputMode::Type);
        assert_eq!(manager.paste_delay_ms, 100);
        assert!(!manager.restore_clipboard);
    }

    #[test]
    fn test_output_empty_string_noop() {
        let cfg = Config::default();
        let mut manager = OutputManager::new(&cfg);

        assert!(manager.output_text("").is_ok());
        assert!(manager.output_text("   \n\t  ").is_ok());
    }
}
