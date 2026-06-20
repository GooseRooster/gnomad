use crate::pipeline::shade::{hex_to_rgb_tuple, shades};
use crate::schemes::types::Scheme;
use std::collections::HashMap;

/// Build the full variable→hex substitution map used for CSS templating.
///
/// Keys are bare variable names (no `@`). Values are hex colour strings (no `#`).
/// Includes all Rewaita semantic variables plus all 45 palette-family shades.
pub fn build_color_map(scheme: &Scheme) -> HashMap<String, String> {
    let mut m: HashMap<String, String> = HashMap::new();

    // ── Semantic / base colours ───────────────────────────────────────────────
    let add = |m: &mut HashMap<String, String>, name: &str, hex: &str| {
        m.insert(name.to_string(), hex.to_string());
    };

    add(&mut m, "window_bg_color", &scheme.base00);
    add(&mut m, "window_fg_color", &scheme.base05);
    add(&mut m, "view_bg_color", &scheme.base00);
    add(&mut m, "view_fg_color", &scheme.base05);
    add(&mut m, "headerbar_bg_color", &scheme.base01);
    add(&mut m, "headerbar_fg_color", &scheme.base05);
    add(&mut m, "card_bg_color", &scheme.base01);
    add(&mut m, "card_fg_color", &scheme.base05);
    add(&mut m, "sidebar_bg_color", &scheme.base01);
    add(&mut m, "sidebar_fg_color", &scheme.base05);
    add(&mut m, "sidebar_backdrop_color", &scheme.base00);
    add(&mut m, "sidebar_border_color", &scheme.base03);
    add(&mut m, "panel_bg_color", &scheme.base01);
    add(&mut m, "panel_fg_color", &scheme.base05);
    add(&mut m, "overview_bg_color", &scheme.base00);
    add(&mut m, "search_fg_color", &scheme.base05);
    add(&mut m, "color_fg_color", &scheme.base05);
    add(&mut m, "border_color", &scheme.base03);

    // LibAdwaita window decoration tokens
    add(&mut m, "headerbar_backdrop_color", &scheme.base00);
    add(&mut m, "headerbar_shade_color", &scheme.base00);
    add(&mut m, "headerbar_border_color", &scheme.base03);
    add(&mut m, "card_shade_color", &scheme.base00);
    add(&mut m, "dialog_bg_color", &scheme.base01);
    add(&mut m, "dialog_fg_color", &scheme.base05);
    add(&mut m, "popover_bg_color", &scheme.base01);
    add(&mut m, "popover_fg_color", &scheme.base05);

    add(&mut m, "accent_color", &scheme.base0d);
    add(&mut m, "accent_bg_color", &scheme.base0d);
    // Choose fg that contrasts against the accent background
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

    // accent_transparent used by gnome-shell template
    let (r, g, b) = hex_to_rgb_tuple(&scheme.base0d);
    m.insert(
        "accent_transparent".to_string(),
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
            let key = format!("{family}_{}", i + 1);
            m.insert(key, shade_hex.clone());
        }
    }

    m
}

fn luminance(hex: &str) -> f32 {
    let (r, g, b) = hex_to_rgb_tuple(hex);
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) / 255.0
}

/// Apply the colour map to a CSS template via direct `@var_name` substitution.
///
/// Longer keys are applied first to avoid partial matches (e.g. `accent_bg_color`
/// before `accent_color`).
pub fn apply_color_map(template: &str, map: &HashMap<String, String>) -> String {
    let mut keys: Vec<&String> = map.keys().collect();
    // Longest key first prevents `@accent_color` matching inside `@accent_bg_color`
    keys.sort_by(|a, b| b.len().cmp(&a.len()));

    let mut result = template.to_string();
    for key in keys {
        let var_ref = format!("@{key}");
        let value = &map[key.as_str()];
        // accent_transparent is already an rgba() expression, not a hex colour
        let replacement = if value.starts_with("rgba(") {
            value.clone()
        } else {
            format!("#{value}")
        };
        result = result.replace(&var_ref, &replacement);
    }
    result
}

/// Build a light-mode variant of the colour map by swapping the dark/light ends
/// of the base16 palette. This is only used for the GTK4 `@media` light block —
/// it gives GTK4 apps genuinely different colour values when `color-scheme` is
/// toggled to `prefer-light`, which bypasses the CSS-value-change-detection cache
/// and forces a real re-render when toggled back to `prefer-dark`.
pub fn build_light_color_map(scheme: &Scheme) -> HashMap<String, String> {
    let mut m: HashMap<String, String> = HashMap::new();
    let add = |m: &mut HashMap<String, String>, name: &str, hex: &str| {
        m.insert(name.to_string(), hex.to_string());
    };

    // Light mode: swap light/dark ends of the palette (base07↔base00, base06↔base01, etc.)
    add(&mut m, "window_bg_color", &scheme.base07);
    add(&mut m, "window_fg_color", &scheme.base01);
    add(&mut m, "view_bg_color", &scheme.base07);
    add(&mut m, "view_fg_color", &scheme.base01);
    add(&mut m, "headerbar_bg_color", &scheme.base06);
    add(&mut m, "headerbar_fg_color", &scheme.base01);
    add(&mut m, "headerbar_backdrop_color", &scheme.base07);
    add(&mut m, "headerbar_shade_color", &scheme.base06);
    add(&mut m, "headerbar_border_color", &scheme.base05);
    add(&mut m, "card_bg_color", &scheme.base07);
    add(&mut m, "card_fg_color", &scheme.base01);
    add(&mut m, "card_shade_color", &scheme.base06);
    add(&mut m, "sidebar_bg_color", &scheme.base06);
    add(&mut m, "sidebar_fg_color", &scheme.base01);
    add(&mut m, "sidebar_backdrop_color", &scheme.base07);
    add(&mut m, "sidebar_border_color", &scheme.base05);
    add(&mut m, "panel_bg_color", &scheme.base06);
    add(&mut m, "panel_fg_color", &scheme.base01);
    add(&mut m, "popover_bg_color", &scheme.base07);
    add(&mut m, "popover_fg_color", &scheme.base01);
    add(&mut m, "dialog_bg_color", &scheme.base07);
    add(&mut m, "dialog_fg_color", &scheme.base01);
    add(&mut m, "overview_bg_color", &scheme.base07);
    add(&mut m, "search_fg_color", &scheme.base01);
    add(&mut m, "color_fg_color", &scheme.base01);
    add(&mut m, "border_color", &scheme.base05);

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
    add(&mut m, "success_fg_color", &scheme.base00);
    add(&mut m, "warning_color", &scheme.base0a);
    add(&mut m, "warning_bg_color", &scheme.base0a);
    add(&mut m, "warning_fg_color", &scheme.base00);
    add(&mut m, "destructive_color", &scheme.base08);
    add(&mut m, "destructive_bg_color", &scheme.base08);
    add(&mut m, "destructive_fg_color", &scheme.base07);
    add(&mut m, "error_color", &scheme.base08);
    add(&mut m, "error_bg_color", &scheme.base08);
    add(&mut m, "error_fg_color", &scheme.base07);

    let (r, g, b) = hex_to_rgb_tuple(&scheme.base0d);
    m.insert(
        "accent_transparent".to_string(),
        format!("rgba({r}, {g}, {b}, 0.5)"),
    );

    // Include shades with same computation (accents don't flip between modes)
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
            m.insert(format!("{family}_{}", i + 1), shade_hex.clone());
        }
    }

    m
}

/// Generate a GTK4 user CSS block with `@define-color` entries wrapped in
/// `@media (prefers-color-scheme: dark|light)` blocks.
pub fn generate_define_color_block(
    dark_map: &HashMap<String, String>,
    light_map: &HashMap<String, String>,
) -> String {
    let render_block = |map: &HashMap<String, String>| -> String {
        let mut keys: Vec<&String> = map.keys().collect();
        keys.sort();
        let mut out = String::new();
        for key in keys {
            let value = &map[key.as_str()];
            if value.starts_with("rgba(") {
                out.push_str(&format!("  @define-color {key} {value};\n"));
            } else {
                out.push_str(&format!("  @define-color {key} #{value};\n"));
            }
        }
        out
    };

    format!(
        "@media (prefers-color-scheme: dark) {{\n{}}}\n\n@media (prefers-color-scheme: light) {{\n{}}}\n",
        render_block(dark_map),
        render_block(light_map),
    )
}
