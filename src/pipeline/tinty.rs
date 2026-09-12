use crate::schemes::types::{Scheme, SchemeSystem};
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use tokio::process::Command;

pub async fn apply_scheme(slug: &str) -> Result<()> {
    let output = Command::new("tinty")
        .args(["apply", slug])
        .output()
        .await
        .context("spawning tinty")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tinty apply failed: {stderr}");
    }
    Ok(())
}

/// Directory tinty scans for user-registered custom schemes — mirrors the
/// exact layout tinty's own `generate-scheme --save` writes to:
/// <tinty_data_dir>/custom-schemes/<system>/<slug>.yaml
pub fn tinty_custom_schemes_dir(system: &SchemeSystem) -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("tinted-theming")
        .join("tinty")
        .join("custom-schemes")
        .join(system.tag(true))
}

#[derive(Serialize)]
struct TintyBuilderYaml {
    system: &'static str,
    name: String,
    slug: String,
    author: String,
    variant: String,
    palette: BTreeMap<String, String>,
}

/// Serialize `scheme` into the canonical tinted-builder YAML format and write
/// it into tinty's own custom-schemes directory so `tinty apply <slug>` can
/// find it. Always overwrites, so edits to gnomad's custom scheme source YAML
/// propagate on next apply. No `tinty sync` needed — tinty's apply command
/// reads custom-schemes/ directly, bypassing its git-managed repos entirely.
///
/// Also registers the scheme's generated-output pattern in every installed
/// template repo's `.git/info/exclude` — see `register_git_excludes`.
pub async fn sync_custom_scheme(scheme: &Scheme) -> Result<()> {
    let dir = tinty_custom_schemes_dir(&scheme.system);
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("creating {}", dir.display()))?;

    let variant = scheme.variant.clone().unwrap_or_else(|| {
        tracing::debug!(
            "custom scheme '{}' has no variant set; defaulting to \"dark\" for tinty sync",
            scheme.slug
        );
        "dark".to_string()
    });

    let mut palette = BTreeMap::new();
    for (k, v) in [
        ("base00", &scheme.base00), ("base01", &scheme.base01), ("base02", &scheme.base02),
        ("base03", &scheme.base03), ("base04", &scheme.base04), ("base05", &scheme.base05),
        ("base06", &scheme.base06), ("base07", &scheme.base07), ("base08", &scheme.base08),
        ("base09", &scheme.base09), ("base0A", &scheme.base0a), ("base0B", &scheme.base0b),
        ("base0C", &scheme.base0c), ("base0D", &scheme.base0d), ("base0E", &scheme.base0e),
        ("base0F", &scheme.base0f),
    ] {
        palette.insert(k.to_string(), format!("#{v}"));
    }
    if scheme.system == SchemeSystem::Base24 {
        for (k, v) in [
            ("base10", &scheme.base10), ("base11", &scheme.base11), ("base12", &scheme.base12),
            ("base13", &scheme.base13), ("base14", &scheme.base14), ("base15", &scheme.base15),
            ("base16", &scheme.base16), ("base17", &scheme.base17),
        ] {
            if let Some(hex) = v {
                palette.insert(k.to_string(), format!("#{hex}"));
            }
        }
    }

    let doc = TintyBuilderYaml {
        system: scheme.system.tag(true),
        name: scheme.name.clone(),
        // Explicit slug so tinty's generated output files are named after
        // gnomad's slug (`<system>-<slug>.*`) instead of slugify(name) —
        // this is what makes the git-exclude pattern below exact.
        slug: scheme.slug.clone(),
        author: scheme.author.clone(),
        variant,
        palette,
    };

    let yaml = serde_yaml::to_string(&doc).context("serializing scheme for tinty sync")?;
    let out_path = dir.join(format!("{}.yaml", scheme.slug));
    tokio::fs::write(&out_path, yaml)
        .await
        .with_context(|| format!("writing {}", out_path.display()))?;
    tracing::debug!("synced custom scheme to tinty: {}", out_path.display());

    register_git_excludes(scheme).await;
    Ok(())
}

/// Keep tinty's cloned template repos git-clean despite custom-scheme output.
///
/// `tinty apply <custom-scheme>` generates theme files named
/// `*<system>-<slug>*` inside each cloned template repo. Those files are
/// untracked, so `tinty sync`/`tinty update` refuse to run ("contains
/// uncommitted changes") — tinty's check is a plain `git status --porcelain`.
///
/// Instead of committing (which needs a git identity and piles up local
/// commits that can clash with `tinty update` checkouts), gnomad adds the
/// output pattern to each repo's `.git/info/exclude`: repo-local, never
/// committed, invisible to `git status`, and ignored files even survive a
/// plain `git clean`. Idempotent — the pattern is appended only once.
async fn register_git_excludes(scheme: &Scheme) {
    // Wildcards on both sides: generated names vary — extensionless
    // (themes/ghostty/base16-peat) and prefix-embedded
    // (themes/st/st-base16-peat-0.9.3.diff) variants exist alongside the
    // plain `base16-peat.<ext>` outputs.
    let pattern = format!("*{}-{}*", scheme.system.tag(true), scheme.slug);
    let repos_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("tinted-theming")
        .join("tinty")
        .join("repos");

    let Ok(mut entries) = tokio::fs::read_dir(&repos_dir).await else {
        // No repos installed yet — tinty will clone them on its own first
        // run; the pattern gets registered on the next custom-scheme apply.
        tracing::debug!("tinty repos dir not found at {}", repos_dir.display());
        return;
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let repo = entry.path();
        if !repo.join(".git").exists() {
            continue;
        }
        let exclude_path = repo.join(".git").join("info").join("exclude");
        let marker = format!("# gnomad: generated output for custom scheme '{}'", scheme.slug);
        let existing = tokio::fs::read_to_string(&exclude_path).await.unwrap_or_default();
        if existing.lines().any(|l| l.trim() == marker) {
            continue;
        }
        let mut content = existing;
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(&format!(
            "\n# gnomad: generated output for custom scheme '{}'\n{pattern}\n",
            scheme.slug
        ));
        if let Err(e) = tokio::fs::write(&exclude_path, content).await {
            tracing::warn!(
                "could not register git exclude in {}: {e}",
                exclude_path.display()
            );
        }
    }
}
