use serde::{Deserialize, Serialize};

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
}

/// A chunk of spoken text with its acoustic properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextSegment {
    pub text: String,
    pub rate_mod: i32,
    pub pitch_mod: i32,
    pub volume_mod: i32,
    pub filter: Option<String>,
}

impl Default for TextSegment {
    fn default() -> Self {
        Self {
            text: String::new(),
            rate_mod: -6,  // Default calm rate (-6%)
            pitch_mod: -4, // Default deeper base (-4Hz)
            volume_mod: 0,
            filter: None,
        }
    }
}

/// An element in the directed timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DirectedItem {
    Speech(TextSegment),
    Silence(u32),
    Breath(u32),
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
                DirectedItem::Speech(_) => 0,
            })
            .sum()
    }
}
