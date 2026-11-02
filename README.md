<p align="center">
  <img src="assets/logo.png" alt="Audioticon Logo" width="300" style="border-radius: 20px;" />
</p>

<h1 align="center">🎧 Audioticon</h1>

<p align="center">
  <strong>Język reżyserii audiobooków sterowany emotikonami z bezstratnym cachem zdań i filtrami DSP.</strong>
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
  <img src="https://img.shields.io/badge/Export-Apple%20M4B%20%2B%20MP3%20Chapters-purple.svg" alt="Apple M4B Export" />
  <img src="https://img.shields.io/badge/Engines-Piper%20(100%25%20Offline)%20%7C%20Edge%20Neural-blueviolet.svg" alt="Engines" />
  <img src="https://img.shields.io/badge/DSP%20Acoustics-Radio%20%7C%20Phone%20%7C%20Cathedral%20%7C%20Robot-critical.svg" alt="DSP Acoustics" />
</p>

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

Pomiary telemetryczne przeprowadzono na wielordzeniowym procesorze CPU x86_64 na zróżnicowanym materiale prozatorskim:

### 1. Zestawienie czasów renderowania

| Testowany materiał | Długość audio | Słowa / Segmenty | Silnik TTS | Zimny cache (1. uruchomienie) | Ciepły cache (edycja/re-render) | Real-Time Factor (RTF) | Przyspieszenie vs odsłuch |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Scena Showcase (DSP + Rozdziały)** | **1 min 23 s** (83 s) | 165 słów / 30 segm. | **Piper (Offline)** | **4,94 s** | **24 ms (0,024 s)** | **0,059** | **16,8×** (zimny) / **3450×** (cache) |
| **Scena Showcase (DSP + Rozdziały)** | **1 min 23 s** (83 s) | 165 słów / 30 segm. | Edge-TTS (Cloud) | 12,09 s | 21 ms (0,021 s) | 0,145 | 6,8× (zimny) / 3900× (cache) |
| **Prolog książki (Narracja ciągła)** | **3 min 16 s** (196 s) | 361 słów / 28 akap. | **Piper (Offline)** | **7,68 s** | **54 ms (0,054 s)** | **0,039** | **25,5×** (zimny) / **3600×** (cache) |
| **Rozdział 1 (Scena z dialogami)** | **8 min 15 s** (495 s) | 1022 słów / 57 akap. | **Piper (Offline)** | **18,09 s** | **139 ms (0,139 s)** | **0,0365** | **27,4×** (zimny) / **3560×** (cache) |

> 💡 **Kluczowa przewaga Cache:** Zmiana pojedynczego słowa lub emotikony w 8-minutowym rozdziale wymaga zsyntetyzowania tylko 1 segmentu (~120 ms). Cały plik audio M4B/MP3 zostaje złożony na nowo i zmasterowany w ułamku sekundy, podczas gdy w rozwiązaniach chmurowych (ElevenLabs) sam transport sieciowy trwa 300–800 ms.
>
> 🔬 **Uwaga metodologiczna:** Czasy Audioticonu obejmują **kompletny proces produkcyjny** (normalizacja językowa, parsing emotikonów, synteza TTS, filtry DSP, generowanie oddechów, 2-przebiegowy mastering radiowy EBU R128 i enkoding M4B z atomem `+faststart`). Zewnętrzne benchmarki modeli TTS (np. Kokoro, F5-TTS) podają zazwyczaj wyłącznie surowy czas inferencji neuronowej na drogich GPU bez miksowania i postprodukcji.

### 2. Zgodność ze standardami nadawczymi i ACX / Audible

Pomiary filtra `ebur128` dla wygenerowanych plików M4B i MP3:
- **Integrated Loudness ($I$):** **-19,0 do -19,6 LUFS** (Wymóg ACX: $-23$ do $-18$ LUFS $\rightarrow$ **Idealnie w celu**).
- **True Peak (Szczyt absolutny):** **-3,0 do -3,7 dBFS** (Wymóg ACX: $\le -3,0$ dBTP $\rightarrow$ **Zero przesterowań, 0 clips**).
- **Częstotliwość próbkowania:** **44,1 kHz** mono (standard studyjny i dystrybucyjny).
- **Loudness Range (LRA):** **3,2 do 5,2 LU** (Wymóg lektorski: kontrolowany zakres dynamiki do odsłuchu w podróży/słuchawkach).
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

## 📦 Szybka instalacja (Quickstart)

### Automatyczny skrypt instalacyjny (Linux):
Pobiera wymagane modele ONNX (polski `darkman` i angielski `lessac`), sprawdza zależności (`ffmpeg`, `python3`) i kompiluje silnik:
```bash
git clone https://github.com/jarohull-ai/Audioticon.git
cd Audioticon
./scripts/setup.sh
```

### Wymagania manualne:
- Rust 1.85+ (`cargo`)
- `ffmpeg` w ścieżce systemowej (`sudo apt install ffmpeg`)
- Python 3.10+ (opcjonalnie z pakietami `piper-tts` i `edge-tts`)

---

## 💻 Użycie CLI

```bash
# Wyświetlenie pomocy
cargo run --release --bin audioticon-cli -- --help

# Renderowanie audiobooka z rozdziałami do formatu Apple M4B (AAC + metadane)
cargo run --release --bin audioticon-cli -- render -i ksiazka.txt -o audiobook.m4b --title "Przykładowa Książka" --author "Autor Książki"

# Renderowanie w 100% OFFLINE (Piper TTS z modelem lokalnym)
cargo run --release --bin audioticon-cli -- render -i ksiazka.txt --provider piper -o audiobook.m4b

# Renderowanie w języku angielskim (z automatycznym doborem modelu en_US)
cargo run --release --bin audioticon-cli -- render -i ksiazka_en.txt --lang en --provider piper -o audiobook_en.m4b

# Eksport skryptu z tagami aktorskimi dla ElevenLabs v3 ([whispers], [sighs], [shouts])
cargo run --release --bin audioticon-cli -- script -i ksiazka.txt -f elevenlabs

# Eksport skryptu dla ChatTTS ([whisper], [sigh], [fast])
cargo run --release --bin audioticon-cli -- script -i ksiazka.txt -f chattts

# Podgląd osi czasu reżyserii z analizą stylu i filtrów
cargo run --release --bin audioticon-cli -- script --demo -f timeline

# Demonstracja efektów dźwiękowych (telefon, radio, katedra, robot)
cargo run --release --bin audioticon-cli -- render --demo-effects -o efekty.mp3

# Zarządzanie buforem pamięci podręcznej (cache)
cargo run --release --bin audioticon-cli -- cache
cargo run --release --bin audioticon-cli -- cache --clear
```

---

## 📝 Przykładowy scenariusz reżyserski

```text
📖 Rozdział 1: Sygnał z Północy 🛑

Stacja Polarna Alfa, dwudziesty czwarty lutego dwa tysiące dwudziestego czwartego roku. 🛑
Wiatr uderzał o metalowe poszycie hangaru z prędkością stu dwudziestu kilometrów na godzinę. ⏸️
Za oknem panowała polarna noc. 🛑
Na stole leżał stary odbiornik radiowy. 🛑

🐢 Zniszczony przez mróz... 😮‍💨 pokryty cienką warstwą szronu. 🛑
W słuchawkach rozległ się nagły szum. 🛑

📻:wojskowe Baza Główna, zgłaszam odbiór! Wykryliśmy anomalie w sektorze czwartym. Odbiór. 🛑

🐇 Centrala, powtórzcie meldunek! Czy potwierdzacie kontakt wzrokowy? 🛑

🤖 System obronny został aktywowany. Trwa analiza zagrożenia. 🛑

Cisza powróciła do hangaru. 🛑
🏛️ Kroki niosły się głuchym echem po pustej hali stacji. 🛑

📱 Połączenie jest zakłócane... ledwo słyszę dyspozytora. 🛑

Wtedy usłyszał cichy trzask tuż za ścianą. 🛑
Wstrzymał oddech. 🛑
🤫 Ktoś stoi tuż za drzwiami. 🛑
```

---

## 📄 Licencja i Prawa Autorskie

Projekt objęty podwójną licencją open-source: **MIT** lub **Apache-2.0** (zobacz pliki [LICENSE-MIT](LICENSE-MIT) oraz [LICENSE-APACHE](LICENSE-APACHE)).

Autor: **Jarosław Kuchta** ([GitHub Profile](https://github.com/jarohull-ai))  
Zgłoszenia błędów i propozycji: [GitHub Issues](https://github.com/jarohull-ai/Audioticon/issues)
