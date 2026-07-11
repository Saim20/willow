use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use tracing::{error, info};

const CHUNK_SAMPLES: usize = 4096;

pub struct MicCapture;

impl MicCapture {
    /// Spawns a dedicated thread that owns the cpal stream (streams are !Send).
    pub fn start<F>(mut on_chunk: F) -> Result<JoinHandle<()>>
    where
        F: FnMut(&[f32]) + Send + 'static,
    {
        thread::Builder::new()
            .name("willow-audio".into())
            .spawn(move || {
                if let Err(e) = (|| -> Result<()> {
                    let host = cpal::default_host();
                    let device = host
                        .default_input_device()
                        .context("no default input device")?;
                    info!("Microphone: {}", device.name().unwrap_or_default());

                    let supported = device
                        .supported_input_configs()
                        .context("enumerate input configs")?;

                    let config = supported
                        .filter(|c| c.sample_format() == SampleFormat::F32)
                        .find(|c| {
                            c.min_sample_rate().0 <= 16000 && c.max_sample_rate().0 >= 16000
                        })
                        .or_else(|| {
                            device
                                .supported_input_configs()
                                .ok()?
                                .find(|c| c.sample_format() == SampleFormat::F32)
                        })
                        .context("no suitable F32 input config")?
                        .with_sample_rate(cpal::SampleRate(16000));

                    let sample_format = config.sample_format();
                    let stream_config: StreamConfig = config.into();
                    let err_fn = |e| error!("audio stream error: {e}");

                    let stream = match sample_format {
                        SampleFormat::F32 => device.build_input_stream(
                            &stream_config,
                            move |data: &[f32], _| on_chunk(data),
                            err_fn,
                            None,
                        )?,
                        SampleFormat::I16 => device.build_input_stream(
                            &stream_config,
                            move |data: &[i16], _| {
                                let floats: Vec<f32> =
                                    data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                                for chunk in floats.chunks(CHUNK_SAMPLES) {
                                    on_chunk(chunk);
                                }
                            },
                            err_fn,
                            None,
                        )?,
                        SampleFormat::U16 => device.build_input_stream(
                            &stream_config,
                            move |data: &[u16], _| {
                                let floats: Vec<f32> = data
                                    .iter()
                                    .map(|&s| (s as f32 - 32768.0) / 32768.0)
                                    .collect();
                                for chunk in floats.chunks(CHUNK_SAMPLES) {
                                    on_chunk(chunk);
                                }
                            },
                            err_fn,
                            None,
                        )?,
                        _ => anyhow::bail!("unsupported sample format {:?}", sample_format),
                    };

                    stream.play()?;
                    Ok(())
                })() {
                    error!("Audio capture failed: {e}");
                }
                loop {
                    thread::sleep(Duration::from_secs(3600));
                }
            })
            .context("spawn audio thread")
    }
}
