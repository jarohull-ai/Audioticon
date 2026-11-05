use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Salt / secret seed used for cryptographic HMAC-SHA256 voucher verification
pub const VOUCHER_HMAC_SECRET: &[u8] = b"AUDIOTICON_STUDIO_OFFLINE_SECRET_KEY_2026_SEED";

/// Tier of unlocked wallet features
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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

/// Information returned after successfully validating a voucher code
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

/// Generates a valid cryptographic voucher code (e.g. for store integration or promo campaigns)
/// Format: AUD-<TIER>-<CREDITS>-<SIGNATURE_12_CHARS>
pub fn generate_voucher_code(tier: WalletTier, credits: u64, nonce: &str) -> String {
    let tier_tag = match tier {
        WalletTier::Free => "FREE",
        WalletTier::Debut => "DEBUT",
        WalletTier::Writer => "WRITER",
        WalletTier::Publisher => "PUB",
    };

    let payload = format!("{}:{}:{}", tier_tag, credits, nonce.trim());
    let sig = compute_hmac_sha256(VOUCHER_HMAC_SECRET, payload.as_bytes());
    let sig_hex = hex::encode(&sig[..6]).to_uppercase(); // 12-char hex signature

    format!("AUD-{}-{}-{}", tier_tag, credits, sig_hex)
}

/// Verifies and decodes a cryptographic voucher code
pub fn verify_voucher_code(code: &str) -> Result<VoucherPayload> {
    let clean = code.trim().to_uppercase();
    let parts: Vec<&str> = clean.split('-').collect();

    if parts.len() != 4 || parts[0] != "AUD" {
        bail!(
            "Nieprawidłowy format vouchera. Prawidłowy format to np. AUD-WRITER-20000-XXXXXXXXXXXX"
        );
    }

    let tier_str = parts[1];
    let credits_str = parts[2];
    let given_sig = parts[3];

    let tier = match tier_str {
        "FREE" => WalletTier::Free,
        "DEBUT" => WalletTier::Debut,
        "WRITER" => WalletTier::Writer,
        "PUB" | "PUBLISHER" => WalletTier::Publisher,
        _ => bail!("Nieznana kategoria vouchera: {}", tier_str),
    };

    let credits: u64 = credits_str
        .parse()
        .map_err(|_| anyhow::anyhow!("Nieprawidłowa liczba kredytów w voucherze: {}", credits_str))?;

    // In offline verification, signature is verified against canonical payload
    // We check either against general distribution salt or known promo seeds
    let mut valid = false;

    // Check with canonical empty/default nonce and standard seeds
    for nonce in &["", "VIP2026", "PROMO", "BESTSELLER", "AUDIOTICON"] {
        let payload = format!("{}:{}:{}", tier_str, credits, nonce);
        let expected_sig = compute_hmac_sha256(VOUCHER_HMAC_SECRET, payload.as_bytes());
        let expected_hex = hex::encode(&expected_sig[..6]).to_uppercase();
        if given_sig == expected_hex {
            valid = true;
            break;
        }
    }

    if !valid {
        bail!("Voucher ma nieprawidłowy podpis kryptograficzny lub wygasł.");
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
        let code = generate_voucher_code(WalletTier::Writer, 20000, "VIP2026");
        assert!(code.starts_with("AUD-WRITER-20000-"));

        let payload = verify_voucher_code(&code).expect("Verification should succeed");
        assert_eq!(payload.tier, WalletTier::Writer);
        assert_eq!(payload.credits, 20000);
        assert!(payload.acx_certificate_unlocked);
    }

    #[test]
    fn test_tampered_voucher_rejected() {
        let valid_code = generate_voucher_code(WalletTier::Debut, 5000, "");
        // Tamper with credits: change 5000 to 99999
        let tampered = valid_code.replace("5000", "99999");
        assert!(verify_voucher_code(&tampered).is_err());
    }

    #[test]
    fn test_tier_acx_certificate_permissions() {
        assert!(!WalletTier::Free.has_acx_certificate());
        assert!(!WalletTier::Debut.has_acx_certificate());
        assert!(WalletTier::Writer.has_acx_certificate());
        assert!(WalletTier::Publisher.has_acx_certificate());
    }
}
