//! Smart renaming: readable, sortable, cross-platform-safe file names.

use std::path::{Path, PathBuf};

use crate::analyze::Category;
use crate::date::CivilDate;

/// Longest stem we generate (well under the 255-byte limits of APFS/NTFS,
/// leaving room for " (n)" and the extension).
const MAX_STEM_CHARS: usize = 120;
/// Camera/OS boilerplate stripped from original names.
const NOISE_PREFIXES: [&str; 8] = [
    "img_", "img-", "dsc_", "dsc", "pxl_", "scan_", "scan", "document",
];
const NOISE_WORDS: [&str; 6] = ["copy", "final", "kopie", "copie", "untitled", "new"];
/// macOS/Windows screenshot boilerplate ("Screen Shot 2026-02-01 at 09.30.12").
const SCREENSHOT_WORDS: [&str; 4] = ["screenshot", "screen", "shot", "at"];
/// Reserved device names on Windows (case-insensitive, with any extension).
const WINDOWS_RESERVED: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Builds `YYYY-MM-DD Category - descriptor.ext` for dated documents,
/// `YYYY-MM-DD Photo descriptor.ext` for photos/screenshots, and otherwise
/// keeps a cleaned-up version of the original name.
pub fn suggest_name(original: &Path, category: Category, date: Option<CivilDate>) -> String {
    let extension = original
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| sanitize_extension(&e.to_ascii_lowercase()));
    let stem = original
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let descriptor = descriptor(stem, category);

    let stem = match (category, date) {
        (c, Some(date)) if c.is_dated_document() => {
            join_nonempty(&[&date.iso(), c.label()], " ", &descriptor, " - ")
        }
        (Category::Photo | Category::Screenshot, Some(date)) => {
            join_nonempty(&[&date.iso(), category.label()], " ", &descriptor, " ")
        }
        _ if descriptor.is_empty() => category.label().to_owned(),
        _ => descriptor,
    };
    let stem = sanitize_stem(&stem);
    match extension {
        Some(ext) if !ext.is_empty() => format!("{stem}.{ext}"),
        _ => stem,
    }
}

fn join_nonempty(head: &[&str], head_sep: &str, tail: &str, tail_sep: &str) -> String {
    let head = head.join(head_sep);
    if tail.is_empty() {
        head
    } else {
        format!("{head}{tail_sep}{tail}")
    }
}

/// Meaningful part of an original name: drops camera prefixes, copy/final
/// noise, duplicate counters like "(1)", pure-number/date fragments, and
/// words that only repeat the category (the new name already says it).
fn descriptor(stem: &str, category: Category) -> String {
    let is_redundant = |word: &str| {
        let lower = word.to_lowercase();
        lower == category.label().to_lowercase()
            || (category == Category::Screenshot && SCREENSHOT_WORDS.contains(&lower.as_str()))
    };
    // Prefixes only count at a boundary: "scan_01" yes, "scandinavia" no.
    let without_prefix = NOISE_PREFIXES
        .iter()
        .find_map(|prefix| {
            let head = stem.get(..prefix.len())?;
            let rest = stem.get(prefix.len()..)?;
            let at_boundary = prefix.ends_with(['_', '-'])
                || rest
                    .chars()
                    .next()
                    .is_none_or(|c| c.is_ascii_digit() || "_- ".contains(c));
            (head.eq_ignore_ascii_case(prefix) && at_boundary).then_some(rest)
        })
        .unwrap_or(stem);
    without_prefix
        .split(|c: char| c == '_' || c == '-' || c.is_whitespace() || c == '.')
        .filter(|w| !w.is_empty())
        .filter(|w| !NOISE_WORDS.contains(&w.to_lowercase().as_str()))
        .filter(|w| !w.chars().all(|c| c.is_ascii_digit() || "()[]:".contains(c)))
        .filter(|w| !is_redundant(w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Makes a stem valid on both macOS and Windows.
pub fn sanitize_stem(stem: &str) -> String {
    let cleaned: String = stem
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let mut cleaned = cleaned.trim().trim_end_matches('.').trim().to_owned();
    if cleaned.chars().count() > MAX_STEM_CHARS {
        let truncated: String = cleaned.chars().take(MAX_STEM_CHARS).collect();
        truncated.trim_end().clone_into(&mut cleaned);
    }
    if cleaned.is_empty() {
        return "File".into();
    }
    if WINDOWS_RESERVED.contains(&cleaned.to_ascii_lowercase().as_str()) {
        cleaned.push('_');
    }
    cleaned
}

/// Extensions get the same character rules as names (no `:` `?` etc.).
fn sanitize_extension(extension: &str) -> String {
    extension
        .chars()
        .map(|c| {
            if "<>:\"/\\|?* ".contains(c) || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect::<String>()
        .trim_end_matches('.')
        .to_owned()
}

/// Returns `candidate`, or `name (2).ext`, `name (3).ext`, … — the first
/// name for which `is_taken` is false. Never overwrites.
pub fn unique_path(candidate: &Path, is_taken: impl Fn(&Path) -> bool) -> PathBuf {
    if !is_taken(candidate) {
        return candidate.to_path_buf();
    }
    let stem = candidate
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("File");
    let extension = candidate.extension().and_then(|e| e.to_str());
    (2..=u32::MAX)
        .map(|n| {
            let name = match extension {
                Some(ext) => format!("{stem} ({n}).{ext}"),
                None => format!("{stem} ({n})"),
            };
            candidate.with_file_name(name)
        })
        .find(|path| !is_taken(path))
        .unwrap_or_else(|| candidate.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date() -> Option<CivilDate> {
        CivilDate::new(2026, 3, 14)
    }

    #[test]
    fn dated_documents_get_sortable_names() {
        let name = suggest_name(
            Path::new("/x/ACME_invoice_final (1).PDF"),
            Category::Invoice,
            date(),
        );
        assert_eq!(name, "2026-03-14 Invoice - ACME.pdf");
    }

    #[test]
    fn camera_boilerplate_is_dropped_for_photos() {
        assert_eq!(
            suggest_name(
                Path::new("IMG_20260314_101500.jpg"),
                Category::Photo,
                date()
            ),
            "2026-03-14 Photo.jpg"
        );
        assert_eq!(
            suggest_name(Path::new("PXL_beach-day.jpg"), Category::Photo, date()),
            "2026-03-14 Photo beach day.jpg"
        );
        assert_eq!(
            suggest_name(
                Path::new("Screenshot 2026-03-14 at 09.30.12.png"),
                Category::Screenshot,
                date()
            ),
            "2026-03-14 Screenshot.png",
            "macOS screenshot boilerplate is dropped"
        );
        assert_eq!(
            suggest_name(Path::new("invoice_acme.pdf"), Category::Invoice, date()),
            "2026-03-14 Invoice - acme.pdf",
            "the category isn't repeated"
        );
    }

    #[test]
    fn undated_files_keep_a_clean_name() {
        assert_eq!(
            suggest_name(Path::new("notes_copy.txt"), Category::Document, None),
            "notes.txt"
        );
        assert_eq!(
            suggest_name(Path::new("Untitled.txt"), Category::Document, None),
            "Document.txt"
        );
        assert_eq!(
            suggest_name(Path::new("README"), Category::Other, None),
            "README"
        );
        assert_eq!(
            suggest_name(Path::new("Scandinavia trip.jpg"), Category::Image, None),
            "Scandinavia trip.jpg"
        );
        assert_eq!(
            suggest_name(Path::new("documentation.pdf"), Category::Document, None),
            "documentation.pdf"
        );
    }

    #[test]
    fn sanitizes_for_both_platforms() {
        assert_eq!(sanitize_stem("a/b:c?"), "a-b-c-");
        assert_eq!(sanitize_stem("CON"), "CON_");
        assert_eq!(sanitize_stem("trailing dots..."), "trailing dots");
        assert_eq!(sanitize_stem("   "), "File");
        assert_eq!(
            suggest_name(Path::new("odd.p?f"), Category::Document, None),
            "odd.p-f"
        );
        assert_eq!(
            sanitize_stem(&"é".repeat(300)).chars().count(),
            MAX_STEM_CHARS
        );
    }

    #[test]
    fn unique_path_appends_counters() {
        let taken = [PathBuf::from("/d/a.txt"), PathBuf::from("/d/a (2).txt")];
        let free = unique_path(Path::new("/d/a.txt"), |p| taken.iter().any(|t| t == p));
        assert_eq!(free, PathBuf::from("/d/a (3).txt"));
        assert_eq!(
            unique_path(Path::new("/d/b.txt"), |_| false),
            PathBuf::from("/d/b.txt")
        );
    }
}
