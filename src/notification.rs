use notify_rust::{Notification, Timeout};

pub struct NotificationManager {
    enabled: bool,
}

impl NotificationManager {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    #[allow(dead_code)]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn recording_started(&self) {
        if !self.enabled {
            return;
        }
        std::thread::spawn(|| {
            let _ = Notification::new()
                .appname("OpenWhisper")
                .summary("🎙️ Listening...")
                .body("Recording audio. Release key or tap to finish.")
                .timeout(Timeout::Milliseconds(2000))
                .show();
        });
    }

    pub fn transcribing(&self) {
        if !self.enabled {
            return;
        }
        std::thread::spawn(|| {
            let _ = Notification::new()
                .appname("OpenWhisper")
                .summary("⏳ Transcribing...")
                .body("Processing speech with Whisper...")
                .timeout(Timeout::Milliseconds(2000))
                .show();
        });
    }

    pub fn transcribed(&self, text: &str) {
        if !self.enabled {
            return;
        }
        let preview = safe_truncate_chars(text, 100);

        std::thread::spawn(move || {
            let _ = Notification::new()
                .appname("OpenWhisper")
                .summary("✍️ Transcribed")
                .body(&preview)
                .timeout(Timeout::Milliseconds(3500))
                .show();
        });
    }

    pub fn error(&self, message: &str) {
        if !self.enabled {
            return;
        }
        let msg = message.to_string();
        std::thread::spawn(move || {
            let _ = Notification::new()
                .appname("OpenWhisper")
                .summary("❌ OpenWhisper Error")
                .body(&msg)
                .timeout(Timeout::Milliseconds(5000))
                .show();
        });
    }
}

pub fn safe_truncate_chars(s: &str, max_chars: usize) -> String {
    let mut char_indices = s.char_indices();
    if let Some((idx, _)) = char_indices.nth(max_chars) {
        format!("{}...", s[..idx].trim_end())
    } else {
        s.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_manager_toggle() {
        let mut mgr = NotificationManager::new(true);
        assert!(mgr.is_enabled());
        mgr.set_enabled(false);
        assert!(!mgr.is_enabled());
    }

    #[test]
    fn test_safe_truncate_chars_short() {
        assert_eq!(safe_truncate_chars("Short text", 20), "Short text");
    }

    #[test]
    fn test_safe_truncate_chars_long() {
        let long_str = "This is a longer string that exceeds the max characters";
        assert_eq!(safe_truncate_chars(long_str, 10), "This is a...");
    }

    #[test]
    fn test_safe_truncate_multibyte_utf8() {
        // Multi-byte Unicode characters (e.g. 🦀 is 4 bytes)
        let s = "🦀 Rust is awesome 🚀";
        // Taking 3 chars: '🦀', ' ', 'R'
        assert_eq!(safe_truncate_chars(s, 3), "🦀 R...");
    }
}
