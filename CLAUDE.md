# gnomad

Rust/Ratatui TUI for GNOME colour scheme and wallpaper management using base16/base24 tinted-theming standards.

## Build & Run

```bash
cargo build              # debug build
cargo build --release    # release build
cargo run                # run TUI
cargo run -- -v          # verbose (logs to ~/.local/share/gnomad/gnomad.log)
cargo run -- --apply <slug>       # headless: apply scheme by slug
cargo run -- --update-schemes     # headless: pull latest schemes repo
```

## Runtime Dependencies

Must be in PATH: `git`, `gowall`, `tinty`

```bash
# gowall CLI (verified from source):
gowall convert <INPUT> -t <PALETTE_JSON> --output <OUTPUT_PATH>

# First-run: clones tinted-theming/schemes to ~/.local/share/gnomad/schemes-repo
# Schemes live under: base16/ and base24/ subdirs
```

## Project Structure

```
src/
  main.rs              — CLI parsing, tracing init, terminal lifecycle, headless modes
  app.rs               — App struct, tokio event loop, async task spawning
  config.rs            — Config struct (TOML at ~/.config/gnomad/config.toml)
  state.rs             — UI state (active tab, selected scheme/wallpaper, etc.)
  pipeline/
    mod.rs             — apply_scheme() and apply_wallpaper() orchestrators
    gowall.rs          — gowall subprocess + palette JSON writer
    gtk_css.rs         — writes ~/.config/gtk-3.0/gtk.css and gtk-4.0/gtk.css
    shell_css.rs       — writes ~/.local/share/themes/<name>/gnome-shell/gnome-shell.css
    palette.rs         — build_color_map(), apply_color_map(), generate_define_color_block()
    shade.rs           — shade interpolation for palette families
    gnome.rs           — GnomeInterface: gsettings get/set, wallpaper, shell reload
    tinty.rs           — tinty apply subprocess
    wallpaper_cache.rs — per-scheme wallpaper cache (JoinSet batch converts)
  schemes/
    types.rs           — Scheme struct, Base16/Base24 YAML parsing
    fetch.rs           — git clone/pull for tinted-theming/schemes repo
  ui/
    scheme_browser.rs  — scheme list + color swatch panel
    wallpaper_picker.rs — wallpaper list + image preview panel
    animation.rs       — spinner, pipeline progress display, palette strip
assets/
  templates/
    gtk3-body.css      — Rewaita GTK3 template (7800+ lines, @variable_name substituted at runtime)
    gnome-shell.css    — GNOME Shell CSS template
```

## CSS Write Paths

| File | Purpose |
|---|---|
| `~/.config/gtk-3.0/gtk.css` | GTK3: full template with all `@var` replaced by hex values |
| `~/.config/gtk-4.0/gtk.css` | GTK4: `@define-color` block only — libadwaita handles widget styling |
| `~/.local/share/themes/<name>/gnome-shell/gnome-shell.css` | GNOME Shell theme |

## Key Technical Decisions

- **GTK4 CSS**: Only `@define-color` entries — libadwaita reads these named colors and applies its own rules. Do NOT write widget CSS rules to gtk-4.0/gtk.css.
- **gowall CLI**: `gowall convert <in> -t <json> --output <out>` — output is a flag, not positional.
- **Accent color**: GNOME's gsettings `accent-color` only accepts named presets. Use CSS `@define-color accent_color/accent_bg_color/accent_fg_color` instead.
- **Shell CSS reload**: Toggle `color-scheme` gsettings value to force GNOME Shell to re-read the CSS, then restore original value.
- **Wallpaper cache**: Converted wallpapers are cached per-scheme at `~/.local/share/gnomad/wallpaper-cache/<slug>/`. Cache is checked before running gowall.
- **Child process I/O**: All subprocesses must use `.stdout(Stdio::null()).stderr(Stdio::null())` — the TUI owns the terminal fd and any inherited I/O corrupts the display.
- **Verbose logging**: `-v` writes to `~/.local/share/gnomad/gnomad.log` (not stderr) since stderr goes to the alternate screen during TUI operation.

## Flatpak Overrides (one-time setup)

```bash
flatpak override --user --filesystem=xdg-config/gtk-3.0
flatpak override --user --filesystem=xdg-config/gtk-4.0
flatpak override --user --filesystem=xdg-data/themes
```
