pub mod gnome;
pub mod gowall;
pub mod gtk_css;
pub mod palette;
pub mod shade;
pub mod shell_css;
pub mod tinty;
pub mod wallpaper_cache;

use crate::config::Config;
use crate::schemes::types::Scheme;
use anyhow::Result;
use std::path::{Path, PathBuf};
use tokio::sync::watch;

/// Run the full scheme-switch pipeline.
/// The status sender is updated at each step for the animation overlay.
pub async fn apply_scheme(
    scheme: &Scheme,
    config: &Config,
    current_wallpaper: Option<&Path>,
    status_tx: watch::Sender<String>,
) -> Result<PathBuf> {
    let gnome = gnome::GnomeInterface::new().await?;

    // Step 1: Convert wallpaper with gowall
    let _ = status_tx.send("[ converting wallpaper... ]".to_string());
    let output_wall = &config.output_wallpaper_path;
    if let Some(input) = current_wallpaper {
        let cache_dir = config.wallpaper_cache_dir.join(&scheme.slug);
        // Check if already converted for this scheme
        let cached = wallpaper_cache::cached_path(input, &cache_dir);
        let source = cached.as_deref().unwrap_or(input);
        if cached.is_none() {
            gowall::convert_wallpaper(scheme, source, output_wall).await?;
        } else {
            // Already converted — just copy to output path
            tokio::fs::copy(source, output_wall).await?;
        }
    }

    // Step 2: Tinty
    let _ = status_tx.send("[ applying tinty scheme... ]".to_string());
    tinty::apply_scheme(&scheme.slug).await?;

    // Step 3: GTK CSS
    let _ = status_tx.send("[ writing gtk css... ]".to_string());
    gtk_css::write_gtk_css(scheme)?;

    // Step 4: Shell CSS
    let _ = status_tx.send("[ writing shell css... ]".to_string());
    shell_css::write_shell_css(scheme, &config.theme_name)?;
    shell_css::write_theme_index(&config.theme_name)?;

    // Step 5: GNOME integration
    let _ = status_tx.send("[ reloading shell... ]".to_string());
    gnome.set_wallpaper(output_wall).await?;
    gnome.reload_shell_css().await?;

    Ok(output_wall.clone())
}

/// Apply a new wallpaper only (no scheme change).
/// Smart: skips gowall if the wallpaper is already cached for the current scheme.
pub async fn apply_wallpaper(
    wallpaper: &Path,
    active_scheme: Option<&Scheme>,
    config: &Config,
    status_tx: watch::Sender<String>,
) -> Result<PathBuf> {
    let gnome = gnome::GnomeInterface::new().await?;
    let output = &config.output_wallpaper_path;

    let skip_convert = active_scheme.map(|s| {
        let cache_dir = config.wallpaper_cache_dir.join(&s.slug);
        wallpaper_cache::is_cached(wallpaper, &cache_dir)
    }).unwrap_or(false);

    if skip_convert {
        let _ = status_tx.send("[ applying wallpaper (cached)... ]".to_string());
        if let Some(s) = active_scheme {
            let cache_dir = config.wallpaper_cache_dir.join(&s.slug);
            if let Some(cached) = wallpaper_cache::cached_path(wallpaper, &cache_dir) {
                tokio::fs::copy(&cached, output).await?;
            }
        }
    } else {
        let _ = status_tx.send("[ converting wallpaper... ]".to_string());
        if let Some(scheme) = active_scheme {
            gowall::convert_wallpaper(scheme, wallpaper, output).await?;
        } else {
            tokio::fs::copy(wallpaper, output).await?;
        }
    }

    let _ = status_tx.send("[ setting wallpaper... ]".to_string());
    gnome.set_wallpaper(output).await?;

    Ok(output.clone())
}
