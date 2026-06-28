mod app;
mod config;
mod pipeline;
mod schemes;
mod state;
mod ui;

use anyhow::{Context, Result};
use app::App;
use clap::Parser;
use config::Config;
use pipeline::gnome::GnomeInterface;
use schemes::fetch;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "gnomad", about = "GNOME colour scheme and wallpaper manager")]
struct Cli {
    #[arg(long, help = "Fetch/update schemes and exit")]
    update_schemes: bool,

    #[arg(long, value_name = "SLUG", help = "Apply a scheme headlessly and exit")]
    apply: Option<String>,

    #[arg(
        long,
        help = "Ensure /tmp/gnomad-current-scheme.json is present and matches the \
                current scheme; regenerates it if missing or stale, then exits"
    )]
    populate_json_scheme: bool,

    #[arg(short, long, help = "Enable debug logging (pipeline steps, paths, errors)")]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let log_filter = if cli.verbose {
        EnvFilter::new("gnomad=debug")
    } else {
        EnvFilter::from_default_env()
    };

    if cli.verbose {
        let log_path = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()))
            .join("gnomad")
            .join("gnomad.log");
        std::fs::create_dir_all(log_path.parent().unwrap()).ok();
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("opening log file {}", log_path.display()))?;
        tracing_subscriber::fmt()
            .with_env_filter(log_filter)
            .with_target(false)
            .with_writer(log_file)
            .init();
        eprintln!("gnomad: verbose logging → {}", log_path.display());
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(log_filter)
            .with_target(false)
            .with_writer(std::io::stderr)
            .init();
    }

    let config_existed = config::config_path().exists();
    let config = Config::load().context("loading config")?;
    if !config_existed {
        if let Err(e) = config.save() {
            eprintln!("gnomad: could not write default config: {e}");
        } else {
            eprintln!("gnomad: created default config at {}", config::config_path().display());
        }
    }

    // Headless: ensure palette JSON is present and current (no binary deps needed)
    if cli.populate_json_scheme {
        return headless_populate_json_scheme(&config).await;
    }

    // Startup checks
    check_binary("git")?;
    if config.wallpaper_enabled {
        check_binary("gowall")?;
    }
    check_binary("tinty")?;

    // Ensure data dirs exist
    let data_base = config::data_dir();
    std::fs::create_dir_all(&data_base)?;
    if config.wallpaper_enabled {
        std::fs::create_dir_all(&config.wallpaper_cache_dir)?;
    }

    // Clone schemes repo if not present
    if !config.schemes_repo_dir.exists() {
        eprintln!("gnomad: cloning tinted-theming/schemes (first run)...");
        fetch::clone_schemes_repo(&config.schemes_repo_dir)
            .await
            .context("cloning schemes repo")?;
    }

    // Headless: update schemes
    if cli.update_schemes {
        fetch::update_schemes_repo(&config.schemes_repo_dir)
            .await
            .context("updating schemes")?;
        println!("Schemes updated.");
        return Ok(());
    }

    // Headless: apply scheme
    if let Some(slug) = cli.apply {
        return headless_apply(&slug, &config).await;
    }

    // Runtime GNOME state
    let gnome = GnomeInterface::new().await?;
    let gnome_color_scheme = gnome.get_color_scheme().await.unwrap_or_default();

    // Warn about missing User Themes extension
    if !gnome.is_user_themes_enabled().await {
        eprintln!(
            "gnomad: WARNING — User Themes extension not detected.\n\
             Shell CSS will be written but won't apply. Enable it with:\n\
             gnome-extensions enable user-theme@gnome-shell-extensions.gcampax.github.com\n\
             Then select 'gnomad' as the shell theme in GNOME Tweaks."
        );
    }

    // Warn about Flatpak overrides
    let themes_dir = dirs::data_local_dir()
        .unwrap_or_default()
        .join("themes")
        .join(&config.theme_name);
    if !themes_dir.exists() {
        eprintln!(
            "gnomad: NOTE — Theme directory not yet created. After first use, run:\n\
             flatpak override --user --filesystem=xdg-config/gtk-3.0\n\
             flatpak override --user --filesystem=xdg-config/gtk-4.0\n\
             flatpak override --user --filesystem=xdg-data/themes"
        );
    }

    // Load schemes
    let schemes = fetch::load_schemes(
        &config.schemes_repo_dir,
        config.custom_schemes_dir.as_deref(),
    )
    .context("loading schemes")?;

    eprintln!("gnomad: loaded {} schemes", schemes.len());

    // Detect terminal graphics protocol before entering raw mode
    let picker = ratatui_image::picker::Picker::from_query_stdio()
        .map_err(|e| eprintln!("gnomad: image preview unavailable ({e})"))
        .ok();

    // Launch TUI
    let mut terminal = ratatui::init();
    let mut app = App::new(config, gnome_color_scheme, picker);
    app.state.set_schemes(schemes, app.config.follow_user_scheme_type);

    // Restore last active scheme so wallpaper picker knows which scheme to convert for
    if let Some(ref slug) = app.config.default_scheme.clone() {
        app.state.active_scheme = app.state.all_schemes.iter()
            .find(|s| &s.slug == slug)
            .cloned();
    }
    // Re-run the filter now that active_scheme is populated so it floats to the top
    // on first open (set_schemes above called rebuild_filter when active_scheme was None).
    app.state.rebuild_filter(app.config.follow_user_scheme_type);

    let result = app.run(&mut terminal).await;
    ratatui::restore();
    result
}

fn check_binary(name: &str) -> Result<()> {
    which::which(name)
        .map(|_| ())
        .with_context(|| format!("'{name}' not found in PATH — please install it"))
}

async fn headless_populate_json_scheme(config: &Config) -> Result<()> {
    let slug = config
        .default_scheme
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("no scheme configured — apply a scheme in gnomad first"))?;

    let scheme = load_scheme_by_slug(slug, config)
        .with_context(|| format!("finding scheme '{slug}'"))?;

    // Fast path: JSON already exists and matches the current scheme name
    if let Ok(json) = std::fs::read_to_string(pipeline::gowall::PALETTE_JSON_PATH) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) {
            if value.get("name").and_then(|n| n.as_str()) == Some(scheme.name.as_str()) {
                println!("ok");
                return Ok(());
            }
        }
    }

    // File missing, unparseable, or stale: regenerate
    pipeline::gowall::write_palette_json(&scheme)?;
    println!("ok");
    Ok(())
}

fn load_scheme_by_slug(slug: &str, config: &Config) -> Result<schemes::types::Scheme> {
    let mut candidates = vec![
        (config.schemes_repo_dir.join("base16").join(format!("{slug}.yaml")), false),
        (config.schemes_repo_dir.join("base24").join(format!("{slug}.yaml")), false),
    ];
    if let Some(ref custom) = config.custom_schemes_dir {
        candidates.push((custom.join(format!("{slug}.yaml")), true));
    }

    for (path, is_custom) in &candidates {
        if let Ok(content) = std::fs::read_to_string(path) {
            return schemes::types::parse_scheme_yaml(&content, path, *is_custom)
                .with_context(|| format!("parsing {}", path.display()));
        }
    }

    anyhow::bail!("scheme '{slug}' not found in schemes repo or custom dir")
}

async fn headless_apply(slug: &str, config: &Config) -> Result<()> {
    let schemes = fetch::load_schemes(
        &config.schemes_repo_dir,
        config.custom_schemes_dir.as_deref(),
    )?;

    let scheme = schemes
        .iter()
        .find(|s| s.slug == slug)
        .ok_or_else(|| anyhow::anyhow!("scheme '{slug}' not found"))?;

    let (status_tx, _) = tokio::sync::watch::channel(String::new());
    pipeline::apply_scheme(scheme, config, None, status_tx).await?;
    println!("Applied scheme: {slug}");
    Ok(())
}
