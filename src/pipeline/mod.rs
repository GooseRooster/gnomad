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
use tracing::debug;

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
    let _ = status_tx.send("[ converting wallpaper... ]".to_string());
    let output_wall = &config.output_wallpaper_path;
    if let Some(source) = source_wallpaper {
        let cache_dir = config.wallpaper_cache_dir.join(&scheme.slug);
        debug!("wallpaper source: {}", source.display());
        debug!("wallpaper cache dir: {}", cache_dir.display());
        debug!("output wall: {}", output_wall.display());

        let cached = wallpaper_cache::cached_path(source, &cache_dir);
        if let Some(ref c) = cached {
            debug!("cache hit: {}", c.display());
            tokio::fs::copy(c, output_wall).await?;
        } else {
            debug!("cache miss — running gowall");
            gowall::convert_wallpaper(scheme, source, output_wall).await?;
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
    gtk_css::write_gtk_css(scheme).map_err(|e| { tracing::error!("gtk css: {e:#}"); e })?;

    // Step 4: Shell CSS
    let _ = status_tx.send("[ writing shell css... ]".to_string());
    debug!("writing shell css to theme: {}", config.theme_name);
    shell_css::write_shell_css(scheme, &config.theme_name)
        .map_err(|e| { tracing::error!("shell css: {e:#}"); e })?;
    shell_css::write_theme_index(&config.theme_name)?;

    // Step 5: GNOME integration
    let _ = status_tx.send("[ reloading shell... ]".to_string());
    debug!("setting wallpaper: {}", output_wall.display());
    gnome.set_wallpaper(output_wall).await?;
    debug!("reloading shell css");
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
