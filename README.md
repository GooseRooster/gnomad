[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/GooseRooster/gnomad)

# Important note
Currently still in heavy development and testing. A release will be published soon.

# gnomad

Your GNOME theming companion - right in the terminal. Built in Rust with [Ratatui](https://ratatui.rs), leveraging [gowall](https://github.com/Achno/gowall) and [tinty](https://github.com/tinted-theming/tinty).

Browse and apply base16/base24 colour schemes across your entire GNOME desktop in one keypress. On scheme switch, gnomad converts your wallpaper to match the palette, delegates terminal and app theming to Tinty, writes custom GTK 3/4 and GNOME Shell CSS, and triggers a shell reload. A second panel lets you browse your wallpaper directory and apply any image against the active scheme.

---

## Screenshots
(coming soon)

---

## Features

- **250+ schemes out of the box** — pulls the full [tinted-theming/schemes](https://github.com/tinted-theming/schemes) catalogue (base16 + base24) via git clone on first run
- **Full GNOME integration** — GTK 3, GTK 4 (libadwaita), and GNOME Shell panel all theme together
- **Wallpaper colour conversion** via gowall — converts your wallpaper to match the active palette on every scheme switch
- **Terminal and app propagation** via Tinty — themes any app Tinty supports (kitty, alacritty, neovim, etc.) based on your Tinty config
- **Smart wallpaper cache** — batch-convert an entire wallpaper directory for any scheme; subsequent switches to that scheme are instant (no reconversion)
- **Image preview** — live wallpaper preview in the picker using Sixel or Kitty Graphics Protocol
- **Colour swatches** — inline base16/base24 palette preview for every scheme in the browser
- **Search** — fuzzy filter the scheme list as you type
- **Custom schemes** — drop your own YAML files into a configured directory and they appear alongside the catalogue
- **Dark/light preference** — optionally filter schemes to match your GNOME colour scheme setting (prefer-dark / prefer-light)

---

## Requirements

- GNOME 45+ on Wayland
- `gowall` in `$PATH` — [installation](https://github.com/Achno/gowall#installation)
- `tinty` in `$PATH` — `cargo install tinty`
- `git` in `$PATH`
- A terminal with [Sixel](https://en.wikipedia.org/wiki/Sixel) or [Kitty Graphics Protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/) support for wallpaper preview (e.g. Kitty, foot, WezTerm, Ghostty)

---

## Installation
Manual installation
```bash
git clone https://github.com/GooseRooser/gnomad
cd gnomad
cargo install --path .
```

Cargo package will be published later.

On first launch gnomad will clone the tinted-theming schemes repository into `~/.local/share/gnomad/schemes-repo` automatically.

---

## One-Time Setup

### 1. Shell theme (required for GNOME Shell panel theming)

The GNOME Shell panel only themes if the User Themes extension is active and "gnomad" is selected as the shell theme. On Fedora, the extension ships with `gnome-shell-extensions`:

```bash
sudo dnf install gnome-shell-extensions
gnome-extensions enable user-theme@gnome-shell-extensions.gcampax.github.com
```

After running gnomad once (which writes the theme files), select the shell theme:

```bash
gsettings set org.gnome.shell.extensions.user-theme name "gnomad"
```

Or via GNOME Tweaks → Appearance → Shell → gnomad.

### 2. Flatpak GTK theming (required for Flatpak apps)

Allow Flatpak apps to see the user GTK CSS and theme directory:

```bash
flatpak override --user --filesystem=xdg-config/gtk-3.0
flatpak override --user --filesystem=xdg-config/gtk-4.0
flatpak override --user --filesystem=xdg-data/themes
```

### 3. Tinty configuration

gnomad calls `tinty apply <scheme-slug>` — it does not manage your Tinty item configuration. Set up `~/.config/tinted-theming/tinty/config.toml` with whatever apps you want Tinty to theme (tinted-shell, tinted-kitty, etc.), then sync:

```bash
tinty sync
```

See the [Tinty documentation](https://github.com/tinted-theming/tinty) for details.

gnomad will warn on startup if User Themes is not detected or if Tinty/gowall are missing from `$PATH`.

---

## Config

Location: `~/.config/gnomad/config.toml`. Created with defaults on first run.

```toml
wallpaper_dir = "/home/user/Pictures/Wallpapers"
custom_schemes_dir = "/home/user/.config/gnomad/schemes"  # optional
theme_name = "gnomad"
default_scheme = "base16-gruvbox-dark-hard"               # optional
output_wallpaper_path = "~/.local/share/gnomad/current-wallpaper.png"
wallpaper_cache_dir = "~/.local/share/gnomad/wallpapers"
follow_user_scheme_type = true  # filter schemes by GNOME dark/light preference
```

| Key | Default | Description |
|---|---|---|
| `wallpaper_dir` | `~/Pictures/Wallpapers` | Directory gnomad reads wallpapers from |
| `custom_schemes_dir` | — | Optional directory of user-supplied YAML scheme files |
| `theme_name` | `gnomad` | Name used for the GTK/Shell theme directory |
| `default_scheme` | — | Slug to pre-select on launch |
| `output_wallpaper_path` | `~/.local/share/gnomad/current-wallpaper.png` | Where the converted wallpaper is written |
| `wallpaper_cache_dir` | `~/.local/share/gnomad/wallpapers` | Root for per-scheme wallpaper cache |
| `follow_user_scheme_type` | `true` | Filter scheme list to match GNOME's prefer-dark/prefer-light setting |

The wallpaper directory can also be changed at runtime with `[d]` in the wallpaper panel.

---

## Key Bindings

| Key | Action |
|---|---|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `g` | Jump to top |
| `G` | Jump to bottom |
| `Ctrl+d` | Half-page down |
| `Ctrl+u` | Half-page up |
| `Tab` / `l` / `h` | Switch between Schemes and Wallpapers panels |
| `Enter` | Apply selected scheme or wallpaper |
| `/` | Search schemes (Schemes panel) |
| `Esc` | Close search |
| `u` | Update schemes from remote (Schemes panel) |
| `c` | Batch-convert wallpaper directory for selected/active scheme |
| `Shift+C` | Force re-convert (bypass cache) |
| `d` | Change wallpaper directory (Wallpapers panel) |
| `q` | Quit |

---

## How It Works

### Scheme switch pipeline

When you press `Enter` on a scheme, gnomad runs these steps sequentially with a spinner overlay:

1. **gowall** — converts your current wallpaper to the new palette
2. **Tinty** — `tinty apply <slug>` — propagates the scheme to configured terminal apps
3. **GTK CSS** — writes colour variables to `~/.config/gtk-3.0/gtk.css` (full template) and `~/.config/gtk-4.0/gnomad-colors.css` (@define-color entries imported by `gtk.css`); GTK4's CSS file-watcher picks up the change and reloads colours in running LibAdwaita apps automatically
4. **Shell CSS** — writes a fully-resolved `gnome-shell.css` to `~/.local/share/themes/gnomad/gnome-shell/`
5. **GNOME reload** — sets the wallpaper URI via gsettings, then toggles `color-scheme` to force the shell to re-read the CSS

The animation overlay is intentionally low-framerate — the light/dark toggle causes a compositor-level freeze across all Wayland clients that cannot be eliminated, only obscured.
This is a limitation with how shell reloading works on GNOME currently (essentially - hacks.) But I decided to turn it into a feature ;)

### Wallpaper switch

Picking a wallpaper and pressing `Enter` runs only gowall + wallpaper set; no CSS or Tinty calls. If the wallpaper has already been converted for the active scheme (cache hit), gowall is skipped entirely and the switch is instant.

### Batch convert

`[c]` converts every image in your wallpaper directory against a scheme and stores the results under `~/.local/share/gnomad/wallpapers/<scheme-slug>/`. This is a pre-warming primitive: subsequent wallpaper switches under that scheme never call gowall. A `manifest.json` in each directory tracks source mtimes for cache invalidation.

---

## Custom Schemes

Place any base16/base24 YAML files in your configured `custom_schemes_dir`. They appear in the browser with a `[*]` tag and support everything the catalogue schemes do. Both the new format (with `palette:` key) and the legacy flat format are parsed.

Tinty custom scheme note: if you encounter issues with `tinty apply` on custom slugs, see [tinted-nvim issue #18](https://github.com/tinted-theming/tinted-nvim/issues/18) for the `FocusGained` workaround.

---

## CLI

```bash
gnomad                    # launch TUI
gnomad --update-schemes   # pull latest schemes and exit
gnomad --apply <slug>     # headless scheme apply and exit (e.g. for scripting)
```

---

## AI Disclosure
The code in this repository was written with assistance from AI.
All code, whether AI-assisted, hand-written or otherwise, is thoroughly tested and verified and all contributors will take ownership of their code, before releases are published.


## Contributing
As always, feature requests, PRs, issues, and bug reports welcome. If the scope of the feature is on the larger side, open an issue first so we can discuss direction.

## Roadmap

- Custom scheme editor
- Scheme favourites and tagging
- GNOME Shell extension for wallpaper rotation with custom transitions, consuming gnomad's wallpaper cache directory directly
- AdwSteam CSS support
- Additional GNOME Tweaks surface (icon theme, fonts)

---

## Third Party Contributions
- [Rewaita](https://github.com/SwordPuffin/Rewaita) — CSS templates (GPL-3.0)
- [ChromaLeon](https://github.com/DerDakon/ChromaLeon) — GTK4 live CSS reload architecture (GPL-3.0)

  gnomad's GTK4 theming writes colour variables to a separate `gnomad-colors.css` file and has
  `gtk.css` import it, rather than writing directly to `gtk.css`. This `@import` pattern is how
  GTK4's CSS provider file-watching is triggered to reload colours in running LibAdwaita apps
  without restarting them. We discovered this mechanism by studying ChromaLeon's source.

## Special Thanks

- [Rewaita](https://github.com/SwordPuffin/Rewaita) - please check it out. I was heavily inspired by the approach Rewaita takes to theming and the CSS templates were directly responsible for even making gnomad possible. Try it, star it!!!
- [ChromaLeon](https://github.com/DerDakon/ChromaLeon) - the GTK4 live reload trick that makes running LibAdwaita apps pick up new colours instantly. Genuinely could not have cracked this without studying their code.
- [Tinted Theming](https://github.com/tinted-theming/home.git) - the incredible base* and tinted* colorscheme support and scheme repository.
- [Gowall](https://github.com/tinted-theming/home.git) - Wallpaper color scheming. What's not to love?
- [Ratatui](https://ratatui.rs) - Cookin

## License

GPL-3.0
