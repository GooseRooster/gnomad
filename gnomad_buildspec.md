
# gnomad — Build Spec v1

> Rust/Ratatui TUI for base16/base24 colour scheme and wallpaper management on GNOME.

---

## Overview

A terminal UI that lets you browse and apply base16/base24 colour schemes across the entire
GNOME desktop in one action. On scheme switch it: converts the current wallpaper to match the
palette via gowall, delegates terminal/app theme propagation to Tinty, writes custom GTK and
GNOME Shell CSS, and triggers a GNOME Shell reload. A second panel lets you pick wallpapers
from a configured directory, converting them to the active scheme on select.

A low-framerate retro-styled animation plays during processing to mask the GNOME Shell CSS
reload stutter (which is a compositor-level freeze affecting all Wayland clients — it cannot
be eliminated, only obscured).

---

## Scope (v1)

**In scope:**
- Scheme browser with base16 and base24 schemes from tinted-theming + user custom directory
- Scheme application pipeline (gowall → wallpaper set → Tinty → GTK CSS + Shell CSS → shell reload)
- Wallpaper picker panel (single wallpaper apply) - Application pipeline should smartly detect if color changes are needed (standard wallpaper changes in the current scheme should be instant, for example, outside of running gowall if it hasnt been converted)
- Batch wallpaper conversion — convert an entire wallpaper directory against any scheme
  in one pass, persisted per scheme slug. A standalone primitive (see Roadmap: this is
  groundwork for a future GNOME Shell extension, not a v1 slideshow feature).
- TUI animation overlay during processing
- Config file for wallpaper directory, theme name, custom scheme directory, preferences

**Out of scope (roadmap):**
- Custom scheme creation/editor
- Non-GNOME desktop support

---

## System Requirements

- GNOME 45+ on Wayland
- Fedora Workstation (primary target) or any GNOME distro
- `gowall` binary in `$PATH`
- `tinty` binary in `$PATH`
- Flatpak apps present (optional — see one-time setup)
- A terminal with Sixel/Kitty Graphics Protocol support

---

## One-Time Environment Setup

Run once by the user. Document in README.

```bash
# Allow Flatpak GTK apps to see the theme directory
flatpak override --user --filesystem=xdg-data/themes

# Install gowall (Fedora example — see gowall docs for other distros)
sudo dnf copr enable achno/gowall
sudo dnf install gowall

# Install Tinty
cargo install tinty

# Sync Tinty template repos (required before first use)
tinty sync

# Install and enable the User Themes extension (for Shell panel theming)
# Fedora ships it in gnome-shell-extensions:
sudo dnf install gnome-shell-extensions
gnome-extensions enable user-theme@gnome-shell-extensions.gcampax.github.com
```

**Shell theme selection (one-time):** After running gnomad for the first time (which
writes the theme files), open GNOME Tweaks → Appearance → Shell and select "gnomad".
This is a one-time step — the theme name is constant, so subsequent scheme switches just
update the CSS files in place and the shell reload picks them up automatically.

Alternatively, set via command line:
```bash
gsettings set org.gnome.shell.extensions.user-theme name "gnomad"
```

gnomad should detect on startup whether the User Themes extension is active and warn
if not, since Shell CSS will be written but won't apply without it.

**Tinty config note:** Users should configure `~/.config/tinted-theming/tinty/config.toml`
with their desired `[[items]]` (tinted-shell, tinted-kitty, etc.) before using gnomad.
gnomad calls `tinty apply` — it does not manage Tinty's config. See Tinty's USAGE.md.


---

## Dependencies (Cargo.toml)

```toml
[dependencies]
ratatui = "0.29"
crossterm = "0.28"
tokio = { version = "1", features = ["full"] }
zbus = { version = "4", features = ["tokio"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }
clap = { version = "4", features = ["derive"] }
anyhow = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
dirs = "5"
tokio-stream = "0.1"

(I am unsure of extra requirements needed for Sixel/KGP)
```

---

## Project Structure

```
src/
├── main.rs                 # Entry point, CLI args, tokio runtime
├── config.rs               # Config file (TOML): dirs, theme name, preferences
├── app.rs                  # Top-level app state, event loop
├── ui/
│   ├── mod.rs
│   ├── scheme_browser.rs   # Scheme list, search, preview swatches
│   ├── wallpaper_picker.rs # Directory browser, wallpaper list
│   └── animation.rs        # Processing animation overlay
├── schemes/
│   ├── mod.rs
│   ├── fetch.rs            # GitHub API fetch + local cache of YAML files
│   └── types.rs            # Base16/base24 scheme struct, YAML deserialisation
├── pipeline/
│   ├── mod.rs
│   ├── gowall.rs           # Write temp JSON palette, spawn gowall subprocess
│   ├── tinty.rs            # Shell out to tinty apply <scheme-slug>
│   ├── gtk_css.rs          # Write GTK3/4 CSS from palette to ~/.local/share/themes
│   ├── shell_css.rs        # Write gnome-shell/gnome-shell.css from palette
│   ├── wallpaper_cache.rs  # Batch gowall conversion + manifest, per scheme slug
│   └── gnome.rs            # zbus calls: set wallpaper, gtk-theme, CSS reload trigger
└── state.rs                # Current scheme, current wallpaper path, app mode
```

---

## Config File

Location: `~/.config/gnomad/config.toml`

```toml
wallpaper_dir = "/home/user/Pictures/Wallpapers"
custom_schemes_dir = "/home/user/.config/gnomad/schemes"   # optional
theme_name = "gnomad"           # Name used for the GTK theme dir
default_scheme = "base16-gruvbox-dark-hard"
schemes_cache_dir = "~/.local/share/gnomad/schemes"
output_wallpaper_path = "~/.local/share/gnomad/current-wallpaper.png"
wallpaper_cache_dir = "~/.local/share/gnomad/wallpapers"  # batch-convert output, per-scheme
follow_user_scheme_type = true # Do we filter available schemes by user's gnome Dark/Light preference?
```

---

## Data: Scheme Fetching

### Remote source

**Repository:** `tinted-theming/schemes`

Fetch both subdirectories:
```
GET https://api.github.com/repos/tinted-theming/schemes/contents/base16
GET https://api.github.com/repos/tinted-theming/schemes/contents/base24
```

Each returns a JSON array of files with `name`, `path`, and `download_url`.
Fetch each `download_url` to get the raw YAML. Cache all files under:
- `~/.local/share/gnomad/schemes/base16/`
- `~/.local/share/gnomad/schemes/base24/`

Add an `[u] update` keybind in the TUI to re-fetch. Fetch automatically on first run.

### Custom scheme directory

If `custom_schemes_dir` is set in config, load all `.yaml`/`.yml` files from that directory
at startup alongside the cached remote schemes. Custom schemes use the same YAML format.
Display them with a `[custom]` tag in the scheme browser list.

Users can place hand-crafted or third-party YAML scheme files here without modifying the
cached remote data.

### Base16 YAML — handle both formats

New format (spec 0.11) — detect by presence of `palette:` key:
```yaml
system: "base16"
name: "Gruvbox dark, hard"
author: "Dawid Kurek"
variant: "dark"
palette:
  base00: "1d2021"
  base01: "3c3836"
  # base02 ... base0F
```

Legacy format:
```yaml
scheme: "Gruvbox dark, hard"
author: "Dawid Kurek"
base00: "1d2021"
base01: "3c3836"
# base02 ... base0F
```

### Base24 YAML

Same as base16 new format, with `system: "base24"` and 8 additional palette entries:
`base10` through `base17`. These extend ANSI terminal colour support. Treat all 8 as
optional in the struct — base16 schemes simply won't have them.

All hex values are WITHOUT `#` prefix. Add `#` on use.

### Scheme struct

```rust
pub struct Scheme {
    pub system: SchemeSystem,   // Base16 | Base24
    pub name: String,
    pub slug: String,           // derived from filename, e.g. "base16-gruvbox-dark-hard"
    pub author: String,
    pub variant: Option<String>,
    pub is_custom: bool,
    // base16 slots (always present)
    pub base00: String, pub base01: String, pub base02: String, pub base03: String,
    pub base04: String, pub base05: String, pub base06: String, pub base07: String,
    pub base08: String, pub base09: String, pub base0a: String, pub base0b: String,
    pub base0c: String, pub base0d: String, pub base0e: String, pub base0f: String,
    // base24 extension slots (optional)
    pub base10: Option<String>, pub base11: Option<String>,
    pub base12: Option<String>, pub base13: Option<String>,
    pub base14: Option<String>, pub base15: Option<String>,
    pub base16: Option<String>, pub base17: Option<String>,
}
```

### Base16 slot semantics

```
base00 — darkest background
base01 — slightly lighter background (status bars, line numbers)
base02 — selection background
base03 — comments, invisibles
base04 — dark foreground (inactive UI)
base05 — default foreground
base06 — light foreground
base07 — lightest foreground
base08 — red / variables
base09 — orange / integers
base0A — yellow / classes
base0B — green / strings
base0C — cyan / support
base0D — blue / functions
base0E — magenta / keywords
base0F — brown / deprecated
```

---

## Pipeline: Scheme Switch

Triggered when user selects and confirms a scheme. Steps run sequentially via tokio.
Animation overlay starts before step 1 and stops after step 5.

### Step 1 — Write gowall JSON palette

Write to `/tmp/gnomad-current-scheme.json`:
```json
{
  "name": "SchemeName",
  "colors": ["#1d2021", "#3c3836", ... ]
}
```

For base16: pass all 16 colors (base00–base0F) with `#` prefix.
For base24: pass all 24 colors (base00–base17) — gowall accepts any count.

### Step 2 — Run gowall

```bash
gowall convert <current_wallpaper_path> <output_wallpaper_path> \
  -t /tmp/gnomad-current-scheme.json
```

Spawn via `tokio::process::Command`. Await completion before continuing.

### Step 3 — Run Tinty

```bash
tinty apply <scheme-slug>
```

e.g. `tinty apply base16-gruvbox-dark-hard`

This handles all terminal emulator colours, shell colours, and any other apps the user
has configured in their Tinty `config.toml`. gnomad does not manage Tinty's item
configuration — that is the user's responsibility.

Tinty natively supports both base16 and base24 scheme slugs.

### Step 4 — Write GTK CSS + Shell CSS

Both CSS templates are forked snapshots of Rewaita's GTK and Shell themes, maintained in
the gnomad repo. Colour variables are substituted from the base16 palette at write time.

**Theme directory layout on disk:**
```
~/.local/share/themes/gnomad/
├── gtk-3.0/
│   └── gtk.css
├── gtk-4.0/
│   └── gtk.css
└── gnome-shell/
    └── gnome-shell.css
```

**GTK CSS** — write to `gtk-3.0/gtk.css` and `gtk-4.0/gtk.css`:
```css
@define-color accent_color #<base0D>;
@define-color accent_bg_color #<base0D>;
@define-color accent_fg_color #<base07>;
@define-color destructive_color #<base08>;
@define-color success_color #<base0B>;
@define-color warning_color #<base0A>;
@define-color error_color #<base08>;
@define-color window_bg_color #<base00>;
@define-color window_fg_color #<base05>;
@define-color view_bg_color #<base01>;
@define-color view_fg_color #<base05>;
@define-color headerbar_bg_color #<base01>;
@define-color headerbar_fg_color #<base05>;
@define-color card_bg_color #<base01>;
@define-color sidebar_bg_color #<base01>;
@define-color popover_bg_color #<base02>;
```

**Shell CSS** — write to `gnome-shell/gnome-shell.css` (forked from Rewaita's shell theme).
Minimum colour mappings for the panel and overview:
```css
/* Panel */
#panel {
  background-color: #<base01>;
  color: #<base05>;
}
#panel .panel-button { color: #<base05>; }
#panel .panel-button:hover { background-color: #<base02>; color: #<base05>; }
#panel .panel-button:focus { background-color: #<base02>; }
#panel .panel-button:active { background-color: #<base0D>; color: #<base07>; }

/* Clock */
.clock-display .clock { color: #<base05>; }

/* Overview */
.search-entry { background-color: #<base01>; color: #<base05>; border-color: #<base03>; }
.search-entry:focus { border-color: #<base0D>; }

/* OSDs and notifications */
.osd { background-color: #<base01>; color: #<base05>; }
.notification { background-color: #<base01>; color: #<base05>; }
```

The full template will include all Rewaita shell selectors — the above is the minimum
colour surface. Fork Rewaita's `gnome-shell.css` at build time and template all hardcoded
colour values against the base16 slot mapping.



NOTE for planning and implementation: Rewaita has been cloned to ~/src/Rewaita. Review the code, verify the CSS schemes to actually fork off.

NOTE on flatpak overrides: An example is available in the ChromaLeon extension, which is cloned in ~/src/ChromaLeon. Verify based off this codebase that our approach for flatpak overrides is correct.

The base24 extension slots are not used in either CSS file — this step is identical for
both scheme systems.

**User Themes dependency:** Shell CSS only takes effect if the User Themes extension is
active and the "gnomad" shell theme is selected in Tweaks (one-time setup). Write the
file unconditionally — warn the user on startup if the extension is not detected.

### Step 5 — GNOME integration via zbus

All calls via `org.gnome.desktop.interface` on the session dbus.

```
a) Set wallpaper:
   picture-uri      → "file://<output_wallpaper_path>"
   picture-uri-dark → "file://<output_wallpaper_path>"

b) Set GTK theme:
   gtk-theme → "<theme_name>"

c) Trigger GNOME Shell CSS reload (light/dark toggle):
   color-scheme → "prefer-light"
   color-scheme → "prefer-dark"
  NOTE: In code, we should of course instead detect what the user had selected already and flip based on that (light - dark - light, or dark - light - dark)
```

Step (c) causes the compositor-level freeze. The animation must be running before this fires.

### Step 6 — Stop animation, update TUI state

Display active scheme name and system tag in status bar. Return to interactive mode.

---

## Pipeline: Wallpaper Pick

Scheme does not change — only the wallpaper is updated.

1. Start animation overlay
2. Run gowall with the current scheme's JSON palette against the selected image, unless this wallpaper has been converted for the scheme and cached already
3. Set GNOME wallpaper via zbus (picture-uri / picture-uri-dark)
4. Stop animation
5. Update displayed wallpaper path in state

No CSS reload, no Tinty call. Only the wallpaper changes.

---

## Pipeline: Batch Convert Wallpaper Directory

A standalone, general-purpose action: convert every image in `wallpaper_dir` against a
given scheme in one pass, and persist the result. There is no v1 feature that consumes
this automatically — it exists as groundwork for a future GNOME Shell extension (see
Roadmap) that will handle wallpaper rotation with custom transitions, bypassing GNOME's
native dynamic-wallpaper XML format entirely.

The reason to build this now rather than later: the persisted output is just "a directory
of scheme-tinted wallpapers." The extension's only requirement will be reading images from
a known directory, with zero awareness of gnomad, gowall, or scheme YAML. Building the
cache as its own primitive today means that contract is ready and stable whenever the
extension is built — and it's independently useful right away (e.g. apply any scheme to
your wallpaper directory ahead of time without fully switching to it).

**Trigger points in the TUI:**
- `[c]` in the wallpaper picker — convert for the currently active scheme
- `[c]` in the scheme browser, on a highlighted (not yet applied) scheme — pre-warms the
  cache for a scheme without fully applying it (no Tinty/CSS/shell reload triggered)

### Persistence model

Cached **per scheme slug**, not in `/tmp` (which may not survive reboot and offers no
reason to persist anyway):

```
~/.local/share/gnomad/wallpapers/
└── <scheme-slug>/
    ├── mountain.png
    ├── coastal-dusk.png
    ├── forest-fog.png
    └── manifest.json
```

`manifest.json` lists source filenames + mtimes from `wallpaper_dir` at conversion time —
used for cache invalidation, and incidentally useful as ordering/metadata for any future
external consumer of the directory.

If `<scheme-slug>/` already exists and its manifest matches `wallpaper_dir`'s current
state, skip conversion entirely — cache hit. Re-running batch-convert for a previously
converted scheme is near-instant. Provide a manual `[shift+c]` force-regenerate to
bypass the cache check.

### Conversion step

For each image in `wallpaper_dir` not already present (per manifest) in the target
scheme's cache directory, run gowall using that scheme's JSON palette:

```bash
gowall convert <source_image> <cache_dir>/<filename> -t <scheme_palette>.json
```

Conversions run concurrently via `tokio::task::JoinSet` — independent, no shared state.
Animation status: `[ converting N of M wallpapers... ]`.

---

## TUI Layout

```
┌─────────────────────────────────────────────────────┐
│ gnomad                         [Tab] Switch panel│
├────────────────────┬────────────────────────────────┤
│                    │                                 │
│  SCHEMES           │  PREVIEW                        │
│  ──────────        │                                 │
│  [b16] gruvbox-dark│  ██ ██ ██ ██ ██ ██ ██ ██       │
│  [b16] solarized   │  ██ ██ ██ ██ ██ ██ ██ ██       │
│  [b24] gruvbox-b24 │  (base16: 2 rows of 8 swatches)│
│  [b16] ocean       │  (base24: 3 rows of 8 swatches)│
│  [  *] my-scheme   │                                 │
│  ...               │  Name:    Gruvbox dark, hard    │
│                    │  System:  base16                │
│  [/] search        │  Author:  Dawid Kurek           │
│  [u] update        │  Variant: dark                  │
│                    │                                 │
│                    │  [Enter] Apply scheme           │
│                    │  [c]     Pre-convert wallpapers │
│                    │          for this scheme        │
└────────────────────┴────────────────────────────────┘
│ Active: base16-gruvbox-dark-hard   Wall: mountain.jpg│
└─────────────────────────────────────────────────────┘
```

Scheme list tags: `[b16]` base16, `[b24]` base24, `[  *]` custom.
NOTE: Scheme pre filtered based on user's prefer-dark/prefer-light setting in GNOME. (can be toggled off in config)

Second panel (tab):
```
┌─────────────────────────────────────────────────────┐
│ gnomad — Wallpapers        [Tab] Switch panel   │
├─────────────────────────────────────────────────────┤
│  Dir: ~/Pictures/Wallpapers                         │
│  ───────────────────────────                        │
│  > mountain.jpg                                     │
│    coastal-dusk.png                                 │
│    forest-fog.jpg                                   │
│    desert-night.png                                 │
│                                                     │
│  [Enter]   Apply with current scheme                │
│  [c]       Convert directory for current scheme     │
│  [shift+c] Force re-convert (bypass cache)           │
│  [d]       Change wallpaper directory                │
└─────────────────────────────────────────────────────┘
```


Important: We need to account for display of the images within the TUI - including converted/non-converted states. Terminals with sixel/kitty graphics protocol will be required.
---

## Animation Overlay

Displayed over the full TUI during any pipeline run. Runs at **8–12fps intentionally** —
the low framerate is both the retro aesthetic and the technical property that makes a
compositor freeze during the GNOME Shell CSS reload imperceptible.

Each pipeline step updates a status string the animation renders:
```
[ converting wallpaper...  ]
[ applying tinty scheme... ]
[ writing gtk css...       ]
[ writing shell css...     ]
[ reloading shell...       ]
```

Use `tokio::time::interval` with an 80–125ms tick for the animation loop.
Run animation and pipeline concurrently via `tokio::join!` or separate tasks.

---

## Key Architectural Decisions

| Decision | Rationale |
|---|---|
| base16 + base24 | Standardised YAML, 250+ schemes, base24 adds full ANSI terminal support |
| Tinty for app propagation | Designed for "bring your own scheme"; Rust binary; official tinted-theming tooling |
| gnomad does NOT manage Tinty config | Clean separation — Tinty's item config is user territory |
| gowall via temp JSON file | Runtime palette injection; ephemeral; no config.yml modification |
| Own CSS (forked Rewaita GTK + Shell snapshot) | Avoids upstream breakage; maintenance controlled; covers both GTK apps and Shell panel |
| User Themes extension for Shell CSS | Official GNOME extension (gnome-shell-extensions package); stable across releases; write CSS unconditionally, warn if extension not detected |
| zbus async for dbus | App stays responsive during pipeline; animation loop unblocked |
| 8–12fps animation | Retro aesthetic + compositor freeze imperceptible at this cadence |
| Flatpak via xdg-data/themes | GTK3 Flatpak apps pick up theme via DConf/Settings portal automatically |
| Light/dark toggle for CSS reload | Only reliable way to force GNOME Shell to re-read CSS without an extension |
| Custom scheme directory | User-local YAML schemes alongside remote catalog; same parsing logic |
| Batch-convert as standalone primitive | Built ahead of any consumer; the persisted cache is the entire integration contract a future extension will need; independently useful today for pre-warming schemes |
| Wallpaper cache keyed by scheme slug | Persisted under `~/.local/share`, survives reboot; repeat scheme use is a cache hit, no reconversion |

---

## Notes for Implementation

- Parse both base16 YAML formats (detect by presence of `palette:` key).

- The scheme `slug` is derived from the YAML filename without extension.
  e.g. `base16-gruvbox-dark-hard.yaml` → slug `base16-gruvbox-dark-hard`.
  This slug is passed directly to `tinty apply`.

- Custom schemes placed in `custom_schemes_dir` should also follow the slug convention.
  Note the known tinted-nvim issue with custom schemes (issue #18) — document the
  FocusGained `.vim` file workaround in the README for affected users.

- gowall's color count in the JSON is flexible — pass 16 or 24 depending on scheme system.

- The GNOME `color-scheme` toggle (step 5c) causes the visible compositor freeze.
  Ensure the animation is already rendering before this call fires.

- `picture-uri` expects a `file://` URI, not a raw path.

- Scheme search/filter in the browser is local — filter the in-memory list, no network call.

- On startup, check whether `~/.local/share/themes/<theme_name>` exists. If not, warn the
  user to run the one-time Flatpak override command.

- Check for `tinty` and `gowall` in `$PATH` on startup and surface clear errors if missing.

- Detect whether the User Themes extension is active on startup via zbus:
  check `org.gnome.shell.extensions.user-theme` schema exists and that the extension
  is enabled. Warn clearly if not — Shell CSS will be written but silently ignored.

- The Shell CSS template is a full fork of Rewaita's `gnome-shell.css`. All hardcoded
  colour values in that file should be replaced with base16 slot substitutions. The GTK
  and Shell templates both live in the gnomad repo under `assets/templates/`.

- Batch-convert is callable from both the wallpaper picker (current scheme) and the scheme
  browser (any highlighted scheme, without fully applying it). Both code paths should call
  the same `wallpaper_cache` module function — there is no separate "slideshow" code path.

- `manifest.json` in each scheme's cache directory exists primarily for cache invalidation,
  but keep its schema simple and stable (filename + mtime list) since it doubles as the
  only metadata an external consumer (e.g. a future extension) would have about ordering
  and source images.

---

## Roadmap (post-v1)

- Custom scheme editor/creator
- Scheme favourites / tagging
- Possible separate side project: a GNOME Shell extension implementing custom shader-based
  wallpaper transitions (beyond GNOME's native cross-fade). Its contract with gnomad is
  trivial by design — it just reads images from `~/.local/share/gnomad/wallpapers/<slug>/`,
  the same directory the batch-convert primitive maintains. No runtime coordination, no IPC,
  no awareness of gowall or scheme YAML required on the extension side. Distinct GJS project,
  not part of the gnomad Rust codebase. Maintenance note: extensions require updates
  across major GNOME releases, unlike gnomad itself.
  - Replicate parts of Gnome tweaks: Icon theme selector, font selector, general appearance stuff
- Custom CSS write for AdwSteam, including user font
