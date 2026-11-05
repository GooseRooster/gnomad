use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;

// These vars are injected by AppImage runtimes (e.g. ghostty's sharun bundler) and
// redirect GIO away from dconf, causing gsettings writes to land in a keyfile instead
// of the session dconf database.
const GSETTINGS_POISON_VARS: &[&str] = &["GSETTINGS_BACKEND", "GIO_MODULE_DIR"];

/// Resolve the `gsettings` binary.
///
/// Prefer the system `/usr/bin/gsettings` when present — Homebrew ships its own
/// `gsettings` (pulled in transitively) that lands earlier in PATH and writes to
/// the wrong dconf database, so we must pin to the distro's copy on such setups.
/// On systems without `/usr/bin/gsettings` (notably NixOS, where everything lives
/// in the store), fall back to PATH resolution, which picks up the wrapped glib.
fn gsettings_bin() -> String {
    if let Ok(bin) = std::env::var("GNOMAD_GSETTINGS_BIN") {
        return bin;
    }
    if Path::new("/usr/bin/gsettings").exists() {
        "/usr/bin/gsettings".to_string()
    } else {
        "gsettings".to_string()
    }
}

pub struct GnomeInterface {
    /// Optional extra GSettings schema source dir (GSETTINGS_SCHEMA_DIR).
    /// Needed for third-party schemas that aren't installed system-wide —
    /// e.g. GNOME extension schemas, which on NixOS only live inside the
    /// extension's own directory. GSETTINGS_SCHEMA_DIR adds a schema source;
    /// it never hides the system ones.
    schema_dir: Option<PathBuf>,
}

impl GnomeInterface {
    pub async fn new() -> Result<Self> {
        Ok(Self { schema_dir: None })
    }

    /// Instance whose gsettings calls also consult `dir` for schemas.
    pub fn with_schema_dir(dir: PathBuf) -> Self {
        Self {
            schema_dir: Some(dir),
        }
    }

    fn gsettings_command(&self) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(gsettings_bin());
        if let Some(dir) = &self.schema_dir {
            cmd.env("GSETTINGS_SCHEMA_DIR", dir);
        }
        cmd
    }

    pub async fn set_wallpaper(&self, path: &Path) -> Result<()> {
        let uri = path_to_file_uri(path);
        self.gsettings_set(
            "org.gnome.desktop.background",
            "picture-uri",
            &format!("'{uri}'"),
        )
        .await?;
        self.gsettings_set(
            "org.gnome.desktop.background",
            "picture-uri-dark",
            &format!("'{uri}'"),
        )
        .await?;
        Ok(())
    }

    /// Set the color-scheme permanently and briefly toggle to signal GTK4/LibAdwaita apps.
    ///
    /// The toggle (opposite → target) creates a net change even if color-scheme was already
    /// at `target`, waking up apps that watch the setting. Ending at `target` also propagates
    /// dark/light mode to QT apps via xdg-desktop-portal.
    pub async fn set_color_scheme(&self, target: &str) -> Result<()> {
        let opposite = if target == "prefer-dark" {
            "prefer-light"
        } else {
            "prefer-dark"
        };

        self.gsettings_set(
            "org.gnome.desktop.interface",
            "color-scheme",
            &format!("'{opposite}'"),
        )
        .await?;

        // Give LibAdwaita time to process the "opposite" signal and re-read gtk-4.0/gtk.css
        // before we restore to target. Without this gap the two gsettings writes can be
        // coalesced and running apps never pick up the new CSS.
        // Doesn't seem necessary anymore - might restore later after further testing.
        //tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        self.gsettings_set(
            "org.gnome.desktop.interface",
            "color-scheme",
            &format!("'{target}'"),
        )
        .await?;

        Ok(())
    }

    /// Reload the GNOME Shell user-theme CSS by disabling then re-enabling the extension.
    /// This is the same mechanism Rewaita uses to force the shell to pick up new CSS.
    /// Silently does nothing if the extension is not enabled.
    pub async fn reload_shell_theme(&self) {
        if !self.is_user_themes_enabled().await {
            return;
        }
        let ext = "user-theme@gnome-shell-extensions.gcampax.github.com";
        let _ = tokio::process::Command::new("gnome-extensions")
            .args(["disable", ext])
            .env_remove("LD_LIBRARY_PATH")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let _ = tokio::process::Command::new("gnome-extensions")
            .args(["enable", ext])
            .env_remove("LD_LIBRARY_PATH")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }

    /// Best-effort current desktop wallpaper path. Prefers the dark variant,
    /// which is what GNOME uses in dark mode. Returns `None` for slideshow XML
    /// or non-file values.
    pub async fn get_wallpaper(&self) -> Result<Option<PathBuf>> {
        let dark = self
            .gsettings_get("org.gnome.desktop.background", "picture-uri-dark")
            .await
            .unwrap_or_default();
        let light = self
            .gsettings_get("org.gnome.desktop.background", "picture-uri")
            .await
            .unwrap_or_default();

        let raw = if !dark.trim_matches('\'').is_empty() {
            dark
        } else {
            light
        };
        Ok(parse_wallpaper_uri(&raw))
    }

    pub async fn get_color_scheme(&self) -> Result<String> {
        let raw = self
            .gsettings_get("org.gnome.desktop.interface", "color-scheme")
            .await?;
        Ok(raw.trim().trim_matches('\'').to_string())
    }

    pub async fn is_user_themes_enabled(&self) -> bool {
        let result = tokio::process::Command::new("gnome-extensions")
            .args(["list", "--enabled"])
            .env_remove("LD_LIBRARY_PATH")
            .output()
            .await;
        match result {
            Ok(out) => String::from_utf8_lossy(&out.stdout)
                .lines()
                .any(|l| l.trim() == "user-theme@gnome-shell-extensions.gcampax.github.com"),
            Err(_) => false,
        }
    }

    async fn gsettings_set(&self, schema: &str, key: &str, value: &str) -> Result<()> {
        tracing::debug!("gsettings set {schema} {key} {value}");
        self.warn_poison_vars();
        let mut cmd = self.gsettings_command();
        cmd.args(["set", schema, key, value])
            .env_remove("LD_LIBRARY_PATH")
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        for var in GSETTINGS_POISON_VARS {
            cmd.env_remove(var);
        }
        let output = cmd
            .output()
            .await
            .with_context(|| format!("gsettings set {schema} {key}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::error!(
                "gsettings set {schema} {key} failed (exit {:?}): {}",
                output.status.code(),
                stderr.trim()
            );
            anyhow::bail!("gsettings set {schema} {key} failed: {}", stderr.trim());
        }
        Ok(())
    }

    async fn gsettings_get(&self, schema: &str, key: &str) -> Result<String> {
        tracing::debug!("gsettings get {schema} {key}");
        self.warn_poison_vars();
        let mut cmd = self.gsettings_command();
        cmd.args(["get", schema, key])
            .env_remove("LD_LIBRARY_PATH")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for var in GSETTINGS_POISON_VARS {
            cmd.env_remove(var);
        }
        let output = cmd
            .output()
            .await
            .with_context(|| format!("gsettings get {schema} {key}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::error!(
                "gsettings get {schema} {key} failed (exit {:?}): {}",
                output.status.code(),
                stderr.trim()
            );
            anyhow::bail!("gsettings get {schema} {key} failed: {}", stderr.trim());
        }
        let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
        tracing::debug!("gsettings get {schema} {key} => {result}");
        Ok(result)
    }

    /// Public gsettings write for third-party schemas (e.g. PaperWM's relocatable
    /// workspace schemas). Same env-leak defence as the internal setter.
    pub async fn gsettings_set_public(&self, schema: &str, key: &str, value: &str) -> Result<()> {
        self.gsettings_set(schema, key, value).await
    }

    /// Public gsettings read for third-party schemas. See `gsettings_set_public`.
    pub async fn gsettings_get_public(&self, schema: &str, key: &str) -> Result<String> {
        self.gsettings_get(schema, key).await
    }

    fn warn_poison_vars(&self) {
        for var in GSETTINGS_POISON_VARS {
            if let Ok(val) = std::env::var(var) {
                tracing::warn!(
                    "AppImage env leak: {var}={val:?} — stripping before gsettings call"
                );
            }
        }
    }
}

fn path_to_file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;

    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'.' | b'_' | b'~' => {
                uri.push(byte as char);
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

fn parse_wallpaper_uri(raw: &str) -> Option<PathBuf> {
    let raw = raw.trim().trim_matches('\'');
    let rest = raw.strip_prefix("file://")?;
    // file:///path -> /path; tolerate file://host/path by keeping from the
    // first slash.
    let rest = if rest.starts_with('/') {
        rest
    } else {
        rest.find('/').map(|i| &rest[i..]).unwrap_or(rest)
    };
    let decoded = percent_decode(rest);
    let path = PathBuf::from(decoded);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    // Slideshows and formats gnomad/gowall cannot preview are not useful as
    // conversion sources.
    if !matches!(
        ext.as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp")
    ) {
        return None;
    }
    path.exists().then_some(path)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_percent_encodes_special_bytes() {
        assert_eq!(
            path_to_file_uri(Path::new("/tmp/a b#c%d'é.png")),
            "file:///tmp/a%20b%23c%25d%27%C3%A9.png"
        );
        assert_eq!(
            path_to_file_uri(Path::new("/tmp/plain-name_1.png")),
            "file:///tmp/plain-name_1.png"
        );
    }

    #[test]
    fn file_uri_round_trips_through_parser() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("space name #1.png");
        std::fs::write(&path, b"x").unwrap();

        let uri = path_to_file_uri(&path);
        assert!(uri.contains("%20"));
        assert_eq!(parse_wallpaper_uri(&format!("'{uri}'")), Some(path));
    }
}
