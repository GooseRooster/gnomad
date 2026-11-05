use crate::config::{Config, GowallTuning};
use crate::pipeline;
use crate::pipeline::gnome::GnomeInterface;
use crate::schemes::fetch;
use crate::schemes::types::Scheme;
use crate::state::{AppMode, AppState, Panel};
use crate::ui::{animation, scheme_browser, tuning, wallpaper_picker};
use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use image::DynamicImage;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::Paragraph,
    DefaultTerminal, Frame,
};
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};
use std::path::PathBuf;
use std::time::Instant;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};
use tracing::debug;

// Result type carried back to the event loop from each spawned pipeline task.
enum TaskResult {
    ApplyScheme {
        scheme: Box<Scheme>,
        result: anyhow::Result<PathBuf>,
    },
    ApplyWallpaper {
        wallpaper: PathBuf,
        result: anyhow::Result<PathBuf>,
    },
    BatchConvert {
        result: anyhow::Result<()>,
    },
    UpdateSchemes {
        result: anyhow::Result<Vec<Scheme>>,
    },
    CreateSlideshow {
        result: anyhow::Result<PathBuf>,
    },
}

struct PreviewTaskResult {
    key: String,
    result: anyhow::Result<PathBuf>,
}

#[derive(Default)]
struct ImageSlot {
    proto: Option<StatefulProtocol>,
    path: Option<PathBuf>,
    rx: Option<oneshot::Receiver<DynamicImage>>,
}

impl ImageSlot {
    fn clear(&mut self) {
        self.proto = None;
        self.path = None;
        self.rx = None;
    }
}

pub struct App {
    pub state: AppState,
    pub config: Config,
    // The original file the user picked from the wallpaper directory.
    // Never updated to the output/converted path — prevents quality degradation.
    source_wallpaper: Option<PathBuf>,
    // Image preview state: original/current and converted panes.
    picker: Option<Picker>,
    source_preview: ImageSlot,
    converted_preview: ImageSlot,
    // Background conversion for the selected scheme/wallpaper.
    preview_task: Option<JoinHandle<PreviewTaskResult>>,
    preview_key: Option<String>,
    preview_status: Option<String>,
    // Animation state (owned here so start_animation() can be called inline)
    anim_state: animation::AnimationState,
    // In-flight pipeline task — non-None while Processing
    processing_task: Option<JoinHandle<TaskResult>>,
    // Set to true when we need to flush kitty protocol artifacts after an
    // animation overlay ends (either by task completion or user cancellation).
    need_terminal_clear: bool,
    // Tuning overlay.
    tuning_open: bool,
    tuning_selected: usize,
    last_tuning_save: Option<Instant>,
}

impl App {
    pub fn new(config: Config, gnome_color_scheme: String, picker: Option<Picker>) -> Self {
        Self {
            state: AppState::new(gnome_color_scheme),
            config,
            source_wallpaper: None,
            picker,
            source_preview: ImageSlot::default(),
            converted_preview: ImageSlot::default(),
            preview_task: None,
            preview_key: None,
            preview_status: None,
            anim_state: animation::AnimationState::new(),
            processing_task: None,
            need_terminal_clear: false,
            tuning_open: false,
            tuning_selected: 0,
            last_tuning_save: None,
        }
    }

    pub async fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        if self.config.wallpaper_features_enabled() {
            self.load_wallpapers();
            self.restore_wallpaper_state();
            if self.source_wallpaper.is_none() {
                self.detect_current_wallpaper().await;
            }
        }
        self.refresh_previews();

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
                Some(event) = event_stream.next() => {
                    match event {
                        Ok(Event::Key(key)) => {
                            if self.handle_key(key).await? {
                                break;
                            }
                        }
                        Ok(_) => {}
                        Err(e) => debug!("terminal event error: {e:?}"),
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
                // Preview conversion completion.
                maybe_preview = Self::await_preview(&mut self.preview_task),
                    if self.preview_task.is_some() =>
                {
                    self.preview_task = None;
                    if let Some(result) = maybe_preview {
                        self.handle_preview_result(result);
                    }
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

    async fn await_preview(
        task: &mut Option<JoinHandle<PreviewTaskResult>>,
    ) -> Option<PreviewTaskResult> {
        match task {
            Some(h) => match h.await {
                Ok(result) => Some(result),
                Err(e) if e.is_cancelled() => None,
                Err(e) => panic!("preview task panicked: {e}"),
            },
            None => std::future::pending().await,
        }
    }

    fn handle_preview_result(&mut self, result: PreviewTaskResult) {
        if self.preview_key.as_deref() != Some(result.key.as_str()) {
            return;
        }
        match result.result {
            Ok(path) => {
                self.preview_status = None;
                let has_picker = self.picker.is_some();
                start_image_load(&mut self.converted_preview, path, has_picker);
            }
            Err(e) => {
                self.preview_status = Some(format!("preview failed: {e:#}"));
                self.converted_preview.clear();
            }
        }
    }

    fn handle_task_result(&mut self, result: TaskResult) {
        match result {
            TaskResult::ApplyScheme { scheme, result } => {
                let scheme = *scheme;
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
                        self.state
                            .rebuild_filter(self.config.follow_user_scheme_type);
                        self.sort_wallpapers_by_cache();
                        self.invalidate_preview();
                    }
                    Err(e) => self.state.last_error = Some(format!("{e:#}")),
                }
            }
            TaskResult::ApplyWallpaper { wallpaper, result } => match result {
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
            },
            TaskResult::BatchConvert { result } => {
                self.invalidate_preview();
                if let Err(e) = result {
                    self.state.last_error = Some(format!("{e:#}"));
                }
            }
            TaskResult::UpdateSchemes { result } => match result {
                Ok(schemes) => {
                    self.state
                        .set_schemes(schemes, self.config.follow_user_scheme_type);
                    self.state.last_error = None;
                }
                Err(e) => self.state.last_error = Some(format!("{e:#}")),
            },
            TaskResult::CreateSlideshow { result } => match result {
                Ok(_) => self.state.last_error = None,
                Err(e) => self.state.last_error = Some(format!("{e:#}")),
            },
        }
    }

    /// Check if background image loads finished; create protocols if so.
    fn poll_image_load(&mut self) {
        poll_slot(&mut self.source_preview, &mut self.picker);
        poll_slot(&mut self.converted_preview, &mut self.picker);
    }

    fn cancel_preview(&mut self) {
        if let Some(task) = self.preview_task.take() {
            task.abort();
        }
    }

    fn clear_previews(&mut self) {
        self.cancel_preview();
        self.source_preview.clear();
        self.converted_preview.clear();
        self.preview_key = None;
        self.preview_status = None;
    }

    /// Recompute which source/converted images should be visible. Starts a
    /// debounced background conversion when the target changes.
    fn refresh_previews(&mut self) {
        if !self.config.wallpaper_features_enabled() {
            self.cancel_preview();
            self.source_preview.clear();
            self.converted_preview.clear();
            self.preview_key = None;
            self.preview_status = None;
            return;
        }

        let source = match self.state.active_panel {
            Panel::Schemes => self.source_wallpaper.clone(),
            Panel::Wallpapers => self.state.selected_wallpaper().cloned(),
        };

        match &source {
            Some(path) => {
                if self.source_preview.path.as_ref() != Some(path) {
                    let has_picker = self.picker.is_some();
                    start_image_load(&mut self.source_preview, path.clone(), has_picker);
                }
            }
            None => self.source_preview.clear(),
        }

        let scheme = match self.state.active_panel {
            Panel::Schemes => self.state.selected_scheme().cloned(),
            Panel::Wallpapers => self.state.active_scheme.clone(),
        };

        let (Some(source), Some(scheme)) = (source, scheme) else {
            self.cancel_preview();
            self.converted_preview.clear();
            self.preview_key = None;
            self.preview_status = None;
            return;
        };

        if self.picker.is_none() {
            self.cancel_preview();
            self.converted_preview.clear();
            self.preview_key = None;
            self.preview_status = Some("image preview unavailable in this terminal".to_string());
            return;
        }

        let tuning = self.config.gowall_tuning.clone();
        let key = pipeline::preview::preview_key(&scheme, &tuning, &source);
        if self.preview_key.as_deref() == Some(key.as_str()) {
            return;
        }

        self.cancel_preview();
        self.preview_key = Some(key.clone());
        self.converted_preview.clear();
        self.preview_status = None;

        // The wallpaper panel already has a full-size cache lookup; reuse it
        // rather than generating a redundant downscaled preview.
        if self.state.active_panel == Panel::Wallpapers {
            let cache_dir = pipeline::wallpaper_cache::scheme_cache_dir(
                &self.config.wallpaper_cache_dir,
                &scheme,
                &self.config.wallpaper_dir,
                &tuning,
            );
            if let Some(cached) =
                pipeline::wallpaper_cache::cached_path(&source, &cache_dir, &scheme, &tuning)
            {
                let has_picker = self.picker.is_some();
                start_image_load(&mut self.converted_preview, cached, has_picker);
                return;
            }
        }

        if let Some(cached) = pipeline::preview::cached_preview(&scheme, &tuning, &source) {
            let has_picker = self.picker.is_some();
            start_image_load(&mut self.converted_preview, cached, has_picker);
            return;
        }

        self.preview_status = Some("generating preview...".to_string());
        let source_for_task = source.clone();
        self.preview_task = Some(tokio::spawn(async move {
            // Debounce so holding j/k does not spawn a conversion per row.
            tokio::time::sleep(Duration::from_millis(300)).await;
            let result = pipeline::preview::generate(&scheme, &tuning, &source_for_task).await;
            PreviewTaskResult { key, result }
        }));
    }

    async fn detect_current_wallpaper(&mut self) {
        let Ok(gnome) = GnomeInterface::new().await else {
            return;
        };
        let Ok(Some(path)) = gnome.get_wallpaper().await else {
            return;
        };

        // Never treat gnomad's own converted output as a source image.
        if let Some(out_dir) = self.config.output_wallpaper_path.parent() {
            if path.starts_with(out_dir) {
                return;
            }
        }
        if path.starts_with(&self.config.wallpaper_cache_dir) {
            return;
        }

        self.source_wallpaper = Some(path.clone());
        if let Some(idx) = self.state.wallpapers.iter().position(|w| w == &path) {
            self.state.selected_wallpaper_idx = idx;
        }
        self.config.last_wallpaper = Some(path);
        let _ = self.config.save();
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
        let wallpaper_features = self.config.wallpaper_features_enabled();
        let tab_hint = if wallpaper_features {
            "  [Tab] Switch panel"
        } else {
            ""
        };
        let mode_hint = if self.config.gowall_only {
            "  [gowall-only]"
        } else {
            ""
        };
        let title = Paragraph::new(format!(" gnomad — {panel_name}{mode_hint}{tab_hint}")).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
        f.render_widget(title, chunks[0]);

        let has_picker = self.picker.is_some();
        let preview_status = self.preview_status.clone();
        match self.state.active_panel {
            Panel::Schemes => scheme_browser::render(
                f,
                chunks[1],
                &self.state,
                wallpaper_features,
                self.source_wallpaper.is_some(),
                self.source_preview.proto.as_mut(),
                self.converted_preview.proto.as_mut(),
                has_picker,
                preview_status.as_deref(),
            ),
            Panel::Wallpapers => {
                let wd = self.config.wallpaper_dir.clone();
                let wcd = self.config.wallpaper_cache_dir.clone();
                wallpaper_picker::render(
                    f,
                    chunks[1],
                    &self.state,
                    &wd,
                    &wcd,
                    &self.config.gowall_tuning,
                    self.source_preview.proto.as_mut(),
                    self.converted_preview.proto.as_mut(),
                    has_picker,
                    preview_status.as_deref(),
                );
            }
        }

        match self.state.active_panel {
            Panel::Schemes => scheme_browser::render_hints(
                f,
                chunks[2],
                wallpaper_features,
                self.config.gowall_only,
            ),
            Panel::Wallpapers => wallpaper_picker::render_hints(f, chunks[2], wallpaper_features),
        }

        if self.tuning_open {
            tuning::render(f, area, &self.config.gowall_tuning, self.tuning_selected);
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
        f.render_widget(
            Paragraph::new(status_bar_text).style(status_style),
            chunks[3],
        );
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
        if self.tuning_open {
            return Ok(self.handle_tuning_key(key));
        }
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
                if self.config.wallpaper_features_enabled() {
                    self.state.toggle_panel();
                    self.invalidate_preview();
                }
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
                if self.config.wallpaper_features_enabled() {
                    self.trigger_batch_convert(false)?;
                }
            }
            KeyCode::Char('C') => {
                if self.config.wallpaper_features_enabled() {
                    self.trigger_batch_convert(true)?;
                }
            }
            KeyCode::Char('s')
                if key.modifiers == KeyModifiers::NONE
                    && self.config.wallpaper_features_enabled()
                    && self.state.active_panel == Panel::Wallpapers =>
            {
                self.trigger_create_slideshow()?;
            }
            KeyCode::Char('t') if key.modifiers == KeyModifiers::NONE => {
                self.tuning_open = true;
                self.tuning_selected = 0;
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

    const TUNING_ROWS: usize = 7;

    fn handle_tuning_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc | KeyCode::Char('t') | KeyCode::Char('q') => {
                self.tuning_open = false;
                self.save_tuning(true);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.tuning_selected = (self.tuning_selected + 1) % Self::TUNING_ROWS;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.tuning_selected =
                    (self.tuning_selected + Self::TUNING_ROWS - 1) % Self::TUNING_ROWS;
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.adjust_tuning(-1, key.modifiers.contains(KeyModifiers::SHIFT));
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.adjust_tuning(1, key.modifiers.contains(KeyModifiers::SHIFT));
            }
            KeyCode::Char('r') if key.modifiers == KeyModifiers::NONE => {
                self.reset_tuning_param(self.tuning_selected);
                self.after_tuning_change();
            }
            KeyCode::Char('R') => {
                self.config.gowall_tuning = GowallTuning::default();
                self.after_tuning_change();
            }
            KeyCode::Char(' ') | KeyCode::Enter if self.tuning_selected == 6 => {
                self.config.gowall_tuning.preserve_neutrals =
                    !self.config.gowall_tuning.preserve_neutrals;
                self.after_tuning_change();
            }
            _ => {}
        }
        false
    }

    fn adjust_tuning(&mut self, dir: i32, coarse: bool) {
        let step = if coarse { 15 } else { 5 };
        match self.tuning_selected {
            0 => self.config.gowall_tuning.hue_shift += dir * step,
            1 => self.config.gowall_tuning.saturation += dir * step,
            2 => self.config.gowall_tuning.lightness += dir * step,
            3 => self.config.gowall_tuning.contrast += dir * step,
            4 => self.config.gowall_tuning.gamma += dir * step,
            5 => self.config.gowall_tuning.intensity += dir * step,
            6 => {
                self.config.gowall_tuning.preserve_neutrals =
                    !self.config.gowall_tuning.preserve_neutrals;
            }
            _ => return,
        }
        self.config.gowall_tuning = self.config.gowall_tuning.clamped();
        self.after_tuning_change();
    }

    fn reset_tuning_param(&mut self, idx: usize) {
        let default = GowallTuning::default();
        match idx {
            0 => self.config.gowall_tuning.hue_shift = default.hue_shift,
            1 => self.config.gowall_tuning.saturation = default.saturation,
            2 => self.config.gowall_tuning.lightness = default.lightness,
            3 => self.config.gowall_tuning.contrast = default.contrast,
            4 => self.config.gowall_tuning.gamma = default.gamma,
            5 => self.config.gowall_tuning.intensity = default.intensity,
            6 => self.config.gowall_tuning.preserve_neutrals = default.preserve_neutrals,
            _ => {}
        }
    }

    fn after_tuning_change(&mut self) {
        self.save_tuning(false);
        self.invalidate_preview();
    }

    fn save_tuning(&mut self, force: bool) {
        let now = Instant::now();
        let due = force
            || self
                .last_tuning_save
                .map(|t| now.duration_since(t) >= Duration::from_millis(500))
                .unwrap_or(true);
        if !due {
            return;
        }
        self.last_tuning_save = Some(now);
        if let Err(e) = self.config.save() {
            self.state.last_error = Some(format!("save config: {e:#}"));
        }
    }

    fn handle_search(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.state.mode = AppMode::Normal;
                self.state.search_query.clear();
                self.state
                    .rebuild_filter(self.config.follow_user_scheme_type);
            }
            KeyCode::Enter => {
                self.state.mode = AppMode::Normal;
            }
            KeyCode::Char(c) => {
                self.state.search_query.push(c);
                self.state
                    .rebuild_filter(self.config.follow_user_scheme_type);
            }
            KeyCode::Backspace => {
                self.state.search_query.pop();
                self.state
                    .rebuild_filter(self.config.follow_user_scheme_type);
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
        self.clear_previews();
        self.need_terminal_clear = true;
        let old_scheme = self.state.active_scheme.clone();
        let kind = if self.config.gowall_only {
            animation::TaskKind::ApplySchemeGowallOnly
        } else if self.config.wallpaper_features_enabled() {
            animation::TaskKind::ApplyScheme
        } else {
            animation::TaskKind::ApplySchemeNoWallpaper
        };
        self.anim_state
            .start_animation(kind, old_scheme.as_ref(), Some(&scheme));
        let _ = self.state.status_tx.send("[ starting... ]".to_string());

        let config = self.config.clone();
        let source_wall = self.source_wallpaper.clone();
        let status_tx = self.state.status_tx.clone();
        let scheme_clone = scheme.clone();

        debug!("apply_scheme: source_wall = {:?}", source_wall);

        self.processing_task = Some(tokio::task::spawn(async move {
            let result =
                pipeline::apply_scheme(&scheme_clone, &config, source_wall.as_deref(), status_tx)
                    .await;
            TaskResult::ApplyScheme {
                scheme: Box::new(scheme),
                result,
            }
        }));

        Ok(())
    }

    fn apply_selected_wallpaper(&mut self) -> Result<()> {
        if !self.config.wallpaper_features_enabled() {
            return Ok(());
        }
        let Some(wallpaper) = self.state.selected_wallpaper().cloned() else {
            return Ok(());
        };

        self.state.mode = AppMode::Processing;
        self.clear_previews();
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
        if !self.config.wallpaper_features_enabled() {
            return Ok(());
        }
        let scheme = match self.state.active_panel {
            Panel::Schemes => self.state.selected_scheme().cloned(),
            Panel::Wallpapers => self.state.active_scheme.clone(),
        };

        let Some(scheme) = scheme else {
            self.state.last_error = Some("No scheme selected for batch convert".to_string());
            return Ok(());
        };

        self.state.mode = AppMode::Processing;
        self.clear_previews();
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
                &config.gowall_tuning,
                &config.wallpaper_dir,
                &config.wallpaper_cache_dir,
                force,
                config.hooks.on_wallpaper_batch_convert.clone(),
                status_tx,
            )
            .await;
            TaskResult::BatchConvert { result }
        }));

        Ok(())
    }

    fn trigger_create_slideshow(&mut self) -> Result<()> {
        let scheme = match self.state.active_panel {
            Panel::Schemes => self.state.selected_scheme().cloned(),
            Panel::Wallpapers => self.state.active_scheme.clone(),
        };

        let Some(scheme) = scheme else {
            self.state.last_error = Some("No active scheme for slideshow".to_string());
            return Ok(());
        };

        self.state.mode = AppMode::Processing;
        self.clear_previews();
        self.need_terminal_clear = true;
        let current_scheme = self.state.active_scheme.clone();
        self.anim_state.start_animation(
            animation::TaskKind::CreateSlideshow,
            current_scheme.as_ref(),
            current_scheme.as_ref(),
        );

        let config = self.config.clone();
        let status_tx = self.state.status_tx.clone();

        self.processing_task = Some(tokio::task::spawn(async move {
            let result = pipeline::slideshow::create_slideshow(
                &config.wallpaper_dir,
                &config.wallpaper_cache_dir,
                &scheme.slug,
                &scheme,
                &config.gowall_tuning,
                config.slideshow_static_secs,
                config.slideshow_transition_secs,
                config.hooks.on_wallpaper_batch_convert.clone(),
                status_tx,
            )
            .await;
            TaskResult::CreateSlideshow { result }
        }));

        Ok(())
    }

    fn trigger_update_schemes(&mut self) -> Result<()> {
        self.state.mode = AppMode::Processing;
        self.clear_previews();
        self.need_terminal_clear = true;
        self.anim_state
            .start_animation(animation::TaskKind::UpdateSchemes, None, None);
        let _ = self
            .state
            .status_tx
            .send("[ updating schemes... ]".to_string());

        let repo_dir = self.config.schemes_repo_dir.clone();
        let custom_dir = Some(self.config.custom_schemes_dir.clone());

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
        if !last.exists() || !is_supported_image(last) {
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
            .filter(|p| is_supported_image(p))
            .collect();
        entries.sort();
        self.state.wallpapers = entries;
        self.sort_wallpapers_by_cache();
        self.invalidate_preview();
    }

    fn invalidate_preview(&mut self) {
        self.refresh_previews();
    }

    /// Re-sort the wallpaper list so cached entries for the current scheme come first.
    fn sort_wallpapers_by_cache(&mut self) {
        let current = self
            .state
            .wallpapers
            .get(self.state.selected_wallpaper_idx)
            .cloned();
        let cache_dir = self.state.active_scheme.as_ref().map(|s| {
            pipeline::wallpaper_cache::scheme_cache_dir(
                &self.config.wallpaper_cache_dir,
                s,
                &self.config.wallpaper_dir,
                &self.config.gowall_tuning,
            )
        });
        self.state.wallpapers.sort_by(|a, b| {
            // Check file existence rather than manifest validity — this surfaces already-
            // converted wallpapers even when the manifest is absent or stale.
            let exists_in_cache = |p: &std::path::PathBuf| -> bool {
                let Some(ref cd) = cache_dir else {
                    return false;
                };
                let Some(name) = p.file_name() else {
                    return false;
                };
                cd.join(name).exists()
            };
            let a_cached = exists_in_cache(a);
            let b_cached = exists_in_cache(b);
            b_cached.cmp(&a_cached).then_with(|| a.cmp(b))
        });
        if let Some(current) = current {
            if let Some(idx) = self.state.wallpapers.iter().position(|w| w == &current) {
                self.state.selected_wallpaper_idx = idx;
            }
        }
    }
}

fn is_supported_image(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp")
    )
}

fn poll_slot(slot: &mut ImageSlot, picker: &mut Option<Picker>) {
    let Some(rx) = &mut slot.rx else { return };
    match rx.try_recv() {
        Ok(img) => {
            if let Some(picker) = picker {
                slot.proto = Some(picker.new_resize_protocol(img));
            }
            slot.rx = None;
        }
        Err(oneshot::error::TryRecvError::Empty) => {}
        Err(oneshot::error::TryRecvError::Closed) => {
            slot.rx = None;
        }
    }
}

fn start_image_load(slot: &mut ImageSlot, path: PathBuf, has_picker: bool) {
    if slot.path.as_ref() == Some(&path) {
        return;
    }
    slot.proto = None;
    slot.path = Some(path.clone());
    slot.rx = None;
    if !has_picker {
        return;
    }

    let (tx, rx) = oneshot::channel();
    slot.rx = Some(rx);
    tokio::task::spawn_blocking(move || {
        if let Ok(img) = image::open(&path) {
            // Keep terminal-preview memory bounded for 4K sources.
            let img = if img.width().max(img.height()) > 2048 {
                img.thumbnail(2048, 2048)
            } else {
                img
            };
            let _ = tx.send(img);
        }
    });
}
