use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;

static ACTIVE_REGISTRY: std::sync::OnceLock<RwLock<PresetRegistry>> = std::sync::OnceLock::new();

/// Returns a clone of the globally active preset registry
pub fn get_active_registry() -> PresetRegistry {
    ACTIVE_REGISTRY
        .get_or_init(|| RwLock::new(PresetRegistry::default()))
        .read()
        .unwrap()
        .clone()
}

/// Updates the globally active preset registry (e.g. after loading project config)
pub fn set_active_registry(registry: PresetRegistry) {
    let rw = ACTIVE_REGISTRY.get_or_init(|| RwLock::new(PresetRegistry::default()));
    let mut guard = rw.write().unwrap();
    *guard = registry;
}

/// High-level semantic macro parameters for an acoustic effect preset.
/// Users adjust human-friendly 0.0..1.0 sliders (or project TOML) instead
/// of dealing with cryptic, unsafe raw FFmpeg filter strings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EffectPreset {
    /// Base effect type: "phone", "radio", "megaphone", "cathedral", "dungeon", "thought", "robot", "whisper"
    pub base: String,

    /// Output volume scale (e.g. 1.0 = unity, 1.25 = +25%)
    #[serde(default)]
    pub volume: Option<f32>,

    /// Pitch modifier in Hertz (e.g. -4 for deeper voice, +6 for higher)
    #[serde(default)]
    pub pitch: Option<i32>,

    /// Speech rate modifier in percent (e.g. -10 = slower, +15 = faster)
    #[serde(default)]
    pub rate: Option<i32>,

    // Semantic macro sliders (typically 0.0 to 1.0):
    /// Distortion / saturation / bitcrushing grit (0.0 = clean, 1.0 = heavy grit)
    #[serde(default)]
    pub distortion: Option<f32>,

    /// Frequency bandwidth (0.0 = narrow lo-fi bandpass, 1.0 = wide open)
    #[serde(default)]
    pub bandwidth: Option<f32>,

    /// Reverb room volume / space scale (0.0 = dry/small room, 1.0 = vast cavern)
    #[serde(default)]
    pub room_size: Option<f32>,

    /// Reverb reflection decay duration (0.0 = tight reflections, 1.0 = long tail)
    #[serde(default)]
    pub reverb_tail: Option<f32>,

    /// High frequency absorption / dampening in reverberation (0.0 = bright stone, 1.0 = dark damp wood)
    #[serde(default)]
    pub damping: Option<f32>,

    /// Modulation frequency & depth (for robot ringmod / tremolo)
    #[serde(default)]
    pub modulation: Option<f32>,

    /// High-frequency breath friction boost (for whispers)
    #[serde(default)]
    pub air: Option<f32>,
}

impl Default for EffectPreset {
    fn default() -> Self {
        Self {
            base: "none".to_string(),
            volume: None,
            pitch: None,
            rate: None,
            distortion: None,
            bandwidth: None,
            room_size: None,
            reverb_tail: None,
            damping: None,
            modulation: None,
            air: None,
        }
    }
}

impl EffectPreset {
    pub fn new_base(base: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            ..Default::default()
        }
    }

    /// Converts macro parameters into an optimized, mathematically bounded FFmpeg audio filter string.
    pub fn to_ffmpeg_filter(&self) -> String {
        let base_name = self.base.to_lowercase();
        let vol = self.volume.unwrap_or(1.0).clamp(0.1, 3.0);

        match base_name.as_str() {
            "phone" => {
                // Bandwidth: 0.0 = 400Hz-2800Hz, 1.0 = 200Hz-3800Hz
                let bw = self.bandwidth.unwrap_or(0.5).clamp(0.0, 1.0);
                let hp = (420.0 - bw * 200.0) as u32;
                let lp = (2800.0 + bw * 900.0) as u32;
                let mut filter = format!("highpass=f={},lowpass=f={},volume={:.2}", hp, lp, vol);
                if let Some(dist) = self.distortion {
                    let bits = (14.0 - dist.clamp(0.0, 1.0) * 8.0) as u32; // 14 bits down to 6 bits
                    filter.push_str(&format!(
                        ",acrusher=level_in=1:level_out=0.9:bits={}:mode=lin",
                        bits
                    ));
                }
                filter
            }
            "radio" => {
                // Bandwidth: 0.0 = 500Hz-2200Hz, 1.0 = 300Hz-3400Hz
                let bw = self.bandwidth.unwrap_or(0.4).clamp(0.0, 1.0);
                let hp = (500.0 - bw * 180.0) as u32;
                let lp = (2200.0 + bw * 1000.0) as u32;
                let dist = self.distortion.unwrap_or(0.35).clamp(0.0, 1.0);
                let bits = (12.0 - dist * 6.0) as u32; // 12 bits down to 6 bits
                format!(
                    "highpass=f={},lowpass=f={},volume={:.2},acrusher=level_in=1:level_out=0.9:bits={}:mode=lin",
                    hp, lp, vol, bits
                )
            }
            "megaphone" => {
                let bw = self.bandwidth.unwrap_or(0.5).clamp(0.0, 1.0);
                let hp = (600.0 - bw * 150.0) as u32;
                let lp = (3200.0 + bw * 800.0) as u32;
                let tail = self.reverb_tail.unwrap_or(0.3).clamp(0.0, 1.0);
                let delay = (70.0 + tail * 60.0) as u32;
                let decay = 0.15 + tail * 0.2;
                format!(
                    "highpass=f={},lowpass=f={},equalizer=f=1800:t=q:w=2:g=4,volume={:.2},aecho=0.8:0.35:{}:{:.2}",
                    hp, lp, vol, delay, decay
                )
            }
            "cathedral" => {
                // Room size: pad duration 0.2s - 1.2s, delay 70ms - 260ms
                let size = self.room_size.unwrap_or(0.5).clamp(0.0, 1.0);
                let pad_dur = 0.3 + size * 0.7;
                let tail = self.reverb_tail.unwrap_or(0.4).clamp(0.0, 1.0);
                let d1 = (60.0 + size * 60.0) as u32;
                let d2 = (140.0 + size * 140.0) as u32;
                let dec1 = 0.2 + tail * 0.2;
                let dec2 = 0.1 + tail * 0.15;
                let mut filter = format!(
                    "apad=pad_dur={:.2},aecho=0.8:0.45:{}|{}:{:.2}|{:.2}",
                    pad_dur, d1, d2, dec1, dec2
                );
                if let Some(damp) = self.damping {
                    let lp = (7500.0 - damp.clamp(0.0, 1.0) * 4500.0) as u32;
                    filter.push_str(&format!(",lowpass=f={}", lp));
                }
                if (vol - 1.0).abs() > 0.01 {
                    filter.push_str(&format!(",volume={:.2}", vol));
                }
                filter
            }
            "dungeon" => {
                let size = self.room_size.unwrap_or(0.7).clamp(0.0, 1.0);
                let pad_dur = 0.6 + size * 0.8;
                let tail = self.reverb_tail.unwrap_or(0.6).clamp(0.0, 1.0);
                let d1 = (90.0 + size * 60.0) as u32;
                let d2 = (180.0 + size * 120.0) as u32;
                let dec1 = 0.25 + tail * 0.2;
                let dec2 = 0.15 + tail * 0.15;
                let damp = self.damping.unwrap_or(0.5).clamp(0.0, 1.0);
                let lp = (4500.0 - damp * 2000.0) as u32;
                format!(
                    "apad=pad_dur={:.2},lowpass=f={},aecho=0.8:0.65:{}|{}:{:.2}|{:.2}",
                    pad_dur, lp, d1, d2, dec1, dec2
                )
            }
            "thought" => {
                let damp = self.damping.unwrap_or(0.5).clamp(0.0, 1.0);
                let lp = (4500.0 - damp * 1500.0) as u32;
                let boost = 2.0 + (1.0 - damp) * 2.5;
                format!(
                    "lowpass=f={},equalizer=f=300:t=q:w=1:g={:.1},volume={:.2},aecho=0.8:0.35:28:0.25",
                    lp, boost, vol
                )
            }
            "robot" => {
                // Ring modulation tremolo freq: 30Hz .. 90Hz (default 55Hz Dalek)
                let mod_val = self.modulation.unwrap_or(0.5).clamp(0.0, 1.0);
                let trem_f = (30.0 + mod_val * 60.0) as u32;
                let dist = self.distortion.unwrap_or(0.5).clamp(0.0, 1.0);
                let q_val = 1.5 + dist * 2.0;
                let gain_val = (6.0 + dist * 6.0) as u32;
                format!(
                    "aresample=24000,asetrate=24000*0.86,aresample=24000,atempo=1.16,tremolo=f={}:d=0.88,equalizer=f=950:t=q:w={:.1}:g={},aecho=0.8:0.5:16|32:0.35|0.2",
                    trem_f, q_val, gain_val
                )
            }
            "whisper" => {
                let air_boost = self.air.unwrap_or(0.7).clamp(0.0, 1.0);
                let g = (4.0 + air_boost * 6.0) as u32;
                let cut_lows = (450.0 + (1.0 - air_boost) * 100.0) as u32;
                format!(
                    "highpass=f={},lowpass=f=6000,equalizer=f=3200:t=q:w=1.5:g={},volume={:.2}",
                    cut_lows, g, vol
                )
            }
            _ => {
                if (vol - 1.0).abs() > 0.01 {
                    format!("volume={:.2}", vol)
                } else {
                    String::new()
                }
            }
        }
    }
}

/// Project metadata in `audioticon.toml`
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ProjectMetaConfig {
    pub title: Option<String>,
    pub author: Option<String>,
    pub narrator: Option<String>,
    pub year: Option<String>,
}

/// Full project configuration in `audioticon.toml`
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ProjectConfig {
    pub project: Option<ProjectMetaConfig>,
    #[serde(default)]
    pub presets: HashMap<String, EffectPreset>,
}

impl ProjectConfig {
    /// Loads `audioticon.toml` from a given file path or directory
    pub fn load_from_path(path: &Path) -> Result<Self> {
        let file_path = if path.is_dir() {
            path.join("audioticon.toml")
        } else {
            path.to_path_buf()
        };

        if !file_path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&file_path)
            .with_context(|| format!("Nie można odczytać pliku konfiguracji: {:?}", file_path))?;

        let config: ProjectConfig = toml::from_str(&content)
            .with_context(|| format!("Błąd składni TOML w pliku: {:?}", file_path))?;

        Ok(config)
    }

    /// Discovers `audioticon.toml` in input file directory or current working directory
    pub fn discover(hint_path: Option<&Path>) -> Self {
        if let Some(hint) = hint_path {
            if let Some(parent) = hint.parent() {
                if let Ok(cfg) = Self::load_from_path(&parent.join("audioticon.toml")) {
                    if !cfg.presets.is_empty() || cfg.project.is_some() {
                        return cfg;
                    }
                }
            }
        }

        // Try current working directory
        if let Ok(cfg) = Self::load_from_path(Path::new("audioticon.toml")) {
            return cfg;
        }

        Self::default()
    }
}

/// Registry of base factory presets and custom project/user presets.
#[derive(Debug, Clone)]
pub struct PresetRegistry {
    /// Active presets indexed by tag (e.g. "radio", "📻:wojskowe", "phone:retro")
    presets: HashMap<String, EffectPreset>,
}

impl Default for PresetRegistry {
    fn default() -> Self {
        let mut r = Self {
            presets: HashMap::new(),
        };
        r.register_factory_defaults();
        r
    }
}

impl PresetRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn register_factory_defaults(&mut self) {
        self.presets.insert(
            "phone".to_string(),
            EffectPreset {
                base: "phone".to_string(),
                volume: Some(1.2),
                ..Default::default()
            },
        );
        self.presets.insert(
            "radio".to_string(),
            EffectPreset {
                base: "radio".to_string(),
                volume: Some(1.25),
                distortion: Some(0.35),
                ..Default::default()
            },
        );
        self.presets.insert(
            "megaphone".to_string(),
            EffectPreset {
                base: "megaphone".to_string(),
                volume: Some(1.2),
                ..Default::default()
            },
        );
        self.presets.insert(
            "cathedral".to_string(),
            EffectPreset {
                base: "cathedral".to_string(),
                room_size: Some(0.4),
                reverb_tail: Some(0.35),
                ..Default::default()
            },
        );
        self.presets.insert(
            "dungeon".to_string(),
            EffectPreset {
                base: "dungeon".to_string(),
                room_size: Some(0.7),
                reverb_tail: Some(0.6),
                ..Default::default()
            },
        );
        self.presets.insert(
            "thought".to_string(),
            EffectPreset {
                base: "thought".to_string(),
                volume: Some(1.2),
                ..Default::default()
            },
        );
        self.presets.insert(
            "robot".to_string(),
            EffectPreset {
                base: "robot".to_string(),
                modulation: Some(0.5),
                ..Default::default()
            },
        );
        self.presets.insert(
            "whisper".to_string(),
            EffectPreset {
                base: "whisper".to_string(),
                volume: Some(1.8),
                air: Some(0.7),
                ..Default::default()
            },
        );
    }

    /// Loads custom presets from project config
    pub fn load_project_config(&mut self, config: &ProjectConfig) {
        for (name, preset) in &config.presets {
            self.presets.insert(name.clone(), preset.clone());
        }
    }

    /// Resolves an effect tag (e.g. "radio", "📻:wojskowe", "phone:retro", "🏛️:kaplica").
    /// If a custom variant is not found, gracefully falls back to the base effect with a warning.
    pub fn resolve(&self, tag: &str) -> (EffectPreset, Option<String>) {
        if let Some(exact) = self.presets.get(tag) {
            return (exact.clone(), None);
        }

        // Check if tag is in the form of "emoji:variant" or "base:variant"
        if let Some((base_key, variant)) = tag.split_once(':') {
            // First check if base_key itself matches a preset
            let clean_base = base_key.trim().trim_start_matches('[').trim_end_matches(']');
            let canonical_base = match clean_base {
                "📱" | "phone" | "telefon" => "phone",
                "📻" | "radio" => "radio",
                "📢" | "megaphone" | "megafon" | "shout" => "megaphone",
                "🏛️" | "🏛" | "cathedral" | "katedra" => "cathedral",
                "🏰" | "dungeon" | "lochy" => "dungeon",
                "💭" | "thought" | "mysli" | "myśli" => "thought",
                "🤖" | "robot" => "robot",
                "🤫" | "whisper" | "szept" => "whisper",
                other => other,
            };

            // Check if user defined "canonical_base:variant" (e.g. "radio:wojskowe")
            let canonical_full = format!("{}:{}", canonical_base, variant.trim());
            if let Some(p) = self.presets.get(&canonical_full) {
                return (p.clone(), None);
            }

            // Fallback to canonical base
            if let Some(base_preset) = self.presets.get(canonical_base) {
                let warning = format!(
                    "Wariant '{}' nie został znaleziony w audioticon.toml. Używam wariantu bazowego '{}'.",
                    tag, canonical_base
                );
                return (base_preset.clone(), Some(warning));
            }
        }

        // Try canonical base mapping
        let clean_tag = tag.trim().trim_start_matches('[').trim_end_matches(']');
        let canonical = match clean_tag {
            "📱" | "phone" | "telefon" => "phone",
            "📻" | "radio" => "radio",
            "📢" | "megaphone" | "megafon" | "shout" => "megaphone",
            "🏛️" | "🏛" | "cathedral" | "katedra" => "cathedral",
            "🏰" | "dungeon" | "lochy" => "dungeon",
            "💭" | "thought" | "mysli" | "myśli" => "thought",
            "🤖" | "robot" => "robot",
            "🤫" | "whisper" | "szept" => "whisper",
            other => other,
        };

        if let Some(p) = self.presets.get(canonical) {
            return (p.clone(), None);
        }

        (
            EffectPreset::default(),
            Some(format!("Nieznany efekt: '{}'", tag)),
        )
    }

    /// Returns list of all currently registered presets
    pub fn list_presets(&self) -> Vec<(&String, &EffectPreset)> {
        let mut list: Vec<_> = self.presets.iter().collect();
        list.sort_by_key(|(k, _)| (*k).clone());
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_factory_defaults() {
        let reg = PresetRegistry::new();
        let (p, warn) = reg.resolve("radio");
        assert!(warn.is_none());
        assert_eq!(p.base, "radio");
        let filter = p.to_ffmpeg_filter();
        assert!(filter.contains("acrusher"));
    }

    #[test]
    fn test_variant_resolution_and_fallback() {
        let mut reg = PresetRegistry::new();
        let toml_str = r#"
        [presets."📻:wojskowe"]
        base = "radio"
        distortion = 0.8
        bandwidth = 0.2
        volume = 1.3
        "#;
        let cfg: ProjectConfig = toml::from_str(toml_str).unwrap();
        reg.load_project_config(&cfg);

        // Exact match
        let (custom, warn) = reg.resolve("📻:wojskowe");
        assert!(warn.is_none());
        assert_eq!(custom.base, "radio");
        assert_eq!(custom.volume, Some(1.3));
        let filter = custom.to_ffmpeg_filter();
        assert!(filter.contains("volume=1.30"));

        // Fallback for unknown variant
        let (fallback, warn) = reg.resolve("📻:kosmiczne");
        assert!(warn.is_some());
        assert_eq!(fallback.base, "radio");
    }

    #[test]
    fn test_custom_cathedral_parameters() {
        let preset = EffectPreset {
            base: "cathedral".to_string(),
            room_size: Some(0.2), // small chapel
            reverb_tail: Some(0.2),
            damping: Some(0.8), // warm stone
            ..Default::default()
        };
        let filter = preset.to_ffmpeg_filter();
        assert!(filter.contains("apad=pad_dur=0.44"));
        assert!(filter.contains("lowpass=f=3900"));
    }
}
