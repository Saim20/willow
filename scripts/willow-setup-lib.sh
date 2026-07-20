#!/usr/bin/env bash
# Shared setup helpers for Willow (sourced by deploy-dev.sh / download-model.sh / PKGBUILD).
# shellcheck disable=SC2034

WILLOW_SETUP_LIB_VERSION=1

_willow_log() {
    echo "$@" >&2
}

willow_has_nvidia() {
    [[ -e /dev/nvidia0 ]] || command -v nvidia-smi >/dev/null 2>&1
}

willow_has_cuda_pkgs() {
    # Arch: cuda + cudnn packages present (enough for linking GPU sherpa).
    if command -v pacman >/dev/null 2>&1; then
        pacman -Q cuda >/dev/null 2>&1 && pacman -Q cudnn >/dev/null 2>&1
        return $?
    fi
    [[ -x /opt/cuda/bin/nvcc ]] || [[ -x /usr/local/cuda/bin/nvcc ]]
}

# Sherpa GPU packs need CUDA 12 cuBLAS (libcublasLt.so.12). Arch cuda 13 only has .so.13.
willow_default_cuda12_compat_dir() {
    echo "${XDG_CACHE_HOME:-${HOME}/.cache}/willow/cuda12-compat"
}

willow_default_cuda12_compat_lib_dir() {
    echo "$(willow_default_cuda12_compat_dir)/lib"
}

willow_has_cuda12_cublas() {
    local d
    for d in \
        "$(willow_default_cuda12_compat_lib_dir)" \
        ${LD_LIBRARY_PATH//:/ } \
        /usr/lib \
        /usr/local/cuda/lib64 \
        /opt/cuda/lib64 \
        /opt/cuda/targets/x86_64-linux/lib \
        /usr/local/cuda-12/lib64 \
        /usr/local/cuda-12.4/lib64 \
        /usr/local/cuda-12.6/lib64
    do
        [[ -z "$d" ]] && continue
        if [[ -f "${d}/libcublasLt.so.12" ]]; then
            return 0
        fi
    done
    return 1
}

# Download NVIDIA CUDA 12 redistributable libs (cublas/cudart/cufft) into user cache.
# Lets sherpa's CUDA-12 ORT EP run on a CUDA-13 system driver without replacing the toolkit.
willow_setup_cuda12_compat() {
    local root
    root="$(willow_default_cuda12_compat_dir)"
    local lib_dir="${root}/lib"
    if [[ -f "${lib_dir}/libcublasLt.so.12" && -f "${lib_dir}/libcudart.so.12" && -f "${lib_dir}/libcufft.so.11" ]]; then
        _willow_log "✓ CUDA 12 compat libs already present (${lib_dir})"
        echo "${lib_dir}"
        return 0
    fi

    _willow_log "→ Fetching CUDA 12 runtime libs for sherpa GPU (system is CUDA 13)…"
    mkdir -p "${root}/wheels" "${lib_dir}" "${root}/extract"
    local wheels_dir="${root}/wheels"
    local pkg url filename
    for pkg in nvidia-cublas-cu12 nvidia-cuda-runtime-cu12 nvidia-cufft-cu12; do
        url="$(python3 - "${pkg}" <<'PY'
import json, sys, urllib.request
pkg = sys.argv[1]
data = json.load(urllib.request.urlopen(f"https://pypi.org/pypi/{pkg}/json", timeout=60))
ver = data["info"]["version"]
files = data["releases"].get(ver, [])
cands = [f for f in files if f["packagetype"] == "bdist_wheel"
         and "manylinux" in f["filename"] and "x86_64" in f["filename"]]
if not cands:
    cands = [f for f in files if f["packagetype"] == "bdist_wheel" and "x86_64" in f["filename"]]
if not cands:
    raise SystemExit(f"no wheel for {pkg}")
print(cands[0]["url"])
PY
)" || return 1
        filename="$(basename "${url}")"
        if [[ ! -f "${wheels_dir}/${filename}" ]]; then
            _willow_log "  downloading ${filename}…"
            if command -v curl >/dev/null 2>&1; then
                curl -L --fail --progress-bar "${url}" -o "${wheels_dir}/${filename}" || return 1
            else
                wget -O "${wheels_dir}/${filename}" "${url}" || return 1
            fi
        fi
        unzip -o -q "${wheels_dir}/${filename}" -d "${root}/extract" 'nvidia/*/lib/*.so*' 2>/dev/null \
            || unzip -o -q "${wheels_dir}/${filename}" -d "${root}/extract" '*/lib/*.so*' || true
    done

    find "${root}/extract" -type f \( \
        -name 'libcublas*.so*' -o -name 'libcudart*.so*' -o -name 'libcufft*.so*' -o -name 'libnvjitlink*.so*' \
    \) -exec cp -a {} "${lib_dir}/" \;

    if [[ ! -f "${lib_dir}/libcublasLt.so.12" ]]; then
        _willow_log "ERROR: failed to install CUDA 12 compat libs"
        return 1
    fi
    _willow_log "✓ CUDA 12 compat libs ready (${lib_dir})"
    echo "${lib_dir}"
}

willow_cuda_runtime_lib_path() {
    # Prefixed path for LD_LIBRARY_PATH: cuda12-compat + sherpa + system cuda.
    local parts=()
    local compat
    compat="$(willow_default_cuda12_compat_lib_dir)"
    [[ -d "${compat}" ]] && parts+=("${compat}")
    local sherpa="${SHERPA_ONNX_LIB_DIR:-$(willow_default_cuda_lib_dir)}"
    [[ -d "${sherpa}" ]] && parts+=("${sherpa}")
    local toolkit
    if toolkit="$(willow_cuda_toolkit_lib_dir 2>/dev/null)"; then
        parts+=("${toolkit}")
    fi
    local IFS=':'
    echo "${parts[*]}"
}

willow_cuda_toolkit_lib_dir() {
    for d in \
        /opt/cuda/targets/x86_64-linux/lib \
        /opt/cuda/lib64 \
        /usr/local/cuda/lib64
    do
        if [[ -d "$d" ]]; then
            echo "$d"
            return 0
        fi
    done
    return 1
}

willow_default_cuda_dest() {
    # Prefer user cache — ~/.local/share/willow is often root-owned after pacman install.
    local cache="${XDG_CACHE_HOME:-${HOME}/.cache}/willow/sherpa-onnx-cuda"
    echo "${cache}"
}

willow_default_cuda_lib_dir() {
    echo "$(willow_default_cuda_dest)/lib"
}

# Ensure ~/.local/share/willow is writable by the current user (fixes root-owned dir from pacman).
willow_ensure_share_writable() {
    local base="${HOME}/.local/share/willow"
    mkdir -p "${HOME}/.local/share" 2>/dev/null || true
    if [[ -d "${base}" && ! -w "${base}" ]]; then
        _willow_log "→ ${base} is not writable (often root-owned from package install)"
        if command -v sudo >/dev/null 2>&1; then
            if sudo chown -R "$(id -u):$(id -g)" "${base}"; then
                _willow_log "✓ Fixed ownership of ${base}"
            else
                _willow_log "NOTE: could not chown ${base} — models/CUDA may fail under that path"
            fi
        else
            _willow_log "NOTE: run: sudo chown -R \$USER:\$USER ${base}"
        fi
    fi
    mkdir -p "${base}/models" "${base}/scripts" 2>/dev/null || true
}

willow_sherpa_version() {
    echo "${SHERPA_ONNX_VERSION:-1.13.4}"
}

# Download CUDA sherpa-onnx shared libs into DEST (directory that will contain lib/).
# Idempotent: skips if libonnxruntime is already present.
willow_setup_cuda_libs() {
    local dest="${1:-$(willow_default_cuda_dest)}"
    local version
    version="$(willow_sherpa_version)"
    local archive="sherpa-onnx-v${version}-cuda-12.x-cudnn-9.x-linux-x64-gpu.tar.bz2"
    local url="https://github.com/k2-fsa/sherpa-onnx/releases/download/v${version}/${archive}"
    local lib_dir="${dest}/lib"

    mkdir -p "${dest}" 2>/dev/null || {
        _willow_log "ERROR: cannot create ${dest}"
        return 1
    }

    if [[ -f "${lib_dir}/libonnxruntime.so" || -f "${lib_dir}/libonnxruntime.so.1" ]] \
        || ls "${lib_dir}"/libonnxruntime.so* >/dev/null 2>&1; then
        _willow_log "✓ CUDA sherpa libs already present (${lib_dir})"
        echo "${lib_dir}"
        return 0
    fi

    if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1; then
        _willow_log "ERROR: curl or wget required to download CUDA libs"
        return 1
    fi

    _willow_log "→ Downloading CUDA sherpa-onnx ${version}…"
    local tmp
    tmp="$(mktemp -d)"
    local archive_path="${tmp}/${archive}"
    local ok=0
    if command -v curl >/dev/null 2>&1; then
        curl -L --fail --progress-bar "${url}" -o "${archive_path}" && ok=1
    else
        wget -O "${archive_path}" "${url}" && ok=1
    fi
    if [[ "$ok" -ne 1 ]]; then
        rm -rf "${tmp}"
        return 1
    fi

    mkdir -p "${dest}"
    if ! tar -xjf "${archive_path}" -C "${dest}" --strip-components=1; then
        rm -rf "${tmp}"
        return 1
    fi
    rm -rf "${tmp}"

    if [[ ! -d "${lib_dir}" ]]; then
        local found
        found="$(find "${dest}" -type d -name lib 2>/dev/null | head -n1 || true)"
        if [[ -n "${found}" ]]; then
            lib_dir="${found}"
        fi
    fi

    if ! ls "${lib_dir}"/libonnxruntime.so* >/dev/null 2>&1; then
        _willow_log "ERROR: onnxruntime not found under ${dest}"
        return 1
    fi

    _willow_log "✓ CUDA sherpa libs ready (${lib_dir})"
    echo "${lib_dir}"
}

# Ensure ~/.config/willow config exists and has inference defaults.
willow_ensure_user_config() {
    local home_dir="${1:-$HOME}"
    local share_cfg="${2:-/usr/share/willow}"
    local config_dir="${home_dir}/.config/willow"
    mkdir -p "${config_dir}"

    if [[ ! -f "${config_dir}/config.json" ]]; then
        if [[ -f "${share_cfg}/config.json" ]]; then
            cp "${share_cfg}/config.json" "${config_dir}/config.json"
        elif [[ -n "${WILLOW_SOURCE_ROOT:-}" && -f "${WILLOW_SOURCE_ROOT}/config.json" ]]; then
            cp "${WILLOW_SOURCE_ROOT}/config.json" "${config_dir}/config.json"
        fi
    fi

    if [[ ! -f "${config_dir}/context.json" ]]; then
        if [[ -f "${share_cfg}/context.json" ]]; then
            cp "${share_cfg}/context.json" "${config_dir}/context.json"
        elif [[ -n "${WILLOW_SOURCE_ROOT:-}" && -f "${WILLOW_SOURCE_ROOT}/context.json" ]]; then
            cp "${WILLOW_SOURCE_ROOT}/context.json" "${config_dir}/context.json"
        fi
    fi

    # Merge inference defaults if missing (python keeps formatting simple).
    local cfg="${config_dir}/config.json"
    [[ -f "${cfg}" ]] || return 0
    if command -v python3 >/dev/null 2>&1; then
        local prefer_cuda="${3:-0}"
        python3 - "${cfg}" "${prefer_cuda}" <<'PY'
import json, sys
path, prefer = sys.argv[1], sys.argv[2] == "1"
with open(path) as f:
    cfg = json.load(f)
inf = cfg.setdefault("inference", {})
inf.setdefault("provider", "auto" if prefer else "cpu")
inf.setdefault("num_threads", 0)
inf.pop("use_whisper", None)
cfg.pop("tts", None)
sv = cfg.setdefault("speaker_verification", {})
sv["enabled"] = False
cm = cfg.setdefault("command_mode", {})
cm.setdefault("endpoint_silence", 0.5)
cm.setdefault("incomplete_hold", 1.5)
cm.setdefault("min_speech_duration", 0.1)
cm.setdefault("vad_threshold", 0.45)
cm.setdefault("whisper_pre_pad", 0.15)
cm.setdefault("preroll", 0.15)
sa = cfg.setdefault("streaming_asr", {})
sa.setdefault("endpoint_silence_typing", 0.45)
sa["endpoint_silence_command"] = cm.get("endpoint_silence", 0.5)
with open(path, "w") as f:
    json.dump(cfg, f, indent=2)
    f.write("\n")
PY
    fi
}

# True when a downloaded model directory looks usable.
# Whisper packs use tiny.en-tokens.txt (not tokens.txt); KWS uses tokens.txt + transducer onnx.
willow_model_dir_ready() {
    local dest="$1"
    local kind="${2:-}"
    [[ -d "${dest}" ]] || return 1
    case "${kind}" in
        whisper)
            ls "${dest}"/*encoder*.onnx &>/dev/null 2>&1 \
                && ls "${dest}"/*decoder*.onnx &>/dev/null 2>&1 \
                && { [[ -f "${dest}/tokens.txt" ]] || ls "${dest}"/*tokens*.txt &>/dev/null 2>&1; }
            ;;
        kws)
            [[ -f "${dest}/tokens.txt" ]] \
                && ls "${dest}"/*encoder*.onnx &>/dev/null 2>&1 \
                && ls "${dest}"/*decoder*.onnx &>/dev/null 2>&1 \
                && ls "${dest}"/*joiner*.onnx &>/dev/null 2>&1
            ;;
        *)
            ls "${dest}"/*.onnx &>/dev/null 2>&1 && [[ -f "${dest}/tokens.txt" ]]
            ;;
    esac
}

# Download speech models into MODEL_DIR (idempotent).
willow_download_models() {
    local model_dir="${1:-${HOME}/.local/share/willow/models}"
    local source_root="${2:-${WILLOW_SOURCE_ROOT:-}}"
    local base_url="https://github.com/k2-fsa/sherpa-onnx/releases/download"
    local total_steps=3

    report_progress() {
        printf 'WILLOW_PROGRESS:%s:%s:%s\n' "$1" "${total_steps}" "$2" >&2
    }

    download_tar() {
        local url="$1" dest_name="$2" step="$3"
        local dest="${model_dir}/${dest_name}"
        if willow_model_dir_ready "${dest}" "${dest_name}"; then
            report_progress "${step}" "${dest_name} already installed"
            _willow_log "✓ ${dest_name} already present"
            return 0
        fi
        report_progress "${step}" "Downloading ${dest_name}…"
        _willow_log "Downloading ${dest_name}…"
        mkdir -p "${model_dir}"
        local archive="${model_dir}/$(basename "${url}")"
        rm -f "${archive}"
        if command -v curl >/dev/null 2>&1; then
            if ! curl -L --fail --progress-bar "${url}" -o "${archive}"; then
                _willow_log "ERROR: download failed: ${url}"
                rm -f "${archive}"
                return 1
            fi
        else
            if ! wget -O "${archive}" "${url}"; then
                _willow_log "ERROR: download failed: ${url}"
                rm -f "${archive}"
                return 1
            fi
        fi
        local asize
        asize="$(stat -c%s "${archive}" 2>/dev/null || echo 0)"
        if [[ "${asize}" -lt 1000 ]]; then
            _willow_log "ERROR: ${dest_name} archive too small (${asize} bytes) — likely a failed download"
            rm -f "${archive}"
            return 1
        fi
        report_progress "${step}" "Extracting ${dest_name}…"
        rm -rf "${dest}"
        mkdir -p "${dest}"
        if ! tar -xjf "${archive}" -C "${dest}" --strip-components=1; then
            _willow_log "ERROR: extract failed for ${dest_name}"
            rm -f "${archive}"
            rm -rf "${dest}"
            return 1
        fi
        rm -f "${archive}"
        # Normalize sherpa Whisper token filename for tools that expect tokens.txt.
        if [[ "${dest_name}" == "whisper" && ! -f "${dest}/tokens.txt" ]]; then
            local tok
            tok="$(find "${dest}" -maxdepth 1 -type f -name '*tokens*.txt' | head -n1 || true)"
            if [[ -n "${tok}" ]]; then
                ln -sfn "$(basename "${tok}")" "${dest}/tokens.txt"
            fi
        fi
        if ! willow_model_dir_ready "${dest}" "${dest_name}"; then
            _willow_log "ERROR: ${dest_name} extract incomplete under ${dest}"
            ls -la "${dest}" >&2 || true
            return 1
        fi
        report_progress "${step}" "${dest_name} installed"
        _willow_log "✓ ${dest_name} installed"
    }

    download_file() {
        local url="$1" dest="$2" step="$3" label="$4"
        if [[ -f "${dest}" ]] && [[ "$(stat -c%s "${dest}" 2>/dev/null || echo 0)" -gt 1000 ]]; then
            report_progress "${step}" "${label} already installed"
            _willow_log "✓ ${label} already present"
            return 0
        fi
        report_progress "${step}" "Downloading ${label}…"
        _willow_log "Downloading ${label}…"
        mkdir -p "$(dirname "${dest}")"
        if command -v curl >/dev/null 2>&1; then
            if ! curl -L --fail --progress-bar "${url}" -o "${dest}"; then
                _willow_log "ERROR: download failed: ${url}"
                rm -f "${dest}"
                return 1
            fi
        else
            if ! wget -O "${dest}" "${url}"; then
                _willow_log "ERROR: download failed: ${url}"
                rm -f "${dest}"
                return 1
            fi
        fi
        if [[ "$(stat -c%s "${dest}" 2>/dev/null || echo 0)" -lt 1000 ]]; then
            _willow_log "ERROR: ${label} file too small — download likely failed"
            rm -f "${dest}"
            return 1
        fi
        report_progress "${step}" "${label} installed"
        _willow_log "✓ ${label} installed"
    }

    _willow_log "Installing models → ${model_dir}"
    download_tar \
        "${base_url}/kws-models/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2" \
        "kws" "1" || return 1
    download_tar \
        "${base_url}/asr-models/sherpa-onnx-whisper-tiny.en.tar.bz2" \
        "whisper" "2" || return 1
    download_file \
        "${base_url}/asr-models/silero_vad.onnx" \
        "${model_dir}/vad/silero_vad.onnx" \
        "3" "vad" || return 1

    mkdir -p "${model_dir}/kws"
    if [[ ! -f "${model_dir}/kws/keywords_raw.txt" ]]; then
        cat >"${model_dir}/kws/keywords_raw.txt" <<'EOF'
hey willow
stop typing
exit typing
normal mode
go to normal mode
exit
cancel
stop listening
go back
start typing
typing mode
begin typing
dictation mode
start dictation
EOF
    fi

    # Encode keywords when possible; else ship default encoded file.
    if [[ -f "${model_dir}/kws/bpe.model" && -f "${model_dir}/kws/tokens.txt" ]]; then
        local keywords_script=""
        for candidate in \
            "/usr/share/willow/scripts/generate-keywords.py" \
            "${source_root}/scripts/generate-keywords.py"; do
            if [[ -n "${candidate}" && -f "${candidate}" ]]; then
                keywords_script="${candidate}"
                break
            fi
        done
        if [[ -n "${keywords_script}" ]] \
            && command -v python3 >/dev/null 2>&1 \
            && python3 -c "import sentencepiece" >/dev/null 2>&1; then
            python3 "${keywords_script}" \
                --tokens "${model_dir}/kws/tokens.txt" \
                --bpe-model "${model_dir}/kws/bpe.model" \
                --input "${model_dir}/kws/keywords_raw.txt" \
                --output "${model_dir}/kws/keywords.txt" \
                && _willow_log "✓ Encoded keywords.txt" || true
        fi
        if [[ ! -f "${model_dir}/kws/keywords.txt" ]] \
            || ! grep -q '▁' "${model_dir}/kws/keywords.txt" 2>/dev/null; then
            for fallback in \
                "${source_root}/data/kws-default-keywords.txt" \
                "/usr/share/willow/kws-default-keywords.txt"; do
                if [[ -f "${fallback}" ]]; then
                    cp "${fallback}" "${model_dir}/kws/keywords.txt"
                    _willow_log "✓ Installed default encoded keywords.txt"
                    break
                fi
            done
        fi
    fi

    report_progress "${total_steps}" "All models installed"
}

# Decide how to build: prints "cuda" or "cpu" and exports SHERPA_ONNX_LIB_DIR when cuda.
willow_resolve_build_mode() {
    local cuda_dest="${1:-$(willow_default_cuda_dest)}"
    if [[ "${WILLOW_FORCE_CPU:-0}" == "1" ]]; then
        echo "cpu"
        return 0
    fi
    if willow_has_nvidia && willow_has_cuda_pkgs; then
        local lib_dir
        if ! lib_dir="$(willow_setup_cuda_libs "${cuda_dest}")"; then
            _willow_log "NOTE: CUDA lib setup failed — building CPU"
            echo "cpu"
            return 0
        fi
        export SHERPA_ONNX_LIB_DIR="${lib_dir}"
        if ! willow_has_cuda12_cublas; then
            local compat
            if compat="$(willow_setup_cuda12_compat)"; then
                export WILLOW_CUDA12_COMPAT_LIB_DIR="${compat}"
            else
                _willow_log "NOTE: could not fetch CUDA 12 compat libs — GPU EP may fall back to CPU"
            fi
        else
            export WILLOW_CUDA12_COMPAT_LIB_DIR="$(willow_default_cuda12_compat_lib_dir)"
        fi
        export LD_LIBRARY_PATH="$(willow_cuda_runtime_lib_path)${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
        echo "cuda"
        return 0
    fi
    echo "cpu"
}

willow_cargo_build() {
    local manifest="${1:?Cargo.toml path}"
    local mode="${2:-cpu}"
    if [[ "${mode}" == "cuda" ]]; then
        local lib_dir="${SHERPA_ONNX_LIB_DIR:-$(willow_default_cuda_lib_dir)}"
        if [[ ! -d "${lib_dir}" ]]; then
            _willow_log "ERROR: CUDA lib dir missing: ${lib_dir}"
            return 1
        fi
        if ! willow_has_cuda12_cublas; then
            willow_setup_cuda12_compat >/dev/null || true
        fi
        local runtime_path
        runtime_path="$(willow_cuda_runtime_lib_path)"
        # Arch ships static /usr/lib/libonnxruntime.a — without forcing dylib, lld links
        # that archive and fails on C++ ABI symbols. Prefer the shared CUDA pack.
        export SHERPA_ONNX_LIB_DIR="${lib_dir}"
        export LIBRARY_PATH="${lib_dir}${LIBRARY_PATH:+:${LIBRARY_PATH}}"
        export LD_LIBRARY_PATH="${runtime_path}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
        export RUSTFLAGS="-Lnative=${lib_dir} -Clink-arg=-Wl,-Bdynamic -Clink-arg=-l:libonnxruntime.so -Clink-arg=-l:libsherpa-onnx-c-api.so -Clink-arg=-Wl,-rpath,${lib_dir}"
        _willow_log "→ Building willow-service (release, CUDA)"
        _willow_log "  SHERPA_ONNX_LIB_DIR=${lib_dir}"
        _willow_log "  LD_LIBRARY_PATH=${runtime_path}"
        cargo build --release --no-default-features --features cuda --manifest-path "${manifest}"
    else
        # Drop CUDA link flags if a prior shell session exported them.
        unset RUSTFLAGS SHERPA_ONNX_LIB_DIR
        _willow_log "→ Building willow-service (release, CPU)"
        cargo build --release --manifest-path "${manifest}"
    fi
}
