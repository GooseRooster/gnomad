use crate::pipeline::palette::{apply_color_map, build_color_map, generate_define_color_block};
use crate::schemes::types::Scheme;
use anyhow::{Context, Result};
use std::path::Path;

static GTK3_BODY: &str = include_str!("../../assets/templates/gtk3-body.css");

const GNOMAD_START: &str = "/* gnomad-start */";
const GNOMAD_END: &str = "/* gnomad-end */";

/// Write GTK3 user CSS and GTK4 @define-color block for the given scheme.
pub fn write_gtk_css(scheme: &Scheme) -> Result<()> {
    let dark_map = build_color_map(scheme);

    // GTK3: full template with all @variable_name substituted
    let gtk3_css = apply_color_map(GTK3_BODY, &dark_map);
    write_css(&gtk3_user_css_path(), &gtk3_css).context("writing gtk-3.0/gtk.css")?;

    // GTK4: write @define-color entries to a separate gnomad-colors.css file.
    // GTK4's GtkCssProvider monitors @imported files and reloads them into the live
    // CSS cascade when they change on disk — updating running LibAdwaita apps automatically.
    let colors_css = generate_define_color_block(&dark_map);
    write_css(&gtk4_gnomad_colors_path(), &colors_css)
        .context("writing gtk-4.0/gnomad-colors.css")?;

    // Ensure gtk.css imports gnomad-colors.css (inject once; safe to leave across scheme changes).
    ensure_gtk4_import().context("updating gtk-4.0/gtk.css")?;

    Ok(())
}

/// Inject `@import url("gnomad-colors.css")` into gtk.css if not already present.
/// Existing non-gnomad content in gtk.css is preserved.
fn ensure_gtk4_import() -> Result<()> {
    let gtk4_path = gtk4_user_css_path();
    let import_block = format!(
        "{}\n\
         /* @import architecture for GTK4 live CSS reload inspired by ChromaLeon\n\
          * https://github.com/DerDakon/ChromaLeon — GPL-3.0 */\n\
         @import url(\"gnomad-colors.css\");\n\
         {}",
        GNOMAD_START, GNOMAD_END
    );

    let existing = if gtk4_path.exists() {
        std::fs::read_to_string(&gtk4_path)
            .with_context(|| format!("reading {}", gtk4_path.display()))?
    } else {
        String::new()
    };

    if existing.contains("gnomad-colors.css") {
        return Ok(());
    }

    // Strip any old gnomad block written directly (from previous gnomad versions that
    // wrote @define-color or @media blocks directly into gtk.css).
    let cleaned = strip_gnomad_block(&existing);
    let new_content = if cleaned.trim().is_empty() {
        format!("{}\n", import_block)
    } else {
        format!("{}\n\n{}\n", cleaned.trim_end(), import_block)
    };

    write_css(&gtk4_path, &new_content)
}

/// Remove any block previously written directly by gnomad (bounded by GNOMAD_START/END markers
/// or the old @media block pattern). Used during migration to the @import approach.
fn strip_gnomad_block(css: &str) -> String {
    // Remove marker-bounded gnomad blocks
    let mut out = css.to_string();
    while let (Some(start), Some(end)) = (out.find(GNOMAD_START), out.find(GNOMAD_END)) {
        if start < end {
            let block_end = end + GNOMAD_END.len();
            // Trim a trailing newline if present
            let block_end = if out.as_bytes().get(block_end) == Some(&b'\n') {
                block_end + 1
            } else {
                block_end
            };
            out.replace_range(start..block_end, "");
        } else {
            break;
        }
    }
    out
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

fn gtk4_gnomad_colors_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
        .join("gtk-4.0")
        .join("gnomad-colors.css")
}
