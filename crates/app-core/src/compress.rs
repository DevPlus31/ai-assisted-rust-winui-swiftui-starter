//! Image compression and format conversion. Originals are never modified:
//! results are written as new files, and a result that isn't smaller than
//! the original (when not changing format) is discarded.

use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageFormat};
use rayon::prelude::*;

use crate::error::{FileFailure, describe_io};
use crate::naming::unique_path;
use crate::user_text::TextKey;

/// Attempts at finding a free output name when parallel workers race.
const MAX_NAME_ATTEMPTS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OutputFormat {
    /// Lossy, quality 1–100.
    Jpeg {
        quality: u8,
    },
    Png,
    /// Lossless WebP (the pure-Rust encoder has no lossy mode).
    WebpLossless,
}

impl OutputFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Jpeg { .. } => "jpg",
            Self::Png => "png",
            Self::WebpLossless => "webp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompressOptions {
    pub format: OutputFormat,
    /// Longest edge in pixels; larger images are downscaled.
    pub max_dimension: Option<u32>,
}

impl Default for CompressOptions {
    fn default() -> Self {
        Self {
            format: OutputFormat::Jpeg { quality: 82 },
            max_dimension: Some(3840),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompressOutcome {
    Written {
        source: PathBuf,
        output: PathBuf,
        before: u64,
        after: u64,
    },
    /// Re-encoding would not have saved space.
    NotSmaller {
        source: PathBuf,
    },
    Failed(FileFailure),
}

/// Compresses/converts each image into `output_dir` (created if needed).
/// Output names keep the original stem; collisions get " (n)".
pub fn compress_images(
    sources: &[PathBuf],
    output_dir: &Path,
    options: CompressOptions,
) -> Vec<CompressOutcome> {
    if let Err(error) = fs::create_dir_all(output_dir) {
        let reason = describe_io(&error);
        return sources
            .iter()
            .map(|s| CompressOutcome::Failed(FileFailure::because(s, reason)))
            .collect();
    }
    // Each worker encodes and writes immediately, so at most one encoded
    // image per thread is held in memory. Names are claimed atomically.
    sources
        .par_iter()
        .map(|source| {
            let encoded = crate::error::contained(|| encode(source, options))
                .unwrap_or(Err(TextKey::ImageUnreadable));
            match encoded {
                Ok((bytes, before)) => write_output(source, output_dir, options, &bytes, before),
                Err(reason) => CompressOutcome::Failed(FileFailure::because(source, reason)),
            }
        })
        .collect()
}

fn encode(source: &Path, options: CompressOptions) -> Result<(Vec<u8>, u64), TextKey> {
    let before = fs::metadata(source).map_err(|e| describe_io(&e))?.len();
    let mut image = crate::imaging::open_upright(source).ok_or(TextKey::ImageUnreadable)?;
    if let Some(max) = options.max_dimension
        && image.width().max(image.height()) > max
    {
        image = image.resize(max, max, image::imageops::FilterType::Lanczos3);
    }
    let mut bytes = Vec::new();
    let encoded = match options.format {
        OutputFormat::Jpeg { quality } => {
            let rgb = DynamicImage::ImageRgb8(crate::imaging::flatten_onto_white(&image));
            JpegEncoder::new_with_quality(&mut bytes, quality.clamp(1, 100)).encode_image(&rgb)
        }
        OutputFormat::Png => {
            image.write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
        }
        OutputFormat::WebpLossless => {
            image.write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::WebP)
        }
    };
    encoded.map_err(|_| TextKey::ImageNotConverted)?;
    Ok((bytes, before))
}

fn write_output(
    source: &Path,
    output_dir: &Path,
    options: CompressOptions,
    bytes: &[u8],
    before: u64,
) -> CompressOutcome {
    let same_format = source
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            let e = e.to_ascii_lowercase();
            e == options.format.extension() || (e == "jpeg" && options.format.extension() == "jpg")
        });
    if same_format && bytes.len() as u64 >= before {
        return CompressOutcome::NotSmaller {
            source: source.to_path_buf(),
        };
    }
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let candidate = output_dir.join(format!("{stem}.{}", options.format.extension()));
    let Some((output, file)) = claim_output(&candidate, source) else {
        return CompressOutcome::Failed(FileFailure::because(source, TextKey::FileExists));
    };
    let written = write_all_synced(file, bytes);
    if written.is_err() {
        let _ = fs::remove_file(&output); // don't leave a truncated image
    }
    match written {
        Ok(()) => CompressOutcome::Written {
            source: source.to_path_buf(),
            output,
            before,
            after: bytes.len() as u64,
        },
        Err(error) => CompressOutcome::Failed(FileFailure::new(source, &error)),
    }
}

fn write_all_synced(file: fs::File, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut writer = BufWriter::new(file);
    writer.write_all(bytes)?;
    writer
        .into_inner()
        .map_err(std::io::IntoInnerError::into_error)?
        .sync_all()
}

/// Atomically creates the first free `name (n).ext` (never overwrites).
fn claim_output(candidate: &Path, source: &Path) -> Option<(PathBuf, fs::File)> {
    (0..MAX_NAME_ATTEMPTS).find_map(|_| {
        let path = unique_path(candidate, |p| p.exists() || p == source);
        fs::File::create_new(&path).ok().map(|file| (path, file))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn noisy_png(path: &Path, size: u32) {
        RgbImage::from_fn(size, size, |x, y| {
            let v = u8::try_from((x * 7 + y * 13) % 256).unwrap();
            Rgb([v, v.wrapping_mul(3), v.wrapping_add(90)])
        })
        .save(path)
        .unwrap();
    }

    #[test]
    fn converts_png_to_smaller_jpeg_and_downscales() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("big.png");
        noisy_png(&source, 600);
        let out = dir.path().join("out");

        let outcomes = compress_images(
            std::slice::from_ref(&source),
            &out,
            CompressOptions {
                format: OutputFormat::Jpeg { quality: 70 },
                max_dimension: Some(300),
            },
        );

        let CompressOutcome::Written {
            output,
            before,
            after,
            ..
        } = &outcomes[0]
        else {
            panic!("expected a written file, got {outcomes:?}");
        };
        assert!(after < before);
        assert_eq!(output, &out.join("big.jpg"));
        assert_eq!(image::image_dimensions(output).unwrap(), (300, 300));
        assert!(source.exists(), "original must be kept");
    }

    #[test]
    fn never_overwrites_existing_outputs() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("pic.png");
        noisy_png(&source, 64);
        fs::write(dir.path().join("pic.jpg"), b"existing").unwrap();

        let outcomes = compress_images(&[source], dir.path(), CompressOptions::default());

        assert!(
            matches!(&outcomes[0], CompressOutcome::Written { output, .. } if output.ends_with("pic (2).jpg"))
        );
        assert_eq!(fs::read(dir.path().join("pic.jpg")).unwrap(), b"existing");
    }

    #[test]
    fn reports_unreadable_images() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("fake.jpg");
        fs::write(&source, b"nope").unwrap();
        let outcomes = compress_images(&[source], dir.path(), CompressOptions::default());
        assert!(
            matches!(&outcomes[0], CompressOutcome::Failed(f) if f.message.key == TextKey::ImageUnreadable)
        );
    }
}
