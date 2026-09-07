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
}

impl OutputManager {
    pub fn new(config: &Config) -> Self {
        Self {
            injector: TextInjector::new(),
            mode: config.output_mode,
            paste_delay_ms: config.paste_delay_ms,
        }
    }

    pub fn update_config(&mut self, config: &Config) {
        self.mode = config.output_mode;
        self.paste_delay_ms = config.paste_delay_ms;
    }

    pub fn output_text(&mut self, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            return Ok(());
        }

        match self.mode {
            OutputMode::Paste => {
                // Set clipboard first, then trigger paste
                set_clipboard(text)?;
                self.injector.paste_clipboard(self.paste_delay_ms)?;
            }
            OutputMode::ClipboardOnly => {
                set_clipboard(text)?;
            }
            OutputMode::Type => {
                // Fallback to paste if type not implemented, or paste directly
                set_clipboard(text)?;
                self.injector.paste_clipboard(self.paste_delay_ms)?;
            }
        }
        Ok(())
    }
}
