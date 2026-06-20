use crate::schemes::types::Scheme;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

const SCHEME_PIPELINE: &[&str] = &[
    "convert wallpaper",
    "apply tinty",
    "write gtk css",
    "write shell css",
    "reload shell",
];

pub struct AnimationState {
    pub frame: usize,
}

impl AnimationState {
    pub fn new() -> Self {
        Self { frame: 0 }
    }

    pub fn tick(&mut self) {
        self.frame = (self.frame + 1) % SPINNER_FRAMES.len();
    }

    pub fn spinner(&self) -> &str {
        SPINNER_FRAMES[self.frame]
    }
}

pub fn render(f: &mut Frame, anim: &AnimationState, status: &str, scheme: Option<&Scheme>) {
    let area = f.area();

    // Blank the whole screen
    f.render_widget(Clear, area);
    f.render_widget(
        Block::default().style(Style::default().bg(Color::Rgb(14, 14, 20))),
        area,
    );

    // Palette strip at top and bottom (requires scheme to be known)
    if let Some(s) = scheme {
        if area.height > 4 {
            render_palette_strip(f, Rect { x: area.x, y: area.y, width: area.width, height: 1 }, s);
            render_palette_strip(
                f,
                Rect { x: area.x, y: area.y + area.height - 1, width: area.width, height: 1 },
                s,
            );
        }
    }

    // Determine pipeline step
    let current_step = pipeline_step(status);
    let is_scheme_pipeline = current_step.is_some();
    let spinner = anim.spinner();

    // Popup height: 3 header lines + up to 5 step lines + 2 padding = 10 for scheme, 5 for other
    let popup_height = if is_scheme_pipeline { 11u16 } else { 5 };

    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(popup_height),
            Constraint::Min(0),
        ])
        .split(area);

    let horiz = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(10),
            Constraint::Percentage(80),
            Constraint::Percentage(10),
        ])
        .split(vert[1]);

    let popup = horiz[1];
    let mut lines: Vec<Line> = Vec::new();

    // Header: spinner + title
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(spinner, Style::default().fg(Color::Cyan)),
        Span::styled("  gnomad", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    ]));

    if let Some(s) = scheme {
        lines.push(Line::from(vec![Span::styled(
            format!("     {}", s.name),
            Style::default().fg(Color::White),
        )]));
    }

    lines.push(Line::from(""));

    if is_scheme_pipeline {
        let step = current_step.unwrap();
        for (i, label) in SCHEME_PIPELINE.iter().enumerate() {
            let (icon, style) = if i < step {
                ("✓", Style::default().fg(Color::Green))
            } else if i == step {
                (spinner, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            } else {
                ("○", Style::default().fg(Color::DarkGray))
            };
            lines.push(Line::from(vec![Span::styled(
                format!("  {icon}  {label}"),
                style,
            )]));
        }
    } else {
        let display = if status.is_empty() { "processing..." } else { status.trim_matches(|c| c == '[' || c == ']').trim() };
        lines.push(Line::from(vec![Span::styled(
            format!("  {spinner}  {display}"),
            Style::default().fg(Color::Cyan),
        )]));
    }

    let para = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .style(Style::default().fg(Color::Cyan).bg(Color::Rgb(14, 14, 20))),
    );

    f.render_widget(para, popup);
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
