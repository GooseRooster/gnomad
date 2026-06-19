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
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "gnomad", about = "GNOME colour scheme and wallpaper manager")]
struct Cli {
    #[arg(long, help = "Fetch/update schemes and exit")]
    update_schemes: bool,

    #[arg(long, value_name = "SLUG", help = "Apply a scheme headlessly and exit")]
    apply: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let cli = Cli::parse();
    let config = Config::load().context("loading config")?;

    // Startup checks
    check_binary("git")?;
    check_binary("gowall")?;
    check_binary("tinty")?;

    // Ensure data dirs exist
    let data_base = config::data_dir();
    std::fs::create_dir_all(&data_base)?;
    std::fs::create_dir_all(&config.wallpaper_cache_dir)?;

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

    let result = app.run(&mut terminal).await;
    ratatui::restore();
    result
}

fn check_binary(name: &str) -> Result<()> {
    which::which(name)
        .map(|_| ())
        .with_context(|| format!("'{name}' not found in PATH — please install it"))
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
