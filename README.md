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
8. **System Modułowych Presetów Akustycznych (`audioticon.toml`)**:
   - Dowolne rozszerzanie i modyfikowanie efektów DSP przez makro-suwaki `0.0..1.0` (intensywność, pogłos, pasmo, przesterowanie, odległość, modulatory).
   - Warianty inline w tekście z natychmiastowym fallbackiem: `📻:wojskowe`, `📱[retro]`, `🏛️:kaplica`, `🤫:intymny`, `🤖:ciezki`.
9. **Standard ACX / Audible & EBU R128 Masteringu**:
   - Wbudowany 2-przebiegowy procesor dynamiki EBU R128 (`loudnorm=I=-19:TP=-3.0:LRA=7`) z organicznym generowaniem tła akustycznego studia (room tone ~ -68 dBFS) eliminujący efekt "bramki cyfrowej ciszy" w słuchawkach.

---

## 📊 Pomiary wydajności i Benchmarki (Real-World Telemetry)

Testy przeprowadzono na procesorze wielordzeniowym CPU x86_64 na oryginalnym tekście powieści *„Archiwum Pyłu”* (plik EPUB):

### 1. Zestawienie czasów renderowania

| Testowany materiał | Długość audio | Słowa / Segmenty | Silnik TTS | Zimny cache (1. uruchomienie) | Ciepły cache (edycja/re-render) | Real-Time Factor (RTF) | Przyspieszenie vs odsłuch |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Scena Showcase (DSP + Rozdziały)** | **1 min 23 s** (83 s) | 165 słów / 30 segm. | **Piper (Offline)** | **4,94 s** | **24 ms (0,024 s)** | **0,059** | **16,8×** (zimny) / **3450×** (cache) |
| **Scena Showcase (DSP + Rozdziały)** | **1 min 23 s** (83 s) | 165 słów / 30 segm. | Edge-TTS (Cloud) | 12,09 s | 21 ms (0,021 s) | 0,145 | 6,8× (zimny) / 3900× (cache) |
| **Prolog: Granica w Dorohusku** | **3 min 16 s** (196 s) | 361 słów / 28 akap. | **Piper (Offline)** | **7,68 s** | **54 ms (0,054 s)** | **0,039** | **25,5×** (zimny) / **3600×** (cache) |
| **Rozdział 1: Cięcie** | **8 min 15 s** (495 s) | 1022 słów / 57 akap. | **Piper (Offline)** | **18,09 s** | **139 ms (0,139 s)** | **0,0365** | **27,4×** (zimny) / **3560×** (cache) |

> 💡 **Kluczowa przewaga Cache:** Zmiana pojedynczego słowa lub emotikony w 8-minutowym rozdziale wymaga zsyntetyzowania tylko 1 segmentu (~120 ms). Cały plik audio M4B/MP3 zostaje złożony na nowo i zmasterowany w ułamku sekundy, podczas gdy w rozwiązaniach chmurowych (ElevenLabs) sam transport sieciowy trwa 300–800 ms.

### 2. Zgodność ze standardami nadawczymi i ACX / Audible

Pomiary filtra `ebur128` dla wygenerowanego pliku rozdziału powieści:
- **Integrated Loudness ($I$):** **-19,2 LUFS** (Wymóg ACX: $-23$ do $-18$ LUFS $\rightarrow$ **Idealnie w celu**)
- **True Peak (Szczyt absolutny):** **-3,0 dBFS** (Wymóg ACX: $\le -3,0$ dBTP $\rightarrow$ **Zero przesterowań, 0 clips**)
- **Loudness Range (LRA):** **3,2 LU** (Wymóg lektorski: kontrolowany zakres dynamiki do odsłuchu w podróży/słuchawkach)
- **Zasobożerność RAM:** ~115 MB (proces roboczy Piper z modelem ONNX darkman: 63,2 MB).

---

## 🏆 Porównanie z systemami globalnymi

| Możliwość | **Audioticon** | ElevenLabs Projects | Kokoro 82M | F5-TTS | Cartesia Sonic |
| :--- | :---: | :---: | :---: | :---: | :---: |
| **Środowisko pracy** | **100% Offline / Prywatne** (lub Cloud) | Tylko Chmura | Lokalnie / Self-hosted | Lokalnie / GPU | Tylko Chmura |
| **Reżyseria Emotikonami w tekście** | **TAK (natywna)** | NIE (brak) | NIE | NIE | NIE |
| **Bezstratny Cache zdań (SHA-256)** | **TAK (< 140 ms re-render)** | Częściowy (w UI) | Brak (wymaga kodu) | Brak | Brak |
| **Plany akustyczne DSP (radio, tel, sala)** | **TAK (zintegrowane)** | NIE (wymaga DAW) | NIE | NIE | NIE |
| **Natywny eksport M4B z rozdziałami** | **TAK (1 klik)** | NIE (paczki zip MP3) | NIE | NIE | NIE |
| **Automatyczny mastering ACX/EBU R128** | **TAK (-19 LUFS / -3 dBTP)** | Częściowy | NIE | NIE | NIE |
| **Sprzęt wymagany** | Zwykły CPU (laptop) | Przeglądarka / Internet | GPU / mocny CPU | Nowoczesne GPU | Internet |

---

## 🎛️ Konfiguracja Presetów Akustycznych (`audioticon.toml`)

Audioticon pozwala tworzyć własne brzmienia efektów i nadawać im nazwy w pliku `audioticon.toml` w katalogu projektu:

```toml
[presets."radio:wojskowe"]
base_effect = "radio"
intensity = 1.0
distortion = 0.5
bandwidth = 0.6
volume = 1.15

[presets."🏛️:kaplica"]
base_effect = "cathedral"
reverb_decay = 0.35
distance = 0.4
volume = 1.0
```

W tekście scenariusza wystarczy użyć wariantu:
```text
📻:wojskowe Uwaga patrol drugi, zgłaszam podejrzany ruch w sektorze czwartym. 🛑
🏛️:kaplica W tym pomieszczeniu panowała absolutna cisza. 🛑
```

Sprawdzenie dostępnych presetów i ich parametrów:
```bash
cargo run --bin audioticon-cli -- presets
```

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
