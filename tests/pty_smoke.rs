//! Process-level smoke test: launch the real binary in a PTY and verify the
//! terminal lifecycle works end to end.
//!
//! `TestBackend` tests cover rendering and input handling, but they bypass
//! crossterm, raw mode, alternate-screen setup, and process shutdown. This test
//! exercises those paths with fake external commands and an isolated XDG tree.

use expectrl::{process::unix::WaitStatus, Eof, Expect, Session};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tempfile::TempDir;

struct PtyEnv {
    _temp: TempDir,
    root: PathBuf,
}

impl PtyEnv {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        for dir in [
            "bin",
            "wallpapers",
            "schemes-repo/base16",
            "schemes-repo/base24",
            "custom-schemes",
            "data/out",
            ".config/gnomad",
        ] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        let env = Self { _temp: temp, root };
        env.write_tool(
            "gowall",
            r#"#!/bin/sh
exit 0
"#,
        );
        env.write_tool(
            "gsettings",
            r#"#!/bin/sh
if [ "$1" = "get" ]; then echo "''"; fi
exit 0
"#,
        );
        env.write_tool("gnome-extensions", "#!/bin/sh\nexit 0\n");
        env.write_tool("tinty", "#!/bin/sh\nexit 0\n");

        fs::write(
            env.root.join("custom-schemes/test-scheme.yaml"),
            r#"system: base16
name: Test Scheme
author: Test
variant: dark
palette:
  base00: "1d2021"
  base01: "282828"
  base02: "3c3836"
  base03: "504945"
  base04: "665c54"
  base05: "d5c4a1"
  base06: "ebdbb2"
  base07: "fbf1c7"
  base08: "fb4934"
  base09: "fe8019"
  base0A: "fabd2f"
  base0B: "b8bb26"
  base0C: "8ec07c"
  base0D: "83a598"
  base0E: "d3869b"
  base0F: "d65d0e"
"#,
        )
        .unwrap();
        fs::write(
            env.root.join(".config/gnomad/config.toml"),
            format!(
                r#"wallpaper_dir = "{}"
custom_schemes_dir = "{}"
schemes_repo_dir = "{}"
theme_name = "test-theme"
default_scheme = "test-scheme"
wallpaper_enabled = true
gowall_only = true
"#,
                env.root.join("wallpapers").display(),
                env.root.join("custom-schemes").display(),
                env.root.join("schemes-repo").display(),
            ),
        )
        .unwrap();
        env
    }

    fn write_tool(&self, name: &str, body: &str) {
        let path = self.root.join("bin").join(name);
        fs::write(&path, body).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_gnomad"));
        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let mut path = self.root.join("bin").into_os_string();
        path.push(":");
        path.push(old_path);
        cmd.env("HOME", &self.root)
            .env("XDG_CONFIG_HOME", self.root.join(".config"))
            .env("XDG_DATA_HOME", self.root.join(".local/share"))
            .env("XDG_CACHE_HOME", self.root.join(".cache"))
            .env("PATH", path)
            .env("TERM", "xterm-256color")
            .env("GNOMAD_DISABLE_IMAGE_PREVIEW", "1")
            .env(
                "GNOMAD_GSETTINGS_BIN",
                self.root.join("bin").join("gsettings"),
            );
        cmd
    }
}

fn has_file(path: &Path) -> bool {
    path.exists()
}

#[test]
fn tui_starts_renders_and_quits_cleanly() {
    let env = PtyEnv::new();
    let mut session = Session::spawn(env.command()).expect("spawn gnomad in a PTY");
    session.set_expect_timeout(Some(Duration::from_secs(8)));

    session
        .expect("SCHEMES")
        .expect("TUI rendered the scheme panel");
    session.send("q").expect("send quit key");
    session.expect(Eof).expect("TUI exited after q");

    let status = session.get_process().wait().expect("wait for gnomad");
    assert!(
        matches!(status, WaitStatus::Exited(_, 0)),
        "unexpected exit status: {status:?}"
    );

    // Sanity-check the isolated environment was actually used.
    assert!(!has_file(&env.root.join(".config/gtk-3.0/gtk.css")));
    assert!(!has_file(&env.root.join(
        ".local/share/themes/test-theme/gnome-shell/gnome-shell.css"
    )));
}
