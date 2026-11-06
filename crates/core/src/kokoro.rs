//! Kokoro-82M Neural Text-To-Speech Engine Bridge for Audioticon
//!
//! Provides production offline support for Kokoro-82M (Apache-2.0):
//! - Persistent Python ONNX Runtime worker (`kokoro_worker.py` via `kokoro-onnx`)
//! - 24,000 Hz neural synthesis resampled and conditioned through FFmpeg DSP to 44,100 Hz
//! - Honest model verification: fails with actionable download guidance when `.onnx` / `voices.bin` are missing

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

use crate::audio::{build_ffmpeg_filter_chain, create_silence_wav, DEFAULT_SAMPLE_RATE, SpeechSynthesizer};
use crate::models::models_dir;
use crate::types::TextSegment;

pub const KOKORO_SAMPLE_RATE: u32 = 24000;

const KOKORO_WORKER_CODE: &str = include_str!("kokoro_worker.py");

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

pub struct KokoroBridge {
    pub model_path: PathBuf,
    pub voices_path: PathBuf,
    pub default_voice: KokoroVoice,
    pub speed: f32,
    pub python_bin: PathBuf,
    pub worker_script: PathBuf,
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
                if clean.contains("male") && !clean.contains("female") || clean.contains("mezczyzna") || clean.contains("mężczyzna") {
                    KokoroVoice::AmAdam
                } else if clean.contains("female") || clean.contains("kobieta") {
                    KokoroVoice::AfBella
                } else if clean.contains("guard") || clean.contains("straznik") || clean.contains("strażnik") {
                    KokoroVoice::BmGeorge
                } else {
                    clean.parse::<KokoroVoice>().unwrap_or_else(|_| self.default_voice.clone())
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
        let temp_raw_wav = out_path.with_extension(format!("{}_{}.kokoro_raw.wav", pid, unique_id));
        let temp_wav = out_path.with_extension(format!("{}_{}.temp.wav", pid, unique_id));

        let voice = self.resolve_kokoro_voice(segment.voice.as_deref());
        let speed = (self.speed * (1.0 + (segment.rate_mod as f32 / 100.0))).clamp(0.5, 2.0);

        let req_json = serde_json::json!({
            "text": trimmed,
            "out_path": temp_raw_wav.to_str().unwrap_or_default(),
            "voice": voice.as_str(),
            "speed": speed,
            "lang": voice.lang_code()
        });

        let mut worker = self.acquire_worker().await?;

        let synth_res: Result<()> = {
            let mut line = req_json.to_string();
            line.push('\n');

            worker.stdin.write_all(line.as_bytes())?;
            worker.stdin.flush()?;

            let mut resp_line = String::new();
            worker.reader.read_line(&mut resp_line)?;

            let val: serde_json::Value = serde_json::from_str(&resp_line).with_context(|| {
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

        let mut filter_chain = build_ffmpeg_filter_chain(segment.filter.as_deref());
        if segment.pitch_mod != 0 {
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
        assert!(res.is_err(), "Missing ONNX model must return an error, never a fake sine wave");
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
}
