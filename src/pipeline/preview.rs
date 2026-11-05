//! Downscaled conversion previews.
//!
//! Preview conversions are deliberately kept out of the full-size wallpaper
//! cache: browsing hundreds of schemes would otherwise create hundreds of
//! multi-megabyte PNGs. Previews are capped at `PREVIEW_MAX_PX` and pruned.

use crate::config::{self, GowallTuning};
use crate::pipeline::{blend, gowall};
use crate::schemes::types::Scheme;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const PREVIEW_MAX_PX: u32 = 1600;
const PREVIEW_KEEP: usize = 32;

fn mtime_secs(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Identity of a preview request, used to avoid restarting unchanged work.
pub fn preview_key(scheme: &Scheme, tuning: &GowallTuning, source: &Path) -> String {
    let canonical = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());
    let material = format!(
        "{}|{}|{}|{}|{}",
        scheme.palette_hash(),
        tuning.fingerprint(),
        canonical.display(),
        mtime_secs(source),
        PREVIEW_MAX_PX,
    );
    let mut h: u64 = 14695981039346656037;
    for b in material.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    format!("{h:016x}")
}

pub fn preview_path(key: &str) -> PathBuf {
    config::preview_cache_dir().join(format!("preview-{key}.png"))
}

pub fn cached_preview(scheme: &Scheme, tuning: &GowallTuning, source: &Path) -> Option<PathBuf> {
    let path = preview_path(&preview_key(scheme, tuning, source));
    path.exists().then_some(path)
}

fn downscale(source: &Path, dest: &Path) -> Result<()> {
    let img = image::open(source)
        .with_context(|| format!("opening preview source {}", source.display()))?;
    let (w, h) = (img.width(), img.height());
    let out = if w.max(h) > PREVIEW_MAX_PX {
        img.thumbnail(PREVIEW_MAX_PX, PREVIEW_MAX_PX)
    } else {
        img
    };
    out.save(dest)
        .with_context(|| format!("writing preview source {}", dest.display()))?;
    Ok(())
}

/// Generate (or reuse) a downscaled preview for `source` with `scheme`.
pub async fn generate(scheme: &Scheme, tuning: &GowallTuning, source: &Path) -> Result<PathBuf> {
    let key = preview_key(scheme, tuning, source);
    let final_path = preview_path(&key);
    if final_path.exists() {
        return Ok(final_path);
    }

    let dir = config::preview_cache_dir();
    tokio::fs::create_dir_all(&dir).await?;

    let src_tmp = dir.join(format!("src-{key}.png"));
    let raw_tmp = dir.join(format!("raw-{key}.png"));
    let blend_tmp = dir.join(format!("blend-{key}.png"));

    let src_for_task = source.to_path_buf();
    let src_tmp_for_task = src_tmp.clone();
    tokio::task::spawn_blocking(move || downscale(&src_for_task, &src_tmp_for_task))
        .await
        .context("joining preview downscale task")??;

    let result = async {
        let intensity = tuning.intensity;
        if intensity > 0 {
            gowall::convert_wallpaper(scheme, tuning, &src_tmp, &raw_tmp).await?;
        }

        let src = src_tmp.clone();
        let raw = raw_tmp.clone();
        let out = blend_tmp.clone();
        tokio::task::spawn_blocking(move || blend::apply_intensity(&src, &raw, intensity, &out))
            .await
            .context("joining preview blend task")??;

        tokio::fs::rename(&blend_tmp, &final_path).await?;
        Ok::<(), anyhow::Error>(())
    }
    .await;

    let _ = tokio::fs::remove_file(&src_tmp).await;
    let _ = tokio::fs::remove_file(&raw_tmp).await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&blend_tmp).await;
    }
    prune_previews(PREVIEW_KEEP);
    result?;
    Ok(final_path)
}

/// Keep the newest `keep` preview files; remove older ones and stray temps.
pub fn prune_previews(keep: usize) {
    let dir = config::preview_cache_dir();
    let Ok(read_dir) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = read_dir
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_str()?;
            if !name.starts_with("preview-") {
                return None;
            }
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect();
    files.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (_, path) in files.into_iter().skip(keep) {
        let _ = std::fs::remove_file(path);
    }
}
