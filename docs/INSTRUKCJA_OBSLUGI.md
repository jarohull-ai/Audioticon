# 🎧 Audioticon Studio — Kompletny Podręcznik Użytkownika

Witaj w **Audioticon Studio** — modularnym środowisku produkcyjnym do reżyserii audiobooków, słuchowisk i treści głosowych AI. Program łączy prostotę edytora tekstu z precyzją cyfrowej stacji roboczej (DAW), umożliwiając tworzenie profesjonalnych produkcji zgodnych ze standardami **Audible / ACX / Storytel** w 100% lokalnie i bez subskrypcji chmurowych.

---

## 1. Wymagania i Pierwsze Uruchomienie

Audioticon Studio jest aplikacją w pełni autonomiczną. Cały interfejs graficzny jest skompilowany bezpośrednio w pliku wykonywalnym.

### Uruchomienie programu
1. **Linux / macOS:**
   - Wypakuj paczkę `audioticon-v0.2.8-...`.
   - Kliknij dwukrotnie plik `Start_Audioticon.sh` lub wpisz w terminalu:
     ```bash
     ./Start_Audioticon.sh
     ```
2. **Windows:**
   - Kliknij dwukrotnie plik `Start_Audioticon.bat`.
3. Program automatycznie uruchomi lokalny serwer i otworzy Studio w Twojej domyślnej przeglądarce pod adresem **`http://localhost:3000`**.

### Wymagania dotyczące FFmpeg
Do masteringu EBU R128, bezstratnego eksportu plików **M4B (z rozdziałami)** oraz **FLAC** wymagany jest program `ffmpeg`:
- **Ubuntu / Debian / Linux Mint:** `sudo apt install ffmpeg`
- **macOS (Homebrew):** `brew install ffmpeg`
- **Windows (PowerShell):** `winget install Gyan.FFmpeg`

---

## 2. Licencja Gumroad i Aktywacja Offline

Audioticon oferuje trzy poziomy licencji:
- **Free:** Pełna synteza audio, edytor tekstu, odsłuch, eksport MP3.
- **Solo ($45):** Nielimitowany eksport do formatu **M4B** z rozdziałami, bezstratny **FLAC 24-bit**, eksport napisów **SRT / VTT / TikTok Karaoke**.
- **Studio ($99):** Wszystko z Solo + Certyfikacja **ACX / Audible / Storytel**, zaawansowany **VoiceLab & Punch-In**, komercyjne profile ekspresji.

### Jak aktywować klucz:
1. W prawym górnym rogu ekranu Studio kliknij przycisk **🛡️ Licencja**.
2. Wklej klucz licencyjny z e-maila potwierdzającego zakup na platformie **Gumroad**.
3. Kliknij **Aktywuj licencję**.
4. Program połączy się z API Gumroad, dokona autoryzacji i zapisze bezpieczny token kryptograficzny HMAC-SHA256 w pliku `~/.local/share/audioticon/wallet.json`.
5. **Od tej chwili możesz pracować całkowicie offline** — program nie wymaga stałego połączenia z Internetem.

---

## 3. Import Treści Książki

Możesz zaimportować swój manuskrypt na dwa sposoby:
1. **Przeciągnij i upuść (Drag & Drop):** Upuść plik bezpośrenio w pole edytora Studio.
2. **Przycisk Importu:** Kliknij ikonę folderu w lewym panelu edytora.

**Obsługiwane formaty:**
- **EPUB** (elektroniczne książki z automatycznym podziałem na rozdziały),
- **PDF** (dokumenty z inteligentnym oczyszczaniem podziału wierszy),
- **DOCX / ODT / RTF** (dokumenty edytorów Word / LibreOffice),
- **Markdown (.md) / Zwykły tekst (.txt)**.

---

## 4. Reżyseria Emotikonami (Emoji Directing)

Zamiast pisać skomplikowane skrypty SSML, Audioticon pozwala reżyserować głos lektora intuicyjnymi emotikonami wplecionymi w tekst:

| Emotikon | Funkcja | Efekt akustyczny |
|---|---|---|
| 📖 `Rozdział 1: Początek` | Znacznik rozdziału | Tworzy punkt nawigacyjny w pliku M4B i pauzę wstępną |
| 🛑 | Oddech / Pauza | Wstawia naturalny lektorski oddech lub przerwę logiczną |
| 🤫 | Szept | Przełącza na profil szeptu (Narrative, Action, Romantic, CloseMic) |
| 💥 | Krzyk / Akcja | Podbicie ekspresji, głośności i dynamiki wypowiedzi |
| 📱 | Telefon / Radio | Filtr pasmowy (300 Hz – 3400 Hz) symulujący rozmowę telefoniczną |
| 🏛️ | Katedra / Przestrzeń | Głęboki pogłos przestrzenny (reverb) |
| 📻 | Stare Radio | Efekt vintage z obcięciem pasma i delikatnym nasyceniem |

### Bubble Menu (Dymek kontekstowy)
Zaznacz dowolne słowo lub zdanie w edytorze — nad tekstem pojawi się podręczne menu umożliwiające nadanie stylu jednym kliknięciem myszy.

---

## 5. Modele Głosowe i Silniki TTS

### Piper ONNX (Domyślny, rekomendowany dla języka polskiego)
- Działa w 100% lokalnie i generuje audio w ułamku sekundy.
- **Dostępne głosy polskie:** `pl_PL-darkman-medium` (głęboki, męski narrator), `pl_PL-gosia-medium` (ciepły, kobiecy lektor), `pl_PL-mc_speech-medium`.
- **Głosy angielskie:** `en_US-lessac-medium`.

### Kokoro-82M (Neuralny model wielojęzyczny)
- Ultra-naturalna intonacja oparta na architekturze StyleTTS 2.
- Obsługuje głosy amerykańskie (`af_bella`, `am_adam`, `af_sarah`) oraz brytyjskie (`bm_george`, `bf_emma`).
- Natywna inferencja w czystym Rust przez bibliotekę ONNX Runtime (`ort`).

---

## 6. Oś Czasu, Waveform i Punch-In

Po wygenerowaniu audio na dole ekranu pojawia się modularny panel osi czasu:
- **Interaktywna fala dźwiękowa (Waveform):** Kliknięcie w dowolny fragment odtwarza audio od wybranego miejsca i podświetla odpowiednie zdanie w tekście.
- **AI Retake (Szybka dogrywka):** Zmieniłeś jedno słowo w rozdziale? Kliknij ikonę odświeżenia przy konkretnym kafelku — Audioticon przeliczy tylko to jedno zdanie, korzystając z pamięci podręcznej (Sentence Cache) dla pozostałych 99% książki.
- **Punch-In (Mikrofon na żywo):** Możesz nagrać własną poprawkę głosem z mikrofonu. Dogrywka zostanie automatycznie wyrównana głośnościowo (Studio Leveler) i wklejona w oś czasu z opcją cofnięcia (Revert).

---

## 7. Certyfikacja Jakości ACX / EBU R128

W zakładce **Mastering ACX** sprawdzisz, czy Twój plik spełnia rygorystyczne normy wydawnicze (Audible, Amazon ACX, Storytel, Apple Books):
- **Zintegrowana głośność (Integrated Loudness):** Wymóg ACX: od `-23.0 LUFS` do `-18.0 LUFS`.
- **Poziom szczytowy (True Peak):** Maksymalnie `-3.0 dBFS`.
- **Poziom tła (Noise Floor):** Poniżej `-60 dBFS`.

Kliknięcie **Pobierz Certyfikat ACX** generuje plik JSON/PDF z podpisem kryptograficznym potwierdzającym zgodność dla dystrybutora.

---

## 8. Eksport Gotowego Audiobooka

W oknie eksportu możesz wybrać format dopasowany do Twojego celu:
1. **Audiobook M4B (Zalecany):**
   - Zawiera pełną strukturę rozdziałów, tytuły, autora i okładkę.
   - Gotowy do bezpośredniego wgrania na iPhone'a, odtwarzacze Audible lub aplikacje lektorskie.
2. **Bezstratny FLAC 24-bit:**
   - Dźwięk studyjny najwyższej próby, idealny do archiwizacji i dalszej obróbki w zewnętrznych DAW.
3. **Pliki MP3 320 kbps:**
   - Uniwersalna kompatybilność z każdym urządzeniem.
4. **Napisy i TikTok Karaoke (SRT, VTT, JSON):**
   - Idealne do tworzenia zwiastunów książki na TikTok, YouTube Shorts i Instagram Reels z podświetlaniem aktualnie czytanego słowa.

---

## 9. Pomoc i Wsparcie Techniczne

- **E-mail:** `jarohull.ai@gmail.com`
- **Repozytorium GitHub:** [https://github.com/jarohull-ai/Audioticon](https://github.com/jarohull-ai/Audioticon)
- **Zgłaszanie uwag:** [GitHub Issues](https://github.com/jarohull-ai/Audioticon/issues)
