// ChromaLeon-compatible wallpaper pixel extractor.
//
// Part of gnomad's ChromaLeon -> tinty bridge (assets/chromaleon-bridge/).
// Licensed under GPL-3.0-or-later.
//
// ChromaLeon scales the wallpaper with GdkPixbuf
// (new_from_stream_at_scale(..., 64, 64, true)) and then quantises the
// resulting pixbuf.  ImageMagick's resize filters do not reproduce those
// pixels exactly, so this helper does the same GdkPixbuf load and emits the
// tightly packed RGB bytes (base64) for chromaleon_palette.py.
//
// Usage: gjs chromaleon_pixels.js /path/to/wallpaper.jpg

const GdkPixbuf = imports.gi.GdkPixbuf;
const GLib = imports.gi.GLib;
const System = imports.system;

const path = ARGV[0];
if (!path) {
  printerr("chromaleon_pixels: missing wallpaper path");
  System.exit(1);
}

let pixbuf;
try {
  pixbuf = GdkPixbuf.Pixbuf.new_from_file_at_scale(path, 64, 64, true);
} catch (error) {
  printerr(`chromaleon_pixels: ${error.message}`);
  System.exit(1);
}

const width = pixbuf.get_width();
const height = pixbuf.get_height();
const stride = pixbuf.get_rowstride();
const channels = pixbuf.get_n_channels();
const pixels = pixbuf.get_pixels();

const rgb = new Uint8Array(width * height * 3);
let offset = 0;
for (let y = 0; y < height; y++) {
  for (let x = 0; x < width; x++) {
    const source = y * stride + x * channels;
    rgb[offset++] = pixels[source];
    rgb[offset++] = pixels[source + 1];
    rgb[offset++] = pixels[source + 2];
  }
}

print(GLib.base64_encode(rgb));
