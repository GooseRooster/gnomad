use crate::schemes::types::{parse_scheme_yaml, Scheme};
use anyhow::{Context, Result};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

/// Clone the tinted-theming/schemes repo on first run.
pub async fn clone_schemes_repo(repo_dir: &Path) -> Result<()> {
    if let Some(parent) = repo_dir.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let status = Command::new("git")
        .args([
            "clone",
            "--depth=1",
            "https://github.com/tinted-theming/schemes",
            repo_dir.to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .context("spawning git clone")?;

    if !status.success() {
        anyhow::bail!("git clone failed with status {status}");
    }
    Ok(())
}

/// Pull latest changes into an already-cloned repo.
pub async fn update_schemes_repo(repo_dir: &Path) -> Result<()> {
    let status = Command::new("git")
        .args(["pull", "--ff-only"])
        .current_dir(repo_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .context("spawning git pull")?;

    if !status.success() {
        anyhow::bail!("git pull failed with status {status}");
    }
    Ok(())
}

/// Load all schemes from the cloned repo and optional custom dir.
pub fn load_schemes(repo_dir: &Path, custom_dir: Option<&Path>) -> Result<Vec<Scheme>> {
    let mut schemes = Vec::new();

    for subdir in ["base16", "base24"] {
        let dir = repo_dir.join(subdir);
        if !dir.exists() {
            continue;
        }
        for entry in std::fs::read_dir(&dir)
            .with_context(|| format!("reading {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
                continue;
            }
            match load_scheme_file(&path, false) {
                Ok(s) => schemes.push(s),
                Err(e) => {
                    tracing::warn!("skipping {}: {e:#}", path.display());
                }
            }
        }
    }

    if let Some(dir) = custom_dir {
        if dir.exists() {
            for entry in std::fs::read_dir(dir)
                .with_context(|| format!("reading custom dir {}", dir.display()))?
            {
                let entry = entry?;
                let path = entry.path();
                let ext = path.extension().and_then(|e| e.to_str());
                if !matches!(ext, Some("yaml") | Some("yml")) {
                    continue;
                }
                match load_scheme_file(&path, true) {
                    Ok(s) => schemes.push(s),
                    Err(e) => {
                        tracing::warn!("skipping custom {}: {e:#}", path.display());
                    }
                }
            }
        }
    }

    schemes.sort_by(|a, b| a.slug.cmp(&b.slug));
    Ok(schemes)
}

fn load_scheme_file(path: &Path, is_custom: bool) -> Result<Scheme> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    parse_scheme_yaml(&content, path, is_custom)
        .with_context(|| format!("parsing {}", path.display()))
}
