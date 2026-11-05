# 🎧 AUDIOTICON — Specyfikacja Architektury i Produktu

> **Wizja:** Osobista stacja robocza (DAW sterowany tekstem) do intuicyjnej reżyserii audiobooków za pomocą emotikonów, oparta na technologii Rust + Tauri.

---

## 1. Gramatyka Emotikonowa (Słownik Reżyserski)

| Emotikon | Znaczenie | Działanie w silniku audio (DSP / TTS) |
| :---: | :--- | :--- |
| **`🛑`** | Długa pauza dramaturgiczna | Wstawienie **900 ms** ciszy studyjnej |
| **`⏸️`** | Krótka pauza oddechowa | Wstawienie **400 ms** pauzy na oddech |
| **`😮‍💨`** | Przydech / westchnienie lektora | Wstawienie **350 ms** delikatnej pauzy lub próbki wdechu |
| **`🐢`** | Zwolnienie tempa (mrok, powaga) | Parametr tempa: **`-14%`** |
| **`🐇`** | Przyspieszenie (akcja, panika) | Parametr tempa: **`+15%`** |
| **`📉`** | Opadający ton (ciężka kadencja) | Obniżenie wysokości dźwięku o **`-8Hz`** (zapobiega piskliwym końcówkom) |
| **`📈`** | Wznoszący ton (pytanie, napięcie) | Podbicie tonu o **`+6Hz`** |
| **`💥`** | Mocny akcent na słowo | Zwiększenie dynamiki o **`+15%` (+3 dB)** |
| **`🤫`** | Szept / intymny monolog | Ściszenie o **`-25%` (-6 dB)**, lekkie zwolnienie tempa |
| **`📱`** | Filtr telefoniczny | Nałożenie filtru pasmowego (Bandpass 300Hz–3400Hz + lekki przester) |
| **`📻`** | Krótkofalówka / radio | Pasmo radiowe + charakterystyczne kliknięcie PTT (*roger beep*) |

---

## 2. Architektura Workspace'u Rust

```text
audioticon/
├── Cargo.toml                  # Główny manifest workspace'u
├── SPEC.md                     # Niniejsza specyfikacja
├── crates/
│   ├── core/                   # Parser emotikonów, buforowanie i reguły audio
│   └── cli/                    # Narzędzie konsolowe i lokalny serwer Studio
└── ui/                         # Aplikacja desktopowa Tauri 2 + React + Tailwind
```

---

## 3. Plan Rozwoju i Ekosystem: Writing Agent na Bieliku

> **Status:** 🔬 W trakcie prac badawczo-rozwojowych (R&D) prowadzonych przez Autora projektu.  
> **Zasada:** Brak bezpośredniej integracji z kodem silnika audio do czasu zakończenia testów i pełnego ustabilizowania jakości generowanego formatu.

### Koncepcja Podziału Ról w Kombajnie Audio:
Bielik nie zastępuje silnika głosowego (TTS), lecz stanowi **inteligentną warstwę pre-produkcyjną (mózg przed syntezą audio)**:

```text
Surowy tekst / Rozdział książki
   ↓
Bielik jako Writing Agent & Reżyser AI
   ↓ (Analiza scen, dialogów, emocji, pauz, ról głosowych)
Scenariusz w stabilnym formacie Audioticonu
   ↓
Audioticon Core (Parser + Granular Cache + DSP + Audio Ledger ACX)
   ↓
Piper / Edge TTS / ElevenLabs v3
   ↓
Gotowy Audiobook (MP3 / M4B)
```

### Zadania Bielika w Audioticonie:
1. **Analiza struktury:** Automatyczne rozdzielanie narracji od dialogów (myślniki `—`, dwukropki, kwestie).
2. **Casting i role:** Przypisywanie kwestii konkretnym aktorom za pomocą tagów `[narrator]`, `[kapitan]`, `[anna]`.
3. **Reżyseria emocjonalna:** Wstrzykiwanie znaczników tempa i dynamiki (`🐢`, `🐇`, `💥`, `🤫`).
4. **Plany akustyczne (DSP):** Wykrywanie kontekstu rozmów telefonicznych (`📱`), radiostacji (`📻`), jaskiń i świątyń (`🏛️`).
5. **Rytmika i dramaturgia:** Wstawianie pauz dramaturgicznych (`🛑`), oddechowych (`⏸️`) oraz westchnień lektorskich (`😮‍💨`).

### Stabilny Kontrakt Tekstowy (Zasady Integracji):
* **Wierność 100%:** Model nie streszcza, nie dopisuje ani nie modyfikuje oryginalnych słów autora — wyłącznie wstrzykuje znaczniki i emotikony Audioticonu.
* **Izolacja testowa:** Zanim nastąpi wpięcie do oficjalnego UI/CLI, Bielik przejdzie etap testów na rzeczywistych rozdziałach, weryfikujący poprawność generowanych znaczników przez parser `audioticon-core`.
* **Zbieranie danych treningowych (Flywheel):** Ręczne poprawki reżyserskie nanoszone przez użytkownika w Studio posłużą docelowo jako dataset do fine-tuningu dedykowanego adaptera LoRA (`Bielik-AudioDirector`).
