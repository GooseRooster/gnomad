use crate::pipeline::gowall::write_palette_json;
use crate::schemes::types::Scheme;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::task::JoinSet;

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub scheme_slug: String,
    pub entries: HashMap<String, ManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub source: String,
    pub mtime_secs: u64,
}

impl Manifest {
    fn path(cache_dir: &Path) -> PathBuf {
        cache_dir.join("manifest.json")
    }

    pub fn load(cache_dir: &Path) -> Option<Self> {
        let content = std::fs::read_to_string(Self::path(cache_dir)).ok()?;
        serde_json::from_str(&content).ok()
    }

    pub fn save(&self, cache_dir: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(Self::path(cache_dir), json)
            .context("writing manifest.json")
    }
}

/// Check if a specific wallpaper has already been converted for this scheme.
pub fn is_cached(wallpaper_path: &Path, cache_dir: &Path) -> bool {
    let Some(manifest) = Manifest::load(cache_dir) else {
        return false;
    };
    let Some(filename) = wallpaper_path.file_name().and_then(|f| f.to_str()) else {
        return false;
    };
    let Some(entry) = manifest.entries.get(filename) else {
        return false;
    };

    // Check mtime matches
    let current_mtime = std::fs::metadata(wallpaper_path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    entry.mtime_secs == current_mtime && cache_dir.join(filename).exists()
}

/// Get the cached wallpaper path for the given source, if it exists.
pub fn cached_path(wallpaper_path: &Path, cache_dir: &Path) -> Option<PathBuf> {
    if is_cached(wallpaper_path, cache_dir) {
        wallpaper_path
            .file_name()
            .map(|f| cache_dir.join(f))
    } else {
        None
    }
}

/// Batch-convert all images in wallpaper_dir for the given scheme.
/// If force is true, skip manifest check and reconvert everything.
pub async fn batch_convert(
    scheme: &Scheme,
    wallpaper_dir: &Path,
    cache_dir: &Path,
    force: bool,
    status_tx: tokio::sync::watch::Sender<String>,
) -> Result<()> {
    let slug_cache_dir = cache_dir.join(&scheme.slug);
    tokio::fs::create_dir_all(&slug_cache_dir).await?;

    write_palette_json(scheme)?;

    let existing_manifest = if force {
        None
    } else {
        Manifest::load(&slug_cache_dir)
    };

    // Collect images to convert
    let mut to_convert: Vec<PathBuf> = Vec::new();
    let mut all_entries: HashMap<String, ManifestEntry> = HashMap::new();

    let mut read_dir = tokio::fs::read_dir(wallpaper_dir)
        .await
        .context("reading wallpaper dir")?;

    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();
        if !is_image(&path) {
            continue;
        }
        let filename = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("")
            .to_string();

        let mtime = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        all_entries.insert(
            filename.clone(),
            ManifestEntry {
                source: path.to_string_lossy().to_string(),
                mtime_secs: mtime,
            },
        );

        let already_done = existing_manifest.as_ref().and_then(|m| m.entries.get(&filename))
            .map(|e| e.mtime_secs == mtime && slug_cache_dir.join(&filename).exists())
            .unwrap_or(false);

        if !already_done {
            to_convert.push(path);
        }
    }

    let total = to_convert.len();
    if total == 0 {
        let _ = status_tx.send(format!("[ all {} wallpapers already cached ]", all_entries.len()));
        return Ok(());
    }

    let mut set: JoinSet<Result<()>> = JoinSet::new();
    let slug_cache_dir_arc = std::sync::Arc::new(slug_cache_dir.clone());

    for (i, src) in to_convert.iter().enumerate() {
        let _ = status_tx.send(format!("[ converting {} of {} wallpapers... ]", i + 1, total));
        let src = src.clone();
        let cache = slug_cache_dir_arc.clone();
        let dst = cache.join(src.file_name().unwrap());

        set.spawn(async move {
            let status = tokio::process::Command::new("gowall")
                .args([
                    "convert",
                    src.to_str().unwrap(),
                    "-t",
                    crate::pipeline::gowall::PALETTE_JSON_PATH,
                    "--output",
                    dst.to_str().unwrap(),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .context("spawning gowall")?;
            if !status.success() {
                anyhow::bail!("gowall failed for {}", src.display());
            }
            Ok(())
        });
    }

    while let Some(result) = set.join_next().await {
        result.context("task panicked")??;
    }

    let manifest = Manifest {
        scheme_slug: scheme.slug.clone(),
        entries: all_entries,
    };
    manifest.save(&slug_cache_dir)?;

    let _ = status_tx.send(format!("[ converted {total} wallpapers ]"));
    Ok(())
}

fn is_image(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp")
    )
}
