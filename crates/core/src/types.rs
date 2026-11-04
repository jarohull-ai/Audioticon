use serde::{Deserialize, Serialize};

/// Styl produkcji audiobooka / słuchowiska
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ProductionStyle {
    /// 1. Narrator: Tradycyjny audiobook (jeden lektor, zachowane pauzy/tempo, pominięte zmiany głosów i filtry otoczenia)
    Narrator,
    /// 2. MultiVoice: Narracja wieloosobowa (oddzielne głosy postaci i narratora, ale czyste studio bez filtrów lo-fi)
    MultiVoice,
    /// 3. AudioDrama: Pełne słuchowisko (wielogłosowość + pełne efekty otoczenia: telefon, radio, katedra, myśli)
    #[default]
    AudioDrama,
}

impl std::fmt::Display for ProductionStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProductionStyle::Narrator => write!(f, "narrator"),
            ProductionStyle::MultiVoice => write!(f, "multi-voice"),
            ProductionStyle::AudioDrama => write!(f, "audio-drama"),
        }
    }
}

/// Action triggered by an emoji in the script.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EmojiAction {
    /// Silence pause in milliseconds (e.g. 🛑 900ms, ⏸️ 400ms)
    Pause(u32),
    /// Organic lektor breath / sigh in milliseconds (e.g. 😮‍💨 380ms)
    Breath(u32),
    /// Rate modification in percent (e.g. 🐢 -14%, 🐇 +15%)
    RateMod(i32),
    /// Pitch modification in Hertz (e.g. 📉 -8Hz, 📈 +6Hz)
    PitchMod(i32),
    /// Volume modification in percent (e.g. 💥 +15%, 🤫 -25%)
    VolumeMod(i32),
    /// DSP filter preset (e.g. 📱 "phone", 📻 "radio", 📢 "megaphone")
    Filter(String),
    /// Chapter title marker (e.g. 📖 "Rozdział 1")
    Chapter,
    /// Character / Speaker voice override for multi-voice casting (e.g. "zofia", "marek", "ryan")
    Voice(String),
    /// Sound effect / foley trigger (e.g. 🔔, 🚪, 👣, 📄, [sfx:notes])
    SoundEffect(String),
    /// Ambient background soundscape (e.g. ❄️ winter wind, 🌧️ rain, [ambient:wiatr], None to stop)
    Ambience(Option<String>),
}

/// Expressive actor delivery and emotion tags for next-gen neural voice models.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ActorTag {
    Whisper,  // 🤫 Szept
    Sigh,     // 😮‍💨 Westchnienie
    Shout,    // 💥 Emfaza / podniesiony głos
    Laugh,    // 😂 Śmiech
    Hesitant, // 🐢 Wahanie / powolna ostrożność
    Urgent,   // 🐇 Nerwowość / pośpiech
    Custom(String),
}

impl ActorTag {
    /// Formats tag for ChatTTS (e.g. "[whisper]", "[sigh]")
    pub fn as_chattts_tag(&self) -> &'static str {
        match self {
            ActorTag::Whisper => "[whisper]",
            ActorTag::Sigh => "[sigh]",
            ActorTag::Laugh => "[laugh]",
            ActorTag::Shout => "[shout]",
            ActorTag::Hesitant => "[hesitate]",
            ActorTag::Urgent => "[fast]",
            ActorTag::Custom(_) => "",
        }
    }

    /// Formats tag for ElevenLabs v3 actor tags (e.g. "[whispers]", "[sighs]", "[shouts]")
    pub fn as_elevenlabs_tag(&self) -> &'static str {
        match self {
            ActorTag::Whisper => "[whispers]",
            ActorTag::Sigh => "[sighs]",
            ActorTag::Laugh => "[laughs]",
            ActorTag::Shout => "[shouts]",
            ActorTag::Hesitant => "[hesitates]",
            ActorTag::Urgent => "[urgently]",
            ActorTag::Custom(_) => "",
        }
    }

    /// Natural language style description for prompt-based models (e.g. XTTS, Bark, Voicebox)
    pub fn as_style_description(&self) -> &'static str {
        match self {
            ActorTag::Whisper => "whispering softly, intimate close-up voice",
            ActorTag::Sigh => "exhausted voice, heavy breath and sigh",
            ActorTag::Laugh => "chuckles lightly while speaking",
            ActorTag::Shout => "intense emphatic delivery, raised voice",
            ActorTag::Hesitant => "slow, uncertain, hesitant pacing",
            ActorTag::Urgent => "fast, breathless, nervous urgency",
            ActorTag::Custom(_) => "",
        }
    }
}

/// A chunk of spoken text with its acoustic properties and expressive actor tags.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextSegment {
    pub text: String,
    pub rate_mod: i32,
    pub pitch_mod: i32,
    pub volume_mod: i32,
    pub filter: Option<String>,
    pub actor_tags: Vec<ActorTag>,
    #[serde(default)]
    pub voice: Option<String>,
    /// Stereo panning modifier (-1.0 = 100% left, 0.0 = center, 1.0 = 100% right)
    #[serde(default)]
    pub pan: f32,
}

impl Default for TextSegment {
    fn default() -> Self {
        Self {
            text: String::new(),
            rate_mod: -6,  // Default calm rate (-6%)
            pitch_mod: -4, // Default deeper base (-4Hz)
            volume_mod: 0,
            filter: None,
            actor_tags: Vec::new(),
            voice: None,
            pan: 0.0,
        }
    }
}

impl TextSegment {
    /// Formats text for ChatTTS with expressive inline actor tags
    pub fn format_for_chattts(&self) -> String {
        let tags: Vec<&str> = self
            .actor_tags
            .iter()
            .map(|t| t.as_chattts_tag())
            .filter(|s| !s.is_empty())
            .collect();
        if tags.is_empty() {
            self.text.clone()
        } else {
            format!("{} {}", tags.join(" "), self.text)
        }
    }

    /// Formats text for ElevenLabs v3 with expressive inline actor tags
    pub fn format_for_elevenlabs(&self) -> String {
        let tags: Vec<&str> = self
            .actor_tags
            .iter()
            .map(|t| t.as_elevenlabs_tag())
            .filter(|s| !s.is_empty())
            .collect();
        if tags.is_empty() {
            self.text.clone()
        } else {
            format!("{} {}", tags.join(" "), self.text)
        }
    }

    /// Returns natural language style guidance prompt for generative voice models
    pub fn style_prompt(&self) -> Option<String> {
        let styles: Vec<&str> = self
            .actor_tags
            .iter()
            .map(|t| t.as_style_description())
            .filter(|s| !s.is_empty())
            .collect();
        if styles.is_empty() {
            None
        } else {
            Some(styles.join("; "))
        }
    }
}

/// An element in the directed timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DirectedItem {
    Speech(TextSegment),
    Silence(u32),
    Breath(u32),
    Chapter(String),
    SoundEffect(String),
    Ambience(Option<String>),
}

/// A chapter boundary marker with title and time range in milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChapterMark {
    pub title: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

/// Audiobook metadata for M4B and MP3 tags.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AudiobookMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub narrator: Option<String>,
    pub year: Option<String>,
    pub language: Option<String>,
}

/// Summary result of a rendering job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderResult {
    pub total_segments: usize,
    pub duration_ms: u64,
    pub chapters: Vec<ChapterMark>,
}

/// Complete directed script composed of sequential items.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DirectedScript {
    pub items: Vec<DirectedItem>,
}

impl DirectedScript {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn total_pause_ms(&self) -> u32 {
        self.items
            .iter()
            .map(|item| match item {
                DirectedItem::Silence(ms) | DirectedItem::Breath(ms) => *ms,
                DirectedItem::Speech(_)
                | DirectedItem::Chapter(_)
                | DirectedItem::SoundEffect(_)
                | DirectedItem::Ambience(_) => 0,
            })
            .sum()
    }

    /// Exports script into ChatTTS formatted text with control tags
    pub fn to_chattts_script(&self) -> String {
        let mut out = String::new();
        for item in &self.items {
            match item {
                DirectedItem::Chapter(title) => {
                    out.push_str(&format!("\n\n=== {} ===\n", title));
                }
                DirectedItem::Speech(seg) => {
                    if let Some(ref v) = seg.voice {
                        out.push_str(&format!("[speaker:{}] ", v));
                    }
                    out.push_str(&seg.format_for_chattts());
                    out.push(' ');
                }
                DirectedItem::SoundEffect(sfx) => {
                    out.push_str(&format!("[sfx:{}] ", sfx));
                }
                DirectedItem::Ambience(amb) => {
                    if let Some(a) = amb {
                        out.push_str(&format!("[ambient:{}] ", a));
                    }
                }
                DirectedItem::Breath(_) => {
                    out.push_str("[sigh] ");
                }
                DirectedItem::Silence(ms) => {
                    if *ms >= 800 {
                        out.push_str("\n\n");
                    } else if *ms >= 350 {
                        out.push_str(" ... ");
                    }
                }
            }
        }
        out.trim().to_string()
    }

    /// Exports script into ElevenLabs v3 actor-tagged format
    pub fn to_elevenlabs_script(&self) -> String {
        let mut out = String::new();
        for item in &self.items {
            match item {
                DirectedItem::Chapter(title) => {
                    out.push_str(&format!("\n\n# {}\n", title));
                }
                DirectedItem::Speech(seg) => {
                    if let Some(ref v) = seg.voice {
                        out.push_str(&format!("[speaker:{}] ", v));
                    }
                    out.push_str(&seg.format_for_elevenlabs());
                    out.push(' ');
                }
                DirectedItem::SoundEffect(sfx) => {
                    out.push_str(&format!("[sound effect: {}] ", sfx));
                }
                DirectedItem::Ambience(_) => {}
                DirectedItem::Breath(_) => {
                    out.push_str("[sighs] ");
                }
                DirectedItem::Silence(ms) => {
                    if *ms >= 800 {
                        out.push_str("\n\n");
                    } else if *ms >= 350 {
                        out.push_str(" ... ");
                    }
                }
            }
        }
        out.trim().to_string()
    }
}

/// Mastering specification in `.audioticon` project settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectMasteringConfig {
    #[serde(default = "default_target_lufs")]
    pub target_lufs: f64,
    #[serde(default = "default_true_peak")]
    pub true_peak: f64,
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
}

fn default_target_lufs() -> f64 {
    -19.0
}
fn default_true_peak() -> f64 {
    -3.5
}
fn default_sample_rate() -> u32 {
    44100
}

impl Default for ProjectMasteringConfig {
    fn default() -> Self {
        Self {
            target_lufs: default_target_lufs(),
            true_peak: default_true_peak(),
            sample_rate: default_sample_rate(),
        }
    }
}

/// Synthesizer and voice settings in an `.audioticon` project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectSettings {
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub voice: String,
    #[serde(default)]
    pub mastering: Option<ProjectMasteringConfig>,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            provider: "piper".to_string(),
            language: "pl".to_string(),
            voice: "pl_PL-darkman-medium".to_string(),
            mastering: Some(ProjectMasteringConfig::default()),
        }
    }
}

/// Chapter index marker inside `.audioticon` project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectChapter {
    #[serde(default)]
    pub line: usize,
    pub title: String,
}

/// Complete standalone project schema for `.audioticon` files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioticonProject {
    pub version: String,
    pub engine: String,
    pub metadata: AudiobookMetadata,
    #[serde(default)]
    pub settings: ProjectSettings,
    #[serde(default)]
    pub casting: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub chapters: Vec<ProjectChapter>,
    pub script: String,
}

impl AudioticonProject {
    /// Serializes project to formatted JSON string
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserializes project from JSON string
    pub fn from_json(json_str: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audioticon_project_roundtrip() {
        let mut casting = std::collections::HashMap::new();
        casting.insert("narrator".to_string(), "🎙️ Lektor".to_string());
        casting.insert("guard".to_string(), "👨 Strażnik".to_string());
        casting.insert("woman".to_string(), "👩 Kobieta".to_string());

        let project = AudioticonProject {
            version: "0.2.5".to_string(),
            engine: "audioticon".to_string(),
            metadata: AudiobookMetadata {
                title: Some("Archiwum Pyłu".to_string()),
                author: Some("Jarosław Kuchta".to_string()),
                narrator: Some("Lektor".to_string()),
                year: Some("2026".to_string()),
                language: Some("pl".to_string()),
            },
            settings: ProjectSettings {
                provider: "piper".to_string(),
                language: "pl".to_string(),
                voice: "pl_PL-darkman-medium".to_string(),
                mastering: Some(ProjectMasteringConfig::default()),
            },
            casting,
            chapters: vec![
                ProjectChapter {
                    line: 1,
                    title: "Prolog".to_string(),
                },
                ProjectChapter {
                    line: 25,
                    title: "Rozdział 1".to_string(),
                },
            ],
            script: "📖 Prolog 🛑 Dawno, dawno temu...".to_string(),
        };

        let json = project.to_json().expect("Serialize to JSON");
        let decoded = AudioticonProject::from_json(&json).expect("Deserialize from JSON");

        assert_eq!(project, decoded);
        assert_eq!(decoded.metadata.author.as_deref(), Some("Jarosław Kuchta"));
        assert_eq!(decoded.metadata.title.as_deref(), Some("Archiwum Pyłu"));
        assert_eq!(decoded.chapters.len(), 2);
        assert_eq!(decoded.casting.len(), 3);
        assert_eq!(decoded.settings.provider, "piper");
    }
}
