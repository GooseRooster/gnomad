use crate::schemes::types::{Scheme, SchemeSystem};
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

pub const PALETTE_JSON_PATH: &str = "/tmp/gnomad-current-scheme.json";

#[derive(Serialize)]
struct GowallPalette<'a> {
    name: &'a str,
    colors: Vec<String>,
}

/// Write the scheme palette to /tmp and run gowall to convert the wallpaper.
pub async fn convert_wallpaper(scheme: &Scheme, input: &Path, output: &Path) -> Result<()> {
    write_palette_json(scheme)?;
    run_gowall(input, output).await
}

pub fn write_palette_json(scheme: &Scheme) -> Result<()> {
    let colors = build_colors(scheme);
    let palette = GowallPalette {
        name: &scheme.name,
        colors,
    };
    let json = serde_json::to_string_pretty(&palette)?;
    std::fs::write(PALETTE_JSON_PATH, json)
        .context("writing /tmp/gnomad-current-scheme.json")
}

async fn run_gowall(input: &Path, output: &Path) -> Result<()> {
    let mut child = Command::new("gowall")
        .args([
            "convert",
            input.to_str().unwrap(),
            "-t",
            PALETTE_JSON_PATH,
            "--output",
            output.to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("spawning gowall")?;

    // 90-second timeout per image — gowall can stall at 0% CPU on some large
    // files; kill_on_drop ensures the process is reaped when we drop the handle.
    match tokio::time::timeout(
        tokio::time::Duration::from_secs(90),
        child.wait(),
    )
    .await
    {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => anyhow::bail!("gowall exited with {status}"),
        Ok(Err(e)) => anyhow::bail!("gowall wait failed: {e}"),
        Err(_elapsed) => {
            let _ = child.kill().await;
            anyhow::bail!("gowall timed out after 90 s for {}", input.display())
        }
    }
}

fn build_colors(scheme: &Scheme) -> Vec<String> {
    let mut colors = vec![
        format!("#{}", scheme.base00),
        format!("#{}", scheme.base01),
        format!("#{}", scheme.base02),
        format!("#{}", scheme.base03),
        format!("#{}", scheme.base04),
        format!("#{}", scheme.base05),
        format!("#{}", scheme.base06),
        format!("#{}", scheme.base07),
        format!("#{}", scheme.base08),
        format!("#{}", scheme.base09),
        format!("#{}", scheme.base0a),
        format!("#{}", scheme.base0b),
        format!("#{}", scheme.base0c),
        format!("#{}", scheme.base0d),
        format!("#{}", scheme.base0e),
        format!("#{}", scheme.base0f),
    ];

    if scheme.system == SchemeSystem::Base24 {
        for slot in [
            &scheme.base10, &scheme.base11, &scheme.base12, &scheme.base13,
            &scheme.base14, &scheme.base15, &scheme.base16, &scheme.base17,
        ] {
            if let Some(hex) = slot {
                colors.push(format!("#{hex}"));
            }
        }
    }

    colors
}
