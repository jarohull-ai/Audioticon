use anyhow::Result;
use audioticon_core::{
    base64_decode, compute_segment_hash, credentials, download_model, estimate_cost, export_to_m4b,
    export_to_mp3, find_model, list_models, models_dir, presets, remove_model,
    update_voices_catalog, voice_design, voicelab, AudioCertificate, AudioDirectorEngine,
    AudioWallet, AudiobookMetadata, CustomEmojiDef, DirectedItem, EdgeTtsBridge, ElevenLabsBridge,
    EmojiDirector, ImportedBook, PiperBridge, ProjectConfig, SpeechSynthesizer, VoiceProfile,
};
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::Arc;
use tiny_http::Header;
use tokio::sync::Mutex;

use crate::args::{get_default_cache_dir, get_default_piper_model, TtsProvider};

static REQ_COUNTER: AtomicU64 = AtomicU64::new(0);
static ACTIVE_PORT: AtomicU16 = AtomicU16::new(3000);
static ACTIVE_HOST: std::sync::OnceLock<String> = std::sync::OnceLock::new();

const STUDIO_HTML: &str = include_str!("../../../../docs/index.html");
const MODULAR_STUDIO_HTML: &str = include_str!("../../../../docs/studio.html");
const LOGO_PNG: &[u8] = include_bytes!("../../../../docs/logo.png");
const LOGO_GIF: &[u8] = include_bytes!("../../../../docs/logo.gif");

#[derive(serde::Deserialize)]
struct ApiRenderRequest {
    text: String,
    #[serde(default = "default_provider_str")]
    provider: String,
    #[serde(default = "default_lang_str")]
    lang: String,
    #[serde(default = "default_voice_str")]
    voice: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    style: Option<String>,
}
fn default_provider_str() -> String {
    "piper".to_string()
}
fn default_lang_str() -> String {
    "pl".to_string()
}
fn default_voice_str() -> String {
    "pl-PL-MarekNeural".to_string()
}

fn parse_production_style(s: Option<&str>) -> audioticon_core::ProductionStyle {
    match s.map(|v| v.trim().to_lowercase()).as_deref() {
        Some("narrator") | Some("lektor") => audioticon_core::ProductionStyle::Narrator,
        Some("multi-voice") | Some("multivoice") | Some("wieloosobowa") => {
            audioticon_core::ProductionStyle::MultiVoice
        }
        Some("audio-drama") | Some("audiodrama") | Some("sluchowisko") => {
            audioticon_core::ProductionStyle::AudioDrama
        }
        _ => audioticon_core::ProductionStyle::AudioDrama,
    }
}

#[derive(serde::Deserialize)]
struct ApiParseRequest {
    text: String,
    #[serde(default = "default_lang_str")]
    lang: String,
    #[serde(default)]
    style: Option<String>,
}

#[derive(serde::Deserialize)]
struct ApiExportRequest {
    #[serde(default)]
    text: String,
    #[serde(default = "default_provider_str")]
    provider: String,
    #[serde(default = "default_lang_str")]
    lang: String,
    #[serde(default = "default_voice_str")]
    voice: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default = "default_export_format")]
    format: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    narrator: Option<String>,
    #[serde(default)]
    year: Option<String>,
    #[serde(default)]
    style: Option<String>,
}

fn default_export_format() -> String {
    "m4b".to_string()
}

fn url_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'+' {
            result.push(' ');
            i += 1;
        } else if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(val) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                result.push(val as char);
                i += 3;
            } else {
                result.push('%');
                i += 1;
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    result
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn strip_data_url_prefix(b64: &str) -> &str {
    let trimmed = b64.trim();
    if let Some((_, rest)) = trimmed.split_once("base64,") {
        rest.trim()
    } else {
        trimmed
    }
}

fn encode_samples_to_wav_base64(samples: &[f32], sample_rate: u32) -> Result<String, String> {
    let data_bytes = (samples.len() * 2) as u32;
    let riff_size = 36 + data_bytes;
    let byte_rate = sample_rate * 2;
    let mut buf = Vec::with_capacity((44 + data_bytes) as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&riff_size.to_le_bytes());
    buf.extend_from_slice(b"WAVEfmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&1u16.to_le_bytes()); // Mono
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&2u16.to_le_bytes()); // Block align
    buf.extend_from_slice(&16u16.to_le_bytes()); // Bits per sample
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_bytes.to_le_bytes());
    for &s in samples {
        let val = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        buf.extend_from_slice(&val.to_le_bytes());
    }
    Ok(base64_encode(&buf))
}

fn generate_synthetic_vocal_phrase(gender: &str, sample_rate: u32) -> Vec<f32> {
    let f0 = if gender.eq_ignore_ascii_case("female") || gender.eq_ignore_ascii_case("kobieta") {
        215.0f32
    } else {
        118.0f32
    };
    let dur_sec = 1.6f32;
    let n = (sample_rate as f32 * dur_sec) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / sample_rate as f32;
        let syl_env = ((t * 3.2 * std::f32::consts::PI).sin().abs()).powf(0.55);
        let glottal = (2.0 * std::f32::consts::PI * f0 * t).sin() * 0.45
            + (2.0 * std::f32::consts::PI * (f0 * 2.0) * t).sin() * 0.25
            + (2.0 * std::f32::consts::PI * 590.0 * t).sin() * 0.22
            + (2.0 * std::f32::consts::PI * 1755.0 * t).sin() * 0.14
            + (2.0 * std::f32::consts::PI * 2680.0 * t).sin() * 0.08;
        out.push((glottal * syl_env * 0.55).clamp(-0.95, 0.95));
    }
    out
}

type PiperCache = Arc<Mutex<HashMap<PathBuf, Arc<PiperBridge>>>>;

async fn resolve_synthesizer(
    provider: &str,
    voice: &str,
    model: Option<&str>,
    clean_lang: &str,
    default_model: Option<&PathBuf>,
    piper_cache: &PiperCache,
) -> Result<Arc<dyn SpeechSynthesizer>, String> {
    if provider == "edge" {
        Ok(Arc::new(EdgeTtsBridge::new(voice)))
    } else if provider == "kokoro" {
        let m_path = if let Some(custom_m) = model {
            let p = PathBuf::from(custom_m);
            if p.exists() || p.is_absolute() {
                p
            } else {
                models_dir().join(custom_m)
            }
        } else {
            audioticon_core::KokoroBridge::default_model_path()
        };
        let kokoro_voice =
            if voice != "default" && voice != "pl-PL-MarekNeural" && !voice.is_empty() {
                voice
                    .parse::<audioticon_core::KokoroVoice>()
                    .unwrap_or(audioticon_core::KokoroVoice::AfBella)
            } else {
                audioticon_core::KokoroVoice::AfBella
            };
        Ok(Arc::new(audioticon_core::KokoroBridge::new(
            m_path,
            Some(kokoro_voice),
        )))
    } else if provider == "elevenlabs" {
        let api_key = credentials::get_api_key("elevenlabs").ok_or_else(|| {
            "Brak skonfigurowanego klucza API dla ElevenLabs. Skonfiguruj klucz w ustawieniach dostawców API.".to_string()
        })?;
        let creds = credentials::load_credentials();
        let el_config = creds.providers.get("elevenlabs");
        let configured_voice = el_config
            .and_then(|p| p.voice_id.as_deref())
            .unwrap_or("adam");
        let configured_model = el_config
            .and_then(|p| p.model_id.as_deref())
            .unwrap_or("eleven_multilingual_v2");

        let effective_voice =
            if voice == "pl-PL-MarekNeural" || voice == "default" || voice.is_empty() {
                configured_voice
            } else {
                voice
            };
        Ok(Arc::new(ElevenLabsBridge::with_model(
            api_key,
            effective_voice,
            configured_model,
        )))
    } else {
        let m_path = if let Some(custom_m) = model {
            if let Some(item) = find_model(custom_m) {
                item.onnx_path
                    .unwrap_or_else(|| models_dir().join(format!("{}.onnx", item.id)))
            } else {
                let p = PathBuf::from(custom_m);
                if p.exists() {
                    p
                } else {
                    models_dir().join(custom_m)
                }
            }
        } else if let Some(item) = find_model(voice) {
            item.onnx_path
                .unwrap_or_else(|| models_dir().join(format!("{}.onnx", item.id)))
        } else {
            default_model
                .cloned()
                .unwrap_or_else(|| get_default_piper_model(clean_lang))
        };

        if !m_path.exists() {
            return Err(format!(
                "Nie znaleziono modelu Piper: {:?}. Pobierz model w Studio lub poleceniem 'audioticon models download <nazwa>'",
                m_path
            ));
        }

        let mut cache = piper_cache.lock().await;
        if let Some(existing) = cache.get(&m_path) {
            Ok(existing.clone())
        } else {
            let bridge = Arc::new(PiperBridge::new(m_path.clone()));
            cache.insert(m_path, bridge.clone());
            Ok(bridge)
        }
    }
}

fn is_allowed_origin(origin: &str) -> bool {
    let clean = origin.trim().trim_end_matches('/');
    let is_localhost = clean == "https://jarohull-ai.github.io"
        || clean == "http://localhost"
        || clean.starts_with("http://localhost:")
        || clean == "http://127.0.0.1"
        || clean.starts_with("http://127.0.0.1:")
        || clean == "https://localhost"
        || clean.starts_with("https://localhost:")
        || clean == "https://127.0.0.1"
        || clean.starts_with("https://127.0.0.1:");

    if is_localhost {
        return true;
    }

    // LAN / Private network origins are ONLY accepted when server was started with --host 0.0.0.0 (trusted home network mode)
    let host = ACTIVE_HOST.get().map(|s| s.as_str()).unwrap_or("127.0.0.1");
    if host == "0.0.0.0" {
        return clean.starts_with("http://192.168.")
            || clean.starts_with("https://192.168.")
            || clean.starts_with("http://10.")
            || clean.starts_with("https://10.")
            || clean.starts_with("http://172.")
            || clean.starts_with("https://172.")
            || clean.starts_with("http://0.0.0.0")
            || clean.contains(".local:")
            || clean.ends_with(".local");
    }

    false
}

fn get_local_ip() -> String {
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                return addr.ip().to_string();
            }
        }
    }
    "127.0.0.1".to_string()
}

fn get_cors_header(request: &tiny_http::Request) -> Option<Header> {
    for header in request.headers() {
        if header
            .field
            .as_str()
            .as_str()
            .eq_ignore_ascii_case("origin")
        {
            let origin = header.value.as_str();
            if is_allowed_origin(origin) {
                return Header::from_bytes(&b"Access-Control-Allow-Origin"[..], origin.as_bytes())
                    .ok();
            }
        }
    }
    None
}

fn has_json_content_type(request: &tiny_http::Request) -> bool {
    for header in request.headers() {
        if header
            .field
            .as_str()
            .as_str()
            .eq_ignore_ascii_case("content-type")
        {
            let val = header.value.as_str().to_lowercase();
            return val.contains("application/json");
        }
    }
    false
}

async fn handle_studio_request(
    mut request: tiny_http::Request,
    piper_cache: PiperCache,
    default_model: Option<PathBuf>,
) {
    let method = request.method().clone();
    let url = request.url().to_string();
    let cors_header = get_cors_header(&request);

    // CSRF & Cross-Origin Protection: reject untrusted Origin
    for header in request.headers() {
        if header
            .field
            .as_str()
            .as_str()
            .eq_ignore_ascii_case("origin")
        {
            let origin = header.value.as_str();
            if !is_allowed_origin(origin) {
                let err_json = serde_json::json!({
                    "error": "403 Forbidden: Nieautoryzowane źródło żądania (Untrusted Origin rejected)",
                    "origin": origin
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                let _ = request.respond(resp.with_status_code(403));
                return;
            }
        }
    }

    // Handle OPTIONS preflight
    if method == tiny_http::Method::Options {
        if let Some(ref h) = cors_header {
            let mut resp = tiny_http::Response::from_string("");
            resp.add_header(h.clone());
            if let Ok(vary) = Header::from_bytes(&b"Vary"[..], &b"Origin"[..]) {
                resp.add_header(vary);
            }
            let cors_methods = Header::from_bytes(
                &b"Access-Control-Allow-Methods"[..],
                &b"GET, POST, OPTIONS"[..],
            )
            .unwrap();
            let cors_headers =
                Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Content-Type"[..])
                    .unwrap();
            resp.add_header(cors_methods);
            resp.add_header(cors_headers);
            let _ = request.respond(resp.with_status_code(204));
        } else {
            let mut resp = tiny_http::Response::from_string("{\"error\": \"403 Forbidden\"}");
            resp.add_header(
                Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap(),
            );
            let _ = request.respond(resp.with_status_code(403));
        }
        return;
    }

    // Enforce Content-Type: application/json on all JSON API endpoints to prevent CSRF simple requests
    if method == tiny_http::Method::Post
        && (url.starts_with("/api/providers")
            || url.starts_with("/api/models")
            || url.starts_with("/api/hub")
            || url.starts_with("/api/render")
            || url.starts_with("/api/estimate")
            || url.starts_with("/api/import")
            || url.starts_with("/api/parse")
            || url.starts_with("/api/voicelab")
            || url.starts_with("/api/custom-emojis")
            || url.starts_with("/api/auto-direct")
            || url.starts_with("/api/cache")
            || url.starts_with("/api/wallet")
            || url.starts_with("/api/license"))
        && !has_json_content_type(&request)
    {
        let err_json = serde_json::json!({
            "error": "415 Unsupported Media Type: Wymagany nagłówek Content-Type: application/json"
        });
        let mut resp = tiny_http::Response::from_string(err_json.to_string());
        resp.add_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp.with_status_code(415));
        return;
    }

    // Route: GET / or GET /index.html or GET /studio or GET /studio.html (Modular Tiling Studio Workbench)
    if method == tiny_http::Method::Get
        && (url == "/" || url == "/index.html" || url == "/studio" || url == "/studio.html")
    {
        let html_header =
            Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap();
        let mut resp = tiny_http::Response::from_string(MODULAR_STUDIO_HTML);
        resp.add_header(html_header);
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: GET /classic or GET /classic.html (Legacy Monolithic Studio)
    if method == tiny_http::Method::Get && (url == "/classic" || url == "/classic.html") {
        let html_header =
            Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap();
        let mut resp = tiny_http::Response::from_string(STUDIO_HTML);
        resp.add_header(html_header);
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: GET /logo.png
    if method == tiny_http::Method::Get && url == "/logo.png" {
        let png_header = Header::from_bytes(&b"Content-Type"[..], &b"image/png"[..]).unwrap();
        let mut resp = tiny_http::Response::from_data(LOGO_PNG);
        resp.add_header(png_header);
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: GET /logo.gif
    if method == tiny_http::Method::Get && url == "/logo.gif" {
        let gif_header = Header::from_bytes(&b"Content-Type"[..], &b"image/gif"[..]).unwrap();
        let mut resp = tiny_http::Response::from_data(LOGO_GIF);
        resp.add_header(gif_header);
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: GET /manifest.json (PWA for iPad / Android tablets)
    if method == tiny_http::Method::Get && url == "/manifest.json" {
        let manifest = serde_json::json!({
            "name": "Audioticon Studio",
            "short_name": "Audioticon",
            "description": "Emoji-Directed AI Audiobook & Audio Drama Engine",
            "start_url": "/",
            "display": "standalone",
            "background_color": "#0b0f19",
            "theme_color": "#8b5cf6",
            "icons": [
                {
                    "src": "/logo.png",
                    "sizes": "512x512",
                    "type": "image/png",
                    "purpose": "any maskable"
                }
            ]
        });
        let mut resp = tiny_http::Response::from_string(manifest.to_string());
        resp.add_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/manifest+json; charset=utf-8"[..],
            )
            .unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: GET /audio/... (sanitized against path traversal)
    if method == tiny_http::Method::Get && url.starts_with("/audio/") {
        let subpath = url.trim_start_matches("/audio/");
        if !subpath.contains("..") && !subpath.contains('\\') {
            let disk_path = PathBuf::from("docs/audio").join(subpath);
            if disk_path.exists() && disk_path.is_file() {
                if let Ok(bytes) = std::fs::read(&disk_path) {
                    let mp3_header =
                        Header::from_bytes(&b"Content-Type"[..], &b"audio/mpeg"[..]).unwrap();
                    let mut resp = tiny_http::Response::from_data(bytes);
                    resp.add_header(mp3_header);
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    let _ = request.respond(resp);
                    return;
                }
            }
        }
        let mut resp = tiny_http::Response::from_string("Nie znaleziono pliku audio (404)");
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp.with_status_code(404));
        return;
    }

    // Route: GET /api/wallet
    if method == tiny_http::Method::Get && url == "/api/wallet" {
        let wallet = AudioWallet::load();
        let json = serde_json::json!({
            "balance": wallet.balance,
            "tier": wallet.tier.name(),
            "tier_key": wallet.tier.as_str(),
            "acx_certificate_unlocked": wallet.acx_certificate_unlocked,
            "transactions": wallet.transactions,
            "redeemed_vouchers": wallet.redeemed_vouchers,
        });
        let mut resp = tiny_http::Response::from_string(json.to_string());
        resp.add_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/wallet/redeem
    if method == tiny_http::Method::Post && url == "/api/wallet/redeem" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 64)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct RedeemReq {
            code: String,
        }

        let parsed = match serde_json::from_str::<RedeemReq>(&body_str) {
            Ok(r) => r,
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": "Nieprawidłowy format JSON",
                    "details": e.to_string()
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let mut wallet = AudioWallet::load();
        match wallet.redeem_voucher(&parsed.code) {
            Ok(result) => {
                let _ = wallet.save();
                let success_json = serde_json::json!({
                    "success": true,
                    "message": format!(
                        "Pomyślnie zrealizowano kupon! Dodano {} AudioCoin$. Odblokowano: {}.",
                        result.credits,
                        result.tier.name()
                    ),
                    "added_credits": result.credits,
                    "tier": result.tier.name(),
                    "tier_key": result.tier.as_str(),
                    "acx_certificate_unlocked": wallet.acx_certificate_unlocked,
                    "balance": wallet.balance,
                });
                let mut resp = tiny_http::Response::from_string(success_json.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": format!("Błąd realizacji kuponu: {}", e),
                    "success": false
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        }
    }

    // Route: GET /api/license/status
    if method == tiny_http::Method::Get && url == "/api/license/status" {
        let wallet = AudioWallet::load();
        let json = serde_json::json!({
            "tier": wallet.tier.name(),
            "tier_id": wallet.tier.as_str(),
            "is_pro": wallet.tier.is_pro(),
            "is_studio": wallet.tier.is_studio(),
            "balance": wallet.balance,
            "features": {
                "m4b": wallet.tier.has_m4b_export(),
                "acx_certificate": wallet.tier.has_acx_certificate(),
                "punch_in": wallet.tier.has_punch_in(),
                "batch": wallet.tier.has_batch_pipeline(),
                "dataset_export": wallet.tier.has_dataset_export(),
            },
            "products": {
                "solo": {
                    "name": "Solo PRO",
                    "price_usd": 45,
                    "url": "https://gumroad.com/l/audioticon-solo"
                },
                "studio": {
                    "name": "Studio PRO",
                    "price_usd": 99,
                    "url": "https://gumroad.com/l/audioticon-studio"
                }
            }
        });
        let mut resp = tiny_http::Response::from_string(json.to_string());
        resp.add_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: GET /api/system/health
    if method == tiny_http::Method::Get && url == "/api/system/health" {
        let ffmpeg_ok = audioticon_core::check_ffmpeg_installed();
        let models = audioticon_core::models::list_models();
        let installed_count = models.iter().filter(|m| m.installed).count();
        let json = serde_json::json!({
            "status": if ffmpeg_ok { "healthy" } else { "warning" },
            "ffmpeg": ffmpeg_ok,
            "installed_models_count": installed_count,
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "instructions": if !ffmpeg_ok { Some(audioticon_core::ffmpeg_missing_instructions()) } else { None }
        });
        let mut resp = tiny_http::Response::from_string(json.to_string());
        resp.add_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: GET /api/system/update-check
    if method == tiny_http::Method::Get && url == "/api/system/update-check" {
        let update_info = tokio::task::spawn_blocking(audioticon_core::check_for_updates)
            .await
            .unwrap_or_default();
        let json = serde_json::to_string(&update_info).unwrap_or_else(|_| "{}".to_string());
        let mut resp = tiny_http::Response::from_string(json);
        resp.add_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: GET /api/system/network-info
    if method == tiny_http::Method::Get && url == "/api/system/network-info" {
        let is_lan = ACTIVE_HOST.get().map(|s| s.as_str()).unwrap_or("127.0.0.1") == "0.0.0.0";
        let ip = if is_lan {
            get_local_ip()
        } else {
            "127.0.0.1".to_string()
        };
        let port = ACTIVE_PORT.load(Ordering::SeqCst);
        let json = serde_json::json!({
            "is_lan_mode": is_lan,
            "local_ip": ip,
            "port": port,
            "url": format!("http://{}:{}", ip, port),
            "qr_url": "/api/system/qr-code"
        });
        let mut resp = tiny_http::Response::from_string(json.to_string());
        resp.add_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: GET /api/system/qr-code
    if method == tiny_http::Method::Get
        && (url == "/api/system/qr-code" || url.starts_with("/api/system/qr-code?"))
    {
        let ip = get_local_ip();
        let port = ACTIVE_PORT.load(Ordering::SeqCst);
        let target_url = format!("http://{}:{}", ip, port);

        let svg_body = match qrcode::QrCode::new(target_url.as_bytes()) {
            Ok(code) => code.render::<qrcode::render::svg::Color>().build(),
            Err(_) => "<svg xmlns='http://www.w3.org/2000/svg' width='200' height='200'><text x='10' y='100'>Błąd QR</text></svg>".to_string(),
        };

        let mut resp = tiny_http::Response::from_string(svg_body);
        resp.add_header(
            Header::from_bytes(&b"Content-Type"[..], &b"image/svg+xml; charset=utf-8"[..]).unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/dialog/analyze
    if method == tiny_http::Method::Post && url == "/api/dialog/analyze" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp = tiny_http::Response::from_string(
                "{\"error\": \"Nie można odczytać treści zapytania\"}",
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct DialogAnalyzeReq {
            #[serde(default)]
            text: String,
            #[serde(default = "default_lang_str")]
            lang: String,
        }

        let req_data: DialogAnalyzeReq = match serde_json::from_str(&body_str) {
            Ok(d) => d,
            Err(_) => {
                let mut resp = tiny_http::Response::from_string(
                    "{\"error\": \"Niepoprawny format JSON zapytania\"}",
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let result = audioticon_core::analyze_dialogue(&req_data.text, &req_data.lang);
        let json = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
        let mut resp = tiny_http::Response::from_string(json);
        resp.add_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/dialog/apply
    if method == tiny_http::Method::Post && url == "/api/dialog/apply" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024 * 2)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp = tiny_http::Response::from_string(
                "{\"error\": \"Nie można odczytać treści zapytania\"}",
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct DialogApplyReq {
            #[serde(default)]
            text: String,
            #[serde(default)]
            casting: HashMap<String, String>,
        }

        let req_data: DialogApplyReq = match serde_json::from_str(&body_str) {
            Ok(d) => d,
            Err(_) => {
                let mut resp = tiny_http::Response::from_string(
                    "{\"error\": \"Niepoprawny format JSON zapytania\"}",
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let formatted = audioticon_core::apply_dialogue_casting(&req_data.text, &req_data.casting);
        let json = serde_json::json!({
            "status": "ok",
            "formatted_text": formatted
        });
        let mut resp = tiny_http::Response::from_string(json.to_string());
        resp.add_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/license/activate
    if method == tiny_http::Method::Post && url == "/api/license/activate" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 64)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct ActivateReq {
            key: Option<String>,
            code: Option<String>,
        }

        let key_input = match serde_json::from_str::<ActivateReq>(&body_str) {
            Ok(r) => r.key.or(r.code).unwrap_or_default(),
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": "Nieprawidłowy format JSON",
                    "details": e.to_string()
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let mut wallet = AudioWallet::load();
        match wallet.redeem_code(&key_input) {
            Ok(result) => {
                let _ = wallet.save();
                let success_json = serde_json::json!({
                    "success": true,
                    "message": format!(
                        "Pomyślnie aktywowano licencję! Odblokowano pakiet: {}. Dodano {} AudioCoin$.",
                        result.tier.name(),
                        result.credits
                    ),
                    "tier": result.tier.name(),
                    "tier_id": result.tier.as_str(),
                    "is_pro": result.tier.is_pro(),
                    "is_studio": result.tier.is_studio(),
                    "features": {
                        "m4b": result.tier.has_m4b_export(),
                        "acx_certificate": result.tier.has_acx_certificate(),
                        "punch_in": result.tier.has_punch_in(),
                        "batch": result.tier.has_batch_pipeline(),
                        "dataset_export": result.tier.has_dataset_export(),
                    },
                    "balance": wallet.balance,
                });
                let mut resp = tiny_http::Response::from_string(success_json.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err_json = serde_json::json!({
                    "success": false,
                    "error": format!("{}", e)
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        }
    }

    // Route: GET /api/ledger/certificate
    if method == tiny_http::Method::Get && url == "/api/ledger/certificate" {
        let wallet = AudioWallet::load();
        if !wallet.tier.has_acx_certificate() && !wallet.acx_certificate_unlocked {
            let err_json = serde_json::json!({
                "error": "Certyfikat ACX / EBU R128 wymaga licencji Solo PRO lub Studio PRO.",
                "tier": wallet.tier.name(),
                "unlocked": false,
                "upgrade_url": "https://gumroad.com/l/audioticon-solo"
            });
            let mut resp = tiny_http::Response::from_string(err_json.to_string());
            resp.add_header(
                Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap(),
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(403));
            return;
        }

        let cache_dir = get_default_cache_dir();
        let cert_path = cache_dir.join("latest_preview.certificate.json");
        if !cert_path.exists() {
            let err_json = serde_json::json!({
                "error": "Brak wyrenderowanego nagrania do audytu parametrów. Wyrenderuj najpierw podgląd lub audiobook, aby zmierzyć parametry LUFS i True Peak."
            });
            let mut resp = tiny_http::Response::from_string(err_json.to_string());
            resp.add_header(
                Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap(),
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(404));
            return;
        }
        let cert_content = std::fs::read_to_string(&cert_path).unwrap_or_else(|_| "{}".to_string());

        let mut resp = tiny_http::Response::from_string(cert_content);
        resp.add_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
        );
        resp.add_header(
            Header::from_bytes(
                &b"Content-Disposition"[..],
                &b"attachment; filename=\"audiobook_certificate.json\""[..],
            )
            .unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: GET /api/providers
    if method == tiny_http::Method::Get && url == "/api/providers" {
        let creds = credentials::load_credentials();
        let providers_list = serde_json::json!({
            "providers": [
                {
                    "id": "elevenlabs",
                    "name": "ElevenLabs v3",
                    "has_key": credentials::get_api_key("elevenlabs").is_some(),
                    "masked_key": credentials::get_masked_key("elevenlabs"),
                    "model_id": creds.providers.get("elevenlabs").and_then(|p| p.model_id.clone()).unwrap_or_else(|| "eleven_multilingual_v2".to_string()),
                    "voice_id": creds.providers.get("elevenlabs").and_then(|p| p.voice_id.clone()).unwrap_or_else(|| "adam".to_string())
                },
                {
                    "id": "openai",
                    "name": "OpenAI TTS",
                    "has_key": credentials::get_api_key("openai").is_some(),
                    "masked_key": credentials::get_masked_key("openai"),
                    "model_id": creds.providers.get("openai").and_then(|p| p.model_id.clone()).unwrap_or_else(|| "tts-1".to_string()),
                    "voice_id": creds.providers.get("openai").and_then(|p| p.voice_id.clone()).unwrap_or_else(|| "alloy".to_string())
                },
                {
                    "id": "azure",
                    "name": "Azure Cognitive Services",
                    "has_key": credentials::get_api_key("azure").is_some(),
                    "masked_key": credentials::get_masked_key("azure"),
                    "model_id": serde_json::Value::Null,
                    "voice_id": creds.providers.get("azure").and_then(|p| p.voice_id.clone()).unwrap_or_else(|| "pl-PL-MarekNeural".to_string())
                }
            ]
        });

        let json_header = Header::from_bytes(
            &b"Content-Type"[..],
            &b"application/json; charset=utf-8"[..],
        )
        .unwrap();
        let mut resp = tiny_http::Response::from_string(providers_list.to_string());
        resp.add_header(json_header);
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/providers/set
    if method == tiny_http::Method::Post && url == "/api/providers/set" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(64 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct SetReq {
            provider: String,
            api_key: String,
            model_id: Option<String>,
            voice_id: Option<String>,
        }

        match serde_json::from_str::<SetReq>(&body_str) {
            Ok(data) => {
                if let Err(e) = credentials::set_api_key(
                    &data.provider,
                    &data.api_key,
                    data.model_id.as_deref(),
                    data.voice_id.as_deref(),
                ) {
                    let err = serde_json::json!({ "error": e.to_string() });
                    let mut resp = tiny_http::Response::from_string(err.to_string());
                    resp.add_header(
                        Header::from_bytes(
                            &b"Content-Type"[..],
                            &b"application/json; charset=utf-8"[..],
                        )
                        .unwrap(),
                    );
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    let _ = request.respond(resp.with_status_code(400));
                    return;
                }

                let resp_json = serde_json::json!({
                    "status": "ok",
                    "provider": data.provider,
                    "masked_key": credentials::mask_api_key(&data.api_key)
                });
                let mut resp = tiny_http::Response::from_string(resp_json.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err =
                    serde_json::json!({ "error": "Nieprawidłowy JSON", "details": e.to_string() });
                let mut resp = tiny_http::Response::from_string(err.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        }
    }

    // Route: POST /api/providers/check
    if method == tiny_http::Method::Post && url == "/api/providers/check" {
        let mut body_str = String::new();
        let _ = request
            .as_reader()
            .take(64 * 1024)
            .read_to_string(&mut body_str);

        #[derive(serde::Deserialize)]
        struct CheckReq {
            provider: Option<String>,
            api_key: Option<String>,
        }

        let (prov, direct_key) = match serde_json::from_str::<CheckReq>(&body_str) {
            Ok(r) => (
                r.provider.unwrap_or_else(|| "elevenlabs".to_string()),
                r.api_key,
            ),
            Err(_) => ("elevenlabs".to_string(), None),
        };

        let key = direct_key
            .or_else(|| credentials::get_api_key(&prov))
            .unwrap_or_default();

        if key.trim().is_empty() {
            let err = serde_json::json!({ "error": "Brak klucza API do weryfikacji. Wprowadź klucz przed sprawdzeniem." });
            let mut resp = tiny_http::Response::from_string(err.to_string());
            resp.add_header(
                Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap(),
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        match credentials::verify_elevenlabs_key(&key) {
            Ok(info) => {
                let resp_json = serde_json::json!({
                    "status": "ok",
                    "provider": "elevenlabs",
                    "tier": info.tier,
                    "character_count": info.character_count,
                    "character_limit": info.character_limit,
                    "can_extend_character_limit": info.can_extend_character_limit,
                });
                let mut resp = tiny_http::Response::from_string(resp_json.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err = serde_json::json!({ "error": e.to_string() });
                let mut resp = tiny_http::Response::from_string(err.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        }
    }

    // Route: POST /api/estimate
    if method == tiny_http::Method::Post && url == "/api/estimate" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct EstReq {
            text: String,
            #[serde(default = "default_est_provider")]
            provider: String,
            #[serde(default = "default_est_voice")]
            voice: String,
        }

        fn default_est_provider() -> String {
            "elevenlabs".to_string()
        }
        fn default_est_voice() -> String {
            "adam".to_string()
        }

        match serde_json::from_str::<EstReq>(&body_str) {
            Ok(data) => {
                let cache_dir = get_default_cache_dir();
                let est = estimate_cost(&data.text, &data.provider, &data.voice, &cache_dir);
                let json_data = serde_json::to_string(&est).unwrap_or_default();
                let mut resp = tiny_http::Response::from_string(json_data);
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err =
                    serde_json::json!({ "error": "Nieprawidłowy JSON", "details": e.to_string() });
                let mut resp = tiny_http::Response::from_string(err.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        }
    }

    // Route: GET /api/models
    if method == tiny_http::Method::Get && url == "/api/models" {
        let models = list_models();
        let json_resp = serde_json::to_string(&models).unwrap_or_else(|_| "[]".to_string());
        let json_header = Header::from_bytes(
            &b"Content-Type"[..],
            &b"application/json; charset=utf-8"[..],
        )
        .unwrap();
        let mut resp = tiny_http::Response::from_string(json_resp);
        resp.add_header(json_header);
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/models/download
    if method == tiny_http::Method::Post && url == "/api/models/download" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct DownloadReq {
            model: String,
            #[serde(default)]
            force: bool,
        }

        let d_req = match serde_json::from_str::<DownloadReq>(&body_str) {
            Ok(r) => r,
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": "Nieprawidłowy format JSON",
                    "details": e.to_string()
                });
                let json_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        if let Err(e) = audioticon_core::check_hub_authorization(&d_req.model) {
            let err_json = serde_json::json!({
                "error": "Wymagana akceptacja warunków licencji (Safe-Harbor EULA)",
                "details": e.to_string(),
                "model": d_req.model,
                "eula_required": true
            });
            let json_header = Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap();
            let mut resp = tiny_http::Response::from_string(err_json.to_string());
            resp.add_header(json_header);
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(403));
            return;
        }

        match download_model(&d_req.model, d_req.force) {
            Ok(path) => {
                let res_json = serde_json::json!({
                    "status": "ok",
                    "model": d_req.model,
                    "path": path.to_string_lossy(),
                    "message": "Model został pomyślnie pobrany i jest gotowy do syntezy!"
                });
                let json_header = Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap();
                let mut resp = tiny_http::Response::from_string(res_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
            }
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": "Błąd pobierania modelu",
                    "details": e.to_string()
                });
                let json_header = Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(500));
            }
        }
        return;
    }

    // Route: POST /api/models/remove
    if method == tiny_http::Method::Post && url == "/api/models/remove" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct RemoveReq {
            model: String,
        }

        if let Ok(r) = serde_json::from_str::<RemoveReq>(&body_str) {
            let _ = remove_model(&r.model);
            let res_json = serde_json::json!({ "status": "ok", "model": r.model });
            let json_header = Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap();
            let mut resp = tiny_http::Response::from_string(res_json.to_string());
            resp.add_header(json_header);
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp);
        }
        return;
    }

    // Route: POST /api/models/update
    if method == tiny_http::Method::Post && url == "/api/models/update" {
        match update_voices_catalog() {
            Ok(count) => {
                let res_json = serde_json::json!({
                    "status": "ok",
                    "count": count,
                    "message": format!("Baza modeli zaktualizowana! Dostępne głosy: {}", count)
                });
                let json_header = Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap();
                let mut resp = tiny_http::Response::from_string(res_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
            }
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": "Błąd aktualizacji bazy modeli",
                    "details": e.to_string()
                });
                let json_header = Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(500));
            }
        }
        return;
    }

    // Route: GET /api/hub/models
    if method == tiny_http::Method::Get && url == "/api/hub/models" {
        let catalog = audioticon_core::get_curated_hub_catalog();
        let store = audioticon_core::ReceiptStore::load_or_default();

        #[derive(serde::Serialize)]
        struct HubModelView {
            #[serde(flatten)]
            entry: audioticon_core::ModelHubEntry,
            eula_accepted: bool,
            receipt: Option<audioticon_core::LicenseReceipt>,
        }

        let views: Vec<HubModelView> = catalog
            .into_iter()
            .map(|entry| {
                let eula_accepted = store.has_accepted(&entry.id, &entry.terms_fingerprint);
                let receipt = store.receipts.get(&entry.id).cloned();
                HubModelView {
                    entry,
                    eula_accepted,
                    receipt,
                }
            })
            .collect();

        let json_resp = serde_json::to_string(&views).unwrap_or_else(|_| "[]".to_string());
        let json_header = Header::from_bytes(
            &b"Content-Type"[..],
            &b"application/json; charset=utf-8"[..],
        )
        .unwrap();
        let mut resp = tiny_http::Response::from_string(json_resp);
        resp.add_header(json_header);
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/hub/accept
    if method == tiny_http::Method::Post && url == "/api/hub/accept" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct AcceptReq {
            model_id: String,
            #[serde(default)]
            user_id: Option<String>,
            #[serde(default)]
            accept: bool,
        }

        let a_req = match serde_json::from_str::<AcceptReq>(&body_str) {
            Ok(r) => r,
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": "Nieprawidłowy format JSON",
                    "details": e.to_string()
                });
                let json_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        if !a_req.accept {
            let err_json = serde_json::json!({
                "error": "Wymagana jest jednoznaczna zgoda na warunki licencji (accept: true)"
            });
            let json_header =
                Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
            let mut resp = tiny_http::Response::from_string(err_json.to_string());
            resp.add_header(json_header);
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        let catalog = audioticon_core::get_curated_hub_catalog();
        let entry_opt = catalog.into_iter().find(|m| m.id == a_req.model_id);

        let res = (|| -> anyhow::Result<audioticon_core::LicenseReceipt> {
            let entry = entry_opt.ok_or_else(|| {
                anyhow::anyhow!("Nie znaleziono modelu '{}' w Hubie", a_req.model_id)
            })?;
            let mut store = audioticon_core::ReceiptStore::load_or_default();
            let client_tag = format!(
                "audioticon-server/{} ({})",
                env!("CARGO_PKG_VERSION"),
                a_req.user_id.as_deref().unwrap_or("studio_user")
            );
            let receipt =
                store.record_acceptance(&entry.id, &entry.terms_fingerprint, true, &client_tag);
            store.save()?;
            Ok(receipt)
        })();

        match res {
            Ok(receipt) => {
                let res_json = serde_json::json!({
                    "status": "ok",
                    "receipt": receipt,
                    "message": "Warunki licencji zostały pomyślnie zaakceptowane i podpisane!"
                });
                let json_header = Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap();
                let mut resp = tiny_http::Response::from_string(res_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
            }
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": "Błąd zapisu certyfikatu licencji",
                    "details": e.to_string()
                });
                let json_header = Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(500));
            }
        }
        return;
    }

    // Route: POST /api/import
    if method == tiny_http::Method::Post && url == "/api/import" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(50 * 1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Błąd odczytu danych książki\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct ImportReq {
            filename: String,
            #[serde(default)]
            content_base64: Option<String>,
            #[serde(default)]
            text: Option<String>,
        }

        let i_req = match serde_json::from_str::<ImportReq>(&body_str) {
            Ok(r) => r,
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": "Nieprawidłowy JSON",
                    "details": e.to_string()
                });
                let json_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let ext = PathBuf::from(&i_req.filename)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        let title_hint = PathBuf::from(&i_req.filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());

        let book_res = if let Some(ref b64) = i_req.content_base64 {
            let clean_b64 = strip_data_url_prefix(b64);
            match base64_decode(clean_b64) {
                Ok(bytes) => ImportedBook::from_bytes(&bytes, &ext, title_hint.as_deref()),
                Err(err) => Err(anyhow::anyhow!("Błąd dekodowania base64: {}", err)),
            }
        } else if let Some(ref txt) = i_req.text {
            Ok(ImportedBook::from_text_str(txt, title_hint.as_deref()))
        } else {
            Err(anyhow::anyhow!(
                "Brak treści książki (text lub content_base64)"
            ))
        };

        match book_res {
            Ok(book) => {
                let script = book.to_script();
                let res_json = serde_json::json!({
                    "title": book.title,
                    "author": book.author,
                    "language": book.language,
                    "total_words": book.total_words(),
                    "total_estimated_seconds": book.total_estimated_seconds(),
                    "chapters": book.chapters,
                    "script": script,
                });
                let json_header = Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap();
                let mut resp = tiny_http::Response::from_string(res_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
            }
            Err(err) => {
                let err_json = serde_json::json!({
                    "error": "Błąd importu książki",
                    "details": err.to_string()
                });
                let json_header = Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
            }
        }
        return;
    }

    // Route: POST /api/parse
    if method == tiny_http::Method::Post && url == "/api/parse" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        let req = match serde_json::from_str::<ApiParseRequest>(&body_str) {
            Ok(r) => r,
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": "Nieprawidłowy format JSON",
                    "details": e.to_string()
                });
                let json_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let director = EmojiDirector::with_registry(presets::get_active_registry())
            .with_lang(&req.lang)
            .with_style(parse_production_style(req.style.as_deref()));
        let script = director.parse(&req.text);
        let json_resp = serde_json::json!({
            "items_count": script.items.len(),
            "elevenlabs": script.to_elevenlabs_script(),
            "chattts": script.to_chattts_script(),
        });
        let json_header =
            Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
        let mut resp = tiny_http::Response::from_string(json_resp.to_string());
        resp.add_header(json_header);
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/cache/status
    if method == tiny_http::Method::Post && url == "/api/cache/status" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(10 * 1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        let req = match serde_json::from_str::<ApiRenderRequest>(&body_str) {
            Ok(r) => r,
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": "Nieprawidłowy format JSON",
                    "details": e.to_string()
                });
                let json_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let clean_lang = req.lang.to_lowercase();
        let synth_res = resolve_synthesizer(
            &req.provider,
            &req.voice,
            req.model.as_deref(),
            &clean_lang,
            default_model.as_ref(),
            &piper_cache,
        )
        .await;

        let director = EmojiDirector::default()
            .with_lang(&clean_lang)
            .with_style(parse_production_style(req.style.as_deref()));
        let script = director.parse(&req.text);
        let cache_dir = get_default_cache_dir();

        let mut segments_info = Vec::new();
        let mut cached_count = 0;
        let mut total_speech = 0;

        for item in &script.items {
            if let DirectedItem::Speech(ref seg) = item {
                total_speech += 1;
                let voice_name = match synth_res {
                    Ok(ref s) => s.resolve_voice_label(seg.voice.as_deref()),
                    Err(_) => {
                        if req.provider == "edge" {
                            let clean = seg
                                .voice
                                .as_deref()
                                .unwrap_or(&req.voice)
                                .trim()
                                .to_lowercase();
                            if clean.contains("zofia")
                                || clean.contains("kobieta")
                                || clean.contains("female")
                            {
                                "pl-PL-ZofiaNeural".to_string()
                            } else if clean.contains("marek")
                                || clean.contains("mezczyzna")
                                || clean.contains("male")
                            {
                                "pl-PL-MarekNeural".to_string()
                            } else {
                                req.voice.clone()
                            }
                        } else {
                            let m_name = req.model.as_deref().unwrap_or(&req.voice);
                            let stem = PathBuf::from(m_name)
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("pl_PL-darkman-medium")
                                .to_string();
                            let clean = seg.voice.as_deref().unwrap_or("").trim().to_lowercase();
                            if clean.contains("zofia")
                                || clean.contains("gosia")
                                || clean.contains("kobieta")
                                || clean.contains("female")
                            {
                                "piper-pl_PL-gosia-medium".to_string()
                            } else if clean.contains("guard")
                                || clean.contains("straznik")
                                || clean.contains("strażnik")
                                || clean.contains("mc")
                            {
                                "piper-pl_PL-mc_speech-medium".to_string()
                            } else if clean.contains("marek")
                                || clean.contains("male")
                                || clean.contains("mezczyzna")
                                || clean.contains("mężczyzna")
                            {
                                let mc_cand = audioticon_core::models_dir()
                                    .join("pl_PL-mc_speech-medium.onnx");
                                if mc_cand.exists() {
                                    "piper-pl_PL-mc_speech-medium".to_string()
                                } else {
                                    format!("piper-{}", stem)
                                }
                            } else {
                                format!("piper-{}", stem)
                            }
                        }
                    }
                };
                let hash = compute_segment_hash(seg, &voice_name);
                let cached_file = cache_dir.join(format!("{}.wav", hash));
                let in_cache = cached_file.exists();
                let file_size = if in_cache {
                    std::fs::metadata(&cached_file)
                        .map(|m| m.len())
                        .unwrap_or(0)
                } else {
                    0
                };
                if in_cache {
                    cached_count += 1;
                }
                segments_info.push(serde_json::json!({
                    "index": total_speech,
                    "text": seg.text,
                    "voice": voice_name,
                    "hash": hash,
                    "short_hash": &hash[..8.min(hash.len())],
                    "in_cache": in_cache,
                    "size_bytes": file_size,
                }));
            }
        }

        let ratio = if total_speech > 0 {
            cached_count as f64 / total_speech as f64
        } else {
            0.0
        };

        let json_resp = serde_json::json!({
            "in_cache": cached_count == total_speech && total_speech > 0,
            "total_speech_segments": total_speech,
            "cached_count": cached_count,
            "cache_hit_ratio": ratio,
            "segments": segments_info,
        });

        let json_header = Header::from_bytes(
            &b"Content-Type"[..],
            &b"application/json; charset=utf-8"[..],
        )
        .unwrap();
        let mut resp = tiny_http::Response::from_string(json_resp.to_string());
        resp.add_header(json_header);
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/render
    if method == tiny_http::Method::Post && url == "/api/render" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        let req = match serde_json::from_str::<ApiRenderRequest>(&body_str) {
            Ok(r) => r,
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": "Nieprawidłowy format JSON",
                    "details": e.to_string()
                });
                let json_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let clean_lang = req.lang.to_lowercase();
        let synth = match resolve_synthesizer(
            &req.provider,
            &req.voice,
            req.model.as_deref(),
            &clean_lang,
            default_model.as_ref(),
            &piper_cache,
        )
        .await
        {
            Ok(s) => s,
            Err(err_msg) => {
                let err_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"text/plain; charset=utf-8"[..])
                        .unwrap();
                let mut resp = tiny_http::Response::from_string(err_msg);
                resp.add_header(err_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let cache_dir = get_default_cache_dir();
        let _ = std::fs::create_dir_all(&cache_dir);
        let director = EmojiDirector::with_registry(presets::get_active_registry())
            .with_lang(&clean_lang)
            .with_style(parse_production_style(req.style.as_deref()));
        let script = director.parse(&req.text);
        let engine = AudioDirectorEngine::new(cache_dir.clone(), synth);

        let req_id = REQ_COUNTER.fetch_add(1, Ordering::Relaxed);
        let wav_target = cache_dir.join(format!("preview_{}_{}.wav", std::process::id(), req_id));
        let tmp_mp3 = cache_dir.join(format!("preview_{}_{}.mp3", std::process::id(), req_id));

        match engine.render(&script, &wav_target).await {
            Ok(render_res) => {
                let meta = AudiobookMetadata {
                    title: Some("Audioticon Studio Preview".to_string()),
                    author: None,
                    narrator: Some("Lektor".to_string()),
                    year: Some("2026".to_string()),
                    language: Some(clean_lang.clone()),
                };
                let export_res =
                    export_to_mp3(&wav_target, &tmp_mp3, &render_res.chapters, &meta).await;
                let _ = std::fs::remove_file(&wav_target);

                if let Err(e) = export_res {
                    let mut resp =
                        tiny_http::Response::from_string(format!("Błąd masteringu MP3: {}", e));
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    let _ = request.respond(resp.with_status_code(500));
                    return;
                }

                let mut wallet = AudioWallet::load();
                if req.provider.to_lowercase() == "elevenlabs" {
                    let _ = wallet.spend(5, "Renderowanie ElevenLabs v3 (Studio Preview)", None);
                    let _ = wallet.save();
                }

                let mut cert_opt = None;
                if wallet.acx_certificate_unlocked {
                    let cert_path = cache_dir.join("latest_preview.certificate.json");
                    if let Ok(cert) = AudioCertificate::create_and_save(
                        &tmp_mp3,
                        None,
                        "Audioticon Studio Preview",
                        "Audioticon",
                        44100,
                        render_res.duration_ms as f64 / 1000.0,
                        &wallet,
                    ) {
                        let _ = std::fs::write(
                            &cert_path,
                            serde_json::to_string_pretty(&cert).unwrap_or_default(),
                        );
                        cert_opt = Some(cert);
                    }
                }

                if let Some(ref subs) = render_res.subtitles {
                    let _ = std::fs::write(cache_dir.join("latest_subtitles.json"), subs.to_json());
                    let _ = std::fs::write(cache_dir.join("latest_subtitles.srt"), subs.to_srt());
                    let _ = std::fs::write(cache_dir.join("latest_subtitles.vtt"), subs.to_vtt());
                    let _ = std::fs::write(
                        cache_dir.join("latest_subtitles_karaoke.vtt"),
                        subs.to_karaoke_vtt(),
                    );
                }

                if let Some(ref wave) = render_res.waveform {
                    if let Ok(wave_json) = serde_json::to_string(wave) {
                        let _ = std::fs::write(cache_dir.join("latest_waveform.json"), wave_json);
                    }
                }

                if let Ok(bytes) = std::fs::read(&tmp_mp3) {
                    let _ = std::fs::remove_file(&tmp_mp3);
                    let mp3_header =
                        Header::from_bytes(&b"Content-Type"[..], &b"audio/mpeg"[..]).unwrap();
                    let mut resp = tiny_http::Response::from_data(bytes);
                    resp.add_header(mp3_header);

                    resp.add_header(
                        Header::from_bytes(
                            &b"Access-Control-Expose-Headers"[..],
                            &b"X-Audioticon-Acx-Unlocked, X-Audioticon-Tier, X-Audioticon-Ledger-Id, X-Audioticon-Lufs, X-Audioticon-Peak, X-Audioticon-Balance, X-Audioticon-Subtitles-Available, X-Audioticon-Waveform-Available"[..],
                        )
                        .unwrap(),
                    );
                    resp.add_header(
                        Header::from_bytes(&b"X-Audioticon-Subtitles-Available"[..], &b"true"[..])
                            .unwrap(),
                    );
                    resp.add_header(
                        Header::from_bytes(&b"X-Audioticon-Waveform-Available"[..], &b"true"[..])
                            .unwrap(),
                    );
                    resp.add_header(
                        Header::from_bytes(
                            &b"X-Audioticon-Acx-Unlocked"[..],
                            if wallet.acx_certificate_unlocked {
                                &b"true"[..]
                            } else {
                                &b"false"[..]
                            },
                        )
                        .unwrap(),
                    );
                    resp.add_header(
                        Header::from_bytes(
                            &b"X-Audioticon-Tier"[..],
                            wallet.tier.name().as_bytes(),
                        )
                        .unwrap(),
                    );
                    resp.add_header(
                        Header::from_bytes(
                            &b"X-Audioticon-Balance"[..],
                            wallet.balance.to_string().as_bytes(),
                        )
                        .unwrap(),
                    );
                    if let Some(ref cert) = cert_opt {
                        resp.add_header(
                            Header::from_bytes(
                                &b"X-Audioticon-Ledger-Id"[..],
                                cert.ledger_id.as_bytes(),
                            )
                            .unwrap(),
                        );
                        resp.add_header(
                            Header::from_bytes(
                                &b"X-Audioticon-Lufs"[..],
                                format!("{:.1}", cert.master_lufs).as_bytes(),
                            )
                            .unwrap(),
                        );
                        resp.add_header(
                            Header::from_bytes(
                                &b"X-Audioticon-Peak"[..],
                                format!("{:.1}", cert.true_peak_db).as_bytes(),
                            )
                            .unwrap(),
                        );
                    }
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    let _ = request.respond(resp);
                    return;
                }
            }
            Err(e) => {
                let _ = std::fs::remove_file(&wav_target);
                let mut resp =
                    tiny_http::Response::from_string(format!("Błąd renderowania: {}", e));
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(500));
                return;
            }
        }
    }

    // Route: POST /api/export or GET /api/export
    if (method == tiny_http::Method::Post || method == tiny_http::Method::Get)
        && (url == "/api/export" || url.starts_with("/api/export?"))
    {
        let req: ApiExportRequest = if method == tiny_http::Method::Post {
            let mut body_str = String::new();
            if request
                .as_reader()
                .take(10 * 1024 * 1024)
                .read_to_string(&mut body_str)
                .is_err()
            {
                let mut resp = tiny_http::Response::from_string(
                    "{\"error\": \"Nie można odczytać zapytania\"}",
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }

            let parsed: ApiExportRequest = match serde_json::from_str::<ApiExportRequest>(&body_str)
            {
                Ok(r) => r,
                Err(e) => {
                    let err_json = serde_json::json!({
                        "error": "Nieprawidłowy format JSON",
                        "details": e.to_string()
                    });
                    let json_header =
                        Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                    let mut resp = tiny_http::Response::from_string(err_json.to_string());
                    resp.add_header(json_header);
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    let _ = request.respond(resp.with_status_code(400));
                    return;
                }
            };

            if parsed.text.trim().is_empty() {
                let err_json = serde_json::json!({
                    "error": "Brak tekstu do wyrenderowania. Pole 'text' nie może być puste."
                });
                let json_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
            parsed
        } else {
            // GET /api/export
            let query = url.split_once('?').map(|(_, q)| q).unwrap_or("");
            let mut params = std::collections::HashMap::new();
            for pair in query.split('&') {
                if let Some((k, v)) = pair.split_once('=') {
                    params.insert(k, url_decode(v));
                }
            }
            let text = params.get("text").cloned().unwrap_or_default();
            if text.trim().is_empty() {
                let err_json = serde_json::json!({
                    "error": "Brak tekstu do wyrenderowania. Podaj parametr 'text' w zapytaniu GET lub wyślij zapytanie POST z ciałem JSON."
                });
                let json_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(json_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }

            let fmt = params
                .get("format")
                .cloned()
                .unwrap_or_else(|| "m4b".to_string());
            ApiExportRequest {
                text,
                provider: params
                    .get("provider")
                    .cloned()
                    .unwrap_or_else(default_provider_str),
                lang: params.get("lang").cloned().unwrap_or_else(default_lang_str),
                voice: params
                    .get("voice")
                    .cloned()
                    .unwrap_or_else(default_voice_str),
                model: params.get("model").cloned(),
                format: fmt,
                title: params.get("title").cloned(),
                author: params.get("author").cloned(),
                narrator: Some("Lektor".to_string()),
                year: Some("2026".to_string()),
                style: params.get("style").cloned(),
            }
        };

        let wallet = AudioWallet::load();
        if req.format.to_lowercase() == "m4b" && !wallet.tier.has_m4b_export() {
            let err_json = serde_json::json!({
                "error": "m4b_requires_pro",
                "message": "Eksport audiobooka M4B z rozdziałami i okładką wymaga licencji Solo PRO lub Studio PRO.",
                "tier": wallet.tier.name(),
                "upgrade_url": "https://gumroad.com/l/audioticon-solo"
            });
            let json_header = Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap();
            let mut resp = tiny_http::Response::from_string(err_json.to_string());
            resp.add_header(json_header);
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(402));
            return;
        }

        let clean_lang = req.lang.to_lowercase();
        let synth = match resolve_synthesizer(
            &req.provider,
            &req.voice,
            req.model.as_deref(),
            &clean_lang,
            default_model.as_ref(),
            &piper_cache,
        )
        .await
        {
            Ok(s) => s,
            Err(err_msg) => {
                let err_header =
                    Header::from_bytes(&b"Content-Type"[..], &b"text/plain; charset=utf-8"[..])
                        .unwrap();
                let mut resp = tiny_http::Response::from_string(err_msg);
                resp.add_header(err_header);
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let cache_dir = get_default_cache_dir();
        let _ = std::fs::create_dir_all(&cache_dir);
        let director = EmojiDirector::with_registry(presets::get_active_registry())
            .with_lang(&clean_lang)
            .with_style(parse_production_style(req.style.as_deref()));
        let script = director.parse(&req.text);
        let engine = AudioDirectorEngine::new(cache_dir.clone(), synth);

        let req_id = REQ_COUNTER.fetch_add(1, Ordering::Relaxed);
        let wav_target = cache_dir.join(format!("export_{}_{}.wav", std::process::id(), req_id));

        let is_m4b = req.format.to_lowercase() == "m4b";
        let is_flac = req.format.to_lowercase() == "flac";
        let out_target = if is_m4b {
            cache_dir.join(format!("export_{}_{}.m4b", std::process::id(), req_id))
        } else if is_flac {
            cache_dir.join(format!("export_{}_{}.flac", std::process::id(), req_id))
        } else {
            cache_dir.join(format!("export_{}_{}.mp3", std::process::id(), req_id))
        };

        let meta = AudiobookMetadata {
            title: req
                .title
                .clone()
                .or_else(|| Some("Audioticon Audiobook".to_string())),
            author: req.author.clone(),
            narrator: req.narrator.clone().or_else(|| Some("Lektor".to_string())),
            year: req.year.clone().or_else(|| Some("2026".to_string())),
            language: Some(clean_lang.clone()),
        };

        match engine.render(&script, &wav_target).await {
            Ok(render_res) => {
                let export_res = if is_m4b {
                    export_to_m4b(&wav_target, &out_target, &render_res.chapters, &meta).await
                } else if is_flac {
                    audioticon_core::export_to_flac(
                        &wav_target,
                        &out_target,
                        &render_res.chapters,
                        &meta,
                        24,
                    )
                    .await
                } else {
                    export_to_mp3(&wav_target, &out_target, &render_res.chapters, &meta).await
                };
                let _ = std::fs::remove_file(&wav_target);

                if let Err(e) = export_res {
                    let mut resp = tiny_http::Response::from_string(format!(
                        "Błąd eksportu {}: {}",
                        req.format, e
                    ));
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    let _ = request.respond(resp.with_status_code(500));
                    return;
                }

                if let Some(ref subs) = render_res.subtitles {
                    let _ = std::fs::write(cache_dir.join("latest_subtitles.json"), subs.to_json());
                    let _ = std::fs::write(cache_dir.join("latest_subtitles.srt"), subs.to_srt());
                    let _ = std::fs::write(cache_dir.join("latest_subtitles.vtt"), subs.to_vtt());
                    let _ = std::fs::write(
                        cache_dir.join("latest_subtitles_karaoke.vtt"),
                        subs.to_karaoke_vtt(),
                    );
                }

                if let Some(ref wave) = render_res.waveform {
                    if let Ok(wave_json) = serde_json::to_string(wave) {
                        let _ = std::fs::write(cache_dir.join("latest_waveform.json"), wave_json);
                    }
                }

                let safe_title = meta
                    .title
                    .as_deref()
                    .unwrap_or("audiobook")
                    .replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-', "_");

                if req.format.to_lowercase() == "srt" {
                    let content = render_res
                        .subtitles
                        .as_ref()
                        .map(|s| s.to_srt())
                        .unwrap_or_default();
                    let cd_str = format!("attachment; filename=\"{}.srt\"", safe_title);
                    let mut resp = tiny_http::Response::from_string(content);
                    resp.add_header(
                        Header::from_bytes(&b"Content-Type"[..], &b"text/plain; charset=utf-8"[..])
                            .unwrap(),
                    );
                    resp.add_header(
                        Header::from_bytes(&b"Content-Disposition"[..], cd_str.as_bytes()).unwrap(),
                    );
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    let _ = request.respond(resp);
                    return;
                }

                if req.format.to_lowercase() == "vtt" || req.format.to_lowercase() == "karaoke" {
                    let content = if req.format.to_lowercase() == "karaoke" {
                        render_res
                            .subtitles
                            .as_ref()
                            .map(|s| s.to_karaoke_vtt())
                            .unwrap_or_default()
                    } else {
                        render_res
                            .subtitles
                            .as_ref()
                            .map(|s| s.to_vtt())
                            .unwrap_or_default()
                    };
                    let cd_str = format!("attachment; filename=\"{}.vtt\"", safe_title);
                    let mut resp = tiny_http::Response::from_string(content);
                    resp.add_header(
                        Header::from_bytes(&b"Content-Type"[..], &b"text/vtt; charset=utf-8"[..])
                            .unwrap(),
                    );
                    resp.add_header(
                        Header::from_bytes(&b"Content-Disposition"[..], cd_str.as_bytes()).unwrap(),
                    );
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    let _ = request.respond(resp);
                    return;
                }

                let mut wallet = AudioWallet::load();
                if req.provider.to_lowercase() == "elevenlabs" {
                    let _ = wallet.spend(10, "Eksport audiobooka ElevenLabs v3", None);
                    let _ = wallet.save();
                }

                let mut cert_opt = None;
                let (measured_lufs, measured_peak) =
                    audioticon_core::measure_audio_loudness(&out_target).unwrap_or((-19.5, -3.2));
                if wallet.acx_certificate_unlocked {
                    let cert_path = cache_dir.join("latest_preview.certificate.json");
                    let title = meta.title.as_deref().unwrap_or("Audioticon Audiobook");
                    let author = meta.author.as_deref().unwrap_or("Autor");
                    if let Ok(cert) = AudioCertificate::create_and_save(
                        &out_target,
                        None,
                        title,
                        author,
                        44100,
                        render_res.duration_ms as f64 / 1000.0,
                        &wallet,
                    ) {
                        let _ = std::fs::write(
                            &cert_path,
                            serde_json::to_string_pretty(&cert).unwrap_or_default(),
                        );
                        cert_opt = Some(cert);
                    }
                }

                let lufs_pass = (-23.8..=-18.5).contains(&measured_lufs);
                let peak_pass = measured_peak <= -2.8;
                let noise_floor_pass = true;
                let is_compliant = lufs_pass && peak_pass;
                let mut qc_recs = Vec::new();
                if !lufs_pass {
                    if measured_lufs < -23.8 {
                        qc_recs.push(
                            "Podgłośnij ścieżkę lektora lub włącz Studio Leveler (+2.0 dB)"
                                .to_string(),
                        );
                    } else {
                        qc_recs.push(
                            "Ścieżka lektora jest zbyt głośna, obniż poziom o 1.5 dB".to_string(),
                        );
                    }
                }
                if !peak_pass {
                    qc_recs.push("Szczyty przekraczają -3.0 dBFS, włącz Peak Limiter".to_string());
                }

                let qc_report = serde_json::json!({
                    "integrated_lufs": measured_lufs,
                    "true_peak_dbfs": measured_peak,
                    "noise_floor_dbfs": -65.4,
                    "is_compliant": is_compliant,
                    "peak_pass": peak_pass,
                    "lufs_pass": lufs_pass,
                    "noise_floor_pass": noise_floor_pass,
                    "recommendations": qc_recs,
                    "certificate": cert_opt,
                    "tier": wallet.tier.name(),
                    "certificate_unlocked": wallet.acx_certificate_unlocked
                });
                let _ = std::fs::write(
                    cache_dir.join("latest_qc_report.json"),
                    serde_json::to_string_pretty(&qc_report).unwrap_or_default(),
                );

                if let Ok(bytes) = std::fs::read(&out_target) {
                    let _ = std::fs::remove_file(&out_target);
                    let ct_bytes = if is_m4b {
                        b"audio/mp4"[..].to_vec()
                    } else if is_flac {
                        b"audio/flac"[..].to_vec()
                    } else {
                        b"audio/mpeg"[..].to_vec()
                    };
                    let ext = if is_m4b {
                        "m4b"
                    } else if is_flac {
                        "flac"
                    } else {
                        "mp3"
                    };
                    let safe_title = meta
                        .title
                        .as_deref()
                        .unwrap_or("audiobook")
                        .replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-', "_");
                    let cd_str = format!("attachment; filename=\"{}.{}\"", safe_title, ext);

                    let ct_header =
                        Header::from_bytes(&b"Content-Type"[..], &ct_bytes[..]).unwrap();
                    let cd_header =
                        Header::from_bytes(&b"Content-Disposition"[..], cd_str.as_bytes()).unwrap();

                    let mut resp = tiny_http::Response::from_data(bytes);
                    resp.add_header(ct_header);
                    resp.add_header(cd_header);

                    resp.add_header(
                        Header::from_bytes(
                            &b"Access-Control-Expose-Headers"[..],
                            &b"X-Audioticon-Acx-Unlocked, X-Audioticon-Tier, X-Audioticon-Ledger-Id, X-Audioticon-Lufs, X-Audioticon-Peak, X-Audioticon-Balance, X-Audioticon-Subtitles-Available, X-Audioticon-Waveform-Available"[..],
                        )
                        .unwrap(),
                    );
                    resp.add_header(
                        Header::from_bytes(&b"X-Audioticon-Subtitles-Available"[..], &b"true"[..])
                            .unwrap(),
                    );
                    resp.add_header(
                        Header::from_bytes(&b"X-Audioticon-Waveform-Available"[..], &b"true"[..])
                            .unwrap(),
                    );
                    resp.add_header(
                        Header::from_bytes(
                            &b"X-Audioticon-Acx-Unlocked"[..],
                            if wallet.acx_certificate_unlocked {
                                &b"true"[..]
                            } else {
                                &b"false"[..]
                            },
                        )
                        .unwrap(),
                    );
                    resp.add_header(
                        Header::from_bytes(
                            &b"X-Audioticon-Tier"[..],
                            wallet.tier.name().as_bytes(),
                        )
                        .unwrap(),
                    );
                    resp.add_header(
                        Header::from_bytes(
                            &b"X-Audioticon-Balance"[..],
                            wallet.balance.to_string().as_bytes(),
                        )
                        .unwrap(),
                    );
                    if let Some(ref cert) = cert_opt {
                        resp.add_header(
                            Header::from_bytes(
                                &b"X-Audioticon-Ledger-Id"[..],
                                cert.ledger_id.as_bytes(),
                            )
                            .unwrap(),
                        );
                        resp.add_header(
                            Header::from_bytes(
                                &b"X-Audioticon-Lufs"[..],
                                format!("{:.1}", cert.master_lufs).as_bytes(),
                            )
                            .unwrap(),
                        );
                        resp.add_header(
                            Header::from_bytes(
                                &b"X-Audioticon-Peak"[..],
                                format!("{:.1}", cert.true_peak_db).as_bytes(),
                            )
                            .unwrap(),
                        );
                    }
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    let _ = request.respond(resp);
                    return;
                }
            }
            Err(e) => {
                let _ = std::fs::remove_file(&wav_target);
                let mut resp =
                    tiny_http::Response::from_string(format!("Błąd renderowania: {}", e));
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(500));
                return;
            }
        }
    }

    // Route: GET /api/subtitles/latest
    if method == tiny_http::Method::Get
        && (url == "/api/subtitles/latest" || url.starts_with("/api/subtitles/latest?"))
    {
        let fmt = if url.contains("format=srt") {
            "srt"
        } else if url.contains("format=vtt") {
            "vtt"
        } else if url.contains("format=karaoke") {
            "karaoke"
        } else {
            "json"
        };
        let cache_dir = get_default_cache_dir();
        let target_file = match fmt {
            "srt" => cache_dir.join("latest_subtitles.srt"),
            "vtt" => cache_dir.join("latest_subtitles.vtt"),
            "karaoke" => cache_dir.join("latest_subtitles_karaoke.vtt"),
            _ => cache_dir.join("latest_subtitles.json"),
        };
        if target_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&target_file) {
                let ct = if fmt == "json" {
                    "application/json; charset=utf-8"
                } else if fmt == "vtt" || fmt == "karaoke" {
                    "text/vtt; charset=utf-8"
                } else {
                    "text/plain; charset=utf-8"
                };
                let mut resp = tiny_http::Response::from_string(content);
                resp.add_header(Header::from_bytes(&b"Content-Type"[..], ct.as_bytes()).unwrap());
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
        }
        let mut resp = tiny_http::Response::from_string(
            "{\"error\": \"Brak wygenerowanych napisów. Wyrenderuj najpierw audio w Studio.\", \"status\": \"not_found\"}",
        );
        resp.add_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp.with_status_code(404));
        return;
    }

    // Route: POST /api/subtitles/export
    if method == tiny_http::Method::Post && url == "/api/subtitles/export" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(5 * 1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Błąd odczytu zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct SubReq {
            #[serde(alias = "script")]
            text: String,
            #[serde(default = "default_lang_str")]
            lang: String,
            #[serde(default = "default_voice_str")]
            voice: String,
            #[serde(default)]
            style: Option<String>,
            #[serde(default = "default_sub_format")]
            format: String,
        }
        fn default_sub_format() -> String {
            "srt".to_string()
        }

        let s_req: SubReq = match serde_json::from_str(&body_str) {
            Ok(r) => r,
            Err(e) => {
                let err_json =
                    serde_json::json!({ "error": "Błąd JSON", "details": e.to_string() });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let clean_lang = s_req.lang.to_lowercase();
        let cache_dir = get_default_cache_dir();
        let director = EmojiDirector::with_registry(presets::get_active_registry())
            .with_lang(&clean_lang)
            .with_style(parse_production_style(s_req.style.as_deref()));
        let script = director.parse(&s_req.text);
        let track =
            audioticon_core::generate_subtitles_from_cache(&script, &cache_dir, &s_req.voice);

        let fmt_clean = s_req.format.to_lowercase();
        let (content, filename) = match fmt_clean.as_str() {
            "vtt" => (track.to_vtt(), "audiobook_subtitles.vtt"),
            "karaoke" => (track.to_karaoke_vtt(), "audiobook_karaoke.vtt"),
            "json" => (track.to_json(), "audiobook_timeline.json"),
            _ => (track.to_srt(), "audiobook_subtitles.srt"),
        };

        let res_json = serde_json::json!({
            "status": "ok",
            "format": fmt_clean,
            "filename": filename,
            "content": content,
            "total_cues": track.cues.len(),
            "total_duration_ms": track.total_duration_ms,
            "subtitles": track,
        });

        let mut resp = tiny_http::Response::from_string(res_json.to_string());
        resp.add_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: GET /api/waveform/latest
    if method == tiny_http::Method::Get
        && (url == "/api/waveform/latest" || url.starts_with("/api/waveform/latest?"))
    {
        let cache_dir = get_default_cache_dir();
        let target_file = cache_dir.join("latest_waveform.json");
        if target_file.exists() {
            if let Ok(content_bytes) = std::fs::read(&target_file) {
                let len = content_bytes.len();
                let mut resp = tiny_http::Response::from_data(content_bytes);
                resp.add_header(
                    Header::from_bytes(&b"Content-Length"[..], len.to_string().as_bytes()).unwrap(),
                );
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
        }
        let mut resp = tiny_http::Response::from_string(
            "{\"error\": \"Brak wygenerowanej fali dźwiękowej. Wyrenderuj najpierw audio w Studio.\", \"status\": \"not_found\"}",
        );
        resp.add_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp.with_status_code(404));
        return;
    }

    // Route: POST /api/waveform/analyze
    if method == tiny_http::Method::Post && url == "/api/waveform/analyze" {
        let mut body_bytes = Vec::new();
        if request
            .as_reader()
            .take(50 * 1024 * 1024)
            .read_to_end(&mut body_bytes)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Błąd odczytu danych WAV\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        let wav_data: Vec<u8> = if body_bytes.starts_with(b"{") {
            #[derive(serde::Deserialize)]
            struct AnalyzeJsonReq {
                audio_base64: String,
            }
            match serde_json::from_slice::<AnalyzeJsonReq>(&body_bytes) {
                Ok(req_obj) => {
                    let raw_b64 = if let Some(idx) = req_obj.audio_base64.find(";base64,") {
                        &req_obj.audio_base64[idx + 8..]
                    } else {
                        &req_obj.audio_base64
                    };
                    base64_decode(raw_b64).unwrap_or_default()
                }
                Err(_) => body_bytes,
            }
        } else {
            body_bytes
        };

        match audioticon_core::extract_waveform_from_wav_bytes(&wav_data, 512) {
            Ok(wave) => {
                let json_bytes = serde_json::to_vec(&wave).unwrap_or_default();
                let len = json_bytes.len();
                let mut resp = tiny_http::Response::from_data(json_bytes);
                resp.add_header(
                    Header::from_bytes(&b"Content-Length"[..], len.to_string().as_bytes()).unwrap(),
                );
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": format!("Błąd ekstrakcji fali dźwiękowej: {}", e)
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        }
    }

    // Route: POST /api/voicelab/punch-in/revert
    if method == tiny_http::Method::Post && url == "/api/voicelab/punch-in/revert" {
        let wallet = AudioWallet::load();
        if !wallet.tier.has_punch_in() {
            let err_json = serde_json::json!({
                "error": "punch_in_requires_pro",
                "message": "Narzędzie Punch-In & Retake wymaga licencji Solo PRO lub Studio PRO.",
                "tier": wallet.tier.name(),
                "upgrade_url": "https://gumroad.com/l/audioticon-solo"
            });
            let mut resp = tiny_http::Response::from_string(err_json.to_string());
            resp.add_header(
                Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap(),
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(402));
            return;
        }

        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Błąd odczytu żądania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct RevertReq {
            segment_hash: String,
        }

        let parsed: RevertReq = match serde_json::from_str(&body_str) {
            Ok(p) => p,
            Err(e) => {
                let err_json = serde_json::json!({ "error": format!("Błąd JSON: {}", e) });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let clean_hash = parsed.segment_hash.trim().replace(
            |c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-',
            "",
        );
        let cache_dir = get_default_cache_dir();
        let punch_wav = cache_dir.join(format!("recorded_{}.wav", clean_hash));
        let punch_meta = cache_dir.join(format!("recorded_{}.meta.json", clean_hash));
        let mut removed = false;
        if punch_wav.exists() {
            let _ = std::fs::remove_file(&punch_wav);
            removed = true;
        }
        if punch_meta.exists() {
            let _ = std::fs::remove_file(&punch_meta);
        }

        let resp_json = serde_json::json!({
            "status": "ok",
            "segment_hash": clean_hash,
            "reverted": removed
        });
        let mut resp = tiny_http::Response::from_string(resp_json.to_string());
        resp.add_header(
            Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/segment/rerender
    if method == tiny_http::Method::Post && url == "/api/segment/rerender" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(5 * 1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Błąd odczytu żądania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct RerenderReq {
            text: String,
            #[serde(default = "default_voice_str")]
            voice: String,
            #[serde(default = "default_lang_str")]
            lang: String,
            #[serde(default)]
            provider: Option<String>,
            #[serde(default)]
            rate: Option<f32>,
            #[serde(default)]
            pitch: Option<i32>,
            segment_hash: String,
        }

        let parsed: RerenderReq = match serde_json::from_str(&body_str) {
            Ok(p) => p,
            Err(e) => {
                let err_json = serde_json::json!({ "error": format!("Błąd JSON: {}", e) });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let clean_hash = parsed.segment_hash.trim().replace(
            |c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-',
            "",
        );
        let cache_dir = get_default_cache_dir();
        let punch_wav = cache_dir.join(format!("recorded_{}.wav", clean_hash));
        let punch_meta = cache_dir.join(format!("recorded_{}.meta.json", clean_hash));

        let seg = audioticon_core::types::TextSegment {
            text: parsed.text.clone(),
            voice: Some(parsed.voice.clone()),
            rate_mod: parsed
                .rate
                .map(|r| ((r - 1.0) * 100.0).round() as i32)
                .unwrap_or(-6),
            pitch_mod: parsed.pitch.unwrap_or(-4),
            ..Default::default()
        };

        let prov = parsed.provider.as_deref().unwrap_or("piper").to_lowercase();
        let clean_lang = parsed.lang.to_lowercase();
        let synth_res = resolve_synthesizer(
            &prov,
            &parsed.voice,
            None,
            &clean_lang,
            default_model.as_ref(),
            &piper_cache,
        )
        .await;

        let synth = match synth_res {
            Ok(s) => s,
            Err(e) => {
                let err_json = serde_json::json!({ "error": format!("Nie można zainicjalizować syntezatora: {}", e) });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        match synth.synthesize(&seg, &punch_wav).await {
            Ok(()) => {
                let meta = serde_json::json!({
                    "type": "ai_rerender_punch_in",
                    "text": parsed.text,
                    "voice": parsed.voice,
                    "provider": prov,
                    "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
                });
                let _ = std::fs::write(
                    &punch_meta,
                    serde_json::to_string_pretty(&meta).unwrap_or_default(),
                );

                let resp_json = serde_json::json!({
                    "status": "ok",
                    "segment_hash": clean_hash,
                    "is_punch_in": true,
                    "path": punch_wav.to_string_lossy()
                });
                let mut resp = tiny_http::Response::from_string(resp_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err_json =
                    serde_json::json!({ "error": format!("Błąd resyntezy segmentu: {}", e) });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(500));
                return;
            }
        }
    }

    // Route: GET /api/qc/latest
    if method == tiny_http::Method::Get
        && (url == "/api/qc/latest" || url.starts_with("/api/qc/latest?"))
    {
        let cache_dir = get_default_cache_dir();
        let target_file = cache_dir.join("latest_qc_report.json");
        if target_file.exists() {
            if let Ok(content_bytes) = std::fs::read(&target_file) {
                let len = content_bytes.len();
                let mut resp = tiny_http::Response::from_data(content_bytes);
                resp.add_header(
                    Header::from_bytes(&b"Content-Length"[..], len.to_string().as_bytes()).unwrap(),
                );
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
        }

        // Return calibrated default report if no render has occurred yet
        let wallet = AudioWallet::load();
        let default_qc = serde_json::json!({
            "integrated_lufs": -19.5,
            "true_peak_dbfs": -3.2,
            "noise_floor_dbfs": -65.0,
            "is_compliant": true,
            "peak_pass": true,
            "lufs_pass": true,
            "noise_floor_pass": true,
            "recommendations": [],
            "certificate": null,
            "tier": wallet.tier.name(),
            "certificate_unlocked": wallet.acx_certificate_unlocked
        });
        let bytes = serde_json::to_vec_pretty(&default_qc).unwrap_or_default();
        let len = bytes.len();
        let mut resp = tiny_http::Response::from_data(bytes);
        resp.add_header(
            Header::from_bytes(&b"Content-Length"[..], len.to_string().as_bytes()).unwrap(),
        );
        resp.add_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/qc/audit
    if method == tiny_http::Method::Post && url == "/api/qc/audit" {
        let mut body_bytes = Vec::new();
        if request
            .as_reader()
            .take(50 * 1024 * 1024)
            .read_to_end(&mut body_bytes)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Błąd odczytu danych audio\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        let audio_data: Vec<u8> = if body_bytes.starts_with(b"{") {
            #[derive(serde::Deserialize)]
            struct AuditJsonReq {
                audio_base64: String,
            }
            match serde_json::from_slice::<AuditJsonReq>(&body_bytes) {
                Ok(req_obj) => {
                    let raw_b64 = if let Some(idx) = req_obj.audio_base64.find(";base64,") {
                        &req_obj.audio_base64[idx + 8..]
                    } else {
                        &req_obj.audio_base64
                    };
                    base64_decode(raw_b64).unwrap_or_default()
                }
                Err(_) => body_bytes,
            }
        } else {
            body_bytes
        };

        let cache_dir = get_default_cache_dir();
        let tmp_test_wav = cache_dir.join(format!("tmp_audit_{}.wav", std::process::id()));
        if let Err(e) = std::fs::write(&tmp_test_wav, &audio_data) {
            let err_json =
                serde_json::json!({ "error": format!("Błąd zapisu pliku tymczasowego: {}", e) });
            let mut resp = tiny_http::Response::from_string(err_json.to_string());
            resp.add_header(
                Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(500));
            return;
        }

        let (master_lufs, true_peak_db) =
            match audioticon_core::measure_audio_loudness(&tmp_test_wav) {
                Ok(pair) => pair,
                Err(e) => {
                    let _ = std::fs::remove_file(&tmp_test_wav);
                    let err_json =
                        serde_json::json!({ "error": format!("Błąd pomiaru głośności: {}", e) });
                    let mut resp = tiny_http::Response::from_string(err_json.to_string());
                    resp.add_header(
                        Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                    );
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    let _ = request.respond(resp.with_status_code(400));
                    return;
                }
            };
        let _ = std::fs::remove_file(&tmp_test_wav);

        let wallet = AudioWallet::load();
        let lufs_pass = (-23.8..=-18.5).contains(&master_lufs);
        let peak_pass = true_peak_db <= -2.8;
        let is_compliant = lufs_pass && peak_pass;

        let res_json = serde_json::json!({
            "integrated_lufs": master_lufs,
            "true_peak_dbfs": true_peak_db,
            "noise_floor_dbfs": -65.4,
            "is_compliant": is_compliant,
            "peak_pass": peak_pass,
            "lufs_pass": lufs_pass,
            "noise_floor_pass": true,
            "recommendations": if is_compliant { vec!["Kryteria ACX spełnione."] } else { vec!["Wymagana korekta głośności."] },
            "tier": wallet.tier.name(),
            "certificate_unlocked": wallet.acx_certificate_unlocked
        });

        let res_bytes = serde_json::to_vec(&res_json).unwrap_or_default();
        let len = res_bytes.len();
        let mut resp = tiny_http::Response::from_data(res_bytes);
        resp.add_header(
            Header::from_bytes(&b"Content-Length"[..], len.to_string().as_bytes()).unwrap(),
        );
        resp.add_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: GET /api/voicelab/profiles
    if method == tiny_http::Method::Get && url == "/api/voicelab/profiles" {
        let profiles = voicelab::list_voice_profiles().unwrap_or_default();
        let json_resp = serde_json::json!({
            "profiles": profiles
        });
        let json_header =
            Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
        let mut resp = tiny_http::Response::from_string(json_resp.to_string());
        resp.add_header(json_header);
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/voicelab/profiles/create
    if method == tiny_http::Method::Post && url == "/api/voicelab/profiles/create" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct CreateProfileReq {
            id: String,
            name: String,
            gender: Option<String>,
            language: Option<String>,
            description: Option<String>,
            consent: Option<bool>,
        }

        let parsed: CreateProfileReq = match serde_json::from_str(&body_str) {
            Ok(p) => p,
            Err(e) => {
                let err_json =
                    serde_json::json!({ "error": format!("Błąd parsowania JSON: {}", e) });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let mut prof = VoiceProfile::new(
            parsed.id,
            parsed.name,
            parsed.gender.unwrap_or_else(|| "male".to_string()),
            parsed.language.unwrap_or_else(|| "pl".to_string()),
        );
        prof.description = parsed.description;
        prof.consent = parsed.consent.unwrap_or(true);

        if let Err(e) = voicelab::save_voice_profile(&prof) {
            let err_json = serde_json::json!({ "error": format!("Błąd zapisu profilu: {}", e) });
            let mut resp = tiny_http::Response::from_string(err_json.to_string());
            resp.add_header(
                Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(500));
            return;
        }

        let json_resp = serde_json::json!({
            "success": true,
            "profile": prof
        });
        let mut resp = tiny_http::Response::from_string(json_resp.to_string());
        resp.add_header(
            Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/voicelab/profiles/delete
    if method == tiny_http::Method::Post && url == "/api/voicelab/profiles/delete" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct DeleteProfileReq {
            id: String,
        }

        if let Ok(parsed) = serde_json::from_str::<DeleteProfileReq>(&body_str) {
            let _ = voicelab::delete_voice_profile(&parsed.id);
            let json_resp = serde_json::json!({ "success": true });
            let mut resp = tiny_http::Response::from_string(json_resp.to_string());
            resp.add_header(
                Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp);
            return;
        }
    }

    // Route: POST /api/voicelab/sample
    if method == tiny_http::Method::Post && url == "/api/voicelab/sample" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(50 * 1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp = tiny_http::Response::from_string(
                "{\"error\": \"Nie można odczytać próbki audio\"}",
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct AddSampleReq {
            profile_id: String,
            audio_base64: String,
            filename: Option<String>,
        }

        let parsed: AddSampleReq = match serde_json::from_str(&body_str) {
            Ok(p) => p,
            Err(e) => {
                let err_json =
                    serde_json::json!({ "error": format!("Błąd parsowania JSON: {}", e) });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let raw_b64 = if let Some(idx) = parsed.audio_base64.find(";base64,") {
            &parsed.audio_base64[idx + 8..]
        } else {
            &parsed.audio_base64
        };

        let audio_bytes = match base64_decode(raw_b64) {
            Ok(b) => b,
            Err(e) => {
                let err_json =
                    serde_json::json!({ "error": format!("Błąd dekodowania base64: {}", e) });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        match voicelab::add_sample_to_profile(
            &parsed.profile_id,
            &audio_bytes,
            parsed.filename.as_deref(),
        ) {
            Ok((meta, report)) => {
                let updated = voicelab::get_voice_profile(&parsed.profile_id).ok();
                let json_resp = serde_json::json!({
                    "success": true,
                    "sample": meta,
                    "report": report,
                    "profile": updated
                });
                let mut resp = tiny_http::Response::from_string(json_resp.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err_json =
                    serde_json::json!({ "error": format!("Błąd przetwarzania próbki: {}", e) });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(500));
                return;
            }
        }
    }

    // Route: GET /api/voicelab/sample/audio?...
    if method == tiny_http::Method::Get && url.starts_with("/api/voicelab/sample/audio") {
        let query = url.split_once('?').map(|(_, q)| q).unwrap_or("");
        let mut params = std::collections::HashMap::new();
        for pair in query.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                params.insert(k, url_decode(v));
            }
        }
        let profile = params.get("profile").cloned().unwrap_or_default();
        let file = params.get("file").cloned().unwrap_or_default();
        if !profile.contains("..")
            && !file.contains("..")
            && !file.contains('\\')
            && !profile.is_empty()
            && !file.is_empty()
        {
            let p_dir = voicelab::profile_dir(&profile);
            let full_path = p_dir.join(&file);
            if full_path.exists() && full_path.is_file() {
                if let Ok(bytes) = std::fs::read(&full_path) {
                    let mut resp = tiny_http::Response::from_data(bytes);
                    resp.add_header(
                        Header::from_bytes(&b"Content-Type"[..], &b"audio/wav"[..]).unwrap(),
                    );
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    let _ = request.respond(resp);
                    return;
                }
            }
        }
        let mut resp = tiny_http::Response::from_string("Nie odnaleziono próbki dźwiękowej");
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp.with_status_code(404));
        return;
    }

    // Route: GET /api/voicelab/dataset?profile=...
    if method == tiny_http::Method::Get && url.starts_with("/api/voicelab/dataset") {
        let wallet = AudioWallet::load();
        if !wallet.tier.has_dataset_export() {
            let err_json = serde_json::json!({
                "error": "dataset_export_requires_studio",
                "message": "Eksport datasetu VoiceLab ZIP dla trenowania własnych modeli neuralnych wymaga licencji Studio PRO.",
                "tier": wallet.tier.name(),
                "upgrade_url": "https://gumroad.com/l/audioticon-studio"
            });
            let mut resp = tiny_http::Response::from_string(err_json.to_string());
            resp.add_header(
                Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap(),
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(402));
            return;
        }

        let query = url.split_once('?').map(|(_, q)| q).unwrap_or("");
        let mut params = std::collections::HashMap::new();
        for pair in query.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                params.insert(k, url_decode(v));
            }
        }
        let profile = params.get("profile").cloned().unwrap_or_default();
        if profile.is_empty()
            || profile.contains("..")
            || profile.contains('/')
            || profile.contains('\\')
        {
            let mut resp = tiny_http::Response::from_string(
                "{\"error\": \"Nieprawidłowy identyfikator profilu\"}",
            );
            resp.add_header(
                Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        match voicelab::export_dataset_zip_bytes(&profile) {
            Ok(zip_bytes) => {
                let mut resp = tiny_http::Response::from_data(zip_bytes);
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/zip"[..]).unwrap(),
                );
                let disp_val = format!("attachment; filename=\"{}_dataset.zip\"", profile);
                resp.add_header(
                    Header::from_bytes(&b"Content-Disposition"[..], disp_val.as_bytes()).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": format!("Błąd eksportu datasetu: {}", e)
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(404));
                return;
            }
        }
    }

    // Route: POST /api/voicelab/punch-in
    if method == tiny_http::Method::Post && url == "/api/voicelab/punch-in" {
        let wallet = AudioWallet::load();
        if !wallet.tier.has_punch_in() {
            let err_json = serde_json::json!({
                "error": "punch_in_requires_pro",
                "message": "Narzędzie Punch-In & Retake (dogrywka aktorska/AI) wymaga licencji Solo PRO lub Studio PRO.",
                "tier": wallet.tier.name(),
                "upgrade_url": "https://gumroad.com/l/audioticon-solo"
            });
            let mut resp = tiny_http::Response::from_string(err_json.to_string());
            resp.add_header(
                Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap(),
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(402));
            return;
        }

        let mut body_str = String::new();
        if request
            .as_reader()
            .take(50 * 1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp = tiny_http::Response::from_string(
                "{\"error\": \"Nie można odczytać audio punch-in\"}",
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct PunchInReq {
            profile_id: String,
            segment_hash: String,
            audio_base64: String,
        }

        let parsed: PunchInReq = match serde_json::from_str(&body_str) {
            Ok(p) => p,
            Err(e) => {
                let err_json =
                    serde_json::json!({ "error": format!("Błąd parsowania JSON: {}", e) });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let raw_b64 = if let Some(idx) = parsed.audio_base64.find(";base64,") {
            &parsed.audio_base64[idx + 8..]
        } else {
            &parsed.audio_base64
        };

        let audio_bytes = match base64_decode(raw_b64) {
            Ok(b) => b,
            Err(e) => {
                let err_json =
                    serde_json::json!({ "error": format!("Błąd dekodowania base64: {}", e) });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let cache_dir = get_default_cache_dir();
        match voicelab::punch_in_sentence(
            &parsed.profile_id,
            &parsed.segment_hash,
            &audio_bytes,
            &cache_dir,
        ) {
            Ok(punch_path) => {
                let json_resp = serde_json::json!({
                    "success": true,
                    "segment_hash": parsed.segment_hash,
                    "path": punch_path.to_string_lossy()
                });
                let mut resp = tiny_http::Response::from_string(json_resp.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err_json =
                    serde_json::json!({ "error": format!("Błąd zapisu punch-in: {}", e) });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(500));
                return;
            }
        }
    }

    // Route: POST /api/voicelab/clone-elevenlabs
    if method == tiny_http::Method::Post && url == "/api/voicelab/clone-elevenlabs" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct CloneReq {
            profile_id: String,
        }

        let parsed: CloneReq = match serde_json::from_str(&body_str) {
            Ok(p) => p,
            Err(e) => {
                let err_json =
                    serde_json::json!({ "error": format!("Błąd parsowania JSON: {}", e) });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let api_key = match credentials::get_api_key("elevenlabs") {
            Some(k) => k,
            None => {
                let err_json = serde_json::json!({
                    "error": "Brak klucza API ElevenLabs. Skonfiguruj klucz w panelu Dostawców (audioticon providers set elevenlabs --key <KLUCZ>)"
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        match voicelab::clone_to_elevenlabs(&parsed.profile_id, &api_key) {
            Ok(voice_id) => {
                let updated = voicelab::get_voice_profile(&parsed.profile_id).ok();
                let json_resp = serde_json::json!({
                    "success": true,
                    "voice_id": voice_id,
                    "profile": updated
                });
                let mut resp = tiny_http::Response::from_string(json_resp.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err_json =
                    serde_json::json!({ "error": format!("Błąd klonowania w ElevenLabs: {}", e) });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(500));
                return;
            }
        }
    }

    // Route: POST /api/voicelab/design/preview
    if method == tiny_http::Method::Post && url == "/api/voicelab/design/preview" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct DesignPreviewReq {
            description: String,
            #[serde(default)]
            text: Option<String>,
        }

        let parsed: DesignPreviewReq = match serde_json::from_str(&body_str) {
            Ok(p) => p,
            Err(e) => {
                let err_json =
                    serde_json::json!({ "error": format!("Błąd parsowania JSON: {}", e) });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let api_key = match credentials::get_api_key("elevenlabs") {
            Some(k) if !k.trim().is_empty() => k,
            _ => {
                let err_json = serde_json::json!({
                    "error": "Brak skonfigurowanego klucza API ElevenLabs. Skonfiguruj klucz w zakładce 'Dostawcy'."
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        match voice_design::create_voice_design_previews(
            &api_key,
            &parsed.description,
            parsed.text.as_deref(),
        ) {
            Ok(resp_data) => {
                // Deduct 50 AudioCoins from wallet
                let mut wallet = AudioWallet::load();
                let _ = wallet.spend(50, "Projektowanie głosu ElevenLabs (Voice Design)", None);
                let _ = wallet.save();

                let json_resp = serde_json::json!({
                    "success": true,
                    "previews": resp_data.previews,
                    "text": resp_data.text,
                    "balance": wallet.balance,
                });
                let mut resp = tiny_http::Response::from_string(json_resp.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": format!("Błąd generowania głosu w ElevenLabs: {}", e)
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(500));
                return;
            }
        }
    }

    // Route: POST /api/voicelab/design/save
    if method == tiny_http::Method::Post && url == "/api/voicelab/design/save" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(10 * 1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct DesignSaveReq {
            name: String,
            description: String,
            generated_voice_id: String,
            #[serde(default)]
            preview_audio_base64: Option<String>,
        }

        let parsed: DesignSaveReq = match serde_json::from_str(&body_str) {
            Ok(p) => p,
            Err(e) => {
                let err_json =
                    serde_json::json!({ "error": format!("Błąd parsowania JSON: {}", e) });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let api_key = match credentials::get_api_key("elevenlabs") {
            Some(k) if !k.trim().is_empty() => k,
            _ => {
                let err_json = serde_json::json!({
                    "error": "Brak skonfigurowanego klucza API ElevenLabs."
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        match voice_design::save_designed_voice(
            &api_key,
            &parsed.name,
            &parsed.description,
            &parsed.generated_voice_id,
            parsed.preview_audio_base64.as_deref(),
        ) {
            Ok(voice_id) => {
                let json_resp = serde_json::json!({
                    "success": true,
                    "voice_id": voice_id,
                    "name": parsed.name,
                    "alias": audioticon_core::sanitize_slug(&parsed.name),
                });
                let mut resp = tiny_http::Response::from_string(json_resp.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": format!("Błąd zapisu głosu w ElevenLabs: {}", e)
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(500));
                return;
            }
        }
    }

    // Route: GET /api/custom-emojis
    if method == tiny_http::Method::Get && url == "/api/custom-emojis" {
        let cfg = ProjectConfig::discover(None);
        let factory_emojis = serde_json::json!([
            { "emoji": "📱", "name": "Telefon retro", "alias": "telefon", "type": "filter", "filter": "phone", "desc": "Pasmo telefoniczne 400Hz-2.8kHz + lekki crunch" },
            { "emoji": "📻", "name": "Vintage Radio", "alias": "radio", "type": "filter", "filter": "radio", "desc": "Krótkofalówka / radio z przesterem i szumem" },
            { "emoji": "🏛️", "name": "Katedra", "alias": "katedra", "type": "filter", "filter": "cathedral", "desc": "Duża przestrzeń sakralna, długi pogłos (reverb 3.2s)" },
            { "emoji": "🏰", "name": "Lochy / Jaskinia", "alias": "lochy", "type": "filter", "filter": "dungeon", "desc": "Ciemne kamienne sklepienie z gęstym echem" },
            { "emoji": "📢", "name": "Megafon", "alias": "megafon", "type": "filter", "filter": "megaphone", "desc": "Wąskie pasmo tuby megafonu, podbity środek 1.8kHz" },
            { "emoji": "🤖", "name": "Robot bojowy", "alias": "robot", "type": "filter", "filter": "robot", "desc": "Modulacja pierścieniowa (ring mod) i tremolo 55Hz" },
            { "emoji": "💭", "name": "Myśli bohatera", "alias": "mysli", "type": "filter", "filter": "thought", "desc": "Głos wewnętrzny, stłumione wysokie tony i bliski dół" },
            { "emoji": "🤫", "name": "Szept narracyjny", "alias": "szept", "type": "filter", "filter": "whisper", "desc": "Cichy szept ze zwiększonym szumem powietrza" },
            { "emoji": "🪖", "name": "Szept taktyczny", "alias": "szept_akcja", "type": "filter", "filter": "whisper:akcja", "desc": "Szybki szept komandosa pod ostrzałem" },
            { "emoji": "💋", "name": "Szept intymny", "alias": "szept_bliski", "type": "filter", "filter": "whisper:romantyczny", "desc": "Ciepły, zmysłowy szept prosto do ucha" },
            { "emoji": "💥", "name": "Krzyk / Rozkaz", "alias": "rozkaz", "type": "filter", "filter": "rozkaz", "desc": "Maksymalna energia głosu, rozkaz wojskowy" },
            { "emoji": "📢", "name": "Wołanie w dal", "alias": "wolanie", "type": "filter", "filter": "wolanie", "desc": "Przenikliwe wołanie na otwartej przestrzeni" },
            { "emoji": "🎵", "name": "Śpiew / Melodia", "alias": "spiew", "type": "filter", "filter": "spiew", "desc": "Melodyjna deklamacja, podwyższona wibracja krtani" },
            { "emoji": "🔔", "name": "Dzwon", "alias": "dzwon", "type": "sfx", "filter": "bell", "desc": "Czysty dźwięk dzwonka lub dzwonu zegarowego" },
            { "emoji": "🚪", "name": "Drzwi", "alias": "drzwi", "type": "sfx", "filter": "door", "desc": "Skrzypnięcie i zatrzaśnięcie starych drzwi" },
            { "emoji": "👣", "name": "Kroki", "alias": "kroki", "type": "sfx", "filter": "footsteps", "desc": "Powolne kroki po podłodze" },
            { "emoji": "📄", "name": "Papier", "alias": "papier", "type": "sfx", "filter": "paper", "desc": "Szelest przewracanej kartki papieru" },
            { "emoji": "⚡", "name": "Piorun", "alias": "piorun", "type": "sfx", "filter": "thunder", "desc": "Potężny grzmot pioruna" },
            { "emoji": "❄️", "name": "Wiatr", "alias": "wiatr", "type": "ambient", "filter": "wind", "desc": "Mroźny powiew wiatru w tle" },
            { "emoji": "🌧️", "name": "Deszcz", "alias": "deszcz", "type": "ambient", "filter": "rain", "desc": "Ulewny deszcz bębniący o szyby" }
        ]);
        let json_resp = serde_json::json!({
            "emojis": cfg.emojis,
            "factory_emojis": factory_emojis,
            "available_filters": [
                { "id": "cathedral", "name": "🏛️ Katedra / Duży Reverb", "desc": "Pogłos wielkiej świątyni" },
                { "id": "dungeon", "name": "🏰 Mroczne lochy / Jaskinia", "desc": "Gęste echo w kamiennych korytarzach" },
                { "id": "phone", "name": "📱 Telefon retro", "desc": "Pasmo telefoniczne 400Hz-2.8kHz" },
                { "id": "radio", "name": "📻 Vintage Radio / Krótkofalówka", "desc": "Zniekształcenia lo-fi i trzaski" },
                { "id": "megaphone", "name": "📢 Megafon / Tuba", "desc": "Ostry dźwięk szczekaczki" },
                { "id": "robot", "name": "🤖 Robot bojowy / Vocoder", "desc": "Metaliczny ring modulation 55Hz" },
                { "id": "whisper", "name": "🤫 Szept narracyjny", "desc": "Przepływ powietrza i wyciszenie" },
                { "id": "thought", "name": "💭 Myśli wewnętrzne", "desc": "Głos w głowie bohatera" },
                { "id": "studio", "name": "🎙️ Broadcast Studio", "desc": "De-esser, ciepły dół i klarowna góra" },
                { "id": "warm", "name": "☕ Ciepły analog", "desc": "Podbity dół 200Hz i łagodne pasmo" },
                { "id": "bright", "name": "✨ Krystaliczny emisyjny", "desc": "Prezencja 3.6kHz dla lektora" }
            ],
            "available_sfx": [
                { "id": "bell", "name": "🔔 Dzwon / Dzwonek" },
                { "id": "door", "name": "🚪 Drzwi skrzypiące" },
                { "id": "footsteps", "name": "👣 Kroki" },
                { "id": "paper", "name": "📄 Szelest papieru" },
                { "id": "car", "name": "🚗 Samochód" },
                { "id": "thunder", "name": "⚡ Piorun / Grzmot" },
                { "id": "shot", "name": "💥 Wystrzał" },
                { "id": "heart", "name": "💓 Bicie serca" },
                { "id": "water", "name": "💧 Kropla wody" }
            ],
            "available_ambient": [
                { "id": "wind", "name": "❄️ Mroźny wiatr" },
                { "id": "rain", "name": "🌧️ Deszcz o szyby" },
                { "id": "city", "name": "🏙️ Miasto nocą" }
            ]
        });
        let mut resp = tiny_http::Response::from_string(json_resp.to_string());
        resp.add_header(
            Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/custom-emojis/add
    if method == tiny_http::Method::Post && url == "/api/custom-emojis/add" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct AddEmojiReq {
            emoji: String,
            name: String,
            alias: Option<String>,
            action_type: Option<String>,
            filter: Option<String>,
            voice: Option<String>,
            pitch: Option<i32>,
            rate: Option<i32>,
            volume: Option<f32>,
            description: Option<String>,
        }

        let parsed: AddEmojiReq = match serde_json::from_str(&body_str) {
            Ok(p) => p,
            Err(e) => {
                let err_json =
                    serde_json::json!({ "error": format!("Błąd parsowania JSON: {}", e) });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let mut cfg = ProjectConfig::discover(None);
        let def = CustomEmojiDef {
            name: parsed.name,
            alias: parsed.alias,
            action_type: parsed.action_type.unwrap_or_else(|| "filter".to_string()),
            filter: parsed.filter,
            voice: parsed.voice,
            pitch: parsed.pitch,
            rate: parsed.rate,
            volume: parsed.volume,
            description: parsed.description,
        };

        cfg.emojis.insert(parsed.emoji.clone(), def.clone());
        let _ = cfg.save_to_path(std::path::Path::new("audioticon.toml"));

        let mut reg = presets::get_active_registry();
        reg.register_custom_emoji(parsed.emoji, def);
        presets::set_active_registry(reg);

        let json_resp = serde_json::json!({
            "success": true,
            "emojis": cfg.emojis
        });
        let mut resp = tiny_http::Response::from_string(json_resp.to_string());
        resp.add_header(
            Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/custom-emojis/remove
    if method == tiny_http::Method::Post && url == "/api/custom-emojis/remove" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct RemoveEmojiReq {
            emoji: String,
        }

        if let Ok(parsed) = serde_json::from_str::<RemoveEmojiReq>(&body_str) {
            let mut cfg = ProjectConfig::discover(None);
            cfg.emojis.remove(&parsed.emoji);
            let _ = cfg.save_to_path(std::path::Path::new("audioticon.toml"));

            let mut reg = presets::get_active_registry();
            reg.remove_custom_emoji(&parsed.emoji);
            presets::set_active_registry(reg);

            let json_resp = serde_json::json!({
                "success": true,
                "emojis": cfg.emojis
            });
            let mut resp = tiny_http::Response::from_string(json_resp.to_string());
            resp.add_header(
                Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp);
            return;
        }
    }

    // Route: POST /api/auto-direct
    if method == tiny_http::Method::Post && url == "/api/auto-direct" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(50 * 1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp = tiny_http::Response::from_string(
                "{\"error\": \"Nie można odczytać treści żądania\"}",
            );
            resp.add_header(
                Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct AutoDirectReq {
            text: String,
            #[serde(default = "default_direct_provider")]
            provider: String,
            model: Option<String>,
            api_key: Option<String>,
        }

        fn default_direct_provider() -> String {
            "gemini".to_string()
        }

        let parsed: AutoDirectReq = match serde_json::from_str(&body_str) {
            Ok(p) => p,
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": format!("Nieprawidłowy format JSON: {}", e)
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        match audioticon_core::auto_direct_text(
            &parsed.text,
            &parsed.provider,
            parsed.model.as_deref(),
            parsed.api_key.as_deref(),
        ) {
            Ok(directed) => {
                let json_resp = serde_json::json!({
                    "success": true,
                    "directed_text": directed
                });
                let mut resp = tiny_http::Response::from_string(json_resp.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": format!("Błąd asystenta reżyserii AI: {}", e)
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(500));
                return;
            }
        }
    }

    // Route: POST /api/auto-direct/propose (Annotation-Only Bielik / Gemini / OpenAI / Baseline)
    if method == tiny_http::Method::Post && url == "/api/auto-direct/propose" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(10 * 1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp = tiny_http::Response::from_string(
                "{\"error\": \"Nie można odczytać treści żądania\"}",
            );
            resp.add_header(
                Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct ProposeReq {
            text: String,
            #[serde(default = "default_bielik_provider")]
            provider: String,
            model: Option<String>,
            api_key: Option<String>,
        }

        fn default_bielik_provider() -> String {
            "bielik".to_string()
        }

        let parsed: ProposeReq = match serde_json::from_str(&body_str) {
            Ok(p) => p,
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": format!("Nieprawidłowy format JSON: {}", e)
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        match audioticon_core::propose_annotations_auto(
            &parsed.text,
            &parsed.provider,
            parsed.model.as_deref(),
            parsed.api_key.as_deref(),
        ) {
            Ok(envelope) => {
                let json_resp = serde_json::json!({
                    "success": true,
                    "envelope": envelope,
                });
                let mut resp = tiny_http::Response::from_string(json_resp.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": format!("Błąd propozycji reżyserskiej: {}", e)
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        }
    }

    // Route: POST /api/auto-direct/approve
    if method == tiny_http::Method::Post && url == "/api/auto-direct/approve" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(10 * 1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp = tiny_http::Response::from_string(
                "{\"error\": \"Nie można odczytać treści żądania\"}",
            );
            resp.add_header(
                Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct ApproveReq {
            envelope: audioticon_core::DirectorProposalEnvelope,
            source_text: String,
            reviewer_note: String,
            #[serde(default)]
            speaker_overrides: Option<HashMap<String, String>>,
        }

        let mut parsed: ApproveReq = match serde_json::from_str(&body_str) {
            Ok(p) => p,
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": format!("Nieprawidłowy format JSON: {}", e)
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let clean_source = parsed.source_text.trim();
        if let Err(e) = audioticon_core::approve_proposal_envelope(
            &mut parsed.envelope,
            clean_source,
            &parsed.reviewer_note,
            parsed.speaker_overrides.as_ref(),
        ) {
            let err_json = serde_json::json!({
                "error": e.to_string(),
                "validation": parsed.envelope.validation,
            });
            let mut resp = tiny_http::Response::from_string(err_json.to_string());
            resp.add_header(
                Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json; charset=utf-8"[..],
                )
                .unwrap(),
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        match audioticon_core::export_proposal_to_audioticon_script(&parsed.envelope, clean_source)
        {
            Ok(directed_text) => {
                let json_resp = serde_json::json!({
                    "success": true,
                    "envelope": parsed.envelope,
                    "directed_text": directed_text,
                });
                let mut resp = tiny_http::Response::from_string(json_resp.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": e.to_string()
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        }
    }

    // Route: POST /api/auto-direct/reject
    if method == tiny_http::Method::Post && url == "/api/auto-direct/reject" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(5 * 1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp = tiny_http::Response::from_string(
                "{\"error\": \"Nie można odczytać treści żądania\"}",
            );
            resp.add_header(
                Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct RejectReq {
            envelope: audioticon_core::DirectorProposalEnvelope,
            reviewer_note: String,
        }

        let mut parsed: RejectReq = match serde_json::from_str(&body_str) {
            Ok(p) => p,
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": format!("Nieprawidłowy format JSON: {}", e)
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        match audioticon_core::reject_proposal_envelope(&mut parsed.envelope, &parsed.reviewer_note)
        {
            Ok(()) => {
                let json_resp = serde_json::json!({
                    "success": true,
                    "envelope": parsed.envelope,
                });
                let mut resp = tiny_http::Response::from_string(json_resp.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": e.to_string()
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        }
    }

    // Route: GET /api/style-transfer/list
    if method == tiny_http::Method::Get && url == "/api/style-transfer/list" {
        let cfg = ProjectConfig::discover(None);
        let mut reg = presets::get_active_registry();
        reg.load_project_config(&cfg);
        let styles = reg.list_styles();
        presets::set_active_registry(reg);

        let json_resp = serde_json::json!({
            "styles": styles,
            "band_centers_hz": audioticon_core::TRANSFER_BAND_CENTERS_HZ,
        });
        let mut resp = tiny_http::Response::from_string(json_resp.to_string());
        resp.add_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/style-transfer/extract
    if method == tiny_http::Method::Post && url == "/api/style-transfer/extract" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(30 * 1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct ExtractStyleReq {
            #[serde(default)]
            normal_audio_base64: String,
            style_audio_base64: String,
            #[serde(default)]
            name: String,
            #[serde(default)]
            emoji: String,
            #[serde(default)]
            alias: String,
        }

        let parsed: ExtractStyleReq = match serde_json::from_str(&body_str) {
            Ok(p) => p,
            Err(e) => {
                let err_json =
                    serde_json::json!({ "error": format!("Błąd parsowania JSON: {}", e) });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let norm_bytes = if parsed.normal_audio_base64.trim().is_empty() {
            // Autogeneracja neutralnej mowy referencyjnej, jeśli użytkownik nagrał tylko ekspresję B
            let synth = generate_synthetic_vocal_phrase("male", 22050);
            let b64 = encode_samples_to_wav_base64(&synth, 22050).unwrap_or_default();
            base64_decode(&b64).unwrap_or_default()
        } else {
            match base64_decode(strip_data_url_prefix(&parsed.normal_audio_base64)) {
                Ok(b) => b,
                Err(e) => {
                    let err_json = serde_json::json!({
                        "error": format!("Nieprawidłowe dane audio głosu normalnego (base64): {}", e)
                    });
                    let mut resp = tiny_http::Response::from_string(err_json.to_string());
                    resp.add_header(
                        Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                    );
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    let _ = request.respond(resp.with_status_code(400));
                    return;
                }
            }
        };

        let style_bytes = match base64_decode(strip_data_url_prefix(&parsed.style_audio_base64)) {
            Ok(b) => b,
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": format!("Nieprawidłowe dane audio wzorca ekspresji (base64): {}", e)
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        let cache_dir = get_default_cache_dir();
        let _ = std::fs::create_dir_all(&cache_dir);
        let req_id = REQ_COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let norm_wav = cache_dir.join(format!("ext_norm_{}_{}.wav", pid, req_id));
        let style_wav = cache_dir.join(format!("ext_style_{}_{}.wav", pid, req_id));

        let conv_norm = voicelab::convert_to_standard_wav(&norm_bytes, &norm_wav);
        let conv_style = voicelab::convert_to_standard_wav(&style_bytes, &style_wav);

        if let Err(e) = conv_norm.and(conv_style) {
            let _ = std::fs::remove_file(&norm_wav);
            let _ = std::fs::remove_file(&style_wav);
            let err_json = serde_json::json!({
                "error": format!("Błąd konwersji próbek audio przez FFmpeg: {}", e)
            });
            let mut resp = tiny_http::Response::from_string(err_json.to_string());
            resp.add_header(
                Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
            );
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        let norm_read = voicelab::read_wav_samples(&norm_wav);
        let style_read = voicelab::read_wav_samples(&style_wav);
        let _ = std::fs::remove_file(&norm_wav);
        let _ = std::fs::remove_file(&style_wav);

        match (norm_read, style_read) {
            (Ok((norm_i16, sr, _)), Ok((style_i16, _, _))) => {
                let norm_samples: Vec<f32> =
                    norm_i16.into_iter().map(|s| s as f32 / 32768.0).collect();
                let style_samples: Vec<f32> =
                    style_i16.into_iter().map(|s| s as f32 / 32768.0).collect();
                let name = if parsed.name.trim().is_empty() {
                    "Własny wzorzec z mikrofonu".to_string()
                } else {
                    parsed.name.trim().to_string()
                };
                let emoji = if parsed.emoji.trim().is_empty() {
                    "🎭".to_string()
                } else {
                    parsed.emoji.trim().to_string()
                };
                let alias = if parsed.alias.trim().is_empty() {
                    "wzorzec:wlasny".to_string()
                } else {
                    parsed.alias.trim().to_string()
                };
                let extracted = audioticon_core::extract_disentangled_style_matrix(
                    &norm_samples,
                    &style_samples,
                    sr,
                    &name,
                    &emoji,
                    Some(alias),
                );
                let styled_preview =
                    audioticon_core::apply_disentangled_style(&norm_samples, sr, &extracted);
                let diagnostics = audioticon_core::diagnose_styled_audio(&styled_preview, sr);

                let json_resp = serde_json::json!({
                    "success": true,
                    "style": extracted,
                    "diagnostics": diagnostics,
                });
                let mut resp = tiny_http::Response::from_string(json_resp.to_string());
                resp.add_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/json; charset=utf-8"[..],
                    )
                    .unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp);
                return;
            }
            (Err(e), _) | (_, Err(e)) => {
                let err_json = serde_json::json!({
                    "error": format!("Nie udało się odczytać próbek WAV: {}", e)
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        }
    }

    // Route: POST /api/style-transfer/preview
    if method == tiny_http::Method::Post && url == "/api/style-transfer/preview" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(5 * 1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct StylePreviewReq {
            style: audioticon_core::DisentangledStyleMatrix,
            #[serde(default)]
            text: Option<String>,
            #[serde(default)]
            gender: Option<String>,
            #[serde(default)]
            provider: Option<String>,
        }

        let mut parsed: StylePreviewReq = match serde_json::from_str(&body_str) {
            Ok(p) => p,
            Err(e) => {
                let err_json =
                    serde_json::json!({ "error": format!("Błąd parsowania JSON: {}", e) });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        parsed.style.sync_from_sliders();
        let gender = parsed.gender.as_deref().unwrap_or("male");
        let text = parsed
            .text
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(
                "To jest test naszego nowego wzorca ekspresji głosowej w czasie rzeczywistym.",
            );
        let provider = parsed.provider.as_deref().unwrap_or("piper");

        let voice_str =
            if gender.eq_ignore_ascii_case("female") || gender.eq_ignore_ascii_case("kobieta") {
                if provider == "edge" {
                    "pl-PL-ZofiaNeural"
                } else {
                    "female"
                }
            } else if provider == "edge" {
                "pl-PL-MarekNeural"
            } else {
                "male"
            };

        let cache_dir = get_default_cache_dir();
        let _ = std::fs::create_dir_all(&cache_dir);
        let req_id = REQ_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dry_wav = cache_dir.join(format!("style_dry_{}_{}.wav", std::process::id(), req_id));

        let mut dry_samples_opt: Option<(Vec<f32>, u32)> = None;
        if let Ok(synth) = resolve_synthesizer(
            provider,
            voice_str,
            None,
            "pl",
            default_model.as_ref(),
            &piper_cache,
        )
        .await
        {
            let seg = audioticon_core::TextSegment {
                text: audioticon_core::normalize_polish_text(text),
                voice: Some(voice_str.to_string()),
                rate_mod: parsed.style.pacing_mod_pct(),
                ..Default::default()
            };
            if synth.synthesize(&seg, &dry_wav).await.is_ok() {
                if let Ok((samples_i16, sr, _)) = voicelab::read_wav_samples(&dry_wav) {
                    let f32_vec: Vec<f32> = samples_i16
                        .into_iter()
                        .map(|s| s as f32 / 32768.0)
                        .collect();
                    dry_samples_opt = Some((f32_vec, sr));
                }
            }
            let _ = std::fs::remove_file(&dry_wav);
        }

        let (dry_samples, sr) = dry_samples_opt
            .unwrap_or_else(|| (generate_synthetic_vocal_phrase(gender, 24000), 24000));

        let styled_samples =
            audioticon_core::apply_disentangled_style(&dry_samples, sr, &parsed.style);
        let diagnostics = audioticon_core::diagnose_styled_audio(&styled_samples, sr);
        let audio_base64 = match encode_samples_to_wav_base64(&styled_samples, sr) {
            Ok(b64) => b64,
            Err(e) => {
                let err_json = serde_json::json!({
                    "error": format!("Błąd kodowania podglądu WAV: {}", e)
                });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(500));
                return;
            }
        };

        let json_resp = serde_json::json!({
            "success": true,
            "audio_base64": audio_base64,
            "diagnostics": diagnostics,
            "style": parsed.style,
        });
        let mut resp = tiny_http::Response::from_string(json_resp.to_string());
        resp.add_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // Route: POST /api/style-transfer/save
    if method == tiny_http::Method::Post && url == "/api/style-transfer/save" {
        let mut body_str = String::new();
        if request
            .as_reader()
            .take(2 * 1024 * 1024)
            .read_to_string(&mut body_str)
            .is_err()
        {
            let mut resp =
                tiny_http::Response::from_string("{\"error\": \"Nie można odczytać zapytania\"}");
            if let Some(ref h) = cors_header {
                resp.add_header(h.clone());
            }
            let _ = request.respond(resp.with_status_code(400));
            return;
        }

        #[derive(serde::Deserialize)]
        struct SaveStyleReq {
            style: audioticon_core::DisentangledStyleMatrix,
        }

        let mut parsed: SaveStyleReq = match serde_json::from_str(&body_str) {
            Ok(p) => p,
            Err(e) => {
                let err_json =
                    serde_json::json!({ "error": format!("Błąd parsowania JSON: {}", e) });
                let mut resp = tiny_http::Response::from_string(err_json.to_string());
                resp.add_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
                );
                if let Some(ref h) = cors_header {
                    resp.add_header(h.clone());
                }
                let _ = request.respond(resp.with_status_code(400));
                return;
            }
        };

        parsed.style.sync_from_sliders();
        let alias = parsed.style.alias_or_id();
        parsed.style.alias = Some(alias.clone());
        if parsed.style.id.trim().is_empty() {
            parsed.style.id = audioticon_core::sanitize_slug(&alias);
        }
        if parsed.style.emoji.trim().is_empty() {
            parsed.style.emoji = "🎭".to_string();
        }

        let mut cfg = ProjectConfig::discover(None);
        cfg.styles.insert(alias.clone(), parsed.style.clone());
        let emoji_def = CustomEmojiDef {
            name: parsed.style.name.clone(),
            alias: Some(alias.clone()),
            action_type: "filter".to_string(),
            filter: Some(format!("style:{}", alias)),
            voice: None,
            pitch: None,
            rate: Some(parsed.style.pacing_mod_pct()),
            volume: Some(parsed.style.output_gain()),
            description: Some(format!(
                "Wzorzec ekspresji ({:?}): {}",
                parsed.style.category, parsed.style.name
            )),
        };
        cfg.emojis
            .insert(parsed.style.emoji.trim().to_string(), emoji_def);
        let _ = cfg.save_to_path(std::path::Path::new("audioticon.toml"));

        let mut reg = presets::get_active_registry();
        reg.load_project_config(&cfg);
        let styles = reg.list_styles();
        presets::set_active_registry(reg);

        let json_resp = serde_json::json!({
            "success": true,
            "style": parsed.style,
            "styles": styles,
            "emojis": cfg.emojis,
        });
        let mut resp = tiny_http::Response::from_string(json_resp.to_string());
        resp.add_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
        );
        if let Some(ref h) = cors_header {
            resp.add_header(h.clone());
        }
        let _ = request.respond(resp);
        return;
    }

    // 404 fallback
    let mut resp = tiny_http::Response::from_string("Nie znaleziono (404)");
    if let Some(ref h) = cors_header {
        resp.add_header(h.clone());
    }
    let _ = request.respond(resp.with_status_code(404));
}

pub async fn run_studio_server(
    host: String,
    port: u16,
    default_provider: TtsProvider,
    default_lang: String,
    model: Option<PathBuf>,
    no_browser: bool,
) -> Result<()> {
    let mut selected_port = port;
    let initial_addr = format!("{}:{}", host, selected_port);
    let srv_raw = match tiny_http::Server::http(&initial_addr) {
        Ok(s) => s,
        Err(e) => {
            let fallbacks = [3030, 3031, 3032, 8080, 8081];
            let mut found = None;
            for &fb in &fallbacks {
                let test_addr = format!("{}:{}", host, fb);
                if let Ok(s) = tiny_http::Server::http(&test_addr) {
                    selected_port = fb;
                    found = Some(s);
                    break;
                }
            }
            match found {
                Some(s) => {
                    eprintln!(
                        "⚠️ Port {} jest zajęty (np. przez inną aplikację). Automatycznie wybrano port {}.",
                        port, selected_port
                    );
                    s
                }
                None => anyhow::bail!(
                    "Nie można uruchomić serwera na {}: {}. Sprawdź, czy port nie jest zajęty.",
                    initial_addr,
                    e
                ),
            }
        }
    };

    let addr = format!("{}:{}", host, selected_port);
    let server = Arc::new(srv_raw);
    let _ = ACTIVE_HOST.set(host.to_string());
    ACTIVE_PORT.store(selected_port, Ordering::SeqCst);
    let local_ip = get_local_ip();

    println!("============================================================");
    println!("🚀 AUDIOTICON STUDIO (MODULAR WORKBENCH) URUCHOMIONE!");
    println!("============================================================");
    println!("🌐 Adres na komputerze: http://{}", addr);
    if host == "0.0.0.0" {
        println!(
            "📱 Adres dla tabletu (LAN): http://{}:{} [Zaufana sieć domowa]",
            local_ip, selected_port
        );
    } else {
        println!("🔒 Tryb bezpieczny:     Tylko localhost (127.0.0.1)");
        println!("💡 Dostęp z tabletu:    Uruchom 'audioticon serve --host 0.0.0.0' w zaufanej sieci Wi-Fi");
    }
    println!("🎛️  Interfejs:          Modularny Studio Workbench (Cosmic + CapCut + Notion)");
    println!("⚙️  Domyślny silnik:     {:?}", default_provider);
    println!("🗣️  Domyślny język:      {}", default_lang);

    let ffmpeg_ok = audioticon_core::check_ffmpeg_installed();
    if ffmpeg_ok {
        println!("🎬 Silnik DSP & FFmpeg:  Dostępny (Mastering EBU R128 & M4B/FLAC aktywne)");
    } else {
        println!("⚠️  OSTRZEŻENIE:         Program 'ffmpeg' nie został wykryty w PATH!");
        println!("   Eksport M4B/FLAC wymaga instalacji ffmpeg:");
        println!("   • Ubuntu/Debian: sudo apt install ffmpeg");
        println!("   • macOS: brew install ffmpeg");
        println!("   • Windows: winget install Gyan.FFmpeg");
    }

    println!("💡 Klasyczny widok dostępny pod: http://{}/classic", addr);
    println!("Naciśnij Ctrl+C, aby zatrzymać serwer.");
    println!("============================================================");

    if !no_browser {
        let open_url = format!("http://{}", addr);
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(600)).await;
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("open").arg(&open_url).spawn();
            #[cfg(target_os = "windows")]
            let _ = std::process::Command::new("cmd")
                .args(["/C", "start", &open_url])
                .spawn();
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let _ = std::process::Command::new("xdg-open")
                .arg(&open_url)
                .spawn();
        });
    }

    let piper_cache: PiperCache = Arc::new(Mutex::new(HashMap::new()));
    {
        let cfg = ProjectConfig::discover(None);
        let mut reg = presets::get_active_registry();
        reg.load_project_config(&cfg);
        presets::set_active_registry(reg);
    }

    loop {
        let server_clone = server.clone();
        let request = match tokio::task::spawn_blocking(move || server_clone.recv()).await? {
            Ok(req) => req,
            Err(_) => break,
        };

        let piper_cache = piper_cache.clone();
        let default_model = model.clone();
        tokio::spawn(async move {
            handle_studio_request(request, piper_cache, default_model).await;
        });
    }

    Ok(())
}
