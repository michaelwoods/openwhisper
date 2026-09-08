use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EarconType {
    RecordingStarted,
    RecordingStopped,
    Transcribed,
    Cancel,
    Error,
}

#[derive(Debug, Clone)]
pub struct SoundPlayer {
    enabled: bool,
    volume: f32,
}

impl SoundPlayer {
    pub fn new(enabled: bool, volume: f32) -> Self {
        Self {
            enabled,
            volume: volume.clamp(0.0, 1.0),
        }
    }

    #[allow(dead_code)]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    #[allow(dead_code)]
    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn play(&self, earcon: EarconType) {
        if !self.enabled || self.volume <= 0.0 {
            return;
        }

        let volume = self.volume;
        thread::spawn(move || {
            if let Err(err) = play_earcon_internal(earcon, volume) {
                tracing::debug!("Audio feedback playback skipped/failed: {err}");
            }
        });
    }
}

fn play_earcon_internal(earcon: EarconType, volume: f32) -> anyhow::Result<()> {
    let host = cpal::default_host();
    let device = match host.default_output_device() {
        Some(dev) => dev,
        None => {
            tracing::debug!("No default audio output device found for earcon playback");
            return Ok(());
        }
    };

    let default_config = device.default_output_config()?;
    let sample_rate = default_config.sample_rate().0;
    let channels = default_config.channels() as usize;
    let sample_format = default_config.sample_format();

    let samples = Arc::new(generate_earcon_samples(earcon, sample_rate, volume));
    let duration_ms = (samples.len() as f32 / sample_rate as f32 * 1000.0) as u64;

    let pos = Arc::new(AtomicUsize::new(0));
    let is_done = Arc::new(AtomicBool::new(false));

    let samples_cb = Arc::clone(&samples);
    let pos_cb = Arc::clone(&pos);
    let is_done_cb = Arc::clone(&is_done);

    let err_fn = |err| {
        tracing::debug!("Audio feedback stream error: {err}");
    };

    let stream_config: StreamConfig = default_config.into();

    let stream = match sample_format {
        SampleFormat::F32 => device.build_output_stream(
            &stream_config,
            move |data: &mut [f32], _: &_| {
                let mut p = pos_cb.load(Ordering::Relaxed);
                for frame in data.chunks_mut(channels) {
                    let sample = if p < samples_cb.len() {
                        let s = samples_cb[p];
                        p += 1;
                        s
                    } else {
                        is_done_cb.store(true, Ordering::Relaxed);
                        0.0
                    };
                    for out in frame.iter_mut() {
                        *out = sample;
                    }
                }
                pos_cb.store(p, Ordering::Relaxed);
            },
            err_fn,
            None,
        )?,
        SampleFormat::I16 => device.build_output_stream(
            &stream_config,
            move |data: &mut [i16], _: &_| {
                let mut p = pos_cb.load(Ordering::Relaxed);
                for frame in data.chunks_mut(channels) {
                    let sample = if p < samples_cb.len() {
                        let s = samples_cb[p];
                        p += 1;
                        (s * 32767.0).clamp(-32768.0, 32767.0) as i16
                    } else {
                        is_done_cb.store(true, Ordering::Relaxed);
                        0
                    };
                    for out in frame.iter_mut() {
                        *out = sample;
                    }
                }
                pos_cb.store(p, Ordering::Relaxed);
            },
            err_fn,
            None,
        )?,
        SampleFormat::U16 => device.build_output_stream(
            &stream_config,
            move |data: &mut [u16], _: &_| {
                let mut p = pos_cb.load(Ordering::Relaxed);
                for frame in data.chunks_mut(channels) {
                    let sample = if p < samples_cb.len() {
                        let s = samples_cb[p];
                        p += 1;
                        ((s * 32767.0).clamp(-32768.0, 32767.0) as i32 + 32768) as u16
                    } else {
                        is_done_cb.store(true, Ordering::Relaxed);
                        32768
                    };
                    for out in frame.iter_mut() {
                        *out = sample;
                    }
                }
                pos_cb.store(p, Ordering::Relaxed);
            },
            err_fn,
            None,
        )?,
        other => anyhow::bail!("Unsupported output sample format: {:?}", other),
    };

    stream.play()?;

    // Sleep until sound finishes + small margin
    thread::sleep(Duration::from_millis(duration_ms + 40));
    drop(stream);

    Ok(())
}

/// Plays a recorded WAV audio file, polling for cancellation via `stop_flag`.
/// Returns Ok(true) if played to completion, or Ok(false) if stopped/cancelled.
/// Tries standard desktop PipeWire / ALSA audio utilities (pw-play, paplay, aplay) first,
/// falling back to cross-platform native decoding via hound + cpal.
pub fn play_wav_file_cancellable(
    path: &std::path::Path,
    stop_flag: Arc<AtomicBool>,
) -> anyhow::Result<bool> {
    if !path.exists() {
        anyhow::bail!("Audio file does not exist: {:?}", path);
    }

    for cmd in ["pw-play", "paplay", "aplay"] {
        if let Ok(mut child) = std::process::Command::new(cmd)
            .arg(path)
            .spawn()
        {
            while !stop_flag.load(Ordering::Relaxed) {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        return Ok(status.success());
                    }
                    Ok(None) => {
                        thread::sleep(Duration::from_millis(40));
                    }
                    Err(e) => {
                        tracing::warn!("Error polling audio player process {cmd}: {e}");
                        break;
                    }
                }
            }

            if stop_flag.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(false);
            }
        }
    }

    // Cross-platform native decoding fallback via hound + cpal
    play_wav_native_cancellable(path, stop_flag)
}

/// Synchronously plays a recorded WAV audio file, blocking until playback completes.
pub fn play_wav_file_blocking(path: &std::path::Path) -> anyhow::Result<()> {
    let dummy_flag = Arc::new(AtomicBool::new(false));
    let _ = play_wav_file_cancellable(path, dummy_flag)?;
    Ok(())
}

/// Asynchronously plays a recorded WAV audio file in a background thread.
#[allow(dead_code)]
pub fn play_wav_file(path: &std::path::Path) -> anyhow::Result<()> {
    if !path.exists() {
        anyhow::bail!("Audio file does not exist: {:?}", path);
    }

    let path_buf = path.to_path_buf();
    thread::spawn(move || {
        if let Err(err) = play_wav_file_blocking(&path_buf) {
            tracing::error!("Audio playback failed: {err}");
        }
    });

    Ok(())
}

fn play_wav_native_cancellable(
    path: &std::path::Path,
    stop_flag: Arc<AtomicBool>,
) -> anyhow::Result<bool> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let wav_samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max_val = (1 << (spec.bits_per_sample.saturating_sub(1))) as f32;
            reader
                .samples::<i32>()
                .filter_map(|s| s.ok())
                .map(|s| s as f32 / max_val)
                .collect()
        }
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(|s| s.ok()).collect(),
    };

    if wav_samples.is_empty() {
        return Ok(true);
    }

    let host = cpal::default_host();
    let device = match host.default_output_device() {
        Some(d) => d,
        None => anyhow::bail!("No default audio output device found for playback"),
    };

    let default_config = device.default_output_config()?;
    let dev_sample_rate = default_config.sample_rate().0;
    let dev_channels = default_config.channels() as usize;

    let resampled = if spec.sample_rate != dev_sample_rate {
        let ratio = spec.sample_rate as f64 / dev_sample_rate as f64;
        let out_len = ((wav_samples.len() as f64) / ratio).round() as usize;
        let mut out = Vec::with_capacity(out_len);
        for i in 0..out_len {
            let src_idx = (i as f64) * ratio;
            let idx_floor = src_idx.floor() as usize;
            let frac = (src_idx - (idx_floor as f64)) as f32;
            let s = if idx_floor + 1 < wav_samples.len() {
                wav_samples[idx_floor] + frac * (wav_samples[idx_floor + 1] - wav_samples[idx_floor])
            } else if idx_floor < wav_samples.len() {
                wav_samples[idx_floor]
            } else {
                0.0
            };
            out.push(s);
        }
        out
    } else {
        wav_samples
    };

    let samples = Arc::new(resampled);
    let duration_ms = (samples.len() as f32 / dev_sample_rate as f32 * 1000.0) as u64;
    let pos = Arc::new(AtomicUsize::new(0));
    let is_done = Arc::new(AtomicBool::new(false));

    let samples_cb = Arc::clone(&samples);
    let pos_cb = Arc::clone(&pos);
    let is_done_cb = Arc::clone(&is_done);

    let stream_config: StreamConfig = default_config.into();
    let stream = device.build_output_stream(
        &stream_config,
        move |data: &mut [f32], _: &_| {
            let mut p = pos_cb.load(Ordering::Relaxed);
            for frame in data.chunks_mut(dev_channels) {
                let sample = if p < samples_cb.len() {
                    let s = samples_cb[p];
                    p += 1;
                    s
                } else {
                    is_done_cb.store(true, Ordering::Relaxed);
                    0.0
                };
                for out in frame.iter_mut() {
                    *out = sample;
                }
            }
            pos_cb.store(p, Ordering::Relaxed);
        },
        |err| tracing::debug!("Audio playback stream error: {err}"),
        None,
    )?;

    stream.play()?;

    let start = Instant::now();
    let max_dur = Duration::from_millis(duration_ms + 80);
    while start.elapsed() < max_dur
        && !stop_flag.load(Ordering::Relaxed)
        && !is_done.load(Ordering::Relaxed)
    {
        thread::sleep(Duration::from_millis(40));
    }

    drop(stream);
    Ok(!stop_flag.load(Ordering::Relaxed))
}

/// Generates mono floating-point audio samples for a specific earcon.
pub fn generate_earcon_samples(earcon: EarconType, sample_rate: u32, volume: f32) -> Vec<f32> {
    let volume = volume.clamp(0.0, 1.0);
    match earcon {
        // Rising two-tone chime (440 Hz -> 880 Hz, 30ms each, total 60ms)
        EarconType::RecordingStarted => {
            let tone1 = generate_sine_tone(440.0, 0.030, sample_rate, volume);
            let tone2 = generate_sine_tone(880.0, 0.030, sample_rate, volume);
            let mut out = tone1;
            out.extend_from_slice(&tone2);
            out
        }
        // Gentle descending sweep (660 Hz -> 440 Hz, 50ms)
        EarconType::RecordingStopped => {
            generate_frequency_sweep(660.0, 440.0, 0.050, sample_rate, volume)
        }
        // Confirmation blip (1000 Hz, 35ms)
        EarconType::Transcribed => {
            generate_sine_tone(1000.0, 0.035, sample_rate, volume)
        }
        // Gentle descending cancel chime (440 Hz -> 260 Hz, 35ms each)
        EarconType::Cancel => {
            let tone1 = generate_sine_tone(440.0, 0.035, sample_rate, volume * 0.9);
            let tone2 = generate_sine_tone(260.0, 0.045, sample_rate, volume * 0.85);
            let mut out = tone1;
            out.extend_from_slice(&tone2);
            out
        }
        // Double low buzz (220 Hz -> 180 Hz, 40ms each with 20ms pause)
        EarconType::Error => {
            let tone1 = generate_sine_tone(220.0, 0.040, sample_rate, volume);
            let pause_len = (sample_rate as f32 * 0.020) as usize;
            let pause = vec![0.0; pause_len];
            let tone2 = generate_sine_tone(180.0, 0.040, sample_rate, volume);
            let mut out = tone1;
            out.extend_from_slice(&pause);
            out.extend_from_slice(&tone2);
            out
        }
    }
}

/// Generates a constant frequency sine wave with smooth attack and decay envelopes.
fn generate_sine_tone(freq: f32, duration_secs: f32, sample_rate: u32, volume: f32) -> Vec<f32> {
    let total_samples = (sample_rate as f32 * duration_secs).round() as usize;
    if total_samples == 0 {
        return Vec::new();
    }

    let mut samples = Vec::with_capacity(total_samples);
    let attack_len = (total_samples as f32 * 0.15).max(1.0) as usize;
    let decay_len = (total_samples as f32 * 0.20).max(1.0) as usize;

    for i in 0..total_samples {
        let t = i as f32 / sample_rate as f32;
        let wave = (2.0 * PI * freq * t).sin();

        // Calculate envelope multiplier [0.0, 1.0]
        let env = if i < attack_len {
            i as f32 / attack_len as f32
        } else if i > total_samples - decay_len {
            (total_samples - i) as f32 / decay_len as f32
        } else {
            1.0
        };

        samples.push(wave * env * volume);
    }
    samples
}

/// Generates a frequency sweep from start_freq to end_freq with envelope.
fn generate_frequency_sweep(
    start_freq: f32,
    end_freq: f32,
    duration_secs: f32,
    sample_rate: u32,
    volume: f32,
) -> Vec<f32> {
    let total_samples = (sample_rate as f32 * duration_secs).round() as usize;
    if total_samples == 0 {
        return Vec::new();
    }

    let mut samples = Vec::with_capacity(total_samples);
    let attack_len = (total_samples as f32 * 0.15).max(1.0) as usize;
    let decay_len = (total_samples as f32 * 0.20).max(1.0) as usize;

    let mut phase = 0.0f32;
    for i in 0..total_samples {
        let progress = i as f32 / total_samples as f32;
        let current_freq = start_freq + progress * (end_freq - start_freq);
        phase += 2.0 * PI * current_freq / sample_rate as f32;

        let wave = phase.sin();

        let env = if i < attack_len {
            i as f32 / attack_len as f32
        } else if i > total_samples - decay_len {
            (total_samples - i) as f32 / decay_len as f32
        } else {
            1.0
        };

        samples.push(wave * env * volume);
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_samples_non_empty_and_bounded() {
        let sample_rate = 44100;
        let volume = 0.5;

        for earcon in [
            EarconType::RecordingStarted,
            EarconType::RecordingStopped,
            EarconType::Transcribed,
            EarconType::Cancel,
            EarconType::Error,
        ] {
            let samples = generate_earcon_samples(earcon, sample_rate, volume);
            assert!(!samples.is_empty(), "Earcon {:?} generated 0 samples", earcon);

            for &s in &samples {
                assert!(
                    s >= -volume - 0.001 && s <= volume + 0.001,
                    "Sample out of bounds: {} for volume {}",
                    s,
                    volume
                );
            }
        }
    }

    #[test]
    fn test_generate_samples_zero_volume() {
        let samples = generate_earcon_samples(EarconType::Transcribed, 44100, 0.0);
        assert!(!samples.is_empty());
        for &s in &samples {
            assert_eq!(s, 0.0);
        }
    }

    #[test]
    fn test_envelope_smooth_attack_and_decay() {
        let samples = generate_earcon_samples(EarconType::Transcribed, 44100, 1.0);
        // First sample starts at 0.0 due to attack ramp
        assert_eq!(samples[0], 0.0);
        // Last sample ends close to 0.0 due to decay ramp
        let last = *samples.last().unwrap();
        assert!(last.abs() < 0.15, "Last sample was not decayed: {}", last);
    }

    #[test]
    fn test_sound_player_state() {
        let player = SoundPlayer::new(true, 0.8);
        assert!(player.is_enabled());
        assert_eq!(player.volume(), 0.8);

        let clamped_player = SoundPlayer::new(false, 2.5);
        assert!(!clamped_player.is_enabled());
        assert_eq!(clamped_player.volume(), 1.0);
    }
}
