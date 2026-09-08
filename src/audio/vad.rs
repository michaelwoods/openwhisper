use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct VadConfig {
    pub enabled: bool,
    pub silence_timeout: Duration,
    pub energy_threshold: f32,
    pub min_speech_duration: Duration,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            silence_timeout: Duration::from_millis(1800),
            energy_threshold: 0.015,
            min_speech_duration: Duration::from_millis(300),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadDecision {
    Continue,
    SilenceTimeout,
}

pub struct VadDetector {
    config: VadConfig,
    speech_detected: bool,
    speech_started_at: Option<Instant>,
    last_speech_time: Option<Instant>,
    #[allow(dead_code)]
    recording_started_at: Instant,
}

impl VadDetector {
    pub fn new(config: VadConfig) -> Self {
        let now = Instant::now();
        Self {
            config,
            speech_detected: false,
            speech_started_at: None,
            last_speech_time: None,
            recording_started_at: now,
        }
    }

    #[allow(dead_code)]
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    #[allow(dead_code)]
    pub fn has_detected_speech(&self) -> bool {
        self.speech_detected
    }

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        let now = Instant::now();
        self.speech_detected = false;
        self.speech_started_at = None;
        self.last_speech_time = None;
        self.recording_started_at = now;
    }

    /// Computes Root Mean Square (RMS) energy of a buffer of f32 samples.
    pub fn calculate_rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
        (sum_sq / samples.len() as f32).sqrt()
    }

    /// Evaluates a chunk of audio samples and determines if silence timeout was reached.
    pub fn process_chunk(&mut self, samples: &[f32], now: Instant) -> VadDecision {
        if !self.config.enabled || samples.is_empty() {
            return VadDecision::Continue;
        }

        let energy = Self::calculate_rms(samples);

        if energy >= self.config.energy_threshold {
            if !self.speech_detected {
                self.speech_detected = true;
                self.speech_started_at = Some(now);
            }
            self.last_speech_time = Some(now);
            VadDecision::Continue
        } else {
            // Silence chunk
            if self.speech_detected {
                let speech_dur = self
                    .speech_started_at
                    .map(|t| now.duration_since(t))
                    .unwrap_or_default();

                if speech_dur >= self.config.min_speech_duration
                    && let Some(last_speech) = self.last_speech_time
                    && now.duration_since(last_speech) >= self.config.silence_timeout
                {
                    return VadDecision::SilenceTimeout;
                }
            }
            VadDecision::Continue
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_rms() {
        assert_eq!(VadDetector::calculate_rms(&[]), 0.0);
        let silence = vec![0.0; 100];
        assert_eq!(VadDetector::calculate_rms(&silence), 0.0);

        let constant = vec![0.5; 100];
        let rms = VadDetector::calculate_rms(&constant);
        assert!((rms - 0.5).abs() < 1e-5);
    }

    #[test]
    fn test_vad_disabled() {
        let config = VadConfig {
            enabled: false,
            silence_timeout: Duration::from_millis(100),
            energy_threshold: 0.02,
            min_speech_duration: Duration::from_millis(50),
        };
        let mut detector = VadDetector::new(config);
        let now = Instant::now();
        let samples = vec![0.0; 100];
        assert_eq!(
            detector.process_chunk(&samples, now + Duration::from_secs(10)),
            VadDecision::Continue
        );
    }

    #[test]
    fn test_vad_speech_then_silence_timeout() {
        let config = VadConfig {
            enabled: true,
            silence_timeout: Duration::from_millis(200),
            energy_threshold: 0.02,
            min_speech_duration: Duration::from_millis(50),
        };
        let mut detector = VadDetector::new(config);
        let t0 = Instant::now();

        // 1. Initial silence: Should continue
        let silence = vec![0.0; 160];
        assert_eq!(detector.process_chunk(&silence, t0), VadDecision::Continue);
        assert!(!detector.has_detected_speech());

        // 2. Speech occurs
        let speech = vec![0.1; 160];
        let t1 = t0 + Duration::from_millis(60);
        assert_eq!(detector.process_chunk(&speech, t1), VadDecision::Continue);
        assert!(detector.has_detected_speech());

        // 3. Silence begins, under timeout
        let t2 = t1 + Duration::from_millis(100);
        assert_eq!(detector.process_chunk(&silence, t2), VadDecision::Continue);

        // 4. Silence continues past 200ms timeout
        let t3 = t1 + Duration::from_millis(210);
        assert_eq!(
            detector.process_chunk(&silence, t3),
            VadDecision::SilenceTimeout
        );
    }

    #[test]
    fn test_vad_reset() {
        let config = VadConfig {
            enabled: true,
            ..Default::default()
        };
        let mut detector = VadDetector::new(config);
        let t0 = Instant::now();

        let speech = vec![0.1; 160];
        detector.process_chunk(&speech, t0);
        assert!(detector.has_detected_speech());

        detector.reset();
        assert!(!detector.has_detected_speech());
    }
}
