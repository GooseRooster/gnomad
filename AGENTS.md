# AGENTS.md

Operational guidance for coding agents working in this repository. For the
project description and architecture, see [PROJECT.md](PROJECT.md).

## Build & Run

```bash
cargo build              # debug build
cargo build --release    # release build
cargo test               # unit + TestBackend UI + insta snapshots + fake-CLI pipeline + PTY smoke
cargo insta review       # review/accept UI snapshot changes (optional; needs cargo-insta)
cargo run                # run TUI
cargo run -- --gowall-only  # wallpaper-only TUI (no tinty/GTK/Shell writes)
cargo run -- -v          # verbose (logs to ~/.local/share/gnomad/gnomad.log)
cargo run -- --apply <slug>       # headless: apply scheme by slug
cargo run -- --update-schemes     # headless: pull latest schemes repo
cargo run -- --populate-json-scheme  # headless: refresh /tmp/gnomad-current-scheme.json
```

## Runtime Dependencies

Must be in PATH: `git`, `gowall`, `tinty`, `gsettings`, `gnome-extensions`,
`dconf`. The Rust toolchain is required to build.

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
    gowall.rs          — gowall subprocess + palette JSON writer + managed HOME/CLUT cache
    blend.rs           — intensity post-blend between source and converted image
    oklab.rs           — OKLab/OKLCH palette tuning (gowall pipeline only)
    preview.rs         — downscaled, pruned conversion previews
    gtk_css.rs         — writes ~/.config/gtk-3.0/gtk.css and gtk-4.0/gtk.css
    shell_css.rs       — writes ~/.local/share/themes/<name>/gnome-shell/gnome-shell.css
    paperwm.rs         — PaperWM integration: workspace colour derivation + gsettings sync
    palette.rs         — build_color_map(), apply_color_map(), generate_define_color_block()
    shade.rs           — shade interpolation for palette families
    gnome.rs           — GnomeInterface: gsettings get/set, wallpaper, shell reload
    tinty.rs           — tinty apply subprocess
    wallpaper_cache.rs — per-scheme wallpaper cache (JoinSet batch converts)
  schemes/
    types.rs           — Scheme struct, Base16/Base24 YAML parsing
    fetch.rs           — git clone/pull for tinted-theming/schemes repo
  ui/
    scheme_browser.rs  — scheme list + swatches + CURRENT/PREVIEW image panes
    wallpaper_picker.rs — wallpaper list + on-demand converted preview
    tuning.rs          — gowall tuning slider overlay
    animation.rs       — spinner, pipeline progress display, palette strip
  test_support.rs      — cfg(test) UI harness, isolated env, fake external commands
  snapshots/           — committed insta snapshots of the main UI screens
tests/
  pty_smoke.rs         — real binary in a PTY: render + clean quit
assets/
  templates/
    gtk3-body.css      — Rewaita GTK3 template (compact override sheet, hyphen tokens)
    gnome-shell.css    — GNOME Shell CSS template (PaperWM section at tail, gated at write time)
```

## CSS Write Paths

| File | Purpose |
|---|---|
| `~/.config/gtk-3.0/gtk.css` | GTK3: `@define-color` block + compact template, all `@var` replaced by hex values |
| `~/.config/gtk-4.0/gtk.css` | GTK4: `@import` of gnomad-colors.css only — libadwaita handles widget styling |
| `~/.config/gtk-4.0/gnomad-colors.css` | GTK4: underscore-named `@define-color` entries (libadwaita's token convention) |
| `~/.local/share/themes/<name>/gnome-shell/gnome-shell.css` | GNOME Shell theme |

## Key Technical Decisions

- **GTK4 CSS**: Only `@define-color` entries — libadwaita reads these named colors and applies its own rules. Do NOT write widget CSS rules to gtk-4.0/gtk.css.
- **Token convention**: CSS templates use Rewaita's hyphenated tokens (`@window-bg-color`); `build_color_map()` keys match. The GTK4 `@define-color` block keeps **underscores** (`window_bg_color`) because that is libadwaita's own named-colour convention — `build_gtk4_define_map()` exists separately for this reason.
- **GTK3 define block**: generated in Rust (resolving upstream's mix()/shade()/alpha() expressions via `shade.rs` helpers) and prepended to the compact GTK3 template. Upstream's in-file header is stripped at vendoring time. Note: upstream's template references `@borders-color` without defining it — gnomad's map supplies it.
- **Template re-vendoring**: when refreshing from ~/repos/Rewaita, strip the GTK3 template's `@define-color` header and keep gnomad's GPL header; re-apply the gnomad-only shell additions (workspaceSwitcherPopup block, entry focus border) on top of upstream's gnome-shell-template.css.
- **PaperWM integration** (`paperwm_enabled`, default off): workspace colours are NOT CSS-reachable — PaperWM applies them via `Meta.Background.set_color()` and an inline selection-border style. gnomad writes the global `workspace-colors` strv plus each existing workspace's relocatable `color` key (`org.gnome.shell.extensions.paperwm.workspace:<path>/`), which PaperWM applies live with a crossfade — never toggle the extension to reload. Colours are blended ~45% toward the scheme highlight (base07) so they read as switch-animation borders; values must carry a `#` prefix (Cogl `Color.from_string` rejects bare hex). Extension schemas are not system-wide: gsettings needs `GSETTINGS_SCHEMA_DIR` pointed at `<extension path>/schemas` (resolved via `gnome-extensions info`). The window position bar is deliberately NOT CSS-overridden — PaperWM's default accent styling there has better text contrast.
- **custom_schemes_dir defaults to `~/.config/gnomad/schemes`** (always set, not optional). `load_schemes()` skips it silently when the directory doesn't exist yet.
- **Custom schemes vs tinty sync**: `tinty apply <custom-scheme>` generates untracked `*<system>-<slug>*` files inside tinty's cloned template repos, which makes `tinty sync`/`update` refuse to run (their check is `git status --porcelain`). gnomad does NOT auto-commit (needs git identity, conflicts with update checkouts) — it writes an explicit `slug:` field into the synced YAML (making output filenames deterministic) and appends a `*<system>-<slug>*` pattern to each repo's `.git/info/exclude` (repo-local, never committed, invisible to git status). Idempotent via a per-slug comment marker.
- **gowall CLI**: `gowall convert <in> -t <json> --output <out>` — output is a flag, not positional.
- **gowall-only mode** (`gowall_only`, `--gowall-only`/`--full`): `apply_scheme` short-circuits to convert + intensity blend + `set_wallpaper`. It must not call Tinty, GTK/Shell CSS, PaperWM, Steam, `set_color_scheme`, or the User Themes check. `wallpaper_features_enabled()` returns true for either `wallpaper_enabled` or `gowall_only`.
- **Gowall tuning**: gowall exposes no conversion-strength flags. `[gowall_tuning]` transforms the palette in OKLab/OKLCH before writing the JSON, and `intensity` post-blends the converted image back toward the source. Tuning is global (full mode, gowall-only, batch, slideshow). Cache fingerprints include the scheme palette hash + tuning hash so custom-scheme edits and slider changes invalidate correctly.
- **Managed gowall HOME**: gowall is run with `HOME` pointed at `~/.cache/gnomad/gowall/home` and `OutputFolder` at `~/.cache/gnomad/gowall/output`, so tuned CLUTs don't accumulate in `~/Pictures/gowall/cluts`. The user's `ColorCorrectionBackend` setting is copied into the managed config. Keep the newest 64 CLUTs.
- **Preview cache**: scheme/wallpaper previews are downscaled (1600 px), debounced, and stored under `~/.cache/gnomad/previews` (newest 32 kept). They are deliberately separate from the full-size wallpaper cache; browsing hundreds of schemes must not create multi-megabyte full-size conversions.
- **Accent color**: GNOME's gsettings `accent-color` only accepts named presets. Use CSS `@define-color accent_color/accent_bg_color/accent_fg_color` instead.
- **Shell CSS reload**: Toggle `color-scheme` gsettings value to force GNOME Shell to re-read the CSS, then restore original value.
- **Wallpaper cache**: Converted wallpapers are cached per-scheme at `~/.local/share/gnomad/wallpapers/<slug>-<dirhash>[-<tuning>]/` (despite the older `wallpaper-cache/` wording that may appear in history). Cache is checked before running gowall; manifests carry scheme-palette and tuning fingerprints.
- **Wallpaper features default off**: `wallpaper_enabled` defaults to `false` in config — users opt in via `~/.config/gnomad/config.toml`. `gowall` is still a required PATH binary regardless (checked unconditionally at startup, same as `git`). `tinty` is checked only in full mode; `gowall_only` implies wallpaper features via `Config::wallpaper_features_enabled()`.
- **Test harness**: `src/test_support.rs` (cfg(test)) provides `TestEnv` (temp `HOME`/XDG, fake `gowall`/`tinty`/`gsettings`/`gnome-extensions` scripts) and `TestApp` (`Terminal<TestBackend>` + `App`). UI tests set `GNOMAD_DISABLE_IMAGE_PREVIEW=1`; pipeline tests set `GNOMAD_GSETTINGS_BIN` so the fake gsettings is used instead of the pinned `/usr/bin/gsettings`. Env-mutating tests use `#[serial]`. `TestBackend` cannot render ratatui-image protocols; test preview state/paths or use the PTY smoke test. `tests/pty_smoke.rs` drives the real binary in a PTY for crossterm/raw-mode coverage; CI runs it as an explicit step.
- **UI snapshots**: `insta` snapshots live in `src/snapshots/`. Use `TestApp::snapshot_screen()` (trailing blank-cell padding trimmed). For screens that render filesystem paths (e.g. the wallpaper panel) set a fixed display path in the test config instead of replacing the temp root after rendering — path length changes the box/title layout and makes snapshots environment-dependent. After an intentional layout change run `cargo insta review` or `INSTA_UPDATE=always cargo test`; never commit `*.snap.new`.
- **Fake CLI controls**: the fake `gowall` reads failure markers next to `$GNOMAD_TEST_LOG` — `gowall.skip` names one batch file to omit (partial failure) and `gowall.fail` makes the command exit 1 (total failure). Hook-env tests use `TestEnv::hook_command()`/`read_hook_env()` to run a real `sh -c` hook that dumps its environment.
- **Child process I/O**: All subprocesses must use `.stdout(Stdio::null()).stderr(Stdio::null())` — the TUI owns the terminal fd and any inherited I/O corrupts the display.
- **Verbose logging**: `-v` writes to `~/.local/share/gnomad/gnomad.log` (not stderr) since stderr goes to the alternate screen during TUI operation.
- **Env-leak strip on gsettings**: `src/pipeline/gnome.rs` removes `GSETTINGS_BACKEND`, `GIO_MODULE_DIR`, and `LD_LIBRARY_PATH` from every `gsettings`/`gnome-extensions` subprocess. These vars are injected by AppImage runtimes (e.g. ghostty's sharun bundler) and redirect GIO away from dconf, which would cause writes to land in a keyfile instead of the session dconf database. Don't refactor them out without an alternative defence.

## Flatpak Overrides (one-time setup)

```bash
flatpak override --user --filesystem=xdg-config/gtk-3.0
flatpak override --user --filesystem=xdg-config/gtk-4.0
flatpak override --user --filesystem=xdg-data/themes
```
