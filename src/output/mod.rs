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
