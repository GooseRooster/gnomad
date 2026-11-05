use crate::config::GowallTuning;
use crate::pipeline::blend;
use crate::pipeline::gowall::{self, write_palette_json};
use crate::pipeline::hooks;
use crate::schemes::types::Scheme;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub scheme_slug: String,
    /// Palette fingerprint; catches custom-scheme edits that keep the slug.
    #[serde(default)]
    pub scheme_hash: Option<String>,
    /// Tuning/intensity fingerprint.
    #[serde(default)]
    pub tuning_hash: Option<String>,
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
        std::fs::write(Self::path(cache_dir), json).context("writing manifest.json")
    }

    fn matches(&self, scheme: &Scheme, tuning: &GowallTuning) -> bool {
        self.scheme_slug == scheme.slug
            && self.scheme_hash.as_deref() == Some(scheme.palette_hash().as_str())
            && self.tuning_hash.as_deref() == Some(tuning.fingerprint().as_str())
    }
}

/// Compute the per-(scheme, wallpaper-dir, tuning) cache subdirectory.
/// Default tuning keeps the original `<slug>-<dirhash>` name for compatibility.
pub fn scheme_cache_dir(
    cache_root: &Path,
    scheme: &Scheme,
    wallpaper_dir: &Path,
    tuning: &GowallTuning,
) -> PathBuf {
    let base = format!("{}-{}", scheme.slug, dir_hash(wallpaper_dir));
    if tuning.is_identity() {
        cache_root.join(base)
    } else {
        cache_root.join(format!("{base}-{}", tuning.fingerprint()))
    }
}

/// Stable FNV-1a 64-bit hash of a path string. No extra dependencies.
pub(crate) fn dir_hash(path: &Path) -> String {
    let mut h: u64 = 14695981039346656037;
    for b in path.to_string_lossy().bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    format!("{h:016x}")
}

fn mtime_secs(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Check if a specific wallpaper has already been converted for this
/// scheme/tuning combination.
pub fn is_cached(
    wallpaper_path: &Path,
    cache_dir: &Path,
    scheme: &Scheme,
    tuning: &GowallTuning,
) -> bool {
    let Some(manifest) = Manifest::load(cache_dir) else {
        return false;
    };
    if !manifest.matches(scheme, tuning) {
        return false;
    }
    let Some(filename) = wallpaper_path.file_name().and_then(|f| f.to_str()) else {
        return false;
    };
    let Some(entry) = manifest.entries.get(filename) else {
        return false;
    };

    entry.mtime_secs == mtime_secs(wallpaper_path) && cache_dir.join(filename).exists()
}

/// Get the cached wallpaper path for the given source, if it exists.
pub fn cached_path(
    wallpaper_path: &Path,
    cache_dir: &Path,
    scheme: &Scheme,
    tuning: &GowallTuning,
) -> Option<PathBuf> {
    if is_cached(wallpaper_path, cache_dir, scheme, tuning) {
        wallpaper_path.file_name().map(|f| cache_dir.join(f))
    } else {
        None
    }
}

fn temp_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("png");
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
    path.with_file_name(format!(".{stem}.{suffix}.{ext}"))
}

/// Ensure one source wallpaper exists in the cache. Converts and applies the
/// intensity blend on a miss, then updates the manifest.
pub async fn ensure_cached(
    scheme: &Scheme,
    tuning: &GowallTuning,
    cache_dir: &Path,
    source: &Path,
) -> Result<PathBuf> {
    let filename = source
        .file_name()
        .and_then(|f| f.to_str())
        .ok_or_else(|| anyhow::anyhow!("source wallpaper has no filename: {}", source.display()))?;
    let final_path = cache_dir.join(filename);

    if is_cached(source, cache_dir, scheme, tuning) {
        return Ok(final_path);
    }

    tokio::fs::create_dir_all(cache_dir).await?;
    let raw = temp_with_suffix(&final_path, "raw");
    let blend_tmp = temp_with_suffix(&final_path, "blend");

    let convert_result = gowall::convert_wallpaper(scheme, tuning, source, &raw).await;
    if let Err(e) = convert_result {
        let _ = tokio::fs::remove_file(&raw).await;
        return Err(e);
    }

    let source_path = source.to_path_buf();
    let blend_source = source_path.clone();
    let raw_for_blend = raw.clone();
    let blend_tmp_for_blend = blend_tmp.clone();
    let intensity = tuning.intensity;
    let blend_result = tokio::task::spawn_blocking(move || {
        blend::apply_intensity(
            &blend_source,
            &raw_for_blend,
            intensity,
            &blend_tmp_for_blend,
        )
    })
    .await
    .context("joining intensity blend task")?;

    let _ = tokio::fs::remove_file(&raw).await;
    if let Err(e) = blend_result {
        let _ = tokio::fs::remove_file(&blend_tmp).await;
        return Err(e);
    }
    tokio::fs::rename(&blend_tmp, &final_path).await?;

    let mut manifest = Manifest::load(cache_dir).unwrap_or_else(|| Manifest {
        scheme_slug: scheme.slug.clone(),
        scheme_hash: Some(scheme.palette_hash()),
        tuning_hash: Some(tuning.fingerprint()),
        entries: HashMap::new(),
    });
    if !manifest.matches(scheme, tuning) {
        manifest = Manifest {
            scheme_slug: scheme.slug.clone(),
            scheme_hash: Some(scheme.palette_hash()),
            tuning_hash: Some(tuning.fingerprint()),
            entries: HashMap::new(),
        };
    }
    manifest.entries.insert(
        filename.to_string(),
        ManifestEntry {
            source: source_path.to_string_lossy().to_string(),
            mtime_secs: mtime_secs(&source_path),
        },
    );
    manifest.save(cache_dir)?;

    Ok(final_path)
}

/// Batch-convert all images in wallpaper_dir for the given scheme using a single
/// `gowall convert --dir` invocation. If force is true, skip the manifest check.
#[allow(clippy::too_many_arguments)]
pub async fn batch_convert(
    scheme: &Scheme,
    tuning: &GowallTuning,
    wallpaper_dir: &Path,
    cache_dir: &Path,
    force: bool,
    hook_cmd: Option<String>,
    status_tx: tokio::sync::watch::Sender<String>,
) -> Result<()> {
    let slug_cache_dir = scheme_cache_dir(cache_dir, scheme, wallpaper_dir, tuning);
    tokio::fs::create_dir_all(&slug_cache_dir).await?;

    write_palette_json(scheme, tuning)?;

    // Collect source images and check how many still need converting.
    let mut source_entries: HashMap<String, ManifestEntry> = HashMap::new();
    let mut uncached_count = 0usize;

    let existing_manifest =
        Manifest::load(&slug_cache_dir).filter(|m| m.matches(scheme, tuning) && !force);

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
        let mtime = mtime_secs(&path);

        let already_done = existing_manifest
            .as_ref()
            .and_then(|m| m.entries.get(&filename))
            .map(|e| e.mtime_secs == mtime && slug_cache_dir.join(&filename).exists())
            .unwrap_or(false);

        if !already_done {
            uncached_count += 1;
        }

        source_entries.insert(
            filename,
            ManifestEntry {
                source: path.to_string_lossy().to_string(),
                mtime_secs: mtime,
            },
        );
    }

    let source_count = source_entries.len();

    if uncached_count == 0 {
        let _ = status_tx.send(format!("[ all {source_count} wallpapers already cached ]"));
        let envs = [
            ("GNOMAD_SCHEME_SLUG", scheme.slug.clone()),
            ("GNOMAD_CACHE_DIR", slug_cache_dir.display().to_string()),
            ("GNOMAD_WALLPAPER_COUNT", source_count.to_string()),
        ];
        hooks::run(hook_cmd.as_deref(), &envs).await;
        return Ok(());
    }

    let _ = status_tx.send(format!("[ converting {source_count} wallpapers... ]"));

    let _guard = gowall::lock_conversion().await;
    let mut command = gowall::command()?;
    let mut child = command
        .args([
            "convert",
            "--dir",
            wallpaper_dir.to_str().unwrap_or_default(),
            "-t",
            crate::pipeline::gowall::PALETTE_JSON_PATH,
            "--output",
            slug_cache_dir.to_str().unwrap_or_default(),
            "--preview",
            "false",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("spawning gowall")?;

    let ok = match tokio::time::timeout(tokio::time::Duration::from_secs(600), child.wait()).await {
        Ok(Ok(s)) => s.success(),
        _ => {
            let _ = child.kill().await;
            false
        }
    };
    drop(_guard);

    if !ok {
        let _ = status_tx.send("[ batch conversion failed ]".to_string());
        anyhow::bail!("gowall --dir conversion failed");
    }

    // gowall writes the fully-converted images. Apply the intensity blend
    // afterwards if requested, in place.
    if tuning.intensity < 100 {
        for (filename, entry) in &source_entries {
            let source = PathBuf::from(&entry.source);
            let output = slug_cache_dir.join(filename);
            if !output.exists() {
                continue;
            }
            let tmp = temp_with_suffix(&output, "blend");
            let intensity = tuning.intensity;
            let src = source.clone();
            let out = output.clone();
            let tmp_for_blend = tmp.clone();
            let result = tokio::task::spawn_blocking(move || {
                blend::apply_intensity(&src, &out, intensity, &tmp_for_blend)
            })
            .await
            .context("joining batch intensity blend task")?;
            if let Err(e) = result {
                tracing::warn!("blend failed for {}: {e:#}", output.display());
                let _ = tokio::fs::remove_file(&tmp).await;
                continue;
            }
            tokio::fs::rename(&tmp, &output).await?;
        }
    }

    // Build manifest from what gowall actually wrote to the output directory.
    let manifest = build_manifest_from_output(scheme, tuning, &source_entries, &slug_cache_dir)?;
    let completed = manifest.entries.len();
    let failed = source_count.saturating_sub(completed);
    manifest.save(&slug_cache_dir)?;

    if failed == 0 {
        let _ = status_tx.send(format!("[ converted {completed} wallpapers ]"));
        let envs = [
            ("GNOMAD_SCHEME_SLUG", scheme.slug.clone()),
            ("GNOMAD_CACHE_DIR", slug_cache_dir.display().to_string()),
            ("GNOMAD_WALLPAPER_COUNT", completed.to_string()),
        ];
        hooks::run(hook_cmd.as_deref(), &envs).await;
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
    tuning: &GowallTuning,
    source_entries: &HashMap<String, ManifestEntry>,
    slug_cache_dir: &Path,
) -> Result<Manifest> {
    let entries = source_entries
        .iter()
        .filter(|(filename, _)| slug_cache_dir.join(filename).exists())
        .map(|(filename, entry)| (filename.clone(), entry.clone()))
        .collect();
    Ok(Manifest {
        scheme_slug: scheme.slug.clone(),
        scheme_hash: Some(scheme.palette_hash()),
        tuning_hash: Some(tuning.fingerprint()),
        entries,
    })
}

fn is_image(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schemes::types::SchemeSystem;

    fn scheme() -> Scheme {
        Scheme {
            system: SchemeSystem::Base16,
            name: "Test".into(),
            slug: "test".into(),
            author: String::new(),
            variant: None,
            is_custom: true,
            base00: "1d2021".into(),
            base01: "282828".into(),
            base02: "3c3836".into(),
            base03: "504945".into(),
            base04: "665c54".into(),
            base05: "d5c4a1".into(),
            base06: "ebdbb2".into(),
            base07: "fbf1c7".into(),
            base08: "fb4934".into(),
            base09: "fe8019".into(),
            base0a: "fabd2f".into(),
            base0b: "b8bb26".into(),
            base0c: "8ec07c".into(),
            base0d: "83a598".into(),
            base0e: "d3869b".into(),
            base0f: "d65d0e".into(),
            base10: None,
            base11: None,
            base12: None,
            base13: None,
            base14: None,
            base15: None,
            base16: None,
            base17: None,
        }
    }

    #[test]
    fn default_tuning_keeps_legacy_cache_dir_name() {
        let dir = scheme_cache_dir(
            Path::new("/cache"),
            &scheme(),
            Path::new("/wallpapers"),
            &GowallTuning::default(),
        );
        let expected = format!("test-{}", dir_hash(Path::new("/wallpapers")));
        assert_eq!(dir, Path::new("/cache").join(expected));
    }

    #[test]
    fn non_default_tuning_changes_cache_dir_name() {
        let tuning = GowallTuning {
            intensity: 50,
            ..GowallTuning::default()
        };
        let dir = scheme_cache_dir(
            Path::new("/cache"),
            &scheme(),
            Path::new("/wallpapers"),
            &tuning,
        );
        assert!(dir.to_string_lossy().contains(&tuning.fingerprint()));
    }

    #[test]
    fn palette_edit_changes_scheme_hash() {
        let a = scheme();
        let mut b = scheme();
        b.base00 = "ffffff".into();
        assert_ne!(a.palette_hash(), b.palette_hash());
        assert_eq!(a.palette_hash(), scheme().palette_hash());
    }
}
