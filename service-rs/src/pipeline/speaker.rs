use std::fs::{self, File};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use sherpa_onnx::{
    SpeakerEmbeddingExtractor, SpeakerEmbeddingExtractorConfig,
    SpeakerEmbeddingManager,
};
use tracing::{info, warn};

use crate::models::ModelPaths;
use crate::types::EnrollmentState;

const SAMPLE_LEN: usize = 16000 * 2;
const MIN_RMS: f32 = 0.008;
const TIMEOUT: Duration = Duration::from_secs(90);
const HINT_INTERVAL: Duration = Duration::from_secs(8);
const VERIFY_COOLDOWN: Duration = Duration::from_secs(3);

const ENROLLMENT_PROMPTS: [&str; 3] = [
    "Sample 1/3 — try: \"hey willow, open terminal\"",
    "Sample 2/3 — try: \"what time is it today\"",
    "Sample 3/3 — say anything else in your normal voice",
];

pub struct SpeakerVerifier {
    extractor: Option<SpeakerEmbeddingExtractor>,
    manager: Option<SpeakerEmbeddingManager>,
    profile_path: std::path::PathBuf,
    enrolled_user: String,
    threshold: f32,
    enabled: bool,
    enrolled: bool,
    state: EnrollmentState,
    samples: Vec<Vec<f32>>,
    buffer: Vec<f32>,
    reenrolling: bool,
    speech_detected: bool,
    started_at: Option<Instant>,
    last_speech_at: Option<Instant>,
    last_hint_at: Option<Instant>,
    verify_cooldown_until: Option<Instant>,
    last_verify_passed: Option<bool>,
}

impl SpeakerVerifier {
    pub fn new(paths: &ModelPaths) -> Self {
        Self {
            extractor: None,
            manager: None,
            profile_path: paths.speaker_profile_path(),
            enrolled_user: "owner".into(),
            threshold: 0.65,
            enabled: true,
            enrolled: false,
            state: EnrollmentState::Idle,
            samples: Vec::new(),
            buffer: Vec::new(),
            reenrolling: false,
            speech_detected: false,
            started_at: None,
            last_speech_at: None,
            last_hint_at: None,
            verify_cooldown_until: None,
            last_verify_passed: None,
        }
    }

    pub fn initialize(&mut self, paths: &ModelPaths) -> Result<()> {
        let model = paths
            .find_speaker_model()
            .context("speaker model not found")?;
        let config = SpeakerEmbeddingExtractorConfig {
            model: Some(model),
            num_threads: 1,
            debug: false,
            provider: Some("cpu".into()),
        };
        let extractor =
            SpeakerEmbeddingExtractor::create(&config).context("create speaker extractor")?;
        let dim = extractor.dim();
        let manager = SpeakerEmbeddingManager::create(dim).context("create speaker manager")?;
        self.extractor = Some(extractor);
        self.manager = Some(manager);
        self.load_profile()?;
        info!("Speaker verifier initialized");
        Ok(())
    }

    pub fn is_enrolled(&self) -> bool {
        self.enrolled
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn set_threshold(&mut self, threshold: f32) {
        self.threshold = threshold;
    }

    pub fn set_enrolled_user(&mut self, user: &str) {
        self.enrolled_user = user.to_string();
    }

    pub fn enrollment_state(&self) -> EnrollmentState {
        self.state
    }

    pub fn enrollment_progress(&self) -> i32 {
        self.samples.len() as i32
    }

    pub fn enrollment_buffer_fraction(&self) -> f32 {
        if self.state != EnrollmentState::Recording {
            return 0.0;
        }
        (self.buffer.len() as f32 / SAMPLE_LEN as f32).min(1.0)
    }

    pub fn is_reenrolling(&self) -> bool {
        self.reenrolling
    }

    pub fn current_enrollment_prompt(&self) -> &'static str {
        if self.state != EnrollmentState::Recording {
            return "";
        }
        let idx = self.samples.len().min(ENROLLMENT_PROMPTS.len() - 1);
        ENROLLMENT_PROMPTS[idx]
    }

    pub fn in_verify_cooldown(&self) -> bool {
        self.verify_cooldown_until
            .is_some_and(|until| Instant::now() < until)
    }

    pub fn last_verify_result(&self) -> Option<bool> {
        self.last_verify_passed
    }

    pub fn record_verify_pass(&mut self) {
        self.last_verify_passed = Some(true);
        self.verify_cooldown_until = None;
    }

    pub fn record_verify_fail(&mut self) {
        self.last_verify_passed = Some(false);
        self.verify_cooldown_until = Some(Instant::now() + VERIFY_COOLDOWN);
    }

    pub fn start_enrollment(&mut self, user: &str) {
        self.enrolled_user = user.to_string();
        self.samples.clear();
        self.buffer.clear();
        self.state = EnrollmentState::Recording;
        self.reenrolling = self.enrolled;
        self.speech_detected = false;
        let now = Instant::now();
        self.started_at = Some(now);
        self.last_speech_at = Some(now);
        self.last_hint_at = Some(now);
        info!("Started speaker enrollment for {user}");
    }

    pub fn cancel_enrollment(&mut self) {
        self.samples.clear();
        self.buffer.clear();
        self.state = EnrollmentState::Idle;
        self.reenrolling = false;
    }

    pub fn remove_profile(&mut self) {
        if let Some(manager) = &self.manager {
            manager.remove(&self.enrolled_user);
        }
        self.enrolled = false;
        if self.profile_path.exists() {
            let _ = fs::remove_file(&self.profile_path);
        }
    }

    pub fn should_prompt_for_speech(&self) -> bool {
        if self.state != EnrollmentState::Recording {
            return false;
        }
        let now = Instant::now();
        if self.started_at.is_some_and(|s| now.duration_since(s) > TIMEOUT) {
            return false;
        }
        if self.last_hint_at.is_some_and(|t| now.duration_since(t) < HINT_INTERVAL) {
            return false;
        }
        if self.speech_detected
            && self
                .last_speech_at
                .is_some_and(|t| now.duration_since(t) < HINT_INTERVAL)
        {
            return false;
        }
        true
    }

    pub fn mark_speech_prompt_sent(&mut self) {
        self.last_hint_at = Some(Instant::now());
    }

    pub fn add_enrollment_audio(&mut self, chunk: &[f32]) -> bool {
        if self.state != EnrollmentState::Recording {
            return false;
        }
        let now = Instant::now();
        if self.started_at.is_some_and(|s| now.duration_since(s) > TIMEOUT) {
            self.state = EnrollmentState::Failed;
            warn!("Enrollment timed out");
            return false;
        }

        let rms = compute_rms(chunk);
        if rms < MIN_RMS {
            return false;
        }

        self.speech_detected = true;
        self.last_speech_at = Some(now);
        self.buffer.extend_from_slice(chunk);

        while self.buffer.len() >= SAMPLE_LEN {
            self.samples.push(self.buffer[..SAMPLE_LEN].to_vec());
            self.buffer.drain(..SAMPLE_LEN);
            info!("Enrollment sample {} recorded", self.samples.len());
        }
        true
    }

    pub fn finish_enrollment(&mut self) -> bool {
        if self.extractor.is_none() || self.manager.is_none() || self.samples.len() < 2 {
            self.state = EnrollmentState::Failed;
            self.samples.clear();
            self.buffer.clear();
            self.reenrolling = false;
            return false;
        }

        let mut embeddings = Vec::new();
        for sample in &self.samples {
            if let Some(emb) = self.compute_embedding(sample) {
                embeddings.push(emb);
            }
        }

        if embeddings.len() < 2 {
            self.state = EnrollmentState::Failed;
            self.samples.clear();
            self.buffer.clear();
            self.reenrolling = false;
            return false;
        }

        let manager = self.manager.as_ref().unwrap();
        manager.remove(&self.enrolled_user);
        let ok = manager.add_list(&self.enrolled_user, &embeddings);

        if ok {
            self.enrolled = true;
            self.state = EnrollmentState::Complete;
            let _ = self.persist_profile(&embeddings);
            info!("Speaker enrollment complete");
        } else {
            self.state = EnrollmentState::Failed;
            if self.reenrolling {
                let _ = self.load_profile();
            }
        }

        self.samples.clear();
        self.buffer.clear();
        self.reenrolling = false;
        ok
    }

    pub fn verify(&self, audio: &[f32]) -> bool {
        if !self.enabled || !self.enrolled {
            return !self.enabled;
        }
        let Some(embedding) = self.compute_embedding(audio) else {
            return false;
        };
        self.manager
            .as_ref()
            .map(|m| m.verify(&self.enrolled_user, &embedding, self.threshold))
            .unwrap_or(false)
    }

    fn compute_embedding(&self, audio: &[f32]) -> Option<Vec<f32>> {
        let extractor = self.extractor.as_ref()?;
        let stream = extractor.create_stream()?;
        stream.accept_waveform(16000, audio);
        stream.input_finished();
        if !extractor.is_ready(&stream) {
            return None;
        }
        extractor.compute(&stream)
    }

    fn load_profile(&mut self) -> Result<bool> {
        let path = &self.profile_path;
        if !path.is_file() {
            return Ok(false);
        }
        let mut file = File::open(path)?;
        let mut dim_bytes = [0u8; 4];
        let mut count_bytes = [0u8; 4];
        file.read_exact(&mut dim_bytes)?;
        file.read_exact(&mut count_bytes)?;
        let dim = i32::from_le_bytes(dim_bytes);
        let count = i32::from_le_bytes(count_bytes);
        let extractor_dim = self.extractor.as_ref().map(|e| e.dim()).unwrap_or(0);
        if dim != extractor_dim || count <= 0 {
            return Ok(false);
        }

        if let Some(manager) = &self.manager {
            manager.remove(&self.enrolled_user);
            let mut embeddings = Vec::new();
            for _ in 0..count {
                let mut emb = vec![0f32; dim as usize];
                let bytes = &mut vec![0u8; emb.len() * 4];
                file.read_exact(bytes)?;
                for (i, chunk) in bytes.chunks_exact(4).enumerate() {
                    emb[i] = f32::from_le_bytes(chunk.try_into().unwrap());
                }
                embeddings.push(emb);
            }
            let ok = manager.add_list(&self.enrolled_user, &embeddings);
            self.enrolled = ok;
            if ok {
                info!("Speaker profile loaded");
            }
            return Ok(ok);
        }
        Ok(false)
    }

    fn persist_profile(&self, embeddings: &[Vec<f32>]) -> Result<()> {
        if let Some(parent) = self.profile_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let dim = embeddings[0].len() as i32;
        let count = embeddings.len() as i32;
        let mut file = File::create(&self.profile_path)?;
        file.write_all(&dim.to_le_bytes())?;
        file.write_all(&count.to_le_bytes())?;
        for emb in embeddings {
            for v in emb {
                file.write_all(&v.to_le_bytes())?;
            }
        }
        Ok(())
    }
}

fn compute_rms(audio: &[f32]) -> f32 {
    if audio.is_empty() {
        return 0.0;
    }
    let sum: f64 = audio.iter().map(|s| (*s as f64) * (*s as f64)).sum();
    (sum / audio.len() as f64).sqrt() as f32
}
