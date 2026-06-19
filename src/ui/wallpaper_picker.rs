use crate::pipeline::wallpaper_cache;
use crate::state::{AppMode, AppState};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};
use ratatui_image::{StatefulImage, protocol::StatefulProtocol};
use std::path::Path;

pub fn render(
    f: &mut Frame,
    area: Rect,
    state: &AppState,
    wallpaper_dir: &Path,
    wallpaper_cache_dir: &Path,
    image_proto: Option<&mut StatefulProtocol>,
) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    render_list(f, chunks[0], state, wallpaper_dir, wallpaper_cache_dir);
    render_preview(f, chunks[1], state, wallpaper_cache_dir, image_proto);

    if state.mode == AppMode::EditingDir {
        render_dir_prompt(f, area, state);
    }
}

fn render_list(
    f: &mut Frame,
    area: Rect,
    state: &AppState,
    wallpaper_dir: &Path,
    wallpaper_cache_dir: &Path,
) {
    let items: Vec<ListItem> = state
        .wallpapers
        .iter()
        .map(|p| {
            let filename = p.file_name().and_then(|f| f.to_str()).unwrap_or("?");

            let cached = state
                .active_scheme
                .as_ref()
                .map(|s| {
                    wallpaper_cache::is_cached(p, &wallpaper_cache_dir.join(&s.slug))
                })
                .unwrap_or(false);

            let tag = if cached {
                Span::styled("[cached] ", Style::default().fg(Color::Green))
            } else {
                Span::styled("[raw]    ", Style::default().fg(Color::DarkGray))
            };

            let is_current = state
                .current_wallpaper
                .as_deref()
                .map(|cw| same_filename(cw, p))
                .unwrap_or(false);

            let name_style = if is_current {
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

    let dir_label = wallpaper_dir.to_string_lossy();
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

fn render_preview(
    f: &mut Frame,
    area: Rect,
    state: &AppState,
    wallpaper_cache_dir: &Path,
    image_proto: Option<&mut StatefulProtocol>,
) {
    let block = Block::default().borders(Borders::ALL).title(" PREVIEW ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(wallpaper) = state.selected_wallpaper() else {
        f.render_widget(
            Paragraph::new("\n  No wallpaper selected")
                .style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    };

    if let Some(proto) = image_proto {
        f.render_stateful_widget(StatefulImage::new(None), inner, proto);
    } else {
        // Fallback text when terminal has no graphics support
        let filename = wallpaper.file_name().and_then(|f| f.to_str()).unwrap_or("?");
        let cached = state.active_scheme.as_ref().map(|s| {
            wallpaper_cache::is_cached(wallpaper, &wallpaper_cache_dir.join(&s.slug))
        }).unwrap_or(false);
        let status = if cached { "converted" } else { "original" };
        let info = format!("\n  {filename}\n\n  [{status}]\n\n  (no graphics support detected)");
        f.render_widget(
            Paragraph::new(info).style(Style::default().fg(Color::White)),
            inner,
        );
    }
}

/// Overlay prompt for typing a new wallpaper directory path.
fn render_dir_prompt(f: &mut Frame, area: Rect, state: &AppState) {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let prompt_area = vert[1];
    f.render_widget(Clear, prompt_area);

    let text = format!(" Change wallpaper dir: {}_", state.dir_input);
    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Yellow));
    let para = Paragraph::new(text)
        .block(block)
        .style(Style::default().fg(Color::Yellow));
    f.render_widget(para, prompt_area);
}

fn same_filename(a: &Path, b: &Path) -> bool {
    a.file_name() == b.file_name()
}

pub fn render_hints(f: &mut Frame, area: Rect) {
    let hints = " [Enter] Apply  [c] Convert dir  [Shift+C] Force re-convert  [d] Change dir  [Tab/h/l] Switch panel  [q] Quit";
    let para = Paragraph::new(hints).style(Style::default().fg(Color::DarkGray));
    f.render_widget(para, area);
}
