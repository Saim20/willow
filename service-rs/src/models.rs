use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

#[derive(Debug, Clone)]
pub struct TransducerModelFiles {
    pub tokens: String,
    pub encoder: String,
    pub decoder: String,
    pub joiner: String,
}

#[derive(Debug, Clone)]
pub struct WhisperModelFiles {
    pub tokens: String,
    pub encoder: String,
    pub decoder: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct ModelPaths {
    base_path: PathBuf,
}

impl ModelPaths {
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }

    pub fn from_home() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        Self::new(home.join(".local/share/willow/models"))
    }

    pub fn find_kws_model(&self) -> Option<TransducerModelFiles> {
        let dir = self.find_first_dir(&["kws", "kws-zipformer-en"])?;
        self.find_transducer_in_dir(&dir)
    }

    /// Streaming zipformer transducer for command-mode ASR.
    pub fn find_streaming_asr_model(&self) -> Option<TransducerModelFiles> {
        for name in [
            "asr-stream",
            "streaming-asr",
            "sherpa-onnx-streaming-zipformer-en-20M-2023-02-17",
            "sherpa-onnx-streaming-zipformer-en-2023-06-26",
        ] {
            let dir = self.base_path.join(name);
            if let Some(files) = self.find_transducer_in_dir(&dir) {
                return Some(files);
            }
        }
        None
    }

    pub fn find_vad_model(&self) -> Option<String> {
        for name in ["vad", "silero-vad", "silero_vad"] {
            let dir = self.base_path.join(name);
            if let Some(path) = Self::find_onnx_named(&dir, "silero") {
                return Some(path);
            }
            if let Some(path) = Self::find_any_onnx(&dir) {
                return Some(path);
            }
        }
        // Flat file layout: models/silero_vad.onnx
        for flat in [
            self.base_path.join("silero_vad.onnx"),
            self.base_path.join("silero_vad.int8.onnx"),
            self.base_path.join("vad").join("silero_vad.onnx"),
        ] {
            if flat.is_file() {
                return Some(flat.to_string_lossy().into_owned());
            }
        }
        None
    }

    pub fn find_whisper_model(&self) -> Option<WhisperModelFiles> {
        // Prefer base.en when both are installed (accuracy), else tiny.en.
        for name in [
            "whisper",
            "whisper-base.en",
            "whisper-tiny.en",
            "sherpa-onnx-whisper-base.en",
            "sherpa-onnx-whisper-tiny.en",
        ] {
            let dir = self.base_path.join(name);
            if let Some(files) = self.find_whisper_in_dir(&dir) {
                return Some(files);
            }
        }
        None
    }

    pub fn find_speaker_model(&self) -> Option<String> {
        let dir = self.find_first_dir(&["speaker", "speaker-resemblyzer", "wespeaker"])?;
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let name = path.file_name()?.to_string_lossy();
                if name.contains("model") && name.ends_with(".onnx") {
                    return Some(path.to_string_lossy().into_owned());
                }
            }
            for entry in std::fs::read_dir(&dir).ok()?.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "onnx") {
                    return Some(path.to_string_lossy().into_owned());
                }
            }
        }
        None
    }

    pub fn speaker_profile_path(&self) -> PathBuf {
        dirs::home_dir()
            .map(|h| h.join(".config/willow/speaker_profile.bin"))
            .unwrap_or_else(|| PathBuf::from("/tmp/willow_speaker_profile.bin"))
    }

    fn find_first_dir(&self, names: &[&str]) -> Option<PathBuf> {
        for name in names {
            let path = self.base_path.join(name);
            if path.is_dir() {
                return Some(path);
            }
        }
        None
    }

    fn find_any_onnx(dir: &Path) -> Option<String> {
        if !dir.is_dir() {
            return None;
        }
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "onnx") {
                return Some(path.to_string_lossy().into_owned());
            }
        }
        None
    }

    fn find_onnx_named(dir: &Path, needle: &str) -> Option<String> {
        if !dir.is_dir() {
            return None;
        }
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
            if name.contains(needle) && name.ends_with(".onnx") {
                return Some(path.to_string_lossy().into_owned());
            }
        }
        None
    }

    fn find_transducer_in_dir(&self, dir: &Path) -> Option<TransducerModelFiles> {
        let tokens = dir.join("tokens.txt");
        if !tokens.is_file() {
            return None;
        }

        let mut encoder = None;
        let mut decoder = None;
        let mut joiner = None;

        let entries = std::fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
            if !name.ends_with(".onnx") {
                continue;
            }
            // Prefer int8 variants when both float and int8 exist.
            if name.contains("encoder") {
                let take = encoder.is_none() || name.contains("int8");
                if take {
                    encoder = Some(path);
                }
            } else if name.contains("decoder") {
                let take = decoder.is_none() || name.contains("int8");
                if take {
                    decoder = Some(path);
                }
            } else if name.contains("joiner") {
                let take = joiner.is_none() || name.contains("int8");
                if take {
                    joiner = Some(path);
                }
            }
        }

        Some(TransducerModelFiles {
            tokens: tokens.to_string_lossy().into_owned(),
            encoder: encoder.map(|p| p.to_string_lossy().into_owned())?,
            decoder: decoder.map(|p| p.to_string_lossy().into_owned())?,
            joiner: joiner.map(|p| p.to_string_lossy().into_owned())?,
        })
    }

    fn find_whisper_in_dir(&self, dir: &Path) -> Option<WhisperModelFiles> {
        if !dir.is_dir() {
            return None;
        }

        // Official sherpa packs use `tiny.en-tokens.txt` / `base.en-tokens.txt`,
        // not a bare `tokens.txt`.
        let tokens = Self::find_whisper_tokens(dir)?;

        let mut encoder = None;
        let mut decoder = None;
        let entries = std::fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
            if !name.ends_with(".onnx") {
                continue;
            }
            // Prefer int8 variants when both float and int8 exist.
            if name.contains("encoder") {
                let take = encoder.is_none() || name.contains("int8");
                if take {
                    encoder = Some(path);
                }
            } else if name.contains("decoder") {
                let take = decoder.is_none() || name.contains("int8");
                if take {
                    decoder = Some(path);
                }
            }
        }

        let label = dir
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "whisper".into());

        Some(WhisperModelFiles {
            tokens,
            encoder: encoder?.to_string_lossy().into_owned(),
            decoder: decoder?.to_string_lossy().into_owned(),
            label,
        })
    }

    fn find_whisper_tokens(dir: &Path) -> Option<String> {
        let bare = dir.join("tokens.txt");
        if bare.is_file() {
            return Some(bare.to_string_lossy().into_owned());
        }
        let mut best: Option<PathBuf> = None;
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
            if name.ends_with(".txt") && name.contains("tokens") {
                best = Some(path);
                break;
            }
        }
        best.map(|p| p.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_sherpa_whisper_tiny_layout() {
        let dir = std::env::temp_dir().join(format!(
            "willow-whisper-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("tiny.en-tokens.txt"), "a\n").unwrap();
        fs::write(dir.join("tiny.en-encoder.int8.onnx"), b"enc").unwrap();
        fs::write(dir.join("tiny.en-decoder.int8.onnx"), b"dec").unwrap();
        fs::write(dir.join("tiny.en-encoder.onnx"), b"enc-f").unwrap();
        fs::write(dir.join("tiny.en-decoder.onnx"), b"dec-f").unwrap();

        let _paths = ModelPaths::new(dir.parent().unwrap());
        // Point base at parent and look via find_whisper_in_dir through find_whisper_model
        // by nesting under whisper/
        let model_root = dir.parent().unwrap().join(format!(
            "willow-models-{}",
            std::process::id()
        ));
        let whisper = model_root.join("whisper");
        let _ = fs::remove_dir_all(&model_root);
        fs::create_dir_all(&whisper).unwrap();
        for name in [
            "tiny.en-tokens.txt",
            "tiny.en-encoder.int8.onnx",
            "tiny.en-decoder.int8.onnx",
        ] {
            fs::copy(dir.join(name), whisper.join(name)).unwrap();
        }

        let found = ModelPaths::new(&model_root)
            .find_whisper_model()
            .expect("should find tiny.en layout");
        assert!(found.tokens.ends_with("tiny.en-tokens.txt"));
        assert!(found.encoder.contains("encoder.int8"));
        assert!(found.decoder.contains("decoder.int8"));

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&model_root);
    }

    #[test]
    fn finds_streaming_asr_layout() {
        let model_root = std::env::temp_dir().join(format!(
            "willow-stream-models-{}",
            std::process::id()
        ));
        let stream = model_root.join("asr-stream");
        let _ = fs::remove_dir_all(&model_root);
        fs::create_dir_all(&stream).unwrap();
        fs::write(stream.join("tokens.txt"), "a\n").unwrap();
        fs::write(stream.join("encoder-epoch-99-avg-1.int8.onnx"), b"enc").unwrap();
        fs::write(stream.join("decoder-epoch-99-avg-1.int8.onnx"), b"dec").unwrap();
        fs::write(stream.join("joiner-epoch-99-avg-1.int8.onnx"), b"join").unwrap();

        let found = ModelPaths::new(&model_root)
            .find_streaming_asr_model()
            .expect("should find asr-stream transducer");
        assert!(found.encoder.contains("encoder"));
        assert!(found.decoder.contains("decoder"));
        assert!(found.joiner.contains("joiner"));
        assert!(found.tokens.ends_with("tokens.txt"));

        let _ = fs::remove_dir_all(&model_root);
    }
}

pub fn keyword_encoding_available() -> bool {
    find_keyword_script().is_some_and(|script| {
        std::process::Command::new("python3")
            .args(["-c", "import sentencepiece"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
            && script.is_file()
    })
}

fn keyword_script_candidates() -> Vec<std::path::PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(root) = std::env::var("WILLOW_SOURCE_ROOT") {
        candidates.push(std::path::PathBuf::from(root).join("scripts/generate-keywords.py"));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(
            home.join(".local/share/willow/scripts/generate-keywords.py"),
        );
    }
    candidates.push(std::path::PathBuf::from(
        "/usr/share/willow/scripts/generate-keywords.py",
    ));
    let build_tree = std::path::PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../scripts/generate-keywords.py"
    ));
    candidates.push(build_tree);
    candidates
}

fn find_keyword_script() -> Option<std::path::PathBuf> {
    keyword_script_candidates()
        .into_iter()
        .find(|p| p.is_file())
}

pub fn encode_keywords(
    tokens: &str,
    bpe_model: &str,
    output: &Path,
    phrases: &[String],
) -> Result<()> {
    let script = find_keyword_script().ok_or_else(|| anyhow::anyhow!("generate-keywords.py not found"))?;

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let temp_dir = std::env::temp_dir().join("willow-kws");
    std::fs::create_dir_all(&temp_dir)?;
    let input_path = temp_dir.join("keywords_raw.txt");
    let mut input = String::new();
    for phrase in phrases {
        if !phrase.is_empty() {
            input.push_str(phrase);
            input.push('\n');
        }
    }
    std::fs::write(&input_path, input)?;

    let status = std::process::Command::new("python3")
        .arg(script.as_os_str())
        .args(["--tokens", tokens])
        .args(["--bpe-model", bpe_model])
        .args(["--input", &input_path.to_string_lossy()])
        .args(["--output", &output.to_string_lossy()])
        .status()?;

    if !status.success() {
        bail!("keyword encoding failed");
    }
    if !keywords_look_encoded(output) {
        bail!("keyword encoding produced invalid output (missing @TAG aliases)");
    }
    Ok(())
}

pub fn keywords_look_encoded(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .is_some_and(|s| {
            s.lines().any(|line| {
                let line = line.trim();
                !line.is_empty() && line.contains('▁') && line.contains('@')
            })
        })
}
