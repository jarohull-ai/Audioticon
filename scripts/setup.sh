#!/usr/bin/env bash
# ==============================================================================
# 🎧 Audioticon — Automated Environment Setup Script
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

# 2. Sprawdzenie Rust / Cargo
if ! command -v cargo &> /dev/null; then
    echo "❌ Błąd: Rust (cargo) nie jest zainstalowany."
    echo "Zainstaluj ze strony: https://rustup.rs"
    exit 1
else
    echo "✅ Rust: $(cargo --version)"
fi

# 3. Sprawdzenie Python3
if ! command -v python3 &> /dev/null; then
    echo "❌ Błąd: Python 3 nie jest zainstalowany."
    exit 1
else
    echo "✅ Python: $(python3 --version)"
fi

# 4. Utworzenie katalogów na modele
MODELS_DIR="${HOME}/.local/share/audioticon/models"
mkdir -p "${MODELS_DIR}"
echo "📁 Katalog modeli: ${MODELS_DIR}"

# 5. Pobranie modeli Piper (Polski + Angielski)
download_model() {
    local name="$1"
    local url_base="$2"
    if [ ! -f "${MODELS_DIR}/${name}.onnx" ]; then
        echo "⬇️ Pobieranie modelu ${name}.onnx..."
        curl -L --progress-bar -o "${MODELS_DIR}/${name}.onnx" "${url_base}/${name}.onnx"
    else
        echo "✅ Model ${name}.onnx już istnieje."
    fi

    if [ ! -f "${MODELS_DIR}/${name}.onnx.json" ]; then
        echo "⬇️ Pobieranie metadanych ${name}.onnx.json..."
        curl -L -s -o "${MODELS_DIR}/${name}.onnx.json" "${url_base}/${name}.onnx.json"
    else
        echo "✅ Konfiguracja ${name}.onnx.json już istnieje."
    fi
}

# Polski model (darkman)
download_model "pl_PL-darkman-medium" \
    "https://huggingface.co/rhasspy/piper-voices/resolve/main/pl/pl_PL/darkman/medium"

# Angielski model (lessac)
download_model "en_US-lessac-medium" \
    "https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/lessac/medium"

# 6. Kompilacja projektu w trybie release
echo "⚙️ Kompilacja Audioticon (Release)..."
cargo build --release

echo "============================================================"
echo "🎉 Gotowe! Audioticon jest w pełni skonfigurowany."
echo "Uruchomienie demonstracji:"
echo "  cargo run --release --bin audioticon-cli -- render --demo -o demo.mp3"
echo "============================================================"
