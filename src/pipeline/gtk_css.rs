use crate::pipeline::palette::{
    apply_color_map, build_color_map, build_light_color_map, generate_define_color_block,
};
use crate::schemes::types::Scheme;
use anyhow::{Context, Result};
use std::path::Path;

static GTK3_BODY: &str = include_str!("../../assets/templates/gtk3-body.css");

/// Write GTK3 user CSS and GTK4 @define-color block for the given scheme.
pub fn write_gtk_css(scheme: &Scheme) -> Result<()> {
    let dark_map = build_color_map(scheme);
    let light_map = build_light_color_map(scheme);

    // GTK3: full template with all @variable_name substituted (dark map only — GTK3 has no media queries)
    let gtk3_css = apply_color_map(GTK3_BODY, &dark_map);
    let gtk3_path = gtk3_user_css_path();
    write_css(&gtk3_path, &gtk3_css).context("writing gtk-3.0/gtk.css")?;

    // GTK4: @define-color blocks wrapped in @media (prefers-color-scheme: dark|light).
    // The light block gives GTK4/LibAdwaita apps genuinely different values when
    // color-scheme is briefly toggled to prefer-light, bypassing their CSS-value cache
    // and forcing a real re-render when toggled back to prefer-dark.
    let gtk4_css = generate_define_color_block(&dark_map, &light_map);
    let gtk4_path = gtk4_user_css_path();
    write_css(&gtk4_path, &gtk4_css).context("writing gtk-4.0/gtk.css")?;

    Ok(())
}

fn write_css(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)
        .with_context(|| format!("writing {}", path.display()))
}

fn gtk3_user_css_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
        .join("gtk-3.0")
        .join("gtk.css")
}

fn gtk4_user_css_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
        .join("gtk-4.0")
        .join("gtk.css")
}
