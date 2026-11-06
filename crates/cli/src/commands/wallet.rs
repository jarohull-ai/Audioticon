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
                "  Limit produkcyjny BYOK (ElevenLabs): ~{} minut",
                wallet.elevenlabs_minutes()
            );
            println!(
                "  Silniki lokalne (Piper / Kokoro-82M / Edge): 100% BEZPŁATNE (0 monet)"
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
            println!("🛒 PAKIETY LICENCJI AUDIOTICON STUDIO & AUDIOCOIN$");
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
            println!("1. 🥉 PAKIET DEBIUTANT — 39 zł");
            println!("   • 5 000 AudioCoin$ (limity produkcyjne BYOK / reżyseria AI)");
            println!("   • Dostęp do studyjnych bibliotek SFX & Ambient");
            println!("   • Eksport M4B z rozdziałami i masteringiem EBU R128");
            println!();
            println!("2. 🥈 PAKIET PISARZ (NAJPOPULARNIEJSZY) — 119 zł");
            println!("   • 20 000 AudioCoin$ (pełna produkcja wielogłosowa & integracja BYOK ElevenLabs)");
            println!("   • 🌟 OFICJALNY CERTYFIKAT ZGODNOŚCI ACX / STORYTEL (Audio Ledger)");
            println!("   • Rzeczywisty pomiar LUFS / True Peak i eksport bezstratny 24-bit FLAC");
            println!("   • Pełna kadencja oddechowa i foley");
            println!();
            println!("3. 🥇 PAKIET WYDAWCA / STUDIO — 249 zł");
            println!("   • 50 000 AudioCoin$ (VoiceLab + eksport paczek LJSpeech/XTTS + Multi-Voice)");
            println!("   • Nielimitowane certyfikaty ACX / Storytel z podpisem kryptograficznym");
            println!("   • Prawa do komercyjnej produkcji słuchowisk (B2B)");
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
            println!("ℹ️  Uwaga: Silniki lokalne (Piper PL, Kokoro-82M) oraz Edge TTS działają w 100% za darmo.");
            println!("   Integracja z chmurą ElevenLabs działa w modelu BYOK (własny klucz API użytkownika).");
            println!("💳 Kup licencję / voucher online przez BLIK / Kartę / Apple Pay:");
            println!("   https://audioticon.io/pricing  (lub wpisz kod: audioticon wallet redeem <KOD>)");
        }
    }

    Ok(())
}
