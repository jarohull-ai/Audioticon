use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::voucher::{
    active_license_secret, compute_hmac_sha256, verify_voucher_code, VoucherPayload, WalletTier,
};

/// Cost in AudioCoin$ per minute of neural TTS audio
pub const COST_PER_MIN_ELEVENLABS_V3: u64 = 5;
pub const COST_PER_MIN_ELEVENLABS_CLONE: u64 = 50;
pub const COST_PER_MIN_EDGE_TTS: u64 = 0; // 100% Free
pub const COST_PER_MIN_PIPER: u64 = 0;    // 100% Free / Offline

/// Welcome credits granted automatically on a fresh install
pub const STARTING_CREDITS: u64 = 1000;

/// A single financial transaction in the local ledger
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transaction {
    pub timestamp: u64,
    pub amount: i64, // positive = top-up / voucher, negative = spend
    pub balance_after: u64,
    pub description: String,
    pub render_id: Option<String>,
}

/// The local cryptographic AudioCoin$ wallet state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioWallet {
    pub balance: u64,
    pub tier: WalletTier,
    #[serde(default)]
    pub acx_certificate_unlocked: bool,
    pub transactions: Vec<Transaction>,
    #[serde(default)]
    pub redeemed_vouchers: Vec<String>,
    #[serde(default)]
    pub integrity_signature: String,
}

impl Default for AudioWallet {
    fn default() -> Self {
        let mut wallet = Self {
            balance: STARTING_CREDITS,
            tier: WalletTier::Free,
            acx_certificate_unlocked: false,
            transactions: vec![Transaction {
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                amount: STARTING_CREDITS as i64,
                balance_after: STARTING_CREDITS,
                description: "Pakiet powitalny AudioCoin$ (Darmowa instalacja)".to_string(),
                render_id: None,
            }],
            redeemed_vouchers: Vec::new(),
            integrity_signature: String::new(),
        };
        wallet.integrity_signature = wallet.compute_integrity_signature();
        wallet
    }
}

impl AudioWallet {
    /// Computes an HMAC-SHA256 integrity signature covering balance, tier, ACX status, redeemed vouchers, and ledger transactions.
    pub fn compute_integrity_signature(&self) -> String {
        let tx_digest: String = self
            .transactions
            .iter()
            .map(|t| format!("{}:{}:{}", t.timestamp, t.amount, t.balance_after))
            .collect::<Vec<_>>()
            .join("|");
        let vouchers_digest = self.redeemed_vouchers.join(",");
        let payload = format!(
            "AUDIOTICON_WALLET_V2|bal={}|tier={}|acx={}|vouchers=[{}]|tx=[{}]",
            self.balance,
            self.tier.as_str(),
            self.acx_certificate_unlocked,
            vouchers_digest,
            tx_digest
        );
        let sig = compute_hmac_sha256(&active_license_secret(), payload.as_bytes());
        hex::encode(sig)
    }

    /// Verifies that the wallet state has not been manually tampered with in a text editor:
    /// 1. Validates the HMAC-SHA256 `integrity_signature`.
    /// 2. Re-verifies every cryptographic voucher in `redeemed_vouchers` and ensures no duplicate codes exist.
    /// 3. Ensures `tier` and `acx_certificate_unlocked` are backed by valid redeemed vouchers in `redeemed_vouchers`.
    pub fn verify_integrity(&self) -> bool {
        if self.integrity_signature.is_empty()
            || self.integrity_signature != self.compute_integrity_signature()
        {
            return false;
        }

        let mut seen_codes = std::collections::HashSet::new();
        let mut max_verified_tier = WalletTier::Free;
        let mut verified_acx = false;

        for code in &self.redeemed_vouchers {
            let Ok(payload) = verify_voucher_code(code) else {
                return false;
            };
            if !seen_codes.insert(payload.code) {
                return false;
            }
            if payload.tier > max_verified_tier {
                max_verified_tier = payload.tier;
            }
            if payload.acx_certificate_unlocked {
                verified_acx = true;
            }
        }

        if self.tier > max_verified_tier {
            return false;
        }
        if self.acx_certificate_unlocked && !verified_acx {
            return false;
        }

        if let Some(last_tx) = self.transactions.last() {
            if last_tx.balance_after != self.balance {
                return false;
            }
        }

        true
    }

    /// Returns the canonical path to the wallet file (~/.config/audioticon/wallet.json)
    pub fn wallet_path() -> PathBuf {
        if let Ok(custom) = std::env::var("AUDIOTICON_WALLET_PATH") {
            if !custom.trim().is_empty() {
                return PathBuf::from(custom);
            }
        }
        if let Some(config_dir) = dirs::config_dir() {
            config_dir.join("audioticon").join("wallet.json")
        } else if let Some(home) = dirs::home_dir() {
            home.join(".config").join("audioticon").join("wallet.json")
        } else {
            PathBuf::from("wallet.json")
        }
    }

    /// Loads the wallet from disk and verifies its cryptographic integrity,
    /// or resets to a clean default wallet if missing or manually tampered with.
    pub fn load() -> Self {
        let path = Self::wallet_path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(wallet) = serde_json::from_str::<AudioWallet>(&content) {
                    if wallet.verify_integrity() {
                        return wallet;
                    }
                }
            }
        }
        // Fresh install or tampered file detected: reset and persist clean default wallet
        let wallet = Self::default();
        let _ = wallet.save();
        wallet
    }

    /// Saves the wallet to disk securely (0600 permissions on Unix) with an updated HMAC signature
    pub fn save(&self) -> Result<()> {
        let path = Self::wallet_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Nie można utworzyć katalogu: {:?}", parent))?;
        }
        let mut signed = self.clone();
        signed.integrity_signature = signed.compute_integrity_signature();
        let json = serde_json::to_string_pretty(&signed)
            .context("Błąd serializacji portfela AudioCoin")?;

        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut opts = fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true).mode(0o600);
            let mut file = opts
                .open(&path)
                .with_context(|| format!("Nie można otworzyć pliku portfela: {:?}", path))?;
            file.write_all(json.as_bytes())
                .context("Błąd zapisu portfela")?;
        }
        #[cfg(not(unix))]
        {
            fs::write(&path, json).context("Błąd zapisu portfela")?;
        }
        Ok(())
    }

    /// Spends credits. Returns false if insufficient balance.
    pub fn spend(&mut self, amount: u64, description: &str, render_id: Option<String>) -> bool {
        if self.balance < amount {
            return false;
        }
        self.balance -= amount;
        self.transactions.push(Transaction {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            amount: -(amount as i64),
            balance_after: self.balance,
            description: description.to_string(),
            render_id,
        });
        self.integrity_signature = self.compute_integrity_signature();
        let _ = self.save();
        true
    }

    /// Adds credits directly (e.g. from in-app top up)
    pub fn add_credits(&mut self, amount: u64, description: &str) {
        self.balance += amount;
        self.transactions.push(Transaction {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            amount: amount as i64,
            balance_after: self.balance,
            description: description.to_string(),
            render_id: None,
        });
        self.integrity_signature = self.compute_integrity_signature();
        let _ = self.save();
    }

    /// Redeems a cryptographic voucher code and updates balance & unlocked tier
    pub fn redeem_voucher(&mut self, code: &str) -> Result<VoucherPayload> {
        let payload = verify_voucher_code(code)?;

        // Replay attack prevention: check if code was already redeemed
        if self.redeemed_vouchers.contains(&payload.code) {
            bail!("Ten voucher został już wcześniej wykorzystany w tym portfelu.");
        }

        self.balance += payload.credits;
        self.redeemed_vouchers.push(payload.code.clone());

        // Upgrade tier if voucher has higher tier
        if payload.tier > self.tier {
            self.tier = payload.tier;
        }

        if payload.acx_certificate_unlocked {
            self.acx_certificate_unlocked = true;
        }

        self.transactions.push(Transaction {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            amount: payload.credits as i64,
            balance_after: self.balance,
            description: format!("Voucher VIP: {} (+{} AudioCoin$)", payload.tier.as_str(), payload.credits),
            render_id: None,
        });
        self.integrity_signature = self.compute_integrity_signature();

        let _ = self.save();
        Ok(payload)
    }

    /// Returns estimated minutes of ElevenLabs v3 audio affordable
    pub fn elevenlabs_minutes(&self) -> u64 {
        self.balance / COST_PER_MIN_ELEVENLABS_V3
    }

    /// Returns true if user can afford at least 1 minute of ElevenLabs v3
    pub fn can_afford_el(&self) -> bool {
        self.balance >= COST_PER_MIN_ELEVENLABS_V3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_starting_balance_and_minutes() {
        let wallet = AudioWallet::default();
        assert_eq!(wallet.balance, STARTING_CREDITS);
        assert_eq!(wallet.elevenlabs_minutes(), 200);
        assert_eq!(wallet.tier, WalletTier::Free);
        assert!(!wallet.acx_certificate_unlocked);
    }

    #[test]
    fn test_spend_success_and_insufficient() {
        let mut wallet = AudioWallet::default();
        assert!(wallet.spend(300, "Render testowy", None));
        assert_eq!(wallet.balance, STARTING_CREDITS - 300);

        // Cannot spend more than remaining balance
        assert!(!wallet.spend(STARTING_CREDITS, "Za duzo", None));
        assert_eq!(wallet.balance, STARTING_CREDITS - 300);
    }

    #[test]
    fn test_redeem_voucher_and_replay_prevention() {
        let mut wallet = AudioWallet::default();
        let voucher = crate::voucher::generate_voucher_code(WalletTier::Writer, 20000, "VIP2026");

        let res = wallet.redeem_voucher(&voucher);
        assert!(res.is_ok());
        assert_eq!(wallet.balance, STARTING_CREDITS + 20000);
        assert_eq!(wallet.tier, WalletTier::Writer);
        assert!(wallet.acx_certificate_unlocked);

        // Attempting to redeem the same code twice must fail
        let duplicate_res = wallet.redeem_voucher(&voucher);
        assert!(duplicate_res.is_err());
    }

    #[test]
    fn test_wallet_json_notepad_tampering_rejected() {
        let mut wallet = AudioWallet::default();
        assert!(wallet.verify_integrity());

        // 1. Simulate editing balance in Notepad from 1000 to 999999999
        let mut tampered_balance = wallet.clone();
        tampered_balance.balance = 999_999_999;
        assert!(
            !tampered_balance.verify_integrity(),
            "Manual balance modification in JSON must fail integrity verification"
        );

        // 2. Simulate editing acx_certificate_unlocked to true in Notepad without a redeemed voucher
        let mut tampered_acx = wallet.clone();
        tampered_acx.acx_certificate_unlocked = true;
        tampered_acx.integrity_signature = tampered_acx.compute_integrity_signature();
        assert!(
            !tampered_acx.verify_integrity(),
            "ACX certificate unlock without a verified Writer/Publisher voucher must fail integrity verification"
        );

        // 3. Legitimate voucher redemption passes integrity verification
        let voucher = crate::voucher::generate_voucher_code(WalletTier::Writer, 20000, "S2026A");
        wallet.redeem_voucher(&voucher).expect("Redeem valid voucher");
        assert!(wallet.verify_integrity());
    }
}
