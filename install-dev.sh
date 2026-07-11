#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")" && pwd)"
ext_src="$repo_root/gnome-extension/willow@saim"
ext_dest="$HOME/.local/share/gnome-shell/extensions/willow@saim"
service_bin="$repo_root/service-rs/target/release/willow-service"

cargo build --release --manifest-path "$repo_root/service-rs/Cargo.toml"

if [[ ! -f "$service_bin" ]]; then
    printf 'ERROR: %s not found after build.\n' "$service_bin"
    exit 1
fi

sudo install -Dm755 "$service_bin" /usr/bin/willow-service

mkdir -p "$(dirname "$ext_dest")"
if [[ -L "$ext_dest" ]]; then
    :
elif [[ -d "$ext_dest" ]]; then
    cp -r "$ext_src"/* "$ext_dest/"
else
    ln -s "$ext_src" "$ext_dest"
fi

glib-compile-schemas "$ext_src/schemas/"

if [[ ! -f "$ext_src/schemas/gschemas.compiled" ]]; then
    printf 'ERROR: gschemas.compiled was not generated\n'
    exit 1
fi

systemctl --user restart willow.service

printf '\nDev install complete.\n'
printf 'Restart GNOME Shell (Alt+F2, type r, Enter), then run:\n'
printf '  gnome-extensions enable willow@saim\n'
