use crate::pipeline::wallpaper_cache;
use crate::state::AppState;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use std::path::Path;

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    render_list(f, chunks[0], state);
    render_image_preview(f, chunks[1], state);
}

fn render_list(f: &mut Frame, area: Rect, state: &AppState) {
    let items: Vec<ListItem> = state
        .wallpapers
        .iter()
        .map(|p| {
            let filename = p
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("?");

            let cached = state
                .active_scheme
                .as_ref()
                .map(|s| {
                    let cache_dir = &s.slug;
                    // Check against the wallpaper_cache
                    wallpaper_cache::is_cached(
                        p,
                        &dirs::data_local_dir()
                            .unwrap_or_default()
                            .join("gnomad")
                            .join("wallpapers")
                            .join(cache_dir),
                    )
                })
                .unwrap_or(false);

            let tag = if cached {
                Span::styled("[cached] ", Style::default().fg(Color::Green))
            } else {
                Span::styled("[raw]    ", Style::default().fg(Color::DarkGray))
            };

            let active = state
                .current_wallpaper
                .as_deref()
                .map(|cw| same_file(cw, p))
                .unwrap_or(false);

            let name_style = if active {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(vec![
                tag,
                Span::styled(filename.to_string(), name_style),
            ]))
        })
        .collect();

    let dir_label = "~/Pictures/Wallpapers";
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" WALLPAPERS — {dir_label} "));

    let mut list_state = ListState::default();
    list_state.select(Some(state.selected_wallpaper_idx));

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");

    f.render_stateful_widget(list, area, &mut list_state);
}

fn render_image_preview(f: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default().borders(Borders::ALL).title(" PREVIEW ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(wallpaper) = state.selected_wallpaper() else {
        let placeholder = Paragraph::new("\n  No wallpaper selected")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(placeholder, inner);
        return;
    };

    // Show filename + cache status as text placeholder.
    // Full Sixel/KGP image rendering is handled in app.rs via ratatui-image
    // when the terminal supports it.
    let filename = wallpaper
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("?");

    let cached = state.active_scheme.as_ref().map(|s| {
        let cache_dir = dirs::data_local_dir()
            .unwrap_or_default()
            .join("gnomad")
            .join("wallpapers")
            .join(&s.slug);
        wallpaper_cache::is_cached(wallpaper, &cache_dir)
    }).unwrap_or(false);

    let status = if cached { "converted" } else { "original" };
    let info = format!("\n  {filename}\n\n  [{status}]");
    let para = Paragraph::new(info).style(Style::default().fg(Color::White));
    f.render_widget(para, inner);
}

fn same_file(a: &Path, b: &Path) -> bool {
    a.file_name() == b.file_name()
}

pub fn render_hints(f: &mut Frame, area: Rect) {
    let hints = " [Enter] Apply  [c] Convert dir  [Shift+C] Force re-convert  [d] Change dir  [Tab/h/l] Switch panel  [q] Quit";
    let para = Paragraph::new(hints).style(Style::default().fg(Color::DarkGray));
    f.render_widget(para, area);
}
