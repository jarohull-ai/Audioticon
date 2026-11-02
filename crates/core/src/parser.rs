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

    /// Splits the immediate first word (or quoted phrase) for one-shot emphasis modifiers like 💥
    fn split_first_token(s: &str) -> (&str, &str) {
        let trimmed = s.trim_start();
        if let Some(after_quote) = trimmed.strip_prefix('"') {
            if let Some(end_quote) = after_quote.find('"') {
                let token_end = 1 + end_quote + 1;
                let punc_len = trimmed[token_end..]
                    .chars()
                    .take_while(|c| c.is_ascii_punctuation() && !c.is_whitespace())
                    .count();
                return (&trimmed[..token_end + punc_len], trimmed[token_end + punc_len..].trim_start());
            }
        }
        match trimmed.find(char::is_whitespace) {
            Some(pos) => (&trimmed[..pos], trimmed[pos..].trim_start()),
            None => (trimmed, ""),
        }
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
        let mut pending_one_shot_volume = false;

        for mat in self.compiled_regex.find_iter(raw_script) {
            // Process text before the emoji
            let before = &raw_script[last_end..mat.start()];
            if !before.is_empty() {
                if pending_one_shot_volume {
                    let (token, remainder) = Self::split_first_token(before);
                    if !token.is_empty() {
                        let mut emphasized = current_segment.clone();
                        emphasized.text = token.trim().to_string();
                        script.items.push(DirectedItem::Speech(emphasized));
                        // Immediately reset volume for rest of sentence
                        current_segment.volume_mod = 0;
                        pending_one_shot_volume = false;
                    }
                    if !remainder.is_empty() {
                        text_buf.push_str(remainder);
                    }
                } else {
                    text_buf.push_str(before);
                }
            }

            let emoji_str = mat.as_str();
            if let Some(action) = self.actions.get(emoji_str) {
                // If we have buffered text, flush it before the action
                let clean_text = text_buf.trim().to_string();
                if !clean_text.is_empty() {
                    current_segment.text = clean_text;
                    script.items.push(DirectedItem::Speech(current_segment.clone()));
                    text_buf.clear();
                }

                // Apply action
                match action {
                    EmojiAction::Pause(ms) => {
                        script.items.push(DirectedItem::Silence(*ms));
                        // After full pause (like 🛑), reset pacing back to defaults
                        if *ms >= 800 {
                            current_segment = TextSegment::default();
                            pending_one_shot_volume = false;
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
                            pending_one_shot_volume = true;
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
            let tail = &raw_script[last_end..];
            if pending_one_shot_volume {
                let (token, remainder) = Self::split_first_token(tail);
                if !token.is_empty() {
                    let mut emphasized = current_segment.clone();
                    emphasized.text = token.trim().to_string();
                    script.items.push(DirectedItem::Speech(emphasized));
                    current_segment.volume_mod = 0;
                }
                if !remainder.is_empty() {
                    text_buf.push_str(remainder);
                }
            } else {
                text_buf.push_str(tail);
            }
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
    fn test_single_word_explosion() {
        let director = EmojiDirector::default();
        let input = "Na biurku leżał 💥notes. A potem długo nic się nie działo.";
        let script = director.parse(input);

        let speeches: Vec<&TextSegment> = script
            .items
            .iter()
            .filter_map(|it| match it {
                DirectedItem::Speech(seg) => Some(seg),
                _ => None,
            })
            .collect();

        assert_eq!(speeches.len(), 3);
        assert_eq!(speeches[0].text, "Na biurku leżał");
        assert_eq!(speeches[0].volume_mod, 0);

        assert_eq!(speeches[1].text, "notes.");
        assert_eq!(speeches[1].volume_mod, 15);

        assert_eq!(speeches[2].text, "A potem długo nic się nie działo.");
        assert_eq!(speeches[2].volume_mod, 0);
    }

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
    }
}
