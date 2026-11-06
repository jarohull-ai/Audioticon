use anyhow::{bail, Result};
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
    Writer,
    Publisher,
}

impl WalletTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::Debut => "Debiutant",
            Self::Writer => "Pisarz",
            Self::Publisher => "Wydawca",
        }
    }

    pub fn name(&self) -> &'static str {
        self.as_str()
    }

    pub fn has_acx_certificate(&self) -> bool {
        matches!(self, Self::Writer | Self::Publisher)
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
        WalletTier::Writer => "WRITER",
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
        "WRITER" => (WalletTier::Writer, "WRITER"),
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
        assert!(WalletTier::Writer.has_acx_certificate());
        assert!(WalletTier::Publisher.has_acx_certificate());
    }
}
