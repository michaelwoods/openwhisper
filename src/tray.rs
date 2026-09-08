use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
    Idle,
    Recording,
    Transcribing,
    Error,
}

#[cfg(target_os = "linux")]
pub struct OpenWhisperTray {
    pub state: Arc<RwLock<TrayState>>,
    pub socket_path: String,
    pub history: Arc<RwLock<VecDeque<String>>>,
}

#[cfg(target_os = "linux")]
impl OpenWhisperTray {
    pub fn new(state: Arc<RwLock<TrayState>>, socket_path: String, history: Arc<RwLock<VecDeque<String>>>) -> Self {
        Self { state, socket_path, history }
    }

    /// Generate a 24x24 ARGB fallback icon pixmap in-memory
    fn generate_fallback_icon(&self, state: TrayState) -> ksni::Icon {
        let width = 24;
        let height = 24;
        let mut data = vec![0u8; width * height * 4];

        // Color scheme based on state: [A, R, G, B]
        let (r, g, b) = match state {
            TrayState::Idle => (241, 245, 249),        // Crisp light white/slate
            TrayState::Recording => (239, 68, 68),      // Vivid red
            TrayState::Transcribing => (56, 189, 248),  // Cyan blue
            TrayState::Error => (245, 158, 11),        // Amber warning
        };

        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) * 4;
                let mut filled = false;

                // Simple microphone shape rasterizer
                // Capsule: x in 9..=14, y in 3..=13
                if (9..=14).contains(&x) && (3..=13).contains(&y) {
                    filled = true;
                }
                // U-shape cradle: y == 13..=16, x == 6 or x == 17
                if (11..=15).contains(&y) && (x == 6 || x == 17) {
                    filled = true;
                }
                if y == 16 && (6..=17).contains(&x) {
                    filled = true;
                }
                // Stand stem: x in 11..=12, y in 17..=20
                if (11..=12).contains(&x) && (17..=20).contains(&y) {
                    filled = true;
                }
                // Base: y == 21, x in 8..=15
                if y == 21 && (8..=15).contains(&x) {
                    filled = true;
                }

                // If recording, add a red indicator dot in top-right
                if state == TrayState::Recording && (1..=5).contains(&y) && (17..=21).contains(&x) {
                    data[idx] = 255;     // Alpha
                    data[idx + 1] = 239; // Red
                    data[idx + 2] = 68;  // Green
                    data[idx + 3] = 68;  // Blue
                    continue;
                }

                if filled {
                    data[idx] = 255;     // Alpha
                    data[idx + 1] = r;   // Red
                    data[idx + 2] = g;   // Green
                    data[idx + 3] = b;   // Blue
                } else {
                    data[idx] = 0;       // Transparent
                    data[idx + 1] = 0;
                    data[idx + 2] = 0;
                    data[idx + 3] = 0;
                }
            }
        }

        ksni::Icon {
            width: width as i32,
            height: height as i32,
            data,
        }
    }
}

#[cfg(target_os = "linux")]
impl ksni::Tray for OpenWhisperTray {
    fn id(&self) -> String {
        "openwhisper".into()
    }

    fn title(&self) -> String {
        "OpenWhisper Dictation Assistant".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }

    fn status(&self) -> ksni::Status {
        ksni::Status::Active
    }

    fn icon_name(&self) -> String {
        let state = self.state.read().map(|s| *s).unwrap_or(TrayState::Idle);
        match state {
            TrayState::Idle => "openwhisper-tray-idle".into(),
            TrayState::Recording => "openwhisper-tray-recording".into(),
            TrayState::Transcribing => "openwhisper-tray-transcribing".into(),
            TrayState::Error => "openwhisper-tray-error".into(),
        }
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let state = self.state.read().map(|s| *s).unwrap_or(TrayState::Idle);
        vec![self.generate_fallback_icon(state)]
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        // Left-click triggers dictation toggle
        info!("Tray icon activated: toggling dictation");
        let socket = self.socket_path.clone();
        tokio::spawn(async move {
            let _ = crate::hotkey::ipc::send_ipc_command(&socket, crate::hotkey::ipc::IpcCommand::Toggle).await;
        });
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{MenuItem, StandardItem, SubMenu};

        let current_state = self.state.read().map(|s| *s).unwrap_or(TrayState::Idle);
        let status_label = match current_state {
            TrayState::Idle => "● Status: Ready (Idle)".to_string(),
            TrayState::Recording => "🔴 Status: Recording...".to_string(),
            TrayState::Transcribing => "⏳ Status: Transcribing...".to_string(),
            TrayState::Error => "⚠️ Status: Error".to_string(),
        };

        let socket_for_toggle = self.socket_path.clone();

        let mut items: Vec<ksni::MenuItem<Self>> = vec![
            StandardItem {
                label: status_label,
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "🎙️  Toggle Dictation".into(),
                activate: Box::new(move |_| {
                    let s = socket_for_toggle.clone();
                    tokio::spawn(async move {
                        let _ = crate::hotkey::ipc::send_ipc_command(&s, crate::hotkey::ipc::IpcCommand::Toggle).await;
                    });
                }),
                ..Default::default()
            }
            .into(),
        ];

        let history_items = self.history.read().map(|h| h.clone()).unwrap_or_default();
        if !history_items.is_empty() {
            let mut recent_subitems: Vec<ksni::MenuItem<Self>> = Vec::new();
            for (idx, item) in history_items.iter().take(5).enumerate() {
                let full_text = item.clone();
                let display_text = if full_text.chars().count() > 36 {
                    let truncated: String = full_text.chars().take(36).collect();
                    format!("{}. {}...", idx + 1, truncated)
                } else {
                    format!("{}. {}", idx + 1, full_text)
                };

                recent_subitems.push(
                    StandardItem {
                        label: display_text,
                        activate: Box::new(move |_| {
                            let text = full_text.clone();
                            if let Err(e) = crate::output::set_clipboard(&text) {
                                warn!("Failed to copy recent item to clipboard: {e}");
                            } else {
                                info!("Copied recent transcription to clipboard from tray menu");
                                std::thread::spawn(move || {
                                    let _ = notify_rust::Notification::new()
                                        .summary("OpenWhisper — Copied to Clipboard")
                                        .body(&text)
                                        .icon("openwhisper")
                                        .timeout(notify_rust::Timeout::Milliseconds(3000))
                                        .show();
                                });
                            }
                        }),
                        ..Default::default()
                    }
                    .into(),
                );
            }

            items.push(
                SubMenu {
                    label: "📋  Recent Dictations".into(),
                    submenu: recent_subitems,
                    ..Default::default()
                }
                .into(),
            );
        }

        items.push(
            StandardItem {
                label: "⚙️  Settings / Configuration...".into(),
                activate: Box::new(|_| {
                    info!("Launching OpenWhisper Settings GUI from tray menu...");
                    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/usr/local/bin/openwhisper"));
                    if let Err(e) = std::process::Command::new(exe).arg("config-gui").spawn() {
                        warn!("Failed to spawn settings GUI: {}", e);
                    }
                }),
                ..Default::default()
            }
            .into(),
        );

        items.push(MenuItem::Separator);

        items.push(
            StandardItem {
                label: "❌ Quit OpenWhisper".into(),
                activate: Box::new(|_| {
                    info!("Quitting OpenWhisper via system tray menu");
                    std::process::exit(0);
                }),
                ..Default::default()
            }
            .into(),
        );

        items
    }
}

/// Handle to dynamically update the running tray state
#[derive(Clone)]
pub struct TrayController {
    #[cfg(target_os = "linux")]
    handle: Option<ksni::Handle<OpenWhisperTray>>,
    state: Arc<RwLock<TrayState>>,
    history: Arc<RwLock<VecDeque<String>>>,
}

impl TrayController {
    #[cfg(target_os = "linux")]
    pub fn new(
        handle: Option<ksni::Handle<OpenWhisperTray>>,
        state: Arc<RwLock<TrayState>>,
        history: Arc<RwLock<VecDeque<String>>>,
    ) -> Self {
        Self { handle, state, history }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn new(state: Arc<RwLock<TrayState>>, history: Arc<RwLock<VecDeque<String>>>) -> Self {
        Self { state, history }
    }

    pub fn set_state(&self, new_state: TrayState) {
        if let Ok(mut lock) = self.state.write() {
            *lock = new_state;
        }
        #[cfg(target_os = "linux")]
        if let Some(ref handle) = self.handle {
            let handle = handle.clone();
            tokio::spawn(async move {
                handle.update(|tray| {
                    if let Ok(mut lock) = tray.state.write() {
                        *lock = new_state;
                    }
                }).await;
            });
        }
    }

    pub fn add_history(&self, text: String) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Ok(mut lock) = self.history.write() {
            // Avoid duplicate consecutive entries
            if lock.front().map(|s| s.as_str()) == Some(trimmed) {
                return;
            }
            lock.push_front(trimmed.to_string());
            if lock.len() > 10 {
                lock.pop_back();
            }
        }
        #[cfg(target_os = "linux")]
        if let Some(ref handle) = self.handle {
            let handle = handle.clone();
            tokio::spawn(async move {
                handle.update(|_| {}).await;
            });
        }
    }

    #[allow(dead_code)]
    pub fn history(&self) -> Vec<String> {
        self.history.read().map(|h| h.iter().cloned().collect()).unwrap_or_default()
    }
}


/// Spawn the system tray in background if on Linux
pub async fn start_tray_service(socket_path: String) -> (TrayController, Arc<RwLock<TrayState>>) {
    let state = Arc::new(RwLock::new(TrayState::Idle));
    let history = Arc::new(RwLock::new(VecDeque::new()));

    #[cfg(target_os = "linux")]
    {
        use ksni::TrayMethods;
        let tray = OpenWhisperTray::new(state.clone(), socket_path, history.clone());
        match tray.spawn().await {
            Ok(handle) => {
                info!("System tray (StatusNotifierItem) initialized successfully.");
                (TrayController::new(Some(handle), state.clone(), history), state)
            }
            Err(err) => {
                warn!("StatusNotifierWatcher not available or failed to register tray: {err}. Running without system tray.");
                (TrayController::new(None, state.clone(), history), state)
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        (TrayController::new(state.clone(), history), state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tray_fallback_icon_dimensions() {
        let state = Arc::new(RwLock::new(TrayState::Idle));
        let history = Arc::new(RwLock::new(VecDeque::new()));
        let tray = OpenWhisperTray::new(state, "/tmp/test.sock".into(), history);

        for s in [TrayState::Idle, TrayState::Recording, TrayState::Transcribing, TrayState::Error] {
            let icon = tray.generate_fallback_icon(s);
            assert_eq!(icon.width, 24);
            assert_eq!(icon.height, 24);
            assert_eq!(icon.data.len(), 24 * 24 * 4);
        }
    }

    #[test]
    fn test_tray_controller_state_update() {
        let state = Arc::new(RwLock::new(TrayState::Idle));
        let history = Arc::new(RwLock::new(VecDeque::new()));
        let ctrl = TrayController::new(None, state.clone(), history);

        assert_eq!(*state.read().unwrap(), TrayState::Idle);
        ctrl.set_state(TrayState::Recording);
        assert_eq!(*state.read().unwrap(), TrayState::Recording);
        ctrl.set_state(TrayState::Transcribing);
        assert_eq!(*state.read().unwrap(), TrayState::Transcribing);
        ctrl.set_state(TrayState::Error);
        assert_eq!(*state.read().unwrap(), TrayState::Error);
    }

    #[test]
    fn test_tray_controller_history_ring_buffer() {
        let state = Arc::new(RwLock::new(TrayState::Idle));
        let history = Arc::new(RwLock::new(VecDeque::new()));
        let ctrl = TrayController::new(None, state, history);

        assert!(ctrl.history().is_empty());

        // Empty string ignored
        ctrl.add_history("   ".to_string());
        assert!(ctrl.history().is_empty());

        // Adding entries
        ctrl.add_history("First transcription".to_string());
        assert_eq!(ctrl.history(), vec!["First transcription".to_string()]);

        // Consecutive duplicate ignored
        ctrl.add_history("First transcription".to_string());
        assert_eq!(ctrl.history().len(), 1);

        // Add second entry (prepends)
        ctrl.add_history("Second transcription".to_string());
        assert_eq!(ctrl.history(), vec![
            "Second transcription".to_string(),
            "First transcription".to_string(),
        ]);

        // Push 11 items to test ring buffer max capacity (10)
        for i in 1..=11 {
            ctrl.add_history(format!("Batch dictation #{}", i));
        }

        let current = ctrl.history();
        assert_eq!(current.len(), 10);
        assert_eq!(current[0], "Batch dictation #11");
        assert_eq!(current[9], "Batch dictation #2");
    }
}

