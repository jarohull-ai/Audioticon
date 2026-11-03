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
        res[header_end + 4..].to_vec()
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
    let mut child = Command::new(env!("CARGO_BIN_EXE_audioticon"))
        .args(["serve", "--port", &port.to_string(), "--no-browser"])
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

    // 5. POST /api/export with format=m4b: tests full M4B generation if model is present,
    // or validates proper 400 model guidance when running in CI without offline models.
    let export_req = "{\"text\": \"📖 Rozdział 1: Test E2E 🛑 Krótkie zdanie testowe.\", \"format\": \"m4b\", \"title\": \"Test E2E Audiobook\", \"author\": \"Jarosław Kuchta\", \"provider\": \"piper\"}";
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

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn test_api_cache_status_disk_verification() {
    let port = 30000 + (std::process::id() % 10000) as u16 + 14;
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
    // 1. Set mock API key
    let set_out = Command::new(env!("CARGO_BIN_EXE_audioticon"))
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
        .args(["providers", "remove", "elevenlabs"])
        .output()
        .expect("Failed to execute audioticon providers remove");
    assert!(rem_out.status.success());
    let stdout_rem = String::from_utf8_lossy(&rem_out.stdout);
    assert!(stdout_rem.contains("Usunięto konfigurację dostawcy 'elevenlabs'"));
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
        res[header_end + 4..].to_vec()
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
    let port = 39000 + (std::process::id() as u16 % 1000);
    let mut child = Command::new(env!("CARGO_BIN_EXE_audioticon"))
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
    let _ = audioticon_core::remove_api_key("elevenlabs");
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
