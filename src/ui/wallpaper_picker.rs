use crate::config::GowallTuning;
use crate::pipeline::wallpaper_cache;
use crate::state::{AppMode, AppState};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};
use ratatui_image::{protocol::StatefulProtocol, StatefulImage};
use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub fn render(
    f: &mut Frame,
    area: Rect,
    state: &AppState,
    wallpaper_dir: &Path,
    wallpaper_cache_dir: &Path,
    tuning: &GowallTuning,
    source_proto: Option<&mut StatefulProtocol>,
    converted_proto: Option<&mut StatefulProtocol>,
    picker_exists: bool,
    preview_status: Option<&str>,
) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    render_list(
        f,
        chunks[0],
        state,
        wallpaper_dir,
        wallpaper_cache_dir,
        tuning,
    );
    render_preview(
        f,
        chunks[1],
        state,
        source_proto,
        converted_proto,
        picker_exists,
        preview_status,
    );

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
    tuning: &GowallTuning,
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
                    let cache_dir = wallpaper_cache::scheme_cache_dir(
                        wallpaper_cache_dir,
                        s,
                        wallpaper_dir,
                        tuning,
                    );
                    wallpaper_cache::is_cached(p, &cache_dir, s, tuning)
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
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
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
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    f.render_stateful_widget(list, area, &mut list_state);
}

#[allow(clippy::too_many_arguments)]
fn render_preview(
    f: &mut Frame,
    area: Rect,
    state: &AppState,
    source_proto: Option<&mut StatefulProtocol>,
    converted_proto: Option<&mut StatefulProtocol>,
    picker_exists: bool,
    preview_status: Option<&str>,
) {
    let title = state
        .active_scheme
        .as_ref()
        .map(|s| format!(" PREVIEW → {} ", s.name))
        .unwrap_or_else(|| " PREVIEW ".to_string());
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(wallpaper) = state.selected_wallpaper() else {
        f.render_widget(
            Paragraph::new("\n  No wallpaper selected").style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    };

    if let Some(proto) = converted_proto {
        f.render_widget(ratatui::widgets::Clear, inner);
        f.render_stateful_widget(StatefulImage::new(None), inner, proto);
    } else if let Some(proto) = source_proto {
        f.render_widget(ratatui::widgets::Clear, inner);
        f.render_stateful_widget(StatefulImage::new(None), inner, proto);
        if let Some(status) = preview_status {
            f.render_widget(
                Paragraph::new(format!("  {status}")).style(Style::default().fg(Color::DarkGray)),
                inner,
            );
        }
    } else if picker_exists {
        let filename = wallpaper
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("?");
        let status = preview_status.unwrap_or("Loading preview...");
        f.render_widget(
            Paragraph::new(format!("\n  {filename}\n\n  {status}"))
                .style(Style::default().fg(Color::DarkGray)),
            inner,
        );
    } else {
        let filename = wallpaper
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("?");
        let info = format!("\n  {filename}\n\n  (no graphics protocol support detected)");
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

pub fn render_hints(f: &mut Frame, area: Rect, wallpaper_enabled: bool) {
    let hints = if wallpaper_enabled {
        " [Enter] Apply  [t] Tuning  [c] Convert dir  [Shift+C] Force  [s] Slideshow  [d] Change dir  [Tab/h/l] Panel  [q] Quit"
    } else {
        " [Enter] Apply  [t] Tuning  [Tab/h/l] Panel  [q] Quit"
    };
    let para = Paragraph::new(hints).style(Style::default().fg(Color::DarkGray));
    f.render_widget(para, area);
}
