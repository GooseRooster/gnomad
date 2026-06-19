use anyhow::{Context, Result};
use std::path::Path;
use zbus::Connection;
use zbus::proxy;

#[proxy(
    interface = "org.gnome.desktop.interface",
    default_service = "org.gnome.desktop.interface",
    default_path = "/org/gnome/desktop/interface"
)]
trait DesktopInterface {
    #[zbus(property)]
    fn picture_uri(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn set_picture_uri(&self, uri: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn picture_uri_dark(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn set_picture_uri_dark(&self, uri: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn color_scheme(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn set_color_scheme(&self, scheme: &str) -> zbus::Result<()>;
}

pub struct GnomeInterface {
    #[allow(dead_code)]
    conn: Connection,
}

impl GnomeInterface {
    pub async fn new() -> Result<Self> {
        let conn = Connection::session().await.context("connecting to session dbus")?;
        Ok(Self { conn })
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
        let current = self.gsettings_get("org.gnome.desktop.interface", "color-scheme").await?;
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

        // Brief pause to let the shell process the change
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        self.gsettings_set(
            "org.gnome.desktop.interface",
            "color-scheme",
            &format!("'{current}'"),
        )
        .await?;

        Ok(())
    }

    /// Read the current GNOME color-scheme preference.
    pub async fn get_color_scheme(&self) -> Result<String> {
        let raw = self
            .gsettings_get("org.gnome.desktop.interface", "color-scheme")
            .await?;
        Ok(raw.trim().trim_matches('\'').to_string())
    }

    /// Check whether the User Themes extension is enabled.
    pub async fn is_user_themes_enabled(&self) -> bool {
        // Query via gdbus / gsettings — extension is enabled if its schema is present
        // and the extension list includes it.
        let result = tokio::process::Command::new("gnome-extensions")
            .args(["info", "user-theme@gnome-shell-extensions.gcampax.github.com"])
            .output()
            .await;
        match result {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                stdout.contains("State: ENABLED")
            }
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
