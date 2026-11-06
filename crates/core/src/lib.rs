pub mod audio;
pub mod auto_director;
pub mod batch_pipeline;
pub mod cache;
pub mod cadence;
pub mod credentials;
pub mod estimator;
pub mod importer;
pub mod kokoro;
pub mod ledger;
pub mod model_hub;
pub mod models;
pub mod parser;
pub mod pcm_stream;
pub mod presets;
pub mod room_tone;
pub mod sfx;
pub mod studio_leveler;
pub mod style_transfer;
pub mod types;
pub mod verbalizer;
pub mod voice_design;
pub mod voicelab;
pub mod voucher;
pub mod wallet;
pub mod whisper_dsp;
pub mod wsola;

pub use audio::{
    create_ambient_track, create_breath_wav, create_breath_wav_with_gain, create_sfx_wav,
    create_silence_wav, export_to_flac, export_to_m4b, export_to_mp3, mix_dialogue_with_ambience,
    stitch_wav_files, AudioDirectorEngine, EdgeTtsBridge, ElevenLabsBridge, PiperBridge,
    SpeechSynthesizer, DEFAULT_SAMPLE_RATE,
};
pub use auto_director::{
    auto_direct_text, strip_markdown_code_blocks, AUDIOTICON_DIRECTOR_SYSTEM_PROMPT,
};
pub use batch_pipeline::{
    audit_acx_compliance, AcxComplianceReport, BatchStateJournal, SegmentJournalEntry,
    SegmentState, ACX_MAX_INTEGRATED_LUFS, ACX_MAX_NOISE_FLOOR_DBFS, ACX_MAX_TRUE_PEAK_DBFS,
    ACX_MIN_INTEGRATED_LUFS,
};
pub use cache::{
    compute_segment_hash, get_cache_stats, parse_size_string, prune_cache_lru, CacheStats,
    CACHE_VERSION, DEFAULT_MAX_CACHE_BYTES,
};
pub use cadence::{apply_cadence, count_polish_syllables, CadenceConfig};
pub use credentials::{
    credentials_path, get_api_key, get_masked_key, load_credentials, mask_api_key, remove_api_key,
    save_credentials, set_api_key, verify_elevenlabs_key, CredentialsConfig, ElevenLabsAccountInfo,
    ProviderCredentials,
};
pub use estimator::{estimate_cost, CostEstimate};
pub use importer::{
    base64_decode, decode_html_entities, strip_html_tags, ImportedBook, ImportedChapter,
};
pub use kokoro::{KokoroBridge, KokoroVoice, KOKORO_SAMPLE_RATE};
pub use ledger::{measure_audio_loudness, AudioCertificate};
pub use model_hub::{
    authorize_model_access, check_hub_authorization, get_curated_hub_catalog, LicenseReceipt,
    LicenseType, ModelCategory, ModelHubEntry, ReceiptStore,
};
pub use models::{
    download_model, find_model, list_models, models_dir, remove_model, update_voices_catalog,
    ModelCatalogItem,
};
pub use parser::EmojiDirector;
pub use pcm_stream::{PcmStreamFile, DEFAULT_STREAM_BUFFER_SAMPLES};
pub use presets::{CustomEmojiDef, EffectPreset, PresetRegistry, ProjectConfig, ProjectMetaConfig};
pub use room_tone::{
    fill_gaps_with_room_tone, trim_speech_padding, AudioTrackItem, RoomToneProfile,
    ACX_TARGET_ROOM_TONE_DB, DEFAULT_TONE_FADE_MS, PLOSIVE_PROTECTION_MARGIN_MS,
};
pub use sfx::{
    ambient_dir, find_ambient_asset, find_sfx_asset, import_custom_sfx, infer_category,
    list_curated_essentials, list_installed_sfx, sfx_dir, CuratedSfxItem, SfxAssetInfo,
};
pub use studio_leveler::{level_speech, peak_limit, LevelerConfig};
pub use style_transfer::{
    apply_disentangled_style, apply_disentangled_style_to_wav, diagnose_styled_audio,
    estimate_median_f0, extract_disentangled_style_matrix, DisentangledStyleMatrix, StyleCategory,
    StyleDiagnosticReport,
};
pub use types::{
    ActorTag, AudiobookMetadata, AudioticonProject, BreathIntensity, ChapterMark, DirectedItem,
    DirectedScript, EmojiAction, ProductionStyle, ProjectChapter, ProjectMasteringConfig,
    ProjectSettings, RenderResult, TextSegment,
};
pub use verbalizer::normalize_polish_text;
pub use voice_design::{
    create_voice_design_previews, save_designed_voice, DesignedVoicePreview, VoiceDesignResponse,
};
pub use voicelab::{
    add_sample_to_profile, analyze_wav_samples, clone_to_elevenlabs, convert_to_standard_wav,
    delete_voice_profile, export_dataset_zip, export_dataset_zip_bytes, get_voice_profile,
    list_voice_profiles, profile_dir, punch_in_sentence, read_wav_samples, sanitize_slug,
    save_voice_profile, voices_dir, SampleQualityReport, VoiceProfile, VoiceSampleMeta,
};
pub use voucher::{generate_voucher_code, verify_voucher_code, VoucherPayload, WalletTier};
pub use wallet::{AudioWallet, Transaction, STARTING_CREDITS};
pub use whisper_dsp::{
    apply_whisper_to_wav_file, apply_whisper_to_wav_file_with_mode, extract_voice_transfer_matrix,
    is_whisper_segment, levinson_durbin, to_whisper, to_whisper_with_config, VoiceTransferMatrix,
    WhisperConfig, WhisperMode, TRANSFER_BAND_CENTERS_HZ,
};
pub use wsola::wsola_time_stretch;
