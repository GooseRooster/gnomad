use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Palette pre-processing applied before gowall converts an image.
///
/// gowall has no conversion-strength knobs of its own, so gnomad transforms
/// the palette in OKLCH and blends the converted image back toward the
/// original according to `intensity`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GowallTuning {
    /// Hue rotation in degrees (-180..=180).
    #[serde(default)]
    pub hue_shift: i32,
    /// Chroma/saturation scale, 100 = unchanged (0..=200).
    #[serde(default = "default_tuning_100")]
    pub saturation: i32,
    /// Lightness scale, 100 = unchanged (0..=200).
    #[serde(default = "default_tuning_100")]
    pub lightness: i32,
    /// Contrast around OKLab L=0.5 (-100..=100).
    #[serde(default)]
    pub contrast: i32,
    /// Lightness gamma, 100 = unchanged (50..=200).
    #[serde(default = "default_tuning_100")]
    pub gamma: i32,
    /// Blend the converted image back toward the original: 0 = original,
    /// 100 = full conversion.
    #[serde(default = "default_tuning_100")]
    pub intensity: i32,
    /// Keep low-chroma colours neutral when rotating hue / scaling chroma.
    #[serde(default = "default_true")]
    pub preserve_neutrals: bool,
}

impl Default for GowallTuning {
    fn default() -> Self {
        Self {
            hue_shift: 0,
            saturation: 100,
            lightness: 100,
            contrast: 0,
            gamma: 100,
            intensity: 100,
            preserve_neutrals: true,
        }
    }
}

impl GowallTuning {
    /// True when the tuning has no effect on the palette or output image.
    pub fn is_identity(&self) -> bool {
        self.hue_shift == 0
            && self.saturation == 100
            && self.lightness == 100
            && self.contrast == 0
            && self.gamma == 100
            && self.intensity == 100
    }

    /// Stable, dependency-free fingerprint used in cache keys.
    pub fn fingerprint(&self) -> String {
        let material = format!(
            "{}|{}|{}|{}|{}|{}|{}",
            self.hue_shift,
            self.saturation,
            self.lightness,
            self.contrast,
            self.gamma,
            self.intensity,
            self.preserve_neutrals,
        );
        let mut h: u64 = 14695981039346656037;
        for b in material.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        format!("{h:016x}")
    }

    /// Clamp all fields to their supported ranges.
    pub fn clamped(&self) -> Self {
        Self {
            hue_shift: self.hue_shift.clamp(-180, 180),
            saturation: self.saturation.clamp(0, 200),
            lightness: self.lightness.clamp(0, 200),
            contrast: self.contrast.clamp(-100, 100),
            gamma: self.gamma.clamp(50, 200),
            intensity: self.intensity.clamp(0, 100),
            preserve_neutrals: self.preserve_neutrals,
        }
    }
}

fn default_tuning_100() -> i32 {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub wallpaper_dir: PathBuf,
    #[serde(default = "default_custom_schemes_dir")]
    pub custom_schemes_dir: PathBuf,
    #[serde(default = "default_theme_name")]
    pub theme_name: String,
    pub default_scheme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_wallpaper: Option<PathBuf>,
    #[serde(default = "default_schemes_repo_dir")]
    pub schemes_repo_dir: PathBuf,
    #[serde(default = "default_output_wallpaper_path")]
    pub output_wallpaper_path: PathBuf,
    #[serde(default = "default_wallpaper_cache_dir")]
    pub wallpaper_cache_dir: PathBuf,
    #[serde(default = "default_true")]
    pub follow_user_scheme_type: bool,
    #[serde(default)]
    pub wallpaper_enabled: bool,
    /// Gowall-only mode: only convert + set wallpapers. Tinty, GTK CSS,
    /// GNOME Shell CSS, PaperWM and Steam integration are left untouched.
    #[serde(default)]
    pub gowall_only: bool,
    /// Palette pre-processing and conversion intensity for gowall.
    #[serde(default)]
    pub gowall_tuning: GowallTuning,
    #[serde(default)]
    pub adwaita_steam_enabled: bool,
    /// Theme PaperWM (workspace bar, minimap, workspace colours) to match the
    /// applied scheme. Requires the PaperWM extension to be installed.
    #[serde(default)]
    pub paperwm_enabled: bool,
    /// How long each wallpaper is displayed before the crossfade begins (seconds).
    #[serde(default = "default_slideshow_static_secs")]
    pub slideshow_static_secs: u64,
    /// Duration of each crossfade transition (seconds).
    /// GNOME Shell's compositor updates wallpaper opacity at most once per second
    /// (ANIMATION_MIN_WAKEUP_INTERVAL = 1.0 in background.js), incrementing by
    /// ~1.6% per tick. Long transitions (≥1800 s) make each tick imperceptible;
    /// short transitions (≤60 s) produce visible 1fps stepping.
    #[serde(default = "default_slideshow_transition_secs")]
    pub slideshow_transition_secs: u64,
    /// Milliseconds to wait after the wallpaper has been set before the
    /// `on_wallpaper_apply` hook runs.
    ///
    /// Wallpaper-reactive consumers (GNOME Shell extensions such as
    /// ChromaLeon) watch `org.gnome.desktop.background` and re-derive their
    /// colours asynchronously. A hook that fires immediately would read the
    /// *previous* accent, so the settle window is what lets the
    /// "wallpaper changed -> theme followed" chain work end to end.
    /// Set to 0 to run the hook immediately.
    #[serde(default = "default_wallpaper_settle_ms")]
    pub wallpaper_settle_ms: u64,
    #[serde(default)]
    pub hooks: HooksConfig,
}

fn default_theme_name() -> String {
    "gnomad".to_string()
}

fn default_schemes_repo_dir() -> PathBuf {
    data_dir().join("schemes-repo")
}

fn default_custom_schemes_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("gnomad")
        .join("schemes")
}

fn default_output_wallpaper_path() -> PathBuf {
    data_dir().join("current-wallpaper.png")
}

fn default_wallpaper_cache_dir() -> PathBuf {
    data_dir().join("wallpapers")
}

/// Expand a leading `~` or `~/` using `home`. Other `~` uses are left alone;
/// only the conventional home-directory shorthand is supported.
fn expand_tilde_with_home(path: PathBuf, home: &Path) -> PathBuf {
    match path.to_str() {
        Some("~") => home.to_path_buf(),
        Some(rest) => match rest.strip_prefix("~/") {
            Some(suffix) => home.join(suffix),
            None => path,
        },
        None => path,
    }
}

fn expand_tilde(path: PathBuf) -> PathBuf {
    match dirs::home_dir() {
        Some(home) => expand_tilde_with_home(path, &home),
        None => path,
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_scheme_apply: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_wallpaper_apply: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_wallpaper_batch_convert: Option<String>,
}

fn default_slideshow_static_secs() -> u64 {
    3600
}

fn default_slideshow_transition_secs() -> u64 {
    1800
}

fn default_wallpaper_settle_ms() -> u64 {
    1500
}

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("gnomad")
}

/// Disposable cache root (managed gowall HOME + downscaled previews).
pub fn cache_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(data_dir).join("gnomad")
}

/// Gnomad-owned gowall HOME/output tree. Keeping gowall's CLUT cache here
/// prevents slider tuning from littering `~/Pictures/gowall/cluts`.
pub fn gowall_cache_dir() -> PathBuf {
    cache_dir().join("gowall")
}

/// Downscaled conversion previews.
pub fn preview_cache_dir() -> PathBuf {
    cache_dir().join("previews")
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("gnomad")
        .join("config.toml")
}

impl Config {
    /// Wallpaper features are active in full mode only when opted in, but
    /// gowall-only mode is inherently a wallpaper workflow.
    pub fn wallpaper_features_enabled(&self) -> bool {
        self.wallpaper_enabled || self.gowall_only
    }

    pub fn load() -> Result<Self> {
        let path = config_path();
        if !path.exists() {
            return Ok(Self::default_config());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let mut config: Config = toml::from_str(&content).context("parsing config.toml")?;
        config.expand_paths();
        Ok(config)
    }

    /// Expand `~` shorthand in all user-configurable filesystem paths.
    /// TOML has no shell expansion, and gowall/gsettings treat a leading `~`
    /// literally — which used to split reads and writes across two trees.
    fn expand_paths(&mut self) {
        self.wallpaper_dir = expand_tilde(std::mem::take(&mut self.wallpaper_dir));
        self.custom_schemes_dir = expand_tilde(std::mem::take(&mut self.custom_schemes_dir));
        self.schemes_repo_dir = expand_tilde(std::mem::take(&mut self.schemes_repo_dir));
        self.output_wallpaper_path = expand_tilde(std::mem::take(&mut self.output_wallpaper_path));
        self.wallpaper_cache_dir = expand_tilde(std::mem::take(&mut self.wallpaper_cache_dir));
        if let Some(last) = self.last_wallpaper.take() {
            self.last_wallpaper = Some(expand_tilde(last));
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self).context("serializing config")?;
        std::fs::write(&path, content)
            .with_context(|| format!("writing config to {}", path.display()))
    }

    fn default_config() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
        Self {
            wallpaper_dir: home.join("Pictures").join("Wallpapers"),
            custom_schemes_dir: default_custom_schemes_dir(),
            theme_name: default_theme_name(),
            default_scheme: None,
            last_wallpaper: None,
            schemes_repo_dir: default_schemes_repo_dir(),
            output_wallpaper_path: default_output_wallpaper_path(),
            wallpaper_cache_dir: default_wallpaper_cache_dir(),
            follow_user_scheme_type: true,
            wallpaper_enabled: false,
            gowall_only: false,
            gowall_tuning: GowallTuning::default(),
            adwaita_steam_enabled: false,
            paperwm_enabled: false,
            slideshow_static_secs: default_slideshow_static_secs(),
            slideshow_transition_secs: default_slideshow_transition_secs(),
            wallpaper_settle_ms: default_wallpaper_settle_ms(),
            hooks: HooksConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;
    use serial_test::serial;
    use std::fs;

    #[test]
    fn expand_tilde_only_touches_leading_home_shorthand() {
        let home = Path::new("/home/tester");
        assert_eq!(
            expand_tilde_with_home(PathBuf::from("~/Pictures"), home),
            PathBuf::from("/home/tester/Pictures")
        );
        assert_eq!(
            expand_tilde_with_home(PathBuf::from("~"), home),
            PathBuf::from("/home/tester")
        );
        assert_eq!(
            expand_tilde_with_home(PathBuf::from("/absolute"), home),
            PathBuf::from("/absolute")
        );
        assert_eq!(
            expand_tilde_with_home(PathBuf::from("relative/path"), home),
            PathBuf::from("relative/path")
        );
        assert_eq!(
            expand_tilde_with_home(PathBuf::from("~user/path"), home),
            PathBuf::from("~user/path")
        );
    }

    #[test]
    #[serial]
    fn load_expands_tilde_in_every_path_field() {
        let env = TestEnv::new();
        let config_dir = env.root.join(".config/gnomad");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(
            config_dir.join("config.toml"),
            r#"
wallpaper_dir = "~/Pictures/Wallpapers"
custom_schemes_dir = "~/schemes"
last_wallpaper = "~/Pictures/Wallpapers/a.png"
schemes_repo_dir = "~/schemes-repo"
output_wallpaper_path = "~/data/out/current-wallpaper.png"
wallpaper_cache_dir = "~/data/cache"
theme_name = "test-theme"
default_scheme = "test-scheme"
"#,
        )
        .unwrap();

        let config = Config::load().unwrap();
        assert_eq!(config.wallpaper_dir, env.root.join("Pictures/Wallpapers"));
        assert_eq!(config.custom_schemes_dir, env.root.join("schemes"));
        assert_eq!(
            config.last_wallpaper,
            Some(env.root.join("Pictures/Wallpapers/a.png"))
        );
        assert_eq!(config.schemes_repo_dir, env.root.join("schemes-repo"));
        assert_eq!(
            config.output_wallpaper_path,
            env.root.join("data/out/current-wallpaper.png")
        );
        assert_eq!(config.wallpaper_cache_dir, env.root.join("data/cache"));

        config.save().unwrap();
        let saved = fs::read_to_string(config_dir.join("config.toml")).unwrap();
        assert!(!saved.contains("~/"), "config was not normalised:\n{saved}");
    }

    #[test]
    fn gowall_only_implies_wallpaper_features() {
        let mut config = Config::default_config();
        assert!(!config.wallpaper_features_enabled());
        config.gowall_only = true;
        assert!(config.wallpaper_features_enabled());
    }

    #[test]
    fn missing_tuning_fields_default_to_identity() {
        let tuning: GowallTuning = serde_yaml::from_str("hue_shift: 12\n").unwrap();
        assert_eq!(tuning.hue_shift, 12);
        assert_eq!(tuning.saturation, 100);
        assert_eq!(tuning.intensity, 100);
        assert!(tuning.preserve_neutrals);
    }
}
