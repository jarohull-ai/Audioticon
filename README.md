# 🎧 Audioticon

> **Język reżyserii audiobooków sterowany emotikonami z bezstratnym cachem zdań i filtrami DSP.**

[![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

Audioticon rozwiązuje fundamentalny problem syntezy audiobooków: **brak intuicyjnego, czytelnego dla człowieka języka reżyserii**. Zamiast skomplikowanych i nieczytelnych dla pisarzy tagów XML/SSML (`<prosody rate="-10%" pitch="-4Hz"><break time="400ms"/>`), Audioticon pozwala reżyserować tempo, ton, pauzy, emocje i plany akustyczne za pomocą **emotikonów wstawianych bezpośrednio w tekst**.

---

## ⚡ Kluczowe cechy

1. **Partytura emocji (Emoji Grammar)**:
   - Rozdziały i struktura: `📖` (np. `📖 Rozdział 1: Początek 🛑`).
   - Pauzy: `🛑` (900 ms), `⏸️` (400 ms), `😮‍💨` (organiczny proceduralny oddech / westchnienie 380 ms).
   - Tempo i dynamika: `🐢` (wolniej, wahanie), `🐇` (szybciej, pośpiech), `📉` (obniżenie tonu), `📈` (podwyższenie tonu).
   - Emocje i akcent: `💥notes` (jednosłowne uderzenie dynamiczne), `🤫` (szept).
   - Plany akustyczne i filtry DSP: `📱` (telefon), `📻` (krótkofalówka), `📢` (megafon), `🏛️` (pogłos katedry/jaskini z ogonem rewerberacji), `🏰` (lochy), `💭` (monolog wewnętrzny), `🤖` (głos syntetyczny robota).
2. **Polski Verbalizer (Text Normalization)**:
   - Automatyczna zamiana liczb na słowa z polską odmianą gramatyczną (*152* $\rightarrow$ *sto pięćdziesiąt dwa*).
   - Zamiana dat (*24.02.2024* $\rightarrow$ *dwudziestego czwartego lutego dwa tysiące dwudziestego czwartego roku*).
   - Zamiana godzin (*15:47* $\rightarrow$ *piętnasta czterdzieści siedem*).
3. **Most do modeli ekspresyjnych (Universal Expressive Actor Bridge)**:
   - Tłumaczenie emotikonów na tagi aktorskie dla ElevenLabs v3 (`[whispers]`, `[sighs]`, `[shouts]`) oraz ChatTTS (`[whisper]`, `[sigh]`, `[shout]`).
   - Podgląd osi czasu i partytury aktorskiej z promptami stylu przez polecenie `audioticon script`.
4. **Eksport do audiobooków M4B / MP4 z rozdziałami**:
   - Generowanie plików `.m4b` (AAC w kontenerze QuickTime/MP4 z atomem `moov` na początku: `+faststart`).
   - Precyzyjne znaczniki rozdziałów i metadane (`--title`, `--author`, `--narrator`) kompatybilne z Apple Books, Audible, Smart AudioBook Player.
5. **Bezstratny Cache Zdań (Sentence-Level Fingerprinting)**:
   - Każde zdanie posiada unikalny odcisk SHA-256 (tekst + parametry + parametry DSP + wersja + tagi aktorskie).
   - Zmiana jednego przecinka lub słowa w 8-godzinnym audiobooku re-renderuje **tylko to jedno zdanie**, a reszta pobierana jest z SSD w **~20 ms**!
6. **Podwójny silnik (Cloud + 100% Local Offline)**:
   - **Microsoft Edge Neural TTS** (`pl-PL-MarekNeural`, `pl-PL-ZofiaNeural` itp.) — wysoka jakość fonetyczna z asynchronicznym renderowaniem równoległym (współbieżność: 6).
   - **Piper Neural TTS** (ONNX, 100% offline) — zintegrowany z polskim modelem lektorskim `pl_PL-darkman`. Trwały proces roboczy w RAM generuje zdanie w zaledwie **~107 ms** na CPU!
7. **Próżniowe przycinanie ciszy (Sample-Accurate Trimming)**:
   - Automatyczne usuwanie paddingu enkodera MP3/TTS na brzegach fragmentów (`silenceremove`), dzięki czemu pauzy trwają co do milisekundy dokładnie tyle, ile zaplanowano.

---

## 🚀 Wyniki wydajnościowe

Pomiary dla sceny testowej (28 zdań, ~71 s audio):

| Tryb | Pierwsze renderowanie (zimny cache) | Kolejne uruchomienie (ciepły cache) |
| :--- | :--- | :--- |
| **Piper TTS (100% Offline)** | **4,92 s** | **0,02 s (20 ms)** |
| **Edge-TTS (Parallel)** | **6,36 s** | **0,02 s (20 ms)** |

---

## 📦 Instalacja i wymagania

### Wymagania systemowe:
- Rust 1.85+ (`cargo`)
- `ffmpeg` w ścieżce systemowej (`sudo apt install ffmpeg`)
- Python 3.10+ (z pakietami `edge-tts` oraz opcjonalnie `piper-tts`)

### Budowanie:
```bash
git clone https://github.com/jarohull-ai/Audioticon.git
cd Audioticon
cargo build --release
```

---

## 💻 Użycie CLI

```bash
# Wyświetlenie pomocy
cargo run --bin audioticon-cli -- --help

# Renderowanie audiobooka z rozdziałami do formatu Apple M4B (AAC + metadane)
cargo run --bin audioticon-cli -- render -i ksiazka.txt -o audiobook.m4b --title "Archiwum Pyłu" --author "Adam Richter"

# Renderowanie w 100% OFFLINE (Piper TTS z modelem lokalnym)
cargo run --bin audioticon-cli -- render -i ksiazka.txt --provider piper -o audiobook.m4b

# Eksport skryptu z tagami aktorskimi dla ElevenLabs v3 ([whispers], [sighs], [shouts])
cargo run --bin audioticon-cli -- script -i ksiazka.txt -f elevenlabs

# Eksport skryptu dla ChatTTS ([whisper], [sigh], [fast])
cargo run --bin audioticon-cli -- script -i ksiazka.txt -f chattts

# Podgląd osi czasu reżyserii z analizą stylu i filtrów
cargo run --bin audioticon-cli -- script --demo -f timeline

# Demonstracja efektów dźwiękowych (telefon, radio, katedra, robot)
cargo run --bin audioticon-cli -- render --demo-effects -o efekty.mp3

# Zarządzanie buforem pamięci podręcznej (cache)
cargo run --bin audioticon-cli -- cache
cargo run --bin audioticon-cli -- cache --clear
```

---

## 📝 Przykładowy scenariusz reżyserski

```text
Kijów, dwudziesty czwarty lutego dwa tysiące dwudziestego czwartego roku. 🛑

Adam Richter siedział przy biurku w mieszkaniu na Obołoni w Kijowie. ⏸️
Za oknem było szaro, jak przez większość dni tej zimy. 🛑
Na parapecie stał wyschnięty aloes, 😮‍💨 którego nie podlewał od tygodni. 🛑

Na biurku leżał 💥notes. 🛑

🐢 Zniszczony od wilgoci... 😮‍💨 od dotyku... od noszenia w plecaku, który mokł w deszczu i leżał w błocie. 🛑
Okładka wyblakła, rogi starte, grzbiet pęknięty w trzech miejscach. 🛑

W środku... tysiące zapisanych zdarzeń. 🛑
📉Nazwisk. ⏸️
📉Godzin. ⏸️
📉Współrzędnych. 🛑

Nazwał to: 💥Archiwum Pyłu. 🛑

Otworzył notes na stronie z granicy. 🛑
Dwudziesty czwarty lutego dwa tysiące dwudziestego drugiego roku. ⏸️
Godzina piętnasta czterdzieści siedem. ⏸️
Dorohusk. 🛑

Przez chwilę patrzył na te cyfry. 🛑
Potem zamknął notes. 🛑
Nie musiał czytać dalej. 🛑
🤫 Pamiętał zapach. 🛑
```

---

## 📄 Licencja

Projekt objęty podwójną licencją: MIT lub Apache-2.0.
Autor: **Jarosław Kuchta** (<jarohull@gmail.com>)
