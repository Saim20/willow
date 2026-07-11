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

    pub fn base_path(&self) -> &Path {
        &self.base_path
    }

    pub fn find_kws_model(&self) -> Option<TransducerModelFiles> {
        let dir = self.find_first_dir(&["kws", "kws-zipformer-en"])?;
        self.find_transducer_in_dir(&dir)
    }

    pub fn find_streaming_model(&self) -> Option<TransducerModelFiles> {
        let dir = self.find_first_dir(&["streaming", "streaming-zipformer-en"])?;
        self.find_transducer_in_dir(&dir)
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

    pub fn kws_keywords_path(&self) -> PathBuf {
        for candidate in [
            self.base_path.join("kws/keywords.txt"),
            self.base_path.join("keywords.txt"),
        ] {
            if candidate.is_file() {
                return candidate;
            }
        }
        self.base_path.join("kws/keywords.txt")
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
            let name = path.file_name()?.to_string_lossy().to_string();
            if !name.ends_with(".onnx") {
                continue;
            }
            if name.contains("encoder") {
                encoder = Some(path);
            } else if name.contains("decoder") {
                decoder = Some(path);
            } else if name.contains("joiner") {
                joiner = Some(path);
            }
        }

        Some(TransducerModelFiles {
            tokens: tokens.to_string_lossy().into_owned(),
            encoder: encoder.map(|p| p.to_string_lossy().into_owned())?,
            decoder: decoder.map(|p| p.to_string_lossy().into_owned())?,
            joiner: joiner.map(|p| p.to_string_lossy().into_owned())?,
        })
    }
}

pub fn keyword_encoding_available() -> bool {
    for candidate in [
        "/usr/share/willow/scripts/generate-keywords.py",
        concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/generate-keywords.py"),
    ] {
        if Path::new(candidate).is_file() {
            return std::process::Command::new("python3")
                .args(["-c", "import sentencepiece"])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
        }
    }
    false
}

pub fn encode_keywords(
    tokens: &str,
    bpe_model: &str,
    output: &Path,
    phrases: &[String],
) -> Result<()> {
    let script = [
        "/usr/share/willow/scripts/generate-keywords.py",
        concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/generate-keywords.py"),
    ]
    .into_iter()
    .find(|p| Path::new(p).is_file())
    .map(|s| s.to_string());

    let script = script.ok_or_else(|| anyhow::anyhow!("generate-keywords.py not found"))?;

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
        .arg(&script)
        .args(["--tokens", tokens])
        .args(["--bpe-model", bpe_model])
        .args(["--input", &input_path.to_string_lossy()])
        .args(["--output", &output.to_string_lossy()])
        .status()?;

    if !status.success() {
        bail!("keyword encoding failed");
    }
    Ok(())
}

pub fn keywords_look_encoded(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.lines().next().map(|l| l.to_string()))
        .is_some_and(|l| l.contains('▁'))
}
