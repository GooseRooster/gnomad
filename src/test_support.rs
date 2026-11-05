//! Test-only helpers: isolated environment, fake external commands, and a
//! `TestBackend`-driven UI harness.

use crate::app::App;
use crate::config::Config;
use crate::schemes::types::{Scheme, SchemeSystem};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

const ENV_VARS: &[&str] = &[
    "HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "PATH",
    "GNOMAD_TEST_LOG",
    "GNOMAD_DISABLE_IMAGE_PREVIEW",
    "GNOMAD_GSETTINGS_BIN",
];

pub struct TestEnv {
    _temp: TempDir,
    pub root: PathBuf,
    old_env: Vec<(&'static str, Option<OsString>)>,
}

impl TestEnv {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().to_path_buf();
        let old_env = ENV_VARS
            .iter()
            .map(|&key| (key, std::env::var_os(key)))
            .collect::<Vec<_>>();

        for dir in [
            "bin",
            "wallpapers",
            "schemes-repo/base16",
            "schemes-repo/base24",
            "custom-schemes",
            "data/out",
        ] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }

        std::env::set_var("HOME", &root);
        std::env::set_var("XDG_CONFIG_HOME", root.join(".config"));
        std::env::set_var("XDG_DATA_HOME", root.join(".local/share"));
        std::env::set_var("XDG_CACHE_HOME", root.join(".cache"));
        std::env::set_var("GNOMAD_TEST_LOG", root.join("calls.log"));
        std::env::set_var("GNOMAD_DISABLE_IMAGE_PREVIEW", "1");
        std::env::set_var("GNOMAD_GSETTINGS_BIN", root.join("bin/gsettings"));

        let old_path = old_env
            .iter()
            .find(|(key, _)| *key == "PATH")
            .and_then(|(_, value)| value.clone())
            .unwrap_or_default();
        let mut new_path = OsString::from(root.join("bin"));
        new_path.push(":");
        new_path.push(old_path);
        std::env::set_var("PATH", new_path);

        let env = Self {
            _temp: temp,
            root,
            old_env,
        };
        env.write_fake_tools();
        env
    }

    fn write_fake_tools(&self) {
        let gowall = r#"#!/bin/sh
echo "gowall $*" >> "$GNOMAD_TEST_LOG"
test_root=$(dirname "$GNOMAD_TEST_LOG")
if [ -f "$test_root/gowall.fail" ]; then
  exit 1
fi
if [ "$2" = "--dir" ] || [ "$2" = "--batch" ]; then
  dir="$3"
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "--output" ]; then out="$arg"; fi
    prev="$arg"
  done
  if [ -n "$out" ] && [ -d "$dir" ]; then
    mkdir -p "$out"
    cp "$dir"/* "$out"/ 2>/dev/null || true
    if [ -f "$test_root/gowall.skip" ]; then
      skip=$(cat "$test_root/gowall.skip")
      rm -f "$out/$skip"
    fi
  fi
  exit 0
fi
input="$2"
out=""
prev=""
for arg in "$@"; do
  if [ "$prev" = "--output" ]; then out="$arg"; fi
  prev="$arg"
done
if [ -n "$out" ]; then
  mkdir -p "$(dirname "$out")"
  cp "$input" "$out"
fi
exit 0
"#;
        self.write_tool("gowall", gowall);

        self.write_tool(
            "tinty",
            r#"#!/bin/sh
echo "tinty $*" >> "$GNOMAD_TEST_LOG"
exit 0
"#,
        );
        self.write_tool(
            "gsettings",
            r#"#!/bin/sh
echo "gsettings $*" >> "$GNOMAD_TEST_LOG"
if [ "$1" = "get" ]; then
  case "$3" in
    color-scheme) echo "'prefer-dark'" ;;
    *) echo "''" ;;
  esac
fi
exit 0
"#,
        );
        self.write_tool(
            "gnome-extensions",
            r#"#!/bin/sh
echo "gnome-extensions $*" >> "$GNOMAD_TEST_LOG"
exit 0
"#,
        );
    }

    fn write_tool(&self, name: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = self.root.join("bin").join(name);
        fs::write(&path, body).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
    }

    pub fn config(&self) -> Config {
        let mut config = Config::load().expect("load config");
        config.wallpaper_dir = self.root.join("wallpapers");
        config.custom_schemes_dir = self.root.join("custom-schemes");
        config.schemes_repo_dir = self.root.join("schemes-repo");
        config.wallpaper_cache_dir = self.root.join("data/cache");
        config.output_wallpaper_path = self.root.join("data/out/current-wallpaper.png");
        config.theme_name = "test-theme".to_string();
        config
    }

    pub fn wallpaper(&self, name: &str) -> PathBuf {
        let path = self.root.join("wallpapers").join(name);
        fs::write(&path, b"fake-image").unwrap();
        path
    }

    pub fn calls(&self) -> String {
        fs::read_to_string(self.root.join("calls.log")).unwrap_or_default()
    }

    pub fn clear_calls(&self) {
        let _ = fs::remove_file(self.root.join("calls.log"));
    }

    /// Shell command that writes the hook environment to a file under the
    /// test root. Hooks run through `sh -c`, so this exercises the real path.
    pub fn hook_command(&self, name: &str) -> String {
        format!("env > '{}'", self.root.join(name).display())
    }

    /// Read a file produced by [`TestEnv::hook_command`].
    pub fn read_hook_env(&self, name: &str) -> HashMap<String, String> {
        let path = self.root.join(name);
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read hook env file {}: {e}", path.display()));
        content
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    pub fn hook_ran(&self, name: &str) -> bool {
        self.root.join(name).exists()
    }

    pub fn gtk_css(&self) -> PathBuf {
        self.root.join(".config/gtk-3.0/gtk.css")
    }

    pub fn shell_css(&self) -> PathBuf {
        self.root
            .join(".local/share/themes/test-theme/gnome-shell/gnome-shell.css")
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        for (key, value) in &self.old_env {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

pub fn test_scheme() -> Scheme {
    Scheme {
        system: SchemeSystem::Base16,
        name: "Test Scheme".to_string(),
        slug: "test-scheme".to_string(),
        author: "Test".to_string(),
        variant: Some("dark".to_string()),
        is_custom: false,
        base00: "1d2021".into(),
        base01: "282828".into(),
        base02: "3c3836".into(),
        base03: "504945".into(),
        base04: "665c54".into(),
        base05: "d5c4a1".into(),
        base06: "ebdbb2".into(),
        base07: "fbf1c7".into(),
        base08: "fb4934".into(),
        base09: "fe8019".into(),
        base0a: "fabd2f".into(),
        base0b: "b8bb26".into(),
        base0c: "8ec07c".into(),
        base0d: "83a598".into(),
        base0e: "d3869b".into(),
        base0f: "d65d0e".into(),
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

pub struct TestApp {
    pub app: App,
    terminal: Terminal<TestBackend>,
}

impl TestApp {
    pub fn new(config: Config) -> Self {
        let app = App::new(config, "prefer-dark".to_string(), None);
        let terminal = Terminal::new(TestBackend::new(120, 40)).expect("test terminal");
        Self { app, terminal }
    }

    pub fn draw(&mut self) {
        let status = self.app.state.animation_status.borrow().clone();
        let app = &mut self.app;
        self.terminal
            .draw(|frame| app.render(frame, &status))
            .expect("draw");
    }

    pub async fn send(&mut self, code: KeyCode) -> bool {
        self.app
            .handle_key(KeyEvent::new(code, KeyModifiers::NONE))
            .await
            .expect("handle key")
    }

    pub fn screen(&self) -> String {
        let buffer = self.terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    /// Snapshot-friendly screen dump: same text as [`TestApp::screen`], but
    /// with trailing blank-cell padding removed from each line.
    pub fn snapshot_screen(&self) -> String {
        self.screen()
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn contains(&self, needle: &str) -> bool {
        self.screen().contains(needle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[tokio::test]
    #[serial]
    async fn gowall_only_ui_and_tuning_overlay() {
        let env = TestEnv::new();
        let mut config = env.config();
        config.gowall_only = true;
        config.wallpaper_enabled = true;
        config.save().unwrap();

        let mut harness = TestApp::new(config);
        harness.app.state.set_schemes(vec![test_scheme()], false);
        harness.draw();

        assert!(harness.contains("gowall-only"));
        assert!(harness.contains("SCHEMES (1)"));
        assert!(harness.contains("[t] Tuning"));
        assert!(!harness.contains("GOWALL TUNING"));

        harness.send(KeyCode::Char('t')).await;
        harness.draw();
        assert!(harness.contains("GOWALL TUNING"));
        assert!(harness.contains("Intensity"));
        assert!(harness.contains("Neutrals"));

        harness.send(KeyCode::Right).await;
        assert_eq!(harness.app.config.gowall_tuning.hue_shift, 5);

        harness.send(KeyCode::Esc).await;
        harness.draw();
        assert!(!harness.contains("GOWALL TUNING"));
    }

    #[tokio::test]
    #[serial]
    async fn panel_switch_and_search_work() {
        let env = TestEnv::new();
        let mut config = env.config();
        config.gowall_only = true;
        config.save().unwrap();
        env.wallpaper("sample.jpg");

        let mut harness = TestApp::new(config);
        harness.app.state.set_schemes(vec![test_scheme()], false);
        harness.draw();
        assert!(harness.contains("SCHEMES"));

        harness.send(KeyCode::Tab).await;
        harness.draw();
        assert!(harness.contains("WALLPAPERS"));

        harness.send(KeyCode::Tab).await;
        harness.send(KeyCode::Char('/')).await;
        harness.send(KeyCode::Char('t')).await;
        harness.send(KeyCode::Char('e')).await;
        harness.send(KeyCode::Char('s')).await;
        harness.send(KeyCode::Char('t')).await;
        harness.draw();
        assert!(harness.contains("SCHEMES / test"));
    }

    #[tokio::test]
    #[serial]
    async fn small_terminal_renders_without_panicking() {
        let env = TestEnv::new();
        let mut config = env.config();
        config.gowall_only = true;
        config.save().unwrap();

        let mut harness = TestApp::new(config);
        harness.app.state.set_schemes(vec![test_scheme()], false);
        harness.terminal = Terminal::new(TestBackend::new(60, 18)).unwrap();
        harness.draw();
        assert!(harness.contains("gnomad"));
    }

    fn snapshot_config(env: &TestEnv, gowall_only: bool, wallpaper_enabled: bool) -> Config {
        let mut config = env.config();
        config.gowall_only = gowall_only;
        config.wallpaper_enabled = wallpaper_enabled;
        config.save().unwrap();
        config
    }

    fn scheme_snapshot_harness(env: &TestEnv, gowall_only: bool) -> TestApp {
        let config = snapshot_config(env, gowall_only, true);
        let mut harness = TestApp::new(config);
        harness.app.state.set_schemes(vec![test_scheme()], false);
        harness
    }

    #[tokio::test]
    #[serial]
    async fn snapshot_scheme_browser_gowall_only() {
        let env = TestEnv::new();
        let mut harness = scheme_snapshot_harness(&env, true);
        harness.draw();

        insta::assert_snapshot!("scheme_browser_gowall_only", harness.snapshot_screen());
    }

    #[tokio::test]
    #[serial]
    async fn snapshot_scheme_browser_full_mode() {
        let env = TestEnv::new();
        let mut harness = scheme_snapshot_harness(&env, false);
        harness.draw();

        insta::assert_snapshot!("scheme_browser_full_mode", harness.snapshot_screen());
    }

    #[tokio::test]
    #[serial]
    async fn snapshot_wallpaper_panel() {
        let env = TestEnv::new();
        let mut config = snapshot_config(&env, true, true);
        // The panel title renders the wallpaper directory. Use a fixed display
        // path (it is not read here) so the box width/dash layout does not
        // depend on the temp root.
        config.wallpaper_dir = PathBuf::from("/tmp/gnomad-test-wallpapers");
        let alpha = env.wallpaper("alpha.jpg");
        let beta = env.wallpaper("beta.jpg");

        let mut harness = TestApp::new(config);
        harness.app.state.set_schemes(vec![test_scheme()], false);
        harness.app.state.active_scheme = Some(test_scheme());
        harness.app.state.wallpapers = vec![alpha.clone(), beta];
        harness.app.state.current_wallpaper = Some(alpha);

        harness.send(KeyCode::Tab).await;
        harness.draw();

        insta::assert_snapshot!("wallpaper_panel", harness.snapshot_screen());
    }

    #[tokio::test]
    #[serial]
    async fn snapshot_search_active() {
        let env = TestEnv::new();
        let mut harness = scheme_snapshot_harness(&env, true);

        harness.send(KeyCode::Char('/')).await;
        for c in "test".chars() {
            harness.send(KeyCode::Char(c)).await;
        }
        harness.draw();

        insta::assert_snapshot!("scheme_search_active", harness.snapshot_screen());
    }

    #[tokio::test]
    #[serial]
    async fn snapshot_tuning_overlay() {
        let env = TestEnv::new();
        let mut harness = scheme_snapshot_harness(&env, true);

        harness.send(KeyCode::Char('t')).await;
        harness.draw();

        insta::assert_snapshot!("tuning_overlay", harness.snapshot_screen());
    }

    #[tokio::test]
    #[serial]
    async fn snapshot_small_terminal() {
        let env = TestEnv::new();
        let mut harness = scheme_snapshot_harness(&env, true);
        harness.terminal = Terminal::new(TestBackend::new(60, 18)).unwrap();
        harness.draw();

        insta::assert_snapshot!("small_terminal", harness.snapshot_screen());
    }

    #[tokio::test]
    #[serial]
    async fn snapshot_error_status() {
        let env = TestEnv::new();
        let mut harness = scheme_snapshot_harness(&env, true);
        harness.app.state.last_error = Some("could not apply test-scheme".to_string());
        harness.draw();

        insta::assert_snapshot!("error_status", harness.snapshot_screen());
    }
}
