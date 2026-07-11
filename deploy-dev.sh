#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
EXT_SRC="$ROOT/gnome-extension/willow@saim"
LOCAL_EXT="$HOME/.local/share/gnome-shell/extensions/willow@saim"
SERVICE_BIN="$ROOT/service-rs/target/release/willow-service"

mkdir -p "$(dirname "$LOCAL_EXT")"
if [[ ! -e "$LOCAL_EXT" ]]; then
    ln -s "$EXT_SRC" "$LOCAL_EXT"
fi

glib-compile-schemas "$EXT_SRC/schemas/"

if [[ ! -f "$EXT_SRC/schemas/gschemas.compiled" ]]; then
    echo "ERROR: gschemas.compiled was not generated" >&2
    exit 1
fi

cargo build --release --manifest-path "$ROOT/service-rs/Cargo.toml"
sudo install -Dm755 "$SERVICE_BIN" /usr/bin/willow-service

systemctl --user restart willow.service
gnome-extensions disable willow@saim >/dev/null 2>&1 || true
gnome-extensions enable willow@saim

echo "Extension schemas compiled and service restarted."
echo "If the extension still shows ERROR, restart GNOME Shell: Alt+F2 → r → Enter"
