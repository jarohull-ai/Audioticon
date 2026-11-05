use anyhow::{Context, Result};
use audioticon_core::{credentials, sanitize_slug, voice_design, voicelab, AudioWallet, VoiceProfile};

use crate::args::{get_default_cache_dir, VoiceLabAction};

pub fn run_voicelab(action: Option<VoiceLabAction>) -> Result<()> {
    match action.unwrap_or(VoiceLabAction::List) {
        VoiceLabAction::List => {
            let profiles = voicelab::list_voice_profiles()?;
            let root = voicelab::voices_dir();
            println!("🎙️ Profile głosowe lektorów (Voice Lab)");
            println!("📁 Katalog profili: {}", root.display());
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
            if profiles.is_empty() {
                println!("  Brak utworzonych profili głosowych.");
                println!("  💡 Utwórz profil: audioticon voicelab create jarek-lektor --name \"Jarek (Lektor)\"");
            } else {
                for p in &profiles {
                    let clone_badge = match &p.elevenlabs_voice_id {
                        Some(id) => format!("🟢 ElevenLabs: {}", id),
                        None => "⚪ Tylko lokalny".to_string(),
                    };
                    println!("  👤 {} [{}] ({}, {})", p.name, p.id, p.gender, p.language);
                    println!(
                        "     Próbki:  {} plików (łącznie {:.1}s)",
                        p.samples.len(),
                        p.total_duration()
                    );
                    println!("     Status:  {}", clone_badge);
                    if let Some(ref desc) = p.description {
                        println!("     Opis:    {}", desc);
                    }
                }
            }
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
        }
        VoiceLabAction::Create {
            id,
            name,
            gender,
            lang,
            desc,
        } => {
            let mut p = VoiceProfile::new(id, name, gender, lang);
            p.description = desc;
            voicelab::save_voice_profile(&p)?;
            println!(
                "✅ Pomyślnie utworzono profil głosu '{}' [{}]",
                p.name, p.id
            );
            println!(
                "📁 Katalog profilu: {}",
                voicelab::profile_dir(&p.id).display()
            );
        }
        VoiceLabAction::AddSample {
            profile,
            input,
            name,
        } => {
            if !input.exists() {
                anyhow::bail!("Plik audio nie istnieje: {:?}", input);
            }
            let bytes = std::fs::read(&input)
                .with_context(|| format!("Nie można odczytać pliku: {:?}", input))?;
            println!("🎧 Analizuję i normalizuję próbkę audio: {:?}", input);
            let (meta, report) =
                voicelab::add_sample_to_profile(&profile, &bytes, name.as_deref())?;
            println!("✅ Próbka została zapisana w profilu '{}'", profile);
            println!("   Plik:       {}", meta.filename);
            println!("   Czas:       {:.2} s", report.duration_sec);
            println!("   Głośność:   {:.1} dBFS (RMS)", report.rms_db);
            println!("   Szczyt:     {:.1} dBFS (Peak)", report.peak_db);
            println!("   Dynamika:   {:.1} dB (SNR)", report.snr_db);
            println!("   Status:     {}", report.status.to_uppercase());
            for tip in &report.tips {
                println!("   💡 {}", tip);
            }
        }
        VoiceLabAction::PunchIn {
            profile,
            hash,
            input,
            cache_dir,
        } => {
            if !input.exists() {
                anyhow::bail!("Plik nagrania audio nie istnieje: {:?}", input);
            }
            let bytes = std::fs::read(&input)
                .with_context(|| format!("Nie można odczytać pliku: {:?}", input))?;
            let cache_path = cache_dir.unwrap_or_else(get_default_cache_dir);
            let punch_wav = voicelab::punch_in_sentence(&profile, &hash, &bytes, &cache_path)?;
            println!(
                "✅ Zapisano nagranie punch-in dla zdania [{}] lektora '{}'",
                &hash[0..8],
                profile
            );
            println!("📁 Plik w buforze: {:?}", punch_wav);
            println!("💡 Wszystkie kolejne rendery tego zdania użyją Twojego autentycznego nagrania lektorskiego.");
        }
        VoiceLabAction::CloneElevenlabs { profile } => {
            let key = credentials::get_api_key("elevenlabs").ok_or_else(|| {
                anyhow::anyhow!("Brak klucza API ElevenLabs. Użyj: audioticon providers set elevenlabs --key <KLUCZ>")
            })?;
            println!("🚀 Rozpoczynam procedurę klonowania głosu w ElevenLabs...");
            let voice_id = voicelab::clone_to_elevenlabs(&profile, &key)?;
            println!("✅ Głos został pomyślnie sklonowany w ElevenLabs!");
            println!("   ID głosu: {}", voice_id);
            println!(
                "💡 Możesz teraz renderować audiobooka tym głosem, podając flagę --voice {}",
                voice_id
            );
        }
        VoiceLabAction::ExportDataset { profile, output } => {
            println!(
                "📦 Eksportuję próbki lektora '{}' do archiwum ZIP...",
                profile
            );
            let zip_path = voicelab::export_dataset_zip(&profile, &output)?;
            println!(
                "✅ Wyeksportowano dataset gotowy do treningu XTTS / Piper: {:?}",
                zip_path
            );
        }
        VoiceLabAction::Delete { profile } => {
            voicelab::delete_voice_profile(&profile)?;
            println!(
                "✅ Profil głosu '{}' został usunięty wraz z próbkami.",
                profile
            );
        }
        VoiceLabAction::Design {
            prompt,
            name,
            text,
            save,
        } => {
            let key = credentials::get_api_key("elevenlabs").ok_or_else(|| {
                anyhow::anyhow!("Brak klucza API ElevenLabs. Użyj: audioticon providers set elevenlabs --key <KLUCZ>")
            })?;

            println!("🎨 PROJEKTOWANIE GŁOSU (ELEVENLABS VOICE DESIGN)");
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
            println!("  Opis postaci:      \"{}\"", prompt);
            if let Some(ref t) = text {
                println!("  Przykładowy tekst: \"{}\"", t);
            }
            println!("⏳ Generowanie próbek audio przez ElevenLabs AI...");

            let resp =
                voice_design::create_voice_design_previews(&key, &prompt, text.as_deref())?;

            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
            println!("✅ Wygenerowano {} wariantów głosu!", resp.previews.len());

            for (i, p) in resp.previews.iter().enumerate() {
                let dur = p.duration_secs.unwrap_or(0.0);
                println!(
                    "  [{}] ID Próbki: {} ({:.1}s)",
                    i + 1,
                    p.generated_voice_id,
                    dur
                );
            }

            // Spend AudioCoin$ if wallet is active
            let mut wallet = AudioWallet::load();
            let _ = wallet.spend(50, "Projektowanie głosu ElevenLabs (Voice Design)", None);
            let _ = wallet.save();

            let target_name = name.unwrap_or_else(|| "Głos Zaprojektowany".to_string());
            if save {
                if let Some(first) = resp.previews.first() {
                    println!("\n💾 Zapisuję wariant 1 do biblioteki konta ElevenLabs...");
                    let voice_id = voice_design::save_designed_voice(
                        &key,
                        &target_name,
                        &prompt,
                        &first.generated_voice_id,
                        Some(&first.audio_base_64),
                    )?;
                    println!("✅ Pomyślnie dodano głos '{}' do biblioteki!", target_name);
                    println!("   ID głosu: {}", voice_id);
                    println!(
                        "💡 W scenariuszu możesz teraz używać tagu [{}], aby przemawiał tym głosem!",
                        sanitize_slug(&target_name)
                    );
                }
            } else {
                println!("\n💡 Aby zapisać wybraną próbkę do biblioteki, uruchom:");
                println!(
                    "   audioticon voicelab design \"{}\" --name \"{}\" --save",
                    prompt, target_name
                );
            }
        }
    }
    Ok(())
}
