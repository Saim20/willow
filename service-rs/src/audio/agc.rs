//! Adaptive gain control so wake-word / ASR stay stable across mic levels.

const TARGET_RMS: f32 = 0.12;
const NOISE_FLOOR: f32 = 0.004;
const MAX_GAIN: f32 = 12.0;
const MIN_GAIN: f32 = 0.2;
const ATTACK: f32 = 0.35;
const RELEASE: f32 = 0.08;
const GAIN_SMOOTH: f32 = 0.25;

pub struct Agc {
    smoothed_rms: f32,
    gain: f32,
}

impl Agc {
    pub fn new() -> Self {
        Self {
            smoothed_rms: TARGET_RMS,
            gain: 1.0,
        }
    }

    /// Scale `chunk` toward [`TARGET_RMS`]. Near-silence is left mostly alone
    /// so noise is not amplified into false wake-word triggers.
    pub fn process(&mut self, chunk: &[f32]) -> Vec<f32> {
        if chunk.is_empty() {
            return Vec::new();
        }

        let rms = rms(chunk);
        if rms > self.smoothed_rms {
            self.smoothed_rms += ATTACK * (rms - self.smoothed_rms);
        } else {
            self.smoothed_rms += RELEASE * (rms - self.smoothed_rms);
        }

        let desired = if self.smoothed_rms < NOISE_FLOOR {
            // Decay toward unity during silence so the next utterance starts clean.
            1.0
        } else {
            (TARGET_RMS / self.smoothed_rms).clamp(MIN_GAIN, MAX_GAIN)
        };
        self.gain += GAIN_SMOOTH * (desired - self.gain);

        chunk
            .iter()
            .map(|s| soft_clip(s * self.gain))
            .collect()
    }

    pub fn current_gain(&self) -> f32 {
        self.gain
    }
}

impl Default for Agc {
    fn default() -> Self {
        Self::new()
    }
}

fn rms(audio: &[f32]) -> f32 {
    if audio.is_empty() {
        return 0.0;
    }
    let sum: f64 = audio.iter().map(|s| (*s as f64) * (*s as f64)).sum();
    (sum / audio.len() as f64).sqrt() as f32
}

fn soft_clip(x: f32) -> f32 {
    // Gentle tanh-style clip keeps peaks out of hard ±1.0 distortion.
    let a = x.abs();
    if a <= 0.9 {
        x
    } else {
        x.signum() * (0.9 + 0.1 * ((a - 0.9) / 0.1).tanh())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boosts_quiet_speech() {
        let mut agc = Agc::new();
        let quiet: Vec<f32> = (0..1024).map(|i| 0.01 * ((i as f32) * 0.1).sin()).collect();
        let mut out = Vec::new();
        for _ in 0..20 {
            out = agc.process(&quiet);
        }
        let in_rms = rms(&quiet);
        let out_rms = rms(&out);
        assert!(out_rms > in_rms * 2.0, "expected boost, in={in_rms} out={out_rms}");
        assert!(agc.current_gain() > 2.0);
    }

    #[test]
    fn attenuates_loud_speech() {
        let mut agc = Agc::new();
        let loud: Vec<f32> = (0..1024).map(|i| 0.8 * ((i as f32) * 0.1).sin()).collect();
        for _ in 0..20 {
            let _ = agc.process(&loud);
        }
        assert!(agc.current_gain() < 1.0);
    }

    #[test]
    fn silence_does_not_runaway_gain() {
        let mut agc = Agc::new();
        let silence = vec![0.0001f32; 1024];
        for _ in 0..50 {
            let _ = agc.process(&silence);
        }
        assert!(agc.current_gain() < 2.0, "gain={}", agc.current_gain());
    }
}
