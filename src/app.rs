use crate::config::Config;
use crate::pipeline;
use crate::schemes::fetch;
use crate::state::{AppMode, AppState, Panel};
use crate::ui::{animation, scheme_browser, wallpaper_picker};
use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::Paragraph,
};
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};
use std::path::{Path, PathBuf};
use tokio::time::{interval, Duration};

pub struct App {
    pub state: AppState,
    pub config: Config,
    picker: Option<Picker>,
    image_proto: Option<StatefulProtocol>,
    preview_path: Option<PathBuf>,
}

impl App {
    pub fn new(config: Config, gnome_color_scheme: String, picker: Option<Picker>) -> Self {
        Self {
            state: AppState::new(gnome_color_scheme),
            config,
            picker,
            image_proto: None,
            preview_path: None,
        }
    }

    pub async fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        self.load_wallpapers();

        let mut event_stream = EventStream::new();
        let mut anim_tick = interval(Duration::from_millis(100));
        let mut anim_state = animation::AnimationState::new();

        loop {
            let status = self.state.animation_status.borrow().clone();
            self.update_preview();
            terminal.draw(|f| self.render(f, &anim_state, &status))?;

            tokio::select! {
                _ = anim_tick.tick() => {
                    if self.state.mode == AppMode::Processing {
                        anim_state.tick();
                    }
                }
                Some(Ok(event)) = event_stream.next() => {
                    if let Event::Key(key) = event {
                        if self.handle_key(key).await? {
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn render(&mut self, f: &mut Frame, anim: &animation::AnimationState, status: &str) {
        let area = f.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(area);

        let panel_name = match self.state.active_panel {
            Panel::Schemes => "Schemes",
            Panel::Wallpapers => "Wallpapers",
        };
        let title = Paragraph::new(format!(" gnomad — {panel_name}  [Tab] Switch panel"))
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
        f.render_widget(title, chunks[0]);

        match self.state.active_panel {
            Panel::Schemes => scheme_browser::render(f, chunks[1], &self.state),
            Panel::Wallpapers => wallpaper_picker::render(
                f,
                chunks[1],
                &self.state,
                &self.config.wallpaper_dir.clone(),
                &self.config.wallpaper_cache_dir.clone(),
                self.image_proto.as_mut(),
            ),
        }

        match self.state.active_panel {
            Panel::Schemes => scheme_browser::render_hints(f, chunks[2]),
            Panel::Wallpapers => wallpaper_picker::render_hints(f, chunks[2]),
        }

        let active_name = self
            .state
            .active_scheme
            .as_ref()
            .map(|s| s.slug.as_str())
            .unwrap_or("none");
        let wall_name = self
            .state
            .current_wallpaper
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|f| f.to_str())
            .unwrap_or("none");

        let status_bar_text = if let Some(err) = &self.state.last_error {
            format!(" ERROR: {err}")
        } else {
            format!(" Active: {active_name}   Wall: {wall_name}")
        };

        let status_style = if self.state.last_error.is_some() {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let status_bar = Paragraph::new(status_bar_text).style(status_style);
        f.render_widget(status_bar, chunks[3]);

        if self.state.mode == AppMode::Processing {
            animation::render(f, anim, status);
        }
    }

    fn update_preview(&mut self) {
        if self.state.active_panel != Panel::Wallpapers {
            self.image_proto = None;
            self.preview_path = None;
            return;
        }

        let Some(wallpaper) = self.state.selected_wallpaper().cloned() else {
            self.image_proto = None;
            self.preview_path = None;
            return;
        };

        let display_path = if let Some(scheme) = &self.state.active_scheme {
            let cache_dir = self.config.wallpaper_cache_dir.join(&scheme.slug);
            pipeline::wallpaper_cache::cached_path(&wallpaper, &cache_dir)
                .unwrap_or_else(|| wallpaper.clone())
        } else {
            wallpaper.clone()
        };

        if Some(&display_path) == self.preview_path.as_ref() {
            return;
        }

        self.preview_path = Some(display_path.clone());
        self.load_image(&display_path);
    }

    fn load_image(&mut self, path: &Path) {
        let Some(picker) = self.picker.as_mut() else {
            self.image_proto = None;
            return;
        };
        self.image_proto = image::open(path)
            .ok()
            .map(|img| picker.new_resize_protocol(img));
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
        if key.code == KeyCode::Char('q') && self.state.mode == AppMode::Normal {
            return Ok(true);
        }

        match self.state.mode.clone() {
            AppMode::Normal => self.handle_normal(key).await,
            AppMode::Searching => self.handle_search(key),
            AppMode::EditingDir => self.handle_dir_input(key).await,
            AppMode::Processing => Ok(false),
        }
    }

    async fn handle_normal(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Tab | KeyCode::Char('l') | KeyCode::Char('h') => {
                self.state.toggle_panel();
                self.preview_path = None; // force preview reload on panel switch
            }

            KeyCode::Down | KeyCode::Char('j') => {
                self.state.move_down(1);
                self.preview_path = None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.state.move_up(1);
                self.preview_path = None;
            }
            KeyCode::Char('g') => {
                self.state.go_to_top();
                self.preview_path = None;
            }
            KeyCode::Char('G') => {
                self.state.go_to_bottom();
                self.preview_path = None;
            }
            KeyCode::Char('d') if key.modifiers == KeyModifiers::CONTROL => {
                self.state.move_down(10);
                self.preview_path = None;
            }
            KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
                self.state.move_up(10);
                self.preview_path = None;
            }

            KeyCode::Enter => self.trigger_action().await?,

            KeyCode::Char('d')
                if key.modifiers == KeyModifiers::NONE
                    && self.state.active_panel == Panel::Wallpapers =>
            {
                self.state.dir_input =
                    self.config.wallpaper_dir.to_string_lossy().to_string();
                self.state.mode = AppMode::EditingDir;
            }

            KeyCode::Char('c') if key.modifiers == KeyModifiers::NONE => {
                self.trigger_batch_convert(false).await?;
            }
            KeyCode::Char('C') => {
                self.trigger_batch_convert(true).await?;
            }
            KeyCode::Char('u') if key.modifiers == KeyModifiers::NONE => {
                self.trigger_update_schemes().await?;
            }
            KeyCode::Char('/') if self.state.active_panel == Panel::Schemes => {
                self.state.mode = AppMode::Searching;
            }

            _ => {}
        }
        Ok(false)
    }

    fn handle_search(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.state.mode = AppMode::Normal;
            }
            KeyCode::Char(c) => {
                self.state.search_query.push(c);
                self.state.rebuild_filter(self.config.follow_user_scheme_type);
            }
            KeyCode::Backspace => {
                self.state.search_query.pop();
                self.state.rebuild_filter(self.config.follow_user_scheme_type);
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_dir_input(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.state.mode = AppMode::Normal;
                self.state.dir_input.clear();
            }
            KeyCode::Enter => {
                let new_dir = PathBuf::from(self.state.dir_input.trim());
                self.state.dir_input.clear();
                self.state.mode = AppMode::Normal;

                if new_dir.is_dir() {
                    self.config.wallpaper_dir = new_dir;
                    if let Err(e) = self.config.save() {
                        self.state.last_error = Some(format!("save config: {e:#}"));
                    } else {
                        self.state.selected_wallpaper_idx = 0;
                        self.preview_path = None;
                        self.load_wallpapers();
                        self.state.last_error = None;
                    }
                } else {
                    self.state.last_error =
                        Some(format!("Directory not found: {}", new_dir.display()));
                }
            }
            KeyCode::Backspace => {
                self.state.dir_input.pop();
            }
            KeyCode::Char(c) => {
                self.state.dir_input.push(c);
            }
            _ => {}
        }
        Ok(false)
    }

    async fn trigger_action(&mut self) -> Result<()> {
        match self.state.active_panel {
            Panel::Schemes => self.apply_selected_scheme().await,
            Panel::Wallpapers => self.apply_selected_wallpaper().await,
        }
    }

    async fn apply_selected_scheme(&mut self) -> Result<()> {
        let Some(scheme) = self.state.selected_scheme().cloned() else {
            return Ok(());
        };

        self.state.mode = AppMode::Processing;
        let _ = self.state.status_tx.send("[ starting... ]".to_string());

        let config = self.config.clone();
        let current_wall = self.state.current_wallpaper.clone();
        let status_tx = self.state.status_tx.clone();
        let scheme_clone = scheme.clone();

        let result = tokio::task::spawn(async move {
            pipeline::apply_scheme(&scheme_clone, &config, current_wall.as_deref(), status_tx).await
        })
        .await;

        self.state.mode = AppMode::Normal;

        match result {
            Ok(Ok(new_wall)) => {
                self.state.active_scheme = Some(scheme);
                self.state.current_wallpaper = Some(new_wall);
                self.state.last_error = None;
                self.preview_path = None; // force reload with converted version
            }
            Ok(Err(e)) => self.state.last_error = Some(format!("{e:#}")),
            Err(e) => self.state.last_error = Some(format!("task panicked: {e}")),
        }
        Ok(())
    }

    async fn apply_selected_wallpaper(&mut self) -> Result<()> {
        let Some(wallpaper) = self.state.selected_wallpaper().cloned() else {
            return Ok(());
        };

        self.state.mode = AppMode::Processing;
        let config = self.config.clone();
        let active_scheme = self.state.active_scheme.clone();
        let status_tx = self.state.status_tx.clone();

        let result = tokio::task::spawn(async move {
            pipeline::apply_wallpaper(&wallpaper, active_scheme.as_ref(), &config, status_tx).await
        })
        .await;

        self.state.mode = AppMode::Normal;

        match result {
            Ok(Ok(new_wall)) => {
                self.state.current_wallpaper = Some(new_wall);
                self.state.last_error = None;
                self.preview_path = None;
            }
            Ok(Err(e)) => self.state.last_error = Some(format!("{e:#}")),
            Err(e) => self.state.last_error = Some(format!("task panicked: {e}")),
        }
        Ok(())
    }

    async fn trigger_batch_convert(&mut self, force: bool) -> Result<()> {
        let scheme = match self.state.active_panel {
            Panel::Schemes => self.state.selected_scheme().cloned(),
            Panel::Wallpapers => self.state.active_scheme.clone(),
        };

        let Some(scheme) = scheme else {
            self.state.last_error = Some("No scheme selected for batch convert".to_string());
            return Ok(());
        };

        self.state.mode = AppMode::Processing;
        let config = self.config.clone();
        let status_tx = self.state.status_tx.clone();

        let result = tokio::task::spawn(async move {
            pipeline::wallpaper_cache::batch_convert(
                &scheme,
                &config.wallpaper_dir,
                &config.wallpaper_cache_dir,
                force,
                status_tx,
            )
            .await
        })
        .await;

        self.state.mode = AppMode::Normal;
        self.preview_path = None; // refresh preview — cache state may have changed

        if let Ok(Err(e)) = result {
            self.state.last_error = Some(format!("{e:#}"));
        }
        Ok(())
    }

    async fn trigger_update_schemes(&mut self) -> Result<()> {
        self.state.mode = AppMode::Processing;
        let _ = self.state.status_tx.send("[ updating schemes... ]".to_string());

        let repo_dir = self.config.schemes_repo_dir.clone();
        let custom_dir = self.config.custom_schemes_dir.clone();

        let result = tokio::task::spawn(async move {
            fetch::update_schemes_repo(&repo_dir).await?;
            fetch::load_schemes(&repo_dir, custom_dir.as_deref())
        })
        .await;

        self.state.mode = AppMode::Normal;

        match result {
            Ok(Ok(schemes)) => {
                self.state
                    .set_schemes(schemes, self.config.follow_user_scheme_type);
                self.state.last_error = None;
            }
            Ok(Err(e)) => self.state.last_error = Some(format!("{e:#}")),
            Err(e) => self.state.last_error = Some(format!("task panicked: {e}")),
        }
        Ok(())
    }

    fn load_wallpapers(&mut self) {
        if !self.config.wallpaper_dir.exists() {
            self.state.wallpapers.clear();
            return;
        }
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&self.config.wallpaper_dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                matches!(
                    p.extension().and_then(|e| e.to_str()),
                    Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp")
                )
            })
            .collect();
        entries.sort();
        self.state.wallpapers = entries;
    }
}
