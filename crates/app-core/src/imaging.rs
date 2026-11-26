//! Bounded, orientation-correct image decoding shared by duplicate
//! detection and compression.

use std::path::Path;

use image::{DynamicImage, ImageDecoder, ImageReader, Limits, RgbImage};

/// Per-image decode budget. With one decode per rayon thread this bounds
/// peak memory; larger images are reported as unreadable instead.
const MAX_DECODE_BYTES: u64 = 256 * 1024 * 1024;
/// 16K on the long edge covers every camera and screen in use today.
const MAX_DIMENSION: u32 = 16_384;

fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits
}

/// Decodes `path` with limits and applies the EXIF orientation, so phone
/// photos are upright. Panics in decoders are contained.
pub fn open_upright(path: &Path) -> Option<DynamicImage> {
    crate::error::contained(|| {
        let mut reader = ImageReader::open(path).ok()?.with_guessed_format().ok()?;
        reader.limits(limits());
        let mut decoder = reader.into_decoder().ok()?;
        let orientation = decoder.orientation().ok();
        let mut image = DynamicImage::from_decoder(decoder).ok()?;
        if let Some(orientation) = orientation {
            image.apply_orientation(orientation);
        }
        Some(image)
    })
    .flatten()
}

/// RGB with transparency composited onto white (JPEG has no alpha; the
/// default conversion would turn transparent areas black).
pub fn flatten_onto_white(image: &DynamicImage) -> RgbImage {
    if !image.color().has_alpha() {
        return image.to_rgb8();
    }
    let rgba = image.to_rgba8();
    RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let blend = |c: u8| {
            let mixed = (u16::from(c) * u16::from(a) + 255 * (255 - u16::from(a))) / 255;
            u8::try_from(mixed).unwrap_or(u8::MAX)
        };
        image::Rgb([blend(r), blend(g), blend(b)])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn transparent_pixels_become_white_not_black() {
        let transparent = DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([0, 0, 0, 0])));
        assert_eq!(
            flatten_onto_white(&transparent).get_pixel(0, 0).0,
            [255, 255, 255]
        );

        let opaque_red =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 255])));
        assert_eq!(
            flatten_onto_white(&opaque_red).get_pixel(0, 0).0,
            [255, 0, 0]
        );
    }

    #[test]
    fn oversized_or_invalid_images_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("fake.png");
        std::fs::write(&fake, b"\x89PNG\r\n\x1a\n not really").unwrap();
        assert!(open_upright(&fake).is_none());
    }
}
