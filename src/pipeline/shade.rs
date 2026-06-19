/// Generate 5 lightness-shifted shades from a base hex colour.
///
/// Returns `[_1, _2, _3, _4, _5]` where `_1` is the base.
/// Shifts: 0, +12%, +20%, -12%, -20% lightness (clamped to [0, 1]).
pub fn shades(hex: &str) -> [String; 5] {
    let (r, g, b) = parse_hex(hex);
    let (h, s, l) = rgb_to_hsl(r, g, b);

    let shifts: [f64; 5] = [0.0, 0.12, 0.20, -0.12, -0.20];
    shifts.map(|delta| {
        let nl = (l + delta).clamp(0.0, 1.0);
        let (nr, ng, nb) = hsl_to_rgb(h, s, nl);
        format!("{:02x}{:02x}{:02x}", nr, ng, nb)
    })
}

/// Parse a 6-char hex string (no #) into (r, g, b) in [0, 255].
fn parse_hex(hex: &str) -> (u8, u8, u8) {
    let hex = hex.trim_start_matches('#');
    let n = u32::from_str_radix(hex, 16).unwrap_or(0);
    (((n >> 16) & 0xFF) as u8, ((n >> 8) & 0xFF) as u8, (n & 0xFF) as u8)
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let rf = r as f64 / 255.0;
    let gf = g as f64 / 255.0;
    let bf = b as f64 / 255.0;

    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let delta = max - min;
    let l = (max + min) / 2.0;

    if delta == 0.0 {
        return (0.0, 0.0, l);
    }

    let s = delta / (1.0 - (2.0 * l - 1.0).abs());

    let h = if max == rf {
        60.0 * (((gf - bf) / delta) % 6.0)
    } else if max == gf {
        60.0 * ((bf - rf) / delta + 2.0)
    } else {
        60.0 * ((rf - gf) / delta + 4.0)
    };

    let h = if h < 0.0 { h + 360.0 } else { h };
    (h, s, l)
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;

    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    let to_u8 = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to_u8(r1), to_u8(g1), to_u8(b1))
}

/// Parse hex to (r, g, b) for use in rgba() expressions.
pub fn hex_to_rgb_tuple(hex: &str) -> (u8, u8, u8) {
    parse_hex(hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shades_differ() {
        let s = shades("83a598");
        assert_eq!(s[0], "83a598"); // base unchanged
        assert_ne!(s[0], s[1]);
        assert_ne!(s[0], s[3]);
    }

    #[test]
    fn shades_in_gamut() {
        // White edge case
        let s = shades("ffffff");
        for shade in &s {
            assert_eq!(shade.len(), 6);
        }
        // Black edge case
        let s = shades("000000");
        for shade in &s {
            assert_eq!(shade.len(), 6);
        }
    }
}
