# gnomad

A lightweight terminal UI for browsing and applying [base16] / [base24] colour
schemes — plus per-scheme wallpapers — to a live GNOME session. Browse a few
hundred upstream schemes from [tinted-theming/schemes] in a Ratatui TUI, hit
enter, and your GTK3 apps, GTK4/libadwaita apps, and the GNOME Shell itself
repaint to the new palette; with wallpaper enabled, the desktop background gets
re-converted through the same palette.

For agent-facing operational notes (build commands, runtime deps, layout, key
technical decisions), see [AGENTS.md](AGENTS.md). For end-user install / usage,
see the [README](README.md).

## Why it exists

Managing a tinted-theming scheme by hand means editing three CSS files
(`~/.config/gtk-3.0/gtk.css`, `~/.config/gtk-4.0/gtk.css`, and a user-theme CSS
under `~/.local/share/themes/`), plus a `tinty apply` invocation, plus a
`gnome-extensions` disable/enable cycle to wake up the Shell. Then if you want
the wallpaper tinted to match, that's `gowall convert` per image. None of it is
hard in isolation; it's just seven terminal commands and a sprinkling of state.

gnomad collapses all of it into one interactive picker with a single apply
action, plus a headless mode for scripting.

## Architecture

```
 TUI (ratatui + crossterm)
        │
        │  scheme selection / wallpaper selection
        ▼
 pipeline::apply_scheme ──┬──► tinty apply            (terminal palette)
                          ├──► pipeline::palette::build_color_map
                          ├──► pipeline::gtk_css       ──► ~/.config/gtk-3.0/gtk.css
                          │                            ──► ~/.config/gtk-4.0/gtk.css
                          ├──► pipeline::shell_css     ──► ~/.local/share/themes/<name>/gnome-shell/gnome-shell.css
                          ├──► GnomeInterface::set_color_scheme   (gsettings)
                          └──► GnomeInterface::reload_shell_theme (gnome-extensions)

 pipeline::apply_wallpaper ──► gowall convert (per scheme in wallpaper-cache/)
                              ──► GnomeInterface::set_wallpaper  (gsettings)
```

The pipeline is two orchestrators (`apply_scheme`, `apply_wallpaper`) over a
small set of independent modules that each touch one surface. Each module
either writes a file or shells out to one external CLI; none of them care
about the others. The TUI is intentionally thin — it drives state, not work.

Headless entrypoints (`--apply <slug>`, `--update-schemes`,
`--populate-json-scheme`) share the same pipeline; the TUI is just one client.

## Scheme sources

Schemes come from the upstream [`tinted-theming/schemes`] git repo, cloned on
first run into `~/.local/share/gnomad/schemes-repo` and refreshed via
`--update-schemes`. The repo has two layouts:

- `base16/` — 16-slot palette (`base00`..`base0F`); CSS templates expect these.
- `base24/` — 16 base16 slots + 8 additional slots (`base10`..`base17`) for
  finer-grained UI accents. Used when a scheme explicitly declares itself
  base24.

YAML parsing lives in `src/schemes/types.rs`; the git plumbing in
`src/schemes/fetch.rs`.

## Dependency rationale

- **gowall** — no published crate or apt package; the binary is a one-shot
  download from GitHub releases. Used to convert source images into the
  scheme's palette. Results are cached per-scheme under
  `~/.local/share/gnomad/wallpaper-cache/<slug>/` so re-applying the same
  scheme doesn't re-render.
- **tinty** — published on crates.io; `cargo install --locked` is the cleanest
  install path. Invoked as a subprocess for the terminal palette only; the rest
  of the GTK/Shell surfaces are done in-tree for tighter control over CSS
  output.
- **gsettings** (CLI, via `libglib2.0-bin`) and **gnome-extensions** — GNOME's
  official configuration tooling. gnomad prefers shelling out to these over
  linking GIO directly because:
  - keeps gnomad a normal `cargo install` build with no GLib dependency tree,
  - inherits GNOME's own bus/codec handling (and its own quirks around
    AppImage-injected env vars),
  - the only surface gnomad needs from GNOME is settings + user-theme
    enable/disable — both are stable CLI contracts.
- **dconf** — only used to verify the gsettings backend is talking to dconf
  (not a stray keyfile from an AppImage env leak).

## Caching

| Cache | Path | Cleared by |
|---|---|---|
| Schemes repo | `~/.local/share/gnomad/schemes-repo` | `--update-schemes` does `git pull` |
| Per-scheme wallpaper conversions | `~/.local/share/gnomad/wallpaper-cache/<slug>/` | Delete the slug dir |
| Verbose log | `~/.local/share/gnomad/gnomad.log` | Delete the file |

Wallpaper cache is checked before invoking gowall — if a rendered file already
exists for `(source, scheme)`, it's reused. This matters because re-applying the
same scheme (or paging back to a wallpaper you previewed) should be instant.

[base16]: https://github.com/tinted-theming/home
[base24]: https://github.com/tinted-theming/base24-schemes
[tinted-theming/schemes]: https://github.com/tinted-theming/schemes
