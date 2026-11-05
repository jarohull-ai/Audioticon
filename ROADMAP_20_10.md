# 🗺️ AUDIOTICON — Plan Rozwojowy 20/10 (Master Roadmap)

> **Cel:** Przekształcenie Audioticonu w bezkonkurencyjną, w 100% lokalną stację roboczą do reżyserii i produkcji audiobooków oraz słuchowisk (standard 20/10), łączącą unikalny język reżyserii emotikonowej z zaawansowanym arsenałem technologicznym (zainspirowanym VoiceStudio) w czystej architekturze Rust (MIT/Apache-2.0).

---

## 📊 1. Punkt Wyjścia: Gdzie jesteśmy dzisiaj (v0.2.8)

| Obszar | Co mamy w v0.2.8 | Czego brakuje do 20/10 |
|---|---|---|
| **Silnik syntezy (TTS)** | Piper (offline) + Edge TTS + ElevenLabs v3 | Brak lokalnego neuralnego modelu jakości ElevenLabs (np. Kokoro-82M, OmniVoice) |
| **Klonowanie głosu** | VoiceLab (zarządzanie profilami i eksport datasetu) | Brak **lokalnego Zero-Shot Voice Cloning** z 5-sekundowej próbki offline |
| **Reżyseria tekstem** | Emotikony (`🛑`, `🤫`, `💥`, `📞`, `🏛️`, `🐢`, `🐇`) | Brak bezpośredniego mapowania emocji na wagi modelu (emotion embeddings) |
| **Wyrównanie audio** | Szacowanie sylabowe i znakowe w czasie | Brak precyzyjnego wyrównania słowo-w-słowo (**Whisper Forced Alignment do 1 ms**) |
| **Separacja audio** | Podstawowe filtry DSP (Bandpass, Reverb, RingMod) | Brak separacji ścieżek wokalnych / usuwania szumu z próbek (np. Demucs) |
| **Licencjonowanie modeli** | Ręczne pobieranie modeli z HuggingFace | Brak zautomatyzowanego **Model Hub z modalem EULA / Safe-Harbor** |
| **Frontend & UX** | Modularny Workbench (Notion + CapCut + Cosmic Tiling) | Wymaga dalszego dopracowania dokowania i zintegrowania z nowymi silnikami |
| **Platformy** | CLI + Web Workbench (Linux/macOS/Windows) | Brak natywnego buildu desktopowego (Tauri) oraz mobilnego (Android APK) |

---

## 🔍 2. Dekonstrukcja VoiceStudio (`debpalash/VoiceStudio`) — Co bierzemy pod lupę

Przeprowadzamy inżynierię odwrotną koncepcji i funkcji (Clean-Room: bez kopiowania kodu AGPL-3.0):

### Moduł A: Model Hub & Safe-Harbor License Gate
- **Koncepcja:** Aplikacja jest „czystym runtime'em”. Żadne wagi nie są pakowane w instalatorze.
- **Mechanizm EULA:**
  1. Użytkownik przegląda katalog modeli (Piper, Kokoro, OmniVoice, Whisper, Demucs).
  2. Kliknięcie „Pobierz” otwiera szczegółową kartę licencji z informacją o autorze i przeznaczeniu (`Commercial Safe` vs `Non-Commercial / Research`).
  3. Wymuszony checkbox: `[x] Zapoznałem się z licencją i akceptuję jej warunki`.
  4. Pobieranie bezpośrednio ze źródła (HuggingFace CDN) z weryfikacją sumy kontrolnej SHA-256.

### Moduł B: Zero-Shot Voice Cloning (Lokalnie offline)
- **Koncepcja:** Nagranie 3–10 sekund głosu przez mikrofon lub wrzucenie pliku `.wav`.
- **Działanie:** Ekstrakcja embeddingu głośnikowego (speaker vector) i natychmiastowe zsyntetyzowanie kwestii nowym głosem bez wielogodzinnego trenowania.

### Moduł C: Word-Level Forced Alignment (Wyrównanie do milisekundy)
- **Koncepcja:** Użycie modelu ASR (Whisper-Tiny/Base) do precyzyjnego powiązania każdego słowa ze scenariusza ze współrzędnymi czasowymi na wygenerowanej fali dźwiękowej.
- **Korzyść:** Na osi czasu CapCut każdy wyraz i każda emotka ma idealnie wyliczony start i koniec.

### Moduł D: Vocal Isolation & Audio Denoising (Demucs / Audio Clean)
- **Koncepcja:** Czyszczenie nagrań dostarczonych przez użytkownika przed użyciem ich jako wzorca głosu (usunięcie echa, szumu wentylatora, odseparowanie mowy od muzyki w tle).

### Moduł E: Long-Form Audiobook Batch Pipeline (z funkcją Resume)
- **Koncepcja:** Przetwarzanie całych książek (np. 15 godzin materiału) podzielonych na rozdziały i sceny.
- **Odporność:** Zapisywanie stanu każdego segmentu w bazie SQLite/JSON. W razie wyłączenia komputera lub błędu, wznowienie następuje dokładnie od ostatniego zdania.

### Moduł F: Mobile / Edge Runtime (Android APK & Tauri)
- **Koncepcja:** Odchudzony runtime ONNX działający natywnie na telefonach i tabletach (offline), pozwalający na odsłuch i reżyserię z poziomu smartfona.

---

## 🗺️ 3. Fazy Implementacji Audioticon 20/10

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│ FAZA 1: MODEL HUB & LEGAL SAFE-HARBOR (Czystość Prawna i Komercyjna)        │
│ • Model Registry w audioticon-core (metadane licencji, autorzy, sha256)     │
│ • EULA Gate w CLI (--accept-license) oraz w Studio UI (modal licencyjny)    │
│ • Bezpieczne pobieranie z HuggingFace z weryfikacją integralności           │
├─────────────────────────────────────────────────────────────────────────────┤
│ FAZA 2: NEURALNY SILNIK ONNX (Kokoro-82M & OmniVoice)                       │
│ • Czysta implementacja inference ONNX w Rust (crate `ort` - MIT/Apache-2.0) │
│ • Wdrożenie Kokoro-82M (Apache-2.0) — studyjna jakość mowy offline          │
│ • Sterowanie emocjami i intonacją na poziomie promptu / embeddingu           │
├─────────────────────────────────────────────────────────────────────────────┤
│ FAZA 3: ZERO-SHOT VOICE CLONING (Lokalne Klonowanie Głosu)                  │
│ • Rejestrator próbek głosu lektora (3-10s) w Studio UI                     │
│ • Ekstrakcja cech akustycznych i zapis do profilu VoiceLab                  │
│ • Klonowanie mowy offline w 100% na lokalnym CPU/GPU                        │
├─────────────────────────────────────────────────────────────────────────────┤
│ FAZA 4: WHISPER FORCED ALIGNMENT & PRECYZYJNA OŚ CZASU                      │
│ • Integracja whisper-rs (MIT) do pozycjonowania słów co do milisekundy      │
│ • Interaktywne klocki wyrazowe i oddechowe na osi czasu CapCut             │
├─────────────────────────────────────────────────────────────────────────────┤
│ FAZA 5: PRODUKCJA DŁUGOMETRAŻOWA (Batch Audiobook Pipeline)                 │
│ • Moduł kolejkowania rozdziałów z odpornością na awarie (Resume)            │
│ • Eksport EBU R128 (-19.5 LUFS), Apple M4B, ID3v2, 24-bit FLAC              │
├─────────────────────────────────────────────────────────────────────────────┤
│ FAZA 6: EKOSYSTEM MOBILNY I DESKTOP (Tauri 2 + Android APK)                 │
│ • Natywna aplikacja desktopowa bez potrzeby odpalania przeglądarki          │
│ • Budowa instalatora mobilnego Android APK (działającego offline)           │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 📝 4. Zasada Żelaznej Czystości Licencyjnej (Clean-Room Policy)

1. **Zero kopiowania kodu AGPL:** Żaden kod źródłowy z repozytorium VoiceStudio nie może zostać skopiowany, zaimportowany ani przetłumaczony bezpośrednio do repozytorium Audioticon.
2. **Niezależne biblioteki Rust:** Korzystamy wyłącznie z oficjalnych, sprawdzonych bibliotek Rust o licencjach permisywnych (`MIT`, `Apache-2.0`, `BSD-3-Clause`).
3. **Ścisły podział Silnik vs Wagi:** Silnik Audioticon jest w 100% wolny od jakichkolwiek wag modeli. Wszystkie wagi pobiera świadomie użytkownik na swoje urządzenie.
