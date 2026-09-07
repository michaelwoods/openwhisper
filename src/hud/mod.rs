pub mod app;

use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::config::HudPosition;

#[derive(Debug, Clone, PartialEq)]
pub enum HudState {
    Idle,
    Recording {
        started_at: Instant,
        rms_energy: f32,
    },
    Transcribing {
        started_at: Instant,
    },
    Completed {
        text_preview: String,
        completed_at: Instant,
    },
    Error {
        message: String,
        error_at: Instant,
    },
}

#[derive(Debug, Clone)]
pub struct HudModel {
    pub state: HudState,
    pub smoothed_rms: f32,
    pub last_rms_update: Instant,
    pub enabled: bool,
    pub position: HudPosition,
}

impl HudModel {
    pub fn new(enabled: bool, position: HudPosition) -> Self {
        Self {
            state: HudState::Idle,
            smoothed_rms: 0.0,
            last_rms_update: Instant::now(),
            enabled,
            position,
        }
    }

    pub fn set_recording(&mut self) {
        self.state = HudState::Recording {
            started_at: Instant::now(),
            rms_energy: 0.0,
        };
        self.smoothed_rms = 0.0;
        self.last_rms_update = Instant::now();
    }

    pub fn update_audio_level(&mut self, rms: f32) {
        let normalized = (rms * 15.0).clamp(0.0, 1.0);
        // Exponential moving average smoothing for fluid visualizer response
        self.smoothed_rms = self.smoothed_rms * 0.4 + normalized * 0.6;
        self.last_rms_update = Instant::now();

        if let HudState::Recording { started_at, .. } = self.state {
            self.state = HudState::Recording {
                started_at,
                rms_energy: self.smoothed_rms,
            };
        }
    }

    pub fn set_transcribing(&mut self) {
        self.state = HudState::Transcribing {
            started_at: Instant::now(),
        };
    }

    pub fn set_completed(&mut self, text: &str) {
        let preview = if text.len() > 38 {
            format!("{}...", &text[..35].trim_end())
        } else {
            text.trim().to_string()
        };

        self.state = HudState::Completed {
            text_preview: preview,
            completed_at: Instant::now(),
        };
    }

    pub fn set_error(&mut self, message: &str) {
        self.state = HudState::Error {
            message: message.to_string(),
            error_at: Instant::now(),
        };
    }

    pub fn set_idle(&mut self) {
        self.state = HudState::Idle;
        self.smoothed_rms = 0.0;
    }

    /// Computes window opacity alpha factor (0.0 to 1.0) with smooth auto-fadeout
    pub fn calculate_alpha(&self, now: Instant) -> f32 {
        if !self.enabled {
            return 0.0;
        }

        match &self.state {
            HudState::Idle => 0.0,
            HudState::Recording { .. } => 1.0,
            HudState::Transcribing { .. } => 1.0,
            HudState::Completed { completed_at, .. } => {
                let elapsed = now.saturating_duration_since(*completed_at).as_secs_f32();
                let hold_duration = 1.2;
                let fade_duration = 0.6;

                if elapsed <= hold_duration {
                    1.0
                } else if elapsed < hold_duration + fade_duration {
                    1.0 - ((elapsed - hold_duration) / fade_duration).clamp(0.0, 1.0)
                } else {
                    0.0
                }
            }
            HudState::Error { error_at, .. } => {
                let elapsed = now.saturating_duration_since(*error_at).as_secs_f32();
                let hold_duration = 2.0;
                let fade_duration = 0.8;

                if elapsed <= hold_duration {
                    1.0
                } else if elapsed < hold_duration + fade_duration {
                    1.0 - ((elapsed - hold_duration) / fade_duration).clamp(0.0, 1.0)
                } else {
                    0.0
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn is_visible(&self, now: Instant) -> bool {
        self.calculate_alpha(now) > 0.005
    }

    #[allow(dead_code)]
    pub fn elapsed_recording_duration(&self, now: Instant) -> Option<Duration> {
        match &self.state {
            HudState::Recording { started_at, .. } => Some(now.saturating_duration_since(*started_at)),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct HudController {
    model: Arc<RwLock<HudModel>>,
}

impl HudController {
    pub fn new(model: HudModel) -> Self {
        Self {
            model: Arc::new(RwLock::new(model)),
        }
    }

    pub fn model(&self) -> Arc<RwLock<HudModel>> {
        Arc::clone(&self.model)
    }

    pub fn set_recording(&self) {
        if let Ok(mut lock) = self.model.write() {
            lock.set_recording();
        }
    }

    pub fn update_audio_level(&self, rms: f32) {
        if let Ok(mut lock) = self.model.write() {
            lock.update_audio_level(rms);
        }
    }

    pub fn set_transcribing(&self) {
        if let Ok(mut lock) = self.model.write() {
            lock.set_transcribing();
        }
    }

    pub fn set_completed(&self, text: &str) {
        if let Ok(mut lock) = self.model.write() {
            lock.set_completed(text);
        }
    }

    pub fn set_error(&self, message: &str) {
        if let Ok(mut lock) = self.model.write() {
            lock.set_error(message);
        }
    }

    pub fn set_idle(&self) {
        if let Ok(mut lock) = self.model.write() {
            lock.set_idle();
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        if let Ok(mut lock) = self.model.write() {
            lock.enabled = enabled;
        }
    }

    pub fn set_position(&self, position: HudPosition) {
        if let Ok(mut lock) = self.model.write() {
            lock.position = position;
        }
    }
}

/// Starts the floating HUD in a dedicated background window thread if graphical session is available.
pub fn start_hud_service(enabled: bool, position: HudPosition) -> HudController {
    let model = HudModel::new(enabled, position);
    let controller = HudController::new(model);

    if !enabled {
        tracing::info!("Floating HUD overlay disabled in configuration.");
        return controller;
    }

    let has_display = std::env::var("WAYLAND_DISPLAY").is_ok() || std::env::var("DISPLAY").is_ok();
    if !has_display {
        tracing::info!("No graphical display server found (WAYLAND_DISPLAY / DISPLAY unset). Running without HUD overlay.");
        return controller;
    }

    let ctrl_clone = controller.clone();
    std::thread::Builder::new()
        .name("openwhisper-hud".into())
        .spawn(move || {
            if let Err(err) = app::run_hud_window(ctrl_clone) {
                tracing::warn!("HUD window service ended: {err}");
            }
        })
        .expect("Failed to spawn HUD thread");

    tracing::info!("Floating HUD overlay initialized.");
    controller
}

/// Interactive standalone demo cycling through HUD states
pub fn run_hud_demo() -> anyhow::Result<()> {
    let controller = HudController::new(HudModel::new(true, HudPosition::BottomCenter));
    let ctrl_demo = controller.clone();

    std::thread::spawn(move || {
        loop {
            // 1. Recording state for 4 seconds with dynamic audio levels
            ctrl_demo.set_recording();
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(4) {
                let t = start.elapsed().as_secs_f32();
                let rms = ((t * 4.0).sin() * 0.5 + 0.5) * 0.08 + 0.01;
                ctrl_demo.update_audio_level(rms);
                std::thread::sleep(Duration::from_millis(50));
            }

            // 2. Transcribing state for 1.8 seconds
            ctrl_demo.set_transcribing();
            std::thread::sleep(Duration::from_millis(1800));

            // 3. Completed state
            ctrl_demo.set_completed("Dictating effortlessly with OpenWhisper HUD overlay");
            std::thread::sleep(Duration::from_millis(2200));

            // 4. Idle state
            ctrl_demo.set_idle();
            std::thread::sleep(Duration::from_millis(1500));
        }
    });

    println!("🎙️  Running OpenWhisper Floating HUD Demo (press Ctrl+C or close window to exit)...");
    app::run_hud_window(controller)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hud_state_transitions() {
        let mut model = HudModel::new(true, HudPosition::BottomCenter);
        assert_eq!(model.state, HudState::Idle);
        assert!(!model.is_visible(Instant::now()));

        model.set_recording();
        assert!(matches!(model.state, HudState::Recording { .. }));
        assert!(model.is_visible(Instant::now()));
        assert_eq!(model.calculate_alpha(Instant::now()), 1.0);

        model.update_audio_level(0.05);
        assert!(model.smoothed_rms > 0.0);

        model.set_transcribing();
        assert!(matches!(model.state, HudState::Transcribing { .. }));
        assert_eq!(model.calculate_alpha(Instant::now()), 1.0);

        model.set_completed("hello world from whisper dictation assistant test");
        if let HudState::Completed { ref text_preview, .. } = model.state {
            assert!(text_preview.contains("hello world"));
        } else {
            panic!("Expected Completed state");
        }

        model.set_idle();
        assert_eq!(model.state, HudState::Idle);
        assert!(!model.is_visible(Instant::now()));
    }

    #[test]
    fn test_hud_audio_meter_smoothing() {
        let mut model = HudModel::new(true, HudPosition::BottomCenter);
        model.set_recording();

        model.update_audio_level(0.0);
        assert_eq!(model.smoothed_rms, 0.0);

        model.update_audio_level(0.1);
        let first_rms = model.smoothed_rms;
        assert!(first_rms > 0.0 && first_rms <= 1.0);

        // Clamped bounds test
        model.update_audio_level(10.0);
        assert!(model.smoothed_rms <= 1.0);
    }

    #[test]
    fn test_hud_fadeout_alpha() {
        let mut model = HudModel::new(true, HudPosition::BottomCenter);
        let t0 = Instant::now();

        model.set_completed("Test text");
        if let HudState::Completed { ref mut completed_at, .. } = model.state {
            *completed_at = t0;
        }

        // At t0: alpha is 1.0
        assert_eq!(model.calculate_alpha(t0), 1.0);

        // At t0 + 1.0s: still in hold period (1.2s)
        assert_eq!(model.calculate_alpha(t0 + Duration::from_secs(1)), 1.0);

        // At t0 + 1.5s: mid fadeout (between 1.2 and 1.8)
        let alpha_mid = model.calculate_alpha(t0 + Duration::from_millis(1500));
        assert!(alpha_mid > 0.0 && alpha_mid < 1.0);

        // At t0 + 2.0s: completely faded out
        let alpha_end = model.calculate_alpha(t0 + Duration::from_millis(2000));
        assert_eq!(alpha_end, 0.0);
        assert!(!model.is_visible(t0 + Duration::from_millis(2000)));
    }

    #[test]
    fn test_hud_disabled_alpha() {
        let mut model = HudModel::new(false, HudPosition::BottomCenter);
        model.set_recording();
        assert_eq!(model.calculate_alpha(Instant::now()), 0.0);
        assert!(!model.is_visible(Instant::now()));
    }

    #[test]
    fn test_hud_error_fadeout_alpha() {
        let mut model = HudModel::new(true, HudPosition::BottomCenter);
        let t0 = Instant::now();

        model.set_error("Connection timeout");
        if let HudState::Error { ref mut error_at, ref message } = model.state {
            assert_eq!(message, "Connection timeout");
            *error_at = t0;
        }

        // At t0: alpha is 1.0
        assert_eq!(model.calculate_alpha(t0), 1.0);

        // At t0 + 1.5s: still holding (hold is 2.0s)
        assert_eq!(model.calculate_alpha(t0 + Duration::from_millis(1500)), 1.0);

        // At t0 + 2.4s: mid fadeout (fade is between 2.0 and 2.8s)
        let alpha_mid = model.calculate_alpha(t0 + Duration::from_millis(2400));
        assert!(alpha_mid > 0.0 && alpha_mid < 1.0);

        // At t0 + 3.0s: fully hidden
        let alpha_end = model.calculate_alpha(t0 + Duration::from_millis(3000));
        assert_eq!(alpha_end, 0.0);
        assert!(!model.is_visible(t0 + Duration::from_millis(3000)));
    }

    #[test]
    fn test_hud_elapsed_recording_duration() {
        let mut model = HudModel::new(true, HudPosition::BottomCenter);
        assert!(model.elapsed_recording_duration(Instant::now()).is_none());

        let t0 = Instant::now();
        model.set_recording();
        if let HudState::Recording { ref mut started_at, .. } = model.state {
            *started_at = t0;
        }

        let elapsed = model.elapsed_recording_duration(t0 + Duration::from_secs(3)).unwrap();
        assert_eq!(elapsed.as_secs(), 3);
    }

    #[test]
    fn test_hud_controller_methods() {
        let controller = HudController::new(HudModel::new(true, HudPosition::TopCenter));
        controller.set_recording();
        {
            let lock = controller.model();
            let model = lock.read().unwrap();
            assert!(matches!(model.state, HudState::Recording { .. }));
        }

        controller.update_audio_level(0.08);
        controller.set_transcribing();
        {
            let lock = controller.model();
            let model = lock.read().unwrap();
            assert!(matches!(model.state, HudState::Transcribing { .. }));
        }

        controller.set_completed("Test controller");
        controller.set_error("Test controller error");
        controller.set_idle();
        controller.set_enabled(false);
        controller.set_position(HudPosition::TopRight);

        {
            let lock = controller.model();
            let model = lock.read().unwrap();
            assert_eq!(model.state, HudState::Idle);
            assert!(!model.enabled);
            assert_eq!(model.position, HudPosition::TopRight);
        }
    }
}
