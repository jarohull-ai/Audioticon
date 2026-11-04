use crate::types::{BreathIntensity, DirectedItem, DirectedScript};
use serde::{Deserialize, Serialize};

/// Configuration for intelligent lektor cadence, sentence phrasing, and micro-breaths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CadenceConfig {
    /// Whether automatic cadence phrasing is enabled
    pub enabled: bool,
    /// Breath intensity level (Subtle, Natural, Off)
    pub intensity: BreathIntensity,
    /// Minimum syllables spoken before a clause breath can be considered (default: 32 ~ 12-14 words)
    pub min_syllables_for_clause_breath: usize,
    /// Minimum syllables spoken before a sentence breath can be considered (default: 45 ~ 18-20 words)
    pub min_syllables_for_sentence_breath: usize,
    /// Duration of a subtle clause breath in ms (default: 240 ms)
    pub clause_breath_ms: u32,
    /// Duration of a sentence-transition breath in ms (default: 300 ms)
    pub sentence_breath_ms: u32,
    /// Duration of a minor clause pause without breath in ms (default: 160 ms)
    pub clause_pause_ms: u32,
    /// Duration of sentence pause in ms (default: 350 ms)
    pub sentence_pause_ms: u32,
    /// Gain in dBFS for breath synthesis (Subtle: -33.0 dBFS, Natural: -27.5 dBFS)
    pub breath_gain_db: f32,
}

impl Default for CadenceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            intensity: BreathIntensity::Subtle,
            min_syllables_for_clause_breath: 32,
            min_syllables_for_sentence_breath: 45,
            clause_breath_ms: 240,
            sentence_breath_ms: 300,
            clause_pause_ms: 160,
            sentence_pause_ms: 350,
            breath_gain_db: -33.0,
        }
    }
}

impl CadenceConfig {
    pub fn subtle() -> Self {
        Self {
            intensity: BreathIntensity::Subtle,
            breath_gain_db: -33.0,
            clause_breath_ms: 240,
            sentence_breath_ms: 300,
            ..Default::default()
        }
    }

    pub fn natural() -> Self {
        Self {
            intensity: BreathIntensity::Natural,
            breath_gain_db: -27.5,
            clause_breath_ms: 280,
            sentence_breath_ms: 350,
            ..Default::default()
        }
    }

    pub fn off() -> Self {
        Self {
            enabled: false,
            intensity: BreathIntensity::Off,
            ..Default::default()
        }
    }
}

/// Fast, linguistically accurate Polish syllable counter.
/// In Polish, every syllable nucleus is a vowel: a, ą, e, ę, i, o, ó, u, y.
/// When 'i' precedes another vowel (ia, ie, io, iu, ią, ię), it acts as a palatalizing glide
/// forming a single diphthong/nucleus with the following vowel (e.g. "nie" = 1, "Kijów" = 2).
pub fn count_polish_syllables(text: &str) -> usize {
    let mut total = 0;
    for word in text.split_whitespace() {
        let clean: String = word
            .chars()
            .filter(|c| c.is_alphabetic())
            .flat_map(|c| c.to_lowercase())
            .collect();
        if clean.is_empty() {
            continue;
        }

        let chars: Vec<char> = clean.chars().collect();
        let vowels = ['a', 'ą', 'e', 'ę', 'i', 'o', 'ó', 'u', 'y'];
        let mut count = 0;

        for i in 0..chars.len() {
            if vowels.contains(&chars[i]) {
                // If 'i' is followed immediately by another vowel, skip counting 'i'
                if chars[i] == 'i' && i + 1 < chars.len() && vowels.contains(&chars[i + 1]) {
                    continue;
                }
                count += 1;
            }
        }
        total += count.max(1);
    }
    total
}

/// Checks if a lowercase word is a Polish subordinating, coordinating, or relative clause marker.
fn is_polish_conjunction(word: &str) -> bool {
    let clean = word
        .trim_matches(|c: char| !c.is_alphabetic())
        .to_lowercase();
    matches!(
        clean.as_str(),
        "że" | "żeby"
            | "iż"
            | "bo"
            | "ponieważ"
            | "bowiem"
            | "gdyż"
            | "albowiem"
            | "chociaż"
            | "choć"
            | "jakkolwiek"
            | "ale"
            | "lecz"
            | "jednak"
            | "zaś"
            | "natomiast"
            | "gdy"
            | "kiedy"
            | "dopóki"
            | "zanim"
            | "nim"
            | "aż"
            | "jakby"
            | "skoro"
            | "jeśli"
            | "jeżeli"
            | "gdyby"
            | "który"
            | "która"
            | "które"
            | "którzy"
            | "których"
            | "którym"
            | "któremu"
            | "której"
            | "którą"
            | "którego"
            | "gdzie"
            | "skąd"
            | "dokąd"
            | "kto"
            | "co"
            | "czego"
            | "czemu"
            | "czym"
            | "dlaczego"
            | "a"
    )
}

/// Represents a detected syntactic boundary inside a sentence.
#[derive(Debug, PartialEq, Clone)]
enum BoundaryKind {
    SentenceEnd,
    MajorConjunction,
    EmDash,
    Semicolon,
}

#[derive(Debug, Clone)]
struct SplitPoint {
    char_index: usize,
    kind: BoundaryKind,
}

/// Identifies clause and sentence split points in a text segment.
fn find_split_points(text: &str) -> Vec<SplitPoint> {
    let mut points = Vec::new();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let len = chars.len();

    let mut i = 0;
    while i < len {
        let (byte_pos, c) = chars[i];

        // 1. Sentence end: . ! ? … followed by whitespace or end of string
        if (c == '.' || c == '!' || c == '?' || c == '…') && i + 1 < len {
            // Check for ellipsis ...
            let mut skip = 0;
            if c == '.' && i + 2 < len && chars[i + 1].1 == '.' && chars[i + 2].1 == '.' {
                skip = 2;
            }
            let next_idx = i + 1 + skip;
            if next_idx < len && chars[next_idx].1.is_whitespace() {
                // Ensure not an abbreviation like "np." or "itd." if needed
                let split_byte = chars[next_idx].0;
                points.push(SplitPoint {
                    char_index: split_byte,
                    kind: BoundaryKind::SentenceEnd,
                });
                i = next_idx;
            }
        }
        // 2. Em-dash: " — " or " – "
        else if (c == '—' || c == '–')
            && i > 0
            && chars[i - 1].1.is_whitespace()
            && i + 1 < len
            && chars[i + 1].1.is_whitespace()
        {
            points.push(SplitPoint {
                char_index: byte_pos,
                kind: BoundaryKind::EmDash,
            });
        }
        // 3. Semicolon: "; "
        else if c == ';' && i + 1 < len && chars[i + 1].1.is_whitespace() {
            points.push(SplitPoint {
                char_index: byte_pos + 1,
                kind: BoundaryKind::Semicolon,
            });
        }
        // 4. Comma followed by conjunction: ", że", ", który", ", a"
        else if c == ',' && i + 1 < len && chars[i + 1].1.is_whitespace() {
            // Find next word
            let after_space = &text[chars[i + 1].0 + 1..];
            let first_word = after_space.split_whitespace().next().unwrap_or("");
            if is_polish_conjunction(first_word) {
                points.push(SplitPoint {
                    char_index: byte_pos + 1, // Split immediately after comma
                    kind: BoundaryKind::MajorConjunction,
                });
            }
        }

        i += 1;
    }

    points
}

/// Applies intelligent cadence and phrasing to a DirectedScript.
/// Splits monolithic long sentences into natural, beautifully paced clauses with
/// micro-pauses or subtle organic breaths, mimicking a real studio lektor.
pub fn apply_cadence(script: DirectedScript, config: &CadenceConfig) -> DirectedScript {
    if !config.enabled {
        return script;
    }

    let mut paced = DirectedScript::new();
    let mut accumulated_syllables: usize = 0;

    for item in script.items {
        match item {
            DirectedItem::Silence(ms) => {
                if ms >= 300 {
                    accumulated_syllables = 0;
                }
                paced.items.push(DirectedItem::Silence(ms));
            }
            DirectedItem::Breath(ms) => {
                accumulated_syllables = 0;
                paced.items.push(DirectedItem::Breath(ms));
            }
            DirectedItem::Chapter(ch) => {
                accumulated_syllables = 0;
                paced.items.push(DirectedItem::Chapter(ch));
            }
            DirectedItem::SoundEffect(sfx) => {
                paced.items.push(DirectedItem::SoundEffect(sfx));
            }
            DirectedItem::Ambience(amb) => {
                paced.items.push(DirectedItem::Ambience(amb));
            }
            DirectedItem::Speech(segment) => {
                // Dialogue lines that are short (< 12 words) retain their natural punchiness
                let word_count = segment.text.split_whitespace().count();
                let is_character_dialogue = segment.voice.is_some()
                    && segment.voice.as_deref() != Some("default")
                    && segment.voice.as_deref() != Some("narrator");

                if is_character_dialogue && word_count < 12 {
                    accumulated_syllables += count_polish_syllables(&segment.text);
                    paced.items.push(DirectedItem::Speech(segment));
                    continue;
                }

                let text = segment.text.trim();
                let split_points = find_split_points(text);

                if split_points.is_empty() {
                    accumulated_syllables += count_polish_syllables(text);
                    paced.items.push(DirectedItem::Speech(segment));
                    continue;
                }

                let mut last_idx = 0;
                for pt in split_points {
                    if pt.char_index <= last_idx || pt.char_index > text.len() {
                        continue;
                    }

                    let clause_text = text[last_idx..pt.char_index].trim();
                    if clause_text.is_empty() {
                        last_idx = pt.char_index;
                        continue;
                    }

                    let clause_syllables = count_polish_syllables(clause_text);
                    accumulated_syllables += clause_syllables;

                    // Push the spoken clause segment
                    let mut clause_seg = segment.clone();
                    clause_seg.text = clause_text.to_string();
                    paced.items.push(DirectedItem::Speech(clause_seg));

                    // Evaluate cadence transition
                    match pt.kind {
                        BoundaryKind::SentenceEnd => {
                            if accumulated_syllables >= config.min_syllables_for_sentence_breath
                                && config.intensity != BreathIntensity::Off
                            {
                                paced
                                    .items
                                    .push(DirectedItem::Breath(config.sentence_breath_ms));
                                accumulated_syllables = 0;
                            } else {
                                paced
                                    .items
                                    .push(DirectedItem::Silence(config.sentence_pause_ms));
                            }
                        }
                        BoundaryKind::MajorConjunction
                        | BoundaryKind::EmDash
                        | BoundaryKind::Semicolon => {
                            if accumulated_syllables >= config.min_syllables_for_clause_breath
                                && config.intensity != BreathIntensity::Off
                            {
                                paced
                                    .items
                                    .push(DirectedItem::Breath(config.clause_breath_ms));
                                accumulated_syllables = 0;
                            } else {
                                paced
                                    .items
                                    .push(DirectedItem::Silence(config.clause_pause_ms));
                            }
                        }
                    }

                    last_idx = pt.char_index;
                }

                // Push remainder of the segment if any
                if last_idx < text.len() {
                    let rem = text[last_idx..].trim();
                    if !rem.is_empty() {
                        let rem_syllables = count_polish_syllables(rem);
                        accumulated_syllables += rem_syllables;

                        let mut rem_seg = segment.clone();
                        rem_seg.text = rem.to_string();
                        paced.items.push(DirectedItem::Speech(rem_seg));
                    }
                }
            }
        }
    }

    paced
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TextSegment;

    #[test]
    fn test_count_polish_syllables() {
        assert_eq!(count_polish_syllables("nie"), 1);
        assert_eq!(count_polish_syllables("śnieg"), 1);
        assert_eq!(count_polish_syllables("Kijów"), 2);
        assert_eq!(count_polish_syllables("ulice"), 3);
        assert_eq!(count_polish_syllables("Peczerska"), 3);
        assert_eq!(count_polish_syllables("zignorować"), 4);
        assert_eq!(count_polish_syllables("oddech lektora"), 5); // od-dech (2) + lek-to-ra (3) = 5
    }

    #[test]
    fn test_polish_conjunction_detection() {
        assert!(is_polish_conjunction("że"));
        assert!(is_polish_conjunction("którego"));
        assert!(is_polish_conjunction("ponieważ"));
        assert!(is_polish_conjunction("a"));
        assert!(!is_polish_conjunction("stół"));
        assert!(!is_polish_conjunction("człowiek"));
    }

    #[test]
    fn test_apply_cadence_splits_long_compound_sentence() {
        let long_sentence = "Śnieg w Kijowie padał ciężko, oblepiając puste ulice Peczerska, a wiatr od Dniepru niósł ze sobą zapach spalenizny, którego żaden z mieszkańców nie potrafił już zignorować.";

        let mut script = DirectedScript::new();
        script.items.push(DirectedItem::Speech(TextSegment {
            text: long_sentence.to_string(),
            ..Default::default()
        }));

        let config = CadenceConfig {
            enabled: true,
            min_syllables_for_clause_breath: 15,
            min_syllables_for_sentence_breath: 25,
            clause_breath_ms: 220,
            sentence_breath_ms: 280,
            ..Default::default()
        };

        let paced = apply_cadence(script, &config);

        // Verify that the long sentence was split into clauses with breaths/pauses
        assert!(
            paced.items.len() >= 3,
            "Expected split into multiple clauses"
        );

        let has_breath = paced
            .items
            .iter()
            .any(|item| matches!(item, DirectedItem::Breath(_)));
        assert!(has_breath, "Expected at least one natural breath injected");

        // Verify all text words are preserved
        let reconstructed: String = paced
            .items
            .iter()
            .filter_map(|item| match item {
                DirectedItem::Speech(seg) => Some(seg.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");

        assert!(reconstructed.contains("Śnieg w Kijowie"));
        assert!(reconstructed.contains("a wiatr od Dniepru"));
        assert!(reconstructed.contains("którego żaden z mieszkańców"));
    }

    #[test]
    fn test_apply_cadence_disabled_preserves_original() {
        let text = "Krótkie zdanie, które nie powinno być dzielone bez potrzeby.";
        let mut script = DirectedScript::new();
        script.items.push(DirectedItem::Speech(TextSegment {
            text: text.to_string(),
            ..Default::default()
        }));

        let config = CadenceConfig::off();
        let paced = apply_cadence(script.clone(), &config);
        assert_eq!(paced, script);
    }
}
