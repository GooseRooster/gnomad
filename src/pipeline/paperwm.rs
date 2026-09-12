use crate::pipeline::gnome::GnomeInterface;
use crate::pipeline::shade::{hex_to_rgb_tuple, mix_hex};
use crate::schemes::types::Scheme;
use anyhow::Result;
use std::path::PathBuf;
use std::process::Stdio;

const PAPERWM_UUID: &str = "paperwm@paperwm.github.com";
/// PaperWM's relocatable per-workspace schema and its dconf path prefix.
/// Each workspace gets `<path>/<uuid>/` with `color` and `index` keys; the
/// ordered uuid list lives in `org.gnome.shell.extensions.paperwm.workspacelist`.
const WORKSPACE_SCHEMA: &str = "org.gnome.shell.extensions.paperwm.workspace";
const WORKSPACE_PATH_PREFIX: &str = "/org/gnome/shell/extensions/paperwm/workspaces/";
const WORKSPACE_LIST_SCHEMA: &str = "org.gnome.shell.extensions.paperwm.workspacelist";
/// Fallback when PaperWM is unreachable: its shipped default palette length.
const DEFAULT_WORKSPACE_COUNT: usize = 18;
/// Blend factor toward the highlight slot for workspace colours. PaperWM reuses
/// the exact same colour for the workspace background and the selection border
/// drawn during workspace switches, so colours must read as borders first and
/// backgrounds second — hence the strong lean toward the scheme's highlight.
const HIGHLIGHT_BLEND: f64 = 0.45;

/// Apply PaperWM theming for the given scheme (no-op when disabled).
///
/// Two halves:
/// 1. CSS: PaperWM styles its actors via its own stylesheet loaded into the
///    same St theme as gnomad's shell theme, so the PaperWM section in gnomad's
///    template (higher specificity) is simply present or absent. The shell
///    theme reload already handled by the pipeline re-applies it.
/// 2. Workspace colours: written to per-workspace relocatable schemas.
///    PaperWM connects to `changed::color` and applies updates live with a
///    smooth crossfade (tiling.js updateColor/updateBackground), including the
///    selection border — no extension toggle required.
pub async fn apply(scheme: &Scheme, enabled: bool) -> Result<()> {
    if !enabled {
        return Ok(());
    }
    let Some(schemas_dir) = extension_schemas_dir().await else {
        tracing::warn!(
            "paperwm_enabled is set but the PaperWM extension is not installed; \
             skipping workspace colour sync"
        );
        return Ok(());
    };
    // Extension schemas aren't installed system-wide (notably on NixOS), so
    // gsettings needs GSETTINGS_SCHEMA_DIR pointed at the extension's own
    // schemas directory to resolve PaperWM's schema ids.
    let gnome = GnomeInterface::with_schema_dir(schemas_dir);
    apply_workspace_colors(scheme, &gnome).await
}

/// Locate the installed PaperWM extension's schemas directory via
/// `gnome-extensions info` (its output contains a `Path:` line).
pub async fn extension_schemas_dir() -> Option<PathBuf> {
    let output = tokio::process::Command::new("gnome-extensions")
        .args(["info", PAPERWM_UUID])
        .env_remove("LD_LIBRARY_PATH")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let path_line = text.lines().find(|l| l.trim_start().starts_with("Path:"))?;
    let ext_dir = path_line.trim().strip_prefix("Path:")?.trim();
    let dir = PathBuf::from(ext_dir).join("schemas");
    if dir.is_dir() {
        Some(dir)
    } else {
        None
    }
}

/// Write `workspace-colors` (for workspaces created later) and each existing
/// workspace's per-uuid `color` key (live update + crossfade).
async fn apply_workspace_colors(scheme: &Scheme, gnome: &GnomeInterface) -> Result<()> {
    let colors = derive_workspace_colors(scheme);

    // Global key: consulted by PaperWM whenever a workspace has no explicit
    // per-workspace colour (its own default palette length is 18).
    // Values must carry the '#' prefix — PaperWM parses them with
    // Cogl/Clutter Color.from_string, which rejects bare hex.
    let strv = colors
        .iter()
        .map(|c| format!("'#{c}'"))
        .collect::<Vec<_>>()
        .join(", ");
    gnome
        .gsettings_set_public(
            "org.gnome.shell.extensions.paperwm",
            "workspace-colors",
            &format!("[{strv}]"),
        )
        .await?;

    // Per-workspace keys: update existing workspaces live.
    let uuids = workspace_uuids(gnome).await?;
    for (i, uuid) in uuids.iter().enumerate() {
        let Some(color) = colors.get(i) else { break };
        let schema_with_path = format!("{WORKSPACE_SCHEMA}:{WORKSPACE_PATH_PREFIX}{uuid}/");
        gnome
            .gsettings_set_public(&schema_with_path, "color", &format!("'#{color}'"))
            .await?;
    }
    if uuids.len() > colors.len() {
        tracing::warn!(
            "paperwm: {} workspaces but only {} palette colours; extra workspaces cycle the palette",
            uuids.len(),
            colors.len()
        );
    }

    Ok(())
}

/// Read the ordered workspace uuid list from PaperWM's workspacelist schema.
/// Falls back to an empty list when the schema can't be read; the global
/// `workspace-colors` write above still took effect in that case.
async fn workspace_uuids(gnome: &GnomeInterface) -> Result<Vec<String>> {
    let raw = match gnome
        .gsettings_get_public(WORKSPACE_LIST_SCHEMA, "list")
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                "paperwm: could not read workspace list ({e:#}); only workspace-colors updated"
            );
            return Ok(Vec::new());
        }
    };

    let inner = raw
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    let uuids = inner
        .split(',')
        .map(|s| s.trim().trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    if uuids.is_empty() {
        tracing::debug!("paperwm: no existing workspaces registered; nothing to update live");
    }
    Ok(uuids)
}

/// Derive an 18-colour workspace palette from the scheme.
///
/// PaperWM draws the workspace background AND the workspace-switch selection
/// border in the same colour, so a bare dark base colour looks wrong as a
/// border. Each colour is therefore a scheme slot blended toward the scheme's
/// highlight (base07) — light-leaning colours work as borders on any scheme —
/// then luminance-clamped so the result stays usable as a background in both
/// dark and light schemes.
pub fn derive_workspace_colors(scheme: &Scheme) -> Vec<String> {
    let highlight = &scheme.base07;

    // Rotating source slots: muted structural tones first, then the accent-ish
    // range — mirrors PaperWM's default palette vibe (muted earth/blue tones).
    let slots = [
        &scheme.base01, // dark structural
        &scheme.base02, // lighter structural
        &scheme.base0c, // cyan-ish accent
        &scheme.base0d, // blue accent
        &scheme.base0e, // purple accent
        &scheme.base0f, // brown-ish
        &scheme.base08, // red accent
        &scheme.base09, // orange accent
        &scheme.base0a, // yellow accent
        &scheme.base0b, // green accent
        &scheme.base03, // comment/muted
        &scheme.base04, // mid structural
    ];

    let mut colors = Vec::with_capacity(DEFAULT_WORKSPACE_COUNT);
    for i in 0..DEFAULT_WORKSPACE_COUNT {
        let slot = slots[i % slots.len()];
        let mut c = mix_hex(slot, highlight, HIGHLIGHT_BLEND);
        c = clamp_for_background(&c, scheme);
        colors.push(c);
    }
    colors
}

/// Keep a colour in a usable background range relative to the scheme.
/// PaperWM draws fg text on top in some widgets; avoid both near-black
/// (invisible border on dark bgs) and near-white (washed-out) extremes.
fn clamp_for_background(hex: &str, scheme: &Scheme) -> String {
    let lum = relative_luminance(hex);
    let base_lum = relative_luminance(&scheme.base00);

    // Target: clearly distinct from the window background, mid-range brightness.
    let min_dist = 0.18;
    if lum < base_lum + min_dist {
        // Too close to background: lighten toward highlight
        return mix_hex(hex, &scheme.base07, 0.35);
    }
    if lum > 0.92 {
        // Too bright overall: pull back toward the base
        return mix_hex(hex, &scheme.base00, 0.3);
    }
    hex.to_string()
}

fn relative_luminance(hex: &str) -> f32 {
    let (r, g, b) = hex_to_rgb_tuple(hex);
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) / 255.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schemes::types::SchemeSystem;

    fn test_scheme() -> Scheme {
        Scheme {
            system: SchemeSystem::Base16,
            name: "Test".into(),
            slug: "test".into(),
            author: "t".into(),
            variant: Some("dark".into()),
            is_custom: false,
            base00: "282828".into(),
            base01: "3c3836".into(),
            base02: "504945".into(),
            base03: "665c54".into(),
            base04: "bdae93".into(),
            base05: "ebdbb2".into(),
            base06: "fbf1c7".into(),
            base07: "f9f5d7".into(),
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
    fn palette_has_18_colors() {
        let colors = derive_workspace_colors(&test_scheme());
        assert_eq!(colors.len(), DEFAULT_WORKSPACE_COUNT);
    }

    #[test]
    fn colors_are_valid_hex() {
        let colors = derive_workspace_colors(&test_scheme());
        for c in &colors {
            assert_eq!(c.len(), 6, "not a 6-char hex: {c}");
            assert!(c.chars().all(|ch| ch.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn colors_differ_from_raw_slots() {
        // Highlight blend must actually change the colours
        let scheme = test_scheme();
        let colors = derive_workspace_colors(&scheme);
        assert_ne!(colors[0], scheme.base01);
        assert_ne!(colors[3], scheme.base0d);
    }

    #[test]
    fn colors_are_border_visible() {
        // Every colour should be at least somewhat distinct from base00
        // since it doubles as the workspace-switch border colour.
        let scheme = test_scheme();
        let base_lum = relative_luminance(&scheme.base00);
        for c in derive_workspace_colors(&scheme) {
            let l = relative_luminance(&c);
            assert!(
                (l - base_lum).abs() > 0.05,
                "colour {c} too close to background luminance {base_lum}"
            );
        }
    }
}
