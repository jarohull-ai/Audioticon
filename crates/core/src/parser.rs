use crate::presets::PresetRegistry;
use crate::types::{
    ActorTag, DirectedItem, DirectedScript, EmojiAction, ProductionStyle, TextSegment,
};
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
    pub style: ProductionStyle,
}

impl Default for EmojiDirector {
    fn default() -> Self {
        let mut m = HashMap::new();
        // Pauses (register both with and without VS16 variant selector \u{FE0F})
        m.insert("🛑".to_string(), EmojiAction::Pause(900));
        m.insert("[stop]".to_string(), EmojiAction::Pause(900));
        m.insert("⏸️".to_string(), EmojiAction::Pause(400));
        m.insert("⏸".to_string(), EmojiAction::Pause(400));
        m.insert("[pauza]".to_string(), EmojiAction::Pause(400));
        m.insert("[pause]".to_string(), EmojiAction::Pause(400));
        m.insert("😮‍💨".to_string(), EmojiAction::Breath(380));
        m.insert("[oddech]".to_string(), EmojiAction::Breath(380));
        m.insert("[sigh]".to_string(), EmojiAction::Breath(380));

        // Expressive Emotions (Actor Tags for generative voice models)
        m.insert("😊".to_string(), EmojiAction::Emotion(ActorTag::Happy));
        m.insert("[happy]".to_string(), EmojiAction::Emotion(ActorTag::Happy));
        m.insert(
            "[radosc]".to_string(),
            EmojiAction::Emotion(ActorTag::Happy),
        );
        m.insert(
            "[radość]".to_string(),
            EmojiAction::Emotion(ActorTag::Happy),
        );

        m.insert("😢".to_string(), EmojiAction::Emotion(ActorTag::Sad));
        m.insert("😭".to_string(), EmojiAction::Emotion(ActorTag::Sad));
        m.insert("[sad]".to_string(), EmojiAction::Emotion(ActorTag::Sad));
        m.insert("[smutek]".to_string(), EmojiAction::Emotion(ActorTag::Sad));

        m.insert("😡".to_string(), EmojiAction::Emotion(ActorTag::Angry));
        m.insert("🤬".to_string(), EmojiAction::Emotion(ActorTag::Angry));
        m.insert("[angry]".to_string(), EmojiAction::Emotion(ActorTag::Angry));
        m.insert("[gniew]".to_string(), EmojiAction::Emotion(ActorTag::Angry));
        m.insert("[zlosc]".to_string(), EmojiAction::Emotion(ActorTag::Angry));
        m.insert("[złość]".to_string(), EmojiAction::Emotion(ActorTag::Angry));

        m.insert("😲".to_string(), EmojiAction::Emotion(ActorTag::Surprised));
        m.insert("😮".to_string(), EmojiAction::Emotion(ActorTag::Surprised));
        m.insert(
            "[surprised]".to_string(),
            EmojiAction::Emotion(ActorTag::Surprised),
        );
        m.insert(
            "[szok]".to_string(),
            EmojiAction::Emotion(ActorTag::Surprised),
        );
        m.insert(
            "[zaskoczenie]".to_string(),
            EmojiAction::Emotion(ActorTag::Surprised),
        );

        m.insert("😰".to_string(), EmojiAction::Emotion(ActorTag::Nervous));
        m.insert("😨".to_string(), EmojiAction::Emotion(ActorTag::Nervous));
        m.insert(
            "[nervous]".to_string(),
            EmojiAction::Emotion(ActorTag::Nervous),
        );
        m.insert(
            "[nerwy]".to_string(),
            EmojiAction::Emotion(ActorTag::Nervous),
        );
        m.insert(
            "[nerwowo]".to_string(),
            EmojiAction::Emotion(ActorTag::Nervous),
        );

        m.insert("🤢".to_string(), EmojiAction::Emotion(ActorTag::Disgusted));
        m.insert(
            "[disgusted]".to_string(),
            EmojiAction::Emotion(ActorTag::Disgusted),
        );
        m.insert(
            "[obrzydzenie]".to_string(),
            EmojiAction::Emotion(ActorTag::Disgusted),
        );
        m.insert(
            "[odraza]".to_string(),
            EmojiAction::Emotion(ActorTag::Disgusted),
        );

        // Dynamics & Tempo
        m.insert("🐢".to_string(), EmojiAction::RateMod(-14));
        m.insert("[wolno]".to_string(), EmojiAction::RateMod(-14));
        m.insert("[slow]".to_string(), EmojiAction::RateMod(-14));
        m.insert("🐇".to_string(), EmojiAction::RateMod(15));
        m.insert("[szybko]".to_string(), EmojiAction::RateMod(15));
        m.insert("[fast]".to_string(), EmojiAction::RateMod(15));
        m.insert("📉".to_string(), EmojiAction::PitchMod(-8));
        m.insert("[niski]".to_string(), EmojiAction::PitchMod(-8));
        m.insert("📈".to_string(), EmojiAction::PitchMod(6));
        m.insert("[wysoki]".to_string(), EmojiAction::PitchMod(6));
        m.insert("💥".to_string(), EmojiAction::VolumeMod(15));
        m.insert("[akcent]".to_string(), EmojiAction::VolumeMod(15));
        m.insert("[boom]".to_string(), EmojiAction::VolumeMod(15));
        m.insert("🤫".to_string(), EmojiAction::VolumeMod(-25));
        m.insert("[szept]".to_string(), EmojiAction::VolumeMod(-25));
        m.insert("[whisper]".to_string(), EmojiAction::VolumeMod(-25));
        m.insert("💋".to_string(), EmojiAction::VolumeMod(-25));
        m.insert("🪖".to_string(), EmojiAction::VolumeMod(-25));

        // DSP Filters & Acoustic Environments
        m.insert("📱".to_string(), EmojiAction::Filter("phone".to_string()));
        m.insert(
            "[telefon]".to_string(),
            EmojiAction::Filter("phone".to_string()),
        );
        m.insert(
            "[phone]".to_string(),
            EmojiAction::Filter("phone".to_string()),
        );
        m.insert("📻".to_string(), EmojiAction::Filter("radio".to_string()));
        m.insert(
            "[radio]".to_string(),
            EmojiAction::Filter("radio".to_string()),
        );
        m.insert(
            "📢".to_string(),
            EmojiAction::Filter("megaphone".to_string()),
        );
        m.insert(
            "[megafon]".to_string(),
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
        m.insert(
            "[katedra]".to_string(),
            EmojiAction::Filter("cathedral".to_string()),
        );
        m.insert(
            "[cathedral]".to_string(),
            EmojiAction::Filter("cathedral".to_string()),
        );
        m.insert("🏰".to_string(), EmojiAction::Filter("dungeon".to_string()));
        m.insert(
            "[lochy]".to_string(),
            EmojiAction::Filter("dungeon".to_string()),
        );
        m.insert(
            "[dungeon]".to_string(),
            EmojiAction::Filter("dungeon".to_string()),
        );
        m.insert("💭".to_string(), EmojiAction::Filter("thought".to_string()));
        m.insert(
            "[mysli]".to_string(),
            EmojiAction::Filter("thought".to_string()),
        );
        m.insert(
            "[myśli]".to_string(),
            EmojiAction::Filter("thought".to_string()),
        );
        m.insert("🤖".to_string(), EmojiAction::Filter("robot".to_string()));
        m.insert(
            "[robot]".to_string(),
            EmojiAction::Filter("robot".to_string()),
        );

        // Studio, Warmth, Brightness & Raw Audio presets
        m.insert(
            "[studio]".to_string(),
            EmojiAction::Filter("studio".to_string()),
        );
        m.insert("☕".to_string(), EmojiAction::Filter("warm".to_string()));
        m.insert(
            "[warm]".to_string(),
            EmojiAction::Filter("warm".to_string()),
        );
        m.insert(
            "[cieply]".to_string(),
            EmojiAction::Filter("warm".to_string()),
        );
        m.insert(
            "[ciepły]".to_string(),
            EmojiAction::Filter("warm".to_string()),
        );
        m.insert("✨".to_string(), EmojiAction::Filter("bright".to_string()));
        m.insert(
            "[bright]".to_string(),
            EmojiAction::Filter("bright".to_string()),
        );
        m.insert(
            "[jasny]".to_string(),
            EmojiAction::Filter("bright".to_string()),
        );
        m.insert("[raw]".to_string(), EmojiAction::Filter("raw".to_string()));
        m.insert(
            "[surowy]".to_string(),
            EmojiAction::Filter("raw".to_string()),
        );

        // Disentangled Vocal Expression Styles (Shout / Call / Chant)
        m.insert(
            "[rozkaz]".to_string(),
            EmojiAction::Filter("rozkaz".to_string()),
        );
        m.insert(
            "[krzyk]".to_string(),
            EmojiAction::Filter("rozkaz".to_string()),
        );
        m.insert(
            "[wolanie]".to_string(),
            EmojiAction::Filter("wolanie".to_string()),
        );
        m.insert(
            "[wołanie]".to_string(),
            EmojiAction::Filter("wolanie".to_string()),
        );
        m.insert("🎵".to_string(), EmojiAction::Filter("spiew".to_string()));
        m.insert("🎶".to_string(), EmojiAction::Filter("spiew".to_string()));
        m.insert(
            "[spiew]".to_string(),
            EmojiAction::Filter("spiew".to_string()),
        );
        m.insert(
            "[śpiew]".to_string(),
            EmojiAction::Filter("spiew".to_string()),
        );

        // Multi-Voice Casting & Characters
        m.insert("👩".to_string(), EmojiAction::Voice("female".to_string()));
        m.insert("👨".to_string(), EmojiAction::Voice("male".to_string()));
        m.insert("🎙️".to_string(), EmojiAction::Voice("narrator".to_string()));
        m.insert("🎙".to_string(), EmojiAction::Voice("narrator".to_string()));
        m.insert(
            "[glos]".to_string(),
            EmojiAction::Voice("default".to_string()),
        );
        m.insert(
            "[głos]".to_string(),
            EmojiAction::Voice("default".to_string()),
        );
        m.insert(
            "[voice]".to_string(),
            EmojiAction::Voice("default".to_string()),
        );

        // Sound Effects (Foley / SFX)
        m.insert(
            "🔔".to_string(),
            EmojiAction::SoundEffect("bell".to_string()),
        );
        m.insert(
            "🚪".to_string(),
            EmojiAction::SoundEffect("door".to_string()),
        );
        m.insert(
            "👣".to_string(),
            EmojiAction::SoundEffect("footsteps".to_string()),
        );
        m.insert(
            "📄".to_string(),
            EmojiAction::SoundEffect("paper".to_string()),
        );
        m.insert(
            "🚗".to_string(),
            EmojiAction::SoundEffect("car".to_string()),
        );
        m.insert(
            "⚡".to_string(),
            EmojiAction::SoundEffect("thunder".to_string()),
        );
        m.insert(
            "[sfx]".to_string(),
            EmojiAction::SoundEffect("bell".to_string()),
        );

        // Ambient Soundscapes
        m.insert(
            "❄️".to_string(),
            EmojiAction::Ambience(Some("wind".to_string())),
        );
        m.insert(
            "❄".to_string(),
            EmojiAction::Ambience(Some("wind".to_string())),
        );
        m.insert(
            "🌧️".to_string(),
            EmojiAction::Ambience(Some("rain".to_string())),
        );
        m.insert(
            "🌧".to_string(),
            EmojiAction::Ambience(Some("rain".to_string())),
        );
        m.insert(
            "🏙️".to_string(),
            EmojiAction::Ambience(Some("city".to_string())),
        );
        m.insert(
            "🏙".to_string(),
            EmojiAction::Ambience(Some("city".to_string())),
        );
        m.insert(
            "[ambient]".to_string(),
            EmojiAction::Ambience(Some("wind".to_string())),
        );

        // Structure & Chapter markers
        m.insert("📖".to_string(), EmojiAction::Chapter);
        m.insert("📑".to_string(), EmojiAction::Chapter);
        m.insert("[rozdzial]".to_string(), EmojiAction::Chapter);
        m.insert("[rozdział]".to_string(), EmojiAction::Chapter);
        m.insert("[chapter]".to_string(), EmojiAction::Chapter);

        let compiled = Self::build_regex(&m);
        Self {
            actions: m,
            compiled_regex: compiled,
            registry: PresetRegistry::default(),
            lang: "pl".to_string(),
            style: ProductionStyle::default(),
        }
    }
}

impl EmojiDirector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_style(mut self, style: ProductionStyle) -> Self {
        self.style = style;
        self
    }

    pub fn with_registry(registry: PresetRegistry) -> Self {
        let mut director = Self::default();
        for (emoji, def) in registry.custom_emojis() {
            director.register_custom_emoji_def(emoji, def);
        }
        director.registry = registry;
        director
    }

    /// Registers a custom emoji definition into active parser actions and updates regex
    pub fn register_custom_emoji_def(&mut self, emoji: &str, def: &crate::presets::CustomEmojiDef) {
        if def.action_type == "voice" {
            let v_name = def.voice.clone().unwrap_or_else(|| "default".to_string());
            self.actions
                .insert(emoji.to_string(), EmojiAction::Voice(v_name.clone()));
            if let Some(alias) = &def.alias {
                self.actions
                    .insert(format!("[{}]", alias), EmojiAction::Voice(v_name));
            }
        } else if def.action_type == "filter" {
            let f_name = emoji.to_string();
            self.actions
                .insert(emoji.to_string(), EmojiAction::Filter(f_name.clone()));
            if let Some(alias) = &def.alias {
                self.actions
                    .insert(format!("[{}]", alias), EmojiAction::Filter(f_name));
            }
        } else if def.action_type == "modifier" {
            if let Some(p) = def.pitch {
                self.actions
                    .insert(emoji.to_string(), EmojiAction::PitchMod(p));
                if let Some(alias) = &def.alias {
                    self.actions
                        .insert(format!("[{}]", alias), EmojiAction::PitchMod(p));
                }
            } else if let Some(r) = def.rate {
                self.actions
                    .insert(emoji.to_string(), EmojiAction::RateMod(r));
                if let Some(alias) = &def.alias {
                    self.actions
                        .insert(format!("[{}]", alias), EmojiAction::RateMod(r));
                }
            } else if let Some(v) = def.volume {
                let vol_mod = ((v - 1.0) * 100.0) as i32;
                self.actions
                    .insert(emoji.to_string(), EmojiAction::VolumeMod(vol_mod));
                if let Some(alias) = &def.alias {
                    self.actions
                        .insert(format!("[{}]", alias), EmojiAction::VolumeMod(vol_mod));
                }
            }
        } else if def.action_type == "sfx" {
            let sfx_name = def.filter.clone().unwrap_or_else(|| "bell".to_string());
            self.actions
                .insert(emoji.to_string(), EmojiAction::SoundEffect(sfx_name.clone()));
            if let Some(alias) = &def.alias {
                self.actions
                    .insert(format!("[{}]", alias), EmojiAction::SoundEffect(sfx_name));
            }
        } else if def.action_type == "ambient" {
            let amb_name = def.filter.clone().unwrap_or_else(|| "wind".to_string());
            self.actions
                .insert(emoji.to_string(), EmojiAction::Ambience(Some(amb_name.clone())));
            if let Some(alias) = &def.alias {
                self.actions
                    .insert(format!("[{}]", alias), EmojiAction::Ambience(Some(amb_name)));
            }
        }
        self.compiled_regex = Self::build_regex(&self.actions);
    }

    /// Resolves character names or voice aliases to concrete neural voice models
    pub fn resolve_voice(&self, name: &str) -> String {
        let clean = name.trim().to_lowercase();
        if let Some(v) = self.registry.get_voice(&clean) {
            return v.to_string();
        }
        match clean.as_str() {
            "zofia" | "kobieta" | "female" | "ona" | "woman" => "pl-PL-ZofiaNeural".to_string(),
            "marek" | "mezczyzna" | "mężczyzna" | "male" | "on" | "man" => {
                "pl-PL-MarekNeural".to_string()
            }
            "agnieszka" => "pl-PL-AgnieszkaNeural".to_string(),
            "darkman" => "pl_PL-darkman-medium".to_string(),
            "gosia" => "pl_PL-gosia-medium".to_string(),
            "ryan" => "en-US-RyanMultilingualNeural".to_string(),
            "jenny" => "en-US-JennyNeural".to_string(),
            "guy" => "en-US-GuyNeural".to_string(),
            "narrator" | "lektor" | "default" | "domyslny" | "domyślny" => "default".to_string(),
            other => other.to_string(),
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

    /// Canonical baseline acoustics (pitch_mod, rate_mod) for speakers and characters.
    pub fn base_acoustics_for_speaker(voice: Option<&str>) -> (i32, i32) {
        match voice {
            Some(v) => {
                let clean = v.trim().to_lowercase();
                // 1. Check female characters first so "female" is never misclassified by "male" substring
                if clean.contains("zofia")
                    || clean.contains("gosia")
                    || clean.contains("female")
                    || clean.contains("kobieta")
                    || clean.contains("woman")
                    || clean.split(['-', '_', ' ']).any(|t| t == "ona")
                {
                    // Female character: clear natural feminine pitch (0Hz) and pacing (-6%)
                    (0, -6)
                } else if clean.contains("darkman") {
                    // Darkman is Piper's canonical Polish lektor/narrator voice (natural pitch -4Hz, pacing -6%)
                    (-4, -6)
                } else if clean.contains("marek")
                    || clean.contains("mc_speech")
                    || clean.split(['-', '_', ' ']).any(|t| {
                        t == "male"
                            || t == "man"
                            || t == "mezczyzna"
                            || t == "mężczyzna"
                            || t == "on"
                    })
                {
                    // Male character in dialogue: deep -20Hz contrast, deliberate -10% pacing
                    (-20, -10)
                } else {
                    // Default character
                    (-4, -6)
                }
            }
            // Narrator (default lektor)
            None => (-4, -6),
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
        let re_str = format!(
            "(\\[[a-zA-Ząćęłńóśźż_]+(?::[a-zA-Ząćęłńóśźż0-9_-]+)?\\]|{})",
            emojis_pattern.join("|")
        );
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
                let trailing_bracket = if after_colon[name_len..].starts_with(']') {
                    1
                } else {
                    0
                };
                return (
                    Some(&after_colon[..name_len]),
                    1 + name_len + trailing_bracket,
                );
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

            if emoji_start < last_end {
                continue;
            }

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

            let matched_str = mat.as_str();
            let (lookup_key, variant, effective_end) =
                if matched_str.starts_with('[') && matched_str.ends_with(']') {
                    let inside = &matched_str[1..matched_str.len() - 1];
                    if let Some((base, var)) = inside.split_once(':') {
                        (format!("[{}]", base), Some(var), emoji_end)
                    } else {
                        (matched_str.to_string(), None, emoji_end)
                    }
                } else {
                    let after_emoji = &raw_script[emoji_end..];
                    let (v, v_bytes) = Self::extract_variant_suffix(after_emoji);
                    (matched_str.to_string(), v, emoji_end + v_bytes)
                };

            if let Some(action) = self.actions.get(&lookup_key) {
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
                    Some(v) => format!("{}:{}", lookup_key, v),
                    None => lookup_key.clone(),
                };

                // Apply action
                match action {
                    EmojiAction::Pause(ms) => {
                        script.items.push(DirectedItem::Silence(*ms));
                        // After full pause (like 🛑), reset pacing and emotion tags back to the
                        // active speaker's canonical baseline acoustics:
                        if *ms >= 800 {
                            let current_voice = if self.style == ProductionStyle::Narrator {
                                None
                            } else {
                                current_segment.voice.clone()
                            };
                            let current_pan = if self.style == ProductionStyle::AudioDrama {
                                current_segment.pan
                            } else {
                                0.0
                            };
                            current_segment = TextSegment::default();
                            current_segment.voice = current_voice.clone();
                            current_segment.pan = current_pan;
                            let (base_pitch, base_rate) =
                                Self::base_acoustics_for_speaker(current_voice.as_deref());
                            current_segment.pitch_mod = base_pitch;
                            current_segment.rate_mod = base_rate;
                            pending_one_shot_volume = false;
                        }
                    }
                    EmojiAction::Breath(ms) => {
                        script.items.push(DirectedItem::Breath(*ms));
                    }
                    EmojiAction::Emotion(tag) => {
                        // Replace any existing emotion tags with the newly triggered one
                        current_segment.actor_tags.retain(|t| {
                            !matches!(
                                t,
                                ActorTag::Happy
                                    | ActorTag::Sad
                                    | ActorTag::Angry
                                    | ActorTag::Surprised
                                    | ActorTag::Nervous
                                    | ActorTag::Disgusted
                            )
                        });
                        current_segment.actor_tags.push(tag.clone());
                        match tag {
                            ActorTag::Happy => {
                                current_segment.volume_mod = 4;
                                current_segment.pitch_mod = 3;
                            }
                            ActorTag::Sad => {
                                current_segment.volume_mod = -10;
                                current_segment.pitch_mod = -4;
                                current_segment.rate_mod = -8;
                            }
                            ActorTag::Angry => {
                                current_segment.volume_mod = 15;
                                current_segment.pitch_mod = 2;
                            }
                            ActorTag::Surprised => {
                                current_segment.pitch_mod = 8;
                                current_segment.rate_mod = 6;
                            }
                            ActorTag::Nervous => {
                                current_segment.pitch_mod = 4;
                                current_segment.rate_mod = 10;
                            }
                            ActorTag::Disgusted => {
                                current_segment.pitch_mod = -5;
                                current_segment.rate_mod = -6;
                            }
                            _ => {}
                        }
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
                            // 🤫 whisper also slows down slightly, adds Whisper actor tag,
                            // and activates the whisper DSP acoustic filter
                            let resolved_tag = if full_tag == "💋" {
                                "🤫:romantyczny".to_string()
                            } else if full_tag == "🪖" {
                                "🤫:akcja".to_string()
                            } else {
                                full_tag.clone()
                            };
                            let (preset, warn) = self.registry.resolve(&resolved_tag);
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
                            if self.style == ProductionStyle::AudioDrama
                                || resolved_tag.contains(':')
                            {
                                current_segment.filter = Some(resolved_tag);
                            } else {
                                current_segment.filter = None;
                            }
                        }
                    }
                    EmojiAction::Filter(base_name) => {
                        if let Some(style) = self
                            .registry
                            .resolve_style(&full_tag)
                            .or_else(|| self.registry.resolve_style(base_name))
                        {
                            current_segment.rate_mod = style.pacing_mod_pct();
                            current_segment.volume_mod =
                                ((style.output_gain() - 1.0) * 100.0) as i32;
                            current_segment.filter = Some(format!("style:{}", style.alias_or_id()));
                        } else if self.style == ProductionStyle::AudioDrama {
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
                        } else {
                            // In Narrator and MultiVoice modes, bypass acoustic environment distortions
                            current_segment.filter = None;
                        }
                    }
                    EmojiAction::Chapter => {
                        pending_chapter = true;
                    }
                    EmojiAction::Voice(default_name) => {
                        if self.style == ProductionStyle::Narrator {
                            // In single Narrator mode, keep entire book voiced by the unified narrator
                            current_segment.voice = None;
                            current_segment.pitch_mod = 0;
                            current_segment.rate_mod = 0;
                            current_segment.pan = 0.0;
                        } else {
                            let voice_spec = variant.unwrap_or(default_name);
                            let resolved = self.resolve_voice(voice_spec);
                            if resolved == "default"
                                || resolved == "narrator"
                                || resolved.is_empty()
                            {
                                current_segment.voice = None;
                                current_segment.pan = 0.0;
                            } else {
                                current_segment.voice = Some(resolved);
                                if self.style == ProductionStyle::AudioDrama {
                                    let v_lower = voice_spec.to_lowercase();
                                    if v_lower.contains("kobieta")
                                        || v_lower.contains("female")
                                        || v_lower.contains("zofia")
                                    {
                                        current_segment.pan = 0.22;
                                    } else if v_lower.contains("mezczyzna")
                                        || v_lower.contains("mężczyzna")
                                        || v_lower.contains("male")
                                        || v_lower.contains("marek")
                                    {
                                        current_segment.pan = -0.22;
                                    } else {
                                        current_segment.pan = 0.0;
                                    }
                                } else {
                                    current_segment.pan = 0.0;
                                }
                            }
                            // Explicitly apply the speaker's canonical baseline acoustics upon every speaker switch
                            let (base_pitch, base_rate) =
                                Self::base_acoustics_for_speaker(current_segment.voice.as_deref());
                            current_segment.pitch_mod = base_pitch;
                            current_segment.rate_mod = base_rate;
                        }
                    }
                    EmojiAction::SoundEffect(default_name) => {
                        if self.style == ProductionStyle::AudioDrama {
                            let effect_name = variant.unwrap_or(default_name).to_string();
                            script.items.push(DirectedItem::SoundEffect(effect_name));
                        }
                    }
                    EmojiAction::Ambience(default_amb) => {
                        if self.style == ProductionStyle::AudioDrama {
                            let amb_val = match variant {
                                Some("off") | Some("stop") | Some("none") => None,
                                Some(v) => Some(v.to_string()),
                                None => default_amb.clone(),
                            };
                            script.items.push(DirectedItem::Ambience(amb_val));
                        }
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

    #[test]
    fn test_text_bracket_aliases() {
        let director = EmojiDirector::default();
        let input = "[rozdzial] Wstęp [stop] [telefon] Halo baza? [pauza] [radio:wojskowe] Zgłaszam się. [stop]";
        let script = director.parse(input);

        assert!(matches!(&script.items[0], DirectedItem::Chapter(c) if c == "Wstęp"));

        let speeches: Vec<&TextSegment> = script
            .items
            .iter()
            .filter_map(|it| match it {
                DirectedItem::Speech(seg) => Some(seg),
                _ => None,
            })
            .collect();

        // 0: title, 1: phone speech, 2: radio speech
        assert_eq!(speeches[0].text, "Wstęp");
        assert_eq!(speeches[1].text, "Halo baza?");
        assert_eq!(speeches[1].filter.as_deref(), Some("[telefon]"));
        assert_eq!(speeches[2].text, "Zgłaszam się.");
        assert_eq!(speeches[2].filter.as_deref(), Some("[radio]:wojskowe"));
    }

    #[test]
    fn test_multi_voice_casting() {
        let director = EmojiDirector::default();
        let input = "Narrator mówił spokojnie. 🛑 👩 \"Nie boję się.\" 🛑 👨 \"To dobrze.\" 🛑 [głos:zofia] \"Spotkamy się jutro.\" 🛑 [głos:narrator] I zapadł zmierzch.";
        let script = director.parse(input);

        let speeches: Vec<&TextSegment> = script
            .items
            .iter()
            .filter_map(|it| match it {
                DirectedItem::Speech(seg) => Some(seg),
                _ => None,
            })
            .collect();

        // 0: Narrator (default -> None)
        assert_eq!(speeches[0].text, "Narrator mówił spokojnie.");
        assert_eq!(speeches[0].voice, None);
        assert_eq!(speeches[0].pitch_mod, -4);
        assert_eq!(speeches[0].rate_mod, -6);

        // 1: 👩 (female -> "pl-PL-ZofiaNeural")
        assert_eq!(speeches[1].text, "\"Nie boję się.\"");
        assert_eq!(speeches[1].voice.as_deref(), Some("pl-PL-ZofiaNeural"));
        assert_eq!(speeches[1].pitch_mod, 0);
        assert_eq!(speeches[1].rate_mod, -6);

        // 2: 👨 (male -> "pl-PL-MarekNeural")
        assert_eq!(speeches[2].text, "\"To dobrze.\"");
        assert_eq!(speeches[2].voice.as_deref(), Some("pl-PL-MarekNeural"));
        assert_eq!(speeches[2].pitch_mod, -20);
        assert_eq!(speeches[2].rate_mod, -10);

        // 3: [głos:zofia]
        assert_eq!(speeches[3].text, "\"Spotkamy się jutro.\"");
        assert_eq!(speeches[3].voice.as_deref(), Some("pl-PL-ZofiaNeural"));
        assert_eq!(speeches[3].pitch_mod, 0);
        assert_eq!(speeches[3].rate_mod, -6);

        // 4: [głos:narrator] -> None (resets back to narrator default acoustics)
        assert_eq!(speeches[4].text, "I zapadł zmierzch.");
        assert_eq!(speeches[4].voice, None);
        assert_eq!(speeches[4].pitch_mod, -4);
        assert_eq!(speeches[4].rate_mod, -6);
    }

    #[test]
    fn test_narrator_vs_male_character_cache_differentiation() {
        let director = EmojiDirector::default();
        let script_narrator = director.parse("🎙️ Stój! Kto idzie przez bramę?");
        let script_guard = director.parse("👨 Stój! Kto idzie przez bramę?");

        let seg_narrator = match &script_narrator.items[0] {
            DirectedItem::Speech(s) => s,
            _ => panic!("Expected speech"),
        };
        let seg_guard = match &script_guard.items[0] {
            DirectedItem::Speech(s) => s,
            _ => panic!("Expected speech"),
        };

        let hash_narrator = crate::cache::compute_segment_hash(seg_narrator, "pl-PL-MarekNeural");
        let hash_guard = crate::cache::compute_segment_hash(seg_guard, "pl-PL-MarekNeural");

        assert_ne!(
            hash_narrator, hash_guard,
            "Narrator and male character must have distinct cache hashes and audio files"
        );
        assert_eq!(seg_narrator.pitch_mod, -4);
        assert_eq!(seg_guard.pitch_mod, -20);
    }

    #[test]
    fn test_voice_switch_male_to_female_does_not_leak_acoustics() {
        let director = EmojiDirector::default();
        let script = director.parse("👨 Tak. 👩 Stój! Kto idzie?");

        let speeches: Vec<&TextSegment> = script
            .items
            .iter()
            .filter_map(|it| match it {
                DirectedItem::Speech(seg) => Some(seg),
                _ => None,
            })
            .collect();

        assert_eq!(speeches.len(), 2);
        // Male speaker
        assert_eq!(speeches[0].text, "Tak.");
        assert_eq!(speeches[0].pitch_mod, -20);
        assert_eq!(speeches[0].rate_mod, -10);

        // Female speaker must NOT inherit male pitch or rate
        assert_eq!(speeches[1].text, "Stój! Kto idzie?");
        assert_eq!(speeches[1].pitch_mod, 0);
        assert_eq!(speeches[1].rate_mod, -6);
        assert_eq!(speeches[1].voice.as_deref(), Some("pl-PL-ZofiaNeural"));
    }

    #[test]
    fn test_pause_preserves_male_character_acoustics() {
        let director = EmojiDirector::default();
        let script = director.parse("👨 Tak. 🛑 Stój! Kto idzie?");

        let speeches: Vec<&TextSegment> = script
            .items
            .iter()
            .filter_map(|it| match it {
                DirectedItem::Speech(seg) => Some(seg),
                _ => None,
            })
            .collect();

        assert_eq!(speeches.len(), 2);
        // First sentence before pause
        assert_eq!(speeches[0].text, "Tak.");
        assert_eq!(speeches[0].pitch_mod, -20);
        assert_eq!(speeches[0].rate_mod, -10);

        // Second sentence after full pause must still maintain male character acoustics (-20Hz, -10%)
        // and must NOT revert to narrator default (-4Hz, -6%)
        assert_eq!(speeches[1].text, "Stój! Kto idzie?");
        assert_eq!(speeches[1].pitch_mod, -20);
        assert_eq!(speeches[1].rate_mod, -10);
        assert_eq!(speeches[1].voice.as_deref(), Some("pl-PL-MarekNeural"));

        // Hash of second sentence must be different from narrator's hash of the same sentence
        let narrator_seg = TextSegment {
            text: speeches[1].text.clone(),
            ..Default::default()
        };
        let hash_guard = crate::cache::compute_segment_hash(speeches[1], "pl-PL-MarekNeural");
        let hash_narrator = crate::cache::compute_segment_hash(&narrator_seg, "pl-PL-MarekNeural");
        assert_ne!(hash_guard, hash_narrator);
    }

    #[test]
    fn test_base_acoustics_for_speaker_robust_matching() {
        // female_narrator must match female (0, -6), not male
        assert_eq!(
            EmojiDirector::base_acoustics_for_speaker(Some("female_narrator")),
            (0, -6)
        );
        assert_eq!(
            EmojiDirector::base_acoustics_for_speaker(Some("pl-PL-ZofiaNeural")),
            (0, -6)
        );
        assert_eq!(
            EmojiDirector::base_acoustics_for_speaker(Some("pl_PL-gosia-medium")),
            (0, -6)
        );
        assert_eq!(
            EmojiDirector::base_acoustics_for_speaker(Some("ona")),
            (0, -6)
        );

        // male character dialogue voices
        assert_eq!(
            EmojiDirector::base_acoustics_for_speaker(Some("pl-PL-MarekNeural")),
            (-20, -10)
        );
        assert_eq!(
            EmojiDirector::base_acoustics_for_speaker(Some("male_guard")),
            (-20, -10)
        );
        assert_eq!(
            EmojiDirector::base_acoustics_for_speaker(Some("pl_PL-mc_speech-medium")),
            (-20, -10)
        );
        assert_eq!(
            EmojiDirector::base_acoustics_for_speaker(Some("old-man")),
            (-20, -10)
        );

        // darkman is Piper's canonical lektor/narrator voice (acoustics -4Hz, -6%)
        assert_eq!(
            EmojiDirector::base_acoustics_for_speaker(Some("darkman")),
            (-4, -6)
        );
        assert_eq!(
            EmojiDirector::base_acoustics_for_speaker(Some("pl_PL-darkman-medium")),
            (-4, -6)
        );

        // foreign voices with "man" substring that are NOT male
        assert_eq!(
            EmojiDirector::base_acoustics_for_speaker(Some("pt-BR-ManuelaNeural")),
            (-4, -6)
        );
        assert_eq!(
            EmojiDirector::base_acoustics_for_speaker(Some("mandarin_voice")),
            (-4, -6)
        );

        // narrator / default
        assert_eq!(EmojiDirector::base_acoustics_for_speaker(None), (-4, -6));
    }

    #[test]
    fn test_custom_emoji_director_parsing() {
        let mut reg = PresetRegistry::new();
        let toml_str = r#"
        [emojis."🧙‍♂️"]
        name = "Czarodziej"
        alias = "czarodziej"
        action_type = "filter"
        filter = "cathedral"
        pitch = -5

        [emojis."🦇"]
        name = "Wampir"
        alias = "wampir"
        action_type = "voice"
        voice = "pl_PL-darkman-medium"
        "#;
        let cfg: crate::presets::ProjectConfig = toml::from_str(toml_str).unwrap();
        reg.load_project_config(&cfg);

        let director = EmojiDirector::with_registry(reg);
        let script = director.parse("🧙‍♂️ Witaj wędrowcze! 🦇 Nadeszła ciemność.");

        let speeches: Vec<&TextSegment> = script
            .items
            .iter()
            .filter_map(|it| match it {
                DirectedItem::Speech(seg) => Some(seg),
                _ => None,
            })
            .collect();

        assert_eq!(speeches.len(), 2);
        // First speech: wizard custom emoji
        assert_eq!(speeches[0].text, "Witaj wędrowcze!");
        assert_eq!(speeches[0].filter.as_deref(), Some("🧙‍♂️"));
        assert_eq!(speeches[0].pitch_mod, -5);

        // Second speech: vampire custom voice
        assert_eq!(speeches[1].text, "Nadeszła ciemność.");
        assert_eq!(
            speeches[1].voice.as_deref().unwrap().to_lowercase(),
            "pl_pl-darkman-medium"
        );
    }

    #[test]
    fn test_production_styles_filtering() {
        let text = "Narrator mówi. 👩 Dzień dobry! 📱 Halo, słyszysz mnie? 🛑 Koniec rozmowy.";

        // 1. Narrator style (single speaker, no acoustic lo-fi filters)
        let dir_narrator = EmojiDirector::default().with_style(ProductionStyle::Narrator);
        let script_narrator = dir_narrator.parse(text);
        let speeches_narrator: Vec<&TextSegment> = script_narrator
            .items
            .iter()
            .filter_map(|it| match it {
                DirectedItem::Speech(seg) => Some(seg),
                _ => None,
            })
            .collect();

        assert_eq!(speeches_narrator.len(), 4);
        for s in &speeches_narrator {
            assert_eq!(
                s.voice, None,
                "Narrator style must ignore character voice casting"
            );
            assert_eq!(
                s.filter, None,
                "Narrator style must ignore acoustic lo-fi filters"
            );
        }

        // 2. MultiVoice style (multi-voice character casting, but clean studio acoustics without lo-fi filters)
        let dir_multivoice = EmojiDirector::default().with_style(ProductionStyle::MultiVoice);
        let script_multivoice = dir_multivoice.parse(text);
        let speeches_multivoice: Vec<&TextSegment> = script_multivoice
            .items
            .iter()
            .filter_map(|it| match it {
                DirectedItem::Speech(seg) => Some(seg),
                _ => None,
            })
            .collect();

        assert_eq!(speeches_multivoice.len(), 4);
        assert_eq!(speeches_multivoice[0].voice, None);
        assert_eq!(
            speeches_multivoice[1].voice.as_deref(),
            Some("pl-PL-ZofiaNeural")
        );
        assert_eq!(
            speeches_multivoice[2].voice.as_deref(),
            Some("pl-PL-ZofiaNeural")
        );
        // Filter must be stripped in MultiVoice!
        assert_eq!(
            speeches_multivoice[2].filter, None,
            "MultiVoice style must bypass environmental filters"
        );

        // 3. AudioDrama style (full theatrical drama with character voices and acoustic filters)
        let dir_audiodrama = EmojiDirector::default().with_style(ProductionStyle::AudioDrama);
        let script_audiodrama = dir_audiodrama.parse(text);
        let speeches_audiodrama: Vec<&TextSegment> = script_audiodrama
            .items
            .iter()
            .filter_map(|it| match it {
                DirectedItem::Speech(seg) => Some(seg),
                _ => None,
            })
            .collect();

        assert_eq!(speeches_audiodrama.len(), 4);
        assert_eq!(speeches_audiodrama[0].voice, None);
        assert_eq!(
            speeches_audiodrama[1].voice.as_deref(),
            Some("pl-PL-ZofiaNeural")
        );
        assert_eq!(
            speeches_audiodrama[2].voice.as_deref(),
            Some("pl-PL-ZofiaNeural")
        );
        assert_eq!(
            speeches_audiodrama[2].filter.as_deref(),
            Some("📱"),
            "AudioDrama style must apply acoustic filter"
        );
    }

    #[test]
    fn test_emotional_emojis_and_actor_tags() {
        let text =
            "😊 Cudownie! 😢 Niestety... 😡 Wynoś się! 😲 Co to było? 😰 Boję się. 🤢 Obrzydliwe.";
        let director = EmojiDirector::default();
        let script = director.parse(text);

        let speeches: Vec<&TextSegment> = script
            .items
            .iter()
            .filter_map(|it| match it {
                DirectedItem::Speech(seg) => Some(seg),
                _ => None,
            })
            .collect();

        assert_eq!(speeches.len(), 6);
        assert!(speeches[0].actor_tags.contains(&ActorTag::Happy));
        assert_eq!(
            speeches[0].format_for_elevenlabs(),
            "[cheerfully] Cudownie!"
        );

        assert!(speeches[1].actor_tags.contains(&ActorTag::Sad));
        assert_eq!(speeches[1].format_for_elevenlabs(), "[sadly] Niestety...");

        assert!(speeches[2].actor_tags.contains(&ActorTag::Angry));
        assert_eq!(speeches[2].format_for_elevenlabs(), "[angrily] Wynoś się!");

        assert!(speeches[3].actor_tags.contains(&ActorTag::Surprised));
        assert_eq!(
            speeches[3].format_for_elevenlabs(),
            "[surprisedly] Co to było?"
        );

        assert!(speeches[4].actor_tags.contains(&ActorTag::Nervous));
        assert_eq!(speeches[4].format_for_elevenlabs(), "[nervously] Boję się.");

        assert!(speeches[5].actor_tags.contains(&ActorTag::Disgusted));
        assert_eq!(
            speeches[5].format_for_elevenlabs(),
            "[disgustedly] Obrzydliwe."
        );
    }
}
