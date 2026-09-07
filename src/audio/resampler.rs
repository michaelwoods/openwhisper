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
    let mono_samples: Vec<f32> = interleaved_samples
        .chunks_exact(channels)
        .map(|frame| {
            let sum: f32 = frame.iter().sum();
            sum / (channels as f32)
        })
        .collect();

    // 2. Resample if source_rate != target_rate
    let resampled: Vec<f32> = if source_rate == target_rate {
        mono_samples
    } else {
        let ratio = source_rate as f64 / target_rate as f64;
        let out_len = ((mono_samples.len() as f64) / ratio).round() as usize;
        let mut output = Vec::with_capacity(out_len);

        for i in 0..out_len {
            let src_idx = (i as f64) * ratio;
            let idx_floor = src_idx.floor() as usize;
            let frac = (src_idx - (idx_floor as f64)) as f32;

            let sample = if idx_floor + 1 < mono_samples.len() {
                let s0 = mono_samples[idx_floor];
                let s1 = mono_samples[idx_floor + 1];
                s0 + frac * (s1 - s0)
            } else if idx_floor < mono_samples.len() {
                mono_samples[idx_floor]
            } else {
                0.0
            };
            output.push(sample);
        }
        output
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
        assert_eq!(out[0], 32767);
        assert_eq!(out[1599], 32767);
    }

    #[test]
    fn test_resample_44100_to_16k() {
        // 44100 Hz down to 16000 Hz
        let input = vec![0.5; 4410]; // 100ms at 44.1kHz
        let out = resample_to_mono_16k(&input, 1, 44100, 16000);
        // (4410 / (44100 / 16000)).round() = 1600
        assert_eq!(out.len(), 1600);
        assert_eq!(out[800], (0.5f32 * 32767.0).round() as i16);
    }

    #[test]
    fn test_resample_8k_to_16k_upsample() {
        // 8000 Hz up to 16000 Hz (1:2 interpolation)
        let input = vec![0.5; 800]; // 100ms at 8kHz
        let out = resample_to_mono_16k(&input, 1, 8000, 16000);
        assert_eq!(out.len(), 1600);
        assert_eq!(out[0], (0.5f32 * 32767.0).round() as i16);
    }

    #[test]
    fn test_resample_clamping_extremes() {
        let input = vec![-3.0, 5.0];
        let out = resample_to_mono_16k(&input, 1, 16000, 16000);
        assert_eq!(out[0], -32767);
        assert_eq!(out[1], 32767);
    }
}
