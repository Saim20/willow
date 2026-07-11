#!/bin/bash
# Download sherpa-onnx speech models for Willow

set -euo pipefail

MODEL_DIR="${HOME}/.local/share/willow/models"
BASE_URL="https://github.com/k2-fsa/sherpa-onnx/releases/download"
TOTAL_STEPS=3

report_progress() {
    local step="$1"
    local message="$2"
    # stderr is unbuffered when piped to the GNOME extension subprocess
    printf 'WILLOW_PROGRESS:%s:%s:%s\n' "${step}" "${TOTAL_STEPS}" "${message}" >&2
}

download_tar() {
    local url="$1"
    local dest_name="$2"
    local step="$3"
    local dest="${MODEL_DIR}/${dest_name}"

    if [ -d "${dest}" ] && ls "${dest}"/*.onnx &>/dev/null 2>&1 && [ -f "${dest}/tokens.txt" ]; then
        report_progress "${step}" "${dest_name} already installed"
        echo "✓ ${dest_name} already present"
        return 0
    fi

    report_progress "${step}" "Downloading ${dest_name}…"
    echo "Downloading ${dest_name}..."
    mkdir -p "${MODEL_DIR}"
    local archive="${MODEL_DIR}/$(basename "${url}")"
    if command -v curl &>/dev/null; then
        curl -L -s "${url}" -o "${archive}"
    else
        wget -q "${url}" -O "${archive}"
    fi

    report_progress "${step}" "Extracting ${dest_name}…"
    rm -rf "${dest}"
    mkdir -p "${dest}"
    tar -xjf "${archive}" -C "${dest}" --strip-components=1
    rm -f "${archive}"

    report_progress "${step}" "${dest_name} installed"
    echo "✓ ${dest_name} installed to ${dest}"
}

echo "==================================================================="
echo "Willow - Sherpa-onnx Model Downloader"
echo "==================================================================="
echo "Installing models to: ${MODEL_DIR}"
echo ""

download_tar \
    "${BASE_URL}/kws-models/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2" \
    "kws" \
    "1"

download_tar \
    "${BASE_URL}/asr-models/sherpa-onnx-streaming-zipformer-en-2023-06-26.tar.bz2" \
    "streaming" \
    "2"

mkdir -p "${MODEL_DIR}/speaker"
if [ ! -f "${MODEL_DIR}/speaker/model.onnx" ]; then
    report_progress "3" "Downloading speaker model…"
    echo "Downloading speaker model..."
    SPEAKER_URL="${BASE_URL}/speaker-recongition-models/3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx"
    if command -v curl &>/dev/null; then
        curl -L -s "${SPEAKER_URL}" -o "${MODEL_DIR}/speaker/model.onnx"
    else
        wget -q "${SPEAKER_URL}" -O "${MODEL_DIR}/speaker/model.onnx"
    fi
    report_progress "3" "Speaker model installed"
    echo "✓ speaker installed"
else
    report_progress "3" "Speaker model already installed"
    echo "✓ speaker already present"
fi

mkdir -p "${MODEL_DIR}/kws"
if [ ! -f "${MODEL_DIR}/kws/keywords_raw.txt" ]; then
    cat > "${MODEL_DIR}/kws/keywords_raw.txt" <<'EOF'
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
    echo "✓ Created default keywords_raw.txt"
fi

mkdir -p "${HOME}/.config/willow"
if [ ! -f "${HOME}/.config/willow/context.json" ] && [ -f "/usr/share/willow/context.json" ]; then
    cp /usr/share/willow/context.json "${HOME}/.config/willow/context.json"
    echo "✓ Installed default context.json"
fi

report_progress "${TOTAL_STEPS}" "All models installed"

if [ -f "${MODEL_DIR}/kws/bpe.model" ] && [ -f "${MODEL_DIR}/kws/tokens.txt" ]; then
    KEYWORDS_SCRIPT=""
    for candidate in \
        "/usr/share/willow/scripts/generate-keywords.py" \
        "$(dirname "$0")/scripts/generate-keywords.py" \
        "$(dirname "$0")/../scripts/generate-keywords.py"; do
        if [ -f "${candidate}" ]; then
            KEYWORDS_SCRIPT="${candidate}"
            break
        fi
    done

    if [ -n "${KEYWORDS_SCRIPT}" ] && [ -f "${MODEL_DIR}/kws/keywords_raw.txt" ]; then
        PYTHON=""
        if command -v python3 &>/dev/null; then
            PYTHON="python3"
        fi
        if [ -n "${PYTHON}" ] && "${PYTHON}" -c "import sentencepiece" &>/dev/null; then
            "${PYTHON}" "${KEYWORDS_SCRIPT}" \
                --tokens "${MODEL_DIR}/kws/tokens.txt" \
                --bpe-model "${MODEL_DIR}/kws/bpe.model" \
                --input "${MODEL_DIR}/kws/keywords_raw.txt" \
                --output "${MODEL_DIR}/kws/keywords.txt" \
                && echo "✓ Encoded keywords.txt"
        fi
    fi

    if [ ! -f "${MODEL_DIR}/kws/keywords.txt" ] || ! grep -q '▁' "${MODEL_DIR}/kws/keywords.txt" 2>/dev/null; then
        for fallback in \
            "$(dirname "$0")/../data/kws-default-keywords.txt" \
            "/usr/share/willow/kws-default-keywords.txt"; do
            if [ -f "${fallback}" ]; then
                cp "${fallback}" "${MODEL_DIR}/kws/keywords.txt"
                echo "✓ Installed default encoded keywords.txt"
                break
            fi
        done
    fi
fi

echo ""
echo "Restart the service: systemctl --user restart willow.service"
echo "Enroll your voice in: gnome-extensions prefs willow@saim"
echo "==================================================================="
