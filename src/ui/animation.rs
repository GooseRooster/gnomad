use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Clear, Paragraph},
};

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

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

pub fn render(f: &mut Frame, anim: &AnimationState, status: &str) {
    let area = f.area();

    // Centered box
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Length(5),
            Constraint::Min(0),
        ])
        .split(area);

    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(60),
            Constraint::Percentage(20),
        ])
        .split(vertical[1]);

    let popup_area = horizontal[1];

    // Clear the area behind the popup
    f.render_widget(Clear, popup_area);

    let spinner = anim.spinner();
    let display_status = if status.is_empty() {
        "[ processing... ]".to_string()
    } else {
        status.to_string()
    };

    let text = format!("\n{spinner} {display_status}");

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Cyan).bg(Color::Black));

    let para = Paragraph::new(text)
        .block(block)
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Cyan));

    f.render_widget(para, popup_area);
}
