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

willow_default_cuda_dest() {
    # Prefer user cache — ~/.local/share/willow is often root-owned after pacman install.
    local cache="${XDG_CACHE_HOME:-${HOME}/.cache}/willow/sherpa-onnx-cuda"
    echo "${cache}"
}

willow_default_cuda_lib_dir() {
    echo "$(willow_default_cuda_dest)/lib"
}

willow_default_ort_dest() {
    echo "${XDG_CACHE_HOME:-${HOME}/.cache}/willow/onnxruntime-cuda"
}

willow_cuda_toolkit_lib_dir() {
    local d
    for d in \
        /opt/cuda/targets/x86_64-linux/lib \
        /opt/cuda/lib64 \
        /usr/local/cuda/lib64 \
        /usr/lib
    do
        if [[ -e "${d}/libcublasLt.so" || -e "${d}/libcublasLt.so.13" || -e "${d}/libcublasLt.so.12" ]]; then
            echo "$d"
            return 0
        fi
    done
    return 1
}

# Major CUDA toolkit version on this machine (12, 13, …). Empty if unknown.
willow_cuda_major() {
    local ver=""
    if command -v nvcc >/dev/null 2>&1; then
        ver="$(nvcc --version 2>/dev/null | sed -n 's/.*release \([0-9]\+\)\..*/\1/p' | head -n1)"
    fi
    if [[ -z "$ver" ]]; then
        local toolkit
        if toolkit="$(willow_cuda_toolkit_lib_dir 2>/dev/null)"; then
            if [[ -e "${toolkit}/libcublasLt.so.13" ]]; then
                ver=13
            elif [[ -e "${toolkit}/libcublasLt.so.12" ]]; then
                ver=12
            fi
        fi
    fi
    echo "${ver}"
}

# True when system cuBLAS for the active toolkit is discoverable (no pip wheel hack).
willow_has_cublas_runtime() {
    local d major
    major="$(willow_cuda_major)"
    local -a dirs=()
    if [[ -n "${LD_LIBRARY_PATH:-}" ]]; then
        local IFS=':'
        # shellcheck disable=SC2206
        dirs+=(${LD_LIBRARY_PATH})
    fi
    dirs+=(
        /usr/lib
        /opt/cuda/targets/x86_64-linux/lib
        /opt/cuda/lib64
        /usr/local/cuda/lib64
    )
    for d in "${dirs[@]}"; do
        [[ -z "$d" ]] && continue
        if [[ -n "$major" && -f "${d}/libcublasLt.so.${major}" ]]; then
            return 0
        fi
        if [[ -f "${d}/libcublasLt.so.13" || -f "${d}/libcublasLt.so.12" || -f "${d}/libcublasLt.so" ]]; then
            return 0
        fi
    done
    return 1
}

willow_cuda_runtime_lib_path() {
    # Prefixed path for LD_LIBRARY_PATH: sherpa/ORT libs + system CUDA toolkit.
    local parts=()
    local sherpa="${SHERPA_ONNX_LIB_DIR:-$(willow_default_cuda_lib_dir)}"
    [[ -d "${sherpa}" ]] && parts+=("${sherpa}")
    local toolkit
    if toolkit="$(willow_cuda_toolkit_lib_dir 2>/dev/null)"; then
        parts+=("${toolkit}")
    fi
    local IFS=':'
    echo "${parts[*]}"
}

willow_download_file() {
    local url="${1:?}"
    local dest="${2:?}"
    mkdir -p "$(dirname "${dest}")"
    if [[ -f "${dest}" ]]; then
        return 0
    fi
    local tmp="${dest}.partial"
    if command -v curl >/dev/null 2>&1; then
        curl -L --fail --progress-bar "${url}" -o "${tmp}" || return 1
    else
        wget -O "${tmp}" "${url}" || return 1
    fi
    mv -f "${tmp}" "${dest}"
}

willow_ort_version() {
    echo "${WILLOW_ORT_VERSION:-1.27.1}"
}

# ONNX Runtime GPU package matched to the system CUDA major (native .so.N, no compat layer).
willow_setup_onnxruntime_gpu() {
    local major="${1:-$(willow_cuda_major)}"
    local ort_ver
    ort_ver="$(willow_ort_version)"
    local tag="cuda${major}"
    if [[ -z "$major" || "$major" -lt 12 ]]; then
        _willow_log "ERROR: need CUDA 12+ toolkit to fetch a matching ONNX Runtime GPU build"
        return 1
    fi

    local root
    root="$(willow_default_ort_dest)/${tag}-${ort_ver}"
    local stamp="${root}/.willow-ort-ready"
    if [[ -f "${stamp}" && -f "${root}/lib/libonnxruntime.so" && -f "${root}/include/onnxruntime_cxx_api.h" ]]; then
        _willow_log "✓ ONNX Runtime ${ort_ver} (${tag}) ready (${root})"
        echo "${root}"
        return 0
    fi

    local archive="onnxruntime-linux-x64-gpu_${tag}-${ort_ver}.tgz"
    local url="https://github.com/microsoft/onnxruntime/releases/download/v${ort_ver}/${archive}"
    local cache_dir
    cache_dir="$(willow_default_ort_dest)/downloads"
    local archive_path="${cache_dir}/${archive}"

    _willow_log "→ Downloading ONNX Runtime ${ort_ver} GPU (${tag})…"
    willow_download_file "${url}" "${archive_path}" || {
        _willow_log "ERROR: failed to download ${url}"
        return 1
    }

    rm -rf "${root}"
    mkdir -p "${root}"
    local tmp
    tmp="$(mktemp -d)"
    if ! tar -xzf "${archive_path}" -C "${tmp}"; then
        rm -rf "${tmp}"
        return 1
    fi
    # Archive extracts to onnxruntime-linux-x64-gpu-VERSION/ or similar.
    local extracted
    extracted="$(find "${tmp}" -mindepth 1 -maxdepth 1 -type d | head -n1)"
    if [[ -z "${extracted}" ]]; then
        rm -rf "${tmp}"
        _willow_log "ERROR: unexpected ONNX Runtime archive layout"
        return 1
    fi
    # Normalize into root/{include,lib}
    if [[ -d "${extracted}/lib" ]]; then
        mkdir -p "${root}/lib" "${root}/include"
        cp -a "${extracted}/lib"/. "${root}/lib/"
        if [[ -d "${extracted}/include" ]]; then
            cp -a "${extracted}/include"/. "${root}/include/"
        fi
    else
        rm -rf "${tmp}"
        _willow_log "ERROR: ONNX Runtime archive missing lib/"
        return 1
    fi
    rm -rf "${tmp}"

    if [[ ! -f "${root}/lib/libonnxruntime.so" ]]; then
        # Some packs only ship versioned sonames.
        local so
        so="$(ls "${root}/lib"/libonnxruntime.so.* 2>/dev/null | head -n1 || true)"
        if [[ -n "${so}" ]]; then
            ln -sfn "$(basename "${so}")" "${root}/lib/libonnxruntime.so"
        fi
    fi
    if [[ ! -f "${root}/include/onnxruntime_cxx_api.h" ]]; then
        local hdr
        hdr="$(find "${root}" -name onnxruntime_cxx_api.h 2>/dev/null | head -n1 || true)"
        if [[ -n "${hdr}" ]]; then
            mkdir -p "${root}/include"
            cp -a "$(dirname "${hdr}")"/. "${root}/include/"
        fi
    fi
    if [[ ! -f "${root}/lib/libonnxruntime.so" || ! -f "${root}/include/onnxruntime_cxx_api.h" ]]; then
        _willow_log "ERROR: ONNX Runtime install incomplete under ${root}"
        return 1
    fi
    echo "ort=${ort_ver} cuda=${major}" >"${stamp}"
    _willow_log "✓ ONNX Runtime ${ort_ver} (${tag}) ready (${root})"
    echo "${root}"
}

# Build sherpa-onnx C API against a CUDA-matched ONNX Runtime (links system cuBLAS at runtime).
willow_build_sherpa_against_ort() {
    local ort_root="${1:?}"
    local dest="${2:-$(willow_default_cuda_dest)}"
    local version
    version="$(willow_sherpa_version)"
    local major
    major="$(willow_cuda_major)"
    local build_id="sherpa-${version}-ort-$(willow_ort_version)-cuda${major}"
    local stamp="${dest}/.willow-build-id"
    local lib_dir="${dest}/lib"

    if [[ -f "${stamp}" && "$(cat "${stamp}" 2>/dev/null || true)" == "${build_id}" ]] \
        && ls "${lib_dir}"/libsherpa-onnx-c-api.so* >/dev/null 2>&1 \
        && ls "${lib_dir}"/libonnxruntime.so* >/dev/null 2>&1; then
        _willow_log "✓ CUDA sherpa libs already present (${lib_dir}) [${build_id}]"
        echo "${lib_dir}"
        return 0
    fi

    if ! command -v cmake >/dev/null 2>&1; then
        _willow_log "ERROR: cmake required to build sherpa-onnx for CUDA ${major}"
        return 1
    fi

    local src_cache
    src_cache="${XDG_CACHE_HOME:-${HOME}/.cache}/willow/src"
    local src_dir="${src_cache}/sherpa-onnx-${version}"
    local archive="${src_cache}/sherpa-onnx-${version}.tar.gz"
    local url="https://github.com/k2-fsa/sherpa-onnx/archive/refs/tags/v${version}.tar.gz"

    if [[ ! -d "${src_dir}" ]]; then
        _willow_log "→ Fetching sherpa-onnx ${version} sources…"
        willow_download_file "${url}" "${archive}" || return 1
        mkdir -p "${src_cache}"
        local tmp
        tmp="$(mktemp -d)"
        tar -xzf "${archive}" -C "${tmp}"
        rm -rf "${src_dir}"
        mv "${tmp}/sherpa-onnx-${version}" "${src_dir}"
        rm -rf "${tmp}"
    fi

    local build_dir="${src_dir}/build-willow-cuda${major}"
    rm -rf "${build_dir}"
    mkdir -p "${build_dir}" "${lib_dir}"

    _willow_log "→ Building sherpa-onnx ${version} against system CUDA ${major} + ORT…"
    (
        export SHERPA_ONNXRUNTIME_INCLUDE_DIR="${ort_root}/include"
        export SHERPA_ONNXRUNTIME_LIB_DIR="${ort_root}/lib"
        cd "${build_dir}"
        cmake \
            -DCMAKE_BUILD_TYPE=Release \
            -DBUILD_SHARED_LIBS=ON \
            -DSHERPA_ONNX_ENABLE_GPU=ON \
            -DSHERPA_ONNX_ENABLE_PYTHON=OFF \
            -DSHERPA_ONNX_ENABLE_TESTS=OFF \
            -DSHERPA_ONNX_ENABLE_CHECK=OFF \
            -DSHERPA_ONNX_ENABLE_PORTAUDIO=OFF \
            -DSHERPA_ONNX_ENABLE_WEBSOCKET=OFF \
            -DSHERPA_ONNX_ENABLE_BINARY=OFF \
            -DSHERPA_ONNX_ENABLE_TTS=OFF \
            -DSHERPA_ONNX_ENABLE_SPEAKER_DIARIZATION=OFF \
            -DSHERPA_ONNX_BUILD_C_API_EXAMPLES=OFF \
            -DSHERPA_ONNX_USE_PRE_INSTALLED_ONNXRUNTIME_IF_AVAILABLE=ON \
            "${src_dir}"
        cmake --build . -j"$(nproc 2>/dev/null || echo 4)"
    ) || {
        _willow_log "ERROR: sherpa-onnx CUDA build failed"
        return 1
    }

    # Fresh lib dir: ORT GPU + sherpa C API.
    rm -rf "${lib_dir}"
    mkdir -p "${lib_dir}"
    cp -a "${ort_root}/lib"/. "${lib_dir}/"
    find "${build_dir}" -type f \( -name 'libsherpa-onnx-c-api.so*' -o -name 'libsherpa-onnx-cxx-api.so*' \) \
        -exec cp -a {} "${lib_dir}/" \;

    if ! ls "${lib_dir}"/libsherpa-onnx-c-api.so* >/dev/null 2>&1; then
        _willow_log "ERROR: libsherpa-onnx-c-api.so missing after build"
        return 1
    fi
    if [[ ! -e "${lib_dir}/libonnxruntime.so" ]]; then
        local so
        so="$(ls "${lib_dir}"/libonnxruntime.so.* 2>/dev/null | head -n1 || true)"
        [[ -n "${so}" ]] && ln -sfn "$(basename "${so}")" "${lib_dir}/libonnxruntime.so"
    fi

    echo "${build_id}" >"${stamp}"
    _willow_log "✓ CUDA sherpa libs ready (${lib_dir}) [${build_id}]"
    echo "${lib_dir}"
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

# Prepare GPU sherpa + ONNX Runtime libs matched to the system CUDA major.
# CUDA 13+: build sherpa against Microsoft ORT cuda13 (system libcublasLt.so.13).
# CUDA 12: use sherpa's prebuilt cuda-12.x pack (native .so.12).
willow_setup_cuda_libs() {
    local dest="${1:-$(willow_default_cuda_dest)}"
    local major
    major="$(willow_cuda_major)"
    mkdir -p "${dest}" 2>/dev/null || {
        _willow_log "ERROR: cannot create ${dest}"
        return 1
    }

    if [[ -z "${major}" ]]; then
        _willow_log "ERROR: could not detect CUDA toolkit major version"
        return 1
    fi

    if [[ "${major}" -ge 13 ]]; then
        local ort_root
        if ! ort_root="$(willow_setup_onnxruntime_gpu "${major}")"; then
            return 1
        fi
        willow_build_sherpa_against_ort "${ort_root}" "${dest}"
        return $?
    fi

    # CUDA 12 path: official sherpa GPU prebuild (already targets libcublasLt.so.12).
    local version
    version="$(willow_sherpa_version)"
    local build_id="sherpa-${version}-prebuilt-cuda12"
    local stamp="${dest}/.willow-build-id"
    local lib_dir="${dest}/lib"
    if [[ -f "${stamp}" && "$(cat "${stamp}" 2>/dev/null || true)" == "${build_id}" ]] \
        && ls "${lib_dir}"/libonnxruntime.so* >/dev/null 2>&1 \
        && ls "${lib_dir}"/libsherpa-onnx-c-api.so* >/dev/null 2>&1; then
        _willow_log "✓ CUDA sherpa libs already present (${lib_dir}) [${build_id}]"
        echo "${lib_dir}"
        return 0
    fi

    local archive="sherpa-onnx-v${version}-cuda-12.x-cudnn-9.x-linux-x64-gpu.tar.bz2"
    local url="https://github.com/k2-fsa/sherpa-onnx/releases/download/v${version}/${archive}"
    local cache_dir
    cache_dir="${XDG_CACHE_HOME:-${HOME}/.cache}/willow/downloads"
    local archive_path="${cache_dir}/${archive}"

    _willow_log "→ Downloading CUDA sherpa-onnx ${version} (CUDA 12 prebuild)…"
    willow_download_file "${url}" "${archive_path}" || return 1

    local tmp
    tmp="$(mktemp -d)"
    if ! tar -xjf "${archive_path}" -C "${tmp}"; then
        rm -rf "${tmp}"
        return 1
    fi
    rm -rf "${lib_dir}"
    mkdir -p "${dest}"
    # Archive top-level usually contains lib/
    local extracted
    extracted="$(find "${tmp}" -mindepth 1 -maxdepth 1 -type d | head -n1)"
    if [[ -d "${extracted}/lib" ]]; then
        cp -a "${extracted}/." "${dest}/"
    elif [[ -d "${tmp}/lib" ]]; then
        cp -a "${tmp}/." "${dest}/"
    else
        rm -rf "${tmp}"
        _willow_log "ERROR: unexpected sherpa GPU archive layout"
        return 1
    fi
    rm -rf "${tmp}"

    if ! ls "${lib_dir}"/libonnxruntime.so* >/dev/null 2>&1; then
        _willow_log "ERROR: onnxruntime not found under ${dest}"
        return 1
    fi
    echo "${build_id}" >"${stamp}"
    _willow_log "✓ CUDA sherpa libs ready (${lib_dir}) [${build_id}]"
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
cm.setdefault("endpoint_silence", 0.30)
cm.setdefault("min_speech_duration", 0.1)
cm.setdefault("vad_threshold", 0.45)
cm.setdefault("whisper_pre_pad", 0.15)
cm.setdefault("preroll", 0.15)
cm.setdefault("session_idle", 12.0)
cm.pop("incomplete_hold", None)
cm.pop("eager_endpoint", None)
sa = cfg.setdefault("streaming_asr", {})
sa.setdefault("endpoint_silence_typing", 0.45)
sa.setdefault("rule1_min_trailing_silence", 2.4)
sa.setdefault("rule2_min_trailing_silence", 0.6)
sa.pop("endpoint_silence_command", None)
# Align rule2 with silence knob when present
try:
    sa["rule2_min_trailing_silence"] = max(0.3, min(1.5, float(cm.get("endpoint_silence", 0.30)) * 2.0))
except (TypeError, ValueError):
    pass
intent = cfg.setdefault("intent", {})
intent.setdefault("early_fire", True)
intent.setdefault("llm_fallback", False)
wf = cfg.setdefault("workflows", {})
wf.setdefault("session_timeout", 12.0)
llm = inf.setdefault("llm", {})
llm.setdefault("enabled", False)
llm.setdefault("model_path", "")
llm.setdefault("max_tokens", 64)
llm.setdefault("timeout_ms", 400)
tm = cfg.setdefault("typing_mode", {})
tm.setdefault("auto_revert", False)
tm.pop("check_recent_chars", None)
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
        kws|asr-stream)
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
    local total_steps=4

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
        "${base_url}/asr-models/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17.tar.bz2" \
        "asr-stream" "2" || return 1
    download_tar \
        "${base_url}/asr-models/sherpa-onnx-whisper-tiny.en.tar.bz2" \
        "whisper" "3" || return 1
    download_file \
        "${base_url}/asr-models/silero_vad.onnx" \
        "${model_dir}/vad/silero_vad.onnx" \
        "4" "vad" || return 1

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
        if ! willow_has_cublas_runtime; then
            _willow_log "NOTE: CUDA toolkit packages present but cuBLAS runtime not found — building CPU"
            echo "cpu"
            return 0
        fi
        local lib_dir
        if ! lib_dir="$(willow_setup_cuda_libs "${cuda_dest}")"; then
            _willow_log "NOTE: CUDA lib setup failed — building CPU"
            echo "cpu"
            return 0
        fi
        export SHERPA_ONNX_LIB_DIR="${lib_dir}"
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
        local runtime_path
        runtime_path="$(willow_cuda_runtime_lib_path)"
        # Arch ships static /usr/lib/libonnxruntime.a — without forcing dylib, lld links
        # that archive and fails on C++ ABI symbols. Prefer the shared CUDA pack.
        export SHERPA_ONNX_LIB_DIR="${lib_dir}"
        export LIBRARY_PATH="${lib_dir}${LIBRARY_PATH:+:${LIBRARY_PATH}}"
        export LD_LIBRARY_PATH="${runtime_path}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
        export RUSTFLAGS="-Lnative=${lib_dir} -Clink-arg=-Wl,-Bdynamic -Clink-arg=-l:libonnxruntime.so -Clink-arg=-l:libsherpa-onnx-c-api.so -Clink-arg=-Wl,-rpath,${lib_dir}"
        _willow_log "→ Building willow-service (release, CUDA $(willow_cuda_major))"
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
