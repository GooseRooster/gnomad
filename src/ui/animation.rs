use crate::schemes::types::Scheme;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};
use std::time::Instant;

const LOGOS_CONF: &str = include_str!("../../assets/misc/logos.conf");

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

const SCHEME_PIPELINE: &[&str] = &[
    "convert wallpaper",
    "apply tinty",
    "write gtk css",
    "write shell css",
    "reload shell",
];

/// Which visual effect to apply to the ASCII art each animation run.
#[derive(Clone, Copy)]
enum EffectKind {
    /// Oscillate hue by ±degrees at the given period (ms).
    HueShift { degrees: f32, period_ms: u64 },
    /// Pulse brightness (lightness) between base and base+amplitude.
    BrightnessPulse { base: f32, amplitude: f32, period_ms: u64 },
    /// Sweep columns visible left-to-right then right-to-left.
    SweepLR { period_ms: u64 },
    /// Sweep rows visible top-to-bottom then bottom-to-top.
    SweepTB { period_ms: u64 },
    /// Combine hue shift with brightness pulse.
    Combined { hue_degrees: f32, brightness_amp: f32, period_ms: u64 },
}

pub struct AnimationState {
    pub frame: usize,
    logos: Vec<String>,
    current_logo_idx: usize,
    effect: EffectKind,
    /// Base colour applied to art cells (Cyan by default, overridden per effect).
    base_hue: f32,
    /// Time the current animation started.
    anim_start: Instant,
}

impl AnimationState {
    pub fn new() -> Self {
        Self {
            frame: 0,
            logos: parse_logos(LOGOS_CONF),
            current_logo_idx: 0,
            effect: EffectKind::HueShift { degrees: 40.0, period_ms: 1800 },
            base_hue: 190.0, // Cyan-ish
            anim_start: Instant::now(),
        }
    }

    pub fn start_animation(&mut self) {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as usize;

        if !self.logos.is_empty() {
            self.current_logo_idx = seed % self.logos.len();
        }
        let period_base = 1400u64 + (seed % 8) as u64 * 100;
        self.effect = match seed % 5 {
            0 => EffectKind::HueShift {
                degrees: 30.0 + (seed % 60) as f32,
                period_ms: period_base,
            },
            1 => EffectKind::BrightnessPulse {
                base: 0.55,
                amplitude: 0.30,
                period_ms: period_base + 200,
            },
            2 => EffectKind::SweepLR { period_ms: period_base + 400 },
            3 => EffectKind::SweepTB { period_ms: period_base + 300 },
            _ => EffectKind::Combined {
                hue_degrees: 25.0 + (seed % 40) as f32,
                brightness_amp: 0.2,
                period_ms: period_base,
            },
        };
        // Randomise starting hue so each animation feels different
        self.base_hue = 160.0 + (seed % 80) as f32;
        self.anim_start = Instant::now();
        self.frame = 0;
    }

    pub fn tick(&mut self) {
        self.frame = (self.frame + 1) % SPINNER_FRAMES.len();
    }

    pub fn spinner(&self) -> &str {
        SPINNER_FRAMES[self.frame]
    }

    /// Elapsed ms since animation start, for effect phase computation.
    fn elapsed_ms(&self) -> u64 {
        self.anim_start.elapsed().as_millis() as u64
    }
}

fn parse_logos(input: &str) -> Vec<String> {
    let mut logos: Vec<String> = Vec::new();
    let mut current: Option<String> = None;

    for line in input.lines() {
        if let Some((_key, rest)) = line.split_once('=').filter(|(k, _)| k.starts_with("ascii_")) {
            if let Some(prev) = current.take() {
                logos.push(prev.trim_end().to_string());
            }
            current = Some(rest.to_string());
        } else if let Some(buf) = current.as_mut() {
            buf.push('\n');
            buf.push_str(line);
        }
    }
    if let Some(last) = current {
        logos.push(last.trim_end().to_string());
    }
    logos
}

pub fn render(f: &mut Frame, anim: &mut AnimationState, status: &str, scheme: Option<&Scheme>) {
    let area = f.area();

    // Full-screen clear + dark backdrop
    f.render_widget(Clear, area);
    f.render_widget(
        Block::default().style(Style::default().bg(Color::Rgb(14, 14, 20))),
        area,
    );

    // Palette strips top/bottom when scheme is known
    let (content_top, content_bottom) = if let Some(s) = scheme {
        if area.height > 4 {
            render_palette_strip(
                f,
                Rect { x: area.x, y: area.y, width: area.width, height: 1 },
                s,
            );
            render_palette_strip(
                f,
                Rect { x: area.x, y: area.y + area.height - 1, width: area.width, height: 1 },
                s,
            );
            (area.y + 1, area.y + area.height - 1)
        } else {
            (area.y, area.y + area.height)
        }
    } else {
        (area.y, area.y + area.height)
    };

    let available_height = content_bottom.saturating_sub(content_top);

    // Build status lines
    let spinner = anim.spinner();
    let current_step = pipeline_step(status);
    let status_lines = build_status_lines(spinner, status, current_step);
    let status_height = status_lines.len() as u16 + 1; // +1 gap

    // ASCII art
    let logo_text = if anim.logos.is_empty() {
        String::new()
    } else {
        anim.logos[anim.current_logo_idx].clone()
    };
    let art_lines: Vec<&str> = logo_text.lines().collect();
    let art_height = art_lines.len() as u16;
    let art_width = art_lines.iter().map(|l| l.len()).max().unwrap_or(0) as u16;

    // Centre the art + status block vertically
    let block_height = art_height + status_height;
    let vert_offset = available_height.saturating_sub(block_height) / 2;
    let art_y = content_top + vert_offset;
    let art_x = area.x + area.width.saturating_sub(art_width) / 2;

    let art_rect = Rect {
        x: art_x,
        y: art_y,
        width: art_width.min(area.width),
        height: art_height.min(content_bottom.saturating_sub(art_y)),
    };

    // Render ASCII art in base cyan — effects will recolour it afterwards
    if !logo_text.is_empty() {
        let base_color = hsl_to_color(anim.base_hue, 0.7, 0.6);
        let art_lines_styled: Vec<Line> = art_lines
            .iter()
            .map(|l| Line::from(Span::styled(*l, Style::default().fg(base_color))))
            .collect();
        f.render_widget(Paragraph::new(art_lines_styled), art_rect);

        // Apply effect by directly manipulating the ratatui buffer cells
        apply_effect(f, anim, art_rect);
    }

    // Status text below art
    let status_y = art_y + art_height + 1;
    if status_y < content_bottom {
        let status_width = status_lines
            .iter()
            .map(|l: &Line| l.width())
            .max()
            .unwrap_or(0) as u16;
        let status_x = area.x + area.width.saturating_sub(status_width) / 2;
        let status_rect = Rect {
            x: status_x,
            y: status_y,
            width: status_width.min(area.width),
            height: (status_lines.len() as u16).min(content_bottom.saturating_sub(status_y)),
        };
        f.render_widget(Paragraph::new(status_lines), status_rect);
    }
}

/// Apply the chosen effect to cells in `rect` by directly modifying the ratatui buffer.
fn apply_effect(f: &mut Frame, anim: &AnimationState, rect: Rect) {
    let elapsed = anim.elapsed_ms();
    let buf = f.buffer_mut();

    match anim.effect {
        EffectKind::HueShift { degrees, period_ms } => {
            let t = ping_pong_t(elapsed, period_ms);
            let hue_delta = (t - 0.5) * 2.0 * degrees;
            for y in rect.y..rect.y + rect.height {
                for x in rect.x..rect.x + rect.width {
                    shift_cell_hue(buf, x, y, hue_delta);
                }
            }
        }
        EffectKind::BrightnessPulse { base, amplitude, period_ms } => {
            let t = ping_pong_t(elapsed, period_ms);
            let lightness = base + amplitude * t;
            for y in rect.y..rect.y + rect.height {
                for x in rect.x..rect.x + rect.width {
                    set_cell_lightness(buf, x, y, lightness, anim.base_hue);
                }
            }
        }
        EffectKind::SweepLR { period_ms } => {
            let t = ping_pong_t(elapsed, period_ms);
            let visible_cols = (t * rect.width as f32) as u16;
            for y in rect.y..rect.y + rect.height {
                for x in rect.x..rect.x + rect.width {
                    let col = x - rect.x;
                    let alpha = if col <= visible_cols { 1.0 } else { 0.05 };
                    fade_cell_alpha(buf, x, y, alpha, anim.base_hue);
                }
            }
        }
        EffectKind::SweepTB { period_ms } => {
            let t = ping_pong_t(elapsed, period_ms);
            let visible_rows = (t * rect.height as f32) as u16;
            for y in rect.y..rect.y + rect.height {
                for x in rect.x..rect.x + rect.width {
                    let row = y - rect.y;
                    let alpha = if row <= visible_rows { 1.0 } else { 0.05 };
                    fade_cell_alpha(buf, x, y, alpha, anim.base_hue);
                }
            }
        }
        EffectKind::Combined { hue_degrees, brightness_amp, period_ms } => {
            let t = ping_pong_t(elapsed, period_ms);
            let hue_delta = (t - 0.5) * 2.0 * hue_degrees;
            let lightness = 0.45 + brightness_amp * t;
            for y in rect.y..rect.y + rect.height {
                for x in rect.x..rect.x + rect.width {
                    shift_cell_hue_and_lightness(buf, x, y, hue_delta, lightness);
                }
            }
        }
    }
}

/// Smooth 0→1→0 over `period_ms` using sine curve (no hard edges).
fn ping_pong_t(elapsed_ms: u64, period_ms: u64) -> f32 {
    let phase = (elapsed_ms % period_ms) as f32 / period_ms as f32;
    // sine goes 0→1→0 over one full cycle
    (std::f32::consts::PI * phase).sin()
}

fn shift_cell_hue(buf: &mut ratatui::buffer::Buffer, x: u16, y: u16, hue_delta: f32) {
    let pos = ratatui::layout::Position { x, y };
    if let Some(cell) = buf.cell_mut(pos) {
        if let Color::Rgb(r, g, b) = cell.fg {
            let (h, s, l) = rgb_to_hsl(r, g, b);
            if s > 0.01 {
                let new_color = hsl_to_color((h + hue_delta).rem_euclid(360.0), s, l);
                cell.set_fg(new_color);
            }
        }
    }
}

fn set_cell_lightness(
    buf: &mut ratatui::buffer::Buffer,
    x: u16,
    y: u16,
    lightness: f32,
    base_hue: f32,
) {
    let pos = ratatui::layout::Position { x, y };
    if let Some(cell) = buf.cell_mut(pos) {
        if let Color::Rgb(r, g, b) = cell.fg {
            let (h, s, _l) = rgb_to_hsl(r, g, b);
            let hue = if s < 0.01 { base_hue } else { h };
            let sat = if s < 0.01 { 0.7 } else { s };
            cell.set_fg(hsl_to_color(hue, sat, lightness.clamp(0.0, 1.0)));
        }
    }
}

fn fade_cell_alpha(
    buf: &mut ratatui::buffer::Buffer,
    x: u16,
    y: u16,
    alpha: f32,
    base_hue: f32,
) {
    let pos = ratatui::layout::Position { x, y };
    if let Some(cell) = buf.cell_mut(pos) {
        if let Color::Rgb(r, g, b) = cell.fg {
            let (h, s, l) = rgb_to_hsl(r, g, b);
            let hue = if s < 0.01 { base_hue } else { h };
            let sat = if s < 0.01 { 0.7 } else { s };
            let new_l = (l * alpha).clamp(0.0, 1.0);
            cell.set_fg(hsl_to_color(hue, sat, new_l));
        }
    }
}

fn shift_cell_hue_and_lightness(
    buf: &mut ratatui::buffer::Buffer,
    x: u16,
    y: u16,
    hue_delta: f32,
    lightness: f32,
) {
    let pos = ratatui::layout::Position { x, y };
    if let Some(cell) = buf.cell_mut(pos) {
        if let Color::Rgb(r, g, b) = cell.fg {
            let (h, s, _l) = rgb_to_hsl(r, g, b);
            if s > 0.01 {
                let new_color = hsl_to_color(
                    (h + hue_delta).rem_euclid(360.0),
                    s,
                    lightness.clamp(0.0, 1.0),
                );
                cell.set_fg(new_color);
            }
        }
    }
}

// ─── HSL ↔ RGB ────────────────────────────────────────────────────────────────

fn hsl_to_color(h: f32, s: f32, l: f32) -> Color {
    let (r, g, b) = hsl_to_rgb(h, s, l);
    Color::Rgb(r, g, b)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = if h < 60.0 {
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
    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let l = (max + min) / 2.0;
    if delta < 1e-6 {
        return (0.0, 0.0, l);
    }
    let s = delta / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * (((g - b) / delta) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    (h.rem_euclid(360.0), s, l)
}

// ─── Status display ───────────────────────────────────────────────────────────

fn build_status_lines<'a>(
    spinner: &'a str,
    status: &'a str,
    current_step: Option<usize>,
) -> Vec<Line<'a>> {
    let mut lines: Vec<Line> = Vec::new();

    if let Some(step) = current_step {
        for (i, label) in SCHEME_PIPELINE.iter().enumerate() {
            let (icon, style) = if i < step {
                ("✓", Style::default().fg(Color::Green))
            } else if i == step {
                (spinner, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            } else {
                ("○", Style::default().fg(Color::DarkGray))
            };
            lines.push(Line::from(Span::styled(format!("{icon}  {label}"), style)));
        }
    } else {
        let display = if status.is_empty() {
            "processing..."
        } else {
            status.trim_matches(|c| c == '[' || c == ']').trim()
        };
        lines.push(Line::from(vec![
            Span::styled(spinner, Style::default().fg(Color::Cyan)),
            Span::raw("  "),
            Span::styled(display, Style::default().fg(Color::White)),
        ]));
    }

    lines
}

fn pipeline_step(status: &str) -> Option<usize> {
    if status.contains("converting wallpaper") {
        Some(0)
    } else if status.contains("tinty") {
        Some(1)
    } else if status.contains("gtk css") {
        Some(2)
    } else if status.contains("shell css") {
        Some(3)
    } else if status.contains("reload") {
        Some(4)
    } else {
        None
    }
}

fn render_palette_strip(f: &mut Frame, area: Rect, scheme: &Scheme) {
    let colors: [&str; 16] = [
        &scheme.base08, &scheme.base09, &scheme.base0a, &scheme.base0b,
        &scheme.base0c, &scheme.base0d, &scheme.base0e, &scheme.base0f,
        &scheme.base00, &scheme.base01, &scheme.base02, &scheme.base03,
        &scheme.base04, &scheme.base05, &scheme.base06, &scheme.base07,
    ];

    let n = colors.len() as u16;
    let base_w = area.width / n;
    let remainder = area.width % n;

    let mut x = area.x;
    for (i, hex) in colors.iter().enumerate() {
        let w = base_w + if (i as u16) < remainder { 1 } else { 0 };
        if w == 0 {
            continue;
        }
        let swatch = Rect { x, y: area.y, width: w, height: area.height };
        f.render_widget(
            Block::default().style(Style::default().bg(hex_to_color(hex))),
            swatch,
        );
        x += w;
    }
}

fn hex_to_color(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return Color::Black;
    }
    let n = u32::from_str_radix(hex, 16).unwrap_or(0);
    Color::Rgb(
        ((n >> 16) & 0xFF) as u8,
        ((n >> 8) & 0xFF) as u8,
        (n & 0xFF) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_logos ───────────────────────────────────────────────────────────

    #[test]
    fn logos_conf_produces_six_logos() {
        let logos = parse_logos(LOGOS_CONF);
        assert_eq!(logos.len(), 6, "expected 6 logos from logos.conf");
    }

    #[test]
    fn logos_are_non_empty() {
        for (i, logo) in parse_logos(LOGOS_CONF).iter().enumerate() {
            assert!(!logo.trim().is_empty(), "logo {i} is empty");
        }
    }

    #[test]
    fn logos_contain_no_key_prefix() {
        for logo in parse_logos(LOGOS_CONF) {
            assert!(
                !logo.contains("ascii_"),
                "logo content contains raw key: {logo:?}"
            );
        }
    }

    #[test]
    fn parse_logos_custom_two_entries() {
        let input = "ascii_1=hello\nworld\nascii_2=foo\nbar\nbaz";
        let logos = parse_logos(input);
        assert_eq!(logos.len(), 2);
        assert_eq!(logos[0], "hello\nworld");
        assert_eq!(logos[1], "foo\nbar\nbaz");
    }

    #[test]
    fn parse_logos_trailing_blank_lines_trimmed() {
        let input = "ascii_1=line1\n\n\n";
        let logos = parse_logos(input);
        assert_eq!(logos.len(), 1);
        assert_eq!(logos[0], "line1", "trailing blank lines should be trimmed");
    }

    // ── hex_to_color ──────────────────────────────────────────────────────────

    #[test]
    fn hex_to_color_pure_red() {
        assert_eq!(hex_to_color("ff0000"), Color::Rgb(255, 0, 0));
    }

    #[test]
    fn hex_to_color_accepts_hash_prefix() {
        assert_eq!(hex_to_color("#00ff00"), Color::Rgb(0, 255, 0));
    }

    #[test]
    fn hex_to_color_pure_blue() {
        assert_eq!(hex_to_color("0000ff"), Color::Rgb(0, 0, 255));
    }

    #[test]
    fn hex_to_color_invalid_returns_black() {
        assert_eq!(hex_to_color("zzz"), Color::Black);
        assert_eq!(hex_to_color(""), Color::Black);
        assert_eq!(hex_to_color("12345"), Color::Black); // 5 chars, not 6
    }

    // ── hsl_to_rgb / rgb_to_hsl ───────────────────────────────────────────────

    #[test]
    fn hsl_to_rgb_primary_red() {
        assert_eq!(hsl_to_rgb(0.0, 1.0, 0.5), (255, 0, 0));
    }

    #[test]
    fn hsl_to_rgb_primary_green() {
        assert_eq!(hsl_to_rgb(120.0, 1.0, 0.5), (0, 255, 0));
    }

    #[test]
    fn hsl_to_rgb_primary_blue() {
        assert_eq!(hsl_to_rgb(240.0, 1.0, 0.5), (0, 0, 255));
    }

    #[test]
    fn hsl_to_rgb_white() {
        assert_eq!(hsl_to_rgb(0.0, 0.0, 1.0), (255, 255, 255));
    }

    #[test]
    fn hsl_to_rgb_black() {
        assert_eq!(hsl_to_rgb(0.0, 0.0, 0.0), (0, 0, 0));
    }

    #[test]
    fn rgb_to_hsl_roundtrip() {
        // Arbitrary non-primary colour; we check that converting both ways
        // stays within ±2 per channel (rounding in u8 conversion).
        let cases: &[(u8, u8, u8)] = &[
            (180, 90, 30),
            (10, 200, 150),
            (255, 128, 0),
            (64, 64, 192),
        ];
        for &(r, g, b) in cases {
            let (h, s, l) = rgb_to_hsl(r, g, b);
            let (r2, g2, b2) = hsl_to_rgb(h, s, l);
            assert!(
                r.abs_diff(r2) <= 2 && g.abs_diff(g2) <= 2 && b.abs_diff(b2) <= 2,
                "roundtrip failed for ({r},{g},{b}) → hsl({h:.1},{s:.3},{l:.3}) → ({r2},{g2},{b2})"
            );
        }
    }

    #[test]
    fn rgb_to_hsl_achromatic_has_zero_saturation() {
        let (_, s, _) = rgb_to_hsl(128, 128, 128);
        assert!(s < 1e-5, "grey should have s ≈ 0, got {s}");
    }

    // ── ping_pong_t ───────────────────────────────────────────────────────────

    #[test]
    fn ping_pong_at_start_is_zero() {
        assert!(
            ping_pong_t(0, 1000).abs() < 1e-5,
            "t at elapsed=0 should be ~0"
        );
    }

    #[test]
    fn ping_pong_at_half_period_is_one() {
        let t = ping_pong_t(500, 1000);
        assert!(
            (t - 1.0).abs() < 1e-5,
            "t at half period should be ~1.0, got {t}"
        );
    }

    #[test]
    fn ping_pong_at_full_period_is_zero() {
        let t = ping_pong_t(1000, 1000);
        assert!(t.abs() < 1e-5, "t at full period should be ~0, got {t}");
    }

    #[test]
    fn ping_pong_wraps_at_multiple_periods() {
        // Two full periods should look the same as zero
        let t = ping_pong_t(2000, 1000);
        assert!(t.abs() < 1e-5, "t at 2× period should be ~0, got {t}");
    }

    // ── pipeline_step ─────────────────────────────────────────────────────────

    #[test]
    fn pipeline_step_maps_all_steps() {
        let cases = [
            ("[ converting wallpaper... ]", Some(0)),
            ("[ apply tinty ]",             Some(1)),
            ("[ write gtk css ]",           Some(2)),
            ("[ write shell css ]",         Some(3)),
            ("[ reload gnome shell ]",      Some(4)),
        ];
        for (status, expected) in cases {
            assert_eq!(
                pipeline_step(status),
                expected,
                "wrong step for {status:?}"
            );
        }
    }

    #[test]
    fn pipeline_step_unknown_status_is_none() {
        assert_eq!(pipeline_step(""), None);
        assert_eq!(pipeline_step("[ starting... ]"), None);
        assert_eq!(pipeline_step("some random text"), None);
    }
}
