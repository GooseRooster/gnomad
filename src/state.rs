use crate::schemes::types::Scheme;
use std::path::PathBuf;
use tokio::sync::watch;

#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    Normal,
    Processing,
    Searching,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Panel {
    Schemes,
    Wallpapers,
}

pub struct AppState {
    pub active_scheme: Option<Scheme>,
    pub current_wallpaper: Option<PathBuf>,
    pub mode: AppMode,
    pub active_panel: Panel,

    // Scheme browser
    pub all_schemes: Vec<Scheme>,
    pub filtered_schemes: Vec<usize>, // indices into all_schemes
    pub scheme_list_offset: usize,
    pub selected_scheme_idx: usize,   // index into filtered_schemes
    pub search_query: String,

    // Wallpaper picker
    pub wallpapers: Vec<PathBuf>,
    pub wallpaper_list_offset: usize,
    pub selected_wallpaper_idx: usize,

    // GNOME state
    pub gnome_color_scheme: String, // "prefer-dark" | "prefer-light" | "default"

    // Processing animation
    pub animation_status: watch::Receiver<String>,
    pub status_tx: watch::Sender<String>,

    // Error/warning display
    pub last_error: Option<String>,
}

impl AppState {
    pub fn new(gnome_color_scheme: String) -> Self {
        let (status_tx, animation_status) = watch::channel(String::new());
        Self {
            active_scheme: None,
            current_wallpaper: None,
            mode: AppMode::Normal,
            active_panel: Panel::Schemes,
            all_schemes: Vec::new(),
            filtered_schemes: Vec::new(),
            scheme_list_offset: 0,
            selected_scheme_idx: 0,
            search_query: String::new(),
            wallpapers: Vec::new(),
            wallpaper_list_offset: 0,
            selected_wallpaper_idx: 0,
            gnome_color_scheme,
            animation_status,
            status_tx,
            last_error: None,
        }
    }

    pub fn set_schemes(&mut self, schemes: Vec<Scheme>, follow_type: bool) {
        self.all_schemes = schemes;
        self.rebuild_filter(follow_type);
    }

    pub fn rebuild_filter(&mut self, follow_type: bool) {
        let preference = if follow_type {
            match self.gnome_color_scheme.as_str() {
                "prefer-dark" => Some("dark"),
                "prefer-light" => Some("light"),
                _ => None,
            }
        } else {
            None
        };

        self.filtered_schemes = self
            .all_schemes
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                // Text search
                if !self.search_query.is_empty() {
                    let q = self.search_query.to_lowercase();
                    if !s.name.to_lowercase().contains(&q) && !s.slug.contains(&q) {
                        return false;
                    }
                }
                // Variant filter
                if let Some(pref) = preference {
                    if let Some(v) = &s.variant {
                        return v == pref;
                    }
                    // Schemes without variant pass through
                }
                true
            })
            .map(|(i, _)| i)
            .collect();

        self.selected_scheme_idx = self.selected_scheme_idx.min(
            self.filtered_schemes.len().saturating_sub(1)
        );
    }

    pub fn selected_scheme(&self) -> Option<&Scheme> {
        self.filtered_schemes
            .get(self.selected_scheme_idx)
            .and_then(|&i| self.all_schemes.get(i))
    }

    pub fn selected_wallpaper(&self) -> Option<&PathBuf> {
        self.wallpapers.get(self.selected_wallpaper_idx)
    }

    pub fn toggle_panel(&mut self) {
        self.active_panel = match self.active_panel {
            Panel::Schemes => Panel::Wallpapers,
            Panel::Wallpapers => Panel::Schemes,
        };
    }

    // ── Navigation ────────────────────────────────────────────────────────────

    pub fn move_down(&mut self, amount: usize) {
        match self.active_panel {
            Panel::Schemes => {
                let max = self.filtered_schemes.len().saturating_sub(1);
                self.selected_scheme_idx = (self.selected_scheme_idx + amount).min(max);
            }
            Panel::Wallpapers => {
                let max = self.wallpapers.len().saturating_sub(1);
                self.selected_wallpaper_idx = (self.selected_wallpaper_idx + amount).min(max);
            }
        }
    }

    pub fn move_up(&mut self, amount: usize) {
        match self.active_panel {
            Panel::Schemes => {
                self.selected_scheme_idx =
                    self.selected_scheme_idx.saturating_sub(amount);
            }
            Panel::Wallpapers => {
                self.selected_wallpaper_idx =
                    self.selected_wallpaper_idx.saturating_sub(amount);
            }
        }
    }

    pub fn go_to_top(&mut self) {
        match self.active_panel {
            Panel::Schemes => self.selected_scheme_idx = 0,
            Panel::Wallpapers => self.selected_wallpaper_idx = 0,
        }
    }

    pub fn go_to_bottom(&mut self) {
        match self.active_panel {
            Panel::Schemes => {
                self.selected_scheme_idx =
                    self.filtered_schemes.len().saturating_sub(1);
            }
            Panel::Wallpapers => {
                self.selected_wallpaper_idx =
                    self.wallpapers.len().saturating_sub(1);
            }
        }
    }
}
