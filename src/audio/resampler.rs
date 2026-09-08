use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

/// Resamples a single-channel f32 slice from `src_rate` to `dst_rate` using bandlimited sinc/FFT resampling.
///
/// Uses Rubato's FFT resampler with a BlackmanHarris2 anti-aliasing window to eliminate
/// high-frequency Nyquist aliasing. Gracefully falls back to linear interpolation for tiny micro-buffers
/// (< 64 samples) or if resampler construction fails.
pub fn resample_f32_mono(samples: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if samples.is_empty() || src_rate == dst_rate {
        return samples.to_vec();
    }

    // For tiny degenerate buffers (e.g. unit tests with < 64 samples), use linear interpolation
    if samples.len() < 64 {
        return resample_linear(samples, src_rate, dst_rate);
    }

    let input = match InterleavedSlice::new(samples, 1, samples.len()) {
        Ok(s) => s,
        Err(_) => return resample_linear(samples, src_rate, dst_rate),
    };

    let chunk_size = 1024.min(samples.len());
    let mut resampler = match Fft::<f32>::new(
        src_rate as usize,
        dst_rate as usize,
        chunk_size,
        1,
        FixedSync::Both,
    ) {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!(
                "Rubato Fft resampler init failed ({}), falling back to linear",
                e
            );
            return resample_linear(samples, src_rate, dst_rate);
        }
    };

    match resampler.process_all(&input, samples.len(), None) {
        Ok(out) => out.take_data(),
        Err(e) => {
            tracing::debug!("Rubato process_all failed ({}), falling back to linear", e);
            resample_linear(samples, src_rate, dst_rate)
        }
    }
}

/// Fallback linear resampler for arbitrary single-channel f32 buffers
fn resample_linear(samples: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if src_rate == dst_rate || samples.is_empty() {
        return samples.to_vec();
    }

    let ratio = src_rate as f64 / dst_rate as f64;
    let out_len = ((samples.len() as f64) / ratio).round() as usize;
    let mut output = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let src_idx = (i as f64) * ratio;
        let idx_floor = src_idx.floor() as usize;
        let frac = (src_idx - (idx_floor as f64)) as f32;

        let sample = if idx_floor + 1 < samples.len() {
            let s0 = samples[idx_floor];
            let s1 = samples[idx_floor + 1];
            s0 + frac * (s1 - s0)
        } else if idx_floor < samples.len() {
            samples[idx_floor]
        } else {
            0.0
        };
        output.push(sample);
    }
    output
}

/// Resamples multi-channel f32 samples at `source_rate` to mono i16 samples at `target_rate` (16,000 Hz).
pub fn resample_to_mono_16k(
    interleaved_samples: &[f32],
    channels: u16,
    source_rate: u32,
    target_rate: u32,
) -> Vec<i16> {
    if interleaved_samples.is_empty() {
        return Vec::new();
    }

    let channels = channels.max(1) as usize;

    // 1. Downmix interleaved channels to mono
    let mono_samples: Vec<f32> = if channels == 1 {
        interleaved_samples.to_vec()
    } else {
        interleaved_samples
            .chunks_exact(channels)
            .map(|frame| {
                let sum: f32 = frame.iter().sum();
                sum / (channels as f32)
            })
            .collect()
    };

    // 2. Resample if source_rate != target_rate
    let resampled: Vec<f32> = if source_rate == target_rate {
        mono_samples
    } else {
        resample_f32_mono(&mono_samples, source_rate, target_rate)
    };

    // 3. Convert f32 [-1.0, 1.0] to i16 with soft clamping
    resampled
        .into_iter()
        .map(|s| {
            let clamped = s.clamp(-1.0, 1.0);
            (clamped * 32767.0).round() as i16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resample_identity() {
        let input = vec![0.0, 0.5, -0.5, 1.0, -1.0];
        let out = resample_to_mono_16k(&input, 1, 16000, 16000);
        assert_eq!(out.len(), 5);
        assert_eq!(out[0], 0);
        assert_eq!(out[3], 32767);
        assert_eq!(out[4], -32767);
    }

    #[test]
    fn test_resample_empty_input() {
        let out = resample_to_mono_16k(&[], 1, 48000, 16000);
        assert!(out.is_empty());
    }

    #[test]
    fn test_resample_stereo_to_mono() {
        // Stereo: L=0.5, R=0.5 -> mono 0.5
        let input = vec![0.5, 0.5, -0.5, -0.5];
        let out = resample_to_mono_16k(&input, 2, 16000, 16000);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], (0.5f32 * 32767.0).round() as i16);
        assert_eq!(out[1], (-0.5f32 * 32767.0).round() as i16);
    }

    #[test]
    fn test_resample_quad_channel_downmix() {
        // 4 channels: [0.2, 0.4, 0.6, 0.8] -> average 0.5
        let input = vec![0.2, 0.4, 0.6, 0.8];
        let out = resample_to_mono_16k(&input, 4, 16000, 16000);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], (0.5f32 * 32767.0).round() as i16);
    }

    #[test]
    fn test_resample_48k_to_16k() {
        // 48000 Hz down to 16000 Hz (3:1 decimation)
        let input = vec![1.0; 4800]; // 100ms at 48kHz
        let out = resample_to_mono_16k(&input, 1, 48000, 16000);
        assert_eq!(out.len(), 1600); // 100ms at 16kHz
        // Check steady-state middle section (allowing for filter transition at onset/end)
        assert_eq!(out[800], 32767);
    }

    #[test]
    fn test_resample_44100_to_16k() {
        // 44100 Hz down to 16000 Hz
        let input = vec![0.5; 4410]; // 100ms at 44.1kHz
        let out = resample_to_mono_16k(&input, 1, 44100, 16000);
        assert_eq!(out.len(), 1600);
        assert_eq!(out[800], (0.5f32 * 32767.0).round() as i16);
    }

    #[test]
    fn test_resample_8k_to_16k_upsample() {
        // 8000 Hz up to 16000 Hz (1:2 interpolation)
        let input = vec![0.5; 800]; // 100ms at 8kHz
        let out = resample_to_mono_16k(&input, 1, 8000, 16000);
        assert_eq!(out.len(), 1600);
        // Check steady-state middle section (within standard filter ripple tolerance)
        let diff = (out[800] as i32 - (0.5f32 * 32767.0).round() as i32).abs();
        assert!(
            diff < 250,
            "Expected amplitude near 16384, got {} (diff: {})",
            out[800],
            diff
        );
    }

    #[test]
    fn test_anti_aliasing_attenuation() {
        // 12 kHz tone sampled at 48 kHz (above 8 kHz Nyquist limit for 16 kHz target)
        // With bandlimited anti-aliasing filter, 12 kHz must be strongly attenuated when downsampled to 16 kHz.
        let mut input = Vec::with_capacity(4800);
        for i in 0..4800 {
            let t = i as f32 / 48000.0;
            let sine = (t * 12000.0 * 2.0 * std::f32::consts::PI).sin();
            input.push(sine);
        }
        let out = resample_to_mono_16k(&input, 1, 48000, 16000);
        assert_eq!(out.len(), 1600);

        // Compute RMS of the downsampled output in steady-state (frames 200..1400)
        let steady = &out[200..1400];
        let energy: f64 = steady.iter().map(|&s| (s as f64).powi(2)).sum();
        let rms = (energy / steady.len() as f64).sqrt() / 32767.0;

        // An input sine with amplitude 1.0 has RMS ~0.707.
        // The anti-aliasing filter must attenuate it significantly below 0.15.
        assert!(
            rms < 0.15,
            "12kHz tone was not properly attenuated by anti-aliasing filter (RMS: {})",
            rms
        );
    }

    #[test]
    fn test_resample_clamping_extremes() {
        let input = vec![-3.0, 5.0];
        let out = resample_to_mono_16k(&input, 1, 16000, 16000);
        assert_eq!(out[0], -32767);
        assert_eq!(out[1], 32767);
    }
}
