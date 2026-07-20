//! Inference provider selection (CPU / CUDA) and thread counts.

use std::path::{Path, PathBuf};

use tracing::{info, warn};

/// Resolve ONNX Runtime provider string for sherpa-onnx.
///
/// `"auto"` tries CUDA first when an NVIDIA GPU **and** a CUDA-12-compatible
/// cuBLAS runtime are present. Sherpa's published GPU packs need
/// `libcublasLt.so.12`; Arch CUDA 13 only ships `.so.13`, and probing CUDA in
/// that case aborts the process inside ONNX Runtime.
pub fn resolve_provider(requested: &str) -> Vec<&'static str> {
    match requested.trim().to_ascii_lowercase().as_str() {
        "cuda" | "gpu" => {
            if cuda12_runtime_available() {
                vec!["cuda", "cpu"]
            } else {
                warn!(
                    "Requested CUDA but libcublasLt.so.12 was not found \
                     (toolkit may be CUDA 13+). Falling back to CPU."
                );
                vec!["cpu"]
            }
        }
        "cpu" => vec!["cpu"],
        _ => {
            if nvidia_gpu_present() && cuda12_runtime_available() {
                vec!["cuda", "cpu"]
            } else {
                vec!["cpu"]
            }
        }
    }
}

pub fn resolve_num_threads(configured: i32) -> i32 {
    if configured > 0 {
        return configured.clamp(1, 16);
    }
    std::thread::available_parallelism()
        .map(|n| (n.get() as i32).clamp(1, 4))
        .unwrap_or(2)
}

pub fn nvidia_gpu_present() -> bool {
    Path::new("/dev/nvidia0").exists()
        || std::process::Command::new("nvidia-smi")
            .arg("-L")
            .output()
            .map(|o| o.status.success() && !o.stdout.is_empty())
            .unwrap_or(false)
}

/// Sherpa CUDA 12.x GPU builds dynamically load `libcublasLt.so.12`.
pub fn cuda12_runtime_available() -> bool {
    for dir in cuda_lib_search_dirs() {
        if dir.join("libcublasLt.so.12").is_file() {
            return true;
        }
        // Some layouts only ship the unversioned soname symlink.
        let unversioned = dir.join("libcublasLt.so");
        if unversioned.is_file() {
            if let Ok(target) = std::fs::read_link(&unversioned) {
                if target.to_string_lossy().contains("libcublasLt.so.12") {
                    return true;
                }
            }
        }
    }
    false
}

fn cuda_lib_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(ld) = std::env::var("LD_LIBRARY_PATH") {
        for part in ld.split(':').filter(|s| !s.is_empty()) {
            dirs.push(PathBuf::from(part));
        }
    }
    if let Ok(sherpa) = std::env::var("SHERPA_ONNX_LIB_DIR") {
        dirs.push(PathBuf::from(sherpa));
    }
    dirs.extend([
        PathBuf::from("/usr/lib"),
        PathBuf::from("/usr/local/cuda/lib64"),
        PathBuf::from("/opt/cuda/lib64"),
        PathBuf::from("/opt/cuda/targets/x86_64-linux/lib"),
        PathBuf::from("/usr/local/cuda-12/lib64"),
        PathBuf::from("/usr/local/cuda-12.4/lib64"),
        PathBuf::from("/usr/local/cuda-12.6/lib64"),
    ]);
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".cache/willow/cuda12-compat/lib"));
    }
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        dirs.push(PathBuf::from(xdg).join("willow/cuda12-compat/lib"));
    }
    dirs
}

pub fn log_provider_choice(requested: &str, active: &str) {
    if active == "cuda" {
        info!("Using CUDA ONNX Runtime provider (requested={requested})");
        return;
    }
    if requested.eq_ignore_ascii_case("cuda") || requested.eq_ignore_ascii_case("auto") {
        static WARNED: std::sync::Once = std::sync::Once::new();
        WARNED.call_once(|| {
            if nvidia_gpu_present() && !cuda12_runtime_available() {
                warn!(
                    "NVIDIA GPU present but CUDA 12 cuBLAS (libcublasLt.so.12) is missing — \
                     using CPU. Run ./deploy-dev.sh to fetch CUDA 12 compat libs into \
                     ~/.cache/willow/cuda12-compat (keeps system CUDA 13)."
                );
            } else {
                warn!(
                    "CUDA provider unavailable — using CPU. Rebuild with CUDA present \
                     (./deploy-dev.sh or makepkg with cuda+cudnn) for acceleration."
                );
            }
        });
        return;
    }
    info!("Using CPU ONNX Runtime provider");
}
