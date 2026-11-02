pub mod types;
pub mod parser;
pub mod cache;
pub mod audio;
pub mod verbalizer;

pub use types::{DirectedItem, DirectedScript, EmojiAction, TextSegment};
pub use parser::EmojiDirector;
pub use cache::compute_segment_hash;
pub use audio::{AudioDirectorEngine, EdgeTtsBridge, PiperBridge, SpeechSynthesizer, create_silence_wav, stitch_wav_files, DEFAULT_SAMPLE_RATE};
pub use verbalizer::normalize_polish_text;
