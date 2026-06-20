use crate::schemes::types::{Scheme, SchemeSystem};
use crate::state::AppState;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    render_list(f, chunks[0], state);
    render_preview(f, chunks[1], state);
}

fn render_list(f: &mut Frame, area: Rect, state: &AppState) {
    let items: Vec<ListItem> = state
        .filtered_schemes
        .iter()
        .map(|&idx| {
            let scheme = &state.all_schemes[idx];
            let tag = if scheme.is_custom {
                "  *"
            } else {
                scheme.system.tag(false)
            };

            let active = state
                .active_scheme
                .as_ref()
                .map(|s| s.slug == scheme.slug)
                .unwrap_or(false);

            let name_style = if active {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let line = Line::from(vec![
                Span::styled(format!("[{tag}] "), Style::default().fg(Color::DarkGray)),
                Span::styled(scheme.name.clone(), name_style),
            ]);
            ListItem::new(line)
        })
        .collect();

    let title = if state.search_query.is_empty() {
        format!(" SCHEMES ({}) ", state.filtered_schemes.len())
    } else {
        format!(" SCHEMES / {} ", state.search_query)
    };

    let block = Block::default().borders(Borders::ALL).title(title);

    let mut list_state = ListState::default();
    list_state.select(Some(state.selected_scheme_idx));

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    f.render_stateful_widget(list, area, &mut list_state);
}

fn render_preview(f: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default().borders(Borders::ALL).title(" PREVIEW ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(scheme) = state.selected_scheme() else {
        return;
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // base16 row 1
            Constraint::Length(2), // base16 row 2
            Constraint::Length(2), // base24 row (optional)
            Constraint::Length(1), // spacer
            Constraint::Min(4),    // metadata
        ])
        .split(inner);

    // Swatches — 2 rows of 8 for base16
    render_swatch_row(f, chunks[0], &base16_colors_row1(scheme));
    render_swatch_row(f, chunks[1], &base16_colors_row2(scheme));

    if scheme.system == SchemeSystem::Base24 {
        render_swatch_row(f, chunks[2], &base24_colors(scheme));
    }

    // Metadata
    let variant_str = scheme.variant.as_deref().unwrap_or("—");
    let custom_tag = if scheme.is_custom { " [custom]" } else { "" };
    let system_str = match scheme.system {
        SchemeSystem::Base16 => "base16",
        SchemeSystem::Base24 => "base24",
    };

    let meta = format!(
        "Name:    {}{}\nSystem:  {}\nAuthor:  {}\nVariant: {}",
        scheme.name, custom_tag, system_str, scheme.author, variant_str
    );

    let meta_para = Paragraph::new(meta).style(Style::default().fg(Color::White));
    f.render_widget(meta_para, chunks[4]);
}

fn render_swatch_row(f: &mut Frame, area: Rect, colors: &[&str]) {
    if colors.is_empty() {
        return;
    }
    let n = colors.len();
    let width = area.width / n as u16;

    for (i, hex) in colors.iter().enumerate() {
        let x = area.x + (i as u16 * width);
        let swatch_area = Rect {
            x,
            y: area.y,
            width: width.max(1),
            height: area.height,
        };
        let color = hex_to_ratatui_color(hex);
        let block = Block::default().style(Style::default().bg(color));
        f.render_widget(block, swatch_area);
    }
}

fn hex_to_ratatui_color(hex: &str) -> Color {
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

fn base16_colors_row1<'a>(s: &'a Scheme) -> Vec<&'a str> {
    vec![
        &s.base00, &s.base01, &s.base02, &s.base03, &s.base04, &s.base05, &s.base06, &s.base07,
    ]
}

fn base16_colors_row2<'a>(s: &'a Scheme) -> Vec<&'a str> {
    vec![
        &s.base08, &s.base09, &s.base0a, &s.base0b, &s.base0c, &s.base0d, &s.base0e, &s.base0f,
    ]
}

fn base24_colors<'a>(s: &'a Scheme) -> Vec<&'a str> {
    let mut row = Vec::new();
    for slot in [
        &s.base10, &s.base11, &s.base12, &s.base13, &s.base14, &s.base15, &s.base16, &s.base17,
    ] {
        if let Some(h) = slot {
            row.push(h.as_str());
        }
    }
    row
}

pub fn render_hints(f: &mut Frame, area: Rect) {
    let hints = " [Enter] Apply  [c] Pre-convert wallpapers  [u] Update schemes  [/] Search  [Tab/h/l] Switch panel  [q] Quit";
    let para = Paragraph::new(hints).style(Style::default().fg(Color::DarkGray));
    f.render_widget(para, area);
}
