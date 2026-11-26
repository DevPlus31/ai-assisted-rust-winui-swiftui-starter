//! File discovery and type detection.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::CoreError;

/// Bytes read for signature ("magic number") detection.
const SIGNATURE_BYTES: usize = 8192;
/// Hard cap on files returned by one scan, so a mistaken pick of `/` or a
/// huge archive folder can't exhaust memory.
pub const MAX_SCAN_FILES: usize = 50_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FileKind {
    Image,
    Pdf,
    WordDocument,
    Spreadsheet,
    Presentation,
    Text,
    Archive,
    Audio,
    Video,
    Installer,
    Code,
    Other,
}

/// Detects the kind from file contents first, falling back to the extension.
/// Contents win because extensions are often wrong (`photo.pdf` that is a
/// JPEG, downloads without extensions, …).
pub fn detect_kind(path: &Path) -> FileKind {
    let extension = extension_of(path);
    let by_extension = kind_from_extension(&extension);
    let Some(head) = read_head(path) else {
        return by_extension;
    };
    match infer::get(&head).map(|t| t.mime_type()) {
        Some("application/pdf") => FileKind::Pdf,
        Some(mime) if mime.starts_with("image/") => FileKind::Image,
        Some(mime) if mime.starts_with("audio/") => FileKind::Audio,
        Some(mime) if mime.starts_with("video/") => FileKind::Video,
        // Office documents are ZIP containers; trust a matching extension.
        Some("application/zip")
            if matches!(
                by_extension,
                FileKind::WordDocument | FileKind::Spreadsheet | FileKind::Presentation
            ) =>
        {
            by_extension
        }
        Some(
            "application/zip"
            | "application/gzip"
            | "application/x-7z-compressed"
            | "application/vnd.rar"
            | "application/x-tar"
            | "application/x-bzip2"
            | "application/x-xz",
        ) => FileKind::Archive,
        Some(
            "application/x-apple-diskimage"
            | "application/x-msdownload"
            | "application/vnd.microsoft.portable-executable"
            | "application/x-msi",
        ) => FileKind::Installer,
        _ => by_extension,
    }
}

pub fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

fn kind_from_extension(extension: &str) -> FileKind {
    match extension {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "heif" | "tif" | "tiff" | "bmp"
        | "svg" | "raw" | "cr2" | "nef" | "arw" | "dng" => FileKind::Image,
        "pdf" => FileKind::Pdf,
        "doc" | "docx" | "odt" | "rtf" | "pages" => FileKind::WordDocument,
        "xls" | "xlsx" | "ods" | "numbers" | "csv" | "tsv" => FileKind::Spreadsheet,
        "ppt" | "pptx" | "odp" | "key" => FileKind::Presentation,
        "txt" | "md" | "markdown" | "log" | "json" | "xml" | "html" | "htm" | "yaml" | "yml" => {
            FileKind::Text
        }
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" => FileKind::Archive,
        "mp3" | "wav" | "aac" | "m4a" | "flac" | "ogg" | "aiff" => FileKind::Audio,
        "mp4" | "mov" | "mkv" | "avi" | "webm" | "m4v" => FileKind::Video,
        "dmg" | "pkg" | "exe" | "msi" | "msix" | "msixbundle" | "appx" => FileKind::Installer,
        "rs" | "swift" | "c" | "h" | "cpp" | "cs" | "js" | "ts" | "py" | "go" | "java" | "kt"
        | "rb" | "php" | "sh" | "ps1" | "sql" | "css" | "toml" => FileKind::Code,
        _ => FileKind::Other,
    }
}

fn read_head(path: &Path) -> Option<Vec<u8>> {
    let mut head = Vec::with_capacity(SIGNATURE_BYTES);
    fs::File::open(path)
        .ok()?
        .take(SIGNATURE_BYTES as u64)
        .read_to_end(&mut head)
        .ok()?;
    Some(head)
}

/// Lists regular files in `directory`, skipping hidden files, symlinks and
/// bundle directories (e.g. `.app`, `.photoslibrary`) whose insides must
/// never be reorganised.
///
/// # Errors
/// [`CoreError::FolderUnreadable`] when `directory` itself can't be listed.
/// Unreadable sub-folders are skipped.
pub fn scan_folder(directory: &Path, recursive: bool) -> Result<Vec<PathBuf>, CoreError> {
    let mut found = Vec::new();
    let mut pending = vec![directory.to_path_buf()];
    let mut is_root = true;
    while let Some(dir) = pending.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if is_root => return Err(CoreError::FolderUnreadable(e.to_string())),
            Err(_) => continue,
        };
        is_root = false;
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if is_hidden(&path) || file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if recursive && !is_bundle(&path) {
                    pending.push(path);
                }
            } else if file_type.is_file() {
                found.push(path);
                if found.len() >= MAX_SCAN_FILES {
                    found.sort();
                    return Ok(found);
                }
            }
        }
    }
    found.sort();
    Ok(found)
}

fn is_hidden(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
        n.starts_with('.')
            || n.eq_ignore_ascii_case("desktop.ini")
            || n.eq_ignore_ascii_case("thumbs.db")
    })
}

fn is_bundle(path: &Path) -> bool {
    matches!(
        extension_of(path).as_str(),
        "app"
            | "bundle"
            | "framework"
            | "photoslibrary"
            | "musiclibrary"
            | "pages"
            | "numbers"
            | "key"
            | "xcodeproj"
            | "xcworkspace"
            | "git"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    #[test]
    fn contents_override_a_wrong_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("actually-an-image.pdf");
        fs::write(&path, PNG_SIGNATURE).unwrap();
        assert_eq!(detect_kind(&path), FileKind::Image);
    }

    #[test]
    fn falls_back_to_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.MD");
        fs::write(&path, "# hello").unwrap();
        assert_eq!(detect_kind(&path), FileKind::Text);
        assert_eq!(
            detect_kind(&dir.path().join("missing.xlsx")),
            FileKind::Spreadsheet
        );
    }

    #[test]
    fn scan_skips_hidden_files_and_bundles() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "a").unwrap();
        fs::write(dir.path().join(".DS_Store"), "x").unwrap();
        fs::create_dir_all(dir.path().join("Tool.app/Contents")).unwrap();
        fs::write(dir.path().join("Tool.app/Contents/Info.plist"), "x").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/b.txt"), "b").unwrap();

        let shallow = scan_folder(dir.path(), false).unwrap();
        let deep = scan_folder(dir.path(), true).unwrap();

        let names = |paths: &[PathBuf]| -> Vec<String> {
            paths
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(names(&shallow), ["a.txt"]);
        assert_eq!(names(&deep), ["a.txt", "b.txt"]);
    }

    #[test]
    fn unreadable_root_is_an_error() {
        let missing = std::env::temp_dir().join("app-starter-does-not-exist-42");
        assert!(matches!(
            scan_folder(&missing, false),
            Err(CoreError::FolderUnreadable(_))
        ));
    }
}
