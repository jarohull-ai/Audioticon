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
            println!("  Silniki lokalne (Piper / Kokoro-82M / Edge): 100% BEZPŁATNE (0 monet)");
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
        WalletAction::Redeem { code } => match wallet.redeem_voucher(&code) {
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
                    println!(
                        "     Twój następny render wygeneruje plik 'audiobook.certificate.json'"
                    );
                }
                println!(
                    "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
                );
            }
            Err(err) => {
                eprintln!("❌ Błąd realizacji vouchera: {}", err);
            }
        },
        WalletAction::Topup => {
            println!("🛒 PAKIETY LICENCJI AUDIOTICON STUDIO (GUMROAD SECURE CHECKOUT)");
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
            println!("1. 🌟 SOLO PRO (PAKIET PISARZ) — $45 USD (Licencja wieczysta v1.x)");
            println!("   • Wszystkie funkcje Free (Notion editor, Waveform, MP3/WAV, Piper/Kokoro/Edge/ElevenLabs BYOK)");
            println!("   • Eksport kontenera M4B z rozdziałami i okładką");
            println!("   • 🌟 OFICJALNY CERTYFIKAT ZGODNOŚCI ACX / EBU R128 (-24 do -14 LUFS)");
            println!("   • Vocal & AI Punch-In / Retake bez przeliczania całej książki");
            println!("   • Kup online: https://gumroad.com/l/audioticon-solo");
            println!();
            println!("2. 🚀 STUDIO PRO (PAKIET WYDAWCA) — $99 USD (Licencja wieczysta v1.x)");
            println!("   • Wszystkie funkcje Solo PRO");
            println!("   • Multi-book Batch Pipeline (przetwarzanie całych serii wydawniczych z crash-resume)");
            println!(
                "   • Eksport datasetów VoiceLab ZIP dla trenowania własnych modeli neuralnych"
            );
            println!("   • Kup online: https://gumroad.com/l/audioticon-studio");
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
            println!("ℹ️  Podatek VAT / Sales Tax jest automatycznie rozliczany przez Gumroad (Merchant of Record).");
            println!("   Po zakupie otrzymasz klucz licencyjny. Wpisz go w aplikacji lub uruchom:");
            println!("   audioticon wallet redeem <KLUCZ_LUB_VOUCHER>");
        }
    }

    Ok(())
}
