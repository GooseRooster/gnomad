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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{test_scheme, TestEnv};
    use serial_test::serial;
    use tokio::sync::watch;

    fn test_config(env: &TestEnv, gowall_only: bool) -> Config {
        let mut config = env.config();
        config.wallpaper_enabled = true;
        config.gowall_only = gowall_only;
        config.wallpaper_cache_dir = env.root.join("data/cache");
        config.output_wallpaper_path = env.root.join("data/out/current-wallpaper.png");
        std::fs::create_dir_all(config.output_wallpaper_path.parent().unwrap()).unwrap();
        config.save().unwrap();
        config
    }

    #[tokio::test]
    #[serial]
    async fn gowall_only_touches_only_gowall_and_wallpaper() {
        let env = TestEnv::new();
        let config = test_config(&env, true);
        let source = env.wallpaper("sample.jpg");
        let scheme = test_scheme();
        let (tx, _rx) = watch::channel(String::new());

        let output = apply_scheme(&scheme, &config, Some(&source), tx)
            .await
            .unwrap();

        assert!(output.exists());
        let calls = env.calls();
        assert!(calls.contains("gowall convert"), "calls:\n{calls}");
        assert!(
            calls.contains("gsettings set org.gnome.desktop.background picture-uri"),
            "calls:\n{calls}"
        );
        assert!(!calls.contains("tinty"), "calls:\n{calls}");
        assert!(!calls.contains("color-scheme"), "calls:\n{calls}");
        assert!(!calls.contains("gnome-extensions"), "calls:\n{calls}");
        assert!(!env.gtk_css().exists());
        assert!(!env.shell_css().exists());
    }

    #[tokio::test]
    #[serial]
    async fn full_mode_runs_the_complete_pipeline() {
        let env = TestEnv::new();
        let config = test_config(&env, false);
        let source = env.wallpaper("sample.jpg");
        let scheme = test_scheme();
        let (tx, _rx) = watch::channel(String::new());

        let output = apply_scheme(&scheme, &config, Some(&source), tx)
            .await
            .unwrap();

        assert!(output.exists());
        let calls = env.calls();
        assert!(calls.contains("gowall convert"), "calls:\n{calls}");
        assert!(
            calls.contains("tinty apply base16-test-scheme"),
            "calls:\n{calls}"
        );
        assert!(
            calls.contains("gsettings set org.gnome.desktop.interface color-scheme"),
            "calls:\n{calls}"
        );
        assert!(env.gtk_css().exists());
        assert!(env.shell_css().exists());
    }

    #[tokio::test]
    #[serial]
    async fn second_apply_uses_the_wallpaper_cache() {
        let env = TestEnv::new();
        let config = test_config(&env, false);
        let source = env.wallpaper("sample.jpg");
        let scheme = test_scheme();

        let (tx, _rx) = watch::channel(String::new());
        apply_scheme(&scheme, &config, Some(&source), tx)
            .await
            .unwrap();

        env.clear_calls();
        let (tx, _rx) = watch::channel(String::new());
        apply_scheme(&scheme, &config, Some(&source), tx)
            .await
            .unwrap();

        let calls = env.calls();
        assert!(!calls.contains("gowall"), "calls:\n{calls}");
        assert!(calls.contains("tinty"), "calls:\n{calls}");
    }

    #[tokio::test]
    #[serial]
    async fn tuning_change_invalidates_the_cache() {
        let env = TestEnv::new();
        let mut config = test_config(&env, true);
        let source = env.wallpaper("sample.jpg");
        let scheme = test_scheme();

        let (tx, _rx) = watch::channel(String::new());
        apply_scheme(&scheme, &config, Some(&source), tx)
            .await
            .unwrap();

        env.clear_calls();
        config.gowall_tuning.hue_shift = 30;
        let (tx, _rx) = watch::channel(String::new());
        apply_scheme(&scheme, &config, Some(&source), tx)
            .await
            .unwrap();

        assert!(env.calls().contains("gowall"), "calls:\n{}", env.calls());
    }

    #[tokio::test]
    #[serial]
    async fn scheme_apply_hook_gets_scheme_and_wallpaper_env() {
        let env = TestEnv::new();
        let mut config = test_config(&env, false);
        config.hooks.on_scheme_apply = Some(env.hook_command("scheme-hook.env"));
        let source = env.wallpaper("sample.jpg");
        let scheme = test_scheme();
        let (tx, _rx) = watch::channel(String::new());

        let output = apply_scheme(&scheme, &config, Some(&source), tx)
            .await
            .unwrap();
        let vars = env.read_hook_env("scheme-hook.env");

        assert_eq!(vars["GNOMAD_SCHEME_SLUG"], "test-scheme");
        assert_eq!(vars["GNOMAD_SCHEME_NAME"], "Test Scheme");
        assert_eq!(vars["GNOMAD_SCHEME_SYSTEM"], "base16");
        assert_eq!(vars["GNOMAD_SCHEME_VARIANT"], "dark");
        assert_eq!(vars["GNOMAD_WALLPAPER_PATH"], output.display().to_string());
    }

    #[tokio::test]
    #[serial]
    async fn scheme_apply_hook_has_empty_wallpaper_when_disabled() {
        let env = TestEnv::new();
        let mut config = test_config(&env, false);
        config.wallpaper_enabled = false;
        config.hooks.on_scheme_apply = Some(env.hook_command("scheme-hook.env"));
        let scheme = test_scheme();
        let (tx, _rx) = watch::channel(String::new());

        apply_scheme(&scheme, &config, None, tx).await.unwrap();
        let vars = env.read_hook_env("scheme-hook.env");

        assert_eq!(vars["GNOMAD_SCHEME_SLUG"], "test-scheme");
        assert_eq!(vars["GNOMAD_WALLPAPER_PATH"], "");
    }

    #[tokio::test]
    #[serial]
    async fn gowall_only_hook_gets_wallpaper_env() {
        let env = TestEnv::new();
        let mut config = test_config(&env, true);
        config.hooks.on_scheme_apply = Some(env.hook_command("scheme-hook.env"));
        config.hooks.on_wallpaper_apply = Some(env.hook_command("wallpaper-hook.env"));
        let source = env.wallpaper("sample.jpg");
        let scheme = test_scheme();
        let (tx, _rx) = watch::channel(String::new());

        let output = apply_scheme(&scheme, &config, Some(&source), tx)
            .await
            .unwrap();
        let vars = env.read_hook_env("wallpaper-hook.env");

        assert_eq!(vars["GNOMAD_GOWALL_ONLY"], "1");
        assert_eq!(vars["GNOMAD_SCHEME_SLUG"], "test-scheme");
        assert_eq!(vars["GNOMAD_SCHEME_NAME"], "Test Scheme");
        assert_eq!(vars["GNOMAD_SCHEME_SYSTEM"], "base16");
        assert_eq!(vars["GNOMAD_SCHEME_VARIANT"], "dark");
        assert_eq!(vars["GNOMAD_WALLPAPER_PATH"], output.display().to_string());
        assert_eq!(
            vars["GNOMAD_GOWALL_TUNING"],
            config.gowall_tuning.fingerprint()
        );
        assert!(!env.hook_ran("scheme-hook.env"));
    }

    #[tokio::test]
    #[serial]
    async fn apply_wallpaper_hook_gets_active_scheme_env() {
        let env = TestEnv::new();
        let mut config = test_config(&env, true);
        config.hooks.on_wallpaper_apply = Some(env.hook_command("wallpaper-hook.env"));
        let source = env.wallpaper("sample.jpg");
        let scheme = test_scheme();
        let (tx, _rx) = watch::channel(String::new());

        let output = apply_wallpaper(&source, Some(&scheme), &config, tx)
            .await
            .unwrap();
        let vars = env.read_hook_env("wallpaper-hook.env");

        assert_eq!(vars["GNOMAD_SCHEME_SLUG"], "test-scheme");
        assert_eq!(vars["GNOMAD_SCHEME_NAME"], "Test Scheme");
        assert_eq!(vars["GNOMAD_SCHEME_SYSTEM"], "base16");
        assert_eq!(vars["GNOMAD_SCHEME_VARIANT"], "dark");
        assert_eq!(vars["GNOMAD_WALLPAPER_PATH"], output.display().to_string());
        assert_eq!(
            vars["GNOMAD_GOWALL_TUNING"],
            config.gowall_tuning.fingerprint()
        );
        assert!(!vars.contains_key("GNOMAD_GOWALL_ONLY"));
    }

    #[tokio::test]
    #[serial]
    async fn apply_wallpaper_hook_without_scheme_uses_empty_slug() {
        let env = TestEnv::new();
        let mut config = test_config(&env, true);
        config.hooks.on_wallpaper_apply = Some(env.hook_command("wallpaper-hook.env"));
        let source = env.wallpaper("sample.jpg");
        let (tx, _rx) = watch::channel(String::new());

        let output = apply_wallpaper(&source, None, &config, tx).await.unwrap();
        let vars = env.read_hook_env("wallpaper-hook.env");

        assert_eq!(vars["GNOMAD_SCHEME_SLUG"], "");
        assert!(!vars.contains_key("GNOMAD_SCHEME_NAME"));
        assert_eq!(vars["GNOMAD_WALLPAPER_PATH"], output.display().to_string());
        assert_eq!(
            vars["GNOMAD_GOWALL_TUNING"],
            config.gowall_tuning.fingerprint()
        );
    }
}

#[cfg(test)]
mod feature_tests {
    use super::*;
    use crate::test_support::{test_scheme, TestEnv};
    use serial_test::serial;
    use tokio::sync::watch;

    fn test_config(env: &TestEnv) -> Config {
        let mut config = env.config();
        config.wallpaper_enabled = true;
        config.gowall_only = true;
        config.wallpaper_cache_dir = env.root.join("data/cache");
        config.output_wallpaper_path = env.root.join("data/out/current-wallpaper.png");
        std::fs::create_dir_all(config.output_wallpaper_path.parent().unwrap()).unwrap();
        config.save().unwrap();
        config
    }

    #[tokio::test]
    #[serial]
    async fn batch_convert_populates_cache_and_manifest() {
        let env = TestEnv::new();
        let config = test_config(&env);
        let a = env.wallpaper("a.jpg");
        let b = env.wallpaper("b.jpg");
        let scheme = test_scheme();
        let (tx, _rx) = watch::channel(String::new());

        wallpaper_cache::batch_convert(
            &scheme,
            &config.gowall_tuning,
            &config.wallpaper_dir,
            &config.wallpaper_cache_dir,
            false,
            None,
            tx,
        )
        .await
        .unwrap();

        let cache_dir = wallpaper_cache::scheme_cache_dir(
            &config.wallpaper_cache_dir,
            &scheme,
            &config.wallpaper_dir,
            &config.gowall_tuning,
        );
        assert!(wallpaper_cache::is_cached(
            &a,
            &cache_dir,
            &scheme,
            &config.gowall_tuning
        ));
        assert!(wallpaper_cache::is_cached(
            &b,
            &cache_dir,
            &scheme,
            &config.gowall_tuning
        ));
        assert!(env.calls().contains("gowall convert --dir"));
    }

    #[tokio::test]
    #[serial]
    async fn slideshow_generates_xml_from_cached_images() {
        let env = TestEnv::new();
        let config = test_config(&env);
        env.wallpaper("a.jpg");
        env.wallpaper("b.jpg");
        let scheme = test_scheme();
        let (tx, _rx) = watch::channel(String::new());

        let xml = slideshow::create_slideshow(
            &config.wallpaper_dir,
            &config.wallpaper_cache_dir,
            &scheme.slug,
            &scheme,
            &config.gowall_tuning,
            10,
            20,
            None,
            tx,
        )
        .await
        .unwrap();

        assert!(xml.exists());
        let content = std::fs::read_to_string(xml).unwrap();
        assert!(content.contains("<background>"));
        assert!(content.contains("<static>"));
    }

    #[tokio::test]
    #[serial]
    async fn applying_a_wallpaper_uses_the_active_scheme() {
        let env = TestEnv::new();
        let config = test_config(&env);
        let source = env.wallpaper("sample.jpg");
        let scheme = test_scheme();
        let (tx, _rx) = watch::channel(String::new());

        let output = apply_wallpaper(&source, Some(&scheme), &config, tx)
            .await
            .unwrap();

        assert!(output.exists());
        assert!(env.calls().contains("gowall convert"));
        assert!(env
            .calls()
            .contains("gsettings set org.gnome.desktop.background picture-uri"));
    }

    #[tokio::test]
    #[serial]
    async fn batch_convert_hook_gets_cache_env() {
        let env = TestEnv::new();
        let config = test_config(&env);
        env.wallpaper("a.jpg");
        env.wallpaper("b.jpg");
        let scheme = test_scheme();
        let (tx, rx) = watch::channel(String::new());
        let hook = env.hook_command("batch-hook.env");

        wallpaper_cache::batch_convert(
            &scheme,
            &config.gowall_tuning,
            &config.wallpaper_dir,
            &config.wallpaper_cache_dir,
            false,
            Some(hook),
            tx,
        )
        .await
        .unwrap();

        let cache_dir = wallpaper_cache::scheme_cache_dir(
            &config.wallpaper_cache_dir,
            &scheme,
            &config.wallpaper_dir,
            &config.gowall_tuning,
        );
        let vars = env.read_hook_env("batch-hook.env");
        assert_eq!(vars["GNOMAD_SCHEME_SLUG"], "test-scheme");
        assert_eq!(vars["GNOMAD_CACHE_DIR"], cache_dir.display().to_string());
        assert_eq!(vars["GNOMAD_WALLPAPER_COUNT"], "2");
        assert_eq!(rx.borrow().as_str(), "[ converted 2 wallpapers ]");
    }

    #[tokio::test]
    #[serial]
    async fn batch_convert_hook_runs_when_everything_is_cached() {
        let env = TestEnv::new();
        let config = test_config(&env);
        env.wallpaper("a.jpg");
        env.wallpaper("b.jpg");
        let scheme = test_scheme();

        let (tx, _rx) = watch::channel(String::new());
        wallpaper_cache::batch_convert(
            &scheme,
            &config.gowall_tuning,
            &config.wallpaper_dir,
            &config.wallpaper_cache_dir,
            false,
            None,
            tx,
        )
        .await
        .unwrap();

        let (tx, rx) = watch::channel(String::new());
        let hook = env.hook_command("batch-hook.env");
        wallpaper_cache::batch_convert(
            &scheme,
            &config.gowall_tuning,
            &config.wallpaper_dir,
            &config.wallpaper_cache_dir,
            false,
            Some(hook),
            tx,
        )
        .await
        .unwrap();

        let vars = env.read_hook_env("batch-hook.env");
        assert_eq!(vars["GNOMAD_WALLPAPER_COUNT"], "2");
        assert_eq!(rx.borrow().as_str(), "[ all 2 wallpapers already cached ]");
    }

    #[tokio::test]
    #[serial]
    async fn batch_convert_partial_failure_reports_and_skips_hook() {
        let env = TestEnv::new();
        let config = test_config(&env);
        let a = env.wallpaper("a.jpg");
        let b = env.wallpaper("b.jpg");
        let c = env.wallpaper("c.jpg");
        std::fs::write(env.root.join("gowall.skip"), "b.jpg").unwrap();
        let scheme = test_scheme();
        let (tx, rx) = watch::channel(String::new());
        let hook = env.hook_command("batch-hook.env");

        let err = wallpaper_cache::batch_convert(
            &scheme,
            &config.gowall_tuning,
            &config.wallpaper_dir,
            &config.wallpaper_cache_dir,
            false,
            Some(hook),
            tx,
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("1 wallpaper(s) failed"), "{err:#}");
        assert_eq!(rx.borrow().as_str(), "[ 2 done, 1 failed ]");
        assert!(!env.hook_ran("batch-hook.env"));

        let cache_dir = wallpaper_cache::scheme_cache_dir(
            &config.wallpaper_cache_dir,
            &scheme,
            &config.wallpaper_dir,
            &config.gowall_tuning,
        );
        assert!(wallpaper_cache::is_cached(
            &a,
            &cache_dir,
            &scheme,
            &config.gowall_tuning
        ));
        assert!(wallpaper_cache::is_cached(
            &c,
            &cache_dir,
            &scheme,
            &config.gowall_tuning
        ));
        assert!(!wallpaper_cache::is_cached(
            &b,
            &cache_dir,
            &scheme,
            &config.gowall_tuning
        ));

        let manifest = wallpaper_cache::Manifest::load(&cache_dir).expect("manifest");
        assert_eq!(manifest.entries.len(), 2);
        assert!(manifest.entries.contains_key("a.jpg"));
        assert!(manifest.entries.contains_key("c.jpg"));
    }

    #[tokio::test]
    #[serial]
    async fn batch_convert_command_failure_reports_and_skips_hook() {
        let env = TestEnv::new();
        let config = test_config(&env);
        env.wallpaper("a.jpg");
        env.wallpaper("b.jpg");
        std::fs::write(env.root.join("gowall.fail"), "").unwrap();
        let scheme = test_scheme();
        let (tx, rx) = watch::channel(String::new());
        let hook = env.hook_command("batch-hook.env");

        let err = wallpaper_cache::batch_convert(
            &scheme,
            &config.gowall_tuning,
            &config.wallpaper_dir,
            &config.wallpaper_cache_dir,
            false,
            Some(hook),
            tx,
        )
        .await
        .unwrap_err();

        assert!(
            err.to_string().contains("gowall --dir conversion failed"),
            "{err:#}"
        );
        assert_eq!(rx.borrow().as_str(), "[ batch conversion failed ]");
        assert!(!env.hook_ran("batch-hook.env"));

        let cache_dir = wallpaper_cache::scheme_cache_dir(
            &config.wallpaper_cache_dir,
            &scheme,
            &config.wallpaper_dir,
            &config.gowall_tuning,
        );
        assert!(!cache_dir.join("manifest.json").exists());
    }

    #[tokio::test]
    #[serial]
    async fn tilde_config_paths_are_expanded_and_uris_encoded() {
        let env = TestEnv::new();
        let config_dir = env.root.join(".config/gnomad");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
wallpaper_dir = "~/Pictures/Wallpapers"
custom_schemes_dir = "~/schemes"
schemes_repo_dir = "~/schemes-repo"
output_wallpaper_path = "~/data/out/current-wallpaper.png"
wallpaper_cache_dir = "~/data/cache"
theme_name = "test-theme"
default_scheme = "test-scheme"
wallpaper_enabled = true
gowall_only = true
"#,
        )
        .unwrap();
        std::fs::create_dir_all(env.root.join("Pictures/Wallpapers")).unwrap();
        std::fs::create_dir_all(env.root.join("data/out")).unwrap();
        let source = env.root.join("Pictures/Wallpapers/space name.png");
        std::fs::write(&source, b"fake-image").unwrap();

        let config = Config::load().unwrap();
        assert_eq!(config.wallpaper_dir, env.root.join("Pictures/Wallpapers"));
        assert_eq!(config.wallpaper_cache_dir, env.root.join("data/cache"));

        let scheme = test_scheme();
        let (tx, _rx) = watch::channel(String::new());
        let output = apply_scheme(&scheme, &config, Some(&source), tx)
            .await
            .unwrap();

        assert_eq!(output, env.root.join("data/out/current-space name.png"));
        assert!(output.exists());
        let calls = env.calls();
        assert!(!calls.contains("~/.local"), "calls:\n{calls}");
        let picture_calls: Vec<&str> = calls
            .lines()
            .filter(|line| line.contains("picture-uri"))
            .collect();
        assert!(!picture_calls.is_empty(), "calls:\n{calls}");
        assert!(
            picture_calls
                .iter()
                .all(|line| line.contains("current-space%20name.png")),
            "calls:\n{calls}"
        );
    }
}
