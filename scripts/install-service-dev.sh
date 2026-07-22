#!/bin/bash
# Install willow-service for local development without root.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SERVICE_BIN="$ROOT/service-rs/target/release/willow-service"
INSTALL_PREFIX="${WILLOW_INSTALL_PREFIX:-$HOME/.local}"
BIN_DIR="$INSTALL_PREFIX/bin"
DBUS_DIR="$HOME/.local/share/dbus-1/services"
SYSTEMD_DIR="$HOME/.config/systemd/user"
OVERRIDE_DIR="$SYSTEMD_DIR/willow.service.d"

log() {
    echo "$@" >&2
}

if [[ ! -f "$SERVICE_BIN" ]]; then
    log "ERROR: $SERVICE_BIN not found. Build first:"
    log "  cargo build --release --manifest-path $ROOT/service-rs/Cargo.toml"
    exit 1
fi

log "→ Installing binary to $BIN_DIR/willow-service"
mkdir -p "$BIN_DIR" "$DBUS_DIR" "$SYSTEMD_DIR" "$OVERRIDE_DIR"
install -Dm755 "$SERVICE_BIN" "$BIN_DIR/willow-service"

SCRIPTS_DIR="$HOME/.local/share/willow/scripts"
if mkdir -p "$SCRIPTS_DIR" 2>/dev/null; then
    log "→ Installing keyword encoder to $SCRIPTS_DIR/generate-keywords.py"
    install -Dm755 "$ROOT/scripts/generate-keywords.py" "$SCRIPTS_DIR/generate-keywords.py"
else
    log "NOTE: could not install keyword encoder to $SCRIPTS_DIR (using WILLOW_SOURCE_ROOT)"
fi

log "→ Installing D-Bus activation service"
cat >"$DBUS_DIR/com.github.saim.Willow.service" <<EOF
[D-BUS Service]
Name=com.github.saim.Willow
Exec=$BIN_DIR/willow-service
User=
SystemdService=willow.service
EOF

log "→ Installing user systemd unit"
sed "s|ExecStart=/usr/bin/willow-service|ExecStart=$BIN_DIR/willow-service|" \
    "$ROOT/systemd/willow.service" >"$SYSTEMD_DIR/willow.service"

log "→ Updating dev override"
OVERRIDE_ENV="Environment=WILLOW_SOURCE_ROOT=$ROOT
Environment=PATH=$BIN_DIR:/usr/bin:/usr/local/bin
TimeoutStopSec=15"
if [[ -n "${WILLOW_CUDA_LIB_DIR:-}" ]]; then
    LD_PATH="${WILLOW_CUDA_LIB_DIR}"
    if [[ -n "${WILLOW_CUDA_TOOLKIT_LIB_DIR:-}" ]]; then
        LD_PATH="${LD_PATH}:${WILLOW_CUDA_TOOLKIT_LIB_DIR}"
    fi
    OVERRIDE_ENV="${OVERRIDE_ENV}
Environment=SHERPA_ONNX_LIB_DIR=${WILLOW_CUDA_LIB_DIR}
Environment=LD_LIBRARY_PATH=${LD_PATH}"
fi
cat >"$OVERRIDE_DIR/override.conf" <<EOF
[Service]
ExecStart=
ExecStart=$BIN_DIR/willow-service
${OVERRIDE_ENV}
EOF

log "→ Reloading systemd user daemon"
systemctl --user daemon-reload
systemctl --user enable willow.service >/dev/null 2>&1 || true

log "→ Stopping existing service (if any)"
if systemctl --user is-active --quiet willow.service 2>/dev/null; then
    systemctl --user stop willow.service &
    stop_pid=$!
    for _ in $(seq 1 20); do
        if ! kill -0 "$stop_pid" 2>/dev/null; then
            break
        fi
        sleep 0.25
    done
    if kill -0 "$stop_pid" 2>/dev/null; then
        log "   stop is taking longer than expected; waiting..."
        wait "$stop_pid" || true
    fi
fi

log "→ Starting willow.service"
systemctl --user start willow.service --no-block

log "→ Waiting for service to become active"
for i in $(seq 1 30); do
    if systemctl --user is-active --quiet willow.service; then
        log "✓ willow.service is active"
        log "  Binary:     $BIN_DIR/willow-service"
        log "  D-Bus:      $DBUS_DIR/com.github.saim.Willow.service"
        log "  Systemd:    $SYSTEMD_DIR/willow.service"
        exit 0
    fi
    if systemctl --user is-failed --quiet willow.service; then
        log "ERROR: willow.service failed to start"
        journalctl --user -u willow.service -n 30 --no-pager >&2 || true
        exit 1
    fi
    sleep 1
done

log "ERROR: willow.service did not become active within 30s"
journalctl --user -u willow.service -n 30 --no-pager >&2 || true
exit 1
