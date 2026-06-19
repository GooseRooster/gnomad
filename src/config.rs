use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub wallpaper_dir: PathBuf,
    pub custom_schemes_dir: Option<PathBuf>,
    #[serde(default = "default_theme_name")]
    pub theme_name: String,
    pub default_scheme: Option<String>,
    #[serde(default = "default_schemes_repo_dir")]
    pub schemes_repo_dir: PathBuf,
    #[serde(default = "default_output_wallpaper_path")]
    pub output_wallpaper_path: PathBuf,
    #[serde(default = "default_wallpaper_cache_dir")]
    pub wallpaper_cache_dir: PathBuf,
    #[serde(default = "default_true")]
    pub follow_user_scheme_type: bool,
}

fn default_theme_name() -> String {
    "gnomad".to_string()
}

fn default_schemes_repo_dir() -> PathBuf {
    data_dir().join("schemes-repo")
}

fn default_output_wallpaper_path() -> PathBuf {
    data_dir().join("current-wallpaper.png")
}

fn default_wallpaper_cache_dir() -> PathBuf {
    data_dir().join("wallpapers")
}

fn default_true() -> bool {
    true
}

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("gnomad")
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("gnomad")
        .join("config.toml")
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path();
        if !path.exists() {
            return Ok(Self::default_config());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("reading config {}", path.display()))?;
        toml::from_str(&content).context("parsing config.toml")
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
            custom_schemes_dir: None,
            theme_name: default_theme_name(),
            default_scheme: None,
            schemes_repo_dir: default_schemes_repo_dir(),
            output_wallpaper_path: default_output_wallpaper_path(),
            wallpaper_cache_dir: default_wallpaper_cache_dir(),
            follow_user_scheme_type: true,
        }
    }
}
