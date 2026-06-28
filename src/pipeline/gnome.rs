use anyhow::{Context, Result};
use std::path::Path;
use std::process::Stdio;

pub struct GnomeInterface;

impl GnomeInterface {
    pub async fn new() -> Result<Self> {
        Ok(Self)
    }

    pub async fn set_wallpaper(&self, path: &Path) -> Result<()> {
        let uri = format!("file://{}", path.display());
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
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let _ = tokio::process::Command::new("gnome-extensions")
            .args(["enable", ext])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
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
        let status = tokio::process::Command::new("/usr/bin/gsettings")
            .args(["set", schema, key, value])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .with_context(|| format!("gsettings set {schema} {key}"))?;
        if !status.success() {
            anyhow::bail!("gsettings set {schema} {key} failed");
        }
        Ok(())
    }

    async fn gsettings_get(&self, schema: &str, key: &str) -> Result<String> {
        let output = tokio::process::Command::new("/usr/bin/gsettings")
            .args(["get", schema, key])
            .output()
            .await
            .with_context(|| format!("gsettings get {schema} {key}"))?;
        if !output.status.success() {
            anyhow::bail!("gsettings get {schema} {key} failed");
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}
