//! OKLab/OKLCH colour math used by gowall palette tuning.
//!
//! Only the gowall pipeline uses these helpers. The GTK CSS derivation in
//! `pipeline::shade` deliberately stays in HSL/sRGB because it emulates
//! GTK's own `shade()`/`mix()` functions.

use crate::config::GowallTuning;

const NEUTRAL_CHROMA: f32 = 0.02;

#[derive(Debug, Clone, Copy)]
pub struct Oklch {
    pub l: f32,
    pub c: f32,
    pub h: f32,
}

pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn rgb_to_oklab(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let l = 0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_84 * g + 0.629_978_7 * b;

    let l_ = l.cbrt();
    let m_ = m.cbrt();
    let s_ = s.cbrt();

    (
        0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_,
        1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_,
        0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_,
    )
}

fn oklab_to_rgb(l: f32, a: f32, b: f32) -> (f32, f32, f32) {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;

    let l3 = l_ * l_ * l_;
    let m3 = m_ * m_ * m_;
    let s3 = s_ * s_ * s_;

    (
        4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_93 * s3,
        -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_4 * s3,
        -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
    )
}

pub fn hex_to_oklch(hex: &str) -> Oklch {
    let n = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0);
    let r = srgb_to_linear(((n >> 16) & 0xFF) as f32 / 255.0);
    let g = srgb_to_linear(((n >> 8) & 0xFF) as f32 / 255.0);
    let b = srgb_to_linear((n & 0xFF) as f32 / 255.0);

    let (l, a, b_) = rgb_to_oklab(r, g, b);
    Oklch {
        l,
        c: (a * a + b_ * b_).sqrt(),
        h: b_.atan2(a).to_degrees().rem_euclid(360.0),
    }
}

pub fn oklch_to_hex(color: Oklch) -> String {
    if color.l <= 0.0 {
        return "000000".to_string();
    }
    if color.l >= 1.0 {
        return "ffffff".to_string();
    }
    let l = color.l.clamp(0.0, 1.0);
    let c = color.c.clamp(0.0, 0.5);
    let h = color.h.to_radians();
    let (lr, lg, lb) = oklab_to_rgb(l, c * h.cos(), c * h.sin());

    let to_u8 = |v: f32| {
        (linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    format!("{:02x}{:02x}{:02x}", to_u8(lr), to_u8(lg), to_u8(lb))
}

/// Apply hue/chroma/lightness/contrast/gamma tuning to one hex colour.
/// Returns a bare 6-character lowercase hex string.
pub fn tune_hex(hex: &str, tuning: &GowallTuning) -> String {
    let mut lch = hex_to_oklch(hex);
    let neutral = lch.c < NEUTRAL_CHROMA;

    if !(tuning.preserve_neutrals && neutral) {
        lch.h = (lch.h + tuning.hue_shift as f32).rem_euclid(360.0);
        lch.c *= tuning.saturation as f32 / 100.0;
    }

    lch.l *= tuning.lightness as f32 / 100.0;

    let contrast = tuning.contrast as f32 / 100.0;
    lch.l = (lch.l - 0.5) * (1.0 + contrast) + 0.5;

    let gamma = tuning.gamma as f32 / 100.0;
    lch.l = lch.l.clamp(0.0, 1.0).powf(1.0 / gamma);

    oklch_to_hex(lch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(hex: &str) -> (u8, u8, u8) {
        let n = u32::from_str_radix(hex, 16).unwrap();
        (
            ((n >> 16) & 0xFF) as u8,
            ((n >> 8) & 0xFF) as u8,
            (n & 0xFF) as u8,
        )
    }

    #[test]
    fn identity_tuning_round_trips() {
        let tuning = GowallTuning::default();
        for hex in ["000000", "ffffff", "83a598", "fb4934", "1d2021"] {
            let out = tune_hex(hex, &tuning);
            let (r1, g1, b1) = rgb(hex);
            let (r2, g2, b2) = rgb(&out);
            assert!((r1 as i32 - r2 as i32).abs() <= 1, "{hex} -> {out}");
            assert!((g1 as i32 - g2 as i32).abs() <= 1, "{hex} -> {out}");
            assert!((b1 as i32 - b2 as i32).abs() <= 1, "{hex} -> {out}");
        }
    }

    #[test]
    fn neutral_colours_stay_neutral_when_preserved() {
        let tuning = GowallTuning {
            hue_shift: 120,
            saturation: 180,
            ..GowallTuning::default()
        };
        let out = tune_hex("282828", &tuning);
        let (r, g, b) = rgb(&out);
        // Allow tiny rounding drift, but no visible tint.
        assert!((r as i32 - g as i32).abs() <= 2);
        assert!((g as i32 - b as i32).abs() <= 2);
    }

    #[test]
    fn zero_saturation_greys_chromatic_colour() {
        let tuning = GowallTuning {
            saturation: 0,
            ..GowallTuning::default()
        };
        let (r, g, b) = rgb(&tune_hex("83a598", &tuning));
        assert!((r as i32 - g as i32).abs() <= 2);
        assert!((g as i32 - b as i32).abs() <= 2);
    }

    #[test]
    fn zero_lightness_is_black() {
        let tuning = GowallTuning {
            lightness: 0,
            ..GowallTuning::default()
        };
        assert_eq!(tune_hex("83a598", &tuning), "000000");
    }

    #[test]
    fn fingerprint_changes_with_parameters() {
        let a = GowallTuning::default();
        let b = GowallTuning {
            hue_shift: 10,
            ..GowallTuning::default()
        };
        assert_ne!(a.fingerprint(), b.fingerprint());
        assert!(a.is_identity());
        assert!(!b.is_identity());
    }
}
