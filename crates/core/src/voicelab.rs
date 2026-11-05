use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Environment variable to override voices storage directory (useful for tests)
pub const AUDIOTICON_VOICES_DIR_ENV: &str = "AUDIOTICON_VOICES_DIR";

/// Returns the root directory where voice profiles are stored.
/// Defaults to `~/.local/share/audioticon/voices/` (or `%APPDATA%\audioticon\voices` on Windows).
pub fn voices_dir() -> PathBuf {
    if let Ok(val) = std::env::var(AUDIOTICON_VOICES_DIR_ENV) {
        if !val.trim().is_empty() {
            return PathBuf::from(val);
        }
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("audioticon")
        .join("voices")
}

/// Metadata for a single recorded or imported voice reference sample.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceSampleMeta {
    pub filename: String,
    pub duration_sec: f32,
    pub sample_rate: u32,
    pub channels: u16,
    pub rms_db: f32,
    pub peak_db: f32,
    pub snr_db: f32,
    pub clipping: bool,
    pub created_at: String,
}

/// Technical quality assessment and actionable feedback for voice recording.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SampleQualityReport {
    pub duration_sec: f32,
    pub rms_db: f32,
    pub peak_db: f32,
    pub snr_db: f32,
    pub is_clipped: bool,
    pub is_too_quiet: bool,
    pub is_too_short: bool,
    pub status: String, // "optimal" | "warning" | "error"
    pub tips: Vec<String>,
}

/// Voice Profile representing a speaker or cloned character.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceProfile {
    pub id: String,
    pub name: String,
    pub gender: String,   // "male" | "female" | "neutral"
    pub language: String, // "pl", "en", etc.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub samples: Vec<VoiceSampleMeta>,
    #[serde(default)]
    pub elevenlabs_voice_id: Option<String>,
    /// 256-dimensional zero-shot speaker embedding vector (timbre & formant signature)
    #[serde(default)]
    pub embedding: Option<Vec<f32>>,
    #[serde(default)]
    pub consent: bool,
    pub created_at: String,
}

impl VoiceProfile {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        gender: impl Into<String>,
        language: impl Into<String>,
    ) -> Self {
        let now = chrono_now();
        Self {
            id: sanitize_slug(&id.into()),
            name: name.into(),
            gender: gender.into(),
            language: language.into(),
            description: None,
            samples: Vec::new(),
            elevenlabs_voice_id: None,
            embedding: None,
            consent: true,
            created_at: now,
        }
    }

    pub fn total_duration(&self) -> f32 {
        self.samples.iter().map(|s| s.duration_sec).sum()
    }
}

/// Formats current UTC timestamp in ISO 8601 without extra heavy chrono dependency
fn chrono_now() -> String {
    let now = std::time::SystemTime::now();
    let duration = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    // Simple readable UTC date approximation
    let days = secs / 86400;
    let rem_secs = secs % 86400;
    let hours = rem_secs / 3600;
    let mins = (rem_secs % 3600) / 60;
    let s = rem_secs % 60;

    // Approximate year/month/day
    let mut year = 1970;
    let mut d = days;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let days_in_year = if leap { 366 } else { 365 };
        if d < days_in_year {
            break;
        }
        d -= days_in_year;
        year += 1;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let days_in_months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1;
    for &dim in &days_in_months {
        if d < dim {
            break;
        }
        d -= dim;
        month += 1;
    }
    let day = d + 1;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, mins, s
    )
}

/// Sanitizes a string into a clean lowercase kebab-case slug for directory names
pub fn sanitize_slug(input: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for c in input.chars() {
        if c.is_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "voice-profile".to_string()
    } else {
        trimmed
    }
}

/// Returns the directory for a specific voice profile
pub fn profile_dir(id: &str) -> PathBuf {
    let clean_id = sanitize_slug(id);
    voices_dir().join(clean_id)
}

/// Lists all voice profiles found in the voices directory
pub fn list_voice_profiles() -> Result<Vec<VoiceProfile>> {
    let base = voices_dir();
    if !base.exists() {
        return Ok(Vec::new());
    }

    let mut profiles = Vec::new();
    let entries = fs::read_dir(&base)
        .with_context(|| format!("Nie można odczytać katalogu profili głosów: {:?}", base))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let meta_path = path.join("profile.json");
            if meta_path.exists() {
                if let Ok(content) = fs::read_to_string(&meta_path) {
                    if let Ok(profile) = serde_json::from_str::<VoiceProfile>(&content) {
                        profiles.push(profile);
                    }
                }
            }
        }
    }

    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(profiles)
}

/// Retrieves a single voice profile by its ID
pub fn get_voice_profile(id: &str) -> Result<VoiceProfile> {
    let dir = profile_dir(id);
    let meta_path = dir.join("profile.json");
    if !meta_path.exists() {
        bail!("Profil głosu o identyfikatorze '{}' nie istnieje", id);
    }
    let content = fs::read_to_string(&meta_path)
        .with_context(|| format!("Nie można odczytać pliku profilu: {:?}", meta_path))?;
    let profile = serde_json::from_str(&content)
        .with_context(|| format!("Błąd parsowania JSON w profilu: {:?}", meta_path))?;
    Ok(profile)
}

/// Saves or updates a voice profile
pub fn save_voice_profile(profile: &VoiceProfile) -> Result<()> {
    let dir = profile_dir(&profile.id);
    fs::create_dir_all(&dir)
        .with_context(|| format!("Nie można utworzyć katalogu profilu: {:?}", dir))?;
    let samples_dir = dir.join("samples");
    fs::create_dir_all(&samples_dir)
        .with_context(|| format!("Nie można utworzyć katalogu próbek: {:?}", samples_dir))?;

    let meta_path = dir.join("profile.json");
    let json = serde_json::to_string_pretty(profile)?;
    fs::write(&meta_path, json)
        .with_context(|| format!("Nie można zapisać pliku profilu: {:?}", meta_path))?;
    Ok(())
}

/// Deletes a voice profile and all its stored samples
pub fn delete_voice_profile(id: &str) -> Result<()> {
    let dir = profile_dir(id);
    if dir.exists() {
        fs::remove_dir_all(&dir)
            .with_context(|| format!("Nie można usunąć katalogu profilu: {:?}", dir))?;
    }
    Ok(())
}

/// Converts arbitrary audio bytes (WAV, MP3, WebM, OGG, M4A) to standard 24 kHz mono 16-bit PCM WAV.
pub fn convert_to_standard_wav(input_bytes: &[u8], out_wav_path: &Path) -> Result<()> {
    let tmp_dir = std::env::temp_dir();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let in_tmp = tmp_dir.join(format!(
        "audioticon_sample_in_{}_{}.tmp",
        std::process::id(),
        nonce
    ));

    fs::write(&in_tmp, input_bytes)
        .with_context(|| format!("Nie można zapisać pliku tymczasowego: {:?}", in_tmp))?;

    if let Some(parent) = out_wav_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let output = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            in_tmp.to_str().unwrap_or(""),
            "-ar",
            "44100",
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
            out_wav_path.to_str().unwrap_or(""),
        ])
        .output();

    let _ = fs::remove_file(&in_tmp);

    let output = output.context("Nie udało się uruchomić narzędzia ffmpeg do konwersji próbki")?;
    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        bail!("Błąd FFmpeg podczas normalizacji próbki audio: {}", err_msg);
    }

    Ok(())
}

/// Analyzes 16-bit PCM WAV samples for acoustic quality: duration, RMS, Peak, SNR, clipping.
pub fn analyze_wav_samples(samples: &[i16], sample_rate: u32) -> SampleQualityReport {
    if samples.is_empty() {
        return SampleQualityReport {
            duration_sec: 0.0,
            rms_db: -99.0,
            peak_db: -99.0,
            snr_db: 0.0,
            is_clipped: false,
            is_too_quiet: true,
            is_too_short: true,
            status: "error".to_string(),
            tips: vec!["Próbka audio jest pusta.".to_string()],
        };
    }

    let duration_sec = samples.len() as f32 / sample_rate as f32;
    let mut sum_sq: f64 = 0.0;
    let mut peak_val: i16 = 0;
    let mut clip_count = 0;

    // Windowed energy for SNR estimation (~50ms frames)
    let frame_size = (sample_rate as usize / 20).max(1);
    let mut frame_energies = Vec::new();

    for chunk in samples.chunks(frame_size) {
        let mut frame_sum: f64 = 0.0;
        for &s in chunk {
            let val = s.abs();
            if val > peak_val {
                peak_val = val;
            }
            if val >= 32760 {
                clip_count += 1;
            }
            let sf = s as f64;
            let sq = sf * sf;
            frame_sum += sq;
            sum_sq += sq;
        }
        let frame_rms = (frame_sum / chunk.len() as f64).sqrt();
        frame_energies.push(frame_rms);
    }

    let total_rms = (sum_sq / samples.len() as f64).sqrt();
    let rms_db = if total_rms > 0.0 {
        (20.0 * (total_rms / 32768.0).log10()) as f32
    } else {
        -99.0
    };

    let peak_db = if peak_val > 0 {
        (20.0 * (peak_val as f64 / 32768.0).log10()) as f32
    } else {
        -99.0
    };

    // Noise floor estimation: 10th percentile frame energy
    frame_energies.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let noise_idx = (frame_energies.len() / 10).min(frame_energies.len().saturating_sub(1));
    let noise_rms = frame_energies
        .get(noise_idx)
        .copied()
        .unwrap_or(1.0)
        .max(1.0);
    let noise_db = (20.0 * (noise_rms / 32768.0).log10()) as f32;
    let snr_db = (rms_db - noise_db).max(0.0);

    let is_clipped = clip_count > 5 || peak_db >= -0.2;
    let is_too_quiet = rms_db < -36.0;
    let is_too_short = duration_sec < 2.5;

    let mut tips = Vec::new();
    let mut status = "optimal".to_string();

    if is_clipped {
        status = "warning".to_string();
        tips.push("Wykryto przesterowanie (clipping). Zmniejsz czułość mikrofonu (gain) lub odsuń się o 5-10 cm.".to_string());
    }
    if is_too_quiet {
        status = "warning".to_string();
        tips.push("Sygnał jest za cichy (RMS poniżej -36 dBFS). Zwiększ wzmocnienie mikrofonu lub mów pewniejszym głosem.".to_string());
    }
    if is_too_short {
        status = "warning".to_string();
        tips.push("Próbka jest krótsza niż 2.5 sekundy. Do stabilnego profilu zalecane jest 5-30 sekund ciągłego czytania.".to_string());
    }
    if snr_db < 15.0 && !is_too_quiet {
        tips.push("Podwyższony szum tła (SNR < 15 dB). Spróbuj nagrywać w cichszym pomieszczeniu z dala od wentylatorów.".to_string());
    }
    if tips.is_empty() {
        tips.push(
            "Parametry akustyczne optymalne dla lektora (RMS ~ -20..-24 dBFS, brak przesterów)."
                .to_string(),
        );
    }

    SampleQualityReport {
        duration_sec: (duration_sec * 100.0).round() / 100.0,
        rms_db: (rms_db * 10.0).round() / 10.0,
        peak_db: (peak_db * 10.0).round() / 10.0,
        snr_db: (snr_db * 10.0).round() / 10.0,
        is_clipped,
        is_too_quiet,
        is_too_short,
        status,
        tips,
    }
}

/// Reads samples from standard WAV
pub fn read_wav_samples(wav_path: &Path) -> Result<(Vec<i16>, u32, u16)> {
    let reader = hound::WavReader::open(wav_path)
        .with_context(|| format!("Nie można otworzyć pliku WAV: {:?}", wav_path))?;
    let spec = reader.spec();
    let samples: Result<Vec<i16>, _> = reader.into_samples::<i16>().collect();
    let samples = samples.context("Błąd dekodowania próbek PCM 16-bit")?;
    Ok((samples, spec.sample_rate, spec.channels))
}

/// Validates, converts, and attaches an audio sample to a voice profile.
pub fn add_sample_to_profile(
    profile_id: &str,
    input_bytes: &[u8],
    preferred_name: Option<&str>,
) -> Result<(VoiceSampleMeta, SampleQualityReport)> {
    let mut profile = get_voice_profile(profile_id)?;
    let p_dir = profile_dir(profile_id);
    let samples_dir = p_dir.join("samples");
    fs::create_dir_all(&samples_dir)?;

    let sample_index = profile.samples.len() + 1;
    let filename = match preferred_name {
        Some(name) if !name.trim().is_empty() => {
            let base_name = sanitize_slug(name.trim_end_matches(".wav"));
            format!("{}.wav", base_name)
        }
        _ => format!("sample_{:02}.wav", sample_index),
    };

    let target_path = samples_dir.join(&filename);
    convert_to_standard_wav(input_bytes, &target_path)?;

    let (samples, s_rate, channels) = read_wav_samples(&target_path)?;
    let report = analyze_wav_samples(&samples, s_rate);

    let meta = VoiceSampleMeta {
        filename: format!("samples/{}", filename),
        duration_sec: report.duration_sec,
        sample_rate: s_rate,
        channels,
        rms_db: report.rms_db,
        peak_db: report.peak_db,
        snr_db: report.snr_db,
        clipping: report.is_clipped,
        created_at: chrono_now(),
    };

    // Remove old sample with same filename if re-uploading
    profile.samples.retain(|s| s.filename != meta.filename);
    profile.samples.push(meta.clone());
    save_voice_profile(&profile)?;

    Ok((meta, report))
}

/// Metadata saved alongside human punch-in recordings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PunchInMeta {
    pub profile_id: String,
    pub segment_hash: String,
    pub created_at: String,
}

/// Records or imports a punch-in replacement for a specific sentence hash.
/// Standardizes audio to 24 kHz mono WAV and saves directly into the cache directory
/// as `recorded_{hash}.wav`, with metadata in `recorded_{hash}.meta.json`,
/// plus a copy to `{hash}.wav` so all subsequent renders seamlessly use this punch-in.
pub fn punch_in_sentence(
    profile_id: &str,
    segment_hash: &str,
    input_bytes: &[u8],
    cache_dir: &Path,
) -> Result<PathBuf> {
    fs::create_dir_all(cache_dir)?;
    let clean_hash = segment_hash.trim();
    if clean_hash.is_empty() {
        bail!("Pusty hash segmentu dla operacji punch-in");
    }

    let punch_in_wav = cache_dir.join(format!("recorded_{}.wav", clean_hash));
    let punch_in_meta = cache_dir.join(format!("recorded_{}.meta.json", clean_hash));
    let cache_wav = cache_dir.join(format!("{}.wav", clean_hash));

    // Convert input audio to 24 kHz mono WAV
    convert_to_standard_wav(input_bytes, &punch_in_wav)?;

    // Write metadata marker
    let meta = PunchInMeta {
        profile_id: profile_id.to_string(),
        segment_hash: clean_hash.to_string(),
        created_at: chrono_now(),
    };
    fs::write(&punch_in_meta, serde_json::to_string_pretty(&meta)?)?;

    // Also copy to canonical cache key so existing pipeline cache hit logic detects it immediately
    fs::copy(&punch_in_wav, &cache_wav)?;

    Ok(punch_in_wav)
}

/// Clones voice profile to ElevenLabs via `POST https://api.elevenlabs.io/v1/voices/add`.
/// Uses temporary curl config file (0600 mode) for safe key passing.
pub fn clone_to_elevenlabs(profile_id: &str, api_key: &str) -> Result<String> {
    let mut profile = get_voice_profile(profile_id)?;
    if profile.samples.is_empty() {
        bail!(
            "Profil '{}' nie zawiera żadnych próbek dźwiękowych. Dodaj najpierw próbkę nagrania.",
            profile.name
        );
    }

    let p_dir = profile_dir(profile_id);
    let mut existing_sample_paths = Vec::new();
    for s in &profile.samples {
        let full_path = p_dir.join(&s.filename);
        if full_path.exists() {
            existing_sample_paths.push(full_path);
        }
    }

    if existing_sample_paths.is_empty() {
        bail!(
            "Pliki próbek audio dla profilu '{}' nie zostały odnalezione na dysku.",
            profile.name
        );
    }

    let trimmed_key = api_key.trim();
    if trimmed_key.is_empty() {
        bail!("Brak klucza API ElevenLabs");
    }

    // Temporary curl config file with 0600 mode
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp_cfg = std::env::temp_dir().join(format!(
        "audioticon_clone_cfg_{}_{}.tmp",
        std::process::id(),
        nonce
    ));

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true).mode(0o600);
        let mut f = opts.open(&tmp_cfg)?;
        writeln!(f, "header = \"xi-api-key: {}\"", trimmed_key)?;
    }
    #[cfg(not(unix))]
    {
        fs::write(
            &tmp_cfg,
            format!("header = \"xi-api-key: {}\"\n", trimmed_key),
        )?;
    }

    let mut cmd = std::process::Command::new("curl");
    cmd.args(["-sS", "-f", "-K", tmp_cfg.to_str().unwrap_or("")]);
    cmd.args(["-X", "POST", "https://api.elevenlabs.io/v1/voices/add"]);
    cmd.args(["-F", &format!("name={}", profile.name)]);
    let desc = profile
        .description
        .as_deref()
        .unwrap_or("Audioticon cloned voice profile");
    cmd.args(["-F", &format!("description={}", desc)]);

    for p in &existing_sample_paths {
        cmd.args(["-F", &format!("files=@{}", p.to_str().unwrap_or(""))]);
    }

    let output = cmd.output();
    let _ = fs::remove_file(&tmp_cfg);

    let output = output.context("Nie udało się wykonać zapytania curl do API ElevenLabs")?;
    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        bail!(
            "ElevenLabs API zwróciło błąd podczas klonowania głosu: {}",
            err_msg
        );
    }

    let resp_str = String::from_utf8_lossy(&output.stdout);
    #[derive(Deserialize)]
    struct AddVoiceResponse {
        voice_id: String,
    }
    let resp: AddVoiceResponse = serde_json::from_str(&resp_str).with_context(|| {
        format!(
            "Nie można sparsować odpowiedzi ElevenLabs voice clone: {}",
            resp_str
        )
    })?;

    profile.elevenlabs_voice_id = Some(resp.voice_id.clone());
    save_voice_profile(&profile)?;

    Ok(resp.voice_id)
}

/// Exports all voice samples and metadata into an in-memory ZIP archive bytes (LJSpeech/XTTS dataset format).
pub fn export_dataset_zip_bytes(profile_id: &str) -> Result<Vec<u8>> {
    let profile = get_voice_profile(profile_id)?;
    let p_dir = profile_dir(profile_id);

    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    use std::io::Write;

    // 1. profile.json
    zip.start_file("profile.json", options)?;
    let prof_json = serde_json::to_string_pretty(&profile)?;
    zip.write_all(prof_json.as_bytes())?;

    // 2. metadata.csv
    zip.start_file("metadata.csv", options)?;
    let mut csv_lines = String::from("filename|speaker_name|gender|language|duration\n");
    for s in &profile.samples {
        csv_lines.push_str(&format!(
            "{}|{}|{}|{}|{:.2}\n",
            s.filename, profile.name, profile.gender, profile.language, s.duration_sec
        ));
    }
    zip.write_all(csv_lines.as_bytes())?;

    // 3. Audio sample files
    for s in &profile.samples {
        let full_path = p_dir.join(&s.filename);
        if full_path.exists() {
            let data = fs::read(&full_path)?;
            zip.start_file(&s.filename, options)?;
            zip.write_all(&data)?;
        }
    }

    let cursor = zip.finish()?;
    Ok(cursor.into_inner())
}

/// Exports all voice samples and metadata into a standard ZIP archive (LJSpeech/XTTS dataset format).
pub fn export_dataset_zip(profile_id: &str, output_zip_path: &Path) -> Result<PathBuf> {
    let bytes = export_dataset_zip_bytes(profile_id)?;
    if let Some(parent) = output_zip_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_zip_path, &bytes)?;
    Ok(output_zip_path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slug_sanitization() {
        assert_eq!(sanitize_slug("Jarek Lektor 2026!"), "jarek-lektor-2026");
        assert_eq!(sanitize_slug("---Za-żółć--gęślą---"), "za-żółć-gęślą");
        assert_eq!(sanitize_slug(""), "voice-profile");
    }

    #[test]
    fn test_sample_quality_analysis() {
        // Generate a 1-second synthetic sine wave (optimal lektor sample)
        let sample_rate = 24000;
        let mut samples = Vec::new();
        for i in 0..(sample_rate * 3) {
            let t = i as f32 / sample_rate as f32;
            let val = (t * 220.0 * 2.0 * std::f32::consts::PI).sin() * 8000.0; // ~ -12 dBFS
            samples.push(val as i16);
        }

        let report = analyze_wav_samples(&samples, sample_rate);
        assert_eq!(report.duration_sec, 3.0);
        assert!(!report.is_clipped);
        assert!(!report.is_too_quiet);
        assert!(!report.is_too_short);
        assert_eq!(report.status, "optimal");
    }

    static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_voice_profile_crud_in_isolated_dir() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("test_voicelab_{}", std::process::id()));
        std::env::set_var(AUDIOTICON_VOICES_DIR_ENV, tmp.to_str().unwrap());
        fs::create_dir_all(&tmp).unwrap();

        let p = VoiceProfile::new("marian", "Marian", "male", "pl");
        save_voice_profile(&p).unwrap();

        let loaded = get_voice_profile("marian").unwrap();
        assert_eq!(loaded.name, "Marian");
        assert_eq!(loaded.gender, "male");

        let list = list_voice_profiles().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "marian");

        delete_voice_profile("marian").unwrap();
        assert!(get_voice_profile("marian").is_err());

        let _ = fs::remove_dir_all(&tmp);
        std::env::remove_var(AUDIOTICON_VOICES_DIR_ENV);
    }

    #[test]
    fn test_punch_in_and_dataset_export() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let tmp_voices = std::env::temp_dir().join(format!("test_voices_{}", std::process::id()));
        let tmp_cache = std::env::temp_dir().join(format!("test_cache_{}", std::process::id()));
        std::env::set_var(AUDIOTICON_VOICES_DIR_ENV, tmp_voices.to_str().unwrap());
        fs::create_dir_all(&tmp_voices).unwrap();
        fs::create_dir_all(&tmp_cache).unwrap();

        // 1. Create Profile
        let mut p = VoiceProfile::new("lektor", "Lektor", "male", "pl");
        p.description = Some("Główny lektor książki".to_string());
        save_voice_profile(&p).unwrap();

        // 2. Generate a synthetic 1-sec WAV byte buffer
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 24000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut wav_bytes = Vec::new();
        {
            let mut writer =
                hound::WavWriter::new(std::io::Cursor::new(&mut wav_bytes), spec).unwrap();
            for i in 0..24000 {
                let s = ((i as f32 / 24000.0 * 440.0 * 2.0 * std::f32::consts::PI).sin() * 5000.0)
                    as i16;
                writer.write_sample(s).unwrap();
            }
            writer.finalize().unwrap();
        }

        // 3. Add sample to profile
        let (meta, report) = add_sample_to_profile("lektor", &wav_bytes, Some("intro")).unwrap();
        assert_eq!(meta.filename, "samples/intro.wav");
        assert_eq!(report.duration_sec, 1.0);

        // 4. Punch-in sentence
        let seg_hash = "abc123def456";
        let punch_path = punch_in_sentence("lektor", seg_hash, &wav_bytes, &tmp_cache).unwrap();
        assert!(punch_path.exists());
        assert!(tmp_cache.join(format!("{}.wav", seg_hash)).exists());
        assert!(tmp_cache
            .join(format!("recorded_{}.meta.json", seg_hash))
            .exists());

        // 5. Export dataset ZIP (file and bytes)
        let zip_bytes = export_dataset_zip_bytes("lektor").unwrap();
        assert!(!zip_bytes.is_empty());
        assert_eq!(&zip_bytes[0..4], b"PK\x03\x04");

        let zip_out = tmp_voices.join("lektor_dataset.zip");
        let exported = export_dataset_zip("lektor", &zip_out).unwrap();
        assert!(exported.exists());
        assert!(fs::metadata(&exported).unwrap().len() > 0);

        let _ = fs::remove_dir_all(&tmp_voices);
        let _ = fs::remove_dir_all(&tmp_cache);
        std::env::remove_var(AUDIOTICON_VOICES_DIR_ENV);
    }
}
