use crate::pipeline::palette::{apply_color_map, build_color_map};
use crate::schemes::types::Scheme;
use anyhow::{Context, Result};
use std::path::PathBuf;

static SHELL_CSS: &str = include_str!("../../assets/templates/gnome-shell.css");

/// Marker where the template's PaperWM-specific section begins. Everything
/// from here on is only emitted when `paperwm_enabled` is set, so PaperWM
/// keeps its own styling otherwise.
const PAPERWM_MARKER: &str = "/* PaperWM integration";

pub fn write_shell_css(scheme: &Scheme, theme_name: &str, paperwm_enabled: bool) -> Result<()> {
    let map = build_color_map(scheme);

    let (base, paperwm) = match SHELL_CSS.find(PAPERWM_MARKER) {
        Some(idx) => (&SHELL_CSS[..idx], Some(&SHELL_CSS[idx..])),
        None => (SHELL_CSS, None),
    };

    let mut css = apply_color_map(base, &map);
    if paperwm_enabled {
        if let Some(section) = paperwm {
            css.push_str(&apply_color_map(section, &map));
        }
    }

    let path = shell_css_path(theme_name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, css).with_context(|| format!("writing {}", path.display()))
}

pub fn shell_theme_dir(theme_name: &str) -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("themes")
        .join(theme_name)
}

fn shell_css_path(theme_name: &str) -> PathBuf {
    shell_theme_dir(theme_name)
        .join("gnome-shell")
        .join("gnome-shell.css")
}

/// Write an empty index.theme file so GNOME recognizes this as a valid theme.
pub fn write_theme_index(theme_name: &str) -> Result<()> {
    let path = shell_theme_dir(theme_name).join("index.theme");
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = format!(
        "[Desktop Entry]\nType=X-GNOME-Metatheme\nName={theme_name}\nComment=gnomad generated theme\n\
         [X-GNOME-Metatheme]\nGtkTheme={theme_name}\nMetacityTheme=Adwaita\nIconTheme=Adwaita\n"
    );
    std::fs::write(&path, content).with_context(|| format!("writing {}", path.display()))
}
