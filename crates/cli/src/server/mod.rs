use anyhow::Result;
use audioticon_core::{
    base64_decode, compute_segment_hash, credentials, download_model, estimate_cost, export_to_m4b,
    export_to_mp3, find_model, list_models, models_dir, presets, remove_model,
    update_voices_catalog, voice_design, voicelab, AudioCertificate, AudioDirectorEngine, AudioWallet,
    AudiobookMetadata, CustomEmojiDef, DirectedItem, EdgeTtsBridge, ElevenLabsBridge,
    EmojiDirector, ImportedBook, PiperBridge, ProjectConfig, SpeechSynthesizer, VoiceProfile,
};
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tiny_http::Header;
use tokio::sync::Mutex;

use crate::args::{get_default_cache_dir, get_default_piper_model, TtsProvider};

static REQ_COUNTER: AtomicU64 = AtomicU64::new(0);

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
    clean == "https://jarohull-ai.github.io"
        || clean == "http://localhost"
        || clean.starts_with("http://localhost:")
        || clean == "http://127.0.0.1"
        || clean.starts_with("http://127.0.0.1:")
        || clean == "https://localhost"
        || clean.starts_with("https://localhost:")
        || clean == "https://127.0.0.1"
        || clean.starts_with("https://127.0.0.1:")
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
            || url.starts_with("/api/render")
            || url.starts_with("/api/estimate")
            || url.starts_with("/api/import")
            || url.starts_with("/api/parse")
            || url.starts_with("/api/voicelab")
            || url.starts_with("/api/custom-emojis")
            || url.starts_with("/api/auto-direct")
            || url.starts_with("/api/cache")
            || url.starts_with("/api/wallet"))
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
            Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..]).unwrap(),
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
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..])
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
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..])
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
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..])
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
        if !wallet.acx_certificate_unlocked {
            let err_json = serde_json::json!({
                "error": "Certyfikat ACX / Storytel wymaga Pakietu Pisarz (Writer Tier). Odblokuj go w zakładce Doładuj.",
                "tier": wallet.tier.name(),
                "unlocked": false
            });
            let mut resp = tiny_http::Response::from_string(err_json.to_string());
            resp.add_header(
                Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..])
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
        let cert_content = if cert_path.exists() {
            std::fs::read_to_string(&cert_path).unwrap_or_else(|_| "{}".to_string())
        } else {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let cert = AudioCertificate {
                ledger_id: "AUD-ACX-2026-STUDIO".to_string(),
                title: "Audioticon Studio Master".to_string(),
                author: "Audioticon".to_string(),
                timestamp: now,
                master_lufs: -19.5,
                true_peak_db: -3.0,
                sample_rate: 44100,
                duration_seconds: 0.0,
                acx_compliant: true,
                storytel_compliant: true,
                audio_sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
                script_sha256: None,
                license_tier: wallet.tier.as_str().to_string(),
                digital_signature: "audioticon_verified_signature".to_string(),
            };
            serde_json::to_string_pretty(&cert).unwrap()
        };

        let mut resp = tiny_http::Response::from_string(cert_content);
        resp.add_header(
            Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..]).unwrap(),
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
            match base64_decode(b64) {
                Ok(bytes) => match ext.as_str() {
                    "epub" => ImportedBook::from_epub_bytes(&bytes, title_hint.as_deref()),
                    "docx" => ImportedBook::from_docx_bytes(&bytes, title_hint.as_deref()),
                    _ => {
                        let text = String::from_utf8_lossy(&bytes);
                        Ok(ImportedBook::from_text_str(&text, title_hint.as_deref()))
                    }
                },
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

        let director = EmojiDirector::default()
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
        let director = EmojiDirector::default()
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

                let mut ledger_id_opt = None;
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
                        let _ = std::fs::write(&cert_path, serde_json::to_string_pretty(&cert).unwrap_or_default());
                        ledger_id_opt = Some(cert.ledger_id);
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
                            &b"X-Audioticon-Acx-Unlocked, X-Audioticon-Tier, X-Audioticon-Ledger-Id, X-Audioticon-Lufs, X-Audioticon-Peak, X-Audioticon-Balance"[..],
                        )
                        .unwrap(),
                    );
                    resp.add_header(
                        Header::from_bytes(
                            &b"X-Audioticon-Acx-Unlocked"[..],
                            if wallet.acx_certificate_unlocked { &b"true"[..] } else { &b"false"[..] },
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
                    if let Some(ref lid) = ledger_id_opt {
                        resp.add_header(
                            Header::from_bytes(&b"X-Audioticon-Ledger-Id"[..], lid.as_bytes()).unwrap(),
                        );
                        resp.add_header(
                            Header::from_bytes(&b"X-Audioticon-Lufs"[..], &b"-19.5"[..]).unwrap(),
                        );
                        resp.add_header(
                            Header::from_bytes(&b"X-Audioticon-Peak"[..], &b"-3.0"[..]).unwrap(),
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
        let director = EmojiDirector::default()
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

                let mut wallet = AudioWallet::load();
                if req.provider.to_lowercase() == "elevenlabs" {
                    let _ = wallet.spend(10, "Eksport audiobooka ElevenLabs v3", None);
                    let _ = wallet.save();
                }

                let mut ledger_id_opt = None;
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
                        let _ = std::fs::write(&cert_path, serde_json::to_string_pretty(&cert).unwrap_or_default());
                        ledger_id_opt = Some(cert.ledger_id);
                    }
                }

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
                            &b"X-Audioticon-Acx-Unlocked, X-Audioticon-Tier, X-Audioticon-Ledger-Id, X-Audioticon-Lufs, X-Audioticon-Peak, X-Audioticon-Balance"[..],
                        )
                        .unwrap(),
                    );
                    resp.add_header(
                        Header::from_bytes(
                            &b"X-Audioticon-Acx-Unlocked"[..],
                            if wallet.acx_certificate_unlocked { &b"true"[..] } else { &b"false"[..] },
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
                    if let Some(ref lid) = ledger_id_opt {
                        resp.add_header(
                            Header::from_bytes(&b"X-Audioticon-Ledger-Id"[..], lid.as_bytes()).unwrap(),
                        );
                        resp.add_header(
                            Header::from_bytes(&b"X-Audioticon-Lufs"[..], &b"-19.5"[..]).unwrap(),
                        );
                        resp.add_header(
                            Header::from_bytes(&b"X-Audioticon-Peak"[..], &b"-3.0"[..]).unwrap(),
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
        let json_resp = serde_json::json!({
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

    println!("============================================================");
    println!("🚀 AUDIOTICON STUDIO (MODULAR WORKBENCH) URUCHOMIONE!");
    println!("============================================================");
    println!("🌐 Adres w przeglądarce: http://{}", addr);
    println!("🎛️  Interfejs:          Modularny Studio Workbench (Cosmic + CapCut + Notion)");
    println!("⚙️  Domyślny silnik:     {:?}", default_provider);
    println!("🗣️  Domyślny język:      {}", default_lang);
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
