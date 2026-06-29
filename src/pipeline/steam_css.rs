use crate::pipeline::palette::build_color_map;
use crate::pipeline::shade::hex_to_rgb_tuple;
use crate::schemes::types::Scheme;
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

pub enum SteamInstall {
    Native,
    Flatpak,
}

/// Check for an installed Adwaita for Steam theme directory.
/// Native Steam takes priority; Flatpak checked second.
pub fn detect_adwaita_steam() -> Option<SteamInstall> {
    let home = dirs::home_dir()?;
    if home.join(".steam/steam/steamui/adwaita").is_dir() {
        return Some(SteamInstall::Native);
    }
    if home
        .join(".var/app/com.valvesoftware.Steam/.steam/steam/steamui/adwaita")
        .is_dir()
    {
        return Some(SteamInstall::Flatpak);
    }
    None
}

/// Returns write targets:
/// [0] installed custom.css — takes effect after Steam restart
/// [1] AdwSteamGtk config copy — persists across GUI reinstalls; written
///     proactively so it's ready if AdwSteamGtk is installed later
fn custom_css_paths(install: &SteamInstall) -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    let steam_root = match install {
        SteamInstall::Native => home.join(".steam/steam"),
        SteamInstall::Flatpak => {
            home.join(".var/app/com.valvesoftware.Steam/.steam/steam")
        }
    };
    vec![
        steam_root.join("steamui/adwaita/custom/custom.css"),
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

fn rgb(map: &HashMap<String, String>, key: &str) -> String {
    let hex = map.get(key).map(String::as_str).unwrap_or("000000");
    let (r, g, b) = hex_to_rgb_tuple(hex);
    format!("{r}, {g}, {b}")
}

fn generate_css(map: &HashMap<String, String>) -> String {
    format!(
        ":root {{\n\
        \t/* Accent */\n\
        \t--adw-accent-bg-rgb: {accent_bg} !important;\n\
        \t--adw-accent-fg-rgb: {accent_fg} !important;\n\
        \t--adw-accent-rgb: {accent} !important;\n\
        \n\
        \t/* Destructive */\n\
        \t--adw-destructive-bg-rgb: {dest_bg} !important;\n\
        \t--adw-destructive-fg-rgb: {dest_fg} !important;\n\
        \t--adw-destructive-rgb: {dest} !important;\n\
        \n\
        \t/* Success */\n\
        \t--adw-success-bg-rgb: {succ_bg} !important;\n\
        \t--adw-success-fg-rgb: {succ_fg} !important;\n\
        \t--adw-success-rgb: {succ} !important;\n\
        \n\
        \t/* Warning */\n\
        \t--adw-warning-bg-rgb: {warn_bg} !important;\n\
        \t--adw-warning-fg-rgb: {warn_fg} !important;\n\
        \t--adw-warning-rgb: {warn} !important;\n\
        \n\
        \t/* Error */\n\
        \t--adw-error-bg-rgb: {err_bg} !important;\n\
        \t--adw-error-fg-rgb: {err_fg} !important;\n\
        \t--adw-error-rgb: {err} !important;\n\
        \n\
        \t/* Window */\n\
        \t--adw-window-bg-rgb: {win_bg} !important;\n\
        \t--adw-window-fg-rgb: {win_fg} !important;\n\
        \n\
        \t/* View */\n\
        \t--adw-view-bg-rgb: {view_bg} !important;\n\
        \t--adw-view-fg-rgb: {view_fg} !important;\n\
        \n\
        \t/* Headerbar */\n\
        \t--adw-headerbar-bg-rgb: {hdr_bg} !important;\n\
        \t--adw-headerbar-fg-rgb: {hdr_fg} !important;\n\
        \t--adw-headerbar-border-rgb: {hdr_border} !important;\n\
        \n\
        \t/* Sidebar */\n\
        \t--adw-sidebar-bg-rgb: {side_bg} !important;\n\
        \t--adw-sidebar-fg-rgb: {side_fg} !important;\n\
        \t--adw-sidebar-backdrop-rgb: {side_back} !important;\n\
        \t--adw-secondary-sidebar-bg-rgb: {side_bg} !important;\n\
        \t--adw-secondary-sidebar-fg-rgb: {side_fg} !important;\n\
        \t--adw-secondary-sidebar-backdrop-rgb: {win_bg} !important;\n\
        \n\
        \t/* Card */\n\
        \t--adw-card-fg-rgb: {card_fg} !important;\n\
        \n\
        \t/* Dialog */\n\
        \t--adw-dialog-bg-rgb: {dlg_bg} !important;\n\
        \t--adw-dialog-fg-rgb: {dlg_fg} !important;\n\
        \n\
        \t/* Popover */\n\
        \t--adw-popover-bg-rgb: {pop_bg} !important;\n\
        \t--adw-popover-fg-rgb: {pop_fg} !important;\n\
        \n\
        \t/* Misc */\n\
        \t--adw-thumbnail-fg-rgb: {win_fg} !important;\n\
        \t--adw-banner-fg-rgb: {win_fg} !important;\n\
        }}",
        accent_bg = rgb(map, "accent_bg_color"),
        accent_fg = rgb(map, "accent_fg_color"),
        accent = rgb(map, "accent_color"),
        dest_bg = rgb(map, "destructive_bg_color"),
        dest_fg = rgb(map, "destructive_fg_color"),
        dest = rgb(map, "destructive_color"),
        succ_bg = rgb(map, "success_bg_color"),
        succ_fg = rgb(map, "success_fg_color"),
        succ = rgb(map, "success_color"),
        warn_bg = rgb(map, "warning_bg_color"),
        warn_fg = rgb(map, "warning_fg_color"),
        warn = rgb(map, "warning_color"),
        err_bg = rgb(map, "error_bg_color"),
        err_fg = rgb(map, "error_fg_color"),
        err = rgb(map, "error_color"),
        win_bg = rgb(map, "window_bg_color"),
        win_fg = rgb(map, "window_fg_color"),
        view_bg = rgb(map, "view_bg_color"),
        view_fg = rgb(map, "view_fg_color"),
        hdr_bg = rgb(map, "headerbar_bg_color"),
        hdr_fg = rgb(map, "headerbar_fg_color"),
        hdr_border = rgb(map, "headerbar_border_color"),
        side_bg = rgb(map, "sidebar_bg_color"),
        side_fg = rgb(map, "sidebar_fg_color"),
        side_back = rgb(map, "sidebar_backdrop_color"),
        card_fg = rgb(map, "card_fg_color"),
        dlg_bg = rgb(map, "dialog_bg_color"),
        dlg_fg = rgb(map, "dialog_fg_color"),
        pop_bg = rgb(map, "popover_bg_color"),
        pop_fg = rgb(map, "popover_fg_color"),
    )
}
