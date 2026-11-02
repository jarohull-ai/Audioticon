use std::collections::HashMap;
use regex::Regex;
use crate::types::{DirectedItem, DirectedScript, EmojiAction, TextSegment};

/// Configurable Emoji Director Grammar
#[derive(Debug, Clone)]
pub struct EmojiDirector {
    actions: HashMap<String, EmojiAction>,
}

impl Default for EmojiDirector {
    fn default() -> Self {
        let mut m = HashMap::new();
        // Pauses
        m.insert("🛑".to_string(), EmojiAction::Pause(900));
        m.insert("⏸️".to_string(), EmojiAction::Pause(400));
        m.insert("😮‍💨".to_string(), EmojiAction::Pause(350));
        // Dynamics & Tempo
        m.insert("🐢".to_string(), EmojiAction::RateMod(-14));
        m.insert("🐇".to_string(), EmojiAction::RateMod(15));
        m.insert("📉".to_string(), EmojiAction::PitchMod(-8));
        m.insert("📈".to_string(), EmojiAction::PitchMod(6));
        m.insert("💥".to_string(), EmojiAction::VolumeMod(15));
        m.insert("🤫".to_string(), EmojiAction::VolumeMod(-25));
        // DSP Filters
        m.insert("📱".to_string(), EmojiAction::Filter("phone".to_string()));
        m.insert("📻".to_string(), EmojiAction::Filter("radio".to_string()));
        m.insert("📢".to_string(), EmojiAction::Filter("megaphone".to_string()));
        Self { actions: m }
    }
}

impl EmojiDirector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, emoji: impl Into<String>, action: EmojiAction) {
        self.actions.insert(emoji.into(), action);
    }

    /// Parses an emoji-directed text into a structured DirectedScript.
    pub fn parse(&self, raw_script: &str) -> DirectedScript {
        let mut script = DirectedScript::new();
        if self.actions.is_empty() || raw_script.is_empty() {
            if !raw_script.trim().is_empty() {
                script.items.push(DirectedItem::Speech(TextSegment {
                    text: raw_script.trim().to_string(),
                    ..Default::default()
                }));
            }
            return script;
        }

        // Build regex pattern matching registered emojis
        let emojis_pattern: Vec<String> = self.actions.keys().map(|e| regex::escape(e)).collect();
        let re_str = format!("({})", emojis_pattern.join("|"));
        let re = Regex::new(&re_str).expect("Valid regex");

        let mut current_segment = TextSegment::default();
        let mut text_buf = String::new();

        let mut last_end = 0;
        for mat in re.find_iter(raw_script) {
            // Process text before the emoji
            let before = &raw_script[last_end..mat.start()];
            if !before.is_empty() {
                text_buf.push_str(before);
            }

            let emoji_str = mat.as_str();
            if let Some(action) = self.actions.get(emoji_str) {
                // If we have buffered text, flush it before the action
                let clean_text = text_buf.trim().to_string();
                if !clean_text.is_empty() {
                    current_segment.text = clean_text;
                    script.items.push(DirectedItem::Speech(current_segment.clone()));
                    text_buf.clear();
                    // Reset single-word modifications like volume
                    current_segment.volume_mod = 0;
                }

                // Apply action
                match action {
                    EmojiAction::Pause(ms) => {
                        script.items.push(DirectedItem::Silence(*ms));
                        // After full pause (like 🛑), reset pacing back to defaults
                        if *ms >= 800 {
                            current_segment = TextSegment::default();
                        }
                    }
                    EmojiAction::RateMod(rate) => {
                        current_segment.rate_mod = *rate;
                    }
                    EmojiAction::PitchMod(pitch) => {
                        current_segment.pitch_mod = *pitch;
                    }
                    EmojiAction::VolumeMod(vol) => {
                        current_segment.volume_mod = *vol;
                        if *vol == -25 {
                            // Szept also slows down slightly
                            current_segment.rate_mod = -10;
                        }
                    }
                    EmojiAction::Filter(filter_name) => {
                        current_segment.filter = Some(filter_name.clone());
                    }
                }
            }
            last_end = mat.end();
        }

        // Process remaining text
        if last_end < raw_script.len() {
            text_buf.push_str(&raw_script[last_end..]);
        }
        let clean_text = text_buf.trim().to_string();
        if !clean_text.is_empty() {
            current_segment.text = clean_text;
            script.items.push(DirectedItem::Speech(current_segment));
        }

        script
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pauses_and_modifiers() {
        let director = EmojiDirector::default();
        let input = "Na biurku leżał 💥notes. 🛑 🐢 Zniszczony od wilgoci. 🛑";
        let script = director.parse(input);

        assert!(!script.items.is_empty());
        // Verify we have speech and silence items
        let silences: Vec<u32> = script
            .items
            .iter()
            .filter_map(|it| match it {
                DirectedItem::Silence(ms) => Some(*ms),
                _ => None,
            })
            .collect();
        assert_eq!(silences, vec![900, 900]);

        // Verify volume modification was applied on "notes"
        let speeches: Vec<&TextSegment> = script
            .items
            .iter()
            .filter_map(|it| match it {
                DirectedItem::Speech(seg) => Some(seg),
                _ => None,
            })
            .collect();
        assert!(speeches.iter().any(|s| s.text.contains("notes") && s.volume_mod == 15));
        assert!(speeches.iter().any(|s| s.text.contains("Zniszczony") && s.rate_mod == -14));
    }
}
