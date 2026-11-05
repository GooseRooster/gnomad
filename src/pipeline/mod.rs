pub mod blend;
pub mod gnome;
pub mod gowall;
pub mod gtk_css;
pub mod hooks;
pub mod oklab;
pub mod palette;
pub mod paperwm;
pub mod preview;
pub mod shade;
pub mod shell_css;
pub mod slideshow;
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
    if config.gowall_only {
        return apply_scheme_gowall_only(scheme, config, source_wallpaper, status_tx).await;
    }

    let gnome = gnome::GnomeInterface::new().await?;

    // Step 1: Convert wallpaper with gowall
    if config.wallpaper_features_enabled() && source_wallpaper.is_some() {
        let _ = status_tx.send("[ converting wallpaper... ]".to_string());
    }
    let output_wall = source_wallpaper
        .map(|s| output_wallpaper_path(s, config))
        .unwrap_or_else(|| config.output_wallpaper_path.clone());

    if let Some(source) = source_wallpaper {
        let cache_dir = wallpaper_cache::scheme_cache_dir(
            &config.wallpaper_cache_dir,
            scheme,
            &config.wallpaper_dir,
            &config.gowall_tuning,
        );
        debug!("wallpaper source: {}", source.display());
        debug!("wallpaper cache dir: {}", cache_dir.display());
        debug!("output wall: {}", output_wall.display());

        let cached =
            wallpaper_cache::ensure_cached(scheme, &config.gowall_tuning, &cache_dir, source)
                .await?;
        debug!("cache ready: {}", cached.display());
        tokio::fs::copy(&cached, &output_wall).await?;
    }

    // Sync custom scheme into tinty's own custom-schemes dir so `tinty apply`
    // can find it — tinty has no visibility into gnomad's custom_schemes_dir.
    if scheme.is_custom {
        tinty::sync_custom_scheme(scheme).await?;
    }

    // Step 2: Tinty
    let _ = status_tx.send("[ applying tinty scheme... ]".to_string());
    let scheme_arg = format!("{}-{}", scheme.system.tag(true), scheme.slug);
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
    shell_css::write_shell_css(scheme, &config.theme_name, config.paperwm_enabled).map_err(
        |e| {
            tracing::error!("shell css: {e:#}");
            e
        },
    )?;
    shell_css::write_theme_index(&config.theme_name)?;

    // Step 4.5: PaperWM workspace colours (live-updatable via gsettings crossfade)
    if config.paperwm_enabled {
        let _ = status_tx.send("[ applying paperwm colors... ]".to_string());
        debug!("applying paperwm workspace colors");
        if let Err(e) = paperwm::apply(scheme, true).await {
            tracing::warn!("paperwm integration failed (non-fatal): {e:#}");
        }
    }

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

    let envs = [
        ("GNOMAD_SCHEME_SLUG", scheme.slug.clone()),
        ("GNOMAD_SCHEME_NAME", scheme.name.clone()),
        ("GNOMAD_SCHEME_SYSTEM", scheme.system.tag(true).to_string()),
        (
            "GNOMAD_SCHEME_VARIANT",
            scheme.variant.clone().unwrap_or_default(),
        ),
        (
            "GNOMAD_WALLPAPER_PATH",
            if source_wallpaper.is_some() {
                output_wall.display().to_string()
            } else {
                String::new()
            },
        ),
    ];
    hooks::run(config.hooks.on_scheme_apply.as_deref(), &envs).await;

    Ok(output_wall)
}

/// Gowall-only scheme application: convert the current wallpaper and set it.
/// No tinty, GTK CSS, Shell CSS, PaperWM, Steam CSS, or colour-scheme writes.
async fn apply_scheme_gowall_only(
    scheme: &Scheme,
    config: &Config,
    source_wallpaper: Option<&Path>,
    status_tx: watch::Sender<String>,
) -> Result<PathBuf> {
    let source = source_wallpaper.ok_or_else(|| {
        anyhow::anyhow!("no current wallpaper — choose one in the Wallpapers panel first")
    })?;
    let gnome = gnome::GnomeInterface::new().await?;

    let cache_dir = wallpaper_cache::scheme_cache_dir(
        &config.wallpaper_cache_dir,
        scheme,
        &config.wallpaper_dir,
        &config.gowall_tuning,
    );
    let _ = status_tx.send("[ converting wallpaper... ]".to_string());
    let cached =
        wallpaper_cache::ensure_cached(scheme, &config.gowall_tuning, &cache_dir, source).await?;

    let output = output_wallpaper_path(source, config);
    tokio::fs::copy(&cached, &output).await?;

    let _ = status_tx.send("[ setting wallpaper... ]".to_string());
    gnome.set_wallpaper(&output).await?;

    let mut envs = scheme_envs(scheme, Some(&output), &config.gowall_tuning);
    envs.push(("GNOMAD_GOWALL_ONLY", "1".to_string()));
    hooks::run(config.hooks.on_wallpaper_apply.as_deref(), &envs).await;

    Ok(output)
}

fn scheme_envs(
    scheme: &Scheme,
    wallpaper: Option<&Path>,
    tuning: &crate::config::GowallTuning,
) -> Vec<(&'static str, String)> {
    vec![
        ("GNOMAD_SCHEME_SLUG", scheme.slug.clone()),
        ("GNOMAD_SCHEME_NAME", scheme.name.clone()),
        ("GNOMAD_SCHEME_SYSTEM", scheme.system.tag(true).to_string()),
        (
            "GNOMAD_SCHEME_VARIANT",
            scheme.variant.clone().unwrap_or_default(),
        ),
        (
            "GNOMAD_WALLPAPER_PATH",
            wallpaper
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        ),
        ("GNOMAD_GOWALL_TUNING", tuning.fingerprint()),
    ]
}

/// Apply a new wallpaper only (no scheme change).
/// Smart: skips gowall if the wallpaper is already cached for the current scheme.
pub async fn apply_wallpaper(
    wallpaper: &Path,
    active_scheme: Option<&Scheme>,
    config: &Config,
    status_tx: watch::Sender<String>,
) -> Result<PathBuf> {
    if !config.wallpaper_features_enabled() {
        return Ok(output_wallpaper_path(wallpaper, config));
    }
    let gnome = gnome::GnomeInterface::new().await?;
    let output = output_wallpaper_path(wallpaper, config);

    if let Some(scheme) = active_scheme {
        let cache_dir = wallpaper_cache::scheme_cache_dir(
            &config.wallpaper_cache_dir,
            scheme,
            &config.wallpaper_dir,
            &config.gowall_tuning,
        );
        let cached_already =
            wallpaper_cache::is_cached(wallpaper, &cache_dir, scheme, &config.gowall_tuning);
        let _ = status_tx.send(
            if cached_already {
                "[ applying wallpaper (cached)... ]"
            } else {
                "[ converting wallpaper... ]"
            }
            .to_string(),
        );
        let cached =
            wallpaper_cache::ensure_cached(scheme, &config.gowall_tuning, &cache_dir, wallpaper)
                .await?;
        tokio::fs::copy(&cached, &output).await?;
    } else {
        let _ = status_tx.send("[ preparing wallpaper... ]".to_string());
        tokio::fs::copy(wallpaper, &output).await?;
    }

    let _ = status_tx.send("[ setting wallpaper... ]".to_string());
    gnome.set_wallpaper(&output).await?;

    let envs = if let Some(scheme) = active_scheme {
        scheme_envs(scheme, Some(&output), &config.gowall_tuning)
    } else {
        vec![
            ("GNOMAD_WALLPAPER_PATH", output.display().to_string()),
            ("GNOMAD_SCHEME_SLUG", String::new()),
            ("GNOMAD_GOWALL_TUNING", config.gowall_tuning.fingerprint()),
        ]
    };
    hooks::run(config.hooks.on_wallpaper_apply.as_deref(), &envs).await;

    Ok(output)
}
