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
