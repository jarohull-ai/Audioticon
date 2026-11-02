pub mod types;
pub mod parser;
pub mod cache;
pub mod audio;

pub use types::{DirectedItem, DirectedScript, EmojiAction, TextSegment};
pub use parser::EmojiDirector;
pub use cache::compute_segment_hash;
pub use audio::{AudioDirectorEngine, EdgeTtsBridge, SpeechSynthesizer, create_silence_wav, stitch_wav_files, DEFAULT_SAMPLE_RATE};
