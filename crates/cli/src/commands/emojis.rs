use anyhow::Result;
use audioticon_core::{presets, CustomEmojiDef, ProjectConfig};

use crate::args::EmojisAction;

pub fn run_emojis(action: Option<EmojisAction>) -> Result<()> {
    match action.unwrap_or(EmojisAction::List) {
        EmojisAction::List => {
            let cfg = ProjectConfig::discover(None);
            println!("🎨 Zdefiniowane emotikonki w projekcie (audioticon.toml)");
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
            if cfg.emojis.is_empty() {
                println!("  Brak własnych emotikonek w audioticon.toml.");
                println!("  Dostępne są fabryczne emotikony akustyczne (np. 📱, 📻, 📢, 🏛️, 🏰, 💭, 🤖, 🤫).");
                println!("  💡 Dodaj własną: audioticon emojis add --emoji \"🧙‍♂️\" --name \"Czarodziej\" --filter cathedral --pitch -4");
            } else {
                let mut sorted: Vec<_> = cfg.emojis.iter().collect();
                sorted.sort_by_key(|(k, _)| (*k).clone());
                for (emoji, def) in sorted {
                    let alias_str = def.alias.as_deref().unwrap_or("—");
                    println!(
                        "  {} {} [{}] (typ: {})",
                        emoji, def.name, alias_str, def.action_type
                    );
                    if let Some(ref f) = def.filter {
                        println!("     Filtr:   {}", f);
                    }
                    if let Some(ref v) = def.voice {
                        println!("     Głos:    {}", v);
                    }
                    if let Some(p) = def.pitch {
                        println!("     Ton:     {:+}", p);
                    }
                    if let Some(r) = def.rate {
                        println!("     Tempo:   {:+}", r);
                    }
                    if let Some(v) = def.volume {
                        println!("     Głośn:   {:.2}x", v);
                    }
                }
            }
            println!(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            );
        }
        EmojisAction::Add {
            emoji,
            name,
            action_type,
            alias,
            filter,
            voice,
            pitch,
            rate,
            volume,
            desc,
        } => {
            let mut cfg = ProjectConfig::discover(None);
            let def = CustomEmojiDef {
                name: name.clone(),
                alias,
                action_type,
                filter,
                voice,
                pitch,
                rate,
                volume,
                description: desc,
            };
            cfg.emojis.insert(emoji.clone(), def.clone());
            cfg.save_to_path(std::path::Path::new("audioticon.toml"))?;
            let mut reg = presets::get_active_registry();
            reg.register_custom_emoji(emoji.clone(), def);
            presets::set_active_registry(reg);
            println!(
                "✅ Pomyślnie zapisano własną emotikonkę {} [{}] w audioticon.toml!",
                emoji, name
            );
        }
        EmojisAction::Remove { emoji } => {
            let mut cfg = ProjectConfig::discover(None);
            if cfg.emojis.remove(&emoji).is_some() {
                cfg.save_to_path(std::path::Path::new("audioticon.toml"))?;
                let mut reg = presets::get_active_registry();
                reg.remove_custom_emoji(&emoji);
                presets::set_active_registry(reg);
                println!("✅ Usunięto emotikonkę {} z audioticon.toml.", emoji);
            } else {
                println!(
                    "ℹ️ Emotikonka {} nie była zdefiniowana w audioticon.toml.",
                    emoji
                );
            }
        }
    }
    Ok(())
}
