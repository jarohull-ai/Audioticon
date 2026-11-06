# 🗺️ AUDIOTICON — Plan Rozwojowy i Stan Architektury (Master Roadmap)

> **Cel:** Stworzenie w 100% uczciwej, produkcyjnej stacji roboczej do reżyserii i masteringu audiobooków oraz słuchowisk w czystej architekturze Rust (`MIT` / `Apache-2.0`) — bez atrap, bez fałszywych certyfikatów i z pełną przejrzystością licencyjną.

---

## 📊 1. Stan Faktyczny Silnika (Co jest w 100% zaimplementowane i przetestowane)

| Moduł | Plik źródłowy | Status | Opis działania |
|---|---|---|---|
| **Polski Verbalizer** | `crates/core/src/normalizer.rs` | ✅ **Gotowe** | Odmiana liczb, dat, godzin, kwot walutowych, ułamków i rzymskich numerów rozdziałów |
| **Gramatyka Emotikonowa & 3 Style** | `crates/core/src/parser.rs` | ✅ **Gotowe** | Parsowanie ról, pauz, oddechów, filtrów DSP, SFX/Ambient oraz trybów `narrator`, `multi-voice`, `audio-drama` |
| **Kadencja Oddechowa** | `crates/core/src/cadence.rs` | ✅ **Gotowe** | Automatyczne wstawianie naturalnych oddechów lektorskich (`subtle`, `natural`, `off`) |
| **Piper Neural TTS (Offline)** | `crates/core/src/audio.rs` (`piper_worker.py`) | ✅ **Gotowe** | Trwały proces roboczy ONNX w RAM, polskie głosy CC0 (`gosia`, `darkman`, `mc_speech`) + 140 głosów z `voices.json` |
| **Kokoro-82M ONNX (Offline)** | `crates/core/src/kokoro.rs` (`kokoro_worker.py`) | ✅ **Gotowe** | Prawdziwy proces `kokoro-onnx` (`kokoro-v1.0.int8.onnx` + `voices-v1.0.bin`) dla `en-us`, `en-gb`, `es`, `fr`, `it`, `pt-br` |
| **Edge TTS & ElevenLabs v3 (BYOK)** | `crates/core/src/audio.rs` | ✅ **Gotowe** | Integracja chmurowa z własnym kluczem API użytkownika (BYOK) oraz kalkulatorem kosztów (`estimator.rs`) |
| **Strumieniowy Mikser & Crossfade** | `crates/core/src/audio.rs` | ✅ **Gotowe** | Łączenie segmentów WAV z dysku z 5 ms crossfadem `sin/cos` i ciepłym szumem studia (*room tone* `-68 dBFS`) |
| **Cache SHA-256 & Crash-Resume** | `crates/core/src/cache.rs` | ✅ **Gotowe** | Deterministyczne hashowanie zdań (0 ms wznowienie po przerwaniu pracy) + automatyczne czyszczenie LRU |
| **Mastering EBU R128 & Eksport** | `crates/core/src/audio.rs` | ✅ **Gotowe** | Eksport `.m4b` (AAC + rozdziały), `.flac` (16/24-bit Studio Master) i `.mp3` z normalizacją `loudnorm` |
| **Audio Ledger (Pomiar ACX/Storytel)** | `crates/core/src/ledger.rs` | ✅ **Gotowe** | Fizyczny pomiar LUFS i True Peak dBFS wyrenderowanego pliku (`ffmpeg loudnorm=print_format=json` + fallback PCM) |
| **Model Hub & Safe-Harbor EULA** | `crates/core/src/model_hub.rs`, `models.rs` | ✅ **Gotowe** | Pobieranie modeli Piper i Hub (`kokoro-82m-int8`) z kryptograficznymi kwitami EULA dla modeli niekomercyjnych |
| **Importer EPUB / DOCX & Reżyser AI** | `crates/core/src/import.rs`, `autodirect.rs` | ✅ **Gotowe** | Ekstrakcja rozdziałów z e-booków oraz automatyczna reżyseria przez Ollama / LLM z weryfikacją integralności słów |
| **VoiceLab & Dogrywki Punch-In** | `crates/core/src/voicelab.rs` | ✅ **Gotowe** | Pomiar jakości próbek mikrofonowych (SNR, RMS, Peak), dogrywki pojedynczych zdań i eksport paczek `LJSpeech ZIP` |

---

## 🚀 2. Kolejne Etapy Rozwoju (Roadmap)

### Etap A: Natywny Runtime ONNX w czystym Rust (`ort`) i Lokalne Klonowanie Zero-Shot
- Opcjonalne zastąpienie mostka Python (`piper_worker.py` / `kokoro_worker.py`) bezpośrednim linkowaniem `onnxruntime` w Rust (`crate ort`), aby pojedyncza binarka `audioticon` działała bez zewnętrznego środowiska Python.
- Integracja prawdziwego modelu ekstrakcji wektorów głosu (Speaker Encoder ONNX, np. Wespeaker / CosyVoice) do lokalnego klonowania barwy głosu offline.

### Etap B: Precyzyjne Wyrównanie Słów (Whisper Forced Alignment)
- Integracja prawdziwego modelu `whisper.cpp` / `faster-whisper` do wyznaczania znaczników czasowych każdego słowa z dokładnością do milisekund na potrzeby wizualnej osi czasu DAW (Issue #1).

### Etap C: Interfejs DAW Timeline Studio & Aplikacja Desktopowa (Tauri 2)
- Rozbudowa modularnego interfejsu Web Workbench (`docs/studio.html`) o wielościeżkowy podgląd fal dźwiękowych (Waveform Timeline).
- Opakowanie silnika `audioticon` w natywny instalator desktopowy Tauri 2 (Linux `.AppImage`/`.deb`, Windows `.msi`, macOS `.dmg`).

---

## 📝 3. Zasady Inżynieryjne i Licencyjne (Zero-Fake & Clean-Room Policy)

1. **Zero atrap i sztucznych wydmuszek:** Żaden moduł w `crates/core` nie może symulować sztucznej inteligencji za pomocą prostych pętli matematycznych (np. sinusoid udających syntezę mowy lub wpisanych na sztywno wartości LUFS). Jeśli brakuje zewnętrznego modelu ONNX lub klucza API, program zwraca jasny i pomocny komunikat błędu.
2. **Czystość licencyjna (MIT / Apache-2.0):** Kod silnika korzysta wyłącznie z bibliotek o permisywnych licencjach open-source.
3. **Ścisły podział Silnik vs Wagi:** Binarka Audioticon nie zawiera zaszytych wag modeli neuronowych — użytkownik pobiera wybrane modele z HuggingFace / GitHub Releases za pomocą menedżera `audioticon models`.
