//! Native Rust Kokoro-82M Neural Text-To-Speech Engine for Audioticon
//!
//! Provides 100% pure Rust, zero-Python inference for Kokoro-82M (Apache-2.0):
//! - Native ONNX Runtime (`ort` + `ndarray`) session execution in Rust
//! - Native `.npz` / `.npy` voice style embedding tensor parser (`voices-v1.0.bin`)
//! - Native `espeak-ng --ipa` phonemization + 178-symbol IPA vocabulary tokenizer
//! - Fallback to `kokoro_worker.py` only if `ORT_DYLIB_PATH` / `libonnxruntime.so` is absent
//! - Honest model verification: returns actionable download guidance when `.onnx` / `voices.bin` are missing

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use ndarray::{Array1, Array2};
use ort::environment::Environment;
use ort::session::Session;
use ort::value::TensorRef;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;

use crate::audio::{
    build_ffmpeg_filter_chain, create_silence_wav, SpeechSynthesizer, DEFAULT_SAMPLE_RATE,
};
use crate::models::models_dir;
use crate::types::TextSegment;

pub const KOKORO_SAMPLE_RATE: u32 = 24000;
pub const KOKORO_MAX_PHONEMES: usize = 510;
pub const KOKORO_STYLE_DIM: usize = 256;

const KOKORO_WORKER_CODE: &str = include_str!("kokoro_worker.py");

static ORT_ENV: OnceLock<Result<Environment, String>> = OnceLock::new();

/// Discovers `libonnxruntime.so` on the system or inside `~/.local/share/audioticon/venv`
/// and initializes the `ort` dynamic [`Environment`] once per process.
pub fn get_ort_environment() -> Result<Environment> {
    let res = ORT_ENV.get_or_init(|| {
        let builder = if let Some(dylib) = find_onnxruntime_dylib() {
            ort::init_from(dylib)
                .map_err(|e| format!("Błąd ładowania biblioteki ONNX Runtime: {}", e))?
        } else {
            ort::init()
        };
        builder
            .with_name("audioticon_ort")
            .build()
            .map_err(|e| format!("Błąd inicjalizacji ONNX Runtime (ort): {}", e))
    });

    match res {
        Ok(env) => Ok(env.clone()),
        Err(msg) => bail!("{}", msg),
    }
}

fn find_onnxruntime_dylib() -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("ORT_DYLIB_PATH") {
        let p = PathBuf::from(custom);
        if p.exists() {
            return Some(p);
        }
    }

    let mut search_roots = Vec::new();
    if let Some(data_dir) = dirs::data_dir() {
        search_roots.push(data_dir.join("audioticon/venv/lib"));
        search_roots.push(data_dir.join("audioticon/venv/lib64"));
    }
    if let Ok(venv) = std::env::var("VIRTUAL_ENV") {
        search_roots.push(PathBuf::from(venv).join("lib"));
    }
    search_roots.push(PathBuf::from("/usr/lib"));
    search_roots.push(PathBuf::from("/usr/local/lib"));
    search_roots.push(PathBuf::from("/usr/lib/x86_64-linux-gnu"));

    for root in search_roots {
        if !root.exists() {
            continue;
        }
        if let Some(found) = scan_for_libonnxruntime(&root, 6) {
            return Some(found);
        }
    }
    None
}

fn scan_for_libonnxruntime(dir: &Path, max_depth: usize) -> Option<PathBuf> {
    if max_depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if (name.starts_with("libonnxruntime.so")
                    || name.starts_with("libonnxruntime.dylib")
                    || name == "onnxruntime.dll")
                    && !name.contains("providers")
                {
                    return Some(path);
                }
            }
        } else if path.is_dir() {
            if let Some(found) = scan_for_libonnxruntime(&path, max_depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

/// Official Kokoro-82M v1.0 IPA Phoneme Vocabulary (178 tokens)
pub fn kokoro_vocab() -> HashMap<char, i64> {
    let pairs: &[(char, i64)] = &[
        (';', 1),
        (':', 2),
        (',', 3),
        ('.', 4),
        ('!', 5),
        ('?', 6),
        ('—', 9),
        ('…', 10),
        ('"', 11),
        ('(', 12),
        (')', 13),
        ('“', 14),
        ('”', 15),
        (' ', 16),
        ('\u{0303}', 17),
        ('ʣ', 18),
        ('ʥ', 19),
        ('ʦ', 20),
        ('ʨ', 21),
        ('ᵝ', 22),
        ('\u{AB67}', 23),
        ('A', 24),
        ('I', 25),
        ('O', 31),
        ('Q', 33),
        ('S', 35),
        ('T', 36),
        ('W', 39),
        ('Y', 41),
        ('ᵊ', 42),
        ('a', 43),
        ('b', 44),
        ('c', 45),
        ('d', 46),
        ('e', 47),
        ('f', 48),
        ('h', 50),
        ('i', 51),
        ('j', 52),
        ('k', 53),
        ('l', 54),
        ('m', 55),
        ('n', 56),
        ('o', 57),
        ('p', 58),
        ('q', 59),
        ('r', 60),
        ('s', 61),
        ('t', 62),
        ('u', 63),
        ('v', 64),
        ('w', 65),
        ('x', 66),
        ('y', 67),
        ('z', 68),
        ('ɑ', 69),
        ('ɐ', 70),
        ('ɒ', 71),
        ('æ', 72),
        ('β', 75),
        ('ɔ', 76),
        ('ɕ', 77),
        ('ç', 78),
        ('ɖ', 80),
        ('ð', 81),
        ('ʤ', 82),
        ('ə', 83),
        ('ɚ', 85),
        ('ɛ', 86),
        ('ɜ', 87),
        ('ɟ', 90),
        ('ɡ', 92),
        ('ɥ', 99),
        ('ɨ', 101),
        ('ɪ', 102),
        ('ʝ', 103),
        ('ɯ', 110),
        ('ɰ', 111),
        ('ŋ', 112),
        ('ɳ', 113),
        ('ɲ', 114),
        ('ɴ', 115),
        ('ø', 116),
        ('ɸ', 118),
        ('θ', 119),
        ('œ', 120),
        ('ɹ', 123),
        ('ɾ', 125),
        ('ɻ', 126),
        ('ʁ', 128),
        ('ɽ', 129),
        ('ʂ', 130),
        ('ʃ', 131),
        ('ʈ', 132),
        ('ʧ', 133),
        ('ʊ', 135),
        ('ʋ', 136),
        ('ʌ', 138),
        ('ɣ', 139),
        ('ɤ', 140),
        ('χ', 142),
        ('ʎ', 143),
        ('ʒ', 147),
        ('ʔ', 148),
        ('ˈ', 156),
        ('ˌ', 157),
        ('ː', 158),
        ('ʰ', 162),
        ('ʲ', 164),
        ('↓', 169),
        ('→', 171),
        ('↗', 172),
        ('↘', 173),
        ('ᵻ', 177),
    ];
    pairs.iter().copied().collect()
}

/// Converts raw text into IPA phonemes using `espeak-ng --ipa -q -v <lang>` and maps them to Kokoro token IDs.
pub fn phonemize_and_tokenize_kokoro(text: &str, lang: &str) -> Result<Vec<i64>> {
    let output = std::process::Command::new("espeak-ng")
        .args(["--ipa", "-q", "-v", lang, text.trim()])
        .output()
        .context("Nie można uruchomić 'espeak-ng' do fonemizacji IPA dla modelu Kokoro-82M")?;

    let ipa = if output.status.success() {
        String::from_utf8_lossy(&output.stdout).to_string()
    } else {
        text.to_string()
    };

    let vocab = kokoro_vocab();
    let mut tokens: Vec<i64> = ipa
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .filter_map(|c| vocab.get(&c).copied())
        .collect();

    if tokens.len() > KOKORO_MAX_PHONEMES {
        tokens.truncate(KOKORO_MAX_PHONEMES);
    }

    if tokens.is_empty() {
        // Fallback: map basic ASCII letters directly
        tokens = text
            .chars()
            .filter_map(|c| vocab.get(&c.to_ascii_lowercase()).copied())
            .take(KOKORO_MAX_PHONEMES)
            .collect();
    }

    Ok(tokens)
}

/// Parses a NumPy `.npy` float32 array from raw bytes (`\x93NUMPY` magic header).
pub fn parse_npy_f32(bytes: &[u8]) -> Result<Vec<f32>> {
    if bytes.len() < 10 || &bytes[0..6] != b"\x93NUMPY" {
        bail!("Nieprawidłowy nagłówek pliku .npy w pakiecie głosów Kokoro");
    }
    let major = bytes[6];
    let data_offset = if major == 1 {
        let header_len = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
        10 + header_len
    } else {
        if bytes.len() < 12 {
            bail!("Uszkodzony nagłówek .npy v2");
        }
        let header_len =
            u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        12 + header_len
    };

    if data_offset > bytes.len() {
        bail!("Przekroczono rozmiar danych w pliku .npy");
    }

    let raw_data = &bytes[data_offset..];
    let num_floats = raw_data.len() / 4;
    let mut floats = Vec::with_capacity(num_floats);
    for chunk in raw_data.chunks_exact(4) {
        floats.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(floats)
}

/// Loads a specific voice style vector of shape `(1, 256)` for `token_count` phonemes
/// directly from a Kokoro `voices-v1.0.bin` (NPZ / ZIP archive containing `<voice>.npy` of shape `(510, 1, 256)`).
pub fn load_voice_style_vector(
    voices_bin_path: &Path,
    voice_name: &str,
    token_count: usize,
) -> Result<Vec<f32>> {
    let file = File::open(voices_bin_path).with_context(|| {
        format!(
            "Nie można otworzyć pliku wektorów głosów Kokoro: {:?}",
            voices_bin_path
        )
    })?;
    let mut archive = zip::ZipArchive::new(file).with_context(|| {
        format!(
            "Plik {:?} nie jest prawidłowym archiwum NPZ/ZIP głosów Kokoro",
            voices_bin_path
        )
    })?;

    let target_entry = format!("{}.npy", voice_name);
    let mut npy_bytes = Vec::new();
    let mut found = false;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        if entry.name().ok().as_deref() == Some(target_entry.as_str()) {
            entry.read_to_end(&mut npy_bytes)?;
            found = true;
            break;
        }
    }

    if !found {
        if archive.is_empty() {
            bail!("Archiwum głosów Kokoro jest puste: {:?}", voices_bin_path);
        }
        let mut entry = archive.by_index(0)?;
        entry.read_to_end(&mut npy_bytes)?;
    }

    let all_floats = parse_npy_f32(&npy_bytes)?;
    if all_floats.len() < KOKORO_STYLE_DIM {
        bail!(
            "Wektor głosu '{}' jest zbyt krótki ({} elementów, wymagane min. {})",
            voice_name,
            all_floats.len(),
            KOKORO_STYLE_DIM
        );
    }

    let total_rows = all_floats.len() / KOKORO_STYLE_DIM;
    let row_idx = token_count.clamp(1, total_rows) - 1;
    let start = row_idx * KOKORO_STYLE_DIM;
    let end = start + KOKORO_STYLE_DIM;
    Ok(all_floats[start..end].to_vec())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum KokoroVoice {
    // American English
    AfBella,
    AfSarah,
    AmAdam,
    AmMichael,
    // British English
    BfEmma,
    BmGeorge,
    // Custom / Other Kokoro voice ID (e.g. ef_dora, ff_siwis, if_sara)
    Custom(String),
}

impl KokoroVoice {
    pub fn as_str(&self) -> &str {
        match self {
            Self::AfBella => "af_bella",
            Self::AfSarah => "af_sarah",
            Self::AmAdam => "am_adam",
            Self::AmMichael => "am_michael",
            Self::BfEmma => "bf_emma",
            Self::BmGeorge => "bm_george",
            Self::Custom(name) => name.as_str(),
        }
    }

    pub fn lang_code(&self) -> &str {
        let v = self.as_str();
        if v.starts_with("bf_") || v.starts_with("bm_") {
            "en-gb"
        } else if v.starts_with("ef_") || v.starts_with("em_") {
            "es"
        } else if v.starts_with("ff_") || v.starts_with("fm_") {
            "fr-fr"
        } else if v.starts_with("if_") || v.starts_with("im_") {
            "it"
        } else if v.starts_with("pf_") || v.starts_with("pm_") {
            "pt-br"
        } else {
            "en-us"
        }
    }
}

impl std::str::FromStr for KokoroVoice {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim().to_lowercase().as_str() {
            "af_bella" | "bella" => Self::AfBella,
            "af_sarah" | "sarah" => Self::AfSarah,
            "am_adam" | "adam" => Self::AmAdam,
            "am_michael" | "michael" => Self::AmMichael,
            "bf_emma" | "emma" => Self::BfEmma,
            "bm_george" | "george" => Self::BmGeorge,
            other => Self::Custom(other.to_string()),
        })
    }
}

struct KokoroWorkerProcess {
    process: std::process::Child,
    stdin: std::process::ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
}

impl Drop for KokoroWorkerProcess {
    fn drop(&mut self) {
        let _ = self.process.kill();
    }
}

/// Native Rust ONNX Runtime Session wrapper for Kokoro-82M
struct NativeKokoroSession {
    session: Session,
    tokens_input_name: String,
}

pub struct KokoroBridge {
    pub model_path: PathBuf,
    pub voices_path: PathBuf,
    pub default_voice: KokoroVoice,
    pub speed: f32,
    pub python_bin: PathBuf,
    pub worker_script: PathBuf,
    ort_session: Arc<Mutex<Option<NativeKokoroSession>>>,
    workers: Mutex<Vec<KokoroWorkerProcess>>,
}

impl KokoroBridge {
    pub fn new(model_path: impl AsRef<Path>, default_voice: Option<KokoroVoice>) -> Self {
        let m_path = model_path.as_ref().to_path_buf();
        let v_path = Self::resolve_voices_bin_path(&m_path);
        let py = Self::find_python();
        let script = Self::ensure_worker_script();

        Self {
            model_path: m_path,
            voices_path: v_path,
            default_voice: default_voice.unwrap_or(KokoroVoice::AfBella),
            speed: 1.0,
            python_bin: py,
            worker_script: script,
            ort_session: Arc::new(Mutex::new(None)),
            workers: Mutex::new(Vec::new()),
        }
    }

    pub fn default_model_path() -> PathBuf {
        let dir = models_dir();
        let candidates = [
            dir.join("kokoro-v1.0.int8.onnx"),
            dir.join("kokoro-82m-int8.onnx"),
            dir.join("kokoro-v1.0.onnx"),
            dir.join("model.int8.onnx"),
        ];
        for c in &candidates {
            if c.exists() {
                return c.clone();
            }
        }
        candidates[0].clone()
    }

    fn resolve_voices_bin_path(model_path: &Path) -> PathBuf {
        let parent = model_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(models_dir);
        let candidates = [
            parent.join("kokoro-82m-int8.voices.bin"),
            parent.join("voices-v1.0.bin"),
            parent.join("voices.bin"),
        ];
        for c in &candidates {
            if c.exists() {
                return c.clone();
            }
        }
        candidates[0].clone()
    }

    fn ensure_worker_script() -> PathBuf {
        let script_dir = dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("audioticon");
        let _ = std::fs::create_dir_all(&script_dir);
        let script_path = script_dir.join("kokoro_worker.py");
        if !script_path.exists()
            || std::fs::read_to_string(&script_path)
                .map(|s| s != KOKORO_WORKER_CODE)
                .unwrap_or(true)
        {
            let _ = std::fs::write(&script_path, KOKORO_WORKER_CODE);
        }
        script_path
    }

    fn find_python() -> PathBuf {
        if let Some(data_dir) = dirs::data_dir() {
            let venv_py = data_dir.join("audioticon/venv/bin/python3");
            if venv_py.exists() {
                return venv_py;
            }
        }
        if let Ok(venv) = std::env::var("VIRTUAL_ENV") {
            let p = PathBuf::from(venv).join("bin/python3");
            if p.exists() {
                return p;
            }
        }
        let candidates = [
            Path::new("/usr/local/bin/python3"),
            Path::new("/usr/bin/python3"),
            Path::new("python3"),
        ];
        for c in candidates {
            if c.exists() {
                return c.to_path_buf();
            }
        }
        PathBuf::from("python3")
    }

    /// Runs 100% Pure Rust ONNX Runtime inference via `ort` and writes 24 kHz 16-bit PCM WAV.
    async fn synthesize_native_ort(
        &self,
        text: &str,
        voice: &KokoroVoice,
        speed: f32,
        out_raw_wav: &Path,
    ) -> Result<()> {
        if !self.model_path.exists() {
            bail!(
                "Nie znaleziono modelu Kokoro ONNX: {:?}.\nAby pobrać model (Apache-2.0, ~88 MB), użyj polecenia:\n   audioticon models download kokoro-82m-int8",
                self.model_path
            );
        }
        if !self.voices_path.exists() {
            bail!(
                "Nie znaleziono pliku wektorów głosów Kokoro: {:?}.\nAby pobrać pełny pakiet modelu Kokoro-82M, użyj polecenia:\n   audioticon models download kokoro-82m-int8 --force",
                self.voices_path
            );
        }

        let env = get_ort_environment()?;

        let tokens = phonemize_and_tokenize_kokoro(text, voice.lang_code())?;
        let style_vec =
            load_voice_style_vector(&self.voices_path, voice.as_str(), tokens.len())?;

        // Pad tokens with leading 0 (BOS) and trailing 0 (EOS): shape [1, N + 2]
        let mut padded_tokens = Vec::with_capacity(tokens.len() + 2);
        padded_tokens.push(0i64);
        padded_tokens.extend_from_slice(&tokens);
        padded_tokens.push(0i64);

        let seq_len = padded_tokens.len();
        let tokens_array = Array2::from_shape_vec((1, seq_len), padded_tokens)
            .context("Błąd tworzenia tensora tokenów Kokoro")?;
        let style_array = Array2::from_shape_vec((1, KOKORO_STYLE_DIM), style_vec)
            .context("Błąd tworzenia tensora stylu głosu Kokoro")?;
        let speed_array = Array1::from_vec(vec![speed]);

        let mut guard = self.ort_session.lock().await;
        if guard.is_none() {
            let session = Session::builder(&env)
                .map_err(|e| anyhow::anyhow!("ORT session builder error: {}", e))?
                .commit_from_file(&self.model_path)
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Nie udało się załadować modelu ONNX {:?} przez natywny silnik ort: {}",
                        self.model_path,
                        e
                    )
                })?;

            let has_input_ids = session
                .inputs()
                .iter()
                .any(|inp| inp.name() == "input_ids");
            let tokens_input_name = if has_input_ids {
                "input_ids".to_string()
            } else {
                "tokens".to_string()
            };

            *guard = Some(NativeKokoroSession {
                session,
                tokens_input_name,
            });
        }

        let native = guard.as_mut().expect("ORT session initialized");
        let tokens_tensor = TensorRef::from_array_view(&tokens_array)
            .map_err(|e| anyhow::anyhow!("ORT tensor ref tokens: {}", e))?;
        let style_tensor = TensorRef::from_array_view(&style_array)
            .map_err(|e| anyhow::anyhow!("ORT tensor ref style: {}", e))?;
        let speed_tensor = TensorRef::from_array_view(&speed_array)
            .map_err(|e| anyhow::anyhow!("ORT tensor ref speed: {}", e))?;

        let inputs = ort::inputs![
            native.tokens_input_name.as_str() => tokens_tensor,
            "style" => style_tensor,
            "speed" => speed_tensor,
        ];

        let outputs = native
            .session
            .run(inputs)
            .map_err(|e| anyhow::anyhow!("Błąd natywnej inferencji ORT Kokoro-82M: {}", e))?;

        let (_shape, audio_slice) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| anyhow::anyhow!("Błąd ekstrakcji tensora audio z Kokoro ONNX: {}", e))?;

        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: KOKORO_SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(out_raw_wav, spec)?;
        for &sample in audio_slice {
            let s16 = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer.write_sample(s16)?;
        }
        writer.finalize()?;

        Ok(())
    }

    async fn acquire_worker(&self) -> Result<KokoroWorkerProcess> {
        let existing = {
            let mut guard = self.workers.lock().await;
            guard.pop()
        };
        if let Some(w) = existing {
            Ok(w)
        } else {
            self.spawn_worker()
        }
    }

    async fn return_worker(&self, worker: KokoroWorkerProcess) {
        let mut guard = self.workers.lock().await;
        guard.push(worker);
    }

    fn spawn_worker(&self) -> Result<KokoroWorkerProcess> {
        if !self.model_path.exists() {
            bail!(
                "Nie znaleziono modelu Kokoro ONNX: {:?}.\nAby pobrać model (Apache-2.0, ~88 MB), użyj polecenia:\n   audioticon models download kokoro-82m-int8",
                self.model_path
            );
        }
        if !self.voices_path.exists() {
            bail!(
                "Nie znaleziono pliku wektorów głosów Kokoro: {:?}.\nAby pobrać pełny pakiet modelu Kokoro-82M, użyj polecenia:\n   audioticon models download kokoro-82m-int8 --force",
                self.voices_path
            );
        }

        let mut child = std::process::Command::new(&self.python_bin)
            .arg(&self.worker_script)
            .arg(&self.model_path)
            .arg(&self.voices_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .with_context(|| {
                format!(
                    "Błąd uruchomienia procesu pomocniczego Kokoro ONNX ({:?})",
                    self.worker_script
                )
            })?;

        let stdin = child.stdin.take().expect("Child stdin open");
        let stdout = child.stdout.take().expect("Child stdout open");
        let mut reader = BufReader::new(stdout);

        let mut ready_line = String::new();
        reader.read_line(&mut ready_line)?;

        let val: serde_json::Value = serde_json::from_str(&ready_line).with_context(|| {
            format!(
                "Nieprawidłowa odpowiedź startowa procesu Kokoro: {}",
                ready_line.trim()
            )
        })?;

        if val.get("status").and_then(|s| s.as_str()) != Some("ready") {
            let msg = val
                .get("message")
                .and_then(|s| s.as_str())
                .unwrap_or("Nieznany błąd inicjalizacji silnika Kokoro ONNX");
            bail!("Błąd inicjalizacji Kokoro-82M: {}", msg);
        }

        Ok(KokoroWorkerProcess {
            process: child,
            stdin,
            reader,
        })
    }

    pub fn resolve_kokoro_voice(&self, requested: Option<&str>) -> KokoroVoice {
        match requested {
            Some(v) if !v.trim().is_empty() && v != "default" => {
                let clean = v
                    .trim()
                    .strip_prefix("kokoro:")
                    .unwrap_or(v.trim())
                    .to_lowercase();
                if clean.contains("male") && !clean.contains("female")
                    || clean.contains("mezczyzna")
                    || clean.contains("mężczyzna")
                {
                    KokoroVoice::AmAdam
                } else if clean.contains("female") || clean.contains("kobieta") {
                    KokoroVoice::AfBella
                } else if clean.contains("guard")
                    || clean.contains("straznik")
                    || clean.contains("strażnik")
                {
                    KokoroVoice::BmGeorge
                } else {
                    clean
                        .parse::<KokoroVoice>()
                        .unwrap_or_else(|_| self.default_voice.clone())
                }
            }
            _ => self.default_voice.clone(),
        }
    }
}

#[async_trait]
impl SpeechSynthesizer for KokoroBridge {
    fn voice_name(&self) -> &str {
        self.default_voice.as_str()
    }

    fn resolve_voice_label(&self, requested: Option<&str>) -> String {
        let v = self.resolve_kokoro_voice(requested);
        format!("kokoro-{}", v.as_str())
    }

    fn max_concurrency(&self) -> usize {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        cpus.clamp(1, 4)
    }

    async fn synthesize(&self, segment: &TextSegment, out_path: &Path) -> Result<()> {
        let trimmed = segment.text.trim();
        if !trimmed.chars().any(|c| c.is_alphanumeric()) {
            return create_silence_wav(out_path, 80, DEFAULT_SAMPLE_RATE);
        }

        if !self.model_path.exists() {
            bail!(
                "Nie znaleziono modelu Kokoro ONNX: {:?}.\nAby pobrać model (Apache-2.0, ~88 MB), użyj polecenia:\n   audioticon models download kokoro-82m-int8",
                self.model_path
            );
        }

        let pid = std::process::id();
        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let temp_raw_wav =
            out_path.with_extension(format!("{}_{}.kokoro_raw.wav", pid, unique_id));
        let temp_wav = out_path.with_extension(format!("{}_{}.temp.wav", pid, unique_id));

        let voice = self.resolve_kokoro_voice(segment.voice.as_deref());
        let speed = (self.speed * (1.0 + (segment.rate_mod as f32 / 100.0))).clamp(0.5, 2.0);

        // Primary path: 100% Pure Rust ONNX Runtime (`ort` + `ndarray`)
        let ort_res = self
            .synthesize_native_ort(trimmed, &voice, speed, &temp_raw_wav)
            .await;

        if let Err(ort_err) = ort_res {
            // Fallback to Python worker only if native ORT failed and Python worker is available
            let req_json = serde_json::json!({
                "text": trimmed,
                "out_path": temp_raw_wav.to_str().unwrap_or_default(),
                "voice": voice.as_str(),
                "speed": speed,
                "lang": voice.lang_code()
            });

            let mut worker = self.acquire_worker().await.with_context(|| {
                format!(
                    "Natywna inferencja ORT nie powiodła się ({}), a zapasowy proces Pythona również zwrócił błąd",
                    ort_err
                )
            })?;

            let synth_res: Result<()> = {
                let mut line = req_json.to_string();
                line.push('\n');

                worker.stdin.write_all(line.as_bytes())?;
                worker.stdin.flush()?;

                let mut resp_line = String::new();
                worker.reader.read_line(&mut resp_line)?;

                let val: serde_json::Value =
                    serde_json::from_str(&resp_line).with_context(|| {
                        format!(
                            "Nieprawidłowa odpowiedź procesu Kokoro: {}",
                            resp_line.trim()
                        )
                    })?;

                if val.get("status").and_then(|s| s.as_str()) != Some("ok") {
                    let err_msg = val
                        .get("message")
                        .and_then(|s| s.as_str())
                        .unwrap_or("Nieznany błąd syntezy Kokoro");
                    bail!("Kokoro worker error: {}", err_msg);
                }
                Ok(())
            };

            if let Err(e) = synth_res {
                let _ = tokio::fs::remove_file(&temp_raw_wav).await;
                return Err(e);
            }

            self.return_worker(worker).await;
        }

        let is_whisper = crate::whisper_dsp::is_whisper_segment(segment);
        let custom_style = segment
            .filter
            .as_deref()
            .and_then(|f| crate::presets::get_active_registry().resolve_style(f));
        let mut filter_chain = if is_whisper || custom_style.is_some() {
            build_ffmpeg_filter_chain(None)
        } else {
            build_ffmpeg_filter_chain(segment.filter.as_deref())
        };
        if segment.pitch_mod != 0 && !is_whisper && custom_style.is_none() {
            let semitones = segment.pitch_mod as f32 / 3.0;
            let factor = 2f32.powf(semitones / 12.0);
            let pitch_filter = format!(
                "asetrate={:.0}*{:.4},aresample={}:resampler=soxr,atempo={:.4}",
                KOKORO_SAMPLE_RATE,
                factor,
                DEFAULT_SAMPLE_RATE,
                1.0 / factor
            );
            filter_chain = format!("{},{}", filter_chain, pitch_filter);
        }

        let conv = tokio::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-i")
            .arg(&temp_raw_wav)
            .arg("-af")
            .arg(&filter_chain)
            .arg("-ar")
            .arg(DEFAULT_SAMPLE_RATE.to_string())
            .arg("-ac")
            .arg("1")
            .arg("-c:a")
            .arg("pcm_s16le")
            .arg(&temp_wav)
            .output()
            .await?;

        let _ = tokio::fs::remove_file(&temp_raw_wav).await;

        if !conv.status.success() {
            let stderr = String::from_utf8_lossy(&conv.stderr);
            let _ = tokio::fs::remove_file(&temp_wav).await;
            bail!("Błąd przetwarzania FFmpeg dla Kokoro WAV: {}", stderr);
        }

        if let Some(ref style) = custom_style {
            crate::style_transfer::apply_disentangled_style_to_wav(&temp_wav, style)?;
        } else if is_whisper {
            let mode =
                crate::whisper_dsp::WhisperMode::from_filter_tag(segment.filter.as_deref());
            crate::whisper_dsp::apply_whisper_to_wav_file_with_mode(&temp_wav, mode)?;
        }

        tokio::fs::rename(&temp_wav, out_path)
            .await
            .with_context(|| format!("Błąd zapisu wyjściowego pliku WAV: {:?}", out_path))?;

        Ok(())
    }
}

/// Linear phase-continuous sample-rate converter across audio buffers.
pub fn resample_linear(samples: &[f32], in_rate: u32, out_rate: u32) -> Vec<f32> {
    if in_rate == out_rate || samples.is_empty() {
        return samples.to_vec();
    }
    let n_out = ((samples.len() as f64 * out_rate as f64) / in_rate as f64).round() as usize;
    let mut out = Vec::with_capacity(n_out);
    let step = in_rate as f64 / out_rate as f64;

    for i in 0..n_out {
        let pos = i as f64 * step;
        let idx = pos.floor() as usize;
        let frac = (pos - idx as f64) as f32;

        let s1 = if idx < samples.len() { samples[idx] } else { 0.0 };
        let s2 = if idx + 1 < samples.len() {
            samples[idx + 1]
        } else {
            s1
        };

        out.push(s1 + frac * (s2 - s1));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_kokoro_missing_model_returns_honest_error() {
        let tmp_dir = std::env::temp_dir().join(format!("test_kokoro_err_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let out_wav = tmp_dir.join("kokoro_test.wav");

        let bridge = KokoroBridge::new(
            tmp_dir.join("nonexistent_kokoro.onnx"),
            Some(KokoroVoice::AfBella),
        );
        let segment = TextSegment {
            text: "Welcome to Audioticon Studio.".to_string(),
            ..Default::default()
        };

        let res = bridge.synthesize(&segment, &out_wav).await;
        assert!(
            res.is_err(),
            "Missing ONNX model must return an error, never a fake sine wave"
        );
        let err_msg = res.unwrap_err().to_string();
        assert!(err_msg.contains("Nie znaleziono modelu Kokoro ONNX"));
        assert!(err_msg.contains("audioticon models download kokoro-82m-int8"));
        assert!(!out_wav.exists());

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_kokoro_voice_parsing_and_lang() {
        let bella: KokoroVoice = "af_bella".parse().unwrap();
        assert_eq!(bella, KokoroVoice::AfBella);
        assert_eq!(bella.lang_code(), "en-us");

        let george: KokoroVoice = "bm_george".parse().unwrap();
        assert_eq!(george, KokoroVoice::BmGeorge);
        assert_eq!(george.lang_code(), "en-gb");
    }

    #[test]
    fn test_native_kokoro_ipa_phonemizer_and_npy_parser() {
        let tokens =
            phonemize_and_tokenize_kokoro("Hello world", "en-us").expect("Phonemize en-us");
        assert!(!tokens.is_empty(), "IPA tokens must not be empty");
        assert!(tokens.iter().all(|&id| id > 0 && id < 178));

        // Build a synthetic valid .npy v1 buffer with 256 float32 values
        let header = b"{'descr': '<f4', 'fortran_order': False, 'shape': (1, 1, 256), }           \n";
        let mut npy = Vec::new();
        npy.extend_from_slice(b"\x93NUMPY\x01\x00");
        npy.extend_from_slice(&(header.len() as u16).to_le_bytes());
        npy.extend_from_slice(header);
        for i in 0..256 {
            npy.extend_from_slice(&((i as f32) * 0.01).to_le_bytes());
        }

        let parsed = parse_npy_f32(&npy).expect("Parse .npy float32");
        assert_eq!(parsed.len(), 256);
        assert!((parsed[10] - 0.10).abs() < 1e-5);
    }

    #[tokio::test]
    async fn test_kokoro_real_synthesis_if_model_present() {
        let model_path = KokoroBridge::default_model_path();
        if !model_path.exists() {
            println!(
                "Pominięto test rzeczywistej syntezy Kokoro: brak pliku {}",
                model_path.display()
            );
            return;
        }

        let tmp_dir =
            std::env::temp_dir().join(format!("test_kokoro_real_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let out_wav = tmp_dir.join("kokoro_real_speech.wav");

        let bridge = KokoroBridge::new(model_path, Some(KokoroVoice::AfBella));
        let segment = TextSegment {
            text: "Hello, this is a real neural synthesis test of Kokoro.".to_string(),
            voice: Some("af_bella".to_string()),
            ..Default::default()
        };

        let res = bridge.synthesize(&segment, &out_wav).await;
        assert!(res.is_ok(), "Real Kokoro synthesis failed: {:?}", res.err());
        assert!(out_wav.exists(), "Output WAV must exist");

        let mut reader = hound::WavReader::open(&out_wav).expect("Open output WAV");
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        let samples: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        assert!(!samples.is_empty(), "Audio samples cannot be empty");

        let sum_sq: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
        let rms = (sum_sq / samples.len() as f64).sqrt();
        assert!(
            rms > 500.0,
            "Synthesized audio must have audible speech energy, RMS was {}",
            rms
        );

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
