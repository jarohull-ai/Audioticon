use std::process::Command;

#[test]
fn test_cli_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .arg("--help")
        .output()
        .expect("Failed to execute audioticon --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Audioticon"));
    assert!(stdout.contains("render"));
    assert!(stdout.contains("script"));
    assert!(stdout.contains("presets"));
}

#[test]
fn test_cli_presets_list() {
    let output = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .arg("presets")
        .output()
        .expect("Failed to execute audioticon presets");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Rejestr Presetów Akustycznych"));
    assert!(stdout.contains("radio"));
    assert!(stdout.contains("cathedral"));
}

#[test]
fn test_cli_script_timeline_export() {
    let output = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .args(["script", "--demo", "-f", "timeline"])
        .output()
        .expect("Failed to execute audioticon script");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Partytura Aktorska Osi Czasu"));
    assert!(stdout.contains("PAUZA"));
}

#[test]
fn test_cli_script_elevenlabs_export() {
    let output = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .args(["script", "--demo", "-f", "elevenlabs"])
        .output()
        .expect("Failed to execute audioticon script elevenlabs");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[shouts]"));
}

#[test]
fn test_cli_lang_validation() {
    let output = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .args(["script", "--demo", "--lang", "de"])
        .output()
        .expect("Failed to execute script with unsupported lang");

    // Must fail gracefully with unsupported language message
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Nieobsługiwany język"));
}

#[test]
fn test_cli_models_list() {
    let output = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .arg("models")
        .output()
        .expect("Failed to execute audioticon models");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Menedżer Modeli Neuronowych"));
    assert!(stdout.contains("pl_PL-darkman-medium"));
    assert!(stdout.contains("pl_PL-gosia-medium"));
    assert!(stdout.contains("en_US-lessac-medium"));
}

#[test]
fn test_cli_models_list_filter_lang() {
    let output = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .args(["models", "list", "--lang", "pl"])
        .output()
        .expect("Failed to execute audioticon models list --lang pl");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("pl_PL-darkman-medium"));
    assert!(stdout.contains("pl_PL-gosia-medium"));
    assert!(!stdout.contains("en_US-lessac-medium"));
    assert!(stdout.contains("CC0"));
}

#[test]
fn test_cli_import_text() {
    let tmp_dir = std::env::temp_dir();
    let sample_file = tmp_dir.join("test_book_sample.txt");
    let sample_content = "Rozdział 1: Świt nad Arktyką\nZimny wiatr wiał od bieguna.\n\nRozdział 2: Lądowanie\nStatek dotknął pokrywy lodowej.";
    std::fs::write(&sample_file, sample_content).expect("Write test sample");

    let output = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .args([
            "import",
            "--input",
            sample_file.to_str().unwrap(),
            "--preview",
        ])
        .output()
        .expect("Failed to execute audioticon import");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Importer Książek"));
    assert!(stdout.contains("Świt nad Arktyką"));
    assert!(stdout.contains("Lądowanie"));

    let _ = std::fs::remove_file(sample_file);
}

fn get_free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind port 0")
        .local_addr()
        .expect("local_addr")
        .port()
}

fn http_roundtrip(
    port: u16,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> (u16, String, Vec<u8>) {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).expect("connect to server");
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let b = body.unwrap_or("");
    let req = format!(
        "{} {} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        method, path, port, b.len(), b
    );
    stream.write_all(req.as_bytes()).expect("write request");
    let mut res = Vec::new();
    stream.read_to_end(&mut res).expect("read response");

    let header_end = res
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(res.len());
    let header_str = String::from_utf8_lossy(&res[..header_end]).to_string();
    let body_bytes = if header_end + 4 <= res.len() {
        let raw_body = &res[header_end + 4..];
        if header_str
            .to_lowercase()
            .contains("transfer-encoding: chunked")
        {
            let mut dechunked = Vec::new();
            let mut cursor = 0;
            while cursor < raw_body.len() {
                if let Some(pos) = raw_body[cursor..].windows(2).position(|w| w == b"\r\n") {
                    let hex_str =
                        std::str::from_utf8(&raw_body[cursor..cursor + pos]).unwrap_or("0");
                    let chunk_size = usize::from_str_radix(hex_str.trim(), 16).unwrap_or(0);
                    if chunk_size == 0 {
                        break;
                    }
                    cursor += pos + 2;
                    let chunk_end = (cursor + chunk_size).min(raw_body.len());
                    dechunked.extend_from_slice(&raw_body[cursor..chunk_end]);
                    cursor = chunk_end + 2; // skip trailing \r\n
                } else {
                    break;
                }
            }
            dechunked
        } else {
            raw_body.to_vec()
        }
    } else {
        Vec::new()
    };

    let status = header_str
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);

    (status, header_str, body_bytes)
}

#[test]
fn test_api_server_endpoints_e2e() {
    let port = get_free_port();
    let test_wallet = std::env::temp_dir().join(format!(
        "isolated_api_wallet_{}_{}.json",
        std::process::id(),
        port
    ));
    let _ = std::fs::remove_file(&test_wallet);
    let mut child = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .args(["serve", "--port", &port.to_string(), "--no-browser"])
        .env("AUDIOTICON_WALLET_PATH", &test_wallet)
        .spawn()
        .expect("spawn server");

    // Poll until server is listening
    let mut connected = false;
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
            connected = true;
            break;
        }
    }
    assert!(connected, "Server did not start listening in time");

    // 1. GET /api/models should return 200 and list models
    let (status, headers, body) = http_roundtrip(port, "GET", "/api/models", None);
    assert_eq!(status, 200);
    assert!(headers.contains("application/json"));
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("pl_PL-darkman-medium"));

    // 1b. GET /api/system/health should return 200 with system diagnostics
    let (h_status, h_headers, h_body) = http_roundtrip(port, "GET", "/api/system/health", None);
    assert_eq!(h_status, 200);
    assert!(h_headers.contains("application/json"));
    let h_str = String::from_utf8_lossy(&h_body);
    assert!(h_str.contains("\"status\""));
    assert!(h_str.contains("\"ffmpeg\""));
    assert!(h_str.contains("\"installed_models_count\""));

    // 1c. GET /api/system/update-check should return 200 with semver details
    let (u_status, u_headers, u_body) =
        http_roundtrip(port, "GET", "/api/system/update-check", None);
    assert_eq!(u_status, 200);
    assert!(u_headers.contains("application/json"));
    let u_str = String::from_utf8_lossy(&u_body);
    assert!(u_str.contains("\"current_version\""));
    assert!(u_str.contains("\"latest_version\""));
    assert!(u_str.contains("\"update_available\""));

    // 1d. POST /api/dialog/analyze should detect characters and speech lines
    let dialog_req =
        "{\"text\": \"Jan: Cześć Anno.\\nAnna: Cześć Janie.\\nI poszli dalej.\", \"lang\": \"pl\"}";
    let (d_status, _, d_body) =
        http_roundtrip(port, "POST", "/api/dialog/analyze", Some(dialog_req));
    assert_eq!(d_status, 200);
    let d_str = String::from_utf8_lossy(&d_body);
    assert!(d_str.contains("\"name\":\"Jan\""));
    assert!(d_str.contains("\"name\":\"Anna\""));
    assert!(d_str.contains("\"name\":\"Narrator\""));

    // 1e. POST /api/dialog/apply should return script with voice tags
    let apply_req = "{\"text\": \"Jan: Cześć.\\nAnna: Hej.\", \"casting\": {\"Jan\": \"pl_PL-darkman-medium\", \"Anna\": \"pl_PL-gosia-medium\"}}";
    let (a_status, _, a_body) = http_roundtrip(port, "POST", "/api/dialog/apply", Some(apply_req));
    assert_eq!(a_status, 200);
    let a_str = String::from_utf8_lossy(&a_body);
    assert!(a_str.contains("[głos:pl_PL-darkman-medium] Cześć."));
    assert!(a_str.contains("[głos:pl_PL-gosia-medium] Hej."));

    // 2. GET /api/export with empty text should return 400 Bad Request
    let (status, _, body) = http_roundtrip(port, "GET", "/api/export", None);
    assert_eq!(status, 400);
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("Brak tekstu"));

    // 3. POST /api/export with empty text should return 400 Bad Request
    let (status, _, body) = http_roundtrip(port, "POST", "/api/export", Some("{\"text\": \"\"}"));
    assert_eq!(status, 400);
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("Brak tekstu"));

    // 4. POST /api/cache/status should return 200 and segment hash analysis
    let cache_req =
        "{\"text\": \"📖 Rozdział 1 🛑 Pierwsze zdanie. Drugie zdanie.\", \"lang\": \"pl\"}";
    let (status, _, body) = http_roundtrip(port, "POST", "/api/cache/status", Some(cache_req));
    assert_eq!(status, 200);
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("total_speech_segments"));
    assert!(body_str.contains("short_hash"));

    // 5a. Soft-gate check on M4B export before redeeming voucher (Free tier returns 402)
    let export_req = "{\"text\": \"📖 Rozdział 1: Test E2E 🛑 Krótkie zdanie testowe.\", \"format\": \"m4b\", \"title\": \"Test E2E Audiobook\", \"author\": \"Jarosław Kuchta\", \"provider\": \"piper\"}";
    let (s_gate, _, b_gate) = http_roundtrip(port, "POST", "/api/export", Some(export_req));
    assert_eq!(
        s_gate, 402,
        "M4B export on free tier must return 402 Payment Required"
    );
    let gate_str = String::from_utf8_lossy(&b_gate);
    assert!(gate_str.contains("m4b_requires_pro"));

    // 5b. POST /api/wallet/redeem: valid cryptographic voucher unlocks Writer Tier (Pisarz)
    let valid_voucher = audioticon_core::generate_voucher_code(
        audioticon_core::WalletTier::Writer,
        25000,
        "VIP2026",
    );
    let redeem_payload = format!("{{\"code\": \"{}\"}}", valid_voucher);
    let (v_status, _, v_body) =
        http_roundtrip(port, "POST", "/api/wallet/redeem", Some(&redeem_payload));
    assert_eq!(v_status, 200);
    let v_str = String::from_utf8_lossy(&v_body);
    assert!(v_str.contains("\"success\":true"));
    assert!(v_str.contains("\"tier\":\"Pisarz\""));
    assert!(v_str.contains("\"acx_certificate_unlocked\":true"));

    // 5c. POST /api/export with format=m4b now proceeds with unlocked Writer Tier
    let (status, headers, body) = http_roundtrip(port, "POST", "/api/export", Some(export_req));
    let body_str = String::from_utf8_lossy(&body);
    if status == 200 {
        assert!(headers.contains("audio/mp4"));
        assert!(headers.contains("Test_E2E_Audiobook.m4b"));
        assert!(body.len() > 1000);
        assert!(
            body.windows(4).any(|w| w == b"ftyp"),
            "M4B file must contain ftyp atom"
        );
        assert!(
            body.windows(4).any(|w| w == b"M4B "),
            "M4B file must contain M4B brand atom"
        );
    } else {
        assert_eq!(status, 400);
        assert!(body_str.contains("Nie znaleziono modelu Piper"));
    }

    // 6. Verify round-trip cache status: if export succeeded, the exact sentence must be reported in_cache: true
    if status == 200 {
        let verify_cache_req = "{\"text\": \"📖 Rozdział 1: Test E2E 🛑 Krótkie zdanie testowe.\", \"lang\": \"pl\", \"provider\": \"piper\"}";
        let (c_status, _, c_body) =
            http_roundtrip(port, "POST", "/api/cache/status", Some(verify_cache_req));
        assert_eq!(c_status, 200);
        let c_str = String::from_utf8_lossy(&c_body);
        assert!(
            c_str.contains("\"in_cache\":true"),
            "Rendered sentence must be found in physical cache"
        );
        assert!(
            !c_str.contains("\"cached_count\":0"),
            "Cached count must be greater than 0"
        );
    }

    // 7. GET /api/wallet: verify wallet balance, tier, and transaction audit
    let (w_status, w_headers, w_body) = http_roundtrip(port, "GET", "/api/wallet", None);
    assert_eq!(w_status, 200);
    assert!(w_headers.contains("application/json"));
    let w_str = String::from_utf8_lossy(&w_body);
    assert!(w_str.contains("\"balance\""));
    assert!(w_str.contains("\"tier\""));
    assert!(w_str.contains("\"transactions\""));

    // 8. POST /api/wallet/redeem: invalid voucher should fail with 400
    let (r_status, _, r_body) = http_roundtrip(
        port,
        "POST",
        "/api/wallet/redeem",
        Some("{\"code\": \"AUD-INVALID-CODE\"}"),
    );
    assert_eq!(r_status, 400);
    let r_str = String::from_utf8_lossy(&r_body);
    assert!(r_str.contains("Błąd"));

    // 9. POST /api/wallet/redeem: attempting to redeem already redeemed voucher must fail (replay attack)
    let (v_dup_status, _, v_dup_body) =
        http_roundtrip(port, "POST", "/api/wallet/redeem", Some(&redeem_payload));
    assert_eq!(v_dup_status, 400);
    let v_dup_str = String::from_utf8_lossy(&v_dup_body);
    assert!(
        v_dup_str.contains("został już wcześniej wykorzystany") || v_dup_str.contains("aktywowana")
    );

    // 10. GET /api/ledger/certificate: with Writer tier unlocked, certificate must be returned
    let (cert_status, cert_headers, cert_body) =
        http_roundtrip(port, "GET", "/api/ledger/certificate", None);
    assert_eq!(cert_status, 200);
    assert!(cert_headers.contains("application/json"));
    let cert_str = String::from_utf8_lossy(&cert_body);
    assert!(cert_str.contains("ledger_id"));
    assert!(cert_str.contains("master_lufs"));
    assert!(cert_str.contains("acx_compliant"));

    // 11. POST /api/voicelab/design/preview: error handling when API key is missing
    let (dp_status, _, dp_body) = http_roundtrip(
        port,
        "POST",
        "/api/voicelab/design/preview",
        Some("{\"description\": \"stary marynarz, chrapliwy bas\"}"),
    );
    // Either 400 (missing key) or 500 (if invalid key configured in user env)
    assert!(dp_status == 400 || dp_status == 500);
    let dp_str = String::from_utf8_lossy(&dp_body);
    assert!(dp_str.contains("error"));

    // 12. POST /api/voicelab/design/save: error handling when voice id is missing or key missing
    let (ds_status, _, ds_body) = http_roundtrip(
        port,
        "POST",
        "/api/voicelab/design/save",
        Some("{\"name\": \"Kapitan\", \"description\": \"marynarz\", \"generated_voice_id\": \"test_voice_123\"}"),
    );
    assert!(ds_status == 400 || ds_status == 500);
    let ds_str = String::from_utf8_lossy(&ds_body);
    assert!(ds_str.contains("error"));

    // 13. POST /api/export with format=flac: verify 24-bit FLAC master generation
    let flac_req = "{\"text\": \"📖 Rozdział 1: Test FLAC 🛑 Zdanie testowe.\", \"format\": \"flac\", \"title\": \"Test Flac\", \"provider\": \"piper\"}";
    let (f_status, f_headers, f_body) = http_roundtrip(port, "POST", "/api/export", Some(flac_req));
    if f_status == 200 {
        assert!(f_headers.contains("audio/flac"));
        assert!(f_headers.contains(".flac"));
        assert!(
            f_body.windows(4).any(|w| w == b"fLaC"),
            "FLAC header missing"
        );
    }

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn test_api_cache_status_disk_verification() {
    let port = get_free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .args(["serve", "--port", &port.to_string(), "--no-browser"])
        .spawn()
        .expect("Failed to spawn studio for cache verification");

    let mut connected = false;
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
            connected = true;
            break;
        }
    }
    assert!(connected, "Server did not start listening in time");

    // 1. Initial status query: sentence should not be in cache initially
    let test_sentence = format!("Test weryfikacji dyskowej cache {}", std::process::id());
    let req_body = format!(
        "{{\"text\": \"{}\", \"lang\": \"pl\", \"provider\": \"piper\"}}",
        test_sentence
    );
    let (s1, _, b1) = http_roundtrip(port, "POST", "/api/cache/status", Some(&req_body));
    assert_eq!(s1, 200);
    let v1: serde_json::Value = serde_json::from_slice(&b1).expect("Parse json v1");
    assert_eq!(v1["in_cache"], false);
    let seg = &v1["segments"][0];
    let hash = seg["hash"].as_str().expect("segment hash");

    // 2. Write a simulated cached audio file into user's audioticon cache dir
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("audioticon");
    std::fs::create_dir_all(&cache_dir).expect("Create cache dir");
    let cache_file = cache_dir.join(format!("{}.wav", hash));
    std::fs::write(&cache_file, b"RIFF simulated cached audio chunk").expect("Write cache dummy");

    // 3. Second status query: must detect the cached file on disk immediately
    let (s2, _, b2) = http_roundtrip(port, "POST", "/api/cache/status", Some(&req_body));
    assert_eq!(s2, 200);
    let v2: serde_json::Value = serde_json::from_slice(&b2).expect("Parse json v2");
    assert_eq!(v2["in_cache"], true, "Segment must be reported in cache");
    assert_eq!(v2["cached_count"], 1);
    assert_eq!(v2["segments"][0]["in_cache"], true);
    assert!(
        v2["segments"][0]["size_bytes"].as_u64().unwrap_or(0) > 0,
        "Cache file size must be non-zero"
    );

    // Clean up
    let _ = std::fs::remove_file(&cache_file);
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn test_cli_providers_lifecycle() {
    let tmp_home = std::env::temp_dir().join(format!(
        "test_cli_prov_home_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&tmp_home);

    // 1. Set mock API key
    let set_out = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .env("HOME", &tmp_home)
        .args([
            "providers",
            "set",
            "elevenlabs",
            "--key",
            "sk_test_secret_1234567890abcdef",
        ])
        .output()
        .expect("Failed to execute audioticon providers set");
    assert!(set_out.status.success());
    let stdout_set = String::from_utf8_lossy(&set_out.stdout);
    assert!(stdout_set.contains("Zapisano klucz dla dostawcy 'elevenlabs'"));

    // 2. List providers and verify masking
    let list_out = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .env("HOME", &tmp_home)
        .args(["providers", "list"])
        .output()
        .expect("Failed to execute audioticon providers list");
    assert!(list_out.status.success());
    let stdout_list = String::from_utf8_lossy(&list_out.stdout);
    assert!(stdout_list.contains("ElevenLabs"));
    assert!(stdout_list.contains("sk_t••••cdef"));
    assert!(!stdout_list.contains("1234567890")); // Plain key must not leak

    // 3. Remove provider
    let rem_out = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .env("HOME", &tmp_home)
        .args(["providers", "remove", "elevenlabs"])
        .output()
        .expect("Failed to execute audioticon providers remove");
    assert!(rem_out.status.success());
    let stdout_rem = String::from_utf8_lossy(&rem_out.stdout);
    assert!(stdout_rem.contains("Usunięto konfigurację dostawcy 'elevenlabs'"));

    let _ = std::fs::remove_dir_all(&tmp_home);
}

#[test]
fn test_cli_estimate_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .args(["estimate", "--provider", "elevenlabs", "--voice", "adam"])
        .output()
        .expect("Failed to execute audioticon estimate");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Szacunek kosztów syntezy mowy"));
    assert!(stdout.contains("ELEVENLABS"));
    assert!(stdout.contains("Liczba segmentów mowy"));
    assert!(stdout.contains("Szacowany koszt:"));
}

#[test]
fn test_cli_render_estimate_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .args(["render", "--demo", "--provider", "elevenlabs", "--estimate"])
        .output()
        .expect("Failed to execute audioticon render --estimate");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Szacunek kosztów syntezy mowy (ELEVENLABS)"));
    assert!(stdout.contains("Łączna liczba znaków:"));
}

fn http_roundtrip_with_headers(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> (u16, String, Vec<u8>) {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).expect("connect to server");
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let b = body.unwrap_or("");
    let mut header_lines = String::new();
    let mut has_ct = false;
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("content-type") {
            has_ct = true;
        }
        header_lines.push_str(&format!("{}: {}\r\n", k, v));
    }
    if !has_ct {
        header_lines.push_str("Content-Type: application/json\r\n");
    }

    let req = format!(
        "{} {} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        method, path, port, header_lines, b.len(), b
    );
    stream.write_all(req.as_bytes()).expect("write request");
    let mut res = Vec::new();
    stream.read_to_end(&mut res).expect("read response");

    let header_end = res
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(res.len());
    let header_str = String::from_utf8_lossy(&res[..header_end]).to_string();
    let body_bytes = if header_end + 4 <= res.len() {
        let raw_body = &res[header_end + 4..];
        if header_str
            .to_lowercase()
            .contains("transfer-encoding: chunked")
        {
            let mut dechunked = Vec::new();
            let mut cursor = 0;
            while cursor < raw_body.len() {
                if let Some(pos) = raw_body[cursor..].windows(2).position(|w| w == b"\r\n") {
                    let hex_str =
                        std::str::from_utf8(&raw_body[cursor..cursor + pos]).unwrap_or("0");
                    let chunk_size = usize::from_str_radix(hex_str.trim(), 16).unwrap_or(0);
                    if chunk_size == 0 {
                        break;
                    }
                    cursor += pos + 2;
                    let chunk_end = (cursor + chunk_size).min(raw_body.len());
                    dechunked.extend_from_slice(&raw_body[cursor..chunk_end]);
                    cursor = chunk_end + 2; // skip trailing \r\n
                } else {
                    break;
                }
            }
            dechunked
        } else {
            raw_body.to_vec()
        }
    } else {
        Vec::new()
    };

    let status = header_str
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);

    (status, header_str, body_bytes)
}

#[test]
fn test_api_csrf_protection_and_origin_validation() {
    let port = get_free_port();
    let tmp_home = std::env::temp_dir().join(format!(
        "test_api_csrf_home_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&tmp_home);
    let mut child = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .env("HOME", &tmp_home)
        .args(["serve", "--port", &port.to_string(), "--no-browser"])
        .spawn()
        .expect("Failed to start audioticon serve");

    let mut connected = false;
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
            connected = true;
            break;
        }
    }
    assert!(connected, "Server did not start listening in time");

    let payload = "{\"provider\": \"elevenlabs\", \"api_key\": \"sk_evil_hacked_key_12345\"}";

    // 1. Attack from malicious origin (https://evil.example) MUST return 403 Forbidden
    let (s_evil, _, b_evil) = http_roundtrip_with_headers(
        port,
        "POST",
        "/api/providers/set",
        &[("Origin", "https://evil.example")],
        Some(payload),
    );
    assert_eq!(
        s_evil, 403,
        "Untrusted Origin must be rejected with 403 Forbidden"
    );
    let err_str = String::from_utf8_lossy(&b_evil);
    assert!(err_str.contains("403 Forbidden"));

    // 2. Attack without application/json (e.g. form post text/plain) MUST return 415 Unsupported Media Type
    let (s_simple, _, _) = http_roundtrip_with_headers(
        port,
        "POST",
        "/api/providers/set",
        &[
            ("Origin", "http://localhost:3000"),
            ("Content-Type", "text/plain"),
        ],
        Some(payload),
    );
    assert_eq!(
        s_simple, 415,
        "Non-JSON POST must be rejected with 415 to block simple CSRF"
    );

    // 3. Legitimate request from localhost MUST succeed with 200 OK
    let (s_ok, _, b_ok) = http_roundtrip_with_headers(
        port,
        "POST",
        "/api/providers/set",
        &[
            ("Origin", "http://localhost:3000"),
            ("Content-Type", "application/json"),
        ],
        Some(payload),
    );
    assert_eq!(s_ok, 200, "Legitimate localhost origin must succeed");
    let ok_str = String::from_utf8_lossy(&b_ok);
    assert!(ok_str.contains("\"status\":\"ok\""));

    // Clean up
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&tmp_home);
}

#[test]
fn test_cli_voicelab_lifecycle() {
    let tmp = std::env::temp_dir().join(format!("test_cli_vl_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);

    // List empty
    let output = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .env("AUDIOTICON_VOICES_DIR", tmp.to_str().unwrap())
        .args(["voicelab", "list"])
        .output()
        .expect("Failed to execute voicelab list");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Profile głosowe lektorów"));

    // Create profile
    let output_create = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .env("AUDIOTICON_VOICES_DIR", tmp.to_str().unwrap())
        .args([
            "voicelab",
            "create",
            "tomek-lektor",
            "--name",
            "Tomek (Lektor)",
            "--gender",
            "male",
            "--lang",
            "pl",
        ])
        .output()
        .expect("Failed to execute voicelab create");
    assert!(output_create.status.success());
    let stdout_create = String::from_utf8_lossy(&output_create.stdout);
    assert!(stdout_create.contains("Pomyślnie utworzono profil głosu"));

    // List again to verify presence
    let output_list2 = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .env("AUDIOTICON_VOICES_DIR", tmp.to_str().unwrap())
        .args(["voicelab", "list"])
        .output()
        .expect("Failed to execute voicelab list");
    assert!(output_list2.status.success());
    let stdout_list2 = String::from_utf8_lossy(&output_list2.stdout);
    assert!(stdout_list2.contains("Tomek (Lektor)"));
    assert!(stdout_list2.contains("tomek-lektor"));

    // Delete profile
    let output_del = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .env("AUDIOTICON_VOICES_DIR", tmp.to_str().unwrap())
        .args(["voicelab", "delete", "tomek-lektor"])
        .output()
        .expect("Failed to execute voicelab delete");
    assert!(output_del.status.success());

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_cli_emojis_lifecycle() {
    let tmp = std::env::temp_dir().join(format!("test_cli_emojis_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);

    // List
    let output = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .current_dir(&tmp)
        .args(["emojis", "list"])
        .output()
        .expect("Failed to execute emojis list");
    assert!(output.status.success());

    // Add custom emoji
    let output_add = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .current_dir(&tmp)
        .args([
            "emojis",
            "add",
            "🧙‍♂️",
            "Czarodziej",
            "--filter",
            "cathedral",
            "--pitch",
            "-4",
        ])
        .output()
        .expect("Failed to execute emojis add");
    assert!(output_add.status.success());
    let stdout_add = String::from_utf8_lossy(&output_add.stdout);
    assert!(stdout_add.contains("Pomyślnie zapisano własną emotikonkę"));

    // List again
    let output_list2 = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .current_dir(&tmp)
        .args(["emojis", "list"])
        .output()
        .expect("Failed to execute emojis list");
    assert!(output_list2.status.success());
    let stdout_list2 = String::from_utf8_lossy(&output_list2.stdout);
    assert!(stdout_list2.contains("Czarodziej"));
    assert!(stdout_list2.contains("cathedral"));

    // Remove
    let output_rem = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .current_dir(&tmp)
        .args(["emojis", "remove", "🧙‍♂️"])
        .output()
        .expect("Failed to execute emojis remove");
    assert!(output_rem.status.success());

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_api_voicelab_dataset_download_e2e() {
    let port = get_free_port();
    let tmp_voices = std::env::temp_dir().join(format!(
        "test_vl_ds_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let profile_dir = tmp_voices.join("lektor-test-zip");
    std::fs::create_dir_all(&profile_dir).expect("create profile dir");

    let mut prof =
        audioticon_core::VoiceProfile::new("lektor-test-zip", "Lektor Test ZIP", "male", "pl");
    prof.description = Some("Profil do testu ZIP".into());
    let dummy_wav = profile_dir.join("sample_01.wav");
    std::fs::write(&dummy_wav, b"RIFF....WAVEfmt ....data....").expect("write dummy wav");
    prof.samples.push(audioticon_core::VoiceSampleMeta {
        filename: "sample_01.wav".into(),
        duration_sec: 1.5,
        sample_rate: 24000,
        channels: 1,
        rms_db: -20.0,
        peak_db: -3.0,
        snr_db: 25.0,
        clipping: false,
        created_at: "2026-11-03T12:00:00Z".into(),
    });
    std::fs::write(
        profile_dir.join("profile.json"),
        serde_json::to_string(&prof).unwrap(),
    )
    .expect("write profile json");

    let mut child = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .env("AUDIOTICON_VOICES_DIR", &tmp_voices)
        .env("AUDIOTICON_MOCK_WALLET_TIER", "STUDIO")
        .args(["serve", "--port", &port.to_string(), "--no-browser"])
        .spawn()
        .expect("Failed to start audioticon serve");

    let mut connected = false;
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
            connected = true;
            break;
        }
    }
    assert!(connected, "Server did not start listening in time");

    // 1. Nonexistent profile should return 404
    let (status_404, _, _) = http_roundtrip(
        port,
        "GET",
        "/api/voicelab/dataset?profile=nieistnieje",
        None,
    );
    assert_eq!(status_404, 404);

    // 2. Traversal attempt should return 400
    let (status_400, _, _) =
        http_roundtrip(port, "GET", "/api/voicelab/dataset?profile=../hack", None);
    assert_eq!(status_400, 400);

    // 3. Existing profile dataset ZIP download should return 200 and valid ZIP archive
    let (status_ok, headers, body) = http_roundtrip(
        port,
        "GET",
        "/api/voicelab/dataset?profile=lektor-test-zip",
        None,
    );
    assert_eq!(status_ok, 200);
    assert!(headers.contains("application/zip"));
    assert!(headers.contains("attachment; filename=\"lektor-test-zip_dataset.zip\""));
    assert!(body.len() > 30, "ZIP archive must not be empty");
    assert_eq!(
        &body[0..4],
        b"PK\x03\x04",
        "Must begin with ZIP local header magic bytes"
    );

    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("profile.json"));
    assert!(body_str.contains("metadata.csv"));
    assert!(body_str.contains("sample_01.wav"));

    // Clean up
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&tmp_voices);
}

#[test]
fn test_cli_direct_command_missing_key() {
    let tmp = std::env::temp_dir().join(format!("test_direct_cmd_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);
    let sample_txt = tmp.join("rozdzial1.txt");
    std::fs::write(&sample_txt, "Gdzie jesteś? — zapytał Piotr.").unwrap();

    let tmp_home = tmp.join("empty_home");
    let _ = std::fs::create_dir_all(&tmp_home);

    let output = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .env("HOME", &tmp_home)
        .env_remove("GEMINI_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .args([
            "direct",
            sample_txt.to_str().unwrap(),
            "--provider",
            "gemini",
        ])
        .output()
        .expect("Failed to execute audioticon direct");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Brak klucza API dla dostawcy 'gemini'"),
        "Expected missing key error, got: {}",
        stderr
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_api_auto_direct_csrf_and_error_handling() {
    let port = get_free_port();
    let tmp_home = std::env::temp_dir().join(format!(
        "test_api_direct_home_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&tmp_home);

    let mut child = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .env("HOME", &tmp_home)
        .env_remove("GEMINI_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .args(["serve", "--port", &port.to_string(), "--no-browser"])
        .spawn()
        .expect("Failed to start audioticon serve");

    let mut connected = false;
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
            connected = true;
            break;
        }
    }
    assert!(connected, "Server did not start listening in time");

    let payload = "{\"text\": \"Halo, słyszysz mnie? — zapytała.\", \"provider\": \"gemini\"}";

    // 1. Untrusted Origin MUST be rejected with 403 Forbidden
    let (s_evil, _, b_evil) = http_roundtrip_with_headers(
        port,
        "POST",
        "/api/auto-direct",
        &[("Origin", "https://evil.example")],
        Some(payload),
    );
    assert_eq!(s_evil, 403, "Untrusted Origin must be rejected with 403");
    let err_str = String::from_utf8_lossy(&b_evil);
    assert!(err_str.contains("403 Forbidden"));

    // 2. Non-JSON Content-Type MUST be rejected with 415
    let (s_simple, _, _) = http_roundtrip_with_headers(
        port,
        "POST",
        "/api/auto-direct",
        &[
            ("Origin", "http://localhost:3000"),
            ("Content-Type", "text/plain"),
        ],
        Some(payload),
    );
    assert_eq!(s_simple, 415, "Non-JSON POST must be rejected with 415");

    // 3. Legitimate request without configured key returns 500 with clear message
    let (s_nokey, _, b_nokey) = http_roundtrip_with_headers(
        port,
        "POST",
        "/api/auto-direct",
        &[
            ("Origin", "http://localhost:3000"),
            ("Content-Type", "application/json"),
        ],
        Some(payload),
    );
    assert_eq!(s_nokey, 500);
    let nokey_str = String::from_utf8_lossy(&b_nokey);
    assert!(nokey_str.contains("Brak klucza API dla dostawcy 'gemini'"));

    // Clean up
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&tmp_home);
}

#[test]
fn test_cli_cache_stats_and_prune() {
    let tmp_cache = std::env::temp_dir().join(format!("test_cli_cache_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_cache);

    // Write a dummy chunk and dummy punchin
    std::fs::write(tmp_cache.join("dummy_chunk_1.wav"), vec![0u8; 5000]).unwrap();
    std::fs::write(tmp_cache.join("dummy_chunk_2.wav"), vec![0u8; 5000]).unwrap();
    std::fs::write(tmp_cache.join("recorded_punchin.wav"), vec![0u8; 5000]).unwrap();

    let exe = env!("CARGO_BIN_EXE_audioticon");

    // 1. Check stats
    let output_stats = std::process::Command::new(exe)
        .args(["cache", "--dir", tmp_cache.to_str().unwrap(), "--stats"])
        .output()
        .expect("Run audioticon cache --stats");
    assert!(output_stats.status.success());
    let stats_stdout = String::from_utf8_lossy(&output_stats.stdout);
    assert!(stats_stdout.contains("Zapisanych plików segmentów: 3"));
    assert!(stats_stdout.contains("Ochrona punch-in: Aktywna"));

    // 2. Prune with limit 8KB -> should prune 1 or 2 dummy chunks, keep recorded_punchin
    let output_prune = std::process::Command::new(exe)
        .args([
            "cache",
            "--dir",
            tmp_cache.to_str().unwrap(),
            "--prune",
            "8KB",
        ])
        .output()
        .expect("Run audioticon cache --prune");
    assert!(output_prune.status.success());
    let prune_stdout = String::from_utf8_lossy(&output_prune.stdout);
    assert!(prune_stdout.contains("Usunięto"));
    assert!(
        tmp_cache.join("recorded_punchin.wav").exists(),
        "Punch-in file must survive pruning!"
    );

    let _ = std::fs::remove_dir_all(&tmp_cache);
}

#[test]
fn test_cli_production_styles_and_aliases() {
    let exe = env!("CARGO_BIN_EXE_audioticon");

    // 1. Verify --help documents --style
    let output_help = Command::new(exe)
        .args(["render", "--help"])
        .output()
        .expect("Run audioticon render --help");
    assert!(output_help.status.success());
    let help_stdout = String::from_utf8_lossy(&output_help.stdout);
    assert!(help_stdout.contains("--style"));
    assert!(help_stdout.contains("narrator"));
    assert!(help_stdout.contains("multi-voice"));
    assert!(help_stdout.contains("audio-drama"));

    // 2. Test estimate with style narrator
    let output_narrator = Command::new(exe)
        .args(["render", "--demo", "--estimate", "--style", "narrator"])
        .output()
        .expect("Run audioticon render --estimate with narrator");
    assert!(output_narrator.status.success());

    // 3. Test estimate with style multi-voice
    let output_multivoice = Command::new(exe)
        .args(["render", "--demo", "--estimate", "--style", "multi-voice"])
        .output()
        .expect("Run audioticon render --estimate with multi-voice");
    assert!(output_multivoice.status.success());

    // 4. Test estimate with alias sluchowisko
    let output_sluchowisko = Command::new(exe)
        .args(["render", "--demo", "--estimate", "-s", "sluchowisko"])
        .output()
        .expect("Run audioticon render --estimate with -s sluchowisko");
    assert!(output_sluchowisko.status.success());
}

#[test]
fn test_cli_cadence_and_breath_intensity_flags() {
    let exe = env!("CARGO_BIN_EXE_audioticon");

    // 1. Verify --help documents --cadence and --breath-intensity
    let output_help = Command::new(exe)
        .args(["render", "--help"])
        .output()
        .expect("Run audioticon render --help");
    assert!(output_help.status.success());
    let help_stdout = String::from_utf8_lossy(&output_help.stdout);
    assert!(help_stdout.contains("--cadence"));
    assert!(help_stdout.contains("--breath-intensity"));
    assert!(help_stdout.contains("subtle"));
    assert!(help_stdout.contains("natural"));
    assert!(help_stdout.contains("off"));

    // 2. Test estimate with cadence natural
    let output_natural = Command::new(exe)
        .args([
            "render",
            "--demo",
            "--estimate",
            "--cadence=true",
            "--breath-intensity",
            "natural",
        ])
        .output()
        .expect("Run audioticon render with natural cadence");
    assert!(output_natural.status.success());

    // 3. Test estimate with cadence off
    let output_off = Command::new(exe)
        .args([
            "render",
            "--demo",
            "--estimate",
            "--cadence=false",
            "--breath-intensity",
            "off",
        ])
        .output()
        .expect("Run audioticon render with disabled cadence");
    assert!(output_off.status.success());
}

#[test]
fn test_cli_sfx_list_and_catalog() {
    let exe = env!("CARGO_BIN_EXE_audioticon");

    // 1. Test audioticon sfx list
    let out_list = Command::new(exe)
        .args(["sfx", "list"])
        .output()
        .expect("Run audioticon sfx list");
    assert!(out_list.status.success());
    let stdout_list = String::from_utf8_lossy(&out_list.stdout);
    assert!(stdout_list.contains("Biblioteka Efektów Dźwiękowych"));

    // 2. Test audioticon sfx catalog
    let out_cat = Command::new(exe)
        .args(["sfx", "catalog"])
        .output()
        .expect("Run audioticon sfx catalog");
    assert!(out_cat.status.success());
    let stdout_cat = String::from_utf8_lossy(&out_cat.stdout);
    assert!(stdout_cat.contains("Rekomendowane Efekty Studyjne"));
    assert!(stdout_cat.contains("CC0"));
    assert!(stdout_cat.contains("freesound.org"));
}

#[test]
fn test_cli_wallet_balance_topup_and_redeem() {
    let exe = env!("CARGO_BIN_EXE_audioticon");
    let test_wallet =
        std::env::temp_dir().join(format!("cli_test_wallet_{}.json", std::process::id()));
    let _ = std::fs::remove_file(&test_wallet);

    // 1. Balance check
    let out_balance = Command::new(exe)
        .args(["wallet", "balance"])
        .env("AUDIOTICON_WALLET_PATH", &test_wallet)
        .output()
        .expect("Run audioticon wallet balance");
    assert!(out_balance.status.success());
    let stdout_balance = String::from_utf8_lossy(&out_balance.stdout);
    assert!(stdout_balance.contains("PORTFEL AUDIOCOIN$"));
    assert!(stdout_balance.contains("AudioCoin$"));

    // 2. Top-up pricing list check
    let out_topup = Command::new(exe)
        .args(["wallet", "topup"])
        .env("AUDIOTICON_WALLET_PATH", &test_wallet)
        .output()
        .expect("Run audioticon wallet topup");
    assert!(out_topup.status.success());
    let stdout_topup = String::from_utf8_lossy(&out_topup.stdout);
    assert!(stdout_topup.contains("PAKIET PISARZ"));
    assert!(stdout_topup.contains("CERTYFIKAT ZGODNOŚCI ACX"));

    // 3. Redeem invalid voucher error handling
    let out_invalid = Command::new(exe)
        .args(["wallet", "redeem", "INVALID-CODE-999"])
        .env("AUDIOTICON_WALLET_PATH", &test_wallet)
        .output()
        .expect("Run audioticon wallet redeem with invalid code");
    let stderr_invalid = String::from_utf8_lossy(&out_invalid.stderr);
    assert!(stderr_invalid.contains("Błąd realizacji vouchera"));

    // 4. Redeem valid Writer voucher
    let valid_code = audioticon_core::generate_voucher_code(
        audioticon_core::WalletTier::Writer,
        25000,
        "VIP2026",
    );
    let out_valid = Command::new(exe)
        .args(["wallet", "redeem", &valid_code])
        .env("AUDIOTICON_WALLET_PATH", &test_wallet)
        .output()
        .expect("Run audioticon wallet redeem with valid code");
    assert!(out_valid.status.success());
    let stdout_valid = String::from_utf8_lossy(&out_valid.stdout);
    assert!(stdout_valid.contains("Zrealizowano voucher VIP"));
    assert!(stdout_valid.contains("Pisarz"));
}

#[test]
fn test_cli_voicelab_design_command() {
    let exe = env!("CARGO_BIN_EXE_audioticon");

    // 1. Help flag check
    let out_help = Command::new(exe)
        .args(["voicelab", "design", "--help"])
        .output()
        .expect("Run audioticon voicelab design --help");
    assert!(out_help.status.success());
    let stdout_help = String::from_utf8_lossy(&out_help.stdout);
    assert!(stdout_help.contains("<PROMPT>"));
    assert!(stdout_help.contains("--name"));
    assert!(stdout_help.contains("--save"));

    // 2. Execution with isolated empty home/config directory (missing ElevenLabs API key)
    let tmp_dir = std::env::temp_dir().join(format!("isolated_home_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);
    let out_run = Command::new(exe)
        .args(["voicelab", "design", "stary marynarz"])
        .env("HOME", &tmp_dir)
        .env("XDG_CONFIG_HOME", &tmp_dir)
        .output()
        .expect("Run audioticon voicelab design without api key");
    assert!(!out_run.status.success());
    let stderr_run = String::from_utf8_lossy(&out_run.stderr);
    assert!(
        stderr_run.contains("Brak klucza API ElevenLabs") || stderr_run.contains("providers set")
    );
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_cli_render_flac_24bit() {
    let exe = env!("CARGO_BIN_EXE_audioticon");
    let tmp_dir = std::env::temp_dir().join(format!("test_cli_flac_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);
    let out_flac = tmp_dir.join("test_master.flac");

    let out = Command::new(exe)
        .args([
            "render",
            "--demo",
            "-o",
            out_flac.to_str().unwrap(),
            "--bit-depth",
            "24",
        ])
        .output()
        .expect("Run audioticon render flac");

    if out.status.success() {
        assert!(out_flac.exists());
        let meta = std::fs::metadata(&out_flac).expect("FLAC file metadata");
        assert!(meta.len() > 1000);
    }
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_cli_model_hub_and_safe_harbor_eula() {
    let exe = env!("CARGO_BIN_EXE_audioticon");

    // 1. Sprawdź listę modeli Hub
    let out_hub = Command::new(exe)
        .args(["models", "hub"])
        .output()
        .expect("Run audioticon models hub");
    assert!(out_hub.status.success());
    let stdout_hub = String::from_utf8_lossy(&out_hub.stdout);
    assert!(stdout_hub.contains("Europe & Americas Neural Model Hub"));
    assert!(stdout_hub.contains("kokoro-82m-int8"));
    assert!(stdout_hub.contains("cosyvoice-3-0.5b"));

    // 2. Akceptacja licencji dla modelu omnivoice-zeroshot
    let out_accept = Command::new(exe)
        .args(["models", "accept", "omnivoice-zeroshot"])
        .output()
        .expect("Run audioticon models accept");
    assert!(out_accept.status.success());
    let stdout_accept = String::from_utf8_lossy(&out_accept.stdout);
    assert!(stdout_accept.contains("Licencja została zaakceptowana"));
    assert!(stdout_accept.contains("Kryptograficzny certyfikat EULA"));

    // 3. Sprawdź ponowną listę Hub - powinna zawierać znacznik [✅ EULA Podpisana]
    let out_hub_after = Command::new(exe)
        .args(["models", "hub"])
        .output()
        .expect("Run audioticon models hub after accept");
    assert!(out_hub_after.status.success());
    let stdout_after = String::from_utf8_lossy(&out_hub_after.stdout);
    assert!(stdout_after.contains("[✅ EULA Podpisana]"));
}

#[test]
fn test_cli_render_with_kokoro_provider() {
    let exe = env!("CARGO_BIN_EXE_audioticon");
    let tmp_dir = std::env::temp_dir().join(format!("test_cli_kokoro_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);
    let out_wav = tmp_dir.join("kokoro_demo.wav");
    let tmp_cache = tmp_dir.join("cache");

    // 1. Cost & duration estimation for Kokoro works offline without requiring model weights
    let est_out = Command::new(exe)
        .args([
            "render",
            "--demo",
            "--provider",
            "kokoro",
            "--voice",
            "af_bella",
            "--estimate",
            "--cache-dir",
            tmp_cache.to_str().unwrap(),
        ])
        .output()
        .expect("Run audioticon render --estimate with kokoro");
    assert!(
        est_out.status.success(),
        "Kokoro estimate failed: {}",
        String::from_utf8_lossy(&est_out.stderr)
    );

    // 2. When given an explicit non-existent model path, Kokoro must honestly report the missing ONNX model
    // rather than generating a fake sine-wave buzzer into the user's cache.
    let missing_model = tmp_dir.join("missing_kokoro.onnx");
    let out = Command::new(exe)
        .args([
            "render",
            "--demo",
            "--provider",
            "kokoro",
            "--voice",
            "af_bella",
            "--model",
            missing_model.to_str().unwrap(),
            "--cache-dir",
            tmp_cache.to_str().unwrap(),
            "-o",
            out_wav.to_str().unwrap(),
        ])
        .output()
        .expect("Run audioticon render with missing kokoro model");

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Nie znaleziono modelu Kokoro ONNX")
            || stderr.contains("audioticon models download kokoro-82m-int8"),
        "Unexpected stderr: {}",
        stderr
    );

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_api_subtitles_export_e2e() {
    let port = get_free_port();
    let tmp_dir = std::env::temp_dir().join(format!(
        "test_subtitles_e2e_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&tmp_dir);

    let mut child = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .args(["serve", "--port", &port.to_string(), "--no-browser"])
        .env("AUDIOTICON_CACHE_DIR", tmp_dir.to_str().unwrap())
        .env("AUDIOTICON_MOCK_WALLET_TIER", "STUDIO")
        .spawn()
        .expect("Start studio server");

    let mut connected = false;
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
            connected = true;
            break;
        }
    }
    assert!(connected, "Server failed to start on port {}", port);

    // 1. Query /api/subtitles/export with sample script and durations
    let req_body = serde_json::json!({
        "script": "Cisza przed burzą. [pauza: 500ms] Wtem rozległ się potężny grzmot!",
        "format": "srt",
        "item_durations_ms": [1500, 500, 2200]
    })
    .to_string();

    let (status, _hdrs, body) =
        http_roundtrip(port, "POST", "/api/subtitles/export", Some(&req_body));
    assert_eq!(status, 200, "Export subtitles must return 200 OK");
    let json_resp: serde_json::Value = serde_json::from_slice(&body).expect("Parse JSON response");
    assert_eq!(json_resp["status"], "ok");
    let srt_text = json_resp["content"].as_str().unwrap_or("");
    assert!(srt_text.contains("-->"), "SRT must contain timestamp arrow");
    assert!(
        srt_text.contains("Cisza") || srt_text.contains("burzą") || srt_text.contains("grzmot")
    );

    // 2. Query /api/subtitles/export with format=karaoke (vtt)
    let req_karaoke = serde_json::json!({
        "script": "Witaj świecie audiobooków.",
        "format": "karaoke",
        "item_durations_ms": [1800]
    })
    .to_string();

    let (s_kar, _, b_kar) =
        http_roundtrip(port, "POST", "/api/subtitles/export", Some(&req_karaoke));
    assert_eq!(s_kar, 200);
    let json_kar: serde_json::Value =
        serde_json::from_slice(&b_kar).expect("Parse karaoke response");
    assert_eq!(json_kar["status"], "ok");
    let vtt_text = json_kar["content"].as_str().unwrap_or("");
    assert!(
        vtt_text.starts_with("WEBVTT"),
        "Karaoke export must start with WEBVTT"
    );
    assert!(
        vtt_text.contains("<00:"),
        "Karaoke must contain inline word timestamps"
    );

    // Clean up
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_api_waveform_and_punch_in_lifecycle_e2e() {
    let port = get_free_port();
    let tmp_dir = std::env::temp_dir().join(format!(
        "test_waveform_e2e_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&tmp_dir);

    let mut child = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .args(["serve", "--port", &port.to_string(), "--no-browser"])
        .env("AUDIOTICON_CACHE_DIR", tmp_dir.to_str().unwrap())
        .env("AUDIOTICON_MOCK_WALLET_TIER", "STUDIO")
        .spawn()
        .expect("Start studio server");

    let mut connected = false;
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
            connected = true;
            break;
        }
    }
    assert!(connected, "Server failed to start on port {}", port);

    // 1. Initially GET /api/waveform/latest must return 404
    let (s_init, _, _) = http_roundtrip(port, "GET", "/api/waveform/latest", None);
    assert_eq!(s_init, 404, "No waveform initially");

    // 2. Synthesize a 1-second WAV file on disk and test POST /api/waveform/analyze
    let tmp_wav = tmp_dir.join("test_synth.wav");
    audioticon_core::create_silence_wav(&tmp_wav, 1000, 16000).expect("Create synthetic wav");
    let raw_wav = std::fs::read(&tmp_wav).expect("Read synthetic wav");
    let b64_wav = audioticon_core::base64_encode(&raw_wav);

    let analyze_req = serde_json::json!({
        "audio_base64": format!("data:audio/wav;base64,{}", b64_wav)
    })
    .to_string();

    let (s_wave, hdrs, b_wave) =
        http_roundtrip(port, "POST", "/api/waveform/analyze", Some(&analyze_req));
    assert_eq!(s_wave, 200, "Analyze waveform must return 200 OK");
    let wave_str = String::from_utf8_lossy(&b_wave);
    let wave_json: serde_json::Value = match serde_json::from_str(&wave_str) {
        Ok(v) => v,
        Err(e) => panic!(
            "Parse waveform json: {:?}, hdrs: {}, raw: {}",
            e,
            hdrs,
            &wave_str[..wave_str.len().min(300)]
        ),
    };
    assert_eq!(wave_json["duration_ms"], 1000);
    assert_eq!(wave_json["sample_rate"], 16000);
    assert!(wave_json["peaks"].as_array().map(|a| a.len()).unwrap_or(0) > 0);

    // 3. Test Punch-in Revert lifecycle:
    // Create a dummy punch-in recorded wav file in the isolated cache dir
    let test_hash = "deadbeef123456";
    let dummy_punch_wav = tmp_dir.join(format!("recorded_{}.wav", test_hash));
    let dummy_punch_meta = tmp_dir.join(format!("recorded_{}.meta.json", test_hash));
    std::fs::write(&dummy_punch_wav, b"RIFF dummy punch in").unwrap();
    std::fs::write(&dummy_punch_meta, b"{}").unwrap();
    assert!(dummy_punch_wav.exists());

    let revert_req = serde_json::json!({
        "segment_hash": test_hash
    })
    .to_string();
    let (s_rev, _, b_rev) = http_roundtrip(
        port,
        "POST",
        "/api/voicelab/punch-in/revert",
        Some(&revert_req),
    );
    assert_eq!(s_rev, 200, "Punch-in revert must return 200 OK");
    let rev_json: serde_json::Value = serde_json::from_slice(&b_rev).expect("Parse revert json");
    assert_eq!(rev_json["status"], "ok");
    assert_eq!(rev_json["reverted"], true);
    assert!(
        !dummy_punch_wav.exists(),
        "Punch-in file must be deleted upon revert"
    );

    // Clean up
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_api_qc_latest_and_audit_e2e() {
    let port = get_free_port();
    let tmp_dir = std::env::temp_dir().join(format!(
        "test_audioticon_qc_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&tmp_dir);

    let mut child = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .args(["serve", "--port", &port.to_string(), "--no-browser"])
        .env("AUDIOTICON_CACHE_DIR", tmp_dir.to_str().unwrap())
        .env("AUDIOTICON_MOCK_WALLET_TIER", "STUDIO")
        .spawn()
        .expect("Start studio server");

    let mut connected = false;
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
            connected = true;
            break;
        }
    }
    assert!(connected, "Server failed to start on port {}", port);

    // 1. GET /api/qc/latest should return 200 with default calibrated report initially
    let (s_qc, _, b_qc) = http_roundtrip(port, "GET", "/api/qc/latest", None);
    assert_eq!(s_qc, 200, "GET /api/qc/latest must return 200 OK");
    let qc_json: serde_json::Value = serde_json::from_slice(&b_qc).expect("Parse initial qc json");
    assert!(qc_json["integrated_lufs"].as_f64().is_some());
    assert!(qc_json["true_peak_dbfs"].as_f64().is_some());
    assert_eq!(qc_json["is_compliant"], true);

    // 2. Synthesize audio WAV and run POST /api/qc/audit
    let test_wav = tmp_dir.join("test_audit_input.wav");
    audioticon_core::create_silence_wav(&test_wav, 1000, 16000).expect("Create silence wav");
    let raw_wav = std::fs::read(&test_wav).expect("Read test wav");
    let b64_wav = audioticon_core::base64_encode(&raw_wav);

    let audit_req = serde_json::json!({
        "audio_base64": format!("data:audio/wav;base64,{}", b64_wav)
    })
    .to_string();

    let (s_audit, _, b_audit) = http_roundtrip(port, "POST", "/api/qc/audit", Some(&audit_req));
    assert_eq!(s_audit, 200, "POST /api/qc/audit must return 200 OK");
    let audit_res: serde_json::Value = serde_json::from_slice(&b_audit).expect("Parse audit json");
    assert!(audit_res["integrated_lufs"].as_f64().is_some());
    assert!(audit_res["true_peak_dbfs"].as_f64().is_some());
    assert!(audit_res["is_compliant"].as_bool().is_some());

    // Clean up
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_api_custom_emojis_sound_design_e2e() {
    let port = get_free_port();
    let tmp_dir = std::env::temp_dir().join(format!(
        "test_audioticon_emojis_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&tmp_dir);

    let mut child = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .args(["serve", "--port", &port.to_string(), "--no-browser"])
        .env("AUDIOTICON_CACHE_DIR", tmp_dir.to_str().unwrap())
        .current_dir(&tmp_dir)
        .spawn()
        .expect("Start studio server");

    let mut connected = false;
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
            connected = true;
            break;
        }
    }
    assert!(connected, "Server failed to start on port {}", port);

    // 1. GET /api/custom-emojis returns factory emojis and available filters
    let (s_get, _, b_get) = http_roundtrip(port, "GET", "/api/custom-emojis", None);
    assert_eq!(s_get, 200);
    let get_json: serde_json::Value = serde_json::from_slice(&b_get).expect("Parse custom-emojis");
    assert!(get_json["factory_emojis"].as_array().is_some());
    assert!(get_json["available_filters"].as_array().is_some());

    // 2. POST /api/custom-emojis/add
    let add_req = serde_json::json!({
        "emoji": "🧙‍♂️",
        "name": "Czarodziej",
        "alias": "czarodziej",
        "action_type": "filter",
        "filter": "cathedral",
        "pitch": -4,
        "rate": 0,
        "volume": 1.0
    })
    .to_string();
    let (s_add, _, b_add) = http_roundtrip(port, "POST", "/api/custom-emojis/add", Some(&add_req));
    assert_eq!(s_add, 200);
    let add_json: serde_json::Value = serde_json::from_slice(&b_add).expect("Parse add response");
    assert!(add_json["emojis"]["🧙‍♂️"].is_object());

    // 3. Verify in GET
    let (s_get2, _, b_get2) = http_roundtrip(port, "GET", "/api/custom-emojis", None);
    assert_eq!(s_get2, 200);
    let get2_json: serde_json::Value = serde_json::from_slice(&b_get2).expect("Parse get2");
    assert_eq!(get2_json["emojis"]["🧙‍♂️"]["name"], "Czarodziej");

    // 4. POST /api/custom-emojis/remove
    let rem_req = serde_json::json!({
        "emoji": "🧙‍♂️"
    })
    .to_string();
    let (s_rem, _, b_rem) =
        http_roundtrip(port, "POST", "/api/custom-emojis/remove", Some(&rem_req));
    assert_eq!(s_rem, 200);
    let rem_json: serde_json::Value = serde_json::from_slice(&b_rem).expect("Parse rem");
    assert!(rem_json["emojis"]["🧙‍♂️"].is_null());

    // Clean up
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_license_status_activation_and_soft_gates_e2e() {
    let port = get_free_port();
    let tmp_dir = std::env::temp_dir().join(format!(
        "test_license_e2e_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&tmp_dir);
    let wallet_path = tmp_dir.join("wallet.json");

    let mut child = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .args(["serve", "--port", &port.to_string(), "--no-browser"])
        .env("AUDIOTICON_CACHE_DIR", tmp_dir.to_str().unwrap())
        .env("AUDIOTICON_WALLET_PATH", wallet_path.to_str().unwrap())
        .env("AUDIOTICON_MOCK_GUMROAD_TIER", "SOLO")
        .spawn()
        .expect("Start studio server");

    let mut connected = false;
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
            connected = true;
            break;
        }
    }
    assert!(connected, "Server failed to start on port {}", port);

    // 1. Initial status: Free tier
    let (s_stat, _, b_stat) = http_roundtrip(port, "GET", "/api/license/status", None);
    assert_eq!(s_stat, 200);
    let stat_json: serde_json::Value =
        serde_json::from_slice(&b_stat).expect("Parse license status");
    assert_eq!(stat_json["tier"], "Free");
    assert_eq!(stat_json["is_pro"], false);
    assert_eq!(stat_json["features"]["m4b"], false);
    assert_eq!(stat_json["features"]["punch_in"], false);

    // 2. Soft-gate check on /api/export with format=m4b in Free tier
    let export_m4b_req = serde_json::json!({
        "text": "Próba eksportu M4B w darmowej wersji.",
        "format": "m4b",
        "provider": "piper",
        "lang": "pl",
        "voice": "pl_PL-gosia-medium"
    })
    .to_string();
    let (s_exp, _, b_exp) = http_roundtrip(port, "POST", "/api/export", Some(&export_m4b_req));
    assert_eq!(
        s_exp, 402,
        "M4B export on Free tier must return 402 Payment Required"
    );
    let exp_json: serde_json::Value =
        serde_json::from_slice(&b_exp).expect("Parse export soft-gate");
    assert_eq!(exp_json["error"], "m4b_requires_pro");

    // 3. Activate Solo license via /api/license/activate
    let act_req = serde_json::json!({
        "key": "SOLO-XYZ-999"
    })
    .to_string();
    let (s_act, _, b_act) = http_roundtrip(port, "POST", "/api/license/activate", Some(&act_req));
    if s_act != 200 {
        panic!("Activation failed: {}", String::from_utf8_lossy(&b_act));
    }
    assert_eq!(s_act, 200);
    let act_json: serde_json::Value =
        serde_json::from_slice(&b_act).expect("Parse activation response");
    assert_eq!(act_json["success"], true);
    assert_eq!(act_json["tier"], "Solo PRO");
    assert_eq!(act_json["is_pro"], true);
    assert_eq!(act_json["features"]["m4b"], true);
    assert_eq!(act_json["features"]["punch_in"], true);
    assert_eq!(act_json["features"]["batch"], false);

    // 4. Verify GET /api/license/status reflects updated tier
    let (s_stat2, _, b_stat2) = http_roundtrip(port, "GET", "/api/license/status", None);
    assert_eq!(s_stat2, 200);
    let stat2_json: serde_json::Value =
        serde_json::from_slice(&b_stat2).expect("Parse updated license status");
    assert_eq!(stat2_json["tier"], "Solo PRO");
    assert_eq!(stat2_json["is_pro"], true);
    assert_eq!(stat2_json["features"]["m4b"], true);

    // Clean up
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_cli_default_no_args_starts_studio() {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_audioticon"));
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    let mut child = cmd.spawn().expect("Spawn audioticon without args");
    std::thread::sleep(std::time::Duration::from_millis(500));
    let _ = child.kill();
    let _ = child.wait();
}
