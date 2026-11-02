pub mod types;
pub mod parser;
pub mod cache;
pub mod audio;
pub mod verbalizer;
pub mod presets;

pub use types::{
    ActorTag, AudiobookMetadata, ChapterMark, DirectedItem, DirectedScript, EmojiAction,
    RenderResult, TextSegment,
};
pub use parser::EmojiDirector;
pub use presets::{EffectPreset, ProjectConfig, ProjectMetaConfig, PresetRegistry};
pub use cache::compute_segment_hash;
pub use audio::{
    create_breath_wav, create_silence_wav, export_to_m4b, export_to_mp3, stitch_wav_files,
    AudioDirectorEngine, EdgeTtsBridge, PiperBridge, SpeechSynthesizer, DEFAULT_SAMPLE_RATE,
};
pub use verbalizer::normalize_polish_text;
