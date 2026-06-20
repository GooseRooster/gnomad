use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum SchemeSystem {
    Base16,
    Base24,
}

impl SchemeSystem {
    pub fn tag(&self, is_prefix: bool) -> &'static str {
        match self {
            SchemeSystem::Base16 => match is_prefix {
                true => "base16",
                false => "b16",
            },
            SchemeSystem::Base24 => match is_prefix {
                true => "base24",
                false => "b24",
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct Scheme {
    pub system: SchemeSystem,
    pub name: String,
    pub slug: String,
    pub author: String,
    pub variant: Option<String>,
    pub is_custom: bool,
    // base16 slots (no # prefix)
    pub base00: String,
    pub base01: String,
    pub base02: String,
    pub base03: String,
    pub base04: String,
    pub base05: String,
    pub base06: String,
    pub base07: String,
    pub base08: String,
    pub base09: String,
    pub base0a: String,
    pub base0b: String,
    pub base0c: String,
    pub base0d: String,
    pub base0e: String,
    pub base0f: String,
    // base24 extension
    pub base10: Option<String>,
    pub base11: Option<String>,
    pub base12: Option<String>,
    pub base13: Option<String>,
    pub base14: Option<String>,
    pub base15: Option<String>,
    pub base16: Option<String>,
    pub base17: Option<String>,
}

// ── YAML deserialization helpers ──────────────────────────────────────────────

#[derive(Deserialize)]
struct NewFormatYaml {
    system: Option<String>,
    name: String,
    author: String,
    variant: Option<String>,
    palette: HashMap<String, String>,
}

#[derive(Deserialize)]
struct LegacyFormatYaml {
    scheme: String,
    author: String,
    #[serde(flatten)]
    slots: HashMap<String, String>,
}

pub fn parse_scheme_yaml(yaml_str: &str, path: &Path, is_custom: bool) -> Result<Scheme> {
    let slug = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Detect format by presence of `palette:` key
    if yaml_str.contains("palette:") {
        parse_new_format(yaml_str, slug, is_custom)
    } else {
        parse_legacy_format(yaml_str, slug, is_custom)
    }
}

fn parse_new_format(yaml_str: &str, slug: String, is_custom: bool) -> Result<Scheme> {
    let raw: NewFormatYaml = serde_yaml::from_str(yaml_str).context("parsing new-format YAML")?;

    let system = match raw.system.as_deref() {
        Some("base24") => SchemeSystem::Base24,
        _ => SchemeSystem::Base16,
    };

    let p = &raw.palette;
    let get = |key: &str| -> Result<String> {
        p.get(key)
            .map(|s| s.trim_start_matches('#').to_lowercase())
            .ok_or_else(|| anyhow::anyhow!("missing palette slot {key}"))
    };
    let get_opt = |key: &str| -> Option<String> {
        p.get(key).map(|s| s.trim_start_matches('#').to_lowercase())
    };

    Ok(Scheme {
        system,
        name: raw.name,
        slug,
        author: raw.author,
        variant: raw.variant,
        is_custom,
        base00: get("base00")?,
        base01: get("base01")?,
        base02: get("base02")?,
        base03: get("base03")?,
        base04: get("base04")?,
        base05: get("base05")?,
        base06: get("base06")?,
        base07: get("base07")?,
        base08: get("base08")?,
        base09: get("base09")?,
        base0a: get("base0A").or_else(|_| get("base0a"))?,
        base0b: get("base0B").or_else(|_| get("base0b"))?,
        base0c: get("base0C").or_else(|_| get("base0c"))?,
        base0d: get("base0D").or_else(|_| get("base0d"))?,
        base0e: get("base0E").or_else(|_| get("base0e"))?,
        base0f: get("base0F").or_else(|_| get("base0f"))?,
        base10: get_opt("base10"),
        base11: get_opt("base11"),
        base12: get_opt("base12"),
        base13: get_opt("base13"),
        base14: get_opt("base14"),
        base15: get_opt("base15"),
        base16: get_opt("base16"),
        base17: get_opt("base17"),
    })
}

fn parse_legacy_format(yaml_str: &str, slug: String, is_custom: bool) -> Result<Scheme> {
    let raw: LegacyFormatYaml =
        serde_yaml::from_str(yaml_str).context("parsing legacy-format YAML")?;

    // Determine system from slug prefix
    let system = if slug.starts_with("base24") {
        SchemeSystem::Base24
    } else {
        SchemeSystem::Base16
    };

    let s = &raw.slots;
    let get = |key: &str| -> Result<String> {
        s.get(key)
            .map(|v| v.trim_start_matches('#').to_lowercase())
            .ok_or_else(|| anyhow::anyhow!("missing slot {key}"))
    };
    let get_opt = |key: &str| -> Option<String> {
        s.get(key).map(|v| v.trim_start_matches('#').to_lowercase())
    };

    // Legacy format uses uppercase hex keys (base0A, base0B, etc.)
    let base0a = get("base0A").or_else(|_| get("base0a"))?;
    let base0b = get("base0B").or_else(|_| get("base0b"))?;
    let base0c = get("base0C").or_else(|_| get("base0c"))?;
    let base0d = get("base0D").or_else(|_| get("base0d"))?;
    let base0e = get("base0E").or_else(|_| get("base0e"))?;
    let base0f = get("base0F").or_else(|_| get("base0f"))?;

    if base0a.is_empty() {
        bail!("missing required base0A slot");
    }

    Ok(Scheme {
        system,
        name: raw.scheme,
        slug,
        author: raw.author,
        variant: None,
        is_custom,
        base00: get("base00")?,
        base01: get("base01")?,
        base02: get("base02")?,
        base03: get("base03")?,
        base04: get("base04")?,
        base05: get("base05")?,
        base06: get("base06")?,
        base07: get("base07")?,
        base08: get("base08")?,
        base09: get("base09")?,
        base0a,
        base0b,
        base0c,
        base0d,
        base0e,
        base0f,
        base10: get_opt("base10"),
        base11: get_opt("base11"),
        base12: get_opt("base12"),
        base13: get_opt("base13"),
        base14: get_opt("base14"),
        base15: get_opt("base15"),
        base16: get_opt("base16"),
        base17: get_opt("base17"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_new_format() {
        let yaml = r#"
system: "base16"
name: "Test"
author: "Tester"
variant: "dark"
palette:
  base00: "1d2021"
  base01: "3c3836"
  base02: "504945"
  base03: "665c54"
  base04: "bdae93"
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
"#;
        let s = parse_scheme_yaml(yaml, &PathBuf::from("base16-test.yaml"), false).unwrap();
        assert_eq!(s.name, "Test");
        assert_eq!(s.slug, "base16-test");
        assert_eq!(s.base00, "1d2021");
        assert_eq!(s.base0a, "fabd2f");
        assert!(matches!(s.system, SchemeSystem::Base16));
        assert_eq!(s.variant.as_deref(), Some("dark"));
    }

    #[test]
    fn parse_legacy_format() {
        let yaml = r#"
scheme: "Gruvbox Dark"
author: "Dawid Kurek"
base00: "1d2021"
base01: "3c3836"
base02: "504945"
base03: "665c54"
base04: "bdae93"
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
"#;
        let s = parse_scheme_yaml(yaml, &PathBuf::from("base16-gruvbox-dark.yaml"), false).unwrap();
        assert_eq!(s.name, "Gruvbox Dark");
        assert_eq!(s.base0a, "fabd2f");
        assert!(matches!(s.system, SchemeSystem::Base16));
        assert!(s.variant.is_none());
    }

    #[test]
    fn parse_base24() {
        let yaml = r#"
system: "base24"
name: "Test24"
author: "Tester"
palette:
  base00: "000000"
  base01: "111111"
  base02: "222222"
  base03: "333333"
  base04: "444444"
  base05: "555555"
  base06: "666666"
  base07: "777777"
  base08: "880000"
  base09: "885500"
  base0A: "888800"
  base0B: "008800"
  base0C: "008888"
  base0D: "000088"
  base0E: "880088"
  base0F: "884400"
  base10: "aaaaaa"
  base11: "bbbbbb"
  base12: "cc0000"
  base13: "cccc00"
  base14: "00cc00"
  base15: "00cccc"
  base16: "0000cc"
  base17: "cc00cc"
"#;
        let s = parse_scheme_yaml(yaml, &PathBuf::from("base24-test.yaml"), false).unwrap();
        assert!(matches!(s.system, SchemeSystem::Base24));
        assert_eq!(s.base10.as_deref(), Some("aaaaaa"));
        assert_eq!(s.base17.as_deref(), Some("cc00cc"));
    }
}
