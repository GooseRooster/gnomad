use anyhow::{Context, Result};
use std::path::Path;

pub struct GnomeInterface;

impl GnomeInterface {
    pub async fn new() -> Result<Self> {
        Ok(Self)
    }

    pub async fn set_wallpaper(&self, path: &Path) -> Result<()> {
        let uri = format!("file://{}", path.display());
        self.gsettings_set("org.gnome.desktop.background", "picture-uri", &format!("'{uri}'"))
            .await?;
        self.gsettings_set("org.gnome.desktop.background", "picture-uri-dark", &format!("'{uri}'"))
            .await?;
        Ok(())
    }

    /// Toggle color-scheme to force GNOME Shell to reload CSS, then restore.
    pub async fn reload_shell_css(&self) -> Result<()> {
        let current = self
            .gsettings_get("org.gnome.desktop.interface", "color-scheme")
            .await?;
        let current = current.trim().trim_matches('\'');

        let opposite = if current == "prefer-dark" {
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

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        self.gsettings_set(
            "org.gnome.desktop.interface",
            "color-scheme",
            &format!("'{current}'"),
        )
        .await?;

        Ok(())
    }

    pub async fn get_color_scheme(&self) -> Result<String> {
        let raw = self
            .gsettings_get("org.gnome.desktop.interface", "color-scheme")
            .await?;
        Ok(raw.trim().trim_matches('\'').to_string())
    }

    pub async fn is_user_themes_enabled(&self) -> bool {
        let result = tokio::process::Command::new("gnome-extensions")
            .args(["info", "user-theme@gnome-shell-extensions.gcampax.github.com"])
            .output()
            .await;
        match result {
            Ok(out) => String::from_utf8_lossy(&out.stdout).contains("State: ENABLED"),
            Err(_) => false,
        }
    }

    async fn gsettings_set(&self, schema: &str, key: &str, value: &str) -> Result<()> {
        let status = tokio::process::Command::new("gsettings")
            .args(["set", schema, key, value])
            .status()
            .await
            .with_context(|| format!("gsettings set {schema} {key}"))?;
        if !status.success() {
            anyhow::bail!("gsettings set {schema} {key} failed");
        }
        Ok(())
    }

    async fn gsettings_get(&self, schema: &str, key: &str) -> Result<String> {
        let output = tokio::process::Command::new("gsettings")
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
