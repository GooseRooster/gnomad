# ChromaLeon → tinty bridge

Turn [ChromaLeon](https://github.com/Fabito02/ChromaLeon)'s live colours into a
[tinted8](https://github.com/tinted-theming/tinted-theming) scheme, so your
terminals and other tinty targets follow the same colours as the GNOME Shell.

This is the companion to gnomad's **gowall-only mode**: gnomad converts
wallpapers to a scheme's palette and sets them, ChromaLeon themes the desktop
from whatever wallpaper is current — and this bridge keeps the terminal theme
in step with both.

## How it works

ChromaLeon derives an accent colour from the desktop wallpaper and writes it
into `~/.cache/chromaleon/chromaleon-shell{,-dark}.css`. This bridge:

1. reads ChromaLeon's accent from `org.gnome.shell/extensions/chromaleon`,
2. extracts the wallpaper's own swatches (64×64, GdkPixbuf via GJS — the same
   scale ChromaLeon uses, so the colours match its cache),
3. resolves the *actual* shell surface out of ChromaLeon's generated
   stylesheet by evaluating its `st-mix(...)` expressions, so `tint-shell`,
   `tint-panel`, `tinting-strength`, `darker`, `full-light` and `prefer-light`
   are followed automatically,
4. places the eight palette slots in OKLCH — wallpaper hues normalised into
   consistent lightness/chroma bands, missing hues synthesized from the
   wallpaper's median chroma, collapsed slots pushed apart — and keeps the
   accent slot exact,
5. writes `~/.local/share/tinted-theming/tinty/custom-schemes/tinted8/chromaleon.yaml`
   and runs `tinty apply tinted8-chromaleon`.

The terminal stays deliberately a *sibling* of the shell, not a clone: a depth
dial moves the background away from the shell surface in OKLCH while leaving
the accent exact.

### What makes it fire

| Trigger | Mechanism |
|---|---|
| ChromaLeon restyles the shell | `gnomad-chromaleon.path` watches its generated stylesheets |
| The desktop light/dark toggle flips | `gnomad-chromaleon-variant.service` watches `org.gnome.desktop.interface.color-scheme` |
| The wallpaper changes | the same service watches `org.gnome.desktop.background` |

The wallpaper trigger is what makes the bridge work with gnomad's gowall-only
mode. gowall recolours the wallpaper to the scheme's palette *before* it is
set, so the accent ChromaLeon derives from the new image is almost always the
scheme's own colour and barely moves from image to image. ChromaLeon only
rewrites its stylesheets when that accent changes — so without the wallpaper
watch the bridge would never run and the terminal theme would stay on the
previous wallpaper's palette.

## Requirements

- GNOME with the [ChromaLeon](https://extensions.gnome.org/extension/4000/chromaleon/)
  extension installed and enabled
- `tinty`, `dconf`, `python3`, `flock`
- `gjs` (recommended — exact ChromaLeon pixel extraction) or `magick`
  (fallback; the swatches differ slightly)

## Install

```sh
./install.sh
```

That copies the scripts into `~/.local/bin`, installs the three user units
into `~/.config/systemd/user` and enables them. Uninstall with
`./install.sh --uninstall`.

On Nix, the bridge is also packaged:

```nix
# flake.nix
inputs.gnomad.url = "github:GooseRooster/gnomad";

# configuration.nix
home.packages = [ inputs.gnomad.packages.${system}.chromaleon-bridge ];
```

The packaged scripts carry their own `CHROMALEON_PALETTE` / `CHROMALEON_PIXELS`
paths and the tools they need on `PATH`; the systemd units are the same ones
`install.sh` writes, with `%h/.local/bin` swapped for the store path.

> If you already run a ChromaLeon bridge from your own dotfiles, do **not**
> install this one as well — the two would keep overwriting each other's
> `chromaleon.yaml`. Point your existing bridge at this directory's
> `chromaleon_palette.py` / `chromaleon_pixels.js` instead, or drop yours.

Then point gnomad's wallpaper hook at the bridge, in
`~/.config/gnomad/config.toml`:

```toml
[hooks]
on_wallpaper_apply = "/home/user/.local/bin/chromaleon_tinty"
```

gnomad waits `wallpaper_settle_ms` (default 1500) after setting the wallpaper
before running the hook, so ChromaLeon has published its new accent by the
time the bridge reads it.

> The wallpaper watch already covers wallpaper changes from any source, so the
> hook is belt-and-braces rather than strictly required. It is the
> deterministic variant: the settle window is tuned to ChromaLeon's
> asynchronous colour extraction.

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `CHROMALEON_DEPTH` | `1` | 0 = exact shell-surface match, 3 = pronounced separation |
| `CHROMALEON_SURFACE_TARGET` | `auto` | `auto`, `panel` or `menu` — which shell surface to anchor on |
| `CHROMALEON_PALETTE` | next to the script | path to `chromaleon_palette.py` |
| `CHROMALEON_PIXELS` | next to the script | path to `chromaleon_pixels.js` |

Set them in the `Environment=` line of `gnomad-chromaleon.service` and
`gnomad-chromaleon-variant.service`, or export them from your shell profile.

> The extracted swatches depend on the GdkPixbuf build that scales the
> wallpaper, so a distribution's `gjs` and a Nix-packaged one can land a
> couple of units apart in the palette slots. Pick one `gjs` and stick with
> it; the accent slot and the shell surface are unaffected either way.

## Inspecting the result

Without applying anything, `chromaleon_palette.py --audit` prints the resolved
shell surfaces, each palette slot's OKLCH coordinates, its contrast against the
background, the ΔE from the accent, and any warnings:

```sh
URI=$(dconf read /org/gnome/desktop/background/picture-uri-dark | tr -d "'")
WP=$(python3 -c 'import sys,urllib.parse; print(urllib.parse.unquote(sys.argv[1][7:]))' "$URI")
gjs chromaleon_pixels.js "$WP" \
  | python3 chromaleon_palette.py --pixels-b64 \
      --accent "$(dconf read /org/gnome/shell/extensions/chromaleon/accent-color | tr -d "'")" \
      --variant dark \
      --shell-css "$HOME/.cache/chromaleon/chromaleon-shell-dark.css" \
      --audit
```

## A note on ChromaLeon's own state

ChromaLeon derives its accent asynchronously and serialises all of its work
through a single promise chain (`extension.js`, `_runOperation`).
`ColorUtils.calculateVibrantColor()` has no error handling, so a wallpaper the
extension cannot decode — an XML slideshow, a JPEG XL file — leaves that
promise unsettled, and every later operation (including `accent-color`
changes) queues behind it forever.

The symptom is "the extension still shows the new colours in its UI, but
changing them does nothing". Reloading the extension clears it:

```sh
gnome-extensions disable user-accent-colors@fabito02
gnome-extensions enable user-accent-colors@fabito02
```

## Licence

GPL-3.0-or-later, like gnomad. `chromaleon_palette.py` is a port of parts of
ChromaLeon's `utils/colorUtils.js` and `ui/wallpaperPage.js`, which are
GPL-3.0-or-later.
