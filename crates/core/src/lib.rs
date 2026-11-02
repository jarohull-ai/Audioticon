pub mod audio;
pub mod cache;
pub mod parser;
pub mod presets;
pub mod types;
pub mod verbalizer;

pub use audio::{
    create_breath_wav, create_silence_wav, export_to_m4b, export_to_mp3, stitch_wav_files,
    AudioDirectorEngine, EdgeTtsBridge, PiperBridge, SpeechSynthesizer, DEFAULT_SAMPLE_RATE,
};
pub use cache::compute_segment_hash;
pub use parser::EmojiDirector;
pub use presets::{EffectPreset, PresetRegistry, ProjectConfig, ProjectMetaConfig};
pub use types::{
    ActorTag, AudiobookMetadata, ChapterMark, DirectedItem, DirectedScript, EmojiAction,
    RenderResult, TextSegment,
};
pub use verbalizer::normalize_polish_text;
