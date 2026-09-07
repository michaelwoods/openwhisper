use notify_rust::{Notification, Timeout};

pub struct NotificationManager {
    enabled: bool,
}

impl NotificationManager {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
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
        let preview = if text.len() > 100 {
            format!("{}...", &text[..100])
        } else {
            text.to_string()
        };

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
