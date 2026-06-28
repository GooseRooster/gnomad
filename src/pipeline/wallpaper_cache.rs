use crate::pipeline::gowall::write_palette_json;
use crate::schemes::types::Scheme;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

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

/// Batch-convert all images in wallpaper_dir for the given scheme using a single
/// `gowall convert --dir` invocation. If force is true, skip the manifest check.
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

    // Collect source images and check how many still need converting.
    let mut source_entries: HashMap<String, ManifestEntry> = HashMap::new();
    let mut uncached_count = 0usize;

    let existing_manifest = if force { None } else { Manifest::load(&slug_cache_dir) };

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

        let already_done = existing_manifest
            .as_ref()
            .and_then(|m| m.entries.get(&filename))
            .map(|e| e.mtime_secs == mtime && slug_cache_dir.join(&filename).exists())
            .unwrap_or(false);

        if !already_done {
            uncached_count += 1;
        }

        source_entries.insert(filename, ManifestEntry {
            source: path.to_string_lossy().to_string(),
            mtime_secs: mtime,
        });
    }

    let source_count = source_entries.len();

    if uncached_count == 0 {
        let _ = status_tx.send(format!("[ all {source_count} wallpapers already cached ]"));
        return Ok(());
    }

    let _ = status_tx.send(format!("[ converting {source_count} wallpapers... ]"));

    let mut child = tokio::process::Command::new("gowall")
        .args([
            "convert",
            "--dir", wallpaper_dir.to_str().unwrap_or_default(),
            "-t", crate::pipeline::gowall::PALETTE_JSON_PATH,
            "--output", slug_cache_dir.to_str().unwrap_or_default(),
            "--preview", "false",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("spawning gowall")?;

    let ok = match tokio::time::timeout(
        tokio::time::Duration::from_secs(600),
        child.wait(),
    )
    .await
    {
        Ok(Ok(s)) => s.success(),
        _ => {
            let _ = child.kill().await;
            false
        }
    };

    if !ok {
        let _ = status_tx.send("[ batch conversion failed ]".to_string());
        anyhow::bail!("gowall --dir conversion failed");
    }

    // Build manifest from what gowall actually wrote to the output directory.
    let manifest = build_manifest_from_output(scheme, &source_entries, &slug_cache_dir)?;
    let completed = manifest.entries.len();
    let failed = source_count.saturating_sub(completed);
    manifest.save(&slug_cache_dir)?;

    if failed == 0 {
        let _ = status_tx.send(format!("[ converted {completed} wallpapers ]"));
        Ok(())
    } else {
        let _ = status_tx.send(format!("[ {completed} done, {failed} failed ]"));
        anyhow::bail!("{failed} wallpaper(s) failed to convert")
    }
}

/// Build a manifest by checking which source images have a corresponding output
/// file in the cache dir. Only entries that were successfully written are included.
fn build_manifest_from_output(
    scheme: &Scheme,
    source_entries: &HashMap<String, ManifestEntry>,
    slug_cache_dir: &Path,
) -> Result<Manifest> {
    let entries = source_entries
        .iter()
        .filter(|(filename, _)| slug_cache_dir.join(filename).exists())
        .map(|(filename, entry)| (filename.clone(), entry.clone()))
        .collect();
    Ok(Manifest { scheme_slug: scheme.slug.clone(), entries })
}

fn is_image(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp")
    )
}
