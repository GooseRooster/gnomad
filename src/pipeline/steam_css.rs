use crate::pipeline::palette::build_color_map;
use crate::schemes::types::Scheme;
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

pub enum SteamInstall {
    Native,
    Flatpak,
}

/// Check for an installed Adwaita for Steam theme directory.
/// Flatpak is checked first using the canonical .local/share path to avoid
/// false-positive native detection when only the adwaita dir was created by a
/// prior gnomad write (not a real Steam installation).
pub fn detect_adwaita_steam() -> Option<SteamInstall> {
    let home = dirs::home_dir()?;
    // Flatpak: use canonical path, not the .steam/steam symlink
    if home
        .join(".var/app/com.valvesoftware.Steam/.local/share/Steam/steamui/adwaita")
        .is_dir()
    {
        return Some(SteamInstall::Flatpak);
    }
    // Native: prefer the canonical ~/.local/share/Steam location
    if home.join(".local/share/Steam/steamui/adwaita").is_dir() {
        return Some(SteamInstall::Native);
    }
    // Fallback native: .steam/steam path, but only if steam.sh exists to
    // confirm this is a real installation and not a stray adwaita directory.
    if home.join(".steam/steam/steamui/adwaita").is_dir()
        && home.join(".steam/steam/steam.sh").exists()
    {
        return Some(SteamInstall::Native);
    }
    None
}

/// Returns write targets:
/// [0] installed custom.css — the skin's `@import url("custom.css")` (from
///     config.css) resolves here, so it takes effect after Steam restart
/// [1] AdwSteamGtk config copy — persists across GUI reinstalls; written
///     proactively so it's ready if AdwSteamGtk is installed later
fn custom_css_paths(install: &SteamInstall) -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    let steam_root = match install {
        SteamInstall::Flatpak => {
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam")
        }
        SteamInstall::Native => {
            let canonical = home.join(".local/share/Steam");
            if canonical.exists() { canonical } else { home.join(".steam/steam") }
        }
    };
    vec![
        steam_root.join("steamui/adwaita/custom.css"),
        dirs::config_dir()
            .unwrap_or_else(|| home.join(".config"))
            .join("AdwSteamGtk/custom.css"),
    ]
}

pub fn write_steam_css(scheme: &Scheme) -> Result<()> {
    let Some(install) = detect_adwaita_steam() else {
        return Ok(());
    };
    let map = build_color_map(scheme);
    let css = generate_css(&map);
    for path in custom_css_paths(&install) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, &css)?;
    }
    Ok(())
}

/// Format a colour map value as a `#hex` colour (Adwaita for Steam v4.4+
/// palette variables take full colour values, not `-rgb` triplets).
fn hex(map: &HashMap<String, String>, key: &str) -> String {
    format!("#{}", map.get(key).map(String::as_str).unwrap_or("000000"))
}

/// Generate custom.css against Adwaita for Steam's current palette.
///
/// The skin (v4.4) exposes semantic surface variables as `light-dark()`
/// pairs; overriding them with flat values makes the skin follow gnomad's
/// active scheme regardless of the system colour-scheme preference (gnomad
/// rewrites this file on every scheme switch, and the watcher reinstalls).
///
/// Some variables split into `-light`/`-dark` halves (they only exist as a
/// pair); both get the same colour. `--adw-banner` is the notification
/// banner background wash; its text already follows `--adw-window-fg`.
fn generate_css(map: &HashMap<String, String>) -> String {
    format!(
        "/* gnomad → Adwaita for Steam (v4.4 palette variables) */\n\
        :root {{\n\
        \t/* Accent */\n\
        \t--adw-accent-bg-light: {accent} !important;\n\
        \t--adw-accent-bg-dark: {accent} !important;\n\
        \t--adw-accent-fg: {accent_fg} !important;\n\
        \t--adw-accent: {accent} !important;\n\
        \n\
        \t/* Destructive */\n\
        \t--adw-destructive-bg-light: {dest} !important;\n\
        \t--adw-destructive-bg-dark: {dest} !important;\n\
        \t--adw-destructive-fg: {dest_fg} !important;\n\
        \t--adw-destructive: {dest} !important;\n\
        \n\
        \t/* Success */\n\
        \t--adw-success-bg-light: {succ} !important;\n\
        \t--adw-success-bg-dark: {succ} !important;\n\
        \t--adw-success-fg: {succ_fg} !important;\n\
        \t--adw-success: {succ} !important;\n\
        \n\
        \t/* Warning */\n\
        \t--adw-warning-bg-light: {warn} !important;\n\
        \t--adw-warning-bg-dark: {warn} !important;\n\
        \t--adw-warning-fg: {warn_fg} !important;\n\
        \t--adw-warning: {warn} !important;\n\
        \n\
        \t/* Error */\n\
        \t--adw-error-bg-light: {err} !important;\n\
        \t--adw-error-bg-dark: {err} !important;\n\
        \t--adw-error-fg: {err_fg} !important;\n\
        \t--adw-error: {err} !important;\n\
        \n\
        \t/* Surfaces */\n\
        \t--adw-window-bg: {win_bg} !important;\n\
        \t--adw-window-fg: {win_fg} !important;\n\
        \t--adw-view-bg: {view_bg} !important;\n\
        \t--adw-view-fg: {view_fg} !important;\n\
        \t--adw-headerbar-bg: {hdr_bg} !important;\n\
        \t--adw-headerbar-fg: {hdr_fg} !important;\n\
        \t--adw-headerbar-backdrop: {hdr_back} !important;\n\
        \t--adw-headerbar-shade: {hdr_border} !important;\n\
        \n\
        \t/* Sidebars */\n\
        \t--adw-sidebar-bg: {side_bg} !important;\n\
        \t--adw-sidebar-fg: {side_fg} !important;\n\
        \t--adw-sidebar-backdrop: {side_back} !important;\n\
        \t--adw-secondary-sidebar-bg: {side_bg} !important;\n\
        \t--adw-secondary-sidebar-fg: {side_fg} !important;\n\
        \t--adw-secondary-sidebar-backdrop: {win_bg} !important;\n\
        \n\
        \t/* Cards, dialogs, popovers, misc */\n\
        \t--adw-card-bg: {card_bg} !important;\n\
        \t--adw-card-fg: {card_fg} !important;\n\
        \t--adw-dialog-bg: {dlg_bg} !important;\n\
        \t--adw-dialog-fg: {dlg_fg} !important;\n\
        \t--adw-popover-bg: {pop_bg} !important;\n\
        \t--adw-popover-fg: {pop_fg} !important;\n\
        \t--adw-thumbnail-fg: {win_fg} !important;\n\
        \t--adw-banner: {side_bg} !important;\n\
        }}",
        accent = hex(map, "accent-bg-color"),
        accent_fg = hex(map, "accent-fg-color"),
        dest = hex(map, "destructive-bg-color"),
        dest_fg = hex(map, "destructive-fg-color"),
        succ = hex(map, "success-bg-color"),
        succ_fg = hex(map, "success-fg-color"),
        warn = hex(map, "warning-bg-color"),
        warn_fg = hex(map, "warning-fg-color"),
        err = hex(map, "error-bg-color"),
        err_fg = hex(map, "error-fg-color"),
        win_bg = hex(map, "window-bg-color"),
        win_fg = hex(map, "window-fg-color"),
        view_bg = hex(map, "view-bg-color"),
        view_fg = hex(map, "view-fg-color"),
        hdr_bg = hex(map, "headerbar-bg-color"),
        hdr_fg = hex(map, "headerbar-fg-color"),
        hdr_back = hex(map, "headerbar-backdrop-color"),
        hdr_border = hex(map, "headerbar-border-color"),
        side_bg = hex(map, "sidebar-bg-color"),
        side_fg = hex(map, "sidebar-fg-color"),
        side_back = hex(map, "sidebar-backdrop-color"),
        card_bg = hex(map, "card-bg-color"),
        card_fg = hex(map, "card-fg-color"),
        dlg_bg = hex(map, "dialog-bg-color"),
        dlg_fg = hex(map, "dialog-fg-color"),
        pop_bg = hex(map, "popover-bg-color"),
        pop_fg = hex(map, "popover-fg-color"),
    )
}
