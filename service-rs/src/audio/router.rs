use std::collections::VecDeque;
use std::sync::Mutex;

const SAMPLE_RATE: usize = 16000;
const MAX_ROLLING_SECONDS: f32 = 10.0;

pub struct AudioRouter {
    rolling: Mutex<VecDeque<f32>>,
    max_samples: usize,
}

impl AudioRouter {
    pub fn new() -> Self {
        Self {
            rolling: Mutex::new(VecDeque::new()),
            max_samples: (SAMPLE_RATE as f32 * MAX_ROLLING_SECONDS) as usize,
        }
    }

    pub fn push_chunk(&self, chunk: &[f32]) {
        let mut buf = self.rolling.lock().unwrap();
        buf.extend(chunk);
        while buf.len() > self.max_samples {
            buf.pop_front();
        }
    }

    pub fn recent_audio(&self, seconds: f32) -> Vec<f32> {
        let buf = self.rolling.lock().unwrap();
        let count = (seconds * SAMPLE_RATE as f32) as usize;
        let start = buf.len().saturating_sub(count);
        buf.iter().skip(start).copied().collect()
    }
}

impl Default for AudioRouter {
    fn default() -> Self {
        Self::new()
    }
}
