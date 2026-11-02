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
│   └── cli/                    # Narzędzie konsolowe do testowania
└── ui/                         # Aplikacja desktopowa Tauri 2 + React + Tailwind
```
