use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    StartRecording,
    StopAndTranscribe,
    CancelRecording,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeState {
    Idle,
    HoldingPress,
    ActiveToggle,
    Transcribing,
}

pub struct HotkeyEngine {
    state: ModeState,
    started_at: Option<Instant>,
    ptt_threshold: Duration,
}

impl HotkeyEngine {
    pub fn new(ptt_threshold_ms: u64) -> Self {
        Self {
            state: ModeState::Idle,
            started_at: None,
            ptt_threshold: Duration::from_millis(ptt_threshold_ms),
        }
    }

    pub fn current_state(&self) -> ModeState {
        self.state
    }

    pub fn is_recording(&self) -> bool {
        matches!(self.state, ModeState::HoldingPress | ModeState::ActiveToggle)
    }

    /// Handles a key press / key down event
    pub fn on_press(&mut self) -> Action {
        match self.state {
            ModeState::Idle => {
                let now = Instant::now();
                self.state = ModeState::HoldingPress;
                self.started_at = Some(now);
                Action::StartRecording
            }
            ModeState::ActiveToggle => {
                // Second press while in toggle mode stops recording
                self.state = ModeState::Transcribing;
                self.started_at = None;
                Action::StopAndTranscribe
            }
            ModeState::HoldingPress => {
                // Key repeat event from OS, ignore
                Action::None
            }
            ModeState::Transcribing => {
                // Busy transcribing
                Action::None
            }
        }
    }

    /// Handles a key release / key up event
    pub fn on_release(&mut self) -> Action {
        match self.state {
            ModeState::HoldingPress => {
                let started = self.started_at.unwrap_or_else(Instant::now);
                let elapsed = started.elapsed();

                if elapsed >= self.ptt_threshold {
                    // Held longer than threshold: Push-To-Talk completed!
                    self.state = ModeState::Transcribing;
                    self.started_at = None;
                    Action::StopAndTranscribe
                } else {
                    // Brief press: Transition to hands-free Toggle mode!
                    self.state = ModeState::ActiveToggle;
                    Action::None
                }
            }
            ModeState::ActiveToggle => {
                // Key released after second tap, remain in current mode
                Action::None
            }
            ModeState::Idle | ModeState::Transcribing => Action::None,
        }
    }

    /// Explicit toggle command (e.g. from CLI or button)
    pub fn on_toggle(&mut self) -> Action {
        match self.state {
            ModeState::Idle => {
                self.state = ModeState::ActiveToggle;
                self.started_at = Some(Instant::now());
                Action::StartRecording
            }
            ModeState::HoldingPress | ModeState::ActiveToggle => {
                self.state = ModeState::Transcribing;
                self.started_at = None;
                Action::StopAndTranscribe
            }
            ModeState::Transcribing => Action::None,
        }
    }

    /// Cancels any in-progress recording
    pub fn on_cancel(&mut self) -> Action {
        if self.is_recording() {
            self.state = ModeState::Idle;
            self.started_at = None;
            Action::CancelRecording
        } else {
            Action::None
        }
    }

    /// Marks transcription finished, resetting to Idle
    pub fn on_transcription_finished(&mut self) {
        self.state = ModeState::Idle;
        self.started_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_ptt_hold_behavior() {
        let mut engine = HotkeyEngine::new(100);
        assert_eq!(engine.on_press(), Action::StartRecording);
        assert_eq!(engine.current_state(), ModeState::HoldingPress);

        // Hold for longer than threshold
        sleep(Duration::from_millis(150));

        assert_eq!(engine.on_release(), Action::StopAndTranscribe);
        assert_eq!(engine.current_state(), ModeState::Transcribing);
    }

    #[test]
    fn test_toggle_tap_behavior() {
        let mut engine = HotkeyEngine::new(200);
        // Tap down
        assert_eq!(engine.on_press(), Action::StartRecording);

        // Fast release (tap)
        assert_eq!(engine.on_release(), Action::None);
        assert_eq!(engine.current_state(), ModeState::ActiveToggle);

        // Still recording hands-free...
        sleep(Duration::from_millis(50));
        assert!(engine.is_recording());

        // Tap again to finish
        assert_eq!(engine.on_press(), Action::StopAndTranscribe);
        assert_eq!(engine.current_state(), ModeState::Transcribing);
    }

    #[test]
    fn test_cancel_recording() {
        let mut engine = HotkeyEngine::new(200);
        assert_eq!(engine.on_press(), Action::StartRecording);
        assert!(engine.is_recording());

        assert_eq!(engine.on_cancel(), Action::CancelRecording);
        assert_eq!(engine.current_state(), ModeState::Idle);
        assert!(!engine.is_recording());

        // Second cancel when idle should be None
        assert_eq!(engine.on_cancel(), Action::None);
    }

    #[test]
    fn test_explicit_toggle() {
        let mut engine = HotkeyEngine::new(200);
        assert_eq!(engine.on_toggle(), Action::StartRecording);
        assert_eq!(engine.current_state(), ModeState::ActiveToggle);
        assert!(engine.is_recording());

        assert_eq!(engine.on_toggle(), Action::StopAndTranscribe);
        assert_eq!(engine.current_state(), ModeState::Transcribing);
    }

    #[test]
    fn test_key_repeat_suppression() {
        let mut engine = HotkeyEngine::new(300);
        assert_eq!(engine.on_press(), Action::StartRecording);

        // Repeated presses while holding should produce Action::None
        assert_eq!(engine.on_press(), Action::None);
        assert_eq!(engine.on_press(), Action::None);
        assert_eq!(engine.current_state(), ModeState::HoldingPress);
    }

    #[test]
    fn test_transcription_finished_resets_idle() {
        let mut engine = HotkeyEngine::new(100);
        engine.on_press();
        sleep(Duration::from_millis(120));
        engine.on_release();
        assert_eq!(engine.current_state(), ModeState::Transcribing);

        engine.on_transcription_finished();
        assert_eq!(engine.current_state(), ModeState::Idle);
        assert!(!engine.is_recording());
    }

    #[test]
    fn test_release_when_idle() {
        let mut engine = HotkeyEngine::new(100);
        assert_eq!(engine.on_release(), Action::None);
        assert_eq!(engine.current_state(), ModeState::Idle);
    }
}
