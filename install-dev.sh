#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")" && pwd)"
ext_src="$repo_root/gnome-extension/willow@saim"
ext_dest="$HOME/.local/share/gnome-shell/extensions/willow@saim"

echo "→ Building willow-service (release)" >&2
cargo build --release --manifest-path "$repo_root/service-rs/Cargo.toml"

bash "$repo_root/scripts/install-service-dev.sh"

echo "→ Linking GNOME extension" >&2
mkdir -p "$(dirname "$ext_dest")"
if [[ -L "$ext_dest" ]]; then
    :
elif [[ -d "$ext_dest" ]]; then
    cp -r "$ext_src"/* "$ext_dest/"
else
    ln -s "$ext_src" "$ext_dest"
fi

echo "→ Compiling GSettings schemas" >&2
glib-compile-schemas "$ext_src/schemas/"

if [[ ! -f "$ext_src/schemas/gschemas.compiled" ]]; then
    echo "ERROR: gschemas.compiled was not generated" >&2
    exit 1
fi

echo "" >&2
echo "Dev install complete." >&2
echo "Enable extension: gnome-extensions enable willow@saim" >&2
