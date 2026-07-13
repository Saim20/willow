#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
EXT_SRC="$ROOT/gnome-extension/willow@saim"
LOCAL_EXT="$HOME/.local/share/gnome-shell/extensions/willow@saim"

log() {
    echo "$@" >&2
}

log "Willow dev deploy"

if ! command -v cargo >/dev/null 2>&1; then
    log "ERROR: cargo not found. Install the rust package."
    exit 1
fi

if ! command -v glib-compile-schemas >/dev/null 2>&1; then
    log "ERROR: glib-compile-schemas not found. Install glib2."
    exit 1
fi

log "→ Linking GNOME extension"
mkdir -p "$(dirname "$LOCAL_EXT")"
if [[ ! -e "$LOCAL_EXT" ]]; then
    ln -s "$EXT_SRC" "$LOCAL_EXT"
fi

log "→ Compiling GSettings schemas"
glib-compile-schemas "$EXT_SRC/schemas/"

if [[ ! -f "$EXT_SRC/schemas/gschemas.compiled" ]]; then
    log "ERROR: gschemas.compiled was not generated"
    exit 1
fi

log "→ Building willow-service (release)"
cargo build --release --manifest-path "$ROOT/service-rs/Cargo.toml"

if [[ "${WILLOW_INSTALL_SYSTEM:-0}" == "1" || "${1:-}" == "--system" ]]; then
    log "→ Installing system-wide (requires sudo)"
    sudo install -Dm755 "$ROOT/service-rs/target/release/willow-service" /usr/bin/willow-service
    dbus_service="$(mktemp)"
    sed 's|@CMAKE_INSTALL_PREFIX@|/usr|g' "$ROOT/dbus/com.github.saim.Willow.service.in" >"$dbus_service"
    sudo install -Dm644 "$dbus_service" /usr/share/dbus-1/services/com.github.saim.Willow.service
    rm -f "$dbus_service"
    sudo install -Dm644 "$ROOT/systemd/willow.service" /usr/lib/systemd/user/willow.service
    log "→ Restarting user service"
    systemctl --user daemon-reload
    systemctl --user restart willow.service --no-block
else
    bash "$ROOT/scripts/install-service-dev.sh"
fi

if command -v gnome-extensions >/dev/null 2>&1; then
    log "→ Enabling GNOME extension"
    timeout 10 gnome-extensions disable willow@saim >/dev/null 2>&1 || true
    timeout 10 gnome-extensions enable willow@saim || log "NOTE: could not enable extension automatically"
else
    log "NOTE: gnome-extensions not found; enable the extension manually"
fi

log ""
log "Deploy complete."
log "If the extension still shows ERROR, restart GNOME Shell: Alt+F2 → r → Enter"
log "System-wide install: WILLOW_INSTALL_SYSTEM=1 ./deploy-dev.sh"
