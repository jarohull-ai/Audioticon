#!/usr/bin/env bash
# ==============================================================================
# 🎧 Audioticon — Automated Environment Setup & Dependency Installer
# ==============================================================================
set -e

echo "============================================================"
echo "🚀 Konfiguracja środowiska Audioticon (Linux x86_64)"
echo "============================================================"

# 1. Sprawdzenie FFmpeg
if ! command -v ffmpeg &> /dev/null; then
    echo "❌ Błąd: FFmpeg nie jest zainstalowany."
    echo "Zainstaluj go poleceniem: sudo apt update && sudo apt install -y ffmpeg"
    exit 1
else
    echo "✅ FFmpeg: $(ffmpeg -version | head -n 1)"
fi

# 2. Sprawdzenie Python3 oraz python3-venv
if ! command -v python3 &> /dev/null; then
    echo "❌ Błąd: Python 3 nie jest zainstalowany."
    echo "Zainstaluj go poleceniem: sudo apt install -y python3 python3-venv python3-pip"
    exit 1
else
    echo "✅ Python: $(python3 --version)"
fi

# 3. Utworzenie dedykowanego środowiska wirtualnego Python dla Audioticon
DATA_DIR="${HOME}/.local/share/audioticon"
VENV_DIR="${DATA_DIR}/venv"
mkdir -p "${DATA_DIR}"

if [ ! -d "${VENV_DIR}" ]; then
    echo "📦 Tworzenie izolowanego środowiska wirtualnego: ${VENV_DIR}..."
    python3 -m venv "${VENV_DIR}" || {
        echo "❌ Nie udało się utworzyć venv. Upewnij się, że masz pakiet python3-venv:"
        echo "   sudo apt install -y python3-venv"
        exit 1
    }
fi

echo "📦 Instalacja/aktualizacja zależności Python (piper-tts, edge-tts)..."
"${VENV_DIR}/bin/pip" install --upgrade pip --quiet
"${VENV_DIR}/bin/pip" install "piper-tts>=1.2.0" "edge-tts>=6.1.19" --quiet
echo "✅ Pakiety Python zainstalowane w ${VENV_DIR}"

# 4. Utworzenie katalogów na modele
MODELS_DIR="${DATA_DIR}/models"
mkdir -p "${MODELS_DIR}"
echo "📁 Katalog modeli: ${MODELS_DIR}"

# 5. Pobranie modeli Piper z weryfikacją sum kontrolnych SHA-256
download_and_verify() {
    local filename="$1"
    local url="$2"
    local expected_sha="$3"
    local target="${MODELS_DIR}/${filename}"

    if [ -f "${target}" ]; then
        local current_sha
        current_sha=$(sha256sum "${target}" | awk '{print $1}')
        if [ "${current_sha}" = "${expected_sha}" ]; then
            echo "✅ Model ${filename} jest poprawny (SHA-256 zgodny)."
            return 0
        else
            echo "⚠️ Suma kontrolna ${filename} nie zgadza się. Pobieranie ponowne..."
        fi
    fi

    echo "⬇️ Pobieranie ${filename}..."
    curl -L --progress-bar -o "${target}" "${url}"

    local actual_sha
    actual_sha=$(sha256sum "${target}" | awk '{print $1}')
    if [ "${actual_sha}" != "${expected_sha}" ]; then
        echo "❌ BŁĄD INTEGRALNOŚCI: Pobrano uszkodzony plik ${filename}!"
        echo "Oczekiwano: ${expected_sha}"
        echo "Otrzymano:  ${actual_sha}"
        rm -f "${target}"
        exit 1
    fi
    echo "✅ Zweryfikowano ${filename} (SHA-256 OK)."
}

# Model Polski (pl_PL-darkman-medium)
download_and_verify "pl_PL-darkman-medium.onnx" \
    "https://huggingface.co/rhasspy/piper-voices/resolve/main/pl/pl_PL/darkman/medium/pl_PL-darkman-medium.onnx" \
    "db505438a5364e8e2e0242c4324130a873ed660dfbe8d9689cef428ffb1b645f"

download_and_verify "pl_PL-darkman-medium.onnx.json" \
    "https://huggingface.co/rhasspy/piper-voices/resolve/main/pl/pl_PL/darkman/medium/pl_PL-darkman-medium.onnx.json" \
    "70f999f11fa8ad13d3ef779041ee93c9f38be5abdbacdfad42449712fe91c81b"

# Model Angielski (en_US-lessac-medium)
download_and_verify "en_US-lessac-medium.onnx" \
    "https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/lessac/medium/en_US-lessac-medium.onnx" \
    "5efe09e69902187827af646e1a6e9d269dee769f9877d17b16b1b46eeaaf019f"

download_and_verify "en_US-lessac-medium.onnx.json" \
    "https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/lessac/medium/en_US-lessac-medium.onnx.json" \
    "efe19c417bed055f2d69908248c6ba650fa135bc868b0e6abb3da181dab690a0"

# 6. Kompilacja źródeł LUB przygotowanie gotowej paczki binarnej
if [ -f "Cargo.toml" ]; then
    if command -v cargo &> /dev/null; then
        echo "⚙️ Wykryto źródła projektu. Kompilacja Audioticon (Release)..."
        cargo build --release
        BINARY_PATH="./target/release/audioticon-cli"
    else
        echo "⚠️ Nie znaleziono polecenia 'cargo'. Pomiń kompilację lub zainstaluj Rust ze strony https://rustup.rs"
    fi
elif [ -f "./audioticon" ]; then
    chmod +x ./audioticon
    BINARY_PATH="./audioticon"
    echo "✅ Gotowa binarka 'audioticon' została przygotowana."
fi

echo "============================================================"
echo "🎉 Gotowe! Audioticon jest w pełni skonfigurowany i gotowy do pracy."
if [ -n "${BINARY_PATH}" ]; then
    echo "Uruchomienie demonstracji:"
    echo "  ${BINARY_PATH} render --demo -o demo.mp3"
else
    echo "Uruchomienie demonstracji:"
    echo "  audioticon render --demo -o demo.mp3"
fi
echo "============================================================"
