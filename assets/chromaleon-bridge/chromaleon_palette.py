#!/usr/bin/env python3
"""Turn ChromaLeon's wallpaper palette into a tinted8 scheme.

Part of gnomad's ChromaLeon -> tinty bridge (assets/chromaleon-bridge/).
Licensed under GPL-3.0-or-later.

Wallpaper pixels come from ChromaLeon's own GdkPixbuf scaling
(chromaleon_pixels.js, next to this file) so the extracted swatches match the
extension. The extraction/contrast/dedupe path is still a port of
ChromaLeon's utils/colorUtils.js and ui/wallpaperPage.js.

ChromaLeon tints its shell by mixing the chosen accent into a fixed neutral
surface. We resolve that *actual* surface from the generated shell stylesheet
(so tint-shell, tint-panel, tinting-strength, darker, full-light and
gnome-colors are followed automatically) and use it as the scheme's
background anchor. The chromatic ANSI slots are then placed in OKLCH so that
wallpaper colours keep their character without collapsing into "red plus six
variations of red", or turning into neon fallbacks.

The result is deliberately a sibling of the ChromaLeon surface, not a clone:
`--depth` moves the scheme away from the shell surface in OKLCH, adding
luminance separation and contrast while preserving the exact accent.
"""

import argparse
import base64
import math
import re
import sys
from statistics import median

STEP = 24

# Reference hues for the six chromatic tinted8 slots, in OKLCH degrees.
OKLCH_SLOTS = [
    ("red", 29.0),
    ("yellow", 110.0),
    ("green", 145.0),
    ("cyan", 195.0),
    ("blue", 264.0),
    ("magenta", 328.0),
]
SLOT_NAMES = [name for name, _ in OKLCH_SLOTS]
REFERENCE_HUES = dict(OKLCH_SLOTS)

# GNOME's named accents, matching ChromaLeon's GNOME_ACCENTS_HEX table.
GNOME_ACCENTS_HEX = {
    "blue": "#3584e4",
    "teal": "#2190a4",
    "green": "#3a944a",
    "yellow": "#c88800",
    "orange": "#ed5b00",
    "red": "#e62d42",
    "pink": "#d56199",
    "purple": "#9141ac",
    "slate": "#6f8396",
}

# ChromaLeon's contrast anchors (relative luminance of its light/dark bgs).
LIGHT_BG_LUMINANCE = 0.957
DARK_BG_LUMINANCE = 0.037

# ChromaLeon's shell base surfaces, used only when the generated CSS cannot be
# parsed (first session start, dynamic wallpaper, extension reloading, ...).
FALLBACK_BASES = {
    "dark": {
        False: {"panel": "#1b1b1d", "menu": "#36363a"},
        True: {"panel": "#0f0f10", "menu": "#232326"},
    },
    "light": {
        False: {"panel": "#fafafb", "menu": "#fafafb"},
        True: {"panel": "#fafafb", "menu": "#fafafb"},
    },
}

# getConvertedStrength() from ChromaLeon's themeUtils.js.
SHELL_STRENGTH = {
    "dark": {0: 0.05, 1: 0.10, 2: 0.15, 3: 0.20},
    "light": {0: 0.07, 1: 0.12, 2: 0.17, 3: 0.22},
}

# The depth dial. Level 0 is an exact match to the ChromaLeon surface; each
# level pushes the terminal background away from the shell in OKLCH.
DEPTH_L_DARK = [0.0, -0.015, -0.030, -0.045]
DEPTH_C_BG = [1.0, 0.94, 0.88, 0.82]
DEPTH_FG_DARK = [0.0, 0.006, 0.012, 0.018]
DEPTH_C_ANSI = [1.0, 1.04, 1.08, 1.12]
DEPTH_L_LIGHT = [0.0, -0.006, -0.012, -0.018]
DEPTH_FG_LIGHT = [0.0, -0.006, -0.012, -0.018]


def clamp(value, low, high):
    return max(low, min(high, value))


def srgb_to_linear(channel):
    channel /= 255.0
    if channel <= 0.04045:
        return channel / 12.92
    return ((channel + 0.055) / 1.055) ** 2.4


def linear_to_srgb(channel):
    channel = max(0.0, min(1.0, channel))
    if channel <= 0.0031308:
        return 255.0 * 12.92 * channel
    return 255.0 * (1.055 * (channel ** (1 / 2.4)) - 0.055)


def hex_to_rgb(hex_color):
    value = hex_color.lstrip("#")
    if len(value) != 6:
        return (0, 0, 0)
    return tuple(int(value[index : index + 2], 16) for index in (0, 2, 4))


def rgb_to_hex(rgb):
    return "#" + "".join(
        f"{max(0, min(255, round(channel))):02x}" for channel in rgb
    )


def cube_root(value):
    return math.copysign(abs(value) ** (1 / 3), value)


def rgb_to_oklab(rgb):
    red, green, blue = (srgb_to_linear(channel) for channel in rgb)

    long = 0.4122214708 * red + 0.5363325363 * green + 0.0514459929 * blue
    medium = 0.2119034982 * red + 0.6806995451 * green + 0.1073969566 * blue
    short = 0.0883024619 * red + 0.2817188376 * green + 0.6299787005 * blue

    long_ = cube_root(long)
    medium_ = cube_root(medium)
    short_ = cube_root(short)

    return (
        0.2104542553 * long_ + 0.7936177850 * medium_ - 0.0040720468 * short_,
        1.9779984951 * long_ - 2.4285922050 * medium_ + 0.4505937099 * short_,
        0.0259040371 * long_ + 0.7827717662 * medium_ - 0.8086757660 * short_,
    )


def oklab_to_rgb(oklab):
    lightness, a, b = oklab

    long_ = lightness + 0.3963377774 * a + 0.2158037573 * b
    medium_ = lightness - 0.1055613458 * a - 0.0638541728 * b
    short_ = lightness - 0.0894841775 * a - 1.2914855480 * b

    long = long_**3
    medium = medium_**3
    short = short_**3

    red = 4.0767416621 * long - 3.3077115913 * medium + 0.2309699292 * short
    green = -1.2684380046 * long + 2.6097574011 * medium - 0.3413193965 * short
    blue = -0.0041960863 * long - 0.7034186147 * medium + 1.7076147010 * short

    return tuple(linear_to_srgb(channel) for channel in (red, green, blue))


def hex_to_oklab(hex_color):
    return rgb_to_oklab(hex_to_rgb(hex_color))


def hex_to_oklch(hex_color):
    lightness, a, b = hex_to_oklab(hex_color)
    chroma = math.hypot(a, b)
    hue = math.degrees(math.atan2(b, a)) % 360
    return (lightness, chroma, hue)


def oklch_to_hex(lightness, chroma, hue):
    """Convert OKLCH to hex, reducing chroma until the colour is in gamut."""
    lightness = clamp(lightness, 0.0, 1.0)
    chroma = max(0.0, chroma)
    radians = math.radians(hue)

    rgb = (0.0, 0.0, 0.0)
    for _ in range(32):
        a = chroma * math.cos(radians)
        b = chroma * math.sin(radians)
        rgb = oklab_to_rgb((lightness, a, b))
        if all(-1.0 <= channel <= 256.0 for channel in rgb):
            break
        chroma *= 0.94

    return rgb_to_hex(rgb)


def hue_delta(first, second):
    difference = abs(first - second) % 360
    return min(difference, 360 - difference)


def hue_blend(first, second, factor):
    difference = ((second - first + 180) % 360) - 180
    return (first + difference * factor) % 360


def oklab_delta(first_hex, second_hex):
    first = hex_to_oklab(first_hex)
    second = hex_to_oklab(second_hex)
    return math.sqrt(sum((a - b) ** 2 for a, b in zip(first, second)))


def js_round(value):
    """JavaScript's Math.round (half away from zero, i.e. up for positives)."""
    return math.floor(value + 0.5)


def to_hex(channel):
    return f"{max(0, min(255, js_round(channel * 255))):02x}"


def rgb_to_hsl(r, g, b):
    r /= 255.0
    g /= 255.0
    b /= 255.0

    maximum = max(r, g, b)
    minimum = min(r, g, b)
    lightness = (maximum + minimum) / 2

    if maximum == minimum:
        return [0, 0, js_round(lightness * 100)]

    delta = maximum - minimum
    saturation = (
        delta / (2 - maximum - minimum)
        if lightness > 0.5
        else delta / (maximum + minimum)
    )

    if maximum == r:
        hue = (g - b) / delta + (6 if g < b else 0)
    elif maximum == g:
        hue = (b - r) / delta + 2
    else:
        hue = (r - g) / delta + 4

    return [
        js_round((hue / 6) * 360),
        js_round(saturation * 100),
        js_round(lightness * 100),
    ]


def hsl_to_rgb(hue, saturation, lightness):
    hue /= 360.0
    saturation /= 100.0
    lightness /= 100.0

    if saturation == 0:
        return (lightness, lightness, lightness)

    def hue_to_rgb(p, q, t):
        if t < 0:
            t += 1
        if t > 1:
            t -= 1
        if t < 1 / 6:
            return p + (q - p) * 6 * t
        if t < 1 / 2:
            return q
        if t < 2 / 3:
            return p + (q - p) * (2 / 3 - t) * 6
        return p

    q = (
        lightness * (1 + saturation)
        if lightness < 0.5
        else lightness + saturation - lightness * saturation
    )
    p = 2 * lightness - q

    return (
        hue_to_rgb(p, q, hue + 1 / 3),
        hue_to_rgb(p, q, hue),
        hue_to_rgb(p, q, hue - 1 / 3),
    )


def relative_luminance(r, g, b):
    def adjust(channel):
        if channel <= 0.04045:
            return channel / 12.92
        return ((channel + 0.055) / 1.055) ** 2.4

    return 0.2126 * adjust(r) + 0.7152 * adjust(g) + 0.0722 * adjust(b)


def hex_luminance(hex_color):
    return relative_luminance(
        *[channel / 255.0 for channel in hex_to_rgb(hex_color)]
    )


def contrast_ratio(lum1, lum2):
    brightest = max(lum1, lum2)
    darkest = min(lum1, lum2)
    return js_round(((brightest + 0.05) / (darkest + 0.05)) * 100) / 100


def contrast_ratio_hex(first, second):
    return contrast_ratio(hex_luminance(first), hex_luminance(second))


def adjust_contrast(color):
    """ChromaLeon's _adjustContrast, mutating hue/sat/light/hex in place."""
    r, g, b = hsl_to_rgb(color["h"], color["s"], color["l"])
    luminance = relative_luminance(r, g, b)
    lightness = color["l"]
    initial_lightness = lightness

    if contrast_ratio(luminance, LIGHT_BG_LUMINANCE) < 3:
        while contrast_ratio(luminance, LIGHT_BG_LUMINANCE) < 3 and lightness > 0:
            lightness -= 1
            rgb = hsl_to_rgb(color["h"], color["s"], lightness)
            luminance = relative_luminance(*rgb)
    elif contrast_ratio(luminance, DARK_BG_LUMINANCE) < 3:
        while (
            contrast_ratio(luminance, DARK_BG_LUMINANCE) < 3
            and lightness < 100
        ):
            lightness += 1
            rgb = hsl_to_rgb(color["h"], color["s"], lightness)
            luminance = relative_luminance(*rgb)

    lightness_shift = abs(lightness - initial_lightness)
    is_deep_dark = initial_lightness < 15
    is_bright_light = initial_lightness > 85
    is_muted = 8 <= color["s"] < 35

    if is_deep_dark:
        color["s"] = js_round(color["s"] * (initial_lightness / 15.0))
    elif is_bright_light:
        color["s"] = js_round(color["s"] * ((100 - initial_lightness) / 15.0))
    elif lightness_shift > 10 and is_muted:
        boost = 1 + (lightness_shift / 50.0) * 0.4
        color["s"] = min(45, js_round(color["s"] * boost))

    color["l"] = lightness
    r, g, b = hsl_to_rgb(color["h"], color["s"], lightness)
    color["hex"] = "#" + to_hex(r) + to_hex(g) + to_hex(b)


def extract_colors(raw):
    """Port of _extractColorsFromPixbuf; returns up to 60 adjusted colours.

    The caller may pass any pixel count (ChromaLeon's aspect-preserving scale
    is 64xN for most wallpapers), so this no longer requires a 64x64 buffer.
    """
    if len(raw) < 3:
        return []

    buckets = {}
    for offset in range(0, len(raw) - 2, 3):
        r = raw[offset]
        g = raw[offset + 1]
        b = raw[offset + 2]
        hue, saturation, lightness = rgb_to_hsl(r, g, b)

        qr = min(255, (r // STEP) * STEP + STEP // 2)
        qg = min(255, (g // STEP) * STEP + STEP // 2)
        qb = min(255, (b // STEP) * STEP + STEP // 2)
        key = f"#{qr:02x}{qg:02x}{qb:02x}"

        if key not in buckets:
            buckets[key] = {
                "hex": key,
                "count": 0,
                "h": hue,
                "s": saturation,
                "l": lightness,
            }
        buckets[key]["count"] += 1

    colors = list(buckets.values())

    def vibrancy(color):
        return (
            color["s"]
            * color["s"]
            * (1 - abs(color["l"] - 50) / 50)
            * math.log(color["count"] + 1)
        )

    vibrant_ranking = sorted(colors, key=vibrancy, reverse=True)

    final = []
    used = []

    def too_similar(color):
        for seen in used:
            hue_diff = abs(color["h"] - seen["h"])
            if hue_diff > 180:
                hue_diff = 360 - hue_diff
            sat_diff = abs(color["s"] - seen["s"])
            light_diff = abs(color["l"] - seen["l"])
            if hue_diff < 25 and sat_diff < 20 and light_diff < 20:
                return True
        return False

    def keep(color):
        adjust_contrast(color)
        if color["hex"] not in {item["hex"] for item in final} and not too_similar(
            color
        ):
            final.append(color)
            used.append({"h": color["h"], "s": color["s"], "l": color["l"]})

    for color in vibrant_ranking:
        keep(color)

    frequency_ranking = sorted(colors, key=lambda c: c["count"], reverse=True)
    for color in frequency_ranking:
        keep(color)

    return final[:60]


def hex_to_hsl(hex_color):
    value = hex_color.lstrip("#")
    if len(value) != 6:
        return rgb_to_hsl(0, 0, 0)
    return rgb_to_hsl(
        int(value[0:2], 16), int(value[2:4], 16), int(value[4:6], 16)
    )


def split_args(value):
    """Split a comma list, respecting nested parentheses."""
    parts = []
    depth = 0
    start = 0
    for index, character in enumerate(value):
        if character == "(":
            depth += 1
        elif character == ")":
            depth -= 1
        elif character == "," and depth == 0:
            parts.append(value[start:index])
            start = index + 1
    parts.append(value[start:])
    return [part.strip() for part in parts]


def parse_color(token, accent, gnome_accent):
    token = token.strip()
    if token in ("-st-accent-color", "st-accent-color"):
        return gnome_accent or accent

    if token.startswith("#"):
        if len(token) == 7:
            return token.lower()
        if len(token) == 4:
            return "#" + "".join(character * 2 for character in token[1:])

    match = re.match(r"rgba?\(([^)]+)\)", token)
    if match:
        values = [part.strip() for part in match.group(1).split(",")]
        if len(values) >= 3:
            try:
                return rgb_to_hex(tuple(float(value) for value in values[:3]))
            except ValueError:
                return None

    return None


def parse_mix(expression, accent, gnome_accent):
    expression = expression.strip()
    if expression.startswith("st-mix(") and expression.endswith(")"):
        parts = split_args(expression[len("st-mix(") : -1])
        if len(parts) == 3:
            first = parse_mix(parts[0], accent, gnome_accent)
            second = parse_mix(parts[1], accent, gnome_accent)
            if first is None or second is None:
                return None
            factor_text = parts[2].strip()
            try:
                if factor_text.endswith("%"):
                    factor = float(factor_text[:-1]) / 100.0
                else:
                    factor = float(factor_text)
            except ValueError:
                return None
            first_rgb = hex_to_rgb(first)
            second_rgb = hex_to_rgb(second)
            return rgb_to_hex(
                tuple(
                    first_rgb[index] * factor
                    + second_rgb[index] * (1 - factor)
                    for index in range(3)
                )
            )

    return parse_color(expression, accent, gnome_accent)


def css_blocks(text):
    """Yield (selectors, declarations) from the generated shell stylesheet."""
    for match in re.finditer(r"([^{}]+)\{([^{}]*)\}", text):
        yield match.group(1), match.group(2)


def parse_css_surface(text, selector, accent, gnome_accent):
    value = None
    for selectors, body in css_blocks(text):
        names = [name.strip() for name in selectors.split(",")]
        if selector not in names:
            continue
        for declaration in body.split(";"):
            if declaration.strip().startswith("background-color:"):
                value = declaration.split(":", 1)[1].strip()

    if value is None:
        return None

    return parse_mix(value, accent, gnome_accent)


def fallback_surface(
    role, variant, accent, tint_shell, tint_panel, strength, darker
):
    base = FALLBACK_BASES[variant][darker][role]
    tinted = tint_shell and (role != "panel" or tint_panel)
    if not tinted:
        return base

    factor = SHELL_STRENGTH[variant][clamp(strength, 0, 3)]
    accent_rgb = hex_to_rgb(accent)
    base_rgb = hex_to_rgb(base)
    return rgb_to_hex(
        tuple(
            accent_rgb[index] * factor + base_rgb[index] * (1 - factor)
            for index in range(3)
        )
    )


def read_text(path):
    if not path:
        return ""
    try:
        with open(path, "r", encoding="utf-8") as handle:
            return handle.read()
    except OSError:
        return ""


def ensure_contrast(hex_color, background, target, prefer_light):
    if contrast_ratio_hex(hex_color, background) >= target:
        return hex_color

    lightness, chroma, hue = hex_to_oklch(hex_color)
    low, high = (lightness, 1.0) if prefer_light else (0.0, lightness)
    best = None

    for _ in range(40):
        midpoint = (low + high) / 2
        candidate = oklch_to_hex(midpoint, chroma, hue)
        if contrast_ratio_hex(candidate, background) >= target:
            best = candidate
            if prefer_light:
                high = midpoint
            else:
                low = midpoint
        else:
            if prefer_light:
                low = midpoint
            else:
                high = midpoint

    if best is not None:
        return best
    return "#ffffff" if prefer_light else "#000000"


def annotate_colors(colors):
    annotated = []
    for color in colors:
        item = dict(color)
        item["oklch"] = hex_to_oklch(item["hex"])
        annotated.append(item)
    return annotated


def choose_foreground(candidates, variant, depth, background, accent_lch):
    is_dark = variant == "dark"

    # The foreground is tinted by the accent's hue rather than by whichever
    # wallpaper swatch happens to be lightest: a warm canyon accent should
    # produce a warm off-white, not a lavender one.
    if candidates:
        pick = (
            max(candidates, key=lambda color: color["oklch"][0])
            if is_dark
            else min(candidates, key=lambda color: color["oklch"][0])
        )
        lightness, chroma, _hue = pick["oklch"]
        if is_dark:
            lightness = max(lightness, 0.90) + DEPTH_FG_DARK[depth]
            chroma = min(chroma, 0.020)
        else:
            lightness = min(lightness, 0.22) + DEPTH_FG_LIGHT[depth]
            chroma = min(chroma, 0.025)
    else:
        if is_dark:
            lightness, chroma = (0.96, 0.012)
        else:
            lightness, chroma = (0.12, 0.012)

    candidate = oklch_to_hex(lightness, chroma, accent_lch[2])
    return ensure_contrast(candidate, background, 7.0, prefer_light=is_dark)


def choose_slot(reference_hue, candidates, accent_lch, variant, depth):
    is_dark = variant == "dark"
    best = None

    for color in candidates:
        lightness, chroma, hue = color["oklch"]
        if chroma < 0.020 or not (0.18 <= lightness <= 0.92):
            continue
        distance = hue_delta(hue, reference_hue)
        if distance > 35:
            continue
        score = distance - min(chroma, 0.2) * 20
        if best is None or score < best[0]:
            best = (score, color)

    chroma_values = [
        color["oklch"][1]
        for color in candidates
        if color["oklch"][1] >= 0.020 and color["oklch"][0] <= 0.92
    ]
    median_chroma = (
        median(chroma_values) if chroma_values else accent_lch[1] * 0.6
    )

    chroma_low = clamp(accent_lch[1] * 0.45, 0.045, 0.09)
    chroma_high = clamp(accent_lch[1] * 1.15, 0.10, 0.18)
    if is_dark:
        lightness_low, lightness_high = 0.60, 0.72
    else:
        lightness_low, lightness_high = 0.42, 0.55

    if best is not None:
        lightness, chroma, hue = best[1]["oklch"]
        lightness = clamp(lightness, lightness_low, lightness_high)
        chroma = clamp(chroma, chroma_low, chroma_high)
        source = "wallpaper"
    else:
        lightness = (lightness_low + lightness_high) / 2
        chroma = clamp(median_chroma, chroma_low, chroma_high)
        hue = reference_hue
        source = "synthetic"

    chroma = clamp(chroma * DEPTH_C_ANSI[depth], chroma_low, chroma_high)
    return {
        "hex": oklch_to_hex(lightness, chroma, hue),
        "source": source,
    }


def build_palette(candidates, accent, variant, surface, depth):
    is_dark = variant == "dark"
    accent_lch = hex_to_oklch(accent)
    surface_lch = hex_to_oklch(surface)

    if is_dark:
        background_lightness = clamp(
            surface_lch[0] + DEPTH_L_DARK[depth], 0.08, 0.45
        )
    else:
        background_lightness = clamp(
            surface_lch[0] + DEPTH_L_LIGHT[depth], 0.55, 0.99
        )

    background_chroma = surface_lch[1] * DEPTH_C_BG[depth]
    background = oklch_to_hex(
        background_lightness, background_chroma, surface_lch[2]
    )
    foreground = choose_foreground(
        candidates, variant, depth, background, accent_lch
    )

    accent_slot = min(
        OKLCH_SLOTS,
        key=lambda slot: hue_delta(accent_lch[2], slot[1]),
    )[0]

    palette = {}
    sources = {}

    for name, reference_hue in OKLCH_SLOTS:
        if name == accent_slot:
            palette[name] = accent
            sources[name] = "accent"
            continue
        chosen = choose_slot(
            reference_hue, candidates, accent_lch, variant, depth
        )
        palette[name] = chosen["hex"]
        sources[name] = chosen["source"]

    if is_dark:
        palette["black"] = background
        palette["white"] = foreground
    else:
        palette["black"] = foreground
        palette["white"] = background

    # Push apart chromatic slots that collapsed onto each other. The accent is
    # never altered; the other slot moves toward its semantic reference hue.
    for _ in range(6):
        changed = False
        for first_index in range(len(SLOT_NAMES)):
            for second_index in range(first_index + 1, len(SLOT_NAMES)):
                first_name = SLOT_NAMES[first_index]
                second_name = SLOT_NAMES[second_index]
                if (
                    oklab_delta(palette[first_name], palette[second_name])
                    >= 0.075
                ):
                    continue

                if first_name == accent_slot:
                    target = second_name
                elif second_name == accent_slot:
                    target = first_name
                else:
                    target = second_name

                lightness, chroma, hue = hex_to_oklch(palette[target])
                hue = hue_blend(hue, REFERENCE_HUES[target], 0.5)
                if is_dark:
                    lightness = clamp(lightness + 0.025, 0.30, 0.85)
                else:
                    lightness = clamp(lightness - 0.025, 0.25, 0.70)
                chroma = clamp(chroma * 1.05, 0.045, 0.20)
                palette[target] = oklch_to_hex(lightness, chroma, hue)
                if "+separated" not in sources[target]:
                    sources[target] += "+separated"
                changed = True

        if not changed:
            break

    # WCAG contrast guard. The accent keeps its exact value in ui.accent even
    # if the ANSI copy has to be lifted for readability.
    for name in SLOT_NAMES:
        if contrast_ratio_hex(palette[name], background) >= 4.5:
            continue
        if name == accent_slot:
            # Keep the user's accent exact in the ANSI slot; the audit reports
            # the contrast shortfall instead of silently changing it.
            if "+low-contrast" not in sources[name]:
                sources[name] += "+low-contrast"
            continue
        adjusted = ensure_contrast(
            palette[name], background, 4.5, prefer_light=is_dark
        )
        palette[name] = adjusted
        sources[name] += "+contrast"

    return (
        palette,
        {
            "accent_slot": accent_slot,
            "background": background,
            "foreground": foreground,
            "sources": sources,
        },
    )


def emit_yaml(palette, accent, variant):
    order = [
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "magenta",
        "cyan",
        "white",
    ]
    lines = [
        "scheme:",
        '  system: "tinted8"',
        "  supports:",
        '    styling-spec: "0.2.0"',
        '  author: "ChromaLeon (generated)"',
        '  name: "ChromaLeon"',
        '  slug: "chromaleon"',
        '  description: "Generated from the current ChromaLeon wallpaper palette"',
        f'variant: "{variant}"',
        "palette:",
    ]
    for name in order:
        lines.append(f'  {name}: "{palette[name]}"')
    lines += [
        "ui:",
        f'  accent.normal: "{accent}"',
        "",
    ]
    return "\n".join(lines)


def str_to_bool(value):
    if isinstance(value, bool):
        return value
    return str(value).strip().lower() in ("1", "true", "yes", "on")


def print_audit(settings, surfaces, target, palette, meta, accent, variant, depth):
    print("ChromaLeon -> tinted8 audit")
    print(
        "settings: "
        f"variant={variant} depth={depth} target={target} "
        f"accent={accent} gnome_colors={settings['gnome_colors']} "
        f"tint_shell={settings['tint_shell']} tint_panel={settings['tint_panel']} "
        f"strength={settings['tint_strength']} darker={settings['darker']} "
        f"full_light={settings['full_light']}"
    )
    print("\nresolved shell surfaces:")
    for name in ("panel", "menu", "dialog"):
        if name not in surfaces:
            continue
        value = surfaces[name]
        lightness, chroma, hue = hex_to_oklch(value)
        print(
            f"  {name:7} {value}  "
            f"L={lightness:.3f} C={chroma:.3f} H={hue:5.1f}"
        )

    background = meta["background"]
    print(f"\nchosen surface ({target}): {background}")
    print(f"foreground: {meta['foreground']}")
    print(
        "foreground contrast: "
        f"{contrast_ratio_hex(meta['foreground'], background):.2f}"
    )

    print("\npalette:")
    order = [
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "magenta",
        "cyan",
        "white",
    ]
    for name in order:
        value = palette[name]
        lightness, chroma, hue = hex_to_oklch(value)
        ratio = contrast_ratio_hex(value, background)
        extra = f" dE_accent={oklab_delta(value, accent):.3f}"
        print(
            f"  {name:7} {value}  "
            f"L={lightness:.3f} C={chroma:.3f} H={hue:5.1f} "
            f"contrast={ratio:5.2f}{extra} "
            f"[{meta['sources'].get(name, 'anchor')}]"
        )

    print("\nwarnings:")
    warnings = []
    for name in SLOT_NAMES:
        ratio = contrast_ratio_hex(palette[name], background)
        if ratio < 4.5:
            if name == meta["accent_slot"]:
                warnings.append(
                    f"accent slot {name} is {ratio:.2f}:1 against the background "
                    "(kept exact; increase --depth for more contrast)"
                )
            else:
                warnings.append(
                    f"{name} is {ratio:.2f}:1 against the background"
                )
        if "synthetic" in meta["sources"].get(name, ""):
            warnings.append(
                f"{name} was synthesized (no suitable wallpaper hue)"
            )
    if oklab_delta(palette["red"], palette["yellow"]) < 0.075:
        warnings.append("red and yellow are still close in OKLab")
    if not meta["sources"].get(meta["accent_slot"], "").startswith("accent"):
        warnings.append("accent slot was not preserved exactly")
    if not warnings:
        warnings.append("none")
    for warning in warnings:
        print(f"  - {warning}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--accent", default="#785ef0", help="ChromaLeon accent hex"
    )
    parser.add_argument(
        "--variant",
        choices=("dark", "light"),
        default="dark",
        help="Scheme variant",
    )
    parser.add_argument(
        "--gnome-colors",
        type=str_to_bool,
        default=False,
        help="Use the GNOME accent instead of ChromaLeon's accent-color",
    )
    parser.add_argument(
        "--gnome-accent", default="", help="GNOME accent name or hex"
    )
    parser.add_argument(
        "--tint-shell",
        type=str_to_bool,
        default=True,
        help="ChromaLeon tint-shell",
    )
    parser.add_argument(
        "--tint-panel",
        type=str_to_bool,
        default=True,
        help="ChromaLeon tint-panel",
    )
    parser.add_argument(
        "--tint-strength",
        type=int,
        choices=range(4),
        default=1,
        help="ChromaLeon tinting-strength",
    )
    parser.add_argument(
        "--darker",
        type=str_to_bool,
        default=False,
        help="ChromaLeon darker",
    )
    parser.add_argument(
        "--full-light",
        type=str_to_bool,
        default=False,
        help="ChromaLeon full-light",
    )
    parser.add_argument(
        "--shell-css",
        default="",
        help="Generated ChromaLeon shell stylesheet for the active variant",
    )
    parser.add_argument(
        "--surface-target",
        choices=("auto", "panel", "menu"),
        default="auto",
        help="Which shell surface the scheme should track",
    )
    parser.add_argument(
        "--depth",
        type=int,
        choices=range(4),
        default=1,
        help="0 = exact surface match, 3 = pronounced separation",
    )
    parser.add_argument(
        "--pixels-b64",
        action="store_true",
        help="Read base64 RGB produced by chromaleon_pixels.js",
    )
    parser.add_argument(
        "--audit",
        action="store_true",
        help="Print an audit instead of the scheme",
    )
    args = parser.parse_args()

    accent = args.accent
    if not accent.startswith("#") or len(accent) != 7:
        accent = "#785ef0"

    gnome_accent = args.gnome_accent.strip().strip("'")
    if args.gnome_colors:
        if gnome_accent.startswith("#") and len(gnome_accent) == 7:
            accent = gnome_accent
        elif gnome_accent in GNOME_ACCENTS_HEX:
            accent = GNOME_ACCENTS_HEX[gnome_accent]
        # The generated CSS uses -st-accent-color, so the parser needs the
        # resolved hex, not the GNOME accent name.
        gnome_accent = accent
    else:
        gnome_accent = ""

    raw = sys.stdin.buffer.read()
    if args.pixels_b64 and raw:
        try:
            raw = base64.b64decode(raw, validate=False)
        except (ValueError, TypeError):
            print(
                "chromaleon_palette: invalid base64 pixels", file=sys.stderr
            )
            raw = b""

    colors = extract_colors(raw)
    if not colors:
        print(
            "chromaleon_palette: palette extraction empty, deriving from accent",
            file=sys.stderr,
        )

    candidates = annotate_colors(colors)

    css = read_text(args.shell_css)
    surfaces = {}
    if css:
        for role, selectors in (
            ("panel", ("#panel",)),
            ("menu", (".popup-menu-content", ".quick-toggle-menu")),
            ("dialog", (".modal-dialog",)),
        ):
            for selector in selectors:
                value = parse_css_surface(css, selector, accent, gnome_accent)
                if value:
                    surfaces[role] = value
                    break

    if "menu" not in surfaces and "dialog" in surfaces:
        surfaces["menu"] = surfaces["dialog"]

    for role in ("panel", "menu"):
        if role not in surfaces:
            surfaces[role] = fallback_surface(
                role,
                args.variant,
                accent,
                args.tint_shell,
                args.tint_panel,
                args.tint_strength,
                args.darker,
            )

    target = args.surface_target
    if target == "auto":
        if args.tint_shell:
            target = "panel" if args.tint_panel else "menu"
        else:
            # With shell tinting off the panel falls back to the theme's flat
            # black; the menu is the useful neutral app surface instead.
            target = "menu"

    surface = surfaces.get(target, surfaces["panel"])

    palette, meta = build_palette(
        candidates, accent, args.variant, surface, args.depth
    )

    if args.audit:
        print_audit(
            {
                "gnome_colors": args.gnome_colors,
                "tint_shell": args.tint_shell,
                "tint_panel": args.tint_panel,
                "tint_strength": args.tint_strength,
                "darker": args.darker,
                "full_light": args.full_light,
            },
            surfaces,
            target,
            palette,
            meta,
            accent,
            args.variant,
            args.depth,
        )
        return

    sys.stdout.write(emit_yaml(palette, accent, args.variant))


if __name__ == "__main__":
    main()
