# 🎧 Audioticon Studio v1.0.0 — Kompletny Podręcznik Użytkownika

Witaj w **Audioticon Studio** — lekkiej stacji roboczej do reżyserii i produkcji audiobooków oraz słuchowisk. Program łączy prostotę edytora blokowego z precyzją cyfrowej stacji roboczej (DAW), pomagając w przygotowaniu profesjonalnych produkcji zgodnie z technicznymi wytycznymi **Audible / ACX / Storytel / EBU R128**.

Przy wyborze silników **Piper** lub neuralnego **Kokoro-82M** synteza mowy oraz efekty DSP działają **w pełni lokalnie na Twoim komputerze, bez abonamentu i bez wysyłania manuskryptu do chmury**. Granularny cache zdań oszczędza czas przy korektach, a wbudowany analizator i normalizator ACX / EBU R128 (True Peak, RMS/LUFS, szum tła) ułatwia dopasowanie parametrów technicznych do wymogów platform audiobookowych. Dla użytkowników potrzebujących komercyjnych głosów dostępna jest także opcjonalna integracja chmurowa (Edge TTS oraz ElevenLabs).

---

## 1. Wymagania i Pierwsze Uruchomienie

Audioticon Studio opiera się na lekkim silniku w języku Rust (sama binarka waży około 10 MB) ze skompilowanym interfejsem webowym. Do masteringu i eksportu plików M4B/FLAC program korzysta z zewnętrznego narzędzia FFmpeg.

### Uruchomienie programu
1. **Windows:**
   - Wypakuj archiwum `Audioticon-v1.0.0-windows-x86_64.zip`.
   - Kliknij dwukrotnie plik **`Audioticon.exe`** (z fioletową ikoną).
2. **macOS / Linux:**
   - Wypakuj archiwum `Audioticon-v1.0.0-...tar.gz`.
   - Kliknij dwukrotnie plik wykonywalny `audioticon`.
3. Program domyślnie uruchomi lokalny serwer w bezpiecznym trybie localhost i otworzy Studio w Twojej przeglądarce pod adresem **`http://localhost:3000`**.

### Praca na tablecie (iPad / Android):
- Domyślnie program nasłuchuje tylko lokalnie (`127.0.0.1`). Aby umożliwić dostęp z tabletu w zaufanej domowej sieci Wi-Fi, uruchom program w terminalu poleceniem:
  ```bash
  audioticon serve --host 0.0.0.0
  ```
- Kliknij przycisk **`📱 Tablet / Telefon`** w pasku górnym Studio i zeskanuj aparat tabletu wyświetlony kod QR.
- Wybierz w przeglądarce tabletu opcję *„Do ekranu początkowego”* (Safari) lub *„Zainstaluj aplikację”* (Chrome) — na pulpicie pojawi się fioletowa ikona Audioticon, działająca na pełnym ekranie.

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
4. Program połączy się z API Gumroad, zweryfikuje klucz i zapisze stan licencji wraz z lokalnym podpisem integralności portfela (HMAC-SHA256) w pliku `~/.local/share/audioticon/wallet.json`.
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
