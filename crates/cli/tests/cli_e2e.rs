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
