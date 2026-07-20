#!/usr/bin/env bash
# Download sherpa-onnx speech models for Willow (CLI + GNOME prefs).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
# When installed as /usr/bin/willow-download-model, lib lives under /usr/share/willow.
if [[ -f "$ROOT/scripts/willow-setup-lib.sh" ]]; then
    # shellcheck source=scripts/willow-setup-lib.sh
    source "$ROOT/scripts/willow-setup-lib.sh"
    SOURCE_ROOT="$ROOT"
elif [[ -f /usr/share/willow/scripts/willow-setup-lib.sh ]]; then
    # shellcheck disable=SC1091
    source /usr/share/willow/scripts/willow-setup-lib.sh
    SOURCE_ROOT="/usr/share/willow"
else
    echo "ERROR: willow-setup-lib.sh not found" >&2
    exit 1
fi

MODEL_DIR="${HOME}/.local/share/willow/models"

echo "==================================================================="
echo "Willow — speech model download"
echo "==================================================================="

willow_download_models "$MODEL_DIR" "$SOURCE_ROOT"
willow_ensure_user_config "$HOME" "$SOURCE_ROOT" "$(willow_has_nvidia && echo 1 || echo 0)"

echo ""
echo "Restart the service: systemctl --user restart willow.service"
echo "Enroll your voice in: gnome-extensions prefs willow@saim"
echo "==================================================================="
