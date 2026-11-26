//! Cheap image metadata: header dimensions and EXIF (no full decode).

use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;

use crate::date::CivilDate;

const EXIF_PREFIX_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageFacts {
    pub width: u32,
    pub height: u32,
    /// Camera make/model present in EXIF — a strong "photo" signal.
    pub from_camera: bool,
    pub taken: Option<CivilDate>,
}

pub fn read_image_facts(path: &Path) -> Option<ImageFacts> {
    let (width, height) = image::image_dimensions(path).ok()?;
    let (from_camera, taken) = read_exif(path).unwrap_or_default();
    Some(ImageFacts {
        width,
        height,
        from_camera,
        taken,
    })
}

fn read_exif(path: &Path) -> Option<(bool, Option<CivilDate>)> {
    // EXIF lives near the start of JPEG/HEIC/TIFF files. Reading a bounded
    // prefix keeps a huge RAW/TIFF from being loaded whole into memory.
    let mut prefix = Vec::with_capacity(64 * 1024);
    fs::File::open(path)
        .ok()?
        .take(EXIF_PREFIX_BYTES)
        .read_to_end(&mut prefix)
        .ok()?;
    let exif = exif::Reader::new()
        .read_from_container(&mut Cursor::new(prefix))
        .ok()?;
    let has = |tag| exif.get_field(tag, exif::In::PRIMARY).is_some();
    let from_camera = has(exif::Tag::Make) || has(exif::Tag::Model);
    let taken = [exif::Tag::DateTimeOriginal, exif::Tag::DateTime]
        .into_iter()
        .find_map(|tag| {
            let field = exif.get_field(tag, exif::In::PRIMARY)?;
            match &field.value {
                exif::Value::Ascii(values) => values
                    .first()
                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                    .and_then(CivilDate::from_exif),
                _ => None,
            }
        });
    Some((from_camera, taken))
}

impl ImageFacts {
    /// Typical display resolutions (and Retina doubles) for screenshot
    /// detection when the file name gives no hint.
    pub fn looks_like_screen_capture(&self) -> bool {
        const SCREEN_WIDTHS: [u32; 12] = [
            1280, 1366, 1440, 1512, 1680, 1728, 1920, 2560, 2880, 3024, 3456, 3840,
        ];
        !self.from_camera && self.taken.is_none() && SCREEN_WIDTHS.contains(&self.width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_dimensions_without_exif() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shot.png");
        image::RgbImage::new(1920, 4).save(&path).unwrap();

        let facts = read_image_facts(&path).unwrap();

        assert_eq!((facts.width, facts.height), (1920, 4));
        assert!(!facts.from_camera);
        assert!(facts.looks_like_screen_capture());
    }

    #[test]
    fn non_images_have_no_facts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.png");
        fs::write(&path, "not an image").unwrap();
        assert_eq!(read_image_facts(&path), None);
    }
}
