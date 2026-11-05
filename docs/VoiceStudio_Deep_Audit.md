# 🔬 Chirurgiczny Audyt Architektury VoiceStudio (`debpalash/VoiceStudio`)

> **Lokalizacja zbadana:** `/home/jaro/Desktop/tymczasowy/VoiceStudio`  
> **Status:** Klon lokalny (`depth 1`), pełna analiza kodu źródłowego (Python Backend, Android Kotlin Jetpack Compose, Native Rust Bridge).

---

## 📑 Spis Treści
1. [Architektura Systemu i Odpowiedzialność Warstw](#1-architektura-systemu)
2. [Mechanizm Bezpieczeństwa Licencyjnego (Safe-Harbor / Model EULA)](#2-mechanizm-bezpieczeństwa-licencyjnego)
3. [Katalog Modeli (70 Zbadanych Modeli)](#3-katalog-modeli)
4. [Silnik Audiobooków i Przetwarzania Długometrażowego](#4-silnik-audiobooków)
5. [Akustyka Studyjna i DSP (Unikanie Cyfrowej Ciszy)](#5-akustyka-studyjna-i-dsp)
6. [Zero-Shot Voice Cloning (Ekstrakcja z 5–15s próbek)](#6-zero-shot-voice-cloning)
7. [Mobilny Runtime Android (Kotlin Jetpack Compose)](#7-mobilny-runtime-android)
8. [Mapa Drogowa Transferu do Audioticonu (w czystym Rust)](#8-mapa-drogowa-transferu)

---

## 1. Architektura Systemu

VoiceStudio składa się z trzech odrębnych światów:
```text
┌─────────────────────────────────────────────────────────────┐
│ 1. DESKTOP GUI (Electron + Bun + TypeScript)                │
│    oraz MOBILE APP (Android Kotlin + Jetpack Compose)       │
├─────────────────────────────────────────────────────────────┤
│ 2. BACKEND SERVICES & ENGINES (Python FastAPI + PyTorch/ONNX)│
│    • 18 dedykowanych silników (GGUF, Sherpa, Whisper, itp.) │
│    • 80+ wyspecjalizowanych mikrousług przetwarzania audio  │
├─────────────────────────────────────────────────────────────┤
│ 3. NATIVE DESKTOP BRIDGE (Rust / Cargo)                     │
│    • Dostęp do skrótów systemowych, schowka i okien OS      │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. Mechanizm Bezpieczeństwa Licencyjnego (Safe-Harbor / Model EULA)

Zbadano pliki:
- `backend/services/model_licenses.py` (581 linii)
- `backend/services/model_acceptance.py`
- `mobile/android/.../models/ModelsScreen.kt`

### Jak działa ich mechanizm "umycia rąk":
1. **Rejestr Licencji (`model_licenses.json`):**
   Każdy model ma zdefiniowaną kategorię (`commercial` vs `noncommercial`), link do źródła, hash rewizji oraz listę ograniczeń:
   ```json
   {
     "id": "k2-fsa/OmniVoice",
     "license": "CC-BY-NC; Higgs Audio 2; Meta Llama 3",
     "commercial_use": false,
     "review_status": "noncommercial",
     "audit_observations": {
       "commercial_inference": { "status": "noncommercial" },
       "output_terms": { "ownership_or_all_rights_cleared": false }
     }
   }
   ```
2. **Kryptograficzny Kwit Akceptacji (Receipt):**
   Gdy użytkownik akceptuje warunki na telefonie lub desktopie, aplikacja generuje SHA-256 fingerprint licencji i zapisuje lokalny kwit:
   ```kotlin
   receipt(accepted=true, time=System.currentTimeMillis(), version=appVersion, fingerprint=termsFingerprint)
   ```
3. **Izolacja Prawna Twórcy:**
   Aplikacja nigdy nie wysyła zapytania na komercyjny serwer z modelem `CC-BY-NC` bez obecności tego lokalnego kwitu. Twórca oprogramowania nie jest stroną licencji modelu — jest nią wyłącznie użytkownik końcowy.

---

## 3. Katalog Modeli (70 Modeli)

Zbadano 70 modeli zgłoszonych w ich bazie. Najważniejsze perełki, które możemy wdrożyć w Audioticonie:

| Model | Format | Licencja | Przydatność w Audioticonie |
|---|---|---|---|
| **`Kokoro-82M` (v1_1 int8)** | ONNX / GGUF | **Apache-2.0** | **POZYCJA OBOWIĄZKOWA.** 82M parametrów, jakość ElevenLabs, działa na CPU 15x szybciej niż czas rzeczywisty, 100% legalny komercyjnie. |
| **`OmniVoice` (k2-fsa)** | GGUF | CC-BY-NC | Zero-shot cloning z 3s próbki. Świetny do użytku domowego/indywidualnego. |
| **`CosyVoice 3` (0.5B)** | GGUF / ONNX | **Apache-2.0** | Wybitna ekspresja emocjonalna i wielojęzyczność. |
| **`faster-whisper-tiny/base`** | CType / ONNX | **MIT** | Precyzyjny aligner (wyznaczanie początku i końca każdego słowa na osi czasu). |
| **`pyannote/speaker-diarization-3.1`** | PyTorch / ONNX | **MIT** | Automatyczne rozpoznawanie mówców i przypisywanie postaci ze słuchowiska. |
| **`Supertonic-3`** | ONNX int8 | OpenRAIL-M | Lekki silnik ekspresyjny na urządzenia mobilne. |

---

## 4. Silnik Audiobooków i Przetwarzania Długometrażowego

Zbadano pliki:
- `backend/services/audiobook.py`
- `backend/services/longform_render.py`
- `mobile/android/.../BookChapterTrack.kt`
- `mobile/android/.../AudioFiles.kt`

### Kluczowe rozwiązania architektoniczne:
1. **Dyskowe bufory PCM zamiast pamięci RAM (`PcmFile`):**
   - 10-godzinny audiobook w float32 zająłby kilka gigabajtów RAM-u.
   - VoiceStudio używa `Disk-backed mono float PCM` z bezpośrednim czytaniem blokowym (`RandomAccessFile`), więc aplikacja zużywa stałe ~120 MB RAM-u bez względu na długość książki.
2. **Natywne atomy MP4/M4B dla Apple Books (`BookChapterTrack.kt`):**
   - Bezpośrednie wstrzykiwanie atomów QuickTime (`trak`, `tkhd`, `mdia`, `gmhd`, `stbl`, `co64`) dla bezstratnych rozdziałów M4B zgodnych z iOS i Nero.
3. **Content-Addressed Chapter Cache (`longform_render.py`):**
   - Każdy rozdział ma unikalny hash SHA-256 z tekstu, głosu, parametrów tempa i nasiona losowego (`seed`).
   - Mechanizm blokad `CacheLease` uniemożliwia uszkodzenie pliku przy równoległym renderowaniu.

---

## 5. Akustyka Studyjna i DSP (Unikanie Cyfrowej Ciszy)

Zbadano plik `backend/services/audio_dsp.py` oraz `onset_align.py`:

1. **Syntetyczny szum tła studyjnego (`_room_tone` & `fill_gaps`):**
   - Standard ACX/Audible zabrania wstawiania absolutnej cyfrowej ciszy (0.000 Hz) między zdaniami, bo brzmi to nienaturalnie i powoduje trzaski w uszach słuchacza.
   - VoiceStudio mierzy profil szumu otoczenia lektora (`_room_tone_profile`) i wypełnia pauzy mikroskopijnym, niesłyszalnym szumem tła o tej samej barwie.
2. **Wykrywanie ataku mowy (`detect_speech_onset` & `snap_segments_to_speech`):**
   - Cięcia i crossfade nie następują w losowych miejscach, lecz są przyciągane do pierwszego impulsu krtaniowego (onset), zapobiegając ucinaniu spółgłosek bezdźwięcznych (*p, t, k*).
3. **WSOLA Time-Stretching (`NarrationAudio.speed`):**
   - Płynna zmiana tempa (0.5x – 2.0x) z zachowaniem wysokości dźwięku (pitch-preserving overlap-add).

---

## 6. Zero-Shot Voice Cloning (Ekstrakcja z 5–15s próbek)

Zbadano plik `backend/services/speaker_clone.py`:

- **Zasada złotego okna:**
  - Poniżej 3.0s — próbka za krótka, klon staje się płaski i metaliczny.
  - Optymalne okno: **8.0 sekund** czystej mowy.
  - Powyżej 15.0s — niepotrzebne obciążenie kontekstu pamięciowego.
- **Purity Guards (Filtry czystości próbki):**
  - Ochrona przed przenikaniem głosu innego rozmówcy (`ADJACENT_TURN_GUARD_S = 0.3s`).
  - Odrzucanie fragmentów o zbyt niskiej energii lub dominacji szumu.

---

## 7. Mobilny Runtime Android (Kotlin Jetpack Compose)

Zbadano katalog `mobile/android`:
- Aplikacja mobilna jest napisana w **100% natywnym Jetpack Compose**.
- Posiada wbudowany **lokalny serwer mowy na telefonie (`PhoneSpeechServer.kt`)**, dzięki czemu telefon może działać całkowicie offline lub służyć jako bezprzewodowy mikrofon / lektor dla komputera stacjonarnego!
- Obsługuje pobieranie modeli na kartę pamięci / pamięć flash z ograniczeniem do Wi-Fi (`allowMobileModels`).

---

## 8. Wnioski dla Audioticonu (Co piszemy w czystym Rust)

Nie kopiujemy ich kodu (AGPL). Bierzemy ich **genialne specyfikacje algorytmiczne** i wdrażamy je w `crates/core`:

1. **`audioticon_core::model_hub`**:
   Wdrożenie formatu `model_licenses.json` z kwitami kryptograficznymi akceptacji EULA.
2. **`audioticon_core::onnx_engine`**:
   Wdrożenie Kokoro-82M (Apache-2.0) i CosyVoice na bazie crate'a `ort` (MIT/Apache-2.0).
3. **`audioticon_core::room_tone`**:
   Zastąpienie cyfrowej ciszy w pauzach emotikonowych (`🛑`, `⏸️`) adaptacyjnym szumem studyjnym EBU R128.
4. **`audioticon_core::onset`**:
   Inteligentne przyciąganie cięć audio do energii mowy (VAD / Speech Onset).
5. **`audioticon_core::m4b_streamer`**:
   Dyskowe strumieniowanie buforów float32 PCM bez alokacji całego audiobooka w RAM.
