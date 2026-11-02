use std::collections::HashMap;
use regex::Regex;
use crate::types::{DirectedItem, DirectedScript, EmojiAction, TextSegment};

/// Configurable Emoji Director Grammar
#[derive(Debug, Clone)]
pub struct EmojiDirector {
    actions: HashMap<String, EmojiAction>,
    compiled_regex: Regex,
}

impl Default for EmojiDirector {
    fn default() -> Self {
        let mut m = HashMap::new();
        // Pauses (register both with and without VS16 variant selector \u{FE0F})
        m.insert("🛑".to_string(), EmojiAction::Pause(900));
        m.insert("⏸️".to_string(), EmojiAction::Pause(400));
        m.insert("⏸".to_string(), EmojiAction::Pause(400));
        m.insert("😮‍💨".to_string(), EmojiAction::Pause(350));
        
        // Dynamics & Tempo
        m.insert("🐢".to_string(), EmojiAction::RateMod(-14));
        m.insert("🐇".to_string(), EmojiAction::RateMod(15));
        m.insert("📉".to_string(), EmojiAction::PitchMod(-8));
        m.insert("📈".to_string(), EmojiAction::PitchMod(6));
        m.insert("💥".to_string(), EmojiAction::VolumeMod(15));
        m.insert("🤫".to_string(), EmojiAction::VolumeMod(-25));
        
        // DSP Filters & Acoustic Environments
        m.insert("📱".to_string(), EmojiAction::Filter("phone".to_string()));
        m.insert("📻".to_string(), EmojiAction::Filter("radio".to_string()));
        m.insert("📢".to_string(), EmojiAction::Filter("megaphone".to_string()));
        m.insert("🏛️".to_string(), EmojiAction::Filter("cathedral".to_string()));
        m.insert("🏛".to_string(), EmojiAction::Filter("cathedral".to_string()));
        m.insert("🏰".to_string(), EmojiAction::Filter("dungeon".to_string()));
        m.insert("💭".to_string(), EmojiAction::Filter("thought".to_string()));
        m.insert("🤖".to_string(), EmojiAction::Filter("robot".to_string()));

        let compiled = Self::build_regex(&m);
        Self { actions: m, compiled_regex: compiled }
    }
}

impl EmojiDirector {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_regex(actions: &HashMap<String, EmojiAction>) -> Regex {
        if actions.is_empty() {
            return Regex::new("$^").expect("Valid dummy regex");
        }
        // Sort keys by length descending to ensure compound/longer emojis match first
        let mut keys: Vec<&String> = actions.keys().collect();
        keys.sort_by_key(|k| std::cmp::Reverse(k.len()));
        let emojis_pattern: Vec<String> = keys.into_iter().map(|e| regex::escape(e)).collect();
        let re_str = format!("({})", emojis_pattern.join("|"));
        Regex::new(&re_str).expect("Valid emoji regex")
    }

    pub fn register(&mut self, emoji: impl Into<String>, action: EmojiAction) {
        self.actions.insert(emoji.into(), action);
        self.compiled_regex = Self::build_regex(&self.actions);
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

        let mut current_segment = TextSegment::default();
        let mut text_buf = String::new();
        let mut last_end = 0;
        let mut one_shot_volume = false;

        for mat in self.compiled_regex.find_iter(raw_script) {
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

                    // If previous segment had a one-shot emphasis, reset volume
                    if one_shot_volume {
                        current_segment.volume_mod = 0;
                        one_shot_volume = false;
                    }
                }

                // Apply action
                match action {
                    EmojiAction::Pause(ms) => {
                        script.items.push(DirectedItem::Silence(*ms));
                        // After full pause (like 🛑), reset pacing back to defaults
                        if *ms >= 800 {
                            current_segment = TextSegment::default();
                            one_shot_volume = false;
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
                        if *vol > 0 {
                            // 💥 is typically a single-word / single-phrase accent
                            one_shot_volume = true;
                        } else if *vol == -25 {
                            // 🤫 whisper also slows down slightly
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
        let silences: Vec<u32> = script
            .items
            .iter()
            .filter_map(|it| match it {
                DirectedItem::Silence(ms) => Some(*ms),
                _ => None,
            })
            .collect();
        assert_eq!(silences, vec![900, 900]);

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

    #[test]
    fn test_demo_effects_parsing() {
        let director = EmojiDirector::default();
        let input = r#"
Oto demonstracja zaawansowanych efektów akustycznych Audioticon. 🛑

Narrator mówi czystym, studyjnym głosem. ⏸️
Ale nagle odbiera telefon. 🛑
📱 "Halo? Richter, słyszysz mnie? Mamy problem na punkcie kontrolnym!" 🛑

W tle odzywa się wojskowa radiostacja krótkofalowa. 🛑
📻 "Baza jeden do patrolu. Zgłaszam naruszenie perymetru w sektorze czwartym. Odbiór." 🛑
"#;
        let script = director.parse(input);
        for (i, it) in script.items.iter().enumerate() {
            match it {
                DirectedItem::Silence(ms) => println!("[{:02}] SILENCE: {}ms", i, ms),
                DirectedItem::Speech(seg) => println!("[{:02}] SPEECH: filter={:?}, text={:?}", i, seg.filter, seg.text),
            }
        }
    }
}
