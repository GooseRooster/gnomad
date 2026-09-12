use crate::pipeline::shade::{gtk_shade, hex_to_rgb_tuple, mix_hex, shades};
use crate::schemes::types::Scheme;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Build the full variable→hex substitution map used for CSS templating.
///
/// Keys are bare variable names (no `@`), hyphenated to match Rewaita's
/// upstream convention (`@window-bg-color`). Values are hex colour strings
/// (no `#`) or rgba() expressions.
///
/// Includes every token the templates reference (semantic colours, palette
/// family shades) plus all derived `@define-color` values from upstream's
/// GTK3 header block, resolved to concrete hex so substitution covers them.
pub fn build_color_map(scheme: &Scheme) -> HashMap<String, String> {
    let mut m: HashMap<String, String> = HashMap::new();

    // ── Semantic / base colours ───────────────────────────────────────────────
    let add = |m: &mut HashMap<String, String>, name: &str, hex: &str| {
        m.insert(name.to_string(), hex.to_string());
    };

    add(&mut m, "window-bg-color", &scheme.base00);
    add(&mut m, "window-fg-color", &scheme.base05);
    add(&mut m, "view-bg-color", &scheme.base00);
    add(&mut m, "view-fg-color", &scheme.base05);
    add(&mut m, "headerbar-bg-color", &scheme.base01);
    add(&mut m, "headerbar-fg-color", &scheme.base05);
    add(&mut m, "card-bg-color", &scheme.base01);
    add(&mut m, "card-fg-color", &scheme.base05);
    add(&mut m, "sidebar-bg-color", &scheme.base01);
    add(&mut m, "sidebar-fg-color", &scheme.base05);
    add(&mut m, "sidebar-backdrop-color", &scheme.base00);
    add(&mut m, "sidebar-border-color", &scheme.base03);
    add(&mut m, "panel-bg-color", &scheme.base01);
    add(&mut m, "panel-fg-color", &scheme.base05);
    add(&mut m, "overview-bg-color", &scheme.base00);
    add(&mut m, "search-fg-color", &scheme.base05);
    add(&mut m, "color-fg-color", &scheme.base05);
    add(&mut m, "border-color", &scheme.base03);

    // LibAdwaita window decoration tokens
    add(&mut m, "headerbar-backdrop-color", &scheme.base00);
    add(&mut m, "headerbar-shade-color", &scheme.base00);
    add(&mut m, "headerbar-border-color", &scheme.base03);
    add(&mut m, "card-shade-color", &scheme.base00);
    add(&mut m, "dialog-bg-color", &scheme.base01);
    add(&mut m, "dialog-fg-color", &scheme.base05);
    add(&mut m, "popover-bg-color", &scheme.base01);
    add(&mut m, "popover-fg-color", &scheme.base05);

    add(&mut m, "accent-color", &scheme.base0d);
    add(&mut m, "accent-bg-color", &scheme.base0d);
    // Choose fg that contrasts against the accent background
    let accent_fg = if luminance(&scheme.base0d) > 0.4 {
        &scheme.base00
    } else {
        &scheme.base07
    };
    add(&mut m, "accent-fg-color", accent_fg);

    add(&mut m, "success-bg-color", &scheme.base0b);
    add(&mut m, "success-fg-color", &scheme.base07);

    add(&mut m, "warning-color", &scheme.base0a);
    add(&mut m, "warning-bg-color", &scheme.base0a);
    add(&mut m, "warning-fg-color", &scheme.base00);

    add(&mut m, "destructive-color", &scheme.base08);
    add(&mut m, "destructive-bg-color", &scheme.base08);
    add(&mut m, "destructive-fg-color", &scheme.base07);

    add(&mut m, "error-bg-color", &scheme.base08);
    add(&mut m, "error-fg-color", &scheme.base07);

    // accent-transparent used by gnome-shell template (rgba, special-cased in
    // apply_color_map since it is not a bare hex value)
    let (r, g, b) = hex_to_rgb_tuple(&scheme.base0d);
    m.insert(
        "accent-transparent".to_string(),
        format!("rgba({r}, {g}, {b}, 0.5)"),
    );

    // ── Palette family shades ─────────────────────────────────────────────────
    let families: &[(&str, &str)] = &[
        ("blue", &scheme.base0d),
        ("green", &scheme.base0b),
        ("yellow", &scheme.base0a),
        ("orange", &scheme.base09),
        ("red", &scheme.base08),
        ("purple", &scheme.base0e),
        ("brown", &scheme.base0f),
        ("light", &scheme.base07),
        ("dark", &scheme.base01),
    ];

    for (family, base_hex) in families {
        let s = shades(base_hex);
        for (i, shade_hex) in s.iter().enumerate() {
            let key = format!("{family}-{}", i + 1);
            m.insert(key, shade_hex.clone());
        }
    }

    // ── Derived define-color values from upstream's GTK3 header block ────────
    //
    // The compact GTK3 template resolves these via its @define-color header.
    // gnomad strips that header and resolves every expression to concrete hex
    // here, so substitution works uniformly (GTK3 gets the block prepended
    // with these same values — see generate_define_color_block).
    let win_bg = scheme.base00.clone();
    let win_fg = scheme.base05.clone();
    let view_bg = scheme.base00.clone();
    let view_fg = scheme.base05.clone();

    // theme tokens
    add(&mut m, "theme-fg-color", &win_fg);
    add(&mut m, "theme-text-color", &view_fg);
    add(&mut m, "theme-bg-color", &win_bg);
    add(&mut m, "theme-base-color", &view_bg);
    add(&mut m, "theme-selected-bg-color", &scheme.base0d);
    add(&mut m, "theme-selected-fg-color", accent_fg);
    // insensitive
    add(
        &mut m,
        "insensitive-bg-color",
        &mix_hex(&win_bg, &view_bg, 0.4),
    );
    // alpha(win-fg, 0.5) — kept as rgba for correctness
    let (fr, fgc, fb) = hex_to_rgb_tuple(&win_fg);
    m.insert(
        "insensitive-fg-color".to_string(),
        format!("rgba({fr}, {fgc}, {fb}, 0.5)"),
    );
    add(&mut m, "insensitive-base-color", &view_bg);
    // theme-unfocused
    let unfocused_fg = mix_hex(&win_fg, &win_bg, 0.5);
    add(&mut m, "theme-unfocused-fg-color", &unfocused_fg);
    add(&mut m, "theme-unfocused-text-color", &view_fg);
    add(&mut m, "theme-unfocused-bg-color", &win_bg);
    add(&mut m, "theme-unfocused-base-color", &win_bg);
    add(&mut m, "theme-unfocused-selected-bg-color", &scheme.base0d);
    add(&mut m, "theme-unfocused-selected-fg-color", accent_fg);
    add(
        &mut m,
        "unfocused-insensitive-color",
        &mix_hex(&unfocused_fg, &win_bg, 0.5),
    );
    // borders: mix(currentColor, window-bg, 0.85). currentColor is unknown at
    // generation time; window-fg is the practical stand-in used for borders.
    let borders = mix_hex(&win_fg, &win_bg, 0.85);
    add(&mut m, "borders", &borders);
    // Upstream bug: the template references @borders-color in some rules but
    // never defines it. Treat it as the semantic border colour.
    add(&mut m, "borders-color", &scheme.base03);
    add(
        &mut m,
        "unfocused-borders",
        &mix_hex(&win_fg, &win_bg, 0.73),
    );
    // wm tokens (GTK shade() == HSL lightness scaling)
    add(&mut m, "wm-title", &gtk_shade(&win_fg, 1.8));
    add(&mut m, "wm-unfocused-title", &unfocused_fg);
    add(&mut m, "wm-highlight", "000000");
    let (wr, wg, wb) = hex_to_rgb_tuple(&win_fg);
    m.insert(
        "wm-borders-edge".to_string(),
        format!("rgba({wr}, {wg}, {wb}, 0.07)"),
    );
    add(&mut m, "wm-bg-a", &gtk_shade(&win_bg, 1.2));
    add(&mut m, "wm-bg-b", &win_bg);
    m.insert("wm-shadow".to_string(), "rgba(0, 0, 0, 0.35)".to_string());
    m.insert("wm-border".to_string(), "rgba(0, 0, 0, 0.18)".to_string());
    add(&mut m, "wm-button-hover-color-a", &gtk_shade(&win_bg, 1.3));
    add(&mut m, "wm-button-hover-color-b", &win_bg);
    add(
        &mut m,
        "wm-button-active-color-a",
        &gtk_shade(&win_bg, 0.85),
    );
    add(
        &mut m,
        "wm-button-active-color-b",
        &gtk_shade(&win_bg, 0.89),
    );
    add(&mut m, "wm-button-active-color-c", &gtk_shade(&win_bg, 0.9));
    add(&mut m, "content-view-bg", &view_bg);
    add(&mut m, "text-view-bg", &gtk_shade(&view_bg, 0.94));
    // upstream derives success/error "color" via mix(bg, white, 0.4) in its
    // define block; resolved here so substitution covers every reference
    add(
        &mut m,
        "success-color",
        &mix_hex(&scheme.base0b, "ffffff", 0.4),
    );
    add(
        &mut m,
        "error-color",
        &mix_hex(&scheme.base08, "ffffff", 0.4),
    );

    m
}

fn luminance(hex: &str) -> f32 {
    let (r, g, b) = hex_to_rgb_tuple(hex);
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) / 255.0
}

/// Apply the colour map to a CSS template via direct `@var-name` substitution.
///
/// Longer keys are applied first to avoid partial matches (e.g. `accent-bg-color`
/// before `accent-color`).
pub fn apply_color_map(template: &str, map: &HashMap<String, String>) -> String {
    let mut keys: Vec<&String> = map.keys().collect();
    // Longest key first prevents `@accent-color` matching inside `@accent-bg-color`
    keys.sort_by_key(|k| std::cmp::Reverse(k.len()));

    let mut result = template.to_string();
    for key in keys {
        let var_ref = format!("@{key}");
        let value = &map[key.as_str()];
        // accent-transparent / rgba tokens are already colour expressions, not hex
        let replacement = if value.starts_with("rgba(") {
            value.clone()
        } else {
            format!("#{value}")
        };
        result = result.replace(&var_ref, &replacement);
    }
    result
}

/// Generate a flat `@define-color` block from a colour map.
///
/// A timestamp comment is prepended so the file bytes always change between applies,
/// preventing GTK from skipping a CSS reload when the same scheme is re-applied.
pub fn generate_define_color_block(map: &HashMap<String, String>) -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    let mut out = format!(
        "/* gnomad:{ts} — @define-color entries.\n\
          * GTK4 live reload via @import file-watch — technique inspired by ChromaLeon\n\
          * https://github.com/DerDakon/ChromaLeon (GPL-3.0) */\n"
    );
    for key in keys {
        let value = &map[key.as_str()];
        if value.starts_with("rgba(") {
            out.push_str(&format!("@define-color {key} {value};\n"));
        } else {
            out.push_str(&format!("@define-color {key} #{value};\n"));
        }
    }
    out
}

/// GTK4/LibAdwaita colour map: underscore-named tokens exactly as libadwaita
/// expects them in `@define-color` entries (`window_bg_color` etc.).
///
/// This is deliberately separate from the hyphenated template map: the GTK4
/// named-colour API predates/ignores Rewaita's hyphen convention, so the
/// semantic subset libadwaita reads keeps underscores.
pub fn build_gtk4_define_map(scheme: &Scheme) -> HashMap<String, String> {
    let mut m: HashMap<String, String> = HashMap::new();

    let add = |m: &mut HashMap<String, String>, name: &str, hex: &str| {
        m.insert(name.to_string(), hex.to_string());
    };

    add(&mut m, "window_bg_color", &scheme.base00);
    add(&mut m, "window_fg_color", &scheme.base05);
    add(&mut m, "view_bg_color", &scheme.base00);
    add(&mut m, "view_fg_color", &scheme.base05);
    add(&mut m, "headerbar_bg_color", &scheme.base01);
    add(&mut m, "headerbar_fg_color", &scheme.base05);
    add(&mut m, "headerbar_backdrop_color", &scheme.base00);
    add(&mut m, "headerbar_shade_color", &scheme.base00);
    add(&mut m, "headerbar_border_color", &scheme.base03);
    add(&mut m, "headerbar_darker_shade_color", &scheme.base01);
    add(&mut m, "card_bg_color", &scheme.base01);
    add(&mut m, "card_fg_color", &scheme.base05);
    add(&mut m, "card_shade_color", &scheme.base00);
    add(&mut m, "sidebar_bg_color", &scheme.base01);
    add(&mut m, "sidebar_fg_color", &scheme.base05);
    add(&mut m, "sidebar_backdrop_color", &scheme.base00);
    add(&mut m, "sidebar_shade_color", &scheme.base00);
    add(&mut m, "sidebar_border_color", &scheme.base03);
    add(&mut m, "secondary_sidebar_bg_color", &scheme.base01);
    add(&mut m, "secondary_sidebar_fg_color", &scheme.base05);
    add(&mut m, "secondary_sidebar_backdrop_color", &scheme.base00);
    add(&mut m, "secondary_sidebar_border_color", &scheme.base03);
    add(&mut m, "dialog_bg_color", &scheme.base01);
    add(&mut m, "dialog_fg_color", &scheme.base05);
    add(&mut m, "popover_bg_color", &scheme.base01);
    add(&mut m, "popover_fg_color", &scheme.base05);
    add(&mut m, "popover_shade_color", &scheme.base00);
    add(&mut m, "thumbnail_bg_color", &scheme.base01);
    add(&mut m, "thumbnail_fg_color", &scheme.base05);
    add(&mut m, "panel_bg_color", &scheme.base01);
    add(&mut m, "panel_fg_color", &scheme.base05);
    add(&mut m, "accent_color", &scheme.base0d);
    add(&mut m, "accent_bg_color", &scheme.base0d);
    let accent_fg = if luminance(&scheme.base0d) > 0.4 {
        &scheme.base00
    } else {
        &scheme.base07
    };
    add(&mut m, "accent_fg_color", accent_fg);
    add(&mut m, "success_color", &scheme.base0b);
    add(&mut m, "success_bg_color", &scheme.base0b);
    add(&mut m, "success_fg_color", &scheme.base07);
    add(&mut m, "warning_color", &scheme.base0a);
    add(&mut m, "warning_bg_color", &scheme.base0a);
    add(&mut m, "warning_fg_color", &scheme.base00);
    add(&mut m, "destructive_color", &scheme.base08);
    add(&mut m, "destructive_bg_color", &scheme.base08);
    add(&mut m, "destructive_fg_color", &scheme.base07);
    add(&mut m, "error_color", &scheme.base08);
    add(&mut m, "error_bg_color", &scheme.base08);
    add(&mut m, "error_fg_color", &scheme.base07);
    add(&mut m, "shade_color", &scheme.base00);
    add(&mut m, "scrollbar_outline_color", &scheme.base01);

    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_bg_substitutes_before_accent() {
        let mut map = HashMap::new();
        map.insert("accent-color".to_string(), "ff0000".to_string());
        map.insert("accent-bg-color".to_string(), "00ff00".to_string());
        let out = apply_color_map("@accent-bg-color @accent-color", &map);
        assert_eq!(out, "#00ff00 #ff0000");
    }

    #[test]
    fn rgba_values_substituted_verbatim() {
        let mut map = HashMap::new();
        map.insert(
            "accent-transparent".to_string(),
            "rgba(1, 2, 3, 0.5)".to_string(),
        );
        let out = apply_color_map("x: @accent-transparent;", &map);
        assert_eq!(out, "x: rgba(1, 2, 3, 0.5);");
    }
}
