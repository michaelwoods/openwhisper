use nnnoiseless::DenoiseState;

/// Number of samples per 10ms frame expected by RNNoise at 48,000 Hz.
pub const RNNOISE_FRAME_SIZE: usize = nnnoiseless::FRAME_SIZE; // 480 samples

/// Denoises mono f32 samples using RNNoise neural network model.
///
/// If `source_rate` is not 48,000 Hz, the audio is resampled to 48 kHz,
/// processed in 480-sample (10ms) frames through `DenoiseState`,
/// and then resampled back to `target_rate` (or downsampled to 16 kHz for Whisper).
pub fn denoise_audio_mono(
    mono_samples: &[f32],
    source_rate: u32,
    target_rate: u32,
) -> Vec<f32> {
    if mono_samples.is_empty() {
        return Vec::new();
    }

    // 1. Resample to 48,000 Hz if needed (RNNoise requirement)
    let samples_48k: Vec<f32> = if source_rate == 48_000 {
        mono_samples.to_vec()
    } else {
        crate::audio::resampler::resample_f32_mono(mono_samples, source_rate, 48_000)
    };

    // 2. Process through RNNoise DenoiseState in 480-sample chunks
    let mut denoiser = DenoiseState::new();
    let mut denoised_48k = Vec::with_capacity(samples_48k.len());

    let mut in_frame = [0.0f32; RNNOISE_FRAME_SIZE];
    let mut out_frame = [0.0f32; RNNOISE_FRAME_SIZE];

    for chunk in samples_48k.chunks(RNNOISE_FRAME_SIZE) {
        if chunk.len() == RNNOISE_FRAME_SIZE {
            in_frame.copy_from_slice(chunk);
            denoiser.process_frame(&mut out_frame, &in_frame);
            denoised_48k.extend_from_slice(&out_frame);
        } else {
            // Partial last frame: pad with zero, process, and append only the actual chunk length
            in_frame.fill(0.0);
            in_frame[..chunk.len()].copy_from_slice(chunk);
            denoiser.process_frame(&mut out_frame, &in_frame);
            denoised_48k.extend_from_slice(&out_frame[..chunk.len()]);
        }
    }

    // 3. Resample from 48,000 Hz to target_rate (e.g. 16,000 Hz)
    if target_rate == 48_000 {
        denoised_48k
    } else {
        crate::audio::resampler::resample_f32_mono(&denoised_48k, 48_000, target_rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_denoise_empty_input() {
        let out = denoise_audio_mono(&[], 44100, 16000);
        assert!(out.is_empty());
    }

    #[test]
    fn test_denoise_synthetic_sine_and_noise() {
        // 1 second of 440Hz tone + random low amplitude noise at 44.1kHz
        let sample_count = 44100;
        let mut input = Vec::with_capacity(sample_count);
        for i in 0..sample_count {
            let t = i as f32 / 44100.0;
            let sine = (t * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.2;
            let noise = ((i % 17) as f32 / 17.0 - 0.5) * 0.05;
            input.push(sine + noise);
        }

        let denoised = denoise_audio_mono(&input, 44100, 16000);
        // At 16kHz, 1 second should yield approximately 16,000 samples
        assert!((denoised.len() as i64 - 16000).abs() < 20);

        // Values should remain bounded
        for &s in &denoised {
            assert!(s.abs() <= 1.5, "Sample exceeded bounds: {}", s);
        }
    }
}
