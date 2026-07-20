#!/usr/bin/env bash
# One-shot Willow development environment setup.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/willow-setup-lib.sh
source "$ROOT/scripts/willow-setup-lib.sh"

EXT_SRC="$ROOT/gnome-extension/willow@saim"
LOCAL_EXT="$HOME/.local/share/gnome-shell/extensions/willow@saim"
CUDA_DEST="$(willow_default_cuda_dest)"
SYSTEM_INSTALL=0

usage() {
    cat <<EOF
Usage: ./deploy-dev.sh [options]

  --system       Install binary/D-Bus/systemd system-wide (needs sudo)
  --cpu          Force CPU build even if CUDA is available
  --skip-models  Skip speech model download
  --help         Show this help

Sets up deps checks, optional CUDA sherpa libs, release build, models,
GNOME extension link, user service, and config.
EOF
}

SKIP_MODELS=0
for arg in "$@"; do
    case "$arg" in
        --system) SYSTEM_INSTALL=1 ;;
        --cpu) WILLOW_FORCE_CPU=1; export WILLOW_FORCE_CPU ;;
        --skip-models) SKIP_MODELS=1 ;;
        -h|--help) usage; exit 0 ;;
        *)
            _willow_log "Unknown option: $arg"
            usage
            exit 1
            ;;
    esac
done

log() { _willow_log "$@"; }

log "==================================================================="
log "Willow — development setup"
log "==================================================================="

need() {
    if ! command -v "$1" >/dev/null 2>&1; then
        log "ERROR: missing dependency '$1'. Install: $2"
        exit 1
    fi
}

need cargo "rust / cargo"
need glib-compile-schemas "glib2"
need curl "curl (or wget)"
need tar "tar"
need systemctl "systemd"

export WILLOW_SOURCE_ROOT="$ROOT"

# Fix root-owned data dir from prior package installs, then ensure cache for CUDA.
willow_ensure_share_writable
mkdir -p "$(dirname "$CUDA_DEST")" 2>/dev/null || true

# --- Extension ---
log "→ Linking GNOME extension"
mkdir -p "$(dirname "$LOCAL_EXT")"
if [[ ! -e "$LOCAL_EXT" ]]; then
    ln -sfn "$EXT_SRC" "$LOCAL_EXT"
elif [[ ! -L "$LOCAL_EXT" ]]; then
    log "NOTE: $LOCAL_EXT exists and is not a symlink — leaving as-is"
fi

log "→ Compiling GSettings schemas"
glib-compile-schemas "$EXT_SRC/schemas/"
if [[ ! -f "$EXT_SRC/schemas/gschemas.compiled" ]]; then
    log "ERROR: gschemas.compiled was not generated"
    exit 1
fi

# --- Build (CUDA when possible) ---
BUILD_MODE="$(willow_resolve_build_mode "$CUDA_DEST")"
willow_cargo_build "$ROOT/service-rs/Cargo.toml" "$BUILD_MODE"

# Persist LD_LIBRARY_PATH for CUDA builds in user systemd override
CUDA_LIB_DIR=""
COMPAT_LIB_DIR=""
if [[ "$BUILD_MODE" == "cuda" && -n "${SHERPA_ONNX_LIB_DIR:-}" ]]; then
    CUDA_LIB_DIR="$SHERPA_ONNX_LIB_DIR"
    COMPAT_LIB_DIR="${WILLOW_CUDA12_COMPAT_LIB_DIR:-$(willow_default_cuda12_compat_lib_dir)}"
    if [[ ! -f "${COMPAT_LIB_DIR}/libcublasLt.so.12" ]]; then
        COMPAT_LIB_DIR="$(willow_setup_cuda12_compat 2>/dev/null || true)"
    fi
    mkdir -p "$HOME/.config/willow"
    RUNTIME_PATH="$(willow_cuda_runtime_lib_path)"
    cat >"$HOME/.config/willow/sherpa-cuda.env" <<EOF
export SHERPA_ONNX_LIB_DIR=${CUDA_LIB_DIR}
export LD_LIBRARY_PATH=${RUNTIME_PATH}\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}
EOF
fi

# --- Config + models ---
prefer=0
[[ "$BUILD_MODE" == "cuda" ]] && prefer=1
willow_ensure_user_config "$HOME" "$ROOT" "$prefer"

if [[ "$SKIP_MODELS" -eq 0 ]]; then
    log "→ Speech models"
    willow_download_models "$HOME/.local/share/willow/models" "$ROOT"
else
    log "→ Skipping model download (--skip-models)"
fi

# --- Install + start service ---
if [[ "$SYSTEM_INSTALL" -eq 1 || "${WILLOW_INSTALL_SYSTEM:-0}" == "1" ]]; then
    log "→ Installing system-wide (requires sudo)"
    sudo install -Dm755 "$ROOT/service-rs/target/release/willow-service" /usr/bin/willow-service
    dbus_service="$(mktemp)"
    sed 's|@CMAKE_INSTALL_PREFIX@|/usr|g' "$ROOT/dbus/com.github.saim.Willow.service.in" >"$dbus_service"
    sudo install -Dm644 "$dbus_service" /usr/share/dbus-1/services/com.github.saim.Willow.service
    rm -f "$dbus_service"
    sudo install -Dm644 "$ROOT/systemd/willow.service" /usr/lib/systemd/user/willow.service
    if [[ -n "$CUDA_LIB_DIR" ]]; then
        sudo mkdir -p /usr/lib/willow
        sudo cp -a "$CUDA_LIB_DIR"/. /usr/lib/willow/
        sudo mkdir -p /usr/lib/systemd/user/willow.service.d
        sudo tee /usr/lib/systemd/user/willow.service.d/cuda.conf >/dev/null <<EOF
[Service]
Environment=LD_LIBRARY_PATH=/usr/lib/willow
EOF
        sudo touch /usr/share/willow/cuda-enabled
    fi
    systemctl --user daemon-reload
    systemctl --user enable --now willow.service --no-block
else
    # Pass CUDA lib path into the user unit override
    if [[ -n "$CUDA_LIB_DIR" ]]; then
        export WILLOW_CUDA_LIB_DIR="$CUDA_LIB_DIR"
        if [[ -n "${COMPAT_LIB_DIR:-}" ]]; then
            export WILLOW_CUDA12_COMPAT_LIB_DIR="$COMPAT_LIB_DIR"
        elif [[ -n "${WILLOW_CUDA12_COMPAT_LIB_DIR:-}" ]]; then
            :
        else
            export WILLOW_CUDA12_COMPAT_LIB_DIR="$(willow_default_cuda12_compat_lib_dir)"
        fi
    fi
    bash "$ROOT/scripts/install-service-dev.sh"
fi

# --- Extension enable ---
if command -v gnome-extensions >/dev/null 2>&1; then
    log "→ Enabling GNOME extension"
    timeout 10 gnome-extensions disable willow@saim >/dev/null 2>&1 || true
    timeout 10 gnome-extensions enable willow@saim || log "NOTE: enable extension manually after Shell restart"
else
    log "NOTE: gnome-extensions not found; enable willow@saim manually"
fi

log ""
log "==================================================================="
log "Deploy complete (build=${BUILD_MODE})"
log "  Extension:  $LOCAL_EXT"
log "  Models:     $HOME/.local/share/willow/models"
log "  Config:     $HOME/.config/willow/config.json"
if [[ "$BUILD_MODE" == "cuda" ]]; then
    log "  CUDA libs:  ${CUDA_LIB_DIR}"
fi
log ""
log "If the panel icon shows ERROR: Alt+F2 → r → Enter (or log out/in on Wayland)"
log "Enroll voice: gnome-extensions prefs willow@saim"
log "==================================================================="
