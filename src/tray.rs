use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
    Idle,
    Recording,
    Transcribing,
    Degraded,
    Error,
}

#[cfg(target_os = "linux")]
pub struct OpenWhisperTray {
    pub state: Arc<RwLock<TrayState>>,
    pub socket_path: String,
    pub history: Arc<RwLock<VecDeque<String>>>,
    pub server_url: Arc<RwLock<String>>,
    pub last_latency: Arc<RwLock<Option<f32>>>,
    pub last_error: Arc<RwLock<Option<String>>>,
    pub active_device: Arc<RwLock<Option<String>>>,
}

#[cfg(target_os = "linux")]
impl OpenWhisperTray {
    pub fn new(
        state: Arc<RwLock<TrayState>>,
        socket_path: String,
        history: Arc<RwLock<VecDeque<String>>>,
        server_url: Arc<RwLock<String>>,
        last_latency: Arc<RwLock<Option<f32>>>,
        last_error: Arc<RwLock<Option<String>>>,
        active_device: Arc<RwLock<Option<String>>>,
    ) -> Self {
        Self {
            state,
            socket_path,
            history,
            server_url,
            last_latency,
            last_error,
            active_device,
        }
    }

    /// Generate a 24x24 ARGB fallback icon pixmap in-memory with a bold, solid silhouette
    #[allow(dead_code)]
    fn generate_fallback_icon(&self, state: TrayState) -> ksni::Icon {
        let width = 24;
        let height = 24;
        let mut data = vec![0u8; width * height * 4];

        // High-contrast colors matching standard desktop tray themes
        let (r, g, b) = match state {
            TrayState::Idle => (248, 250, 252), // Crisp bright white (#f8fafc)
            TrayState::Recording => (239, 68, 68), // Vivid red (#ef4444)
            TrayState::Transcribing => (56, 189, 248), // Cyan blue (#38bdf8)
            TrayState::Degraded => (245, 158, 11), // Warm amber (#f59e0b)
            TrayState::Error => (148, 163, 184), // Muted slate (#94a3b8)
        };

        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) * 4;
                let mut filled = false;

                // Bold solid microphone capsule: x in 8..=15, y in 2..=12
                if (8..=15).contains(&x) && (2..=12).contains(&y) {
                    // Rounded top & bottom corners
                    let is_top_corner = (x == 8 || x == 15) && y == 2;
                    let is_bottom_corner = (x == 8 || x == 15) && y == 12;
                    if !is_top_corner && !is_bottom_corner {
                        filled = true;
                    }
                }

                // U-shape cradle:
                // Left arm: x in 5..=6, y in 9..=14
                if (5..=6).contains(&x) && (9..=14).contains(&y) {
                    filled = true;
                }
                // Right arm: x in 17..=18, y in 9..=14
                if (17..=18).contains(&x) && (9..=14).contains(&y) {
                    filled = true;
                }
                // Bottom cradle curve: y in 15..=16, x in 6..=17
                if (y == 15 || y == 16) && (6..=17).contains(&x) {
                    filled = true;
                }

                // Stand stem: x in 11..=12, y in 16..=20
                if (11..=12).contains(&x) && (16..=20).contains(&y) {
                    filled = true;
                }

                // Base: x in 7..=16, y in 20..=21
                if (7..=16).contains(&x) && (20..=21).contains(&y) {
                    filled = true;
                }

                // State accents:
                // Recording: sound wave arcs
                if state == TrayState::Recording
                    && ((2..=3).contains(&x) || (20..=21).contains(&x))
                    && (8..=14).contains(&y)
                {
                    data[idx] = 255;
                    data[idx + 1] = 239;
                    data[idx + 2] = 68;
                    data[idx + 3] = 68;
                    continue;
                }

                // Transcribing: side bracket arcs
                if state == TrayState::Transcribing
                    && ((2..=3).contains(&x) || (20..=21).contains(&x))
                    && (8..=15).contains(&y)
                {
                    data[idx] = 255;
                    data[idx + 1] = 56;
                    data[idx + 2] = 189;
                    data[idx + 3] = 248;
                    continue;
                }

                // State badge dots (top right 18..=21, 2..=5)
                if (18..=21).contains(&x) && (2..=5).contains(&y) {
                    match state {
                        TrayState::Recording => {
                            data[idx] = 255;
                            data[idx + 1] = 239;
                            data[idx + 2] = 68;
                            data[idx + 3] = 68;
                            continue;
                        }
                        TrayState::Degraded => {
                            data[idx] = 255;
                            data[idx + 1] = 245;
                            data[idx + 2] = 158;
                            data[idx + 3] = 11;
                            continue;
                        }
                        TrayState::Error => {
                            data[idx] = 255;
                            data[idx + 1] = 239;
                            data[idx + 2] = 68;
                            data[idx + 3] = 68;
                            continue;
                        }
                        _ => {}
                    }
                }

                if filled {
                    data[idx] = 255; // Alpha
                    data[idx + 1] = r; // Red
                    data[idx + 2] = g; // Green
                    data[idx + 3] = b; // Blue
                } else {
                    data[idx] = 0; // Transparent
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

    fn icon_theme_path(&self) -> String {
        // Resolve user icon directory ~/.local/share/icons/hicolor or system /usr/share/icons/hicolor
        if let Some(mut path) = directories::BaseDirs::new().map(|b| b.data_dir().to_path_buf()) {
            path.push("icons");
            path.push("hicolor");
            if path.exists() {
                return path.to_string_lossy().to_string();
            }
        }
        if std::path::Path::new("/usr/share/icons/hicolor").exists() {
            return "/usr/share/icons/hicolor".to_string();
        }
        String::new()
    }

    fn icon_name(&self) -> String {
        let state = self.state.read().map(|s| *s).unwrap_or(TrayState::Idle);
        match state {
            TrayState::Idle => "openwhisper-tray-idle".into(),
            TrayState::Recording => "openwhisper-tray-recording".into(),
            TrayState::Transcribing => "openwhisper-tray-transcribing".into(),
            TrayState::Degraded => "openwhisper-tray-degraded".into(),
            TrayState::Error => "openwhisper-tray-error".into(),
        }
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let state = self.state.read().map(|s| *s).unwrap_or(TrayState::Idle);
        let status_desc = match state {
            TrayState::Idle => "Ready (Idle)",
            TrayState::Recording => "Recording audio...",
            TrayState::Transcribing => "Transcribing with Whisper...",
            TrayState::Degraded => "Degraded (Fallback Mic / Network Warning)",
            TrayState::Error => "Error during transcription",
        };

        let server = self
            .server_url
            .read()
            .map(|s| s.clone())
            .unwrap_or_default();
        let mut desc = format!("Status: {}\nServer: {}", status_desc, server);

        if let Ok(dev_opt) = self.active_device.read()
            && let Some(ref d) = *dev_opt
        {
            desc.push_str(&format!("\nMicrophone: {}", d));
        }

        if let Ok(lat_opt) = self.last_latency.read()
            && let Some(lat) = *lat_opt
        {
            desc.push_str(&format!("\nLast Latency: {:.2}s", lat));
        }

        if let Ok(err_opt) = self.last_error.read()
            && let Some(ref err) = *err_opt
        {
            desc.push_str(&format!("\nLast Error: {}", err));
        }

        ksni::ToolTip {
            title: "OpenWhisper Dictation Assistant".into(),
            description: desc,
            icon_name: self.icon_name(),
            icon_pixmap: Vec::new(),
        }
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        // Return empty so compositors (KDE Plasma, GNOME, Sway) resolve the vector SVG
        // directly from icon_theme_path at native monitor resolution, avoiding blurry bitmap scaling.
        Vec::new()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        // Left-click triggers dictation toggle
        info!("Tray icon activated: toggling dictation");
        let socket = self.socket_path.clone();
        tokio::spawn(async move {
            let _ = crate::hotkey::ipc::send_ipc_command(
                &socket,
                crate::hotkey::ipc::IpcCommand::Toggle,
            )
            .await;
        });
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{MenuItem, StandardItem, SubMenu};

        let current_state = self.state.read().map(|s| *s).unwrap_or(TrayState::Idle);
        let status_label = match current_state {
            TrayState::Idle => "● Status: Ready (Idle)".to_string(),
            TrayState::Recording => "🔴 Status: Recording...".to_string(),
            TrayState::Transcribing => "⏳ Status: Transcribing...".to_string(),
            TrayState::Degraded => "⚠️ Status: Degraded (Fallback Mic)".to_string(),
            TrayState::Error => "❌ Status: Error".to_string(),
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
                        let _ = crate::hotkey::ipc::send_ipc_command(
                            &s,
                            crate::hotkey::ipc::IpcCommand::Toggle,
                        )
                        .await;
                    });
                }),
                ..Default::default()
            }
            .into(),
        ];

        let history_items = self.history.read().map(|h| h.clone()).unwrap_or_default();
        let mut recent_subitems: Vec<ksni::MenuItem<Self>> = Vec::new();
        for (idx, item) in history_items.iter().take(10).enumerate() {
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

        if !history_items.is_empty() {
            recent_subitems.push(MenuItem::Separator);
        }
        recent_subitems.push(
            StandardItem {
                label: "🔍  Browse Full History...".into(),
                activate: Box::new(|_| {
                    info!("Launching OpenWhisper History GUI from tray menu...");
                    let exe =
                        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("openwhisper"));
                    if let Err(e) = std::process::Command::new(exe)
                        .arg("history")
                        .arg("--gui")
                        .spawn()
                    {
                        warn!("Failed to spawn history GUI: {}", e);
                    }
                }),
                ..Default::default()
            }
            .into(),
        );

        items.push(
            SubMenu {
                label: "📋  Recent Dictations".into(),
                submenu: recent_subitems,
                ..Default::default()
            }
            .into(),
        );

        items.push(
            StandardItem {
                label: "⚙️  Settings / Configuration...".into(),
                activate: Box::new(|_| {
                    info!("Launching OpenWhisper Settings GUI from tray menu...");
                    let exe = std::env::current_exe()
                        .unwrap_or_else(|_| PathBuf::from("/usr/local/bin/openwhisper"));
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
    server_url: Arc<RwLock<String>>,
    last_latency: Arc<RwLock<Option<f32>>>,
    last_error: Arc<RwLock<Option<String>>>,
    active_device: Arc<RwLock<Option<String>>>,
}

impl TrayController {
    #[cfg(target_os = "linux")]
    pub fn new(
        handle: Option<ksni::Handle<OpenWhisperTray>>,
        state: Arc<RwLock<TrayState>>,
        history: Arc<RwLock<VecDeque<String>>>,
        server_url: Arc<RwLock<String>>,
        last_latency: Arc<RwLock<Option<f32>>>,
        last_error: Arc<RwLock<Option<String>>>,
        active_device: Arc<RwLock<Option<String>>>,
    ) -> Self {
        Self {
            handle,
            state,
            history,
            server_url,
            last_latency,
            last_error,
            active_device,
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn new(
        state: Arc<RwLock<TrayState>>,
        history: Arc<RwLock<VecDeque<String>>>,
        server_url: Arc<RwLock<String>>,
        last_latency: Arc<RwLock<Option<f32>>>,
        last_error: Arc<RwLock<Option<String>>>,
        active_device: Arc<RwLock<Option<String>>>,
    ) -> Self {
        Self {
            state,
            history,
            server_url,
            last_latency,
            last_error,
            active_device,
        }
    }

    pub fn set_state(&self, new_state: TrayState) {
        if let Ok(mut lock) = self.state.write() {
            *lock = new_state;
        }
        #[cfg(target_os = "linux")]
        if let Some(ref handle) = self.handle {
            let handle = handle.clone();
            tokio::spawn(async move {
                handle
                    .update(|tray| {
                        if let Ok(mut lock) = tray.state.write() {
                            *lock = new_state;
                        }
                    })
                    .await;
            });
        }
    }

    pub fn set_diagnostics(&self, latency: Option<f32>, error: Option<String>) {
        if let Some(l) = latency
            && let Ok(mut lock) = self.last_latency.write()
        {
            *lock = Some(l);
        }
        if let Ok(mut lock) = self.last_error.write() {
            *lock = error.clone();
        }
        #[cfg(target_os = "linux")]
        if let Some(ref handle) = self.handle {
            let handle = handle.clone();
            tokio::spawn(async move {
                handle
                    .update(|tray| {
                        if let Some(l) = latency
                            && let Ok(mut lock) = tray.last_latency.write()
                        {
                            *lock = Some(l);
                        }
                        if let Ok(mut lock) = tray.last_error.write() {
                            *lock = error;
                        }
                    })
                    .await;
            });
        }
    }

    pub fn set_server_url(&self, url: String) {
        if let Ok(mut lock) = self.server_url.write() {
            *lock = url.clone();
        }
        #[cfg(target_os = "linux")]
        if let Some(ref handle) = self.handle {
            let handle = handle.clone();
            tokio::spawn(async move {
                handle
                    .update(|tray| {
                        if let Ok(mut lock) = tray.server_url.write() {
                            *lock = url;
                        }
                    })
                    .await;
            });
        }
    }

    pub fn set_active_device(&self, dev: Option<String>) {
        if let Ok(mut lock) = self.active_device.write() {
            *lock = dev.clone();
        }
        #[cfg(target_os = "linux")]
        if let Some(ref handle) = self.handle {
            let handle = handle.clone();
            tokio::spawn(async move {
                handle
                    .update(|tray| {
                        if let Ok(mut lock) = tray.active_device.write() {
                            *lock = dev;
                        }
                    })
                    .await;
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
        self.history
            .read()
            .map(|h| h.iter().cloned().collect())
            .unwrap_or_default()
    }
}

/// Spawn the system tray in background if on Linux
pub async fn start_tray_service(
    socket_path: String,
    server_url: String,
) -> (TrayController, Arc<RwLock<TrayState>>) {
    let state = Arc::new(RwLock::new(TrayState::Idle));
    let history = Arc::new(RwLock::new(VecDeque::new()));
    let server_url_arc = Arc::new(RwLock::new(server_url));
    let last_latency = Arc::new(RwLock::new(None));
    let last_error = Arc::new(RwLock::new(None));
    let active_device = Arc::new(RwLock::new(None));

    #[cfg(target_os = "linux")]
    {
        use ksni::TrayMethods;
        let tray = OpenWhisperTray::new(
            state.clone(),
            socket_path,
            history.clone(),
            server_url_arc.clone(),
            last_latency.clone(),
            last_error.clone(),
            active_device.clone(),
        );
        match tray.spawn().await {
            Ok(handle) => {
                info!("System tray (StatusNotifierItem) initialized successfully.");
                (
                    TrayController::new(
                        Some(handle),
                        state.clone(),
                        history,
                        server_url_arc,
                        last_latency,
                        last_error,
                        active_device,
                    ),
                    state,
                )
            }
            Err(err) => {
                warn!(
                    "StatusNotifierWatcher not available or failed to register tray: {err}. Running without system tray."
                );
                (
                    TrayController::new(
                        None,
                        state.clone(),
                        history,
                        server_url_arc,
                        last_latency,
                        last_error,
                        active_device,
                    ),
                    state,
                )
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        (
            TrayController::new(
                state.clone(),
                history,
                server_url_arc,
                last_latency,
                last_error,
                active_device,
            ),
            state,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ksni::Tray;

    #[test]
    fn test_tray_fallback_icon_dimensions() {
        let state = Arc::new(RwLock::new(TrayState::Idle));
        let history = Arc::new(RwLock::new(VecDeque::new()));
        let server_url = Arc::new(RwLock::new("http://localhost:8000".into()));
        let last_latency = Arc::new(RwLock::new(None));
        let last_error = Arc::new(RwLock::new(None));
        let active_device = Arc::new(RwLock::new(None));
        let tray = OpenWhisperTray::new(
            state,
            "/tmp/test.sock".into(),
            history,
            server_url,
            last_latency,
            last_error,
            active_device,
        );

        for s in [
            TrayState::Idle,
            TrayState::Recording,
            TrayState::Transcribing,
            TrayState::Degraded,
            TrayState::Error,
        ] {
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
        let server_url = Arc::new(RwLock::new("http://localhost:8000".into()));
        let last_latency = Arc::new(RwLock::new(None));
        let last_error = Arc::new(RwLock::new(None));
        let active_device = Arc::new(RwLock::new(None));
        let ctrl = TrayController::new(
            None,
            state.clone(),
            history,
            server_url,
            last_latency,
            last_error,
            active_device,
        );

        assert_eq!(*state.read().unwrap(), TrayState::Idle);
        ctrl.set_state(TrayState::Recording);
        assert_eq!(*state.read().unwrap(), TrayState::Recording);
        ctrl.set_state(TrayState::Transcribing);
        assert_eq!(*state.read().unwrap(), TrayState::Transcribing);
        ctrl.set_state(TrayState::Degraded);
        assert_eq!(*state.read().unwrap(), TrayState::Degraded);
        ctrl.set_state(TrayState::Error);
        assert_eq!(*state.read().unwrap(), TrayState::Error);
    }

    #[test]
    fn test_tray_controller_history_ring_buffer() {
        let state = Arc::new(RwLock::new(TrayState::Idle));
        let history = Arc::new(RwLock::new(VecDeque::new()));
        let server_url = Arc::new(RwLock::new("http://localhost:8000".into()));
        let last_latency = Arc::new(RwLock::new(None));
        let last_error = Arc::new(RwLock::new(None));
        let active_device = Arc::new(RwLock::new(None));
        let ctrl = TrayController::new(
            None,
            state,
            history,
            server_url,
            last_latency,
            last_error,
            active_device,
        );

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
        assert_eq!(
            ctrl.history(),
            vec![
                "Second transcription".to_string(),
                "First transcription".to_string(),
            ]
        );

        // Push 11 items to test ring buffer max capacity (10)
        for i in 1..=11 {
            ctrl.add_history(format!("Batch dictation #{}", i));
        }

        let current = ctrl.history();
        assert_eq!(current.len(), 10);
        assert_eq!(current[0], "Batch dictation #11");
        assert_eq!(current[9], "Batch dictation #2");
    }

    #[test]
    fn test_tray_tooltip_contents() {
        let state = Arc::new(RwLock::new(TrayState::Idle));
        let history = Arc::new(RwLock::new(VecDeque::new()));
        let server_url = Arc::new(RwLock::new("https://ovms.example.com/v1".into()));
        let last_latency = Arc::new(RwLock::new(Some(0.42)));
        let last_error = Arc::new(RwLock::new(None));
        let active_device = Arc::new(RwLock::new(Some("USB Condenser Mic".into())));
        let tray = OpenWhisperTray::new(
            state,
            "/tmp/test.sock".into(),
            history,
            server_url,
            last_latency,
            last_error,
            active_device,
        );

        let tooltip = tray.tool_tip();
        assert_eq!(tooltip.title, "OpenWhisper Dictation Assistant");
        assert!(tooltip.description.contains("Ready (Idle)"));
        assert!(tooltip.description.contains("https://ovms.example.com/v1"));
        assert!(tooltip.description.contains("0.42s"));
        assert!(tooltip.description.contains("USB Condenser Mic"));
    }
}
