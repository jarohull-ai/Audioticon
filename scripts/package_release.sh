#!/usr/bin/env bash
set -e

# ============================================================================
# AUDIOTICON RELEASE PACKAGER
# Kompiluje wersję release i tworzy gotowe do wysyłki paczki .tar.gz oraz .zip
# ============================================================================

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

# 1. Odczyt wersji z Cargo.toml
VERSION=$(grep -m1 '^version = ' Cargo.toml | cut -d '"' -f 2)
if [ -z "$VERSION" ]; then
    VERSION="1.0.0"
fi

ARCH=$(uname -m)
OS="linux"
PKG_NAME="audioticon-v${VERSION}-${OS}-${ARCH}"
DIST_DIR="$ROOT_DIR/dist"
PKG_DIR="$DIST_DIR/$PKG_NAME"

echo "============================================================"
echo "📦 BUDOWANIE PACZKI DYSTRYBUCYJNEJ AUDIOTICON"
echo "   Wersja: $VERSION"
echo "   Platforma: $OS-$ARCH"
echo "   Folder docelowy: $PKG_DIR"
echo "============================================================"

# 2. Kompilacja wersji release
echo "🔨 Kompilacja cargo build --release -p audioticon-cli..."
cargo build --release -p audioticon-cli

RELEASE_BIN="$ROOT_DIR/target/release/audioticon"
if [ ! -f "$RELEASE_BIN" ]; then
    echo "❌ BŁĄD: Plik binarny $RELEASE_BIN nie został utworzony!"
    exit 1
fi

# 3. Przygotowanie struktury katalogu paczki
echo "📂 Przygotowanie folderu paczki..."
rm -rf "$PKG_DIR"
mkdir -p "$PKG_DIR"

# Kopiowanie i stripowanie pliku binarnego (redukcja rozmiaru bez wpływu na działanie)
echo "✂️ Kopiowanie i usuwanie symboli debugowania (strip)..."
cp "$RELEASE_BIN" "$PKG_DIR/audioticon"
chmod +x "$PKG_DIR/audioticon"
strip "$PKG_DIR/audioticon" 2>/dev/null || true

# 4. Generowanie skryptu Start_Audioticon.sh dla Linux/macOS
cat << 'EOF' > "$PKG_DIR/Start_Audioticon.sh"
#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "============================================================"
echo "🎧 AUDIOTICON STUDIO"
echo "   Twój reżyser audiobooków z syntezą głosu AI i DSP"
echo "============================================================"
echo ""

if [ ! -x "./audioticon" ]; then
    chmod +x ./audioticon
fi

echo "🚀 Uruchamianie serwera Studio..."
echo "🌐 Zaraz otworzy się Twoja przeglądarka internetowa..."
echo "💡 Jeśli przeglądarka nie otworzy się automatycznie, wejdź na:"
echo "   http://localhost:3000"
echo ""
echo "Aby zatrzymać Audioticon, naciśnij Ctrl + C w tym oknie terminala."
echo "============================================================"
echo ""

./audioticon serve
EOF
chmod +x "$PKG_DIR/Start_Audioticon.sh"

# 5. Generowanie skryptu Start_Audioticon.bat dla Windows
cat << 'EOF' > "$PKG_DIR/Start_Audioticon.bat"
@echo off
title Audioticon Studio
cd /d "%~dp0"

echo ============================================================
echo  AUDIOTICON STUDIO
echo  Twoj rezyser audiobookow z synteza glosu AI i DSP
echo ============================================================
echo.
echo Uruchamianie serwera Studio...
echo Za chwile otworzy sie Twoja domyslna przegladarka...
echo Jesli przegladarka sie nie otworzy, wejdz na: http://localhost:3000
echo.
echo Aby zatrzymac Audioticon, nacisnij Ctrl + C w tym oknie.
echo ============================================================
echo.

if exist audioticon.exe (
    audioticon.exe serve
) else (
    echo Blad: Nie znaleziono pliku audioticon.exe.
    pause
)
EOF

# 6. Generowanie instrukcji szybkiego startu
cat << EOF > "$PKG_DIR/INSTRUKCJA_SZYBKI_START.txt"
================================================================================
                    🎧 AUDIOTICON STUDIO — SZYBKI START
================================================================================

Dziękujemy za zakup / pobranie Audioticon Studio!
Poniżej znajdziesz prostą instrukcję uruchomienia i aktywacji programu.

--------------------------------------------------------------------------------
1. URUCHOMIENIE PROGRAMU (LINUX)
--------------------------------------------------------------------------------
1. Wypakuj archiwum do dowolnego folderu na swoim dysku.
2. Otwórz terminal w tym folderze lub po prostu kliknij dwukrotnie plik:
   ./Start_Audioticon.sh
   (lub w terminalu wpisz: ./audioticon serve)
3. Aplikacja automatycznie otworzy Studio w Twojej domyślnej przeglądarce 
   pod adresem: http://localhost:3000

--------------------------------------------------------------------------------
2. AKTYWACJA LICENCJI GUMROAD (TIER SOLO / STUDIO)
--------------------------------------------------------------------------------
1. Jeśli zakupiłeś licencję Solo (\$45) lub Studio (\$99) na Gumroad, 
   otwórz Audioticon Studio w przeglądarce.
2. W prawym górnym rogu kliknij przycisk "Licencja" (lub przejdź do zakładki
   Eksportu i kliknij "Aktywuj licencję").
3. Wklej swój klucz licencyjny z e-maila potwierdzającego zakup z Gumroad
   i kliknij "Aktywuj licencję".
4. Program połączy się z Gumroad, zweryfikuje klucz i zapisze bezpieczny,
   szyfrowany token HMAC na Twoim komputerze.
5. Od tego momentu możesz pracować w 100% OFFLINE — Audioticon nie wymaga
   stałego połączenia z Internetem!

--------------------------------------------------------------------------------
3. WYMAGANIA DODATKOWE — FFmpeg (Dla eksportu M4B, FLAC i MP3)
--------------------------------------------------------------------------------
Audioticon posiada wbudowany silnik DSP w czystym Rust, lecz do bezstratnego
pakowania rozdziałów M4B (AAC) oraz FLAC z metadanymi wykorzystuje standardowy 
program 'ffmpeg'.

Jeśli przy starcie zobaczysz ostrzeżenie o braku FFmpeg, zainstaluj go jednym
poleceniem:
  • Ubuntu / Debian / Mint:
      sudo apt update && sudo apt install ffmpeg
  • Fedora:
      sudo dnf install ffmpeg
  • Arch Linux:
      sudo pacman -S ffmpeg
  • macOS:
      brew install ffmpeg
  • Windows:
      winget install Gyan.FFmpeg

--------------------------------------------------------------------------------
4. MODELE GŁOSOWE (TTS)
--------------------------------------------------------------------------------
Audioticon korzysta z nowoczesnych, otwartych modeli Piper ONNX.
Możesz zarządzać głosami bezpośrednio w zakładce "Katalog Głosów / VoiceLab":
- Dostępne są głosy polskie (Darkman, Gosia, MC Speech) oraz angielskie (Lessac).
- Modele pobierają się automatycznie jednym kliknięciem do folderu:
  ~/.local/share/audioticon/models/

--------------------------------------------------------------------------------
5. JAK STWORZYĆ PIERWSZY AUDIOBOOK?
--------------------------------------------------------------------------------
1. Otwórz edytor Studio.
2. Wklej tekst swojej książki lub przeciągnij plik (EPUB, PDF, DOCX, TXT, MD).
3. Reżyseruj tekst emotikonami! 
   - 📖 Rozdział pierwszy
   - 🛑 Pauza / oddech
   - 🤫 Szept postaci
   - 💥 Krzyk lub dynamiczna akcja
   - 📱 Rozmowa telefoniczna
4. Kliknij "Generuj Audio" na dole ekranu.
5. Sprawdź certyfikat jakości ACX / EBU R128 w zakładce "Mastering ACX".
6. Kliknij "Eksportuj Audiobook" -> wybierz format M4B z rozdziałami!

--------------------------------------------------------------------------------
Pomoc i kontakt:
W razie pytań lub uwag: jarohull.ai@gmail.com
Repozytorium: https://github.com/jarohull-ai/Audioticon
================================================================================
EOF

# Kopiowanie pełnej dokumentacji i licencji
if [ -f "$ROOT_DIR/docs/INSTRUKCJA_OBSLUGI.md" ]; then
    cp "$ROOT_DIR/docs/INSTRUKCJA_OBSLUGI.md" "$PKG_DIR/INSTRUKCJA_OBSLUGI.md"
fi
if [ -f "$ROOT_DIR/LICENSE" ]; then
    cp "$ROOT_DIR/LICENSE" "$PKG_DIR/LICENSE.txt"
fi

# 7. Tworzenie archiwów .tar.gz oraz .zip
echo "📦 Pakowanie do archiwum .tar.gz i .zip..."
cd "$DIST_DIR"
tar -czf "${PKG_NAME}.tar.gz" "$PKG_NAME"
zip -r "${PKG_NAME}.zip" "$PKG_NAME" > /dev/null

echo "🔐 Obliczanie sum kontrolnych SHA256..."
sha256sum "${PKG_NAME}.tar.gz" > "${PKG_NAME}.tar.gz.sha256"
sha256sum "${PKG_NAME}.zip" > "${PKG_NAME}.zip.sha256"

echo ""
echo "============================================================"
echo "✅ PACZKA DYSTRYBUCYJNA GOTOWA!"
echo "============================================================"
ls -lh "$DIST_DIR/${PKG_NAME}".*
echo ""
echo "Pliki gotowe do wysłania na Gumroad / GitHub Releases:"
echo "1. $DIST_DIR/${PKG_NAME}.tar.gz"
echo "2. $DIST_DIR/${PKG_NAME}.zip"
echo "============================================================"
