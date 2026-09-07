use anyhow::{bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream};
use hound::{SampleFormat as WavSampleFormat, WavSpec, WavWriter};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::resampler::resample_to_mono_16k;

pub struct AudioRecorder {
    device_name: Option<String>,
}

pub struct ActiveRecording {
    stream: Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    channels: u16,
    sample_rate: u32,
    is_recording: Arc<AtomicBool>,
    #[allow(dead_code)]
    started_at: Instant,
}

impl AudioRecorder {
    pub fn new(device_name: Option<String>) -> Self {
        Self { device_name }
    }

    pub fn list_input_devices() -> Result<Vec<String>> {
        let host = cpal::default_host();
        let devices = host
            .input_devices()
            .context("Failed to query input audio devices")?;
        let mut names = Vec::new();
        for dev in devices {
            if let Ok(name) = dev.name() {
                names.push(name);
            }
        }
        Ok(names)
    }

    fn select_device(&self) -> Result<Device> {
        let host = cpal::default_host();
        if let Some(ref desired_name) = self.device_name {
            let devices = host
                .input_devices()
                .context("Failed to enumerate audio devices")?;
            for dev in devices {
                if let Ok(name) = dev.name() {
                    if name.to_lowercase().contains(&desired_name.to_lowercase()) {
                        tracing::info!("Using matched audio input device: {}", name);
                        return Ok(dev);
                    }
                }
            }
            bail!(
                "Requested audio device containing {:?} was not found",
                desired_name
            );
        }

        host.default_input_device()
            .context("No default audio input device found")
    }

    pub fn start_recording(&self) -> Result<ActiveRecording> {
        let device = self.select_device()?;
        let dev_name = device.name().unwrap_or_else(|_| "Unknown".to_string());
        tracing::info!("Opening audio input stream on: {}", dev_name);

        let default_config = device
            .default_input_config()
            .context("Failed to get default input audio config")?;
        let sample_rate = default_config.sample_rate().0;
        let channels = default_config.channels();
        let sample_format = default_config.sample_format();

        tracing::info!(
            "Input device config: {} channels, {} Hz, format {:?}",
            channels,
            sample_rate,
            sample_format
        );

        let samples = Arc::new(Mutex::new(Vec::<f32>::with_capacity(sample_rate as usize * 4)));
        let is_recording = Arc::new(AtomicBool::new(true));

        let samples_cb = Arc::clone(&samples);
        let is_recording_cb = Arc::clone(&is_recording);

        let err_fn = |err| {
            tracing::error!("Audio stream error: {:?}", err);
        };

        let stream = match sample_format {
            SampleFormat::F32 => {
                device.build_input_stream(
                    &default_config.into(),
                    move |data: &[f32], _: &_| {
                        if is_recording_cb.load(Ordering::Relaxed) {
                            if let Ok(mut buf) = samples_cb.lock() {
                                buf.extend_from_slice(data);
                            }
                        }
                    },
                    err_fn,
                    None,
                )?
            }
            SampleFormat::I16 => {
                device.build_input_stream(
                    &default_config.into(),
                    move |data: &[i16], _: &_| {
                        if is_recording_cb.load(Ordering::Relaxed) {
                            if let Ok(mut buf) = samples_cb.lock() {
                                for &s in data {
                                    buf.push(s as f32 / 32768.0);
                                }
                            }
                        }
                    },
                    err_fn,
                    None,
                )?
            }
            SampleFormat::U16 => {
                device.build_input_stream(
                    &default_config.into(),
                    move |data: &[u16], _: &_| {
                        if is_recording_cb.load(Ordering::Relaxed) {
                            if let Ok(mut buf) = samples_cb.lock() {
                                for &s in data {
                                    buf.push((s as f32 - 32768.0) / 32768.0);
                                }
                            }
                        }
                    },
                    err_fn,
                    None,
                )?
            }
            other => bail!("Unsupported sample format: {:?}", other),
        };

        stream.play().context("Failed to play audio stream")?;

        Ok(ActiveRecording {
            stream,
            samples,
            channels,
            sample_rate,
            is_recording,
            started_at: Instant::now(),
        })
    }
}

impl ActiveRecording {
    #[allow(dead_code)]
    pub fn duration(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }

    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.is_recording.load(Ordering::Relaxed)
    }

    pub fn sample_buffer(&self) -> Arc<Mutex<Vec<f32>>> {
        Arc::clone(&self.samples)
    }

    pub fn is_recording_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.is_recording)
    }

    /// Copies samples recorded after index `start` and returns the new samples and current total sample length.
    #[allow(dead_code)]
    pub fn copy_samples_from(&self, start: usize) -> (Vec<f32>, usize) {
        if let Ok(guard) = self.samples.lock() {
            let total = guard.len();
            if start < total {
                let slice = guard[start..].to_vec();
                (slice, total)
            } else {
                (Vec::new(), total)
            }
        } else {
            (Vec::new(), start)
        }
    }

    /// Stops recording and encodes the captured audio into standard 16kHz mono WAV bytes.
    pub fn stop(self) -> Result<Vec<u8>> {
        self.is_recording.store(false, Ordering::SeqCst);
        drop(self.stream);

        let raw_samples = {
            let mut guard = self.samples.lock().unwrap();
            std::mem::take(&mut *guard)
        };

        let duration_secs = raw_samples.len() as f32 / (self.channels as f32 * self.sample_rate as f32);
        tracing::info!(
            "Captured {:.2}s of audio ({} raw samples)",
            duration_secs,
            raw_samples.len()
        );

        if raw_samples.is_empty() {
            bail!("No audio samples captured");
        }

        // Resample to 16,000 Hz mono 16-bit PCM
        let pcm_16k = resample_to_mono_16k(&raw_samples, self.channels, self.sample_rate, 16000);

        // Encode to in-memory WAV
        let spec = WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: WavSampleFormat::Int,
        };

        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = WavWriter::new(&mut buffer, spec)
                .context("Failed to initialize WAV writer")?;
            for sample in pcm_16k {
                writer.write_sample(sample)?;
            }
            writer.finalize().context("Failed to finalize WAV audio")?;
        }

        let wav_bytes = buffer.into_inner();
        tracing::info!("Encoded WAV audio size: {} bytes", wav_bytes.len());
        Ok(wav_bytes)
    }
}
