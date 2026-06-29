pub mod gnome;
pub mod gowall;
pub mod gtk_css;
pub mod palette;
pub mod shade;
pub mod shell_css;
pub mod steam_css;
pub mod tinty;
pub mod wallpaper_cache;

use crate::config::Config;
use crate::schemes::types::Scheme;
use anyhow::Result;
use std::path::{Path, PathBuf};
use tokio::sync::watch;
use tracing::debug;

/// Derive the output wallpaper path from the source filename so that switching
/// wallpapers always produces a different URI, forcing GNOME to reload the texture.
fn output_wallpaper_path(source: &Path, config: &Config) -> PathBuf {
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("wallpaper");
    config
        .output_wallpaper_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("current-{stem}.png"))
}

/// Run the full scheme-switch pipeline.
/// `source_wallpaper` must be the ORIGINAL file from the wallpaper directory,
/// never the converted output path (to prevent quality degradation on repeat switches).
pub async fn apply_scheme(
    scheme: &Scheme,
    config: &Config,
    source_wallpaper: Option<&Path>,
    status_tx: watch::Sender<String>,
) -> Result<PathBuf> {
    let gnome = gnome::GnomeInterface::new().await?;

    // Step 1: Convert wallpaper with gowall
    if config.wallpaper_enabled && source_wallpaper.is_some() {
        let _ = status_tx.send("[ converting wallpaper... ]".to_string());
    }
    let output_wall = source_wallpaper
        .map(|s| output_wallpaper_path(s, &config))
        .unwrap_or_else(|| config.output_wallpaper_path.clone());

    if let Some(source) = source_wallpaper {
        let cache_dir = config.wallpaper_cache_dir.join(&scheme.slug);
        debug!("wallpaper source: {}", source.display());
        debug!("wallpaper cache dir: {}", cache_dir.display());
        debug!("output wall: {}", output_wall.display());

        let cached = wallpaper_cache::cached_path(source, &cache_dir);
        if let Some(ref c) = cached {
            debug!("cache hit: {}", c.display());
            tokio::fs::copy(c, &output_wall).await?;
        } else {
            debug!("cache miss — running gowall");
            gowall::convert_wallpaper(scheme, source, &output_wall).await?;
        }
    }

    // Step 2: Tinty
    let _ = status_tx.send("[ applying tinty scheme... ]".to_string());
    let scheme_arg = format!("{}-{}", &scheme.system.tag(true), scheme.slug);
    debug!("tinty apply: {scheme_arg}");
    tinty::apply_scheme(&scheme_arg).await?;

    // Step 3: GTK CSS
    let _ = status_tx.send("[ writing gtk css... ]".to_string());
    debug!("writing gtk css");
    gtk_css::write_gtk_css(scheme).map_err(|e| {
        tracing::error!("gtk css: {e:#}");
        e
    })?;

    // Step 4: Shell CSS
    let _ = status_tx.send("[ writing shell css... ]".to_string());
    debug!("writing shell css to theme: {}", config.theme_name);
    shell_css::write_shell_css(scheme, &config.theme_name).map_err(|e| {
        tracing::error!("shell css: {e:#}");
        e
    })?;
    shell_css::write_theme_index(&config.theme_name)?;

    // Step 5: Adwaita for Steam CSS
    if config.adwaita_steam_enabled {
        let _ = status_tx.send("[ writing steam css... ]".to_string());
        debug!("writing adwaita-for-steam css");
        steam_css::write_steam_css(scheme).map_err(|e| {
            tracing::error!("steam css: {e:#}");
            e
        })?;
    }

    // Step 6: GNOME integration
    let _ = status_tx.send("[ reloading shell... ]".to_string());
    debug!("setting wallpaper: {}", output_wall.display());
    if source_wallpaper.is_some() {
        gnome.set_wallpaper(&output_wall).await?;
    }

    // Toggle color-scheme to target, then set permanently.
    // - Wakes up GTK4/LibAdwaita apps (they reload CSS on color-scheme changes)
    // - Ends at the correct dark/light value so QT apps see the right mode via xdg-portal
    let color_scheme_target = match scheme.variant.as_deref() {
        Some("light") => "prefer-light",
        _ => "prefer-dark",
    };
    debug!("setting color-scheme to {color_scheme_target}");
    gnome.set_color_scheme(color_scheme_target).await?;

    // Reload GNOME Shell CSS via user-theme extension cycle (same mechanism as Rewaita)
    debug!("reloading shell theme extension");
    gnome.reload_shell_theme().await;
    Ok(output_wall)
}

/// Apply a new wallpaper only (no scheme change).
/// Smart: skips gowall if the wallpaper is already cached for the current scheme.
pub async fn apply_wallpaper(
    wallpaper: &Path,
    active_scheme: Option<&Scheme>,
    config: &Config,
    status_tx: watch::Sender<String>,
) -> Result<PathBuf> {
    if !config.wallpaper_enabled {
        return Ok(output_wallpaper_path(wallpaper, config));
    }
    let gnome = gnome::GnomeInterface::new().await?;
    let output = output_wallpaper_path(wallpaper, &config);

    let skip_convert = active_scheme
        .map(|s| {
            let cache_dir = config.wallpaper_cache_dir.join(&s.slug);
            wallpaper_cache::is_cached(wallpaper, &cache_dir)
        })
        .unwrap_or(false);

    if skip_convert {
        let _ = status_tx.send("[ applying wallpaper (cached)... ]".to_string());
        if let Some(s) = active_scheme {
            let cache_dir = config.wallpaper_cache_dir.join(&s.slug);
            if let Some(cached) = wallpaper_cache::cached_path(wallpaper, &cache_dir) {
                tokio::fs::copy(&cached, &output).await?;
            }
        }
    } else {
        let _ = status_tx.send("[ converting wallpaper... ]".to_string());
        if let Some(scheme) = active_scheme {
            gowall::convert_wallpaper(scheme, wallpaper, &output).await?;
        } else {
            tokio::fs::copy(wallpaper, &output).await?;
        }
    }

    let _ = status_tx.send("[ setting wallpaper... ]".to_string());
    gnome.set_wallpaper(&output).await?;

    Ok(output)
}
