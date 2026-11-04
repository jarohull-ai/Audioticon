pub mod cache;
pub mod direct;
pub mod emojis;
pub mod import;
pub mod models;
pub mod providers;
pub mod render;
pub mod script;
pub mod voicelab;

use anyhow::Result;

use crate::args::Commands;

pub async fn dispatch(command: Commands) -> Result<()> {
    match command {
        Commands::Render {
            input,
            output,
            provider,
            voice,
            lang,
            model,
            cache_dir,
            title,
            author,
            narrator,
            year,
            demo,
            demo_effects,
            estimate,
            max_cache_size,
            jobs,
            style,
            cadence,
            breath_intensity,
        } => {
            render::run_render(
                input,
                output,
                provider,
                voice,
                lang,
                model,
                cache_dir,
                title,
                author,
                narrator,
                year,
                demo,
                demo_effects,
                estimate,
                max_cache_size,
                jobs,
                style,
                cadence,
                breath_intensity,
            )
            .await
        }
        Commands::Cache {
            clear,
            dir,
            prune,
            stats,
        } => cache::run_cache(clear, dir, prune, stats),
        Commands::Presets { config } => cache::run_presets(config),
        Commands::Estimate {
            input,
            provider,
            voice,
            cache_dir,
        } => cache::run_estimate(input, provider, voice, cache_dir),
        Commands::Script {
            input,
            format,
            lang,
            demo,
        } => script::run_script(input, format, lang, demo),
        Commands::Serve {
            port,
            host,
            provider,
            lang,
            model,
            no_browser,
        } => crate::server::run_studio_server(host, port, provider, lang, model, no_browser).await,
        Commands::Models { action } => models::run_models(action),
        Commands::Import {
            input,
            output,
            title,
            author,
            preview,
            split_dir,
        } => import::run_import(input, output, title, author, preview, split_dir),
        Commands::Providers { action } => providers::run_providers(action),
        Commands::VoiceLab { action } => voicelab::run_voicelab(action),
        Commands::Emojis { action } => emojis::run_emojis(action),
        Commands::Direct {
            input,
            output,
            provider,
            model,
            key,
            dry_run,
        } => direct::run_direct(input, output, provider, model, key, dry_run),
    }
}
