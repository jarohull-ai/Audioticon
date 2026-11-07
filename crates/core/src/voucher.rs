use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Domain-separated fallback verification key when `AUDIOTICON_LICENSE_SECRET` is not set by a vendor build
pub const VOUCHER_HMAC_SECRET: &[u8] = b"AUDIOTICON_STUDIO_LICENSE_KEY_V2_DERIVATION_SALT";

/// Returns the active license verification key.
/// Production vendor builds or license servers can inject a private secret via `AUDIOTICON_LICENSE_SECRET`.
pub fn active_license_secret() -> Vec<u8> {
    if let Ok(env_secret) = std::env::var("AUDIOTICON_LICENSE_SECRET") {
        let trimmed = env_secret.trim();
        if !trimmed.is_empty() {
            return trimmed.as_bytes().to_vec();
        }
    }
    VOUCHER_HMAC_SECRET.to_vec()
}

/// Tier of unlocked wallet / Studio workstation features
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum WalletTier {
    Free,
    Debut,
    Solo,
    Writer,
    Studio,
    Publisher,
}

impl WalletTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::Debut => "Debiutant",
            Self::Solo => "Solo PRO",
            Self::Writer => "Pisarz",
            Self::Studio => "Studio PRO",
            Self::Publisher => "Wydawca",
        }
    }

    pub fn name(&self) -> &'static str {
        self.as_str()
    }

    pub fn is_pro(&self) -> bool {
        matches!(self, Self::Solo | Self::Writer | Self::Studio | Self::Publisher)
    }

    pub fn is_studio(&self) -> bool {
        matches!(self, Self::Studio | Self::Publisher)
    }

    pub fn has_acx_certificate(&self) -> bool {
        self.is_pro()
    }

    pub fn has_m4b_export(&self) -> bool {
        self.is_pro()
    }

    pub fn has_punch_in(&self) -> bool {
        self.is_pro()
    }

    pub fn has_batch_pipeline(&self) -> bool {
        self.is_studio()
    }

    pub fn has_dataset_export(&self) -> bool {
        self.is_studio()
    }
}

/// Information returned after successfully validating a voucher / license code
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoucherPayload {
    pub code: String,
    pub tier: WalletTier,
    pub credits: u64,
    pub acx_certificate_unlocked: bool,
}

/// Standard HMAC-SHA256 implementation (RFC 2104) using `sha2::Sha256`
pub fn compute_hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut key_block = [0u8; 64];
    if key.len() > 64 {
        let hash = Sha256::digest(key);
        key_block[..32].copy_from_slice(&hash);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= key_block[i];
        opad[i] ^= key_block[i];
    }

    // Inner hash: H(ipad || message)
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(message);
    let inner_hash = inner.finalize();

    // Outer hash: H(opad || inner_hash)
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_hash);
    let result = outer.finalize();

    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

fn sanitize_serial(nonce: &str) -> String {
    let cleaned: String = nonce
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_uppercase();
    if cleaned.is_empty() {
        "S2026".to_string()
    } else {
        cleaned
    }
}

/// Generates a signed cryptographic license / voucher code with an explicit serial ID embedded in the code.
/// Format: `AUD-<TIER>-<CREDITS>-<SERIAL>-<SIGNATURE_12_CHARS>`
pub fn generate_voucher_code(tier: WalletTier, credits: u64, nonce: &str) -> String {
    let tier_tag = match tier {
        WalletTier::Free => "FREE",
        WalletTier::Debut => "DEBUT",
        WalletTier::Solo => "SOLO",
        WalletTier::Writer => "WRITER",
        WalletTier::Studio => "STUDIO",
        WalletTier::Publisher => "PUB",
    };

    let serial = sanitize_serial(nonce);
    let secret = active_license_secret();
    let payload = format!("AUDIOTICON_V2:{}:{}:{}", tier_tag, credits, serial);
    let sig = compute_hmac_sha256(&secret, payload.as_bytes());
    let sig_hex = hex::encode(&sig[..6]).to_uppercase(); // 12-char hex signature

    format!("AUD-{}-{}-{}-{}", tier_tag, credits, serial, sig_hex)
}

/// Verifies and decodes a cryptographic license / voucher code.
/// Rejects any code whose embedded serial and signature do not match `active_license_secret()`.
pub fn verify_voucher_code(code: &str) -> Result<VoucherPayload> {
    let clean = code.trim().to_uppercase();
    let parts: Vec<&str> = clean.split('-').collect();

    if parts.len() != 5 || parts[0] != "AUD" {
        bail!(
            "Nieprawidłowy format vouchera. Prawidłowy format to AUD-<PAKIET>-<MONETY>-<SERIA>-<PODPIS>"
        );
    }

    let tier_str = parts[1];
    let credits_str = parts[2];
    let serial_str = parts[3];
    let given_sig = parts[4];

    if serial_str.is_empty() || !serial_str.chars().all(|c| c.is_ascii_alphanumeric()) {
        bail!("Nieprawidłowy identyfikator serii w kodzie vouchera.");
    }

    let (tier, canonical_tier_tag) = match tier_str {
        "FREE" => (WalletTier::Free, "FREE"),
        "DEBUT" => (WalletTier::Debut, "DEBUT"),
        "SOLO" => (WalletTier::Solo, "SOLO"),
        "WRITER" => (WalletTier::Writer, "WRITER"),
        "STUDIO" => (WalletTier::Studio, "STUDIO"),
        "PUB" | "PUBLISHER" => (WalletTier::Publisher, "PUB"),
        _ => bail!("Nieznana kategoria vouchera: {}", tier_str),
    };

    let credits: u64 = credits_str
        .parse()
        .map_err(|_| anyhow::anyhow!("Nieprawidłowa liczba kredytów w voucherze: {}", credits_str))?;

    let secret = active_license_secret();
    let payload = format!(
        "AUDIOTICON_V2:{}:{}:{}",
        canonical_tier_tag, credits, serial_str
    );
    let expected_sig = compute_hmac_sha256(&secret, payload.as_bytes());
    let expected_hex = hex::encode(&expected_sig[..6]).to_uppercase();

    if given_sig != expected_hex {
        bail!("Voucher ma nieprawidłowy podpis kryptograficzny lub został zmodyfikowany.");
    }

    Ok(VoucherPayload {
        code: clean,
        tier,
        credits,
        acx_certificate_unlocked: tier.has_acx_certificate(),
    })
}

/// Creates a signed local Gumroad token for offline integrity and wallet persistence
pub fn create_gumroad_token(tier: WalletTier, license_key: &str) -> String {
    let tier_tag = match tier {
        WalletTier::Studio | WalletTier::Publisher => "STUDIO",
        _ => "SOLO",
    };
    let clean_key = license_key.trim();
    let secret = active_license_secret();
    let payload = format!("AUDIOTICON_GUMROAD:{}:{}", tier_tag, clean_key);
    let sig = compute_hmac_sha256(&secret, payload.as_bytes());
    let sig_hex = hex::encode(&sig[..6]).to_uppercase();
    format!("GUMROAD-{}-{}-{}", tier_tag, clean_key, sig_hex)
}

/// Verifies and decodes a local signed Gumroad token offline
pub fn verify_gumroad_token(token: &str) -> Result<VoucherPayload> {
    let clean = token.trim();
    let parts: Vec<&str> = clean.split('-').collect();
    if parts.len() < 4 || parts[0] != "GUMROAD" {
        bail!("Nieprawidłowy format tokenu licencji Gumroad.");
    }
    let tier_tag = parts[1];
    let sig = parts[parts.len() - 1];
    let license_key = parts[2..parts.len() - 1].join("-");

    let tier = match tier_tag {
        "STUDIO" => WalletTier::Studio,
        "SOLO" => WalletTier::Solo,
        _ => bail!("Nieznana kategoria licencji Gumroad: {}", tier_tag),
    };

    let secret = active_license_secret();
    let payload = format!("AUDIOTICON_GUMROAD:{}:{}", tier_tag, license_key);
    let expected_sig = compute_hmac_sha256(&secret, payload.as_bytes());
    let expected_hex = hex::encode(&expected_sig[..6]).to_uppercase();

    if sig != expected_hex {
        bail!("Token licencji Gumroad ma nieprawidłowy podpis kryptograficzny.");
    }

    Ok(VoucherPayload {
        code: clean.to_string(),
        tier,
        credits: if tier.is_studio() { 50_000 } else { 20_000 },
        acx_certificate_unlocked: tier.has_acx_certificate(),
    })
}

/// License details returned upon online or mock verification against Gumroad
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GumroadLicenseInfo {
    pub key: String,
    pub tier: WalletTier,
    pub product_name: String,
    pub email: Option<String>,
    pub uses: u32,
}

/// Verifies a license key against the Gumroad API (or clean mock environment variable)
pub fn verify_gumroad_license(license_key: &str) -> Result<GumroadLicenseInfo> {
    let clean_key = license_key.trim();
    if clean_key.is_empty() {
        bail!("Klucz licencyjny nie może być pusty.");
    }

    // Mock environment support for clean offline testing
    if let Ok(mock) = std::env::var("AUDIOTICON_MOCK_GUMROAD_TIER") {
        let mock_upper = mock.to_uppercase();
        if mock_upper == "INVALID" {
            bail!("Weryfikacja Gumroad nie powiodła się: Nieprawidłowy lub nieistniejący klucz licencyjny.");
        } else if mock_upper == "REFUNDED" {
            bail!("Weryfikacja Gumroad: Ta licencja została zrefundowana lub cofnięta.");
        } else if mock_upper == "STUDIO" {
            return Ok(GumroadLicenseInfo {
                key: clean_key.to_string(),
                tier: WalletTier::Studio,
                product_name: "Audioticon Studio PRO".to_string(),
                email: Some("client@studio.com".to_string()),
                uses: 1,
            });
        } else {
            return Ok(GumroadLicenseInfo {
                key: clean_key.to_string(),
                tier: WalletTier::Solo,
                product_name: "Audioticon Solo PRO".to_string(),
                email: Some("client@solo.com".to_string()),
                uses: 1,
            });
        }
    }

    // Call Gumroad API via curl
    let output = std::process::Command::new("curl")
        .arg("-s")
        .arg("-X")
        .arg("POST")
        .arg("https://api.gumroad.com/v1/licenses/verify")
        .arg("-d")
        .arg(format!("license_key={}", clean_key))
        .arg("-d")
        .arg("increment_uses_count=true")
        .output()
        .with_context(|| {
            "Nie udało się wykonać zapytania do API Gumroad (upewnij się, że masz połączenie z siecią i zainstalowany curl)."
        })?;

    if !output.status.success() {
        bail!("Błąd połączenia sieciowego podczas weryfikacji licencji w Gumroad.");
    }

    let response_str = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = serde_json::from_str(&response_str)
        .with_context(|| format!("Niepoprawna odpowiedź z serwera Gumroad: {}", response_str))?;

    let success = val.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
    if !success {
        let msg = val
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Nieprawidłowy klucz licencyjny.");
        bail!("Weryfikacja Gumroad nie powiodła się: {}", msg);
    }

    let purchase = val
        .get("purchase")
        .context("Brak danych o zakupie w odpowiedzi Gumroad.")?;

    if purchase.get("refunded").and_then(|v| v.as_bool()).unwrap_or(false) {
        bail!("Ta licencja została zrefundowana.");
    }
    if purchase.get("chargebacked").and_then(|v| v.as_bool()).unwrap_or(false) {
        bail!("Płatność za tę licencję została cofnięta (chargeback).");
    }
    if purchase.get("disputed").and_then(|v| v.as_bool()).unwrap_or(false) {
        bail!("Licencja została zablokowana ze względu na spór transakcji.");
    }

    let product_name = purchase
        .get("product_name")
        .and_then(|v| v.as_str())
        .unwrap_or("Audioticon Solo PRO")
        .to_string();
    let permalink = purchase.get("permalink").and_then(|v| v.as_str()).unwrap_or("");
    let email = purchase.get("email").and_then(|v| v.as_str()).map(|s| s.to_string());
    let uses = val.get("uses").and_then(|v| v.as_u64()).unwrap_or(1) as u32;

    let tier = if product_name.to_lowercase().contains("studio")
        || permalink.to_lowercase().contains("studio")
    {
        WalletTier::Studio
    } else {
        WalletTier::Solo
    };

    Ok(GumroadLicenseInfo {
        key: clean_key.to_string(),
        tier,
        product_name,
        email,
        uses,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_and_verify_voucher() {
        let code = generate_voucher_code(WalletTier::Writer, 20000, "ORDER901");
        assert!(code.starts_with("AUD-WRITER-20000-ORDER901-"));

        let payload = verify_voucher_code(&code).expect("Verification should succeed");
        assert_eq!(payload.tier, WalletTier::Writer);
        assert_eq!(payload.credits, 20000);
        assert!(payload.acx_certificate_unlocked);
    }

    #[test]
    fn test_tampered_voucher_rejected() {
        let valid_code = generate_voucher_code(WalletTier::Debut, 5000, "S100");
        // Tamper with credits: change 5000 to 99999
        let tampered_credits = valid_code.replace("5000", "99999");
        assert!(verify_voucher_code(&tampered_credits).is_err());

        // Tamper with serial: change S100 to S101
        let tampered_serial = valid_code.replace("S100", "S101");
        assert!(verify_voucher_code(&tampered_serial).is_err());
    }

    #[test]
    fn test_tier_acx_certificate_permissions() {
        assert!(!WalletTier::Free.has_acx_certificate());
        assert!(!WalletTier::Debut.has_acx_certificate());
        assert!(WalletTier::Solo.has_acx_certificate());
        assert!(WalletTier::Writer.has_acx_certificate());
        assert!(WalletTier::Studio.has_acx_certificate());
        assert!(WalletTier::Publisher.has_acx_certificate());

        // Features gating
        assert!(WalletTier::Solo.has_m4b_export());
        assert!(WalletTier::Solo.has_punch_in());
        assert!(!WalletTier::Solo.has_batch_pipeline());
        assert!(!WalletTier::Solo.has_dataset_export());

        assert!(WalletTier::Studio.has_m4b_export());
        assert!(WalletTier::Studio.has_punch_in());
        assert!(WalletTier::Studio.has_batch_pipeline());
        assert!(WalletTier::Studio.has_dataset_export());
    }

    #[test]
    fn test_solo_and_studio_voucher_codes() {
        let solo_code = generate_voucher_code(WalletTier::Solo, 10000, "SOLO123");
        assert!(solo_code.starts_with("AUD-SOLO-10000-SOLO123-"));
        let solo_payload = verify_voucher_code(&solo_code).expect("Solo voucher valid");
        assert_eq!(solo_payload.tier, WalletTier::Solo);
        assert!(solo_payload.tier.has_m4b_export());

        let studio_code = generate_voucher_code(WalletTier::Studio, 50000, "STUDIO456");
        assert!(studio_code.starts_with("AUD-STUDIO-50000-STUDIO456-"));
        let studio_payload = verify_voucher_code(&studio_code).expect("Studio voucher valid");
        assert_eq!(studio_payload.tier, WalletTier::Studio);
        assert!(studio_payload.tier.has_batch_pipeline());
    }

    #[test]
    fn test_gumroad_token_generation_and_offline_verification() {
        let key = "AAAA-BBBB-CCCC-DDDD";
        let token = create_gumroad_token(WalletTier::Solo, key);
        assert!(token.starts_with("GUMROAD-SOLO-AAAA-BBBB-CCCC-DDDD-"));

        let payload = verify_gumroad_token(&token).expect("Token verification succeeds offline");
        assert_eq!(payload.tier, WalletTier::Solo);
        assert_eq!(payload.code, token);
        assert!(payload.acx_certificate_unlocked);

        // Tamper with key
        let tampered = token.replace("AAAA", "XXXX");
        assert!(verify_gumroad_token(&tampered).is_err());
    }

    #[test]
    fn test_mock_gumroad_license_verification() {
        std::env::set_var("AUDIOTICON_MOCK_GUMROAD_TIER", "STUDIO");
        let info = verify_gumroad_license("MOCK-STUDIO-KEY").expect("Mock studio verification succeeds");
        assert_eq!(info.tier, WalletTier::Studio);
        assert_eq!(info.product_name, "Audioticon Studio PRO");

        std::env::set_var("AUDIOTICON_MOCK_GUMROAD_TIER", "INVALID");
        let err = verify_gumroad_license("BAD-KEY").unwrap_err();
        assert!(err.to_string().contains("Nieprawidłowy"));

        std::env::remove_var("AUDIOTICON_MOCK_GUMROAD_TIER");
    }
}
