//! Image post-processing for the gowall `intensity` control.
//!
//! gowall always applies the palette at full strength. To provide a strength
//! slider we blend its output back toward the original source image in
//! linear-light RGB. Preview and full-size paths use the same helper.

use crate::pipeline::oklab;
use anyhow::{Context, Result};
use image::{imageops::FilterType, ImageBuffer, Rgba};
use std::path::Path;

/// Blend `converted` toward `source` by `intensity` percent (0 = source,
/// 100 = converted) and write the result to `output`.
pub fn apply_intensity(
    source: &Path,
    converted: &Path,
    intensity: i32,
    output: &Path,
) -> Result<()> {
    let intensity = intensity.clamp(0, 100);
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    if intensity >= 100 {
        std::fs::copy(converted, output)
            .with_context(|| format!("copying {} to {}", converted.display(), output.display()))?;
        return Ok(());
    }
    if intensity == 0 {
        std::fs::copy(source, output)
            .with_context(|| format!("copying {} to {}", source.display(), output.display()))?;
        return Ok(());
    }

    let src = image::open(source)
        .with_context(|| format!("opening {}", source.display()))?
        .to_rgba8();
    let mut converted = image::open(converted)
        .with_context(|| format!("opening {}", converted.display()))?
        .to_rgba8();

    let (w, h) = src.dimensions();
    if converted.dimensions() != (w, h) {
        converted = image::imageops::resize(&converted, w, h, FilterType::Triangle);
    }

    let t = intensity as f32 / 100.0;
    let linear: [f32; 256] = std::array::from_fn(|i| oklab::srgb_to_linear(i as f32 / 255.0));

    let mut out: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(w, h);
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let a = src.get_pixel(x, y);
        let b = converted.get_pixel(x, y);
        let mix = |ai: u8, bi: u8| -> u8 {
            let v = linear[ai as usize] * (1.0 - t) + linear[bi as usize] * t;
            (oklab::linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        *pixel = Rgba([mix(a[0], b[0]), mix(a[1], b[1]), mix(a[2], b[2]), a[3]]);
    }

    // JPEG has no alpha channel; its encoder rejects RGBA buffers.
    match image::ImageFormat::from_path(output) {
        Ok(image::ImageFormat::Jpeg) => {
            image::DynamicImage::ImageRgba8(out)
                .to_rgb8()
                .save(output)
                .with_context(|| format!("writing {}", output.display()))?;
        }
        _ => {
            out.save(output)
                .with_context(|| format!("writing {}", output.display()))?;
        }
    }
    Ok(())
}
