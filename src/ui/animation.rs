use crate::schemes::types::Scheme;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};
use std::time::Instant;
use unicode_width::UnicodeWidthStr;

const LOGOS_CONF: &str = include_str!("../../assets/misc/logos.conf");
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// Step label tables per task type.
const SCHEME_STEPS: &[&str] =
    &["convert wallpaper", "apply tinty", "write gtk css", "write shell css", "reload shell"];
const SCHEME_STEPS_NO_WALLPAPER: &[&str] =
    &["apply tinty", "write gtk css", "write shell css", "reload shell"];
const WALLPAPER_STEPS: &[&str] = &["prepare wallpaper", "set wallpaper"];

// ── Task kind ─────────────────────────────────────────────────────────────────

/// Which pipeline is running — drives which progress steps are displayed.
#[derive(Clone, Copy, PartialEq)]
pub enum TaskKind {
    ApplyScheme,
    ApplySchemeNoWallpaper,
    ApplyWallpaper,
    BatchConvert,
    UpdateSchemes,
}

// ── Effect kind ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum EffectKind {
    HueShift { degrees: f32, period_ms: u64 },
    BrightnessPulse { base: f32, amplitude: f32, period_ms: u64 },
    SweepLR { period_ms: u64 },
    SweepTB { period_ms: u64 },
    Combined { hue_degrees: f32, brightness_amp: f32, period_ms: u64 },
}

// ── Palette cross-fade ────────────────────────────────────────────────────────

struct PaletteTransition {
    old_bg: (u8, u8, u8),
    new_bg: (u8, u8, u8),
    old_hue: f32,
    new_hue: f32,
}

// ── AnimationState ────────────────────────────────────────────────────────────

pub struct AnimationState {
    pub frame: usize,
    logos: Vec<String>,
    current_logo_idx: usize,
    effect: EffectKind,
    anim_start: Instant,
    task_kind: TaskKind,
    // Scheme-derived colours (settled values, not mid-transition).
    base_hue: f32,
    base_bg: Color,
    // Palette strip data for the "target" scheme.
    palette_scheme: Option<Scheme>,
    // Non-None only when the scheme itself is changing.
    transition: Option<PaletteTransition>,
}

impl AnimationState {
    pub fn new() -> Self {
        Self {
            frame: 0,
            logos: parse_logos(LOGOS_CONF),
            current_logo_idx: 0,
            effect: EffectKind::HueShift { degrees: 40.0, period_ms: 1800 },
            anim_start: Instant::now(),
            task_kind: TaskKind::UpdateSchemes,
            base_hue: 190.0,
            base_bg: Color::Rgb(14, 14, 20),
            palette_scheme: None,
            transition: None,
        }
    }

    /// Call this immediately before spawning a pipeline task.
    ///
    /// `old_scheme` = what is currently applied; `new_scheme` = what will be
    /// applied.  When they differ (and kind == ApplyScheme) the colours will
    /// cross-fade over ~4 s.
    pub fn start_animation(
        &mut self,
        kind: TaskKind,
        old_scheme: Option<&Scheme>,
        new_scheme: Option<&Scheme>,
    ) {
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

        self.task_kind = kind;
        self.anim_start = Instant::now();
        self.frame = 0;

        // Derive hues and palette from the schemes.
        let old_hue = old_scheme.map(scheme_accent_hue).unwrap_or(190.0);
        let new_hue = new_scheme.map(scheme_accent_hue).unwrap_or(old_hue);
        let target = new_scheme.or(old_scheme);

        self.palette_scheme = target.cloned();
        self.base_hue = new_hue;
        self.base_bg = target
            .map(|s| hex_to_color(&s.base00))
            .unwrap_or(Color::Rgb(14, 14, 20));

        // Only cross-fade when the scheme is actually changing.
        let is_scheme_change = matches!(kind, TaskKind::ApplyScheme | TaskKind::ApplySchemeNoWallpaper)
            && old_scheme.is_some()
            && old_scheme.map(|s| s.slug.as_str()) != new_scheme.map(|s| s.slug.as_str());

        self.transition = if is_scheme_change {
            let old = old_scheme.unwrap();
            let new = new_scheme.unwrap();
            Some(PaletteTransition {
                old_bg: color_to_rgb(hex_to_color(&old.base00)),
                new_bg: color_to_rgb(hex_to_color(&new.base00)),
                old_hue,
                new_hue,
            })
        } else {
            None
        };
    }

    pub fn tick(&mut self) {
        self.frame = (self.frame + 1) % SPINNER_FRAMES.len();
    }

    pub fn spinner(&self) -> &str {
        SPINNER_FRAMES[self.frame]
    }

    fn elapsed_ms(&self) -> u64 {
        self.anim_start.elapsed().as_millis() as u64
    }

    /// Hue to use for art colouring — interpolates during a palette transition.
    fn current_hue(&self) -> f32 {
        match &self.transition {
            Some(tr) => lerp_hue(tr.old_hue, tr.new_hue, transition_t(self.elapsed_ms())),
            None => self.base_hue,
        }
    }

    /// Background colour — interpolates during a palette transition.
    fn current_bg(&self) -> Color {
        match &self.transition {
            Some(tr) => lerp_color(tr.old_bg, tr.new_bg, transition_t(self.elapsed_ms())),
            None => self.base_bg,
        }
    }
}

// ── Logo parsing ──────────────────────────────────────────────────────────────

fn parse_logos(input: &str) -> Vec<String> {
    let mut logos: Vec<String> = Vec::new();
    let mut current: Option<String> = None;

    for line in input.lines() {
        if let Some((_key, rest)) =
            line.split_once('=').filter(|(k, _)| k.starts_with("ascii_"))
        {
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

// ── Render ────────────────────────────────────────────────────────────────────

pub fn render(f: &mut Frame, anim: &mut AnimationState, status: &str) {
    let area = f.area();
    let bg = anim.current_bg();

    f.render_widget(Clear, area);
    f.render_widget(Block::default().style(Style::default().bg(bg)), area);

    // Palette strips — uses the target/new scheme's colours.
    let (content_top, content_bottom) = match anim.palette_scheme.as_ref() {
        Some(s) if area.height > 4 => {
            render_palette_strip(
                f,
                Rect { x: area.x, y: area.y, width: area.width, height: 1 },
                s,
            );
            render_palette_strip(
                f,
                Rect {
                    x: area.x,
                    y: area.y + area.height - 1,
                    width: area.width,
                    height: 1,
                },
                s,
            );
            (area.y + 1, area.y + area.height - 1)
        }
        _ => (area.y, area.y + area.height),
    };

    let available_height = content_bottom.saturating_sub(content_top);

    // Status lines — task-kind-aware step list or generic spinner.
    let spinner = anim.spinner();
    let step_labels = task_steps(anim.task_kind);
    let current_step = task_step(status, anim.task_kind);
    let status_lines = build_status_lines(spinner, status, current_step, step_labels);
    let status_height = status_lines.len() as u16 + 1; // +1 gap

    // ASCII art.
    let logo_text = if anim.logos.is_empty() {
        String::new()
    } else {
        anim.logos[anim.current_logo_idx].clone()
    };
    let art_lines: Vec<&str> = logo_text.lines().collect();
    let art_height = art_lines.len() as u16;
    // UnicodeWidthStr::width gives display columns, not byte length —
    // required for correct centering of logos that use block chars (░▒▓█).
    let art_width = art_lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16;

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

    if !logo_text.is_empty() {
        let hue = anim.current_hue();
        let base_color = hsl_to_color(hue, 0.7, 0.6);
        let art_styled: Vec<Line> = art_lines
            .iter()
            .map(|l| Line::from(Span::styled(*l, Style::default().fg(base_color))))
            .collect();
        f.render_widget(Paragraph::new(art_styled), art_rect);
        apply_effect(f, anim, art_rect, hue);
    }

    // Status text centred below the art.
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

    // Dim cancel hint anchored to the bottom of the content area.
    let hint_y = content_bottom.saturating_sub(1);
    if hint_y > area.y {
        const HINT: &str = "[Esc] Cancel";
        let hint_x = area.x + area.width.saturating_sub(HINT.len() as u16) / 2;
        f.render_widget(
            Paragraph::new(Span::styled(HINT, Style::default().fg(Color::DarkGray))),
            Rect { x: hint_x, y: hint_y, width: HINT.len() as u16, height: 1 },
        );
    }
}

// ── Step display helpers ──────────────────────────────────────────────────────

fn task_steps(kind: TaskKind) -> &'static [&'static str] {
    match kind {
        TaskKind::ApplyScheme => SCHEME_STEPS,
        TaskKind::ApplySchemeNoWallpaper => SCHEME_STEPS_NO_WALLPAPER,
        TaskKind::ApplyWallpaper => WALLPAPER_STEPS,
        TaskKind::BatchConvert | TaskKind::UpdateSchemes => &[],
    }
}

fn task_step(status: &str, kind: TaskKind) -> Option<usize> {
    match kind {
        TaskKind::ApplyScheme => {
            if status.contains("converting wallpaper") { Some(0) }
            else if status.contains("tinty") { Some(1) }
            else if status.contains("gtk css") { Some(2) }
            else if status.contains("shell css") { Some(3) }
            else if status.contains("reload") { Some(4) }
            else { None }
        }
        TaskKind::ApplySchemeNoWallpaper => {
            if status.contains("tinty") { Some(0) }
            else if status.contains("gtk css") { Some(1) }
            else if status.contains("shell css") { Some(2) }
            else if status.contains("reload") { Some(3) }
            else { None }
        }
        TaskKind::ApplyWallpaper => {
            // Both the "cached" and "converting" paths map to step 0 (prepare).
            if status.contains("converting wallpaper") || status.contains("applying wallpaper") {
                Some(0)
            } else if status.contains("setting wallpaper") {
                Some(1)
            } else {
                None
            }
        }
        TaskKind::BatchConvert | TaskKind::UpdateSchemes => None,
    }
}

fn build_status_lines<'a>(
    spinner: &'a str,
    status: &'a str,
    current_step: Option<usize>,
    step_labels: &'static [&'static str],
) -> Vec<Line<'a>> {
    let mut lines: Vec<Line> = Vec::new();

    if let Some(step) = current_step {
        for (i, label) in step_labels.iter().enumerate() {
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

// ── Effect application ────────────────────────────────────────────────────────

fn apply_effect(f: &mut Frame, anim: &AnimationState, rect: Rect, hue: f32) {
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
                    set_cell_lightness(buf, x, y, lightness, hue);
                }
            }
        }
        EffectKind::SweepLR { period_ms } => {
            let t = ping_pong_t(elapsed, period_ms);
            let visible_cols = (t * rect.width as f32) as u16;
            for y in rect.y..rect.y + rect.height {
                for x in rect.x..rect.x + rect.width {
                    let alpha = if x - rect.x <= visible_cols { 1.0 } else { 0.05 };
                    fade_cell_alpha(buf, x, y, alpha, hue);
                }
            }
        }
        EffectKind::SweepTB { period_ms } => {
            let t = ping_pong_t(elapsed, period_ms);
            let visible_rows = (t * rect.height as f32) as u16;
            for y in rect.y..rect.y + rect.height {
                for x in rect.x..rect.x + rect.width {
                    let alpha = if y - rect.y <= visible_rows { 1.0 } else { 0.05 };
                    fade_cell_alpha(buf, x, y, alpha, hue);
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

/// 0→1→0 smooth sine envelope over `period_ms`.
fn ping_pong_t(elapsed_ms: u64, period_ms: u64) -> f32 {
    let phase = (elapsed_ms % period_ms) as f32 / period_ms as f32;
    (std::f32::consts::PI * phase).sin()
}

/// Linear 0→1 over 4 s, clamped.  Used for palette cross-fades.
fn transition_t(elapsed_ms: u64) -> f32 {
    (elapsed_ms as f32 / 4000.0).min(1.0)
}

// ── Cell colour manipulation ──────────────────────────────────────────────────

fn shift_cell_hue(buf: &mut ratatui::buffer::Buffer, x: u16, y: u16, hue_delta: f32) {
    let pos = ratatui::layout::Position { x, y };
    if let Some(cell) = buf.cell_mut(pos) {
        if let Color::Rgb(r, g, b) = cell.fg {
            let (h, s, l) = rgb_to_hsl(r, g, b);
            if s > 0.01 {
                cell.set_fg(hsl_to_color((h + hue_delta).rem_euclid(360.0), s, l));
            }
        }
    }
}

fn set_cell_lightness(
    buf: &mut ratatui::buffer::Buffer,
    x: u16, y: u16,
    lightness: f32,
    fallback_hue: f32,
) {
    let pos = ratatui::layout::Position { x, y };
    if let Some(cell) = buf.cell_mut(pos) {
        if let Color::Rgb(r, g, b) = cell.fg {
            let (h, s, _) = rgb_to_hsl(r, g, b);
            let hue = if s < 0.01 { fallback_hue } else { h };
            let sat = if s < 0.01 { 0.7 } else { s };
            cell.set_fg(hsl_to_color(hue, sat, lightness.clamp(0.0, 1.0)));
        }
    }
}

fn fade_cell_alpha(
    buf: &mut ratatui::buffer::Buffer,
    x: u16, y: u16,
    alpha: f32,
    fallback_hue: f32,
) {
    let pos = ratatui::layout::Position { x, y };
    if let Some(cell) = buf.cell_mut(pos) {
        if let Color::Rgb(r, g, b) = cell.fg {
            let (h, s, l) = rgb_to_hsl(r, g, b);
            let hue = if s < 0.01 { fallback_hue } else { h };
            let sat = if s < 0.01 { 0.7 } else { s };
            cell.set_fg(hsl_to_color(hue, sat, (l * alpha).clamp(0.0, 1.0)));
        }
    }
}

fn shift_cell_hue_and_lightness(
    buf: &mut ratatui::buffer::Buffer,
    x: u16, y: u16,
    hue_delta: f32,
    lightness: f32,
) {
    let pos = ratatui::layout::Position { x, y };
    if let Some(cell) = buf.cell_mut(pos) {
        if let Color::Rgb(r, g, b) = cell.fg {
            let (h, s, _) = rgb_to_hsl(r, g, b);
            if s > 0.01 {
                cell.set_fg(hsl_to_color(
                    (h + hue_delta).rem_euclid(360.0),
                    s,
                    lightness.clamp(0.0, 1.0),
                ));
            }
        }
    }
}

// ── Colour math ───────────────────────────────────────────────────────────────

fn hsl_to_color(h: f32, s: f32, l: f32) -> Color {
    let (r, g, b) = hsl_to_rgb(h, s, l);
    Color::Rgb(r, g, b)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = if h < 60.0 { (c, x, 0.0) }
        else if h < 120.0 { (x, c, 0.0) }
        else if h < 180.0 { (0.0, c, x) }
        else if h < 240.0 { (0.0, x, c) }
        else if h < 300.0 { (x, 0.0, c) }
        else { (c, 0.0, x) };
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
    if delta < 1e-6 { return (0.0, 0.0, l); }
    let s = delta / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r { 60.0 * (((g - b) / delta) % 6.0) }
        else if max == g { 60.0 * ((b - r) / delta + 2.0) }
        else { 60.0 * ((r - g) / delta + 4.0) };
    (h.rem_euclid(360.0), s, l)
}

fn hex_to_color(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 { return Color::Black; }
    let n = u32::from_str_radix(hex, 16).unwrap_or(0);
    Color::Rgb(((n >> 16) & 0xFF) as u8, ((n >> 8) & 0xFF) as u8, (n & 0xFF) as u8)
}

fn color_to_rgb(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (14, 14, 20),
    }
}

fn lerp_color(old: (u8, u8, u8), new: (u8, u8, u8), t: f32) -> Color {
    Color::Rgb(
        (old.0 as f32 + (new.0 as f32 - old.0 as f32) * t) as u8,
        (old.1 as f32 + (new.1 as f32 - old.1 as f32) * t) as u8,
        (old.2 as f32 + (new.2 as f32 - old.2 as f32) * t) as u8,
    )
}

fn lerp_hue(a: f32, b: f32, t: f32) -> f32 {
    let mut delta = b - a;
    if delta > 180.0 { delta -= 360.0; }
    if delta < -180.0 { delta += 360.0; }
    (a + delta * t).rem_euclid(360.0)
}

/// Derive a representative hue from a scheme's accent colours.
/// Tries base0d (blue), base0e (purple), base08 (red) in order.
fn scheme_accent_hue(s: &Scheme) -> f32 {
    for hex in [&s.base0d, &s.base0e, &s.base08] {
        if let Color::Rgb(r, g, b) = hex_to_color(hex) {
            let (h, sat, _) = rgb_to_hsl(r, g, b);
            if sat > 0.15 { return h; }
        }
    }
    190.0 // fallback: cyan
}

// ── Palette strip ─────────────────────────────────────────────────────────────

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
        if w == 0 { continue; }
        f.render_widget(
            Block::default().style(Style::default().bg(hex_to_color(hex))),
            Rect { x, y: area.y, width: w, height: area.height },
        );
        x += w;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_logos ───────────────────────────────────────────────────────────

    #[test]
    fn logos_conf_produces_six_logos() {
        assert_eq!(parse_logos(LOGOS_CONF).len(), 6);
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
            assert!(!logo.contains("ascii_"), "logo content leaks raw key: {logo:?}");
        }
    }

    #[test]
    fn parse_logos_custom_two_entries() {
        let logos = parse_logos("ascii_1=hello\nworld\nascii_2=foo\nbar\nbaz");
        assert_eq!(logos.len(), 2);
        assert_eq!(logos[0], "hello\nworld");
        assert_eq!(logos[1], "foo\nbar\nbaz");
    }

    #[test]
    fn parse_logos_trailing_blank_lines_trimmed() {
        let logos = parse_logos("ascii_1=line1\n\n\n");
        assert_eq!(logos.len(), 1);
        assert_eq!(logos[0], "line1");
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
    fn hex_to_color_invalid_returns_black() {
        assert_eq!(hex_to_color("zzz"), Color::Black);
        assert_eq!(hex_to_color(""), Color::Black);
        assert_eq!(hex_to_color("12345"), Color::Black);
    }

    // ── hsl_to_rgb / rgb_to_hsl ───────────────────────────────────────────────

    #[test]
    fn hsl_primary_colours() {
        assert_eq!(hsl_to_rgb(0.0, 1.0, 0.5), (255, 0, 0));
        assert_eq!(hsl_to_rgb(120.0, 1.0, 0.5), (0, 255, 0));
        assert_eq!(hsl_to_rgb(240.0, 1.0, 0.5), (0, 0, 255));
        assert_eq!(hsl_to_rgb(0.0, 0.0, 1.0), (255, 255, 255));
        assert_eq!(hsl_to_rgb(0.0, 0.0, 0.0), (0, 0, 0));
    }

    #[test]
    fn rgb_to_hsl_roundtrip() {
        for &(r, g, b) in &[(180u8, 90u8, 30u8), (10, 200, 150), (255, 128, 0), (64, 64, 192)] {
            let (h, s, l) = rgb_to_hsl(r, g, b);
            let (r2, g2, b2) = hsl_to_rgb(h, s, l);
            assert!(
                r.abs_diff(r2) <= 2 && g.abs_diff(g2) <= 2 && b.abs_diff(b2) <= 2,
                "roundtrip ({r},{g},{b}) → ({r2},{g2},{b2})"
            );
        }
    }

    #[test]
    fn rgb_to_hsl_achromatic_has_zero_saturation() {
        let (_, s, _) = rgb_to_hsl(128, 128, 128);
        assert!(s < 1e-5, "grey should have s ≈ 0, got {s}");
    }

    // ── lerp helpers ──────────────────────────────────────────────────────────

    #[test]
    fn lerp_color_at_endpoints() {
        let black = (0u8, 0u8, 0u8);
        let white = (255u8, 255u8, 255u8);
        assert_eq!(lerp_color(black, white, 0.0), Color::Rgb(0, 0, 0));
        assert_eq!(lerp_color(black, white, 1.0), Color::Rgb(255, 255, 255));
    }

    #[test]
    fn lerp_hue_takes_shortest_arc() {
        // 350° → 10° should go forward 20°, not backward 340°.
        let mid = lerp_hue(350.0, 10.0, 0.5);
        assert!((mid - 0.0).abs() < 1.0, "expected ~0°, got {mid}");
    }

    // ── transition_t ──────────────────────────────────────────────────────────

    #[test]
    fn transition_t_clamps_at_one() {
        assert_eq!(transition_t(4000), 1.0);
        assert_eq!(transition_t(9999), 1.0);
    }

    #[test]
    fn transition_t_at_half_is_point_five() {
        assert!((transition_t(2000) - 0.5).abs() < 1e-5);
    }

    // ── ping_pong_t ───────────────────────────────────────────────────────────

    #[test]
    fn ping_pong_boundaries() {
        assert!(ping_pong_t(0, 1000).abs() < 1e-5);
        assert!((ping_pong_t(500, 1000) - 1.0).abs() < 1e-5);
        assert!(ping_pong_t(1000, 1000).abs() < 1e-5);
        assert!(ping_pong_t(2000, 1000).abs() < 1e-5); // wraps
    }

    // ── task_step ─────────────────────────────────────────────────────────────

    #[test]
    fn task_step_scheme_maps_all_steps() {
        let cases = [
            ("[ converting wallpaper... ]", 0),
            ("[ applying tinty scheme... ]", 1),
            ("[ writing gtk css... ]", 2),
            ("[ writing shell css... ]", 3),
            ("[ reloading shell... ]", 4),
        ];
        for (status, expected) in cases {
            assert_eq!(
                task_step(status, TaskKind::ApplyScheme),
                Some(expected),
                "wrong step for {status:?}"
            );
        }
    }

    #[test]
    fn task_step_wallpaper_maps_both_prepare_variants() {
        assert_eq!(task_step("[ converting wallpaper... ]", TaskKind::ApplyWallpaper), Some(0));
        assert_eq!(task_step("[ applying wallpaper (cached)... ]", TaskKind::ApplyWallpaper), Some(0));
        assert_eq!(task_step("[ setting wallpaper... ]", TaskKind::ApplyWallpaper), Some(1));
    }

    #[test]
    fn task_step_unknown_status_is_none() {
        assert_eq!(task_step("", TaskKind::ApplyScheme), None);
        assert_eq!(task_step("[ starting... ]", TaskKind::ApplyScheme), None);
        assert_eq!(task_step("random", TaskKind::BatchConvert), None);
    }

    #[test]
    fn task_step_batch_and_update_always_none() {
        let statuses = ["[ converting wallpaper... ]", "[ reload... ]", "tinty"];
        for s in statuses {
            assert_eq!(task_step(s, TaskKind::BatchConvert), None);
            assert_eq!(task_step(s, TaskKind::UpdateSchemes), None);
        }
    }
}
