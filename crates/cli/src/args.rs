use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "audioticon")]
#[command(author = "Jarosław Kuchta")]
#[command(version)]
#[command(
    about = "🎧 Audioticon — Reżyser audiobooków sterowany emotikonami z cachem zdań i filtrami DSP",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
pub enum TtsProvider {
    /// Microsoft Edge TTS (wysoka jakość online)
    Edge,
    /// Piper TTS (100% lokalny, ultra szybki model neuronowy ONNX)
    Piper,
    /// ElevenLabs v3 API (profesjonalne głosy generative AI)
    Elevenlabs,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
pub enum ScriptFormat {
    /// Format ElevenLabs v3 z tagami aktorskimi ([whispers], [sighs], [shouts])
    Elevenlabs,
    /// Format ChatTTS z tagami inline ([whisper], [sigh], [shout])
    Chattts,
    /// Strukturalna oś czasu z deskryptorami stylu promptów
    Timeline,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
pub enum ProductionStyleArg {
    /// 1. Narrator: Tradycyjny audiobook (jeden lektor, zachowane pauzy/tempo, pominięte zmiany głosów i filtry otoczenia)
    #[value(name = "narrator", alias = "lektor")]
    Narrator,
    /// 2. MultiVoice: Narracja wieloosobowa (oddzielne głosy postaci i narratora, czyste studio bez filtrów lo-fi)
    #[value(name = "multi-voice", alias = "multivoice", alias = "wieloosobowa")]
    MultiVoice,
    /// 3. AudioDrama: Pełne słuchowisko (wielogłosowość + pełne efekty otoczenia: telefon, radio, katedra, myśli)
    #[value(name = "audio-drama", alias = "audiodrama", alias = "sluchowisko")]
    AudioDrama,
}

impl From<ProductionStyleArg> for audioticon_core::ProductionStyle {
    fn from(arg: ProductionStyleArg) -> Self {
        match arg {
            ProductionStyleArg::Narrator => audioticon_core::ProductionStyle::Narrator,
            ProductionStyleArg::MultiVoice => audioticon_core::ProductionStyle::MultiVoice,
            ProductionStyleArg::AudioDrama => audioticon_core::ProductionStyle::AudioDrama,
        }
    }
}

impl std::fmt::Display for ProductionStyleArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProductionStyleArg::Narrator => write!(f, "narrator (tradycyjny lektor)"),
            ProductionStyleArg::MultiVoice => {
                write!(f, "multi-voice (wieloosobowa w czystym studio)")
            }
            ProductionStyleArg::AudioDrama => {
                write!(f, "audio-drama (słuchowisko z efektami otoczenia)")
            }
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
pub enum BreathIntensityArg {
    /// Subtelny, organiczny oddech studyjny (-33 dBFS, domyślny)
    #[value(name = "subtle", alias = "subtelny")]
    Subtle,
    /// Naturalny, wyraźniejszy oddech lektorski (-27.5 dBFS)
    #[value(name = "natural", alias = "naturalny")]
    Natural,
    /// Wyłączone automatyczne oddechy (jedynie jawne w tekście)
    #[value(name = "off", alias = "wylaczony")]
    Off,
}

impl From<BreathIntensityArg> for audioticon_core::BreathIntensity {
    fn from(arg: BreathIntensityArg) -> Self {
        match arg {
            BreathIntensityArg::Subtle => audioticon_core::BreathIntensity::Subtle,
            BreathIntensityArg::Natural => audioticon_core::BreathIntensity::Natural,
            BreathIntensityArg::Off => audioticon_core::BreathIntensity::Off,
        }
    }
}

impl std::fmt::Display for BreathIntensityArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BreathIntensityArg::Subtle => write!(f, "subtle (subtelny)"),
            BreathIntensityArg::Natural => write!(f, "natural (naturalny)"),
            BreathIntensityArg::Off => write!(f, "off (wyłączony)"),
        }
    }
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Renderuje tekst ze znacznikami emoji do pliku audio (.wav lub .mp3)
    Render {
        /// Ścieżka do pliku z tekstem ze znacznikami emoji (lub pomiń i użyj --demo)
        #[arg(short, long)]
        input: Option<PathBuf>,

        /// Ścieżka do pliku wyjściowego (.wav lub .mp3)
        #[arg(short, long, default_value = "output.mp3")]
        output: PathBuf,

        /// Silnik syntezy mowy: edge (online) lub piper (lokalny offline)
        #[arg(long, value_enum, default_value_t = TtsProvider::Edge)]
        provider: TtsProvider,

        /// Identyfikator głosu Edge-TTS (np. pl-PL-MarekNeural, pl-PL-ZofiaNeural)
        #[arg(short, long, default_value = "pl-PL-MarekNeural")]
        voice: String,

        /// Język tekstu (pl lub en - wyłącza polską normalizację liczb dla języka angielskiego)
        #[arg(long, default_value = "pl")]
        lang: String,

        /// Ścieżka do lokalnego modelu ONNX dla Piper (domyślnie: ~/.local/share/audioticon/models/)
        #[arg(long)]
        model: Option<PathBuf>,

        /// Własny katalog cache (domyślnie ~/.cache/audioticon)
        #[arg(short, long)]
        cache_dir: Option<PathBuf>,

        /// Tytuł audiobooka (metadane M4B / MP3)
        #[arg(long)]
        title: Option<String>,

        /// Autor książki (metadane M4B / MP3)
        #[arg(long)]
        author: Option<String>,

        /// Lektor / narrator (metadane M4B / MP3)
        #[arg(long)]
        narrator: Option<String>,

        /// Rok wydania (metadane M4B / MP3)
        #[arg(long)]
        year: Option<String>,

        /// Uruchamia wbudowaną scenę demonstracyjną (Stacja Polarna Aurora)
        #[arg(long)]
        demo: bool,

        /// Uruchamia scenę demonstracyjną wszystkich efektów akustycznych (telefon, radio, katedra, myśli, robot)
        #[arg(long)]
        demo_effects: bool,

        /// Wyświetl tylko szacunkową liczbę znaków i koszt bez generowania audio
        #[arg(long)]
        estimate: bool,

        /// Maksymalny rozmiar bufora cache (np. 1GB, 500MB, domyślnie 2GB); starsze segmenty są automatycznie czyszczone (LRU)
        #[arg(long)]
        max_cache_size: Option<String>,

        /// Liczba równoległych zadań syntezy TTS (domyślnie dopasowana do silnika i rdzeni CPU)
        #[arg(short = 'j', long = "jobs")]
        jobs: Option<usize>,

        /// Styl produkcji: narrator (jeden lektor), multi-voice (kilka głosów w czystym studio) lub audio-drama (słuchowisko z efektami otoczenia)
        #[arg(short = 's', long = "style", value_enum, default_value_t = ProductionStyleArg::AudioDrama)]
        style: ProductionStyleArg,

        /// Inteligentna kadencja oddechowa i frazowanie zdań lektora (automatyczne mikro-pauzy i oddechy w długich frazach)
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        cadence: bool,

        /// Poziom intensywności oddechów lektora: subtle (subtelny, domyślny), natural (naturalny), off (wyłączony)
        #[arg(long, value_enum, default_value_t = BreathIntensityArg::Subtle)]
        breath_intensity: BreathIntensityArg,
    },
    /// Zarządzanie lokalnym buforem audio (cache)
    Cache {
        /// Wyczyść wszystkie zapisane segmenty
        #[arg(long)]
        clear: bool,

        /// Własny katalog bufora
        #[arg(short, long)]
        dir: Option<PathBuf>,

        /// Ogranicz rozmiar bufora do podanej wielkości (np. 1GB, 500MB) metodą LRU (najstarsze pliki)
        #[arg(long)]
        prune: Option<String>,

        /// Wyświetl statystyki rozmiaru bufora cache
        #[arg(long)]
        stats: bool,
    },
    /// Eksportuje zareżyserowany tekst z emotikonami do formatów modeli ekspresyjnych (ElevenLabs, ChatTTS)
    Script {
        /// Ścieżka do pliku z tekstem ze znacznikami emoji (lub pomiń i użyj --demo)
        #[arg(short, long)]
        input: Option<PathBuf>,

        /// Format wyjściowy tagów aktorskich
        #[arg(short, long, value_enum, default_value_t = ScriptFormat::Elevenlabs)]
        format: ScriptFormat,

        /// Język tekstu (pl lub en)
        #[arg(long, default_value = "pl")]
        lang: String,

        /// Uruchamia wbudowaną scenę demonstracyjną
        #[arg(long)]
        demo: bool,
    },
    /// Wyświetla listę dostępnych presetów akustycznych (systemowych i z audioticon.toml)
    Presets {
        /// Ścieżka do pliku projektu audioticon.toml
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Uruchamia lokalny serwer Audioticon Studio (Workbench) z interfejsem w przeglądarce
    #[command(alias = "studio")]
    Serve {
        /// Port serwera HTTP (domyślnie 3000)
        #[arg(short, long, default_value_t = 3000)]
        port: u16,

        /// Adres nasłuchu (domyślnie 127.0.0.1)
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Domyślny silnik syntezy (piper lub edge)
        #[arg(long, value_enum, default_value_t = TtsProvider::Piper)]
        provider: TtsProvider,

        /// Domyślny język (pl lub en)
        #[arg(long, default_value = "pl")]
        lang: String,

        /// Ścieżka do lokalnego modelu ONNX dla Piper
        #[arg(long)]
        model: Option<PathBuf>,

        /// Nie otwieraj automatycznie przeglądarki
        #[arg(long)]
        no_browser: bool,
    },
    /// Zarządzanie lokalnymi modelami neuronowymi Piper TTS (offline)
    Models {
        #[command(subcommand)]
        action: Option<ModelsAction>,
    },
    /// Importuje całą książkę (.epub, .docx, .txt, .md) z autodetekcją rozdziałów i konwersją do scenariusza
    Import {
        /// Ścieżka do pliku książki (.epub, .docx, .txt, .md)
        #[arg(short, long)]
        input: PathBuf,

        /// Ścieżka do wyjściowego pliku ze scenariuszem (.txt)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Nadpisz tytuł książki
        #[arg(long)]
        title: Option<String>,

        /// Nadpisz autora książki
        #[arg(long)]
        author: Option<String>,

        /// Wyświetl tylko strukturę rozdziałów i statystyki bez zapisu pliku
        #[arg(long)]
        preview: bool,

        /// Podziel i zapisz rozdziały do osobnych plików w podanym katalogu
        #[arg(long)]
        split_dir: Option<PathBuf>,
    },
    /// Zarządzanie kluczami API zewnętrznych dostawców (ElevenLabs, OpenAI, Azure)
    Providers {
        #[command(subcommand)]
        action: Option<ProvidersAction>,
    },
    /// Szacuje liczbę znaków i koszt syntezy skryptu (uwzględniając trafienia w cache)
    Estimate {
        /// Ścieżka do pliku ze scenariuszem (lub pomiń i użyj tekstu demonstracyjnego)
        #[arg(short, long)]
        input: Option<PathBuf>,

        /// Dostawca syntezy mowy: elevenlabs, openai, azure, piper, edge
        #[arg(long, default_value = "elevenlabs")]
        provider: String,

        /// Identyfikator głosu (np. adam, antoni, rachel)
        #[arg(short, long, default_value = "adam")]
        voice: String,

        /// Własny katalog cache
        #[arg(short, long)]
        cache_dir: Option<PathBuf>,
    },
    /// Zarządzanie profilami głosowymi lektorów, próbkami referencyjnymi, punch-in i klonowaniem (Voice Lab)
    #[command(alias = "voicelab")]
    VoiceLab {
        #[command(subcommand)]
        action: Option<VoiceLabAction>,
    },
    /// Zarządzanie i projektowanie własnych emotikonek (DSP i obsada lektorska)
    Emojis {
        #[command(subcommand)]
        action: Option<EmojisAction>,
    },
    /// Automatyczna reżyseria tekstu za pomocą modeli AI (Gemini / OpenAI)
    Direct {
        /// Ścieżka do surowego pliku tekstowego (.txt, .md)
        input: PathBuf,

        /// Ścieżka wyjściowa dla wyreżyserowanego scenariusza (.audioticon)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Dostawca AI (gemini, openai, ollama)
        #[arg(short, long, default_value = "gemini")]
        provider: String,

        /// Nazwa modelu AI (np. gemini-2.5-flash, gpt-4o-mini)
        #[arg(short, long)]
        model: Option<String>,

        /// Klucz API (opcjonalny, domyślnie z credentials.toml lub zmiennej środowiskowej)
        #[arg(short, long)]
        key: Option<String>,

        /// Tylko wyświetl wyreżyserowany scenariusz na ekranie bez zapisywania
        #[arg(long)]
        dry_run: bool,
    },
    /// Zarządzanie biblioteką efektów dźwiękowych (SFX i otoczenie)
    Sfx {
        #[command(subcommand)]
        action: Option<SfxAction>,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum SfxAction {
    /// Wyświetla listę zainstalowanych efektów dźwiękowych (lokalnych i globalnych)
    List,
    /// Wyświetla katalog rekomendowanych darmowych efektów studyjnych CC0
    Catalog,
    /// Importuje własny plik audio (.wav, .ogg, .mp3, .flac) do biblioteki SFX
    Import {
        /// Ścieżka do pliku audio
        file: PathBuf,
        /// Własna nazwa efektu (opcjonalnie)
        #[arg(short, long)]
        name: Option<String>,
        /// Importuj do bieżącego projektu (./sfx/) zamiast globalnej biblioteki
        #[arg(long)]
        project: bool,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum VoiceLabAction {
    /// Wyświetla listę profili głosowych i próbek referencyjnych
    List,
    /// Tworzy nowy profil głosu
    Create {
        /// Identyfikator profilu (np. jarek-lektor)
        id: String,
        /// Czytelna nazwa wyświetlana (np. "Jarek (Lektor)")
        #[arg(short, long)]
        name: String,
        /// Płeć głosu: male, female, neutral
        #[arg(short, long, default_value = "male")]
        gender: String,
        /// Kod języka (np. pl, en)
        #[arg(short, long, default_value = "pl")]
        lang: String,
        /// Krótki opis profilu głosu
        #[arg(short, long)]
        desc: Option<String>,
    },
    /// Dodaje i analizuje próbkę audio (.wav, .mp3, .ogg) do profilu
    AddSample {
        /// Identyfikator profilu głosu
        profile: String,
        /// Ścieżka do pliku audio
        input: PathBuf,
        /// Opcjonalna nazwa próbki
        #[arg(short, long)]
        name: Option<String>,
    },
    /// Podmienia zdanie w buforze audio na nagranie lektorskie (punch-in)
    PunchIn {
        /// Identyfikator profilu lektora
        profile: String,
        /// Hash zdania (fingerprint SHA-256)
        hash: String,
        /// Ścieżka do nagrania audio lektora
        input: PathBuf,
        /// Własny katalog bufora
        #[arg(short, long)]
        cache_dir: Option<PathBuf>,
    },
    /// Klonuje profil głosu do ElevenLabs (Instant Voice Cloning)
    CloneElevenlabs {
        /// Identyfikator profilu głosu
        profile: String,
    },
    /// Eksportuje próbki i metadane do archiwum ZIP (format LJSpeech / XTTS)
    ExportDataset {
        /// Identyfikator profilu głosu
        profile: String,
        /// Ścieżka wyjściowego pliku ZIP
        output: PathBuf,
    },
    /// Usuwa profil głosu i powiązane próbki
    Delete {
        /// Identyfikator profilu do usunięcia
        profile: String,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum EmojisAction {
    /// Wyświetla listę zdefiniowanych emotikonek
    List,
    /// Dodaje lub aktualizuje własną emotikonkę w audioticon.toml
    Add {
        /// Emotikonka (np. "🧙‍♂️", "👽", "🦇")
        emoji: String,
        /// Nazwa własna (np. "Czarodziej")
        name: String,
        /// Typ akcji: filter (preset DSP), voice (głos lektora), modifier (tempo/ton)
        #[arg(long, default_value = "filter")]
        action_type: String,
        /// Tekstowy alias w nawiasach (np. "czarodziej" -> [czarodziej])
        #[arg(short, long)]
        alias: Option<String>,
        /// Nazwa filtru bazowego (np. cathedral, dungeon, robot, radio, phone)
        #[arg(short, long)]
        filter: Option<String>,
        /// Identyfikator głosu (np. darkman, gosia, pl-PL-MarekNeural)
        #[arg(short, long)]
        voice: Option<String>,
        /// Modyfikator tonu w Hz (np. -4, +6)
        #[arg(short, long, allow_hyphen_values = true)]
        pitch: Option<i32>,
        /// Modyfikator tempa w % (np. -10, +15)
        #[arg(short, long, allow_hyphen_values = true)]
        rate: Option<i32>,
        /// Mnożnik głośności (np. 1.25)
        #[arg(long)]
        volume: Option<f32>,
        /// Krótki opis dla podpowiedzi
        #[arg(short, long)]
        desc: Option<String>,
    },
    /// Usuwa własną emotikonkę z audioticon.toml
    Remove {
        /// Emotikonka do usunięcia
        emoji: String,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ProvidersAction {
    /// Wyświetla listę skonfigurowanych dostawców i zamaskowane klucze API
    List,
    /// Konfiguruje klucz API dostawcy (zapisywany bezpiecznie w credentials.toml)
    Set {
        /// Identyfikator dostawcy (np. elevenlabs, openai, azure)
        provider: String,
        /// Klucz API
        #[arg(short, long)]
        key: String,
        /// Domyślny model (opcjonalny, np. eleven_multilingual_v2)
        #[arg(short, long)]
        model: Option<String>,
        /// Domyślny głos (opcjonalny, np. adam, antoni)
        #[arg(short, long)]
        voice: Option<String>,
    },
    /// Weryfikuje ważność klucza API i sprawdza stan konta (np. subskrypcja ElevenLabs)
    Check {
        /// Identyfikator dostawcy do sprawdzenia (domyślnie: elevenlabs)
        #[arg(default_value = "elevenlabs")]
        provider: String,
    },
    /// Usuwa klucz API dostawcy z konfiguracji
    Remove {
        /// Identyfikator dostawcy
        provider: String,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ModelsAction {
    /// Wyświetla listę dostępnych w katalogu i zainstalowanych modeli offline
    List {
        /// Filtruj po kodzie języka (np. pl, en, de, uk, es)
        #[arg(short, long)]
        lang: Option<String>,
        /// Pokaż tylko zainstalowane modele
        #[arg(short, long)]
        installed: bool,
    },
    /// Pobiera wybrany model z oficjalnego repozytorium HuggingFace
    Download {
        /// Identyfikator lub alias modelu (np. gosia, darkman, lessac, amy, pl_PL-gosia-medium)
        name: String,
        /// Wymuś ponowne pobranie nawet jeśli plik już istnieje
        #[arg(long)]
        force: bool,
    },
    /// Usuwa pobrany model offline z dysku
    Remove {
        /// Identyfikator lub alias modelu do usunięcia
        name: String,
    },
    /// Pobiera najnowszą bazę voices.json z oficjalnego repozytorium Piper
    Update,
}

pub fn get_default_cache_dir() -> PathBuf {
    if let Some(user_cache) = dirs::cache_dir() {
        user_cache.join("audioticon")
    } else {
        std::env::temp_dir().join("audioticon_cache")
    }
}

pub fn get_default_piper_model(lang: &str) -> PathBuf {
    let filename = if lang.starts_with("en") {
        "en_US-lessac-medium.onnx"
    } else {
        "pl_PL-darkman-medium.onnx"
    };
    audioticon_core::models::models_dir().join(filename)
}

pub const DEMO_SCENE: &str = r#"
📖 Prolog: Stacja Aurora 🛑

Spitsbergen, 15.01.2026. 🛑

Jan Zawadzki siedział przy pulpicie stacji badawczej na Spitsbergenie. ⏸️
Za oknem szalała zamieć, jak przez większość nocy polarnej. 🛑
Na blacie stał kubek z zimną herbatą, 😮‍💨 której nie pił od godzin. 🛑

Na stole leżał 💥dziennik pokładowy. 🛑

🐢 Przetarty na krawędziach... 😮‍💨 od ciągłego przeglądania... od noszenia w skafandrze arktycznym. 🛑
Okładka zmatowiała, grzbiet przetarty w kilku miejscach. 🛑

W środku... setki pomiarów i odczytów. 🛑
📉Temperatur. ⏸️
📉Wiatrów. ⏸️
📉Współrzędnych. 🛑

Niektóre wpisy nanoszono pośpiesznie ołówkiem. ⏸️
Inne były staranne... 😮‍💨 z czasów, gdy na stacji panował jeszcze całkowity spokój. 🛑

📖 Rozdział 1: Sygnał z Lodu 🛑

Nazwał ten zapis: 💥Projekt Aurora. 🛑

Otworzył dziennik na stronie z pierwszego kontaktu. 🛑
15.01.2025. ⏸️
Godzina 03:14. ⏸️
Sektor Północny. 🛑

Przez dłuższą chwilę patrzył na te współrzędne. 🛑
Potem zamknął dziennik. 🛑
Nie musiał czytać dalej. 🛑
🤫 Pamiętał każdy szczegół. 🛑
"#;

pub const DEMO_EFFECTS_SCENE: &str = r#"
Oto demonstracja zaawansowanych efektów akustycznych Audioticon. 🛑

Narrator mówi czystym, studyjnym głosem. ⏸️
Ale nagle odzywa się interkom satelitarny. 🛑
📱 "Halo? Stacja Aurora, słyszycie nas? Zbliża się arktyczny sztorm!" 🛑

W tle trzaskają zakłócenia wojskowej radiostacji krótkofalowej. 🛑
📻 "Baza do patrolu śnieżnego. Zgłaszam nagły spadek ciśnienia. Natychmiast powrót do hangaru. Odbiór." 🛑

Kierownik stacji włącza tubę i krzyczy przez megafon w hali maszyn: 🛑
📢 "Wszyscy zabezpieczyć śluzy zewnętrzne! To nie są ćwiczenia!" 🛑

Jan wchodzi w głąb wysokiej, opuszczonej kopuły obserwatorium. 🛑
🏛️ Kroki niosły się głuchym echem po stalowych i kamiennych łukach. 🛑

Schodzi technicznymi schodami do podziemnego tunelu w wiecznej zmarzlinie. 🛑
🏰 Ciemność była absolutna, a krople lodu stukały cicho o metalowe podpory. 🛑

Zatrzymał się. 🛑
💭 "Czy aparatura pomiarowa wytrzyma to ciśnienie? Musimy utrzymać zasilanie generatora." 🛑

Nagle z terminala wyłonił się syntetyczny głos autonomicznego systemu stacji: 🛑
🤖 "Autodiagnostyka zakończona. Wykryto anomalię w reaktorze pomocniczym." 🛑

I znowu zapadła lodowata cisza. 🛑
"#;
