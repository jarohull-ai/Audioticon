use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::voucher::{verify_voucher_code, VoucherPayload, WalletTier};

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
}

impl Default for AudioWallet {
    fn default() -> Self {
        Self {
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
        }
    }
}

impl AudioWallet {
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

    /// Loads the wallet from disk, or initializes a new one with starting credits
    pub fn load() -> Self {
        let path = Self::wallet_path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(wallet) = serde_json::from_str::<AudioWallet>(&content) {
                    return wallet;
                }
            }
        }
        // Fresh install: persist default wallet
        let wallet = Self::default();
        let _ = wallet.save();
        wallet
    }

    /// Saves the wallet to disk securely (0600 permissions on Unix)
    pub fn save(&self) -> Result<()> {
        let path = Self::wallet_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Nie można utworzyć katalogu: {:?}", parent))?;
        }
        let json = serde_json::to_string_pretty(self)
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
        if self.tier == WalletTier::Free
            || (self.tier == WalletTier::Debut && payload.tier != WalletTier::Free)
            || payload.tier == WalletTier::Publisher
        {
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
}
