use crate::config::Config;
use crate::pipeline;
use crate::schemes::fetch;
use crate::schemes::types::Scheme;
use crate::state::{AppMode, AppState, Panel};
use crate::ui::{animation, scheme_browser, wallpaper_picker};
use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use image::DynamicImage;
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::Paragraph,
};
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};
use std::path::PathBuf;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};
use tracing::debug;

// Result type carried back to the event loop from each spawned pipeline task.
enum TaskResult {
    ApplyScheme { scheme: Scheme, result: anyhow::Result<PathBuf> },
    ApplyWallpaper { wallpaper: PathBuf, result: anyhow::Result<PathBuf> },
    BatchConvert { result: anyhow::Result<()> },
    UpdateSchemes { result: anyhow::Result<Vec<Scheme>> },
}

pub struct App {
    pub state: AppState,
    pub config: Config,
    // The original file the user picked from the wallpaper directory.
    // Never updated to the output/converted path — prevents quality degradation.
    source_wallpaper: Option<PathBuf>,
    // Image preview state
    picker: Option<Picker>,
    image_proto: Option<StatefulProtocol>,
    preview_path: Option<PathBuf>,
    // Async image loading: spawn_blocking + oneshot so navigation stays responsive
    image_rx: Option<oneshot::Receiver<DynamicImage>>,
    // Animation state (owned here so start_animation() can be called inline)
    anim_state: animation::AnimationState,
    // In-flight pipeline task — non-None while Processing
    processing_task: Option<JoinHandle<TaskResult>>,
    // Set to true when we need to flush kitty protocol artifacts after an
    // animation overlay ends (either by task completion or user cancellation).
    need_terminal_clear: bool,
}

impl App {
    pub fn new(config: Config, gnome_color_scheme: String, picker: Option<Picker>) -> Self {
        Self {
            state: AppState::new(gnome_color_scheme),
            config,
            source_wallpaper: None,
            picker,
            image_proto: None,
            preview_path: None,
            image_rx: None,
            anim_state: animation::AnimationState::new(),
            processing_task: None,
            need_terminal_clear: false,
        }
    }

    pub async fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        self.load_wallpapers();
        self.restore_wallpaper_state();

        let mut event_stream = EventStream::new();
        // 120 ms tick — intentionally low FPS to be compositor-friendly during shell reload
        let mut anim_tick = interval(Duration::from_millis(120));

        loop {
            // Flush kitty protocol images left behind by the animation overlay.
            // Must happen before the next draw, not inside it.
            if self.need_terminal_clear {
                self.need_terminal_clear = false;
                // Delete all kitty graphics protocol images. CSI 2J (sent by
                // terminal.clear()) does not remove virtual-placement images from
                // the terminal's pixel layer; the kitty APC delete-all command does.
                // Non-kitty terminals ignore unrecognised APC sequences.
                use std::io::Write;
                let _ = terminal.backend_mut().write_all(b"\x1b_Ga=d,d=A\x1b\\");
                let _ = terminal.backend_mut().flush();
                terminal.clear()?;
            }

            self.poll_image_load();

            let status = self.state.animation_status.borrow().clone();
            terminal.draw(|f| self.render(f, &status))?;

            tokio::select! {
                _ = anim_tick.tick() => {
                    if self.state.mode == AppMode::Processing {
                        self.anim_state.tick();
                    }
                }
                Some(Ok(event)) = event_stream.next() => {
                    if let Event::Key(key) = event {
                        if self.handle_key(key).await? {
                            break;
                        }
                    }
                }
                // Task completion branch — fires as soon as the pipeline finishes,
                // without ever blocking the render loop above.
                maybe_result = Self::await_task(&mut self.processing_task),
                    if self.processing_task.is_some() =>
                {
                    self.processing_task = None;
                    self.state.mode = AppMode::Normal;
                    if let Some(result) = maybe_result {
                        self.handle_task_result(result);
                    }
                    self.need_terminal_clear = true;
                }
            }
        }

        Ok(())
    }

    // Helper future: resolves when the JoinHandle completes.
    // Returns None when the task was cancelled (user pressed Esc).
    async fn await_task(task: &mut Option<JoinHandle<TaskResult>>) -> Option<TaskResult> {
        match task {
            Some(h) => match h.await {
                Ok(result) => Some(result),
                Err(e) if e.is_cancelled() => None,
                Err(e) => panic!("pipeline task panicked: {e}"),
            },
            None => std::future::pending().await,
        }
    }

    fn handle_task_result(&mut self, result: TaskResult) {
        match result {
            TaskResult::ApplyScheme { scheme, result } => {
                match result {
                    Ok(output_path) => {
                        self.config.default_scheme = Some(scheme.slug.clone());
                        if let Err(e) = self.config.save() {
                            tracing::warn!("failed to save config: {e:#}");
                        }
                        self.state.active_scheme = Some(scheme);
                        self.state.current_wallpaper = Some(output_path);
                        self.state.last_error = None;
                        // Re-pin the newly active scheme at the top of the list.
                        self.state.rebuild_filter(self.config.follow_user_scheme_type);
                        self.invalidate_preview();
                    }
                    Err(e) => self.state.last_error = Some(format!("{e:#}")),
                }
            }
            TaskResult::ApplyWallpaper { wallpaper, result } => {
                match result {
                    Ok(output_path) => {
                        self.source_wallpaper = Some(wallpaper.clone());
                        self.config.last_wallpaper = Some(wallpaper);
                        if let Err(e) = self.config.save() {
                            tracing::warn!("failed to save config after wallpaper change: {e:#}");
                        }
                        self.state.current_wallpaper = Some(output_path);
                        self.state.last_error = None;
                        self.invalidate_preview();
                    }
                    Err(e) => self.state.last_error = Some(format!("{e:#}")),
                }
            }
            TaskResult::BatchConvert { result } => {
                self.invalidate_preview();
                if let Err(e) = result {
                    self.state.last_error = Some(format!("{e:#}"));
                }
            }
            TaskResult::UpdateSchemes { result } => {
                match result {
                    Ok(schemes) => {
                        self.state.set_schemes(schemes, self.config.follow_user_scheme_type);
                        self.state.last_error = None;
                    }
                    Err(e) => self.state.last_error = Some(format!("{e:#}")),
                }
            }
        }
    }

    /// Check if a background image load finished; create the protocol if so.
    fn poll_image_load(&mut self) {
        let Some(rx) = &mut self.image_rx else { return };
        match rx.try_recv() {
            Ok(img) => {
                if let Some(picker) = &mut self.picker {
                    self.image_proto = Some(picker.new_resize_protocol(img));
                }
                self.image_rx = None;
            }
            Err(oneshot::error::TryRecvError::Empty) => {}
            Err(oneshot::error::TryRecvError::Closed) => {
                self.image_rx = None;
            }
        }
    }

    /// Kick off a background image load if the selected wallpaper has changed.
    fn maybe_start_image_load(&mut self) {
        if self.state.active_panel != Panel::Wallpapers {
            self.image_proto = None;
            self.preview_path = None;
            self.image_rx = None;
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
        self.image_proto = None;
        self.image_rx = None;

        if self.picker.is_none() {
            return;
        }

        let (tx, rx) = oneshot::channel();
        self.image_rx = Some(rx);
        tokio::task::spawn_blocking(move || {
            if let Ok(img) = image::open(&display_path) {
                let _ = tx.send(img);
            }
        });
    }

    fn render(&mut self, f: &mut Frame, status: &str) {
        // Render the animation full-screen and bail out early — the kitty
        // graphics protocol used by the image preview operates at the pixel
        // level and is not cleared by ratatui's cell buffer, so we must not
        // render the wallpaper picker at all while Processing.
        if self.state.mode == AppMode::Processing {
            animation::render(f, &mut self.anim_state, status);
            return;
        }

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
            Panel::Wallpapers => {
                let wd = self.config.wallpaper_dir.clone();
                let wcd = self.config.wallpaper_cache_dir.clone();
                wallpaper_picker::render(
                    f,
                    chunks[1],
                    &self.state,
                    &wd,
                    &wcd,
                    self.image_proto.as_mut(),
                );
            }
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
            .source_wallpaper
            .as_ref()
            .or(self.state.current_wallpaper.as_ref())
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
        f.render_widget(Paragraph::new(status_bar_text).style(status_style), chunks[3]);

    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
        if key.code == KeyCode::Char('q') && self.state.mode == AppMode::Normal {
            return Ok(true);
        }

        match self.state.mode.clone() {
            AppMode::Normal => self.handle_normal(key).await,
            AppMode::Searching => self.handle_search(key),
            AppMode::EditingDir => self.handle_dir_input(key).await,
            AppMode::Processing => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                    if let Some(task) = self.processing_task.take() {
                        task.abort();
                        // processing_task is now None so the select! task-completion
                        // branch (guarded by is_some()) will never fire — we must
                        // restore Normal mode and schedule the terminal clear here.
                    }
                    self.state.mode = AppMode::Normal;
                    self.need_terminal_clear = true;
                }
                Ok(false)
            }
        }
    }

    async fn handle_normal(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Tab | KeyCode::Char('l') | KeyCode::Char('h') => {
                self.state.toggle_panel();
                self.invalidate_preview();
            }

            KeyCode::Down | KeyCode::Char('j') => {
                self.state.move_down(1);
                self.invalidate_preview();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.state.move_up(1);
                self.invalidate_preview();
            }
            KeyCode::Char('g') => {
                self.state.go_to_top();
                self.invalidate_preview();
            }
            KeyCode::Char('G') => {
                self.state.go_to_bottom();
                self.invalidate_preview();
            }
            KeyCode::Char('d') if key.modifiers == KeyModifiers::CONTROL => {
                self.state.move_down(10);
                self.invalidate_preview();
            }
            KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
                self.state.move_up(10);
                self.invalidate_preview();
            }

            KeyCode::Enter => self.trigger_action()?,

            KeyCode::Char('d')
                if key.modifiers == KeyModifiers::NONE
                    && self.state.active_panel == Panel::Wallpapers =>
            {
                self.state.dir_input = self.config.wallpaper_dir.to_string_lossy().to_string();
                self.state.mode = AppMode::EditingDir;
            }

            KeyCode::Char('c') if key.modifiers == KeyModifiers::NONE => {
                self.trigger_batch_convert(false)?;
            }
            KeyCode::Char('C') => {
                self.trigger_batch_convert(true)?;
            }
            KeyCode::Char('u') if key.modifiers == KeyModifiers::NONE => {
                self.trigger_update_schemes()?;
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
            KeyCode::Esc => {
                self.state.mode = AppMode::Normal;
                self.state.search_query.clear();
                self.state.rebuild_filter(self.config.follow_user_scheme_type);
            }
            KeyCode::Enter => {
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
                        self.invalidate_preview();
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

    fn trigger_action(&mut self) -> Result<()> {
        match self.state.active_panel {
            Panel::Schemes => self.apply_selected_scheme(),
            Panel::Wallpapers => self.apply_selected_wallpaper(),
        }
    }

    fn apply_selected_scheme(&mut self) -> Result<()> {
        let Some(scheme) = self.state.selected_scheme().cloned() else {
            return Ok(());
        };

        self.state.mode = AppMode::Processing;
        self.image_proto = None;
        self.image_rx = None;
        self.need_terminal_clear = true;
        let old_scheme = self.state.active_scheme.clone();
        self.anim_state.start_animation(
            animation::TaskKind::ApplyScheme,
            old_scheme.as_ref(),
            Some(&scheme),
        );
        let _ = self.state.status_tx.send("[ starting... ]".to_string());

        let config = self.config.clone();
        let source_wall = self.source_wallpaper.clone();
        let status_tx = self.state.status_tx.clone();
        let scheme_clone = scheme.clone();

        debug!("apply_scheme: source_wall = {:?}", source_wall);

        self.processing_task = Some(tokio::task::spawn(async move {
            let result = pipeline::apply_scheme(
                &scheme_clone,
                &config,
                source_wall.as_deref(),
                status_tx,
            )
            .await;
            TaskResult::ApplyScheme { scheme, result }
        }));

        Ok(())
    }

    fn apply_selected_wallpaper(&mut self) -> Result<()> {
        let Some(wallpaper) = self.state.selected_wallpaper().cloned() else {
            return Ok(());
        };

        self.state.mode = AppMode::Processing;
        self.image_proto = None;
        self.image_rx = None;
        self.need_terminal_clear = true;
        let current_scheme = self.state.active_scheme.clone();
        self.anim_state.start_animation(
            animation::TaskKind::ApplyWallpaper,
            current_scheme.as_ref(),
            current_scheme.as_ref(),
        );

        let config = self.config.clone();
        let active_scheme = current_scheme;
        let status_tx = self.state.status_tx.clone();
        let wall_clone = wallpaper.clone();

        debug!("apply_wallpaper: {}", wallpaper.display());

        self.processing_task = Some(tokio::task::spawn(async move {
            let result =
                pipeline::apply_wallpaper(&wall_clone, active_scheme.as_ref(), &config, status_tx)
                    .await;
            TaskResult::ApplyWallpaper { wallpaper, result }
        }));

        Ok(())
    }

    fn trigger_batch_convert(&mut self, force: bool) -> Result<()> {
        let scheme = match self.state.active_panel {
            Panel::Schemes => self.state.selected_scheme().cloned(),
            Panel::Wallpapers => self.state.active_scheme.clone(),
        };

        let Some(scheme) = scheme else {
            self.state.last_error = Some("No scheme selected for batch convert".to_string());
            return Ok(());
        };

        self.state.mode = AppMode::Processing;
        self.image_proto = None;
        self.image_rx = None;
        self.need_terminal_clear = true;
        let batch_scheme = self.state.active_scheme.clone();
        self.anim_state.start_animation(
            animation::TaskKind::BatchConvert,
            batch_scheme.as_ref(),
            batch_scheme.as_ref(),
        );

        let config = self.config.clone();
        let status_tx = self.state.status_tx.clone();

        self.processing_task = Some(tokio::task::spawn(async move {
            let result = pipeline::wallpaper_cache::batch_convert(
                &scheme,
                &config.wallpaper_dir,
                &config.wallpaper_cache_dir,
                force,
                status_tx,
            )
            .await;
            TaskResult::BatchConvert { result }
        }));

        Ok(())
    }

    fn trigger_update_schemes(&mut self) -> Result<()> {
        self.state.mode = AppMode::Processing;
        self.image_proto = None;
        self.image_rx = None;
        self.need_terminal_clear = true;
        self.anim_state.start_animation(animation::TaskKind::UpdateSchemes, None, None);
        let _ = self.state.status_tx.send("[ updating schemes... ]".to_string());

        let repo_dir = self.config.schemes_repo_dir.clone();
        let custom_dir = self.config.custom_schemes_dir.clone();

        self.processing_task = Some(tokio::task::spawn(async move {
            let result = async {
                fetch::update_schemes_repo(&repo_dir).await?;
                fetch::load_schemes(&repo_dir, custom_dir.as_deref())
            }
            .await;
            TaskResult::UpdateSchemes { result }
        }));

        Ok(())
    }

    fn restore_wallpaper_state(&mut self) {
        let Some(ref last) = self.config.last_wallpaper.clone() else {
            return;
        };
        if !last.exists() {
            self.config.last_wallpaper = None;
            return;
        }
        self.source_wallpaper = Some(last.clone());
        if let Some(idx) = self.state.wallpapers.iter().position(|w| w == last) {
            self.state.selected_wallpaper_idx = idx;
        }
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
        self.invalidate_preview();
    }

    fn invalidate_preview(&mut self) {
        self.preview_path = None;
        self.maybe_start_image_load();
    }
}
