<p align="center">
  <img src="assets/logo.gif" alt="Audioticon Animated Logo" width="220" style="border-radius: 20px;" />
</p>

<h1 align="center">🎧 Audioticon</h1>

<p align="center">
  <strong>Stacja robocza i język reżyserii audiobooków sterowany emotikonami z bezstratnym cachem zdań, wielogłosowym castingiem, akustyką DSP i masteringiem parametrów technicznych ACX / Storytel (EBU R128).</strong>
</p>

<p align="center">
  <a href="https://github.com/jarohull-ai/Audioticon/actions"><img src="https://github.com/jarohull-ai/Audioticon/actions/workflows/ci.yml/badge.svg" alt="CI Build & Tests" /></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/Rust-1.85%2B-DEA584.svg?logo=rust&logoColor=white" alt="Rust 1.85+" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT%20%2F%20Apache--2.0-blue.svg" alt="License: MIT/Apache-2.0" /></a>
  <img src="https://img.shields.io/badge/RAM-~115%20MB%20Footprint-success.svg" alt="RAM Footprint" />
  <img src="https://img.shields.io/badge/i18n-Polish%20%2B%20English-informational.svg" alt="Languages: PL + EN" />
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Speed-18x--27x%20Real--Time%20(CPU)-brightgreen.svg" alt="18x-27x Real-Time" />
  <img src="https://img.shields.io/badge/Sentence%20Cache-~20ms%20Turnaround%20(SHA--256)-blue.svg" alt="SHA-256 Cache" />
  <img src="https://img.shields.io/badge/Mastering-ACX%20%2F%20Audible%20%26%20EBU%20R128-green.svg" alt="ACX / EBU R128" />
  <img src="https://img.shields.io/badge/Export-Apple%20M4B%20%7C%2024--bit%20FLAC%20%7C%20MP3-purple.svg" alt="Apple M4B & 24-bit FLAC Export" />
  <img src="https://img.shields.io/badge/Engines-Piper%20%7C%20Kokoro--82M%20%7C%20Edge%20%7C%20ElevenLabs-blueviolet.svg" alt="Engines" />
  <img src="https://img.shields.io/badge/DSP%20Acoustics-Radio%20%7C%20Phone%20%7C%20Cathedral%20%7C%20Foley%20SFX-critical.svg" alt="DSP Acoustics" />
</p>

<p align="center">
  <a href="https://jarohull-ai.github.io/Audioticon/">
    <img src="https://img.shields.io/badge/🚀%20Live%20Demo-Audioticon%20Playground-06b6d4?style=for-the-badge&logo=google-chrome&logoColor=white" alt="Live Demo Playground" />
  </a>
</p>

Audioticon rozwiązuje fundamentalny problem syntezy audiobooków i słuchowisk: **brak intuicyjnego, czytelnego dla pisarzy i lektorów języka reżyserii**. Zamiast skomplikowanych tagów XML/SSML (`<prosody rate="-10%" pitch="-4Hz"><break time="400ms"/>`) lub żmudnego cięcia ścieżek w programach DAW (Audacity, Reaper, Pro Tools), Audioticon pozwala reżyserować tempo, ton, pauzy, oddechy, obsadę głosową, efekty Foley (SFX), tła ambientowe i plany akustyczne za pomocą **emotikonów wstawianych bezpośrednio w tekst**.

> 🚀 **Wypróbuj na żywo w przeglądarce bez instalacji:** [Otwórz Audioticon Live Playground](https://jarohull-ai.github.io/Audioticon/) — wklej lub wpisz tekst ze znacznikami emoji, zobacz wizualną oś czasu audio i odsłuchaj z efektami DSP w czasie rzeczywistym!

---

## ⚡ Kluczowe możliwości

1. **Partytura emocji i akustyki (Emoji Grammar)**:
   - **Rozdziały i struktura:** `📖` (np. `📖 Rozdział 1: Początek 🛑`) — tworzy natywne znaczniki rozdziałów w kontenerach `.m4b`, `.mp3` i `.flac`.
   - **Pauzy i oddechy:** `🛑` (900 ms), `⏸️` (400 ms), `😮‍💨` (proceduralny oddech lektorski z modelowaniem formantów gardłowych 750 Hz / 1750 Hz).
   - **Tempo i dynamika:** `🐢` (wolniej, zaduma), `🐇` (szybciej, pośpiech), `📉` (obniżenie tonu), `📈` (podwyższenie tonu), `💥słowo` (jednosłowny akcent dynamiczny), `🤫` (szept).
   - **Wielogłosowy casting postaci:** `🎙️` (Narrator), `👩` (Postać kobieca), `👨` (Postać męska), `💂` / `[voice:strażnik]` (własny głos lub profil z VoiceLab).
   - **Plany akustyczne DSP:** `📱` (telefon), `📻` (krótkofalówka / radio z wariantami np. `📻:wojskowe`), `📢` (megafon), `🏛️` (pogłos katedry/hali z ogonem rewerberacji), `🏰` (lochy), `💭` (monolog wewnętrzny), `🤖` (robot).
   - **Efekty Foley (SFX) i Ambient:** `🔔` (dzwonek/telefon), `📄` (szelest kartki), `👣` (kroki), `🚪` (drzwi), `⚡` (grzmot), `🔫` (wystrzał), `💓` (bicie serca), `🕰️` (zegar), `🌧️` (tło deszczu), `🌊` (fale oceanu), `☕` (gwar kawiarni), `🌲` (las), `🔥` (ognisko).
2. **4 Silniki Syntezy Mowy (100% Offline & Cloud BYOK)**:
   - **Piper Neural TTS (`--provider piper`, 100% Offline):** lokalny silnik ONNX utrzymywany w pamięci RAM (`piper_worker.py`). Obsługuje polskie głosy lektorskie na licencji CC0 (`pl_PL-darkman-medium`, `pl_PL-gosia-medium`, `pl_PL-mc_speech-medium`) oraz ponad 140 modeli wielojęzycznych.
   - **Kokoro-82M ONNX (`--provider kokoro`, 100% Offline):** otwarty model 82M parametrów na licencji Apache-2.0 (`kokoro_worker.py`, 24 kHz $\rightarrow$ 44.1 kHz) dla języków `en-us`, `en-gb`, `es`, `fr`, `it`, `pt-br` (`af_bella`, `af_sarah`, `am_adam`, `am_michael`, `bf_emma`, `bm_george`).
   - **Microsoft Edge Neural TTS (`--provider edge`):** szybka synteza chmurowa (`pl-PL-MarekNeural`, `pl-PL-ZofiaNeural`) z współbieżnością do 6 strumieni.
   - **ElevenLabs v3 (`--provider elevenlabs`, BYOK):** natywna integracja z API ElevenLabs (Bring Your Own Key) wraz z automatycznym tłumaczeniem emotikonów na tagi ekspresyjne (`[whispers]`, `[sighs]`, `[shouts]`) i kalkulatorem kosztów przed renderem (`--estimate`).
3. **Polski Verbalizer (Text Normalization)**:
   - Automatyczna zamiana liczb na słowa z polską odmianą gramatyczną (*152* $\rightarrow$ *sto pięćdziesiąt dwa*).
   - Zamiana dat (*24.02.2024* $\rightarrow$ *dwudziestego czwartego lutego dwa tysiące dwudziestego czwartego roku*).
   - Zamiana godzin (*15:47* $\rightarrow$ *piętnasta czterdzieści siedem*), kwot walutowych i skrótów.
4. **3 Style Produkcji (`--style`) i Kadencja Oddechowa (`--cadence`)**:
   - `narrator` (czysty lektor jednoosobowy — filtruje efekty SFX/ambient i wielogłos, zachowując wyłącznie pauzy i intonację),
   - `multi-voice` (narrator + podział ról dialogowych bez efektów dźwiękowych),
   - `audio-drama` (pełne słuchowisko wielogłosowe z efektami Foley, planami DSP i tłami ambientowymi).
   - Automatyczne modelowanie naturalnego rytmu oddechowego przed długimi zdaniami (`--cadence --breath-intensity subtle|natural|off`).
5. **Bezstratny Cache Zdań (Sentence-Level SHA-256 Fingerprinting) & Crash-Resume**:
   - Każde zdanie posiada deterministyczny odcisk SHA-256 (`compute_segment_hash`) uwzględniający tekst, lektora, tempo, ton i filtr DSP.
   - Zmiana jednego przecinka w 10-godzinnej książce re-renderuje **tylko to jedno zdanie**, a pozostałe segmenty są łączone strumieniowo z dysku z 5-milisekundowym crossfadem `sin/cos` w **~20 ms** (automatyczny *Crash-Resume* i czyszczenie *LRU*).
6. **Import książek EPUB / DOCX i Reżyser AI (`audioticon import` & `audioticon direct`)**:
   - Bezpośredni import plików `.epub` oraz `.docx` z ekstrakcją spisu treści, rozdziałów i metadanych.
   - Automatyczna reżyseria surowego tekstu przez lokalny model Ollama lub chmurę LLM (Gemini / OpenAI / Anthropic) z zachowaniem 100% integralności słów oryginału (`verify_word_integrity`).
7. **VoiceLab & Dogrywki Lektorskie (Human Punch-In)**:
   - Zarządzanie profilami głosowymi, pomiar jakości akustycznej nagrań mikrofonowych (RMS, True Peak, SNR, wykrywanie przesterów).
   - Funkcja **Punch-In**: możliwość nagrania lub podmiany pojedynczego zdania własnym głosem lektora bezpośrednio w cache (`recorded_<hash>.wav`), które bezszwowo łączy się z resztą audiobooka.
   - Eksport próbek do standardowego archiwum datasetu `LJSpeech / XTTS (.zip)` lub projektowanie głosu z opisu (`audioticon voicelab design`).
8. **Eksport Apple M4B, 24-bit Studio Master FLAC, MP3 & Raport Jakości Audio Ledger**:
   - Eksport do `.m4b` (AAC + rozdziały QuickTime `+faststart`), `.flac` (16-bit lub **24-bit Studio Master**) oraz `.mp3` (ID3v2 + rozdziały).
   - **Audio Ledger (`ledger.rs`):** generuje raport techniczny z sumą SHA-256 `.certificate.json` po wykonaniu **rzeczywistego pomiaru fizycznego pliku audio** filtrem `ffmpeg -af loudnorm=print_format=json` (Integrated Loudness LUFS oraz True Peak dBFS), weryfikując zgodność parametrów z normami **Audible (ACX)** i **Storytel (EBU R128)**.
9. **Model Hub & Safe-Harbor EULA (`audioticon models hub`)**:
   - Katalog certyfikowanych modeli neuronowych z jawną klasyfikacją licencyjną (`CommercialSafe` MIT/Apache-2.0/CC0 vs `NonCommercialOnly`) i kryptograficznym rejestrem podpisanych licencji EULA (`model_receipts.json`).

---

## 📊 Pomiary wydajności i Benchmarki (Real-World Telemetry)

Pomiary telemetryczne przeprowadzono na wielordzeniowym procesorze CPU x86_64 na zróżnicowanym materiale prozatorskim:

### 1. Zestawienie czasów renderowania

| Testowany materiał | Długość audio | Słowa / Segmenty | Silnik TTS | Zimny cache (1. uruchomienie) | Ciepły cache (edycja/re-render) | Real-Time Factor (RTF) | Przyspieszenie vs odsłuch |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Scena Showcase (DSP + Rozdziały)** | **1 min 23 s** (83 s) | 165 słów / 30 segm. | **Piper (Offline)** | **4,94 s** | **24 ms (0,024 s)** | **0,059** | **16,8×** (zimny) / **3450×** (cache) |
| **Scena Showcase (DSP + Rozdziały)** | **1 min 23 s** (83 s) | 165 słów / 30 segm. | Edge-TTS (Cloud) | 12,09 s | 21 ms (0,021 s) | 0,145 | 6,8× (zimny) / 3900× (cache) |
| **Prolog książki (Narracja ciągła)** | **3 min 16 s** (196 s) | 361 słów / 28 akap. | **Piper (Offline)** | **7,68 s** | **54 ms (0,054 s)** | **0,039** | **25,5×** (zimny) / **3600×** (cache) |
| **Rozdział 1 (Scena z dialogami)** | **8 min 15 s** (495 s) | 1022 słów / 57 akap. | **Piper (Offline)** | **18,09 s** | **139 ms (0,139 s)** | **0,0365** | **27,4×** (zimny) / **3560×** (cache) |

### 2. Zgodność ze standardami nadawczymi Audible (ACX) & Storytel (EBU R128)

Każdy wyeksportowany audiobook przechodzi przez łańcuch masteringu `loudnorm=I=-19.5:TP=-3.0:LRA=11` z organicznym tłem akustycznym studia (*warm room tone* ~ -68 dBFS), a moduł **Audio Ledger** mierzy fizyczny plik wyjściowy:
- **Integrated Loudness ($I$):** **-19,0 do -19,6 LUFS** (Wymóg ACX: $-23,0$ do $-18,5$ LUFS; Storytel: $-21,0$ do $-19,0$ LUFS).
- **True Peak (Szczyt absolutny):** **$\le -3,0$ dBFS** (Zero przesterowań międzypróbkowych).
- **Częstotliwość próbkowania:** **44,1 kHz** mono (16-bit PCM/AAC/MP3 lub 24-bit Studio Master FLAC).

---

## 📦 Szybka instalacja (Quickstart)

### Automatyczny skrypt instalacyjny (Linux):
Pobiera wymagane modele ONNX (polski `darkman` i angielski `lessac`), sprawdza zależności (`ffmpeg`, `python3`) i kompiluje silnik:
```bash
git clone https://github.com/jarohull-ai/Audioticon.git
cd Audioticon
./scripts/setup.sh
```

### Wymagania manualne:
- **Rust 1.85+** (`cargo`)
- **`ffmpeg`** w ścieżce systemowej (`sudo apt install ffmpeg`)
- **Python 3.10+** z pakietami dla wybranych silników:
  - Piper Offline: `pip install piper-tts`
  - Kokoro-82M Offline: `pip install kokoro-onnx soundfile`
  - Edge TTS: `pip install edge-tts`

---

## 💻 Użycie CLI i Audioticon Studio

### 🌐 Audioticon Studio (Modular Workbench GUI)

Uruchom lokalny serwer i graficzną stację roboczą w przeglądarce jednym poleceniem:

```bash
audioticon studio
# lub: audioticon serve --port 3000
```

Audioticon otwiera modularny interfejs **Studio Workbench** (`http://127.0.0.1:3000`, klasyczny widok dostępny pod `/classic`):
- **Edytor reżyserski z paletą emotikonów:** wstawiaj znaczniki emocji, filtrów DSP, ról aktorskich i efektów SFX jednym kliknięciem lub skrótami `/`.
- **Inspektor Cache SHA-256 i Dogrywki (Punch-In):** podgląd statusu każdego zdania w pamięci podręcznej z możliwością dogrania korekty własnym głosem przez mikrofon.
- **VoiceLab & Model Hub:** nagrywaj próbki głosu z analizą SNR/RMS, pobieraj modele offline (Piper PL, Kokoro-82M) i konfiguruj klucze BYOK (ElevenLabs).
- **Kalkulator kosztów i eksport 1-klik:** wyceniaj koszt przed syntezą i eksportuj gotowe książki do `.m4b`, `.flac` (24-bit) lub `.mp3` wraz z certyfikatem `.certificate.json`.

### ⌨️ Kompletny przegląd poleceń CLI

```bash
# 1. Renderowanie audiobooka (M4B / 24-bit FLAC / MP3 / WAV)
audioticon render -i ksiazka.epub -o audiobook.m4b --provider piper --voice gosia
audioticon render -i ksiazka.txt -o master.flac --bit-depth 24 --style audio-drama --cadence
audioticon render -i book_en.txt --lang en --provider kokoro --voice af_bella -o book_en.m4b

# 2. Kalkulator czasu, cache hits i kosztów API przed syntezą
audioticon estimate -i ksiazka.epub --provider elevenlabs
audioticon render -i ksiazka.txt --estimate --provider piper

# 3. Import książek (.epub / .docx / .txt) i automatyczna reżyseria AI (LLM / Ollama)
audioticon import -i ksiazka.epub -o scenariusz.txt --chapters-dir ./rozdzialy
audioticon direct -i surowy_tekst.txt -o wyrezyserowany.txt --provider ollama --model Bielik-11B-v2.3-Instruct

# 4. Menedżer modeli offline (Piper + Europe & Americas Neural Model Hub)
audioticon models list --lang pl
audioticon models hub
audioticon models download gosia
audioticon models download kokoro-82m-int8
audioticon models accept omnivoice-zeroshot

# 5. VoiceLab — profile głosowe, jakość próbek, dogrywki Punch-In i projektowanie głosu
audioticon voicelab list
audioticon voicelab create --name "Lektor Marek" --gender male --lang pl --sample nagranie.wav
audioticon voicelab punch-in --id lektor-marek --hash 4309872b --audio korekta_zdania.wav
audioticon voicelab export lektor-marek -o dataset_xtts.zip
audioticon voicelab design "Głęboki, spokojny męski głos radiowy z lekką chrypką" --name "Detektyw" --save

# 6. Biblioteka efektów Foley / SFX, własne emotikony i presety DSP
audioticon sfx list
audioticon sfx catalog
audioticon emojis list
audioticon emojis add --emoji "🐉" --pause 600 --pitch -12 --rate -15 --filter cathedral
audioticon presets

# 7. Konfiguracja kluczy API (BYOK), cache LRU i licencje Audio Ledger
audioticon providers list
audioticon providers set elevenlabs --key <TWÓJ_KLUCZ_API>
audioticon cache
audioticon cache --prune 5GB
audioticon wallet balance
```

---

## 🎛️ Konfiguracja Projektu i Presetów (`audioticon.toml`)

W katalogu projektu możesz umieścić plik `audioticon.toml`, aby zdefiniować domyślne metadane książki oraz własne warianty filtrów DSP:

```toml
[project]
title = "Sygnał z Północy"
author = "Jarosław Kuchta"
narrator = "Darkman"
year = "2026"

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

W tekście scenariusza wystarczy użyć wariantu inline:
```text
📻:wojskowe Uwaga patrol drugi, zgłaszam podejrzany ruch w sektorze czwartym. 🛑
🏛️:kaplica W tym pomieszczeniu panowała absolutna cisza. 🛑
```

---

## 🏗️ Architektura Projektu (Rust Workspace)

```text
audioticon/
├── crates/
│   ├── core/                     # Biblioteka audioticon-core
│   │   └── src/
│   │       ├── audio.rs          # Silnik renderujący, Piper/Edge/ElevenLabs, crossfade 5ms, mastering EBU R128
│   │       ├── kokoro.rs         # Mostek Kokoro-82M ONNX (kokoro_worker.py) + resampling 24kHz -> 44.1kHz
│   │       ├── parser.rs         # Parser gramatyki emotikonowej i 3 stylów produkcji
│   │       ├── normalizer.rs     # Polski Verbalizer (liczby, daty, godziny, waluty, skróty)
│   │       ├── cadence.rs        # Modelowanie naturalnej kadencji i pauz oddechowych
│   │       ├── presets.rs        # Silnik parametrycznych filtrów DSP i konfiguracji audioticon.toml
│   │       ├── sfx.rs            # Biblioteka efektów Foley / SFX i proceduralnych teł ambientowych
│   │       ├── cache.rs          # SHA-256 sentence fingerprinting i czyszczenie LRU
│   │       ├── import.rs         # Importer książek EPUB (ZIP/XHTML) oraz DOCX (XML)
│   │       ├── autodirect.rs     # Automatyczny Reżyser AI (LLM / Ollama) z weryfikacją integralności słów
│   │       ├── voicelab.rs       # Profile głosowe, analiza SNR/RMS/Peak, Punch-In, eksport LJSpeech ZIP
│   │       ├── voice_design.rs   # Kreator głosów z opisu tekstowego (Voice Design API)
│   │       ├── models.rs         # Menedżer i downloader modeli Piper oraz Model Hub
│   │       ├── model_hub.rs      # Katalog Safe-Harbor i kryptograficzne kwity licencyjne EULA
│   │       ├── ledger.rs         # Certyfikacja ACX/Storytel z fizycznym pomiarem LUFS i True Peak (FFmpeg)
│   │       ├── estimator.rs      # Kalkulator czasu trwania, cache hits i kosztów API
│   │       ├── credentials.rs    # Bezpieczny magazyn kluczy BYOK (0600 permissions)
│   │       └── wallet.rs         # Lokalny menedżer licencji Studio i voucherów HMAC-SHA256
│   └── cli/                      # Aplikacja audioticon-cli + serwer HTTP Studio Workbench
├── docs/                         # Interfejsy Web Studio (Modular Workbench & Classic Playground)
├── scripts/                      # Skrypty instalacyjne i narzędziowe
└── Cargo.toml                    # Manifest Workspace
```

---

## 📄 Licencja i Prawa Autorskie

Projekt objęty podwójną licencją open-source: **MIT** lub **Apache-2.0** (zobacz pliki [LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE) oraz [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)).

Autor: **Jarosław Kuchta** ([GitHub Profile](https://github.com/jarohull-ai))  
Dokumentacja Wiki: [Audioticon GitHub Wiki](https://github.com/jarohull-ai/Audioticon/wiki)  
Zgłoszenia błędów i propozycji: [GitHub Issues](https://github.com/jarohull-ai/Audioticon/issues)
