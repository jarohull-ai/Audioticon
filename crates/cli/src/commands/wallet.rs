use anyhow::Result;
use audioticon_core::AudioWallet;

use crate::args::WalletAction;

pub fn run_wallet(action: Option<WalletAction>) -> Result<()> {
    let mut wallet = AudioWallet::load();

    match action.unwrap_or(WalletAction::Balance) {
        WalletAction::Balance => {
            println!("🪙 PORTFEL AUDIOCOIN$ & LICENCJE");
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
            println!("  Stan konta:            {} AudioCoin$", wallet.balance);
            println!("  Poziom licencji:       {}", wallet.tier.as_str());
            println!(
                "  Certyfikat ACX/Storytel: {}",
                if wallet.acx_certificate_unlocked {
                    "✅ ODBLOKOWANY (Audio Ledger)"
                } else {
                    "❌ Niedostępny (Wymagany Pakiet Pisarz)"
                }
            );
            println!(
                "  Szacunkowy czas ElevenLabs: ~{} minut",
                wallet.elevenlabs_minutes()
            );
            println!(
                "  Silniki offline (Piper / Edge): 100% BEZPŁATNE (0 monet)"
            );
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
            println!("📜 Ostatnie transakcje:");
            let recent: Vec<_> = wallet.transactions.iter().rev().take(5).collect();
            if recent.is_empty() {
                println!("  Brak transakcji.");
            } else {
                for tx in recent {
                    let sign = if tx.amount >= 0 { "+" } else { "" };
                    println!(
                        "  • {:<12} | {}{} AudioCoin$ | Saldo: {}",
                        tx.description, sign, tx.amount, tx.balance_after
                    );
                }
            }
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
            println!("💡 Chcesz doładować konto lub odblokować Certyfikat ACX?");
            println!("   Użyj: audioticon wallet topup");
            println!("   Masz kod vouchera? Użyj: audioticon wallet redeem <KOD>");
        }
        WalletAction::Redeem { code } => {
            match wallet.redeem_voucher(&code) {
                Ok(payload) => {
                    println!("🎉 SUKCES! Zrealizowano voucher VIP!");
                    println!(
                        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
                    );
                    println!("  Dodano kredyty:        +{} AudioCoin$", payload.credits);
                    println!("  Nowe saldo:            {} AudioCoin$", wallet.balance);
                    println!("  Zaktualizowany poziom: {}", payload.tier.as_str());
                    if payload.acx_certificate_unlocked {
                        println!("  🌟 ODBLOKOWANO: Oficjalny Certyfikat Zgodności ACX / Storytel!");
                        println!("     Twój następny render wygeneruje plik 'audiobook.certificate.json'");
                    }
                    println!(
                        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
                    );
                }
                Err(err) => {
                    eprintln!("❌ Błąd realizacji vouchera: {}", err);
                }
            }
        }
        WalletAction::Topup => {
            println!("🛒 PAKIETY DOŁADOWAŃ AUDIOCOIN$ (IN-APP)");
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
            println!("1. 🥉 PAKIET DEBIUTANT — 39 zł");
            println!("   • 5 000 AudioCoin$ (~16h lektora ElevenLabs v3)");
            println!("   • Dostęp do darmowych bibliotek SFX");
            println!("   • Podstawowy mastering");
            println!();
            println!("2. 🥈 PAKIET PISARZ (NAJPOPULARNIEJSZY) — 119 zł");
            println!("   • 20 000 AudioCoin$ (~66h lektora ElevenLabs v3)");
            println!("   • 🌟 OFICJALNY CERTYFIKAT ZGODNOŚCI ACX / STORYTEL (Audio Ledger)");
            println!("   • Gwarancja akceptacji technicznej dystrybutorów");
            println!("   • Pełna kadencja oddechowa i foley");
            println!();
            println!("3. 🥇 PAKIET WYDAWCA / STUDIO — 249 zł");
            println!("   • 50 000 AudioCoin$ (Klonowanie głosu VoiceLab + Multi-Voice)");
            println!("   • Nielimitowane certyfikaty ACX / Storytel");
            println!("   • Prawa do komercyjnej odsprzedaży słuchowisk (B2B)");
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
            println!("💳 Kup voucher online przez BLIK / Kartę / Apple Pay na stronie:");
            println!("   https://audioticon.io/pricing  (lub wpisz kod: audioticon wallet redeem <KOD>)");
        }
    }

    Ok(())
}
