use crate::config::GowallTuning;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

const LABELS: [&str; 7] = [
    "Hue",
    "Saturation",
    "Lightness",
    "Contrast",
    "Gamma",
    "Intensity",
    "Neutrals",
];

pub fn render(f: &mut Frame, area: Rect, tuning: &GowallTuning, selected: usize) {
    let width = area.width.saturating_mul(3) / 4;
    let width = width.max(54).min(area.width);
    let height = 13.min(area.height);
    let popup = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };
    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" GOWALL TUNING ")
        .style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let mut constraints = vec![Constraint::Length(1); LABELS.len()];
    constraints.push(Constraint::Min(1));
    constraints.push(Constraint::Length(1));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    for (i, label) in LABELS.iter().enumerate() {
        let (value, range) = match i {
            0 => (tuning.hue_shift as f32, (-180.0, 180.0)),
            1 => (tuning.saturation as f32, (0.0, 200.0)),
            2 => (tuning.lightness as f32, (0.0, 200.0)),
            3 => (tuning.contrast as f32, (-100.0, 100.0)),
            4 => (tuning.gamma as f32, (50.0, 200.0)),
            5 => (tuning.intensity as f32, (0.0, 100.0)),
            _ => (0.0, (0.0, 1.0)),
        };

        let bar = if i == 6 {
            if tuning.preserve_neutrals {
                "[ on ]".to_string()
            } else {
                "[ off ]".to_string()
            }
        } else {
            slider(value, range.0, range.1, 24)
        };

        let value_text = match i {
            0 => format!("{:+}°", tuning.hue_shift),
            1 => format!("{}%", tuning.saturation),
            2 => format!("{}%", tuning.lightness),
            3 => format!("{:+}%", tuning.contrast),
            4 => format!("{:.2}x", tuning.gamma as f32 / 100.0),
            5 => format!("{}%", tuning.intensity),
            _ => String::new(),
        };

        let style = if i == selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let line = Line::from(vec![
            Span::styled(format!(" {:<11}", label), style),
            Span::styled(bar, style),
            Span::styled(format!("  {value_text}"), style),
        ]);
        f.render_widget(Paragraph::new(line).style(style), rows[i]);
    }

    let hint = " [j/k] Select  [h/l] Adjust  [Shift] Coarse  [r] Reset  [R] Reset all  [Esc] Close";
    f.render_widget(
        Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
        rows[LABELS.len()],
    );
}

fn slider(value: f32, min: f32, max: f32, width: usize) -> String {
    let ratio = if max > min {
        ((value - min) / (max - min)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let filled = (ratio * width as f32).round() as usize;
    format!(
        "[{}{}]",
        "█".repeat(filled.min(width)),
        "░".repeat(width.saturating_sub(filled))
    )
}
