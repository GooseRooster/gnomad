use crate::schemes::types::Scheme;
use std::path::PathBuf;
use tokio::sync::watch;

#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    Normal,
    Processing,
    Searching,
    EditingDir,
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
    pub selected_scheme_idx: usize,   // index into filtered_schemes
    pub search_query: String,

    // Wallpaper picker
    pub wallpapers: Vec<PathBuf>,
    pub selected_wallpaper_idx: usize,

    // GNOME state
    pub gnome_color_scheme: String, // "prefer-dark" | "prefer-light" | "default"

    // Processing animation
    pub animation_status: watch::Receiver<String>,
    pub status_tx: watch::Sender<String>,

    // Dir editing (EditingDir mode)
    pub dir_input: String,

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
            selected_scheme_idx: 0,
            search_query: String::new(),
            wallpapers: Vec::new(),
            selected_wallpaper_idx: 0,
            gnome_color_scheme,
            animation_status,
            status_tx,
            dir_input: String::new(),
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

        // Float the active scheme to the top so it's always reachable with `gg`.
        // Clone the slug to avoid a simultaneous borrow of self.
        let active_slug = self.active_scheme.as_ref().map(|s| s.slug.clone());
        if let Some(slug) = active_slug {
            if let Some(pos) = self
                .filtered_schemes
                .iter()
                .position(|&i| self.all_schemes[i].slug == slug)
            {
                if pos > 0 {
                    let entry = self.filtered_schemes.remove(pos);
                    self.filtered_schemes.insert(0, entry);
                    // Adjust the cursor so it keeps pointing at the same scheme.
                    // Elements before `pos` shift right by 1; elements after are
                    // unaffected (remove then re-insert at 0 cancel each other out).
                    if self.selected_scheme_idx < pos {
                        self.selected_scheme_idx += 1;
                    }
                    // When cursor was on the applied scheme (== pos), leave it at pos.
                    // The active scheme is now at index 0; cursor stays near where the
                    // user was browsing rather than jumping to the top.
                }
            }
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schemes::types::SchemeSystem;

    fn make_scheme(name: &str, slug: &str, variant: Option<&str>) -> Scheme {
        Scheme {
            system: SchemeSystem::Base16,
            name: name.to_string(),
            slug: slug.to_string(),
            author: String::new(),
            variant: variant.map(|s| s.to_string()),
            is_custom: false,
            base00: "1d2021".to_string(),
            base01: "282828".to_string(),
            base02: "3c3836".to_string(),
            base03: "504945".to_string(),
            base04: "665c54".to_string(),
            base05: "d5c4a1".to_string(),
            base06: "ebdbb2".to_string(),
            base07: "fbf1c7".to_string(),
            base08: "fb4934".to_string(),
            base09: "fe8019".to_string(),
            base0a: "fabd2f".to_string(),
            base0b: "b8bb26".to_string(),
            base0c: "8ec07c".to_string(),
            base0d: "83a598".to_string(),
            base0e: "d3869b".to_string(),
            base0f: "d65d0e".to_string(),
            base10: None,
            base11: None,
            base12: None,
            base13: None,
            base14: None,
            base15: None,
            base16: None,
            base17: None,
        }
    }

    fn state_with_schemes(schemes: Vec<Scheme>) -> AppState {
        let mut s = AppState::new("default".to_string());
        s.set_schemes(schemes, false);
        s
    }

    // ── rebuild_filter ────────────────────────────────────────────────────────

    #[test]
    fn filter_no_query_passes_all() {
        let schemes = vec![
            make_scheme("Gruvbox Dark", "gruvbox-dark", Some("dark")),
            make_scheme("Gruvbox Light", "gruvbox-light", Some("light")),
            make_scheme("Solarized", "solarized", None),
        ];
        let s = state_with_schemes(schemes);
        assert_eq!(s.filtered_schemes.len(), 3);
    }

    #[test]
    fn filter_search_matches_name_substring() {
        let schemes = vec![
            make_scheme("Gruvbox Dark", "gruvbox-dark", None),
            make_scheme("Nord", "nord", None),
            make_scheme("Dracula", "dracula", None),
        ];
        let mut s = state_with_schemes(schemes);
        s.search_query = "gruvbox".to_string();
        s.rebuild_filter(false);
        assert_eq!(s.filtered_schemes.len(), 1);
        assert_eq!(s.all_schemes[s.filtered_schemes[0]].slug, "gruvbox-dark");
    }

    #[test]
    fn filter_search_matches_slug() {
        let schemes = vec![
            make_scheme("One Dark", "one-dark", None),
            make_scheme("One Light", "one-light", None),
            make_scheme("Nord", "nord", None),
        ];
        let mut s = state_with_schemes(schemes);
        s.search_query = "one-".to_string();
        s.rebuild_filter(false);
        assert_eq!(s.filtered_schemes.len(), 2);
    }

    #[test]
    fn filter_search_is_case_insensitive() {
        let schemes = vec![
            make_scheme("Gruvbox Dark", "gruvbox-dark", None),
            make_scheme("Nord", "nord", None),
        ];
        let mut s = state_with_schemes(schemes);
        s.search_query = "GRUVBOX".to_string();
        s.rebuild_filter(false);
        assert_eq!(s.filtered_schemes.len(), 1);
    }

    #[test]
    fn filter_search_no_match_gives_empty() {
        let schemes = vec![
            make_scheme("Nord", "nord", None),
            make_scheme("Dracula", "dracula", None),
        ];
        let mut s = state_with_schemes(schemes);
        s.search_query = "zzz-nonexistent".to_string();
        s.rebuild_filter(false);
        assert!(s.filtered_schemes.is_empty());
    }

    #[test]
    fn filter_variant_dark_excludes_light() {
        let schemes = vec![
            make_scheme("Theme Dark", "theme-dark", Some("dark")),
            make_scheme("Theme Light", "theme-light", Some("light")),
        ];
        let mut s = AppState::new("prefer-dark".to_string());
        s.set_schemes(schemes, true); // follow_type = true
        assert_eq!(s.filtered_schemes.len(), 1);
        assert_eq!(s.all_schemes[s.filtered_schemes[0]].slug, "theme-dark");
    }

    #[test]
    fn filter_variant_passes_schemes_without_variant() {
        let schemes = vec![
            make_scheme("Has Variant", "has-variant", Some("light")),
            make_scheme("No Variant", "no-variant", None),
        ];
        let mut s = AppState::new("prefer-dark".to_string());
        s.set_schemes(schemes, true);
        // "light" variant excluded, None-variant passes through
        assert_eq!(s.filtered_schemes.len(), 1);
        assert_eq!(s.all_schemes[s.filtered_schemes[0]].slug, "no-variant");
    }

    #[test]
    fn filter_no_follow_type_ignores_variant() {
        let schemes = vec![
            make_scheme("Dark Theme", "dark-theme", Some("dark")),
            make_scheme("Light Theme", "light-theme", Some("light")),
        ];
        let mut s = AppState::new("prefer-dark".to_string());
        s.set_schemes(schemes, false); // follow_type = false — variant ignored
        assert_eq!(s.filtered_schemes.len(), 2);
    }

    // ── navigation ────────────────────────────────────────────────────────────

    #[test]
    fn move_down_advances_selection() {
        let schemes = vec![
            make_scheme("A", "a", None),
            make_scheme("B", "b", None),
            make_scheme("C", "c", None),
        ];
        let mut s = state_with_schemes(schemes);
        s.move_down(1);
        assert_eq!(s.selected_scheme_idx, 1);
    }

    #[test]
    fn move_down_clamps_at_last_item() {
        let schemes = vec![make_scheme("A", "a", None), make_scheme("B", "b", None)];
        let mut s = state_with_schemes(schemes);
        s.move_down(100);
        assert_eq!(s.selected_scheme_idx, 1);
    }

    #[test]
    fn move_up_decrements_selection() {
        let schemes = vec![
            make_scheme("A", "a", None),
            make_scheme("B", "b", None),
        ];
        let mut s = state_with_schemes(schemes);
        s.selected_scheme_idx = 1;
        s.move_up(1);
        assert_eq!(s.selected_scheme_idx, 0);
    }

    #[test]
    fn move_up_at_zero_does_not_underflow() {
        let schemes = vec![make_scheme("A", "a", None)];
        let mut s = state_with_schemes(schemes);
        s.move_up(1);
        assert_eq!(s.selected_scheme_idx, 0);
    }

    #[test]
    fn go_to_top_resets_to_zero() {
        let schemes = vec![
            make_scheme("A", "a", None),
            make_scheme("B", "b", None),
        ];
        let mut s = state_with_schemes(schemes);
        s.selected_scheme_idx = 1;
        s.go_to_top();
        assert_eq!(s.selected_scheme_idx, 0);
    }

    #[test]
    fn go_to_bottom_jumps_to_last() {
        let schemes = vec![
            make_scheme("A", "a", None),
            make_scheme("B", "b", None),
            make_scheme("C", "c", None),
        ];
        let mut s = state_with_schemes(schemes);
        s.go_to_bottom();
        assert_eq!(s.selected_scheme_idx, 2);
    }

    #[test]
    fn go_to_bottom_on_empty_stays_zero() {
        let mut s = AppState::new("default".to_string());
        s.go_to_bottom();
        assert_eq!(s.selected_scheme_idx, 0);
    }

    // ── selected_scheme ───────────────────────────────────────────────────────

    #[test]
    fn selected_scheme_returns_correct_entry() {
        let schemes = vec![
            make_scheme("A", "a", None),
            make_scheme("B", "b", None),
        ];
        let mut s = state_with_schemes(schemes);
        s.selected_scheme_idx = 1;
        assert_eq!(s.selected_scheme().map(|s| s.slug.as_str()), Some("b"));
    }

    #[test]
    fn selected_scheme_none_when_empty() {
        let s = AppState::new("default".to_string());
        assert!(s.selected_scheme().is_none());
    }

    #[test]
    fn selected_scheme_idx_clamped_when_filter_narrows() {
        let schemes = vec![
            make_scheme("Nord", "nord", None),
            make_scheme("Dracula", "dracula", None),
            make_scheme("Gruvbox", "gruvbox", None),
        ];
        let mut s = state_with_schemes(schemes);
        // Move to last item, then narrow filter to 1 result
        s.selected_scheme_idx = 2;
        s.search_query = "nord".to_string();
        s.rebuild_filter(false);
        // idx must be clamped to 0 (only 1 result)
        assert_eq!(s.selected_scheme_idx, 0);
        assert_eq!(s.selected_scheme().map(|s| s.slug.as_str()), Some("nord"));
    }

    // ── toggle_panel ──────────────────────────────────────────────────────────

    #[test]
    fn toggle_panel_switches_between_schemes_and_wallpapers() {
        let mut s = AppState::new("default".to_string());
        assert_eq!(s.active_panel, Panel::Schemes);
        s.toggle_panel();
        assert_eq!(s.active_panel, Panel::Wallpapers);
        s.toggle_panel();
        assert_eq!(s.active_panel, Panel::Schemes);
    }
}
