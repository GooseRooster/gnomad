use crate::config::{self, GowallTuning};
use crate::pipeline::oklab;
use crate::schemes::types::{Scheme, SchemeSystem};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

pub const PALETTE_JSON_PATH: &str = "/tmp/gnomad-current-scheme.json";

/// Serialise gowall conversions. The CLUT cache is a shared on-disk resource,
/// and two processes generating the same palette at once can race.
static GOWALL_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub async fn lock_conversion() -> tokio::sync::MutexGuard<'static, ()> {
    GOWALL_LOCK.lock().await
}

#[derive(Serialize)]
struct GowallPalette<'a> {
    name: &'a str,
    colors: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct UserGowallConfig {
    #[serde(rename = "ColorCorrectionBackend", default)]
    color_correction_backend: String,
}

#[derive(Serialize)]
struct ManagedGowallConfig {
    #[serde(rename = "EnableImagePreviewing")]
    enable_image_previewing: bool,
    #[serde(rename = "OutputFolder")]
    output_folder: String,
    #[serde(rename = "ColorCorrectionBackend")]
    color_correction_backend: String,
}

struct ManagedEnv {
    home: PathBuf,
}

/// Build a gnomad-owned HOME for gowall so tuning CLUTs land in our disposable
/// cache rather than `~/Pictures/gowall/cluts`. The user's conversion backend
/// preference (`nn`) is copied across; everything else is ours.
fn managed_env() -> Result<ManagedEnv> {
    let root = config::gowall_cache_dir();
    let home = root.join("home");
    let output = root.join("output");
    let config_dir = home.join(".config/gowall");
    std::fs::create_dir_all(&config_dir)
        .with_context(|| format!("creating {}", config_dir.display()))?;
    std::fs::create_dir_all(&output).with_context(|| format!("creating {}", output.display()))?;

    let backend = dirs::home_dir()
        .map(|h| h.join(".config/gowall/config.yml"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_yaml::from_str::<UserGowallConfig>(&s).ok())
        .map(|c| c.color_correction_backend)
        .unwrap_or_default();

    let managed = ManagedGowallConfig {
        enable_image_previewing: false,
        output_folder: output.display().to_string(),
        color_correction_backend: backend,
    };
    let yaml = serde_yaml::to_string(&managed)?;
    std::fs::write(config_dir.join("config.yml"), yaml)
        .with_context(|| format!("writing managed gowall config in {}", config_dir.display()))?;

    Ok(ManagedEnv { home })
}

/// A gowall command with the managed HOME already configured.
pub fn command() -> Result<Command> {
    let env = managed_env()?;
    let mut cmd = Command::new("gowall");
    cmd.env("HOME", &env.home);
    Ok(cmd)
}

/// Remove old managed CLUT files, keeping the newest `keep`.
pub fn prune_cluts(keep: usize) {
    let cluts = config::gowall_cache_dir().join("output/cluts");
    let Ok(read_dir) = std::fs::read_dir(&cluts) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = read_dir
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect();
    files.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (_, path) in files.into_iter().skip(keep) {
        let _ = std::fs::remove_file(path);
    }
}

/// Write the (tuned) scheme palette to /tmp and run gowall to convert the image.
pub async fn convert_wallpaper(
    scheme: &Scheme,
    tuning: &GowallTuning,
    input: &Path,
    output: &Path,
) -> Result<()> {
    write_palette_json(scheme, tuning)?;
    run_gowall(input, output).await
}

pub fn write_palette_json(scheme: &Scheme, tuning: &GowallTuning) -> Result<()> {
    let colors = build_colors(scheme, tuning);
    let palette = GowallPalette {
        name: &scheme.name,
        colors,
    };
    let json = serde_json::to_string_pretty(&palette)?;
    std::fs::write(PALETTE_JSON_PATH, json).context("writing /tmp/gnomad-current-scheme.json")
}

async fn run_gowall(input: &Path, output: &Path) -> Result<()> {
    let _guard = lock_conversion().await;
    let mut child = command()?
        .args([
            "convert",
            input.to_str().unwrap(),
            "-t",
            PALETTE_JSON_PATH,
            "--output",
            output.to_str().unwrap(),
            "--preview",
            "false",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("spawning gowall")?;

    // 90-second timeout per image — gowall can stall at 0% CPU on some large
    // files; kill_on_drop ensures the process is reaped when we drop the handle.
    match tokio::time::timeout(tokio::time::Duration::from_secs(90), child.wait()).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => anyhow::bail!("gowall exited with {status}"),
        Ok(Err(e)) => anyhow::bail!("gowall wait failed: {e}"),
        Err(_elapsed) => {
            let _ = child.kill().await;
            anyhow::bail!("gowall timed out after 90 s for {}", input.display())
        }
    }
}

fn build_colors(scheme: &Scheme, tuning: &GowallTuning) -> Vec<String> {
    let tuned = |hex: &str| format!("#{}", oklab::tune_hex(hex, tuning));

    let mut colors = vec![
        tuned(&scheme.base00),
        tuned(&scheme.base01),
        tuned(&scheme.base02),
        tuned(&scheme.base03),
        tuned(&scheme.base04),
        tuned(&scheme.base05),
        tuned(&scheme.base06),
        tuned(&scheme.base07),
        tuned(&scheme.base08),
        tuned(&scheme.base09),
        tuned(&scheme.base0a),
        tuned(&scheme.base0b),
        tuned(&scheme.base0c),
        tuned(&scheme.base0d),
        tuned(&scheme.base0e),
        tuned(&scheme.base0f),
    ];

    if scheme.system == SchemeSystem::Base24 {
        for hex in [
            &scheme.base10,
            &scheme.base11,
            &scheme.base12,
            &scheme.base13,
            &scheme.base14,
            &scheme.base15,
            &scheme.base16,
            &scheme.base17,
        ]
        .into_iter()
        .flatten()
        {
            colors.push(tuned(hex));
        }
    }

    colors
}
