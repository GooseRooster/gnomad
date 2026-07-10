use crate::pipeline::{gnome::GnomeInterface, wallpaper_cache};
use crate::schemes::types::Scheme;
use crate::config::data_dir;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[allow(clippy::too_many_arguments)]
pub async fn create_slideshow(
    wallpaper_dir: &Path,
    cache_dir: &Path,
    slug: &str,
    scheme: &Scheme,
    static_secs: u64,
    transition_secs: u64,
    hook_cmd: Option<String>,
    status_tx: tokio::sync::watch::Sender<String>,
) -> Result<PathBuf> {
    wallpaper_cache::batch_convert(scheme, wallpaper_dir, cache_dir, false, hook_cmd, status_tx.clone())
        .await?;

    let _ = status_tx.send("[ generating slideshow xml... ]".to_string());

    let slug_cache_dir = wallpaper_cache::scheme_cache_dir(cache_dir, slug, wallpaper_dir);
    let images = collect_images(&slug_cache_dir).await?;

    if images.is_empty() {
        anyhow::bail!("No wallpapers cached for scheme '{slug}'");
    }

    let xml = generate_xml(&images, static_secs, transition_secs);

    let slideshow_dir = data_dir().join("slideshows");
    tokio::fs::create_dir_all(&slideshow_dir)
        .await
        .context("creating slideshows directory")?;
    let dir_tag = wallpaper_cache::dir_hash(wallpaper_dir);
    let xml_path = slideshow_dir.join(format!("{slug}-{dir_tag}.xml"));
    tokio::fs::write(&xml_path, xml)
        .await
        .context("writing slideshow xml")?;

    let _ = status_tx.send("[ setting slideshow wallpaper... ]".to_string());

    let gnome = GnomeInterface::new().await?;
    gnome.set_wallpaper(&xml_path).await?;

    Ok(xml_path)
}

async fn collect_images(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut images = Vec::new();
    let mut read_dir = tokio::fs::read_dir(dir)
        .await
        .context("reading wallpaper cache dir")?;
    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();
        if is_image(&path) {
            images.push(path);
        }
    }
    images.sort();
    Ok(images)
}

fn is_image(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp")
    )
}

fn generate_xml(images: &[PathBuf], static_secs: u64, transition_secs: u64) -> String {
    let n = images.len();
    let static_duration = static_secs as f64;
    let transition_duration = transition_secs as f64;

    let mut xml = String::from(
        "<background>\n\
        \t<starttime>\n\
        \t\t<year>2018</year>\n\
        \t\t<month>1</month>\n\
        \t\t<day>1</day>\n\
        \t\t<hour>6</hour>\n\
        \t\t<minute>5</minute>\n\
        \t\t<second>0</second>\n\
        \t</starttime>\n",
    );

    for (i, image) in images.iter().enumerate() {
        let next = &images[(i + 1) % n];
        let path = image.display();
        let next_path = next.display();

        xml.push_str(&format!(
            "\t<static>\n\
            \t\t<file>{path}</file>\n\
            \t\t<duration>{static_duration:.1}</duration>\n\
            \t</static>\n\
            \t<transition type=\"overlay\">\n\
            \t\t<duration>{transition_duration:.1}</duration>\n\
            \t\t<from>{path}</from>\n\
            \t\t<to>{next_path}</to>\n\
            \t</transition>\n"
        ));
    }

    xml.push_str("</background>\n");
    xml
}
