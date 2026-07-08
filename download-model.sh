#!/bin/bash
# Download sherpa-onnx speech models for Willow

set -euo pipefail

MODEL_DIR="${HOME}/.local/share/willow/models"
BASE_URL="https://github.com/k2-fsa/sherpa-onnx/releases/download"

download_tar() {
    local url="$1"
    local dest_name="$2"
    local dest="${MODEL_DIR}/${dest_name}"

    if [ -d "${dest}" ] && ls "${dest}"/*.onnx &>/dev/null 2>&1 && [ -f "${dest}/tokens.txt" ]; then
        echo "✓ ${dest_name} already present"
        return 0
    fi

    echo "Downloading ${dest_name}..."
    mkdir -p "${MODEL_DIR}"
    local archive="${MODEL_DIR}/$(basename "${url}")"
    if command -v curl &>/dev/null; then
        curl -L --progress-bar "${url}" -o "${archive}"
    else
        wget --show-progress "${url}" -O "${archive}"
    fi

    tar -xjf "${archive}" -C "${MODEL_DIR}"
    local extracted
    extracted=$(tar -tjf "${archive}" | head -1 | cut -d/ -f1)
    rm -f "${archive}"

    if [ -n "${extracted}" ] && [ -d "${MODEL_DIR}/${extracted}" ]; then
        rm -rf "${dest}"
        mv "${MODEL_DIR}/${extracted}" "${dest}"
    fi

    echo "✓ ${dest_name} installed to ${dest}"
}

echo "==================================================================="
echo "Willow - Sherpa-onnx Model Downloader"
echo "==================================================================="
echo "Installing models to: ${MODEL_DIR}"
echo ""

download_tar \
    "${BASE_URL}/kws-models/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2" \
    "kws"

download_tar \
    "${BASE_URL}/asr-models/sherpa-onnx-streaming-zipformer-en-2023-06-26.tar.bz2" \
    "streaming"

mkdir -p "${MODEL_DIR}/speaker"
if [ ! -f "${MODEL_DIR}/speaker/model.onnx" ]; then
    echo "Downloading speaker model..."
    SPEAKER_URL="${BASE_URL}/speaker-recongition-models/3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx"
    if command -v curl &>/dev/null; then
        curl -L --progress-bar "${SPEAKER_URL}" -o "${MODEL_DIR}/speaker/model.onnx"
    else
        wget --show-progress "${SPEAKER_URL}" -O "${MODEL_DIR}/speaker/model.onnx"
    fi
    echo "✓ speaker installed"
else
    echo "✓ speaker already present"
fi

mkdir -p "${MODEL_DIR}/kws"
if [ ! -f "${MODEL_DIR}/kws/keywords.txt" ]; then
    cat > "${MODEL_DIR}/kws/keywords.txt" <<'EOF'
hey willow
stop typing
exit typing
normal mode
exit
typing mode
start typing
EOF
    echo "✓ Created default keywords.txt"
fi

mkdir -p "${HOME}/.config/willow"
if [ ! -f "${HOME}/.config/willow/context.json" ] && [ -f "/usr/share/willow/context.json" ]; then
    cp /usr/share/willow/context.json "${HOME}/.config/willow/context.json"
    echo "✓ Installed default context.json"
fi

echo ""
echo "Restart the service: systemctl --user restart willow.service"
echo "Enroll your voice in: gnome-extensions prefs willow@saim"
echo "==================================================================="
