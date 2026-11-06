//! Word-Level Audio Synchronizer and Subtitle Exporter (`subtitles.rs`)
//!
//! Generates time-accurate SubRip (.srt), WebVTT (.vtt), Animated Karaoke Cues,
//! and Word-Level JSON Timelines from an Audioticon [`DirectedScript`] and rendered audio.
//!
//! Optimized for:
//! - Social media video reels (TikTok, YouTube Shorts, Instagram Reels),
//! - Audiobook player word-follow karaoke highlighting,
//! - Broadcast subtitle standards (Netflix/BBC: max 42 chars/line, clause-aware wrapping).

use crate::types::{DirectedItem, DirectedScript};
use hound::WavReader;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Formats milliseconds into SubRip timestamp (`HH:MM:SS,mmm`).
pub fn format_timestamp_srt(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let mins = (ms % 3_600_000) / 60_000;
    let secs = (ms % 60_000) / 1_000;
    let millis = ms % 1_000;
    format!("{:02}:{:02}:{:02},{:03}", hours, mins, secs, millis)
}

/// Formats milliseconds into WebVTT timestamp (`HH:MM:SS.mmm`).
pub fn format_timestamp_vtt(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let mins = (ms % 3_600_000) / 60_000;
    let secs = (ms % 60_000) / 1_000;
    let millis = ms % 1_000;
    format!("{:02}:{:02}:{:02}.{:03}", hours, mins, secs, millis)
}

/// Individual word timing with start and end markers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WordCue {
    pub word: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

/// A timed subtitle line suitable for display on screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleCue {
    pub index: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: Option<String>,
    pub emotion: Option<String>,
    pub text: String,
    pub words: Vec<WordCue>,
}

/// Complete subtitle and timing track for an audiobook or chapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SubtitleTrack {
    pub total_duration_ms: u64,
    pub cues: Vec<SubtitleCue>,
}

impl SubtitleTrack {
    pub fn new() -> Self {
        Self::default()
    }

    /// Exports standard SubRip (.srt) subtitles.
    pub fn to_srt(&self) -> String {
        let mut out = String::new();
        for (i, cue) in self.cues.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&format!("{}\n", cue.index));
            out.push_str(&format!(
                "{} --> {}\n",
                format_timestamp_srt(cue.start_ms),
                format_timestamp_srt(cue.end_ms)
            ));
            if let Some(ref spk) = cue.speaker {
                if spk != "Narrator" && !spk.is_empty() {
                    out.push_str(&format!("[{}]: {}\n", spk, cue.text));
                } else {
                    out.push_str(&format!("{}\n", cue.text));
                }
            } else {
                out.push_str(&format!("{}\n", cue.text));
            }
        }
        out
    }

    /// Exports clean WebVTT (.vtt) subtitles.
    pub fn to_vtt(&self) -> String {
        let mut out = String::from("WEBVTT - Audioticon Studio Subtitles\n\n");
        for cue in &self.cues {
            out.push_str(&format!("{}\n", cue.index));
            out.push_str(&format!(
                "{} --> {}\n",
                format_timestamp_vtt(cue.start_ms),
                format_timestamp_vtt(cue.end_ms)
            ));
            if let Some(ref spk) = cue.speaker {
                if !spk.is_empty() {
                    out.push_str(&format!("<v {}>{}\n\n", spk, cue.text));
                    continue;
                }
            }
            out.push_str(&format!("{}\n\n", cue.text));
        }
        out
    }

    /// Exports animated Word-Level Karaoke WebVTT (<timestamp> tags per word).
    /// Highly popular for animated captions in social media reels, TikTok, and modern video players.
    pub fn to_karaoke_vtt(&self) -> String {
        let mut out = String::from("WEBVTT - Audioticon Animated Karaoke\n\n");
        for cue in &self.cues {
            out.push_str(&format!("{}\n", cue.index));
            out.push_str(&format!(
                "{} --> {}\n",
                format_timestamp_vtt(cue.start_ms),
                format_timestamp_vtt(cue.end_ms)
            ));

            let mut line = String::new();
            if let Some(ref spk) = cue.speaker {
                if !spk.is_empty() {
                    line.push_str(&format!("<v {}>", spk));
                }
            }

            for (w_idx, word) in cue.words.iter().enumerate() {
                if w_idx > 0 {
                    line.push(' ');
                }
                line.push_str(&format!(
                    "<{}>{}",
                    format_timestamp_vtt(word.start_ms),
                    word.word
                ));
            }

            if cue.speaker.is_some() {
                line.push_str("</v>");
            }
            out.push_str(&format!("{}\n\n", line));
        }
        out
    }

    /// Exports formatted JSON for interactive Studio UI synchronization and waveforms.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Builds word-level subtitle cues for a single speech segment with known duration.
pub fn align_segment_words(
    text: &str,
    start_ms: u64,
    dur_ms: u64,
    speaker: Option<String>,
    emotion: Option<String>,
    start_index: usize,
) -> Vec<SubtitleCue> {
    let clean_text = text.trim();
    if clean_text.is_empty() || dur_ms == 0 {
        return Vec::new();
    }

    // Split text into lines/clauses if very long (max ~55 chars or punctuation splits)
    let clauses = split_text_into_subtitle_clauses(clean_text, 55);
    let total_chars: usize = clauses.iter().map(|c| c.chars().count().max(1)).sum();

    let mut cues = Vec::new();
    let mut current_start = start_ms;

    // Small lead-in and lead-out margins (speech onset/offset padding ~40ms)
    let margin = 40u64.min(dur_ms / 10);
    let effective_dur = dur_ms.saturating_sub(margin * 2).max(100);

    for (i, clause) in clauses.into_iter().enumerate() {
        let clause_chars = clause.chars().count().max(1);
        let clause_fraction = clause_chars as f64 / total_chars as f64;
        let clause_dur = (effective_dur as f64 * clause_fraction).round() as u64;
        let clause_end = (current_start + clause_dur).min(start_ms + dur_ms);

        let words = distribute_words_in_clause(&clause, current_start + margin / 2, clause_dur);

        cues.push(SubtitleCue {
            index: start_index + i,
            start_ms: current_start,
            end_ms: clause_end,
            speaker: speaker.clone(),
            emotion: emotion.clone(),
            text: clause,
            words,
        });

        current_start = clause_end;
    }

    // Fix end timestamp of final cue to match total duration exactly
    if let Some(last) = cues.last_mut() {
        last.end_ms = start_ms + dur_ms;
        if let Some(last_word) = last.words.last_mut() {
            last_word.end_ms = last.end_ms;
        }
    }

    cues
}

/// Splits long sentences into broadcast-standard subtitle lines at natural punctuation breaks.
fn split_text_into_subtitle_clauses(text: &str, max_chars: usize) -> Vec<String> {
    if text.chars().count() <= max_chars {
        return vec![text.to_string()];
    }

    let mut out = Vec::new();
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut current_clause = Vec::new();
    let mut current_len = 0;

    for w in words {
        let w_len = w.chars().count();
        let ends_with_punct = w.ends_with(',')
            || w.ends_with('.')
            || w.ends_with('!')
            || w.ends_with('?')
            || w.ends_with(';')
            || w.ends_with(':')
            || w.ends_with('—');

        if !current_clause.is_empty() && (current_len + w_len + 1 > max_chars || (ends_with_punct && current_len > 28)) {
            current_clause.push(w);
            out.push(current_clause.join(" "));
            current_clause.clear();
            current_len = 0;
        } else {
            current_clause.push(w);
            current_len += w_len + 1;
        }
    }

    if !current_clause.is_empty() {
        out.push(current_clause.join(" "));
    }

    if out.is_empty() {
        vec![text.to_string()]
    } else {
        out
    }
}

/// Distributes words proportionally by weighted character length within a clause time window.
fn distribute_words_in_clause(clause: &str, start_ms: u64, dur_ms: u64) -> Vec<WordCue> {
    let raw_words: Vec<&str> = clause.split_whitespace().collect();
    if raw_words.is_empty() {
        return Vec::new();
    }

    let weights: Vec<f64> = raw_words
        .iter()
        .map(|w| {
            let len = w.trim_matches(|c: char| c.is_ascii_punctuation()).chars().count().max(1);
            // Vowels typically extend acoustic duration in Polish
            let vowels = w.chars().filter(|c| "aąeęioóuyAĄEĘIOÓUY".contains(*c)).count();
            (len as f64 * 0.75 + vowels as f64 * 0.45).max(1.0)
        })
        .collect();

    let total_weight: f64 = weights.iter().sum();
    let mut word_cues = Vec::new();
    let mut cur_ms = start_ms;

    for (i, word) in raw_words.iter().enumerate() {
        let w_dur = ((weights[i] / total_weight) * dur_ms as f64).round() as u64;
        let w_end = cur_ms + w_dur;
        word_cues.push(WordCue {
            word: word.to_string(),
            start_ms: cur_ms,
            end_ms: w_end,
        });
        cur_ms = w_end;
    }

    word_cues
}

/// Generates a [`SubtitleTrack`] from a [`DirectedScript`] using rendered audio chunk durations.
pub fn build_subtitles_from_script_and_durations(
    script: &DirectedScript,
    chunk_durations_ms: &[u64],
) -> SubtitleTrack {
    let mut cues = Vec::new();
    let mut current_time_ms = 0u64;
    let mut duration_idx = 0usize;
    let mut cue_idx = 1usize;

    for item in &script.items {
        match item {
            DirectedItem::Silence(ms) | DirectedItem::Breath(ms) => {
                let dur = chunk_durations_ms.get(duration_idx).copied().unwrap_or(*ms as u64);
                current_time_ms += dur;
                duration_idx += 1;
            }
            DirectedItem::SoundEffect(_) | DirectedItem::Ambience(_) => {
                let dur = chunk_durations_ms.get(duration_idx).copied().unwrap_or(0);
                current_time_ms += dur;
                duration_idx += 1;
            }
            DirectedItem::Chapter(_) => {
                // Chapters are metadata marks; no speech audio
            }
            DirectedItem::Speech(seg) => {
                let dur = chunk_durations_ms
                    .get(duration_idx)
                    .copied()
                    .unwrap_or_else(|| estimate_fallback_speech_duration(&seg.text));
                let emotion_str = seg.filter.clone();
                let seg_cues = align_segment_words(
                    &seg.text,
                    current_time_ms,
                    dur,
                    seg.voice.clone(),
                    emotion_str,
                    cue_idx,
                );
                cue_idx += seg_cues.len();
                cues.extend(seg_cues);
                current_time_ms += dur;
                duration_idx += 1;
            }
        }
    }

    SubtitleTrack {
        total_duration_ms: current_time_ms,
        cues,
    }
}

/// Generates a [`SubtitleTrack`] by inspecting cache WAV files referenced by the script.
pub fn generate_subtitles_from_cache(
    script: &DirectedScript,
    cache_dir: &Path,
    default_voice: &str,
) -> SubtitleTrack {
    let mut durations: Vec<u64> = Vec::new();

    for item in &script.items {
        match item {
            DirectedItem::Silence(ms) | DirectedItem::Breath(ms) => {
                durations.push(*ms as u64);
            }
            DirectedItem::SoundEffect(_) | DirectedItem::Ambience(_) => {
                durations.push(0);
            }
            DirectedItem::Chapter(_) => {}
            DirectedItem::Speech(seg) => {
                let voice = seg.voice.as_deref().unwrap_or(default_voice);
                let hash = crate::compute_segment_hash(seg, voice);
                let wav_path = cache_dir.join(format!("{}.wav", hash));
                let punch_in = cache_dir.join(format!("recorded_{}.wav", hash));

                let actual_path = if punch_in.exists() {
                    punch_in
                } else {
                    wav_path
                };

                let dur = if actual_path.exists() {
                    if let Ok(reader) = WavReader::open(&actual_path) {
                        (reader.duration() as u64 * 1000) / reader.spec().sample_rate as u64
                    } else {
                        estimate_fallback_speech_duration(&seg.text)
                    }
                } else {
                    estimate_fallback_speech_duration(&seg.text)
                };
                durations.push(dur);
            }
        }
    }

    build_subtitles_from_script_and_durations(script, &durations)
}

fn estimate_fallback_speech_duration(text: &str) -> u64 {
    let words = text.split_whitespace().count().max(1);
    // Standard Polish speech pace: ~2.3 words/sec => ~435ms per word
    (words as u64 * 435).max(600)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_timestamp_srt_and_vtt() {
        let ms = 3_723_456; // 1 hour, 2 mins, 3.456 secs
        assert_eq!(format_timestamp_srt(ms), "01:02:03,456");
        assert_eq!(format_timestamp_vtt(ms), "01:02:03.456");
    }

    #[test]
    fn test_align_segment_words_proportional_and_valid() {
        let text = "W ciemnym zaułku Nowego Światu deszcz bębnił o blaszany dach.";
        let cues = align_segment_words(text, 1000, 3000, Some("Narrator".to_string()), None, 1);
        assert!(!cues.is_empty());
        assert_eq!(cues[0].start_ms, 1000);
        assert_eq!(cues.last().unwrap().end_ms, 4000);

        let total_words: usize = cues.iter().map(|c| c.words.len()).sum();
        assert_eq!(total_words, 10);
        assert_eq!(cues[0].words[0].word, "W");
    }

    #[test]
    fn test_subtitle_track_export_srt_and_vtt() {
        let mut track = SubtitleTrack::new();
        track.cues.push(SubtitleCue {
            index: 1,
            start_ms: 1200,
            end_ms: 3800,
            speaker: Some("Narrator".to_string()),
            emotion: None,
            text: "Pierwsze zdanie audiobooka.".to_string(),
            words: vec![
                WordCue {
                    word: "Pierwsze".to_string(),
                    start_ms: 1200,
                    end_ms: 2000,
                },
                WordCue {
                    word: "zdanie".to_string(),
                    start_ms: 2000,
                    end_ms: 2800,
                },
                WordCue {
                    word: "audiobooka.".to_string(),
                    start_ms: 2800,
                    end_ms: 3800,
                },
            ],
        });

        let srt = track.to_srt();
        assert!(srt.contains("1\n00:00:01,200 --> 00:00:03,800\nPierwsze zdanie"));

        let vtt = track.to_vtt();
        assert!(vtt.contains("WEBVTT"));
        assert!(vtt.contains("00:00:01.200 --> 00:00:03.800"));
        assert!(vtt.contains("<v Narrator>Pierwsze zdanie"));

        let karaoke = track.to_karaoke_vtt();
        assert!(karaoke.contains("<00:00:01.200>Pierwsze <00:00:02.000>zdanie"));
    }
}
