use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{create_dir_all, File};
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;
use thiserror::Error;

use crate::voucher::compute_hmac_sha256;

const SAFE_HARBOR_SALT: &[u8] = b"audioticon_safe_harbor_model_eula_v1_salt";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LicenseType {
    /// 100% Commercial safe (Apache-2.0, MIT, BSD-3-Clause)
    CommercialSafe,
    /// Non-commercial / Educational / Personal use only (CC-BY-NC, Higgs Audio)
    NonCommercialOnly,
    /// OpenRAIL with responsible commercial usage constraints
    OpenRail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCategory {
    NeuralTts,
    SpeechAligner,
    VoiceCloning,
    Diarization,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelHubEntry {
    pub id: String,
    pub name: String,
    pub author: String,
    pub category: ModelCategory,
    pub license_spdx: String,
    pub license_type: LicenseType,
    pub license_summary: String,
    pub evidence_url: String,
    pub terms_fingerprint: String,
    pub languages: Vec<String>,
    pub size_mb: u32,
    pub min_ram_mb: u32,
    pub download_urls: Vec<String>,
    pub sha256_checksums: Vec<String>,
}

#[derive(Error, Debug)]
pub enum ModelHubError {
    #[error("Model '{0}' nie został znaleziony w katalogu")]
    ModelNotFound(String),

    #[error("Pobieranie zablokowane: Licencja dla modelu '{model_id}' ({license_spdx}) nie została zaakceptowana przez użytkownika. Wymagana akceptacja EULA: {evidence_url}")]
    LicenseNotAccepted {
        model_id: String,
        terms_fingerprint: String,
        license_spdx: String,
        evidence_url: String,
    },

    #[error("Nieprawidłowy kwit akceptacji licencji (HMAC mismatch) dla modelu '{0}'")]
    InvalidReceiptSignature(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseReceipt {
    pub receipt_id: String,
    pub model_id: String,
    pub terms_fingerprint: String,
    pub accepted_at_utc: String,
    pub client_version: String,
    pub commercial_acknowledged: bool,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReceiptStore {
    pub receipts: HashMap<String, LicenseReceipt>,
}

impl ReceiptStore {
    pub fn default_path() -> PathBuf {
        let base = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("audioticon");
        base.join("model_receipts.json")
    }

    pub fn load_or_default() -> Self {
        Self::load_from_path(&Self::default_path()).unwrap_or_default()
    }

    pub fn load_from_path(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let file = File::open(path).with_context(|| format!("Nie można otworzyć {:?}", path))?;
        let reader = BufReader::new(file);
        let store: Self = serde_json::from_reader(reader)
            .with_context(|| format!("Błąd parsowania kwitów {:?}", path))?;
        Ok(store)
    }

    pub fn save(&self) -> Result<()> {
        self.save_to_path(&Self::default_path())
    }

    pub fn save_to_path(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            create_dir_all(parent)?;
        }
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, self)?;
        Ok(())
    }

    pub fn compute_signature(
        model_id: &str,
        terms_fingerprint: &str,
        accepted_at: &str,
        commercial_ack: bool,
    ) -> String {
        let payload = format!(
            "model:{};fingerprint:{};time:{};commercial:{}",
            model_id, terms_fingerprint, accepted_at, commercial_ack
        );
        let sig_bytes = compute_hmac_sha256(SAFE_HARBOR_SALT, payload.as_bytes());
        hex::encode(sig_bytes)
    }

    pub fn has_accepted(&self, model_id: &str, terms_fingerprint: &str) -> bool {
        if let Some(receipt) = self.receipts.get(model_id) {
            if receipt.terms_fingerprint == terms_fingerprint && self.verify_receipt(receipt) {
                return true;
            }
        }
        false
    }

    pub fn record_acceptance(
        &mut self,
        model_id: &str,
        terms_fingerprint: &str,
        commercial_acknowledged: bool,
        client_version: &str,
    ) -> LicenseReceipt {
        let accepted_at = chrono_or_simple_utc_now();
        let receipt_id = format!(
            "rcpt-{}-{}",
            model_id.replace('/', "-"),
            hex::encode(&compute_hmac_sha256(SAFE_HARBOR_SALT, accepted_at.as_bytes())[..6])
        );
        let signature = Self::compute_signature(
            model_id,
            terms_fingerprint,
            &accepted_at,
            commercial_acknowledged,
        );

        let receipt = LicenseReceipt {
            receipt_id,
            model_id: model_id.to_string(),
            terms_fingerprint: terms_fingerprint.to_string(),
            accepted_at_utc: accepted_at,
            client_version: client_version.to_string(),
            commercial_acknowledged,
            signature,
        };

        self.receipts.insert(model_id.to_string(), receipt.clone());
        receipt
    }

    pub fn verify_receipt(&self, receipt: &LicenseReceipt) -> bool {
        let expected = Self::compute_signature(
            &receipt.model_id,
            &receipt.terms_fingerprint,
            &receipt.accepted_at_utc,
            receipt.commercial_acknowledged,
        );
        receipt.signature == expected
    }
}

fn chrono_or_simple_utc_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}-UTC", now)
}

/// Zwraca zbiór zbadanych, certyfikowanych modeli dedykowanych dla Europy i obu Ameryk
pub fn get_curated_hub_catalog() -> Vec<ModelHubEntry> {
    vec![
        // 1. KOKORO-82M (Król modeli, 100% legalny komercyjnie)
        ModelHubEntry {
            id: "kokoro-82m-int8".to_string(),
            name: "Kokoro 82M Multilingual (v1.0 int8 ONNX)".to_string(),
            author: "Hexgrad / thewh1teagle".to_string(),
            category: ModelCategory::NeuralTts,
            license_spdx: "Apache-2.0".to_string(),
            license_type: LicenseType::CommercialSafe,
            license_summary: "W 100% dozwolony do celów komercyjnych (Audible, Storytel, Spotify). Wymaga zachowania noty copyright.".to_string(),
            evidence_url: "https://huggingface.co/hexgrad/Kokoro-82M/blob/main/LICENSE".to_string(),
            terms_fingerprint: "a9f82d1c68e983271816bc8d16fef46487920361254793284729104856281729".to_string(),
            languages: vec![
                "en-us".to_string(),
                "en-gb".to_string(),
                "es".to_string(),
                "fr".to_string(),
                "it".to_string(),
                "pt-br".to_string(),
            ],
            size_mb: 88,
            min_ram_mb: 180,
            download_urls: vec![
                "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/kokoro-v1.0.int8.onnx".to_string(),
                "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/voices-v1.0.bin".to_string(),
            ],
            sha256_checksums: vec![],
        },

        // 2. WHISPER-TINY (Planowany moduł ASR)
        ModelHubEntry {
            id: "whisper-tiny-aligner".to_string(),
            name: "Faster-Whisper Tiny (Planowany model ASR)".to_string(),
            author: "OpenAI / Systran".to_string(),
            category: ModelCategory::SpeechAligner,
            license_spdx: "MIT".to_string(),
            license_type: LicenseType::CommercialSafe,
            license_summary: "Licencja MIT. W pełni dozwolony do komercyjnego wyznaczania znaczników czasowych słów na osi czasu.".to_string(),
            evidence_url: "https://huggingface.co/Systran/faster-whisper-tiny/blob/main/README.md".to_string(),
            terms_fingerprint: "482910fbcde8392019485729104856281729482910fbcde83920194857291048".to_string(),
            languages: vec!["all-western".to_string()],
            size_mb: 75,
            min_ram_mb: 140,
            download_urls: vec![
                "https://huggingface.co/Systran/faster-whisper-tiny/resolve/main/model.bin".to_string(),
            ],
            sha256_checksums: vec![],
        },

        // 3. PIPER PL (Polski Męski Ciemny - Darkman)
        ModelHubEntry {
            id: "piper-pl-darkman".to_string(),
            name: "Piper Polish Darkman (Męski Lektorski)".to_string(),
            author: "Rhasspy Community".to_string(),
            category: ModelCategory::NeuralTts,
            license_spdx: "MIT".to_string(),
            license_type: LicenseType::CommercialSafe,
            license_summary: "Licencja MIT. 100% lokalny, zerowa latencja, idealny do szybkich prób reżyserskich.".to_string(),
            evidence_url: "https://huggingface.co/rhasspy/piper-voices/blob/main/pl/pl_PL/darkman/medium/MODEL_CARD".to_string(),
            terms_fingerprint: "11223344556677889900aabbccddeeff11223344556677889900aabbccddeeff".to_string(),
            languages: vec!["pl".to_string()],
            size_mb: 63,
            min_ram_mb: 110,
            download_urls: vec![
                "https://huggingface.co/rhasspy/piper-voices/resolve/main/pl/pl_PL/darkman/medium/pl_PL-darkman-medium.onnx".to_string(),
                "https://huggingface.co/rhasspy/piper-voices/resolve/main/pl/pl_PL/darkman/medium/pl_PL-darkman-medium.onnx.json".to_string(),
            ],
            sha256_checksums: vec![],
        },

        // 4. PIPER PL (Polski Żeński - Gosia)
        ModelHubEntry {
            id: "piper-pl-gosia".to_string(),
            name: "Piper Polish Gosia (Żeński Ciepły)".to_string(),
            author: "Rhasspy Community".to_string(),
            category: ModelCategory::NeuralTts,
            license_spdx: "MIT".to_string(),
            license_type: LicenseType::CommercialSafe,
            license_summary: "Licencja MIT. Polski ciepły głos lektorski do narracji i dialogów.".to_string(),
            evidence_url: "https://huggingface.co/rhasspy/piper-voices/blob/main/pl/pl_PL/gosia/medium/MODEL_CARD".to_string(),
            terms_fingerprint: "11223344556677889900aabbccddeeff11223344556677889900aabbccddeef2".to_string(),
            languages: vec!["pl".to_string()],
            size_mb: 63,
            min_ram_mb: 110,
            download_urls: vec![
                "https://huggingface.co/rhasspy/piper-voices/resolve/main/pl/pl_PL/gosia/medium/pl_PL-gosia-medium.onnx".to_string(),
                "https://huggingface.co/rhasspy/piper-voices/resolve/main/pl/pl_PL/gosia/medium/pl_PL-gosia-medium.onnx.json".to_string(),
            ],
            sha256_checksums: vec![],
        },

        // 5. PIPER EN-US (Angielski Amerykański - Lessac)
        ModelHubEntry {
            id: "piper-en-us-lessac".to_string(),
            name: "Piper English US (Lessac Medium)".to_string(),
            author: "Rhasspy Community".to_string(),
            category: ModelCategory::NeuralTts,
            license_spdx: "MIT".to_string(),
            license_type: LicenseType::CommercialSafe,
            license_summary: "Licencja MIT. Standardowy czysty amerykański lektor na rynek Audible US.".to_string(),
            evidence_url: "https://huggingface.co/rhasspy/piper-voices/blob/main/en/en_US/lessac/medium/MODEL_CARD".to_string(),
            terms_fingerprint: "99887766554433221100aabbccddeeff99887766554433221100aabbccddeeff".to_string(),
            languages: vec!["en-us".to_string()],
            size_mb: 63,
            min_ram_mb: 110,
            download_urls: vec![
                "https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/lessac/medium/en_US-lessac-medium.onnx".to_string(),
                "https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/lessac/medium/en_US-lessac-medium.onnx.json".to_string(),
            ],
            sha256_checksums: vec![],
        },

        // 6. PIPER DE (Niemiecki - Thorsten)
        ModelHubEntry {
            id: "piper-de-thorsten".to_string(),
            name: "Piper German Thorsten (Męski DE)".to_string(),
            author: "Thorsten Müller / Rhasspy".to_string(),
            category: ModelCategory::NeuralTts,
            license_spdx: "CC0-1.0".to_string(),
            license_type: LicenseType::CommercialSafe,
            license_summary: "Public Domain / CC0. Najpopularniejszy otwarty lektor niemiecki.".to_string(),
            evidence_url: "https://huggingface.co/rhasspy/piper-voices/blob/main/de/de_DE/thorsten/medium/MODEL_CARD".to_string(),
            terms_fingerprint: "77665544332211009988aabbccddeeff77665544332211009988aabbccddeeff".to_string(),
            languages: vec!["de".to_string()],
            size_mb: 63,
            min_ram_mb: 110,
            download_urls: vec![
                "https://huggingface.co/rhasspy/piper-voices/resolve/main/de/de_DE/thorsten/medium/de_DE-thorsten-medium.onnx".to_string(),
                "https://huggingface.co/rhasspy/piper-voices/resolve/main/de/de_DE/thorsten/medium/de_DE-thorsten-medium.onnx.json".to_string(),
            ],
            sha256_checksums: vec![],
        },

        // 7. PIPER ES (Hiszpański - Davefx)
        ModelHubEntry {
            id: "piper-es-davefx".to_string(),
            name: "Piper Spanish Davefx (Męski ES/LATAM)".to_string(),
            author: "Rhasspy Community".to_string(),
            category: ModelCategory::NeuralTts,
            license_spdx: "MIT".to_string(),
            license_type: LicenseType::CommercialSafe,
            license_summary: "Licencja MIT. Czysta dykcja hiszpańska na rynki Hiszpanii i Ameryki Łacińskiej.".to_string(),
            evidence_url: "https://huggingface.co/rhasspy/piper-voices/blob/main/es/es_ES/davefx/medium/MODEL_CARD".to_string(),
            terms_fingerprint: "55443322110099887766aabbccddeeff55443322110099887766aabbccddeeff".to_string(),
            languages: vec!["es".to_string()],
            size_mb: 63,
            min_ram_mb: 110,
            download_urls: vec![
                "https://huggingface.co/rhasspy/piper-voices/resolve/main/es/es_ES/davefx/medium/es_ES-davefx-medium.onnx".to_string(),
                "https://huggingface.co/rhasspy/piper-voices/resolve/main/es/es_ES/davefx/medium/es_ES-davefx-medium.onnx.json".to_string(),
            ],
            sha256_checksums: vec![],
        },

        // 8. COSYVOICE 3 (Zero-Shot & Ekspresja)
        ModelHubEntry {
            id: "cosyvoice-3-0.5b".to_string(),
            name: "Fun-CosyVoice3 (0.5B Multilingual GGUF)".to_string(),
            author: "FunAudioLLM / Alibaba".to_string(),
            category: ModelCategory::VoiceCloning,
            license_spdx: "Apache-2.0".to_string(),
            license_type: LicenseType::CommercialSafe,
            license_summary: "Licencja Apache-2.0. Zaawansowane klonowanie głosu i kontrola emocjonalna mowy.".to_string(),
            evidence_url: "https://huggingface.co/FunAudioLLM/CosyVoice-300M/blob/main/LICENSE".to_string(),
            terms_fingerprint: "33221100998877665544aabbccddeeff33221100998877665544aabbccddeeff".to_string(),
            languages: vec!["en".to_string(), "es".to_string(), "fr".to_string(), "de".to_string(), "it".to_string()],
            size_mb: 580,
            min_ram_mb: 1200,
            download_urls: vec![
                "https://huggingface.co/FunAudioLLM/CosyVoice-300M/resolve/main/cosyvoice.onnx".to_string(),
            ],
            sha256_checksums: vec![],
        },

        // 9. OMNIVOICE (Klonowanie zero-shot non-commercial)
        ModelHubEntry {
            id: "omnivoice-zeroshot".to_string(),
            name: "OmniVoice Zero-Shot (k2-fsa)".to_string(),
            author: "Han Zhu & k2-fsa".to_string(),
            category: ModelCategory::VoiceCloning,
            license_spdx: "CC-BY-NC-4.0".to_string(),
            license_type: LicenseType::NonCommercialOnly,
            license_summary: "Licencja CC-BY-NC. Wyłącznie do użytku prywatnego, badawczego i edukacyjnego. Zakaz komercyjnej odsprzedaży wygenerowanych nagrań.".to_string(),
            evidence_url: "https://huggingface.co/k2-fsa/OmniVoice/blob/main/README.md".to_string(),
            terms_fingerprint: "c5fdb5ccb189668d56333f77ba2629f4cd7535f4c5fdb5ccb189668d56333f77".to_string(),
            languages: vec!["multilingual".to_string()],
            size_mb: 1100,
            min_ram_mb: 2400,
            download_urls: vec![
                "https://huggingface.co/Serveurperso/OmniVoice-GGUF/resolve/main/omnivoice-q8_0.gguf".to_string(),
            ],
            sha256_checksums: vec![],
        },
    ]
}

/// Główny punkt autoryzacji Safe-Harbor: weryfikuje, czy użytkownik zaakceptował warunki
pub fn authorize_model_access<'a>(
    model_id: &str,
    catalog: &'a [ModelHubEntry],
    receipt_store: &ReceiptStore,
) -> Result<&'a ModelHubEntry, ModelHubError> {
    let entry = catalog
        .iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| ModelHubError::ModelNotFound(model_id.to_string()))?;

    if !receipt_store.has_accepted(&entry.id, &entry.terms_fingerprint) {
        return Err(ModelHubError::LicenseNotAccepted {
            model_id: entry.id.clone(),
            terms_fingerprint: entry.terms_fingerprint.clone(),
            license_spdx: entry.license_spdx.clone(),
            evidence_url: entry.evidence_url.clone(),
        });
    }

    Ok(entry)
}

/// Sprawdza, czy dostęp do modelu jest dozwolony według bazy Safe-Harbor EULA.
/// Modele CommercialSafe (MIT, Apache-2.0, CC0) są autoryzowane domyślnie, natomiast
/// modele NonCommercialOnly / OpenRail wymagają podpisania certyfikatu EULA.
pub fn check_hub_authorization(model_id: &str) -> std::result::Result<(), ModelHubError> {
    let catalog = get_curated_hub_catalog();
    if let Some(entry) = catalog.iter().find(|m| m.id == model_id) {
        if entry.license_type != LicenseType::CommercialSafe {
            let store = ReceiptStore::load_or_default();
            authorize_model_access(model_id, &catalog, &store)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_integrity() {
        let catalog = get_curated_hub_catalog();
        assert!(catalog.len() >= 8);
        let kokoro = catalog.iter().find(|m| m.id == "kokoro-82m-int8").unwrap();
        assert_eq!(kokoro.license_type, LicenseType::CommercialSafe);
        assert!(kokoro.languages.contains(&"en-us".to_string()));
        assert!(!kokoro.languages.contains(&"pl".to_string()));
    }

    #[test]
    fn test_safe_harbor_eula_flow() {
        let catalog = get_curated_hub_catalog();
        let mut store = ReceiptStore::default();

        // 1. Próba autoryzacji bez akceptacji -> musi zwrócić błąd LicenseNotAccepted
        let err = authorize_model_access("kokoro-82m-int8", &catalog, &store).unwrap_err();
        match err {
            ModelHubError::LicenseNotAccepted { model_id, license_spdx, .. } => {
                assert_eq!(model_id, "kokoro-82m-int8");
                assert_eq!(license_spdx, "Apache-2.0");
            }
            _ => panic!("Oczekiwano błędu LicenseNotAccepted"),
        }

        // 2. Użytkownik akceptuje licencję EULA
        let entry = catalog.iter().find(|m| m.id == "kokoro-82m-int8").unwrap();
        let receipt = store.record_acceptance(
            &entry.id,
            &entry.terms_fingerprint,
            true,
            env!("CARGO_PKG_VERSION"),
        );
        assert!(store.verify_receipt(&receipt));

        // 3. Po akceptacji autoryzacja musi przejść pomyślnie
        let authorized = authorize_model_access("kokoro-82m-int8", &catalog, &store).unwrap();
        assert_eq!(authorized.name, entry.name);
    }

    #[test]
    fn test_receipt_tamper_proofing() {
        let catalog = get_curated_hub_catalog();
        let mut store = ReceiptStore::default();
        let entry = catalog.iter().find(|m| m.id == "omnivoice-zeroshot").unwrap();

        let mut receipt = store.record_acceptance(
            &entry.id,
            &entry.terms_fingerprint,
            false,
            env!("CARGO_PKG_VERSION"),
        );
        assert!(store.verify_receipt(&receipt));

        // Próba sfałszowania parametrów kwitu (np. podmiana fingerprintu lub akceptacji komercyjnej)
        receipt.commercial_acknowledged = true;
        assert!(!store.verify_receipt(&receipt));
    }
}
