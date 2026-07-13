use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result};
use tracing::{error, info};

const CHUNK_SAMPLES: usize = 1024;
const SAMPLE_RATE: u32 = 16000;

pub struct MicCapture;

impl MicCapture {
    /// Spawns a dedicated thread for microphone capture via PulseAudio/PipeWire.
    /// Set the returned stop flag to end capture and allow the thread to join.
    pub fn start<F>(mut on_chunk: F) -> Result<(JoinHandle<()>, Arc<AtomicBool>)>
    where
        F: FnMut(&[f32]) + Send + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();

        let handle = thread::Builder::new()
            .name("willow-audio".into())
            .spawn(move || {
                if let Err(e) = run_pulse_capture(&stop_flag, &mut on_chunk) {
                    error!("Audio capture failed: {e}");
                }
            })
            .context("spawn audio thread")?;

        Ok((handle, stop))
    }
}

fn run_pulse_capture(
    stop: &AtomicBool,
    on_chunk: &mut dyn FnMut(&[f32]),
) -> Result<()> {
    use libpulse_binding as pulse;
    use libpulse_simple_binding as psimple;

    let spec = pulse::sample::Spec {
        format: pulse::sample::Format::FLOAT32NE,
        channels: 1,
        rate: SAMPLE_RATE,
    };
    if !spec.is_valid() {
        anyhow::bail!("invalid pulse audio spec");
    }

    let simple = psimple::Simple::new(
        None,
        "willow-service",
        pulse::stream::Direction::Record,
        None,
        "capture",
        &spec,
        None,
        None,
    )
    .map_err(|e| anyhow::anyhow!("pulse connect: {e:?}"))?;

    info!("Microphone: PulseAudio default source (16 kHz mono)");

    let bytes_per_chunk = CHUNK_SAMPLES * std::mem::size_of::<f32>();
    let mut buf = vec![0u8; bytes_per_chunk];

    while !stop.load(Ordering::Relaxed) {
        simple
            .read(&mut buf)
            .map_err(|e| anyhow::anyhow!("pulse read: {e:?}"))?;
        let chunk: Vec<f32> = buf
            .chunks_exact(4)
            .map(|b| f32::from_ne_bytes(b.try_into().unwrap()))
            .collect();
        on_chunk(&chunk);
    }
    Ok(())
}
