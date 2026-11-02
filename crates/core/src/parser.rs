use crate::presets::PresetRegistry;
use crate::types::{ActorTag, DirectedItem, DirectedScript, EmojiAction, TextSegment};
use crate::verbalizer::normalize_polish_text;
use regex::Regex;
use std::collections::HashMap;

/// Configurable Emoji Director Grammar
#[derive(Debug, Clone)]
pub struct EmojiDirector {
    actions: HashMap<String, EmojiAction>,
    compiled_regex: Regex,
    pub registry: PresetRegistry,
    pub lang: String,
}

impl Default for EmojiDirector {
    fn default() -> Self {
        let mut m = HashMap::new();
        // Pauses (register both with and without VS16 variant selector \u{FE0F})
        m.insert("🛑".to_string(), EmojiAction::Pause(900));
        m.insert("⏸️".to_string(), EmojiAction::Pause(400));
        m.insert("⏸".to_string(), EmojiAction::Pause(400));
        m.insert("😮‍💨".to_string(), EmojiAction::Breath(380));

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
        m.insert(
            "📢".to_string(),
            EmojiAction::Filter("megaphone".to_string()),
        );
        m.insert(
            "🏛️".to_string(),
            EmojiAction::Filter("cathedral".to_string()),
        );
        m.insert(
            "🏛".to_string(),
            EmojiAction::Filter("cathedral".to_string()),
        );
        m.insert("🏰".to_string(), EmojiAction::Filter("dungeon".to_string()));
        m.insert("💭".to_string(), EmojiAction::Filter("thought".to_string()));
        m.insert("🤖".to_string(), EmojiAction::Filter("robot".to_string()));

        // Structure & Chapter markers
        m.insert("📖".to_string(), EmojiAction::Chapter);
        m.insert("📑".to_string(), EmojiAction::Chapter);

        let compiled = Self::build_regex(&m);
        Self {
            actions: m,
            compiled_regex: compiled,
            registry: PresetRegistry::default(),
            lang: "pl".to_string(),
        }
    }
}

impl EmojiDirector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_registry(registry: PresetRegistry) -> Self {
        Self {
            registry,
            ..Default::default()
        }
    }

    pub fn with_lang(mut self, lang: impl Into<String>) -> Self {
        self.lang = lang.into();
        self
    }

    fn normalize_text(&self, text: &str) -> String {
        if self.lang.starts_with("en") {
            text.to_string()
        } else {
            normalize_polish_text(text)
        }
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
                return (
                    &trimmed[..token_end + punc_len],
                    trimmed[token_end + punc_len..].trim_start(),
                );
            }
        }
        match trimmed.find(char::is_whitespace) {
            Some(pos) => (&trimmed[..pos], trimmed[pos..].trim_start()),
            None => (trimmed, ""),
        }
    }

    /// Extracts optional suffix like ":wojskowe" or "[wojskowe]" following an emoji.
    /// Returns (Option<variant_name>, bytes_consumed).
    fn extract_variant_suffix(remainder: &str) -> (Option<&str>, usize) {
        if remainder.is_empty() {
            return (None, 0);
        }

        // Check for [variant_name]
        if remainder.starts_with('[') {
            if let Some(close_bracket) = remainder.find(']') {
                let inside = &remainder[1..close_bracket];
                if !inside.is_empty()
                    && inside
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                {
                    return (Some(inside), close_bracket + 1);
                }
            }
        }

        // Check for :variant_name (ensure followed by alphanumeric, NOT space or regular dialogue colon)
        if let Some(after_colon) = remainder.strip_prefix(':') {
            let name_len = after_colon
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .map(|c| c.len_utf8())
                .sum::<usize>();

            if name_len > 0 {
                return (Some(&after_colon[..name_len]), 1 + name_len);
            }
        }

        (None, 0)
    }

    /// Splits text at newline or sentence boundary to isolate a chapter title.
    fn split_chapter_title(s: &str) -> (&str, &str) {
        let trimmed = s.trim_start();
        if let Some(pos) = trimmed.find(['\n', '\r']) {
            let (first, remainder) = trimmed.split_at(pos);
            (first.trim(), remainder.trim_start())
        } else {
            (trimmed.trim(), "")
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
        let mut pending_chapter = false;

        for mat in self.compiled_regex.find_iter(raw_script) {
            let emoji_start = mat.start();
            let emoji_end = mat.end();

            // Process text before the emoji
            let before = &raw_script[last_end..emoji_start];
            if !before.is_empty() {
                if pending_chapter {
                    let (title, remainder) = Self::split_chapter_title(before);
                    if !title.is_empty() {
                        let clean_title = title.trim_end_matches(['.', ':', ';', ' ']).trim();
                        script
                            .items
                            .push(DirectedItem::Chapter(clean_title.to_string()));
                        let mut title_seg = current_segment.clone();
                        title_seg.text = self.normalize_text(title);
                        script.items.push(DirectedItem::Speech(title_seg));
                        pending_chapter = false;
                    }
                    if !remainder.is_empty() {
                        text_buf.push_str(remainder);
                    }
                } else if pending_one_shot_volume {
                    let (token, remainder) = Self::split_first_token(before);
                    if !token.is_empty() {
                        let mut emphasized = current_segment.clone();
                        emphasized.text = self.normalize_text(token.trim());
                        emphasized.actor_tags.push(ActorTag::Shout);
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
            let after_emoji = &raw_script[emoji_end..];
            let (variant, variant_bytes) = Self::extract_variant_suffix(after_emoji);
            let effective_end = emoji_end + variant_bytes;

            if let Some(action) = self.actions.get(emoji_str) {
                // If we have buffered text, flush it before the action
                let clean_text = text_buf.trim().to_string();
                if !clean_text.is_empty() {
                    current_segment.text = self.normalize_text(&clean_text);
                    script
                        .items
                        .push(DirectedItem::Speech(current_segment.clone()));
                    text_buf.clear();
                }

                let full_tag = match variant {
                    Some(v) => format!("{}:{}", emoji_str, v),
                    None => emoji_str.to_string(),
                };

                // Apply action
                match action {
                    EmojiAction::Pause(ms) => {
                        script.items.push(DirectedItem::Silence(*ms));
                        // After full pause (like 🛑), reset pacing and emotion tags back to defaults
                        if *ms >= 800 {
                            current_segment = TextSegment::default();
                            pending_one_shot_volume = false;
                        }
                    }
                    EmojiAction::Breath(ms) => {
                        script.items.push(DirectedItem::Breath(*ms));
                    }
                    EmojiAction::RateMod(rate) => {
                        let (preset, _) = self.registry.resolve(&full_tag);
                        current_segment.rate_mod = preset.rate.unwrap_or(*rate);
                        if current_segment.rate_mod < 0 {
                            current_segment.actor_tags.push(ActorTag::Hesitant);
                        } else if current_segment.rate_mod > 0 {
                            current_segment.actor_tags.push(ActorTag::Urgent);
                        }
                    }
                    EmojiAction::PitchMod(pitch) => {
                        let (preset, _) = self.registry.resolve(&full_tag);
                        current_segment.pitch_mod = preset.pitch.unwrap_or(*pitch);
                    }
                    EmojiAction::VolumeMod(vol) => {
                        current_segment.volume_mod = *vol;
                        if *vol > 0 {
                            pending_one_shot_volume = true;
                        } else if *vol == -25 {
                            // 🤫 whisper also slows down slightly, adds Whisper actor tag, and activates the whisper DSP acoustic filter
                            let (preset, warn) = self.registry.resolve(&full_tag);
                            if let Some(w) = warn {
                                eprintln!("⚠️ {}", w);
                            }
                            current_segment.rate_mod = preset.rate.unwrap_or(-10);
                            if let Some(p) = preset.pitch {
                                current_segment.pitch_mod = p;
                            }
                            if let Some(v) = preset.volume {
                                current_segment.volume_mod = ((v - 1.0) * 100.0) as i32;
                            }
                            current_segment.actor_tags.push(ActorTag::Whisper);
                            current_segment.filter = Some(full_tag);
                        }
                    }
                    EmojiAction::Filter(_base_name) => {
                        let (preset, warn) = self.registry.resolve(&full_tag);
                        if let Some(w) = warn {
                            eprintln!("⚠️ {}", w);
                        }
                        if let Some(v) = preset.volume {
                            current_segment.volume_mod = ((v - 1.0) * 100.0) as i32;
                        }
                        if let Some(rate) = preset.rate {
                            current_segment.rate_mod = rate;
                        }
                        if let Some(pitch) = preset.pitch {
                            current_segment.pitch_mod = pitch;
                        }
                        current_segment.filter = Some(full_tag);
                    }
                    EmojiAction::Chapter => {
                        pending_chapter = true;
                    }
                }
            }
            last_end = effective_end;
        }

        // Process remaining text
        if last_end < raw_script.len() {
            let tail = &raw_script[last_end..];
            if pending_chapter {
                let (title, remainder) = Self::split_chapter_title(tail);
                if !title.is_empty() {
                    let clean_title = title.trim_end_matches(['.', ':', ';', ' ']).trim();
                    script
                        .items
                        .push(DirectedItem::Chapter(clean_title.to_string()));
                    let mut title_seg = current_segment.clone();
                    title_seg.text = self.normalize_text(title);
                    script.items.push(DirectedItem::Speech(title_seg));
                }
                if !remainder.is_empty() {
                    text_buf.push_str(remainder);
                }
            } else if pending_one_shot_volume {
                let (token, remainder) = Self::split_first_token(tail);
                if !token.is_empty() {
                    let mut emphasized = current_segment.clone();
                    emphasized.text = self.normalize_text(token.trim());
                    emphasized.actor_tags.push(ActorTag::Shout);
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
            current_segment.text = self.normalize_text(&clean_text);
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
    fn test_verbalized_script() {
        let director = EmojiDirector::default();
        let input = "Data: 24.02.2022. 🛑 Godzina: 15:47. 🛑";
        let script = director.parse(input);

        let speeches: Vec<&TextSegment> = script
            .items
            .iter()
            .filter_map(|it| match it {
                DirectedItem::Speech(seg) => Some(seg),
                _ => None,
            })
            .collect();

        assert!(speeches[0].text.contains("dwudziestego czwartego lutego"));
        assert!(speeches[1].text.contains("piętnasta czterdzieści siedem"));
    }

    #[test]
    fn test_chapter_parsing() {
        let director = EmojiDirector::default();
        let input = "📖 Rozdział 1: Początek. 🛑 Adam wstał rano.";
        let script = director.parse(input);

        let mut chapters = Vec::new();
        for item in &script.items {
            if let DirectedItem::Chapter(ch) = item {
                chapters.push(ch.clone());
            }
        }

        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0], "Rozdział 1: Początek");
    }

    #[test]
    fn test_actor_tags_translation() {
        let director = EmojiDirector::default();
        let input = "Na stole stał 💥budzik. 🛑 🤫 Cicho, dziecko śpi. 🛑 Zmęczony 😮‍💨 usiadł.";
        let script = director.parse(input);

        let eleven = script.to_elevenlabs_script();
        assert!(eleven.contains("[shouts] budzik."));
        assert!(eleven.contains("[whispers] Cicho, dziecko śpi."));
        assert!(eleven.contains("[sighs]"));

        let chat = script.to_chattts_script();
        assert!(chat.contains("[shout] budzik."));
        assert!(chat.contains("[whisper] Cicho, dziecko śpi."));
        assert!(chat.contains("[sigh]"));
    }

    #[test]
    fn test_emoji_variants() {
        let director = EmojiDirector::default();
        let input = "Komunikat: 📻:wojskowe Uwaga patrol! 🛑 A przez komórkę 📱[stary] Odbiór. 🛑";
        let script = director.parse(input);

        let speeches: Vec<&TextSegment> = script
            .items
            .iter()
            .filter_map(|it| match it {
                DirectedItem::Speech(seg) => Some(seg),
                _ => None,
            })
            .collect();

        assert_eq!(speeches[1].text, "Uwaga patrol!");
        assert_eq!(speeches[1].filter.as_deref(), Some("📻:wojskowe"));

        assert_eq!(speeches[3].text, "Odbiór.");
        assert_eq!(speeches[3].filter.as_deref(), Some("📱:stary"));
    }

    #[test]
    fn test_dialogue_colon_not_treated_as_variant() {
        let director = EmojiDirector::default();
        let input = "Powiedział 🤫: Nie budź ich.";
        let script = director.parse(input);

        let speeches: Vec<&TextSegment> = script
            .items
            .iter()
            .filter_map(|it| match it {
                DirectedItem::Speech(seg) => Some(seg),
                _ => None,
            })
            .collect();

        assert_eq!(speeches[0].text, "Powiedział");
        assert_eq!(speeches[1].text, ": Nie budź ich.");
        assert_eq!(speeches[1].filter.as_deref(), Some("🤫"));
    }
}
