#!/usr/bin/env sh
# Install the ChromaLeon -> tinty bridge for the current user.
#
#   ./install.sh          install and enable
#   ./install.sh --uninstall
#
# The scripts land in ~/.local/bin, the user units in ~/.config/systemd/user.
# Nothing outside $HOME is touched and no root privileges are needed.

set -eu

_self="$0"
if command -v readlink >/dev/null 2>&1; then
  _resolved="$(readlink -f "$_self" 2>/dev/null || true)"
  if [ -n "$_resolved" ]; then
    _self="$_resolved"
  fi
fi
src="$(CDPATH= cd -- "$(dirname -- "$_self")" && pwd)"

home="${HOME:?HOME is not set}"
bin_dir="$home/.local/bin"
unit_dir="$home/.config/systemd/user"

units="gnomad-chromaleon.path gnomad-chromaleon.service gnomad-chromaleon-variant.service"

if [ "${1:-}" = "--uninstall" ]; then
  systemctl --user disable --now gnomad-chromaleon.path gnomad-chromaleon-variant.service 2>/dev/null || true
  for unit in $units; do
    rm -f "$unit_dir/$unit"
  done
  rm -f "$bin_dir/chromaleon_tinty" \
        "$bin_dir/chromaleon_tinty_watch" \
        "$bin_dir/chromaleon_palette.py" \
        "$bin_dir/chromaleon_pixels.js"
  systemctl --user daemon-reload 2>/dev/null || true
  echo "removed."
  exit 0
fi

echo "checking dependencies..."
missing=""
for dep in dconf flock python3 tinty; do
  command -v "$dep" >/dev/null 2>&1 || missing="$missing $dep"
done
if command -v gjs >/dev/null 2>&1; then
  echo "  gjs: yes (exact ChromaLeon pixel extraction)"
else
  echo "  gjs: no  (falling back to ImageMagick; swatches will differ slightly)"
fi
if command -v magick >/dev/null 2>&1; then
  echo "  magick: yes"
else
  echo "  magick: no (only needed without gjs)"
fi
if [ -n "$missing" ]; then
  echo "missing required commands:$missing" >&2
  exit 1
fi

mkdir -p "$bin_dir" "$unit_dir"
for file in chromaleon_tinty chromaleon_tinty_watch chromaleon_palette.py chromaleon_pixels.js; do
  install -m 0755 "$src/$file" "$bin_dir/$file"
done
for unit in $units; do
  install -m 0644 "$src/systemd/$unit" "$unit_dir/$unit"
done

systemctl --user daemon-reload
systemctl --user enable --now gnomad-chromaleon.path gnomad-chromaleon-variant.service

cat <<EOF

installed:
  $bin_dir/chromaleon_tinty
  $bin_dir/chromaleon_tinty_watch
  $unit_dir/gnomad-chromaleon.path
  $unit_dir/gnomad-chromaleon.service
  $unit_dir/gnomad-chromaleon-variant.service

Now point gnomad's wallpaper hook at the bridge, in
$home/.config/gnomad/config.toml:

  [hooks]
  on_wallpaper_apply = "$bin_dir/chromaleon_tinty"

gnomad waits wallpaper_settle_ms (default 1500) after setting the wallpaper
before running the hook, so ChromaLeon has published its new accent by then.
EOF
