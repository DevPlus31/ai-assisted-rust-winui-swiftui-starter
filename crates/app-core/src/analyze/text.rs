//! Local text extraction used to understand document contents.
//!
//! Every extractor is bounded (bytes read, bytes decompressed, pages parsed)
//! because inputs are untrusted user files.

use std::fs;
use std::io::Read;
use std::path::Path;

use crate::files::{FileKind, extension_of};

/// Characters of text kept for classification.
pub const MAX_TEXT_CHARS: usize = 20_000;
const MAX_PLAIN_BYTES: u64 = 256 * 1024;
const MAX_DOCX_XML_BYTES: u64 = 4 * 1024 * 1024;
/// lopdf decompresses streams without a budget, so only modest PDFs are
/// parsed; larger ones are classified by name and metadata.
const MAX_PDF_BYTES: u64 = 10 * 1024 * 1024;
const MAX_PDF_PAGES: u32 = 3;

/// Returns up to [`MAX_TEXT_CHARS`] of text, or `None` when the format is
/// unsupported or unreadable. Never panics on malformed input.
pub fn extract_text(path: &Path, kind: FileKind) -> Option<String> {
    let text = match kind {
        FileKind::Text | FileKind::Code => read_plain(path),
        FileKind::Spreadsheet if matches!(extension_of(path).as_str(), "csv" | "tsv") => {
            read_plain(path)
        }
        FileKind::WordDocument if extension_of(path) == "docx" => read_docx(path),
        FileKind::Pdf => read_pdf(path),
        _ => None,
    }?;
    let text: String = text.chars().take(MAX_TEXT_CHARS).collect();
    (!text.trim().is_empty()).then_some(text)
}

fn read_plain(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_PLAIN_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn read_docx(path: &Path) -> Option<String> {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).ok()?).ok()?;
    let entry = archive.by_name("word/document.xml").ok()?;
    if entry.size() > MAX_DOCX_XML_BYTES {
        return None; // zip-bomb guard (declared size)
    }
    let mut xml = String::new();
    // `take` also guards against a lying size header.
    entry
        .take(MAX_DOCX_XML_BYTES)
        .read_to_string(&mut xml)
        .ok()?;
    Some(strip_xml(&xml.replace("</w:p>", "\n")))
}

fn read_pdf(path: &Path) -> Option<String> {
    if fs::metadata(path).ok()?.len() > MAX_PDF_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    // lopdf can panic on hostile files; contain it to this one document.
    std::panic::catch_unwind(move || {
        let document = lopdf::Document::load_mem(&bytes).ok()?;
        let pages: Vec<u32> = document
            .get_pages()
            .keys()
            .copied()
            .take(MAX_PDF_PAGES as usize)
            .collect();
        document.extract_text(&pages).ok()
    })
    .ok()
    .flatten()
}

fn strip_xml(xml: &str) -> String {
    let mut text = String::with_capacity(xml.len() / 4);
    let mut in_tag = false;
    for c in xml.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_plain_text_and_csv() {
        let dir = tempfile::tempdir().unwrap();
        let txt = dir.path().join("a.txt");
        fs::write(&txt, "Invoice number 42").unwrap();
        assert_eq!(
            extract_text(&txt, FileKind::Text).as_deref(),
            Some("Invoice number 42")
        );

        let csv = dir.path().join("b.csv");
        fs::write(&csv, "date,amount\n2026-01-01,4").unwrap();
        assert!(
            extract_text(&csv, FileKind::Spreadsheet)
                .unwrap()
                .contains("amount")
        );
    }

    #[test]
    fn extracts_paragraphs_from_docx() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cv.docx");
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(
            br"<w:document><w:body><w:p><w:r><w:t>Curriculum Vitae</w:t></w:r></w:p><w:p><w:r><w:t>Skills &amp; Education</w:t></w:r></w:p></w:body></w:document>",
        )
        .unwrap();
        zip.finish().unwrap();

        let text = extract_text(&path, FileKind::WordDocument).unwrap();

        assert_eq!(text.trim(), "Curriculum Vitae\nSkills & Education");
    }

    #[test]
    fn malformed_files_yield_none_instead_of_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("broken.pdf");
        fs::write(&pdf, b"%PDF-1.7\n garbage \x00\xff").unwrap();
        assert_eq!(extract_text(&pdf, FileKind::Pdf), None);

        let docx = dir.path().join("broken.docx");
        fs::write(&docx, b"PK\x03\x04 not really").unwrap();
        assert_eq!(extract_text(&docx, FileKind::WordDocument), None);
    }

    #[test]
    fn text_is_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.txt");
        fs::write(&path, "x".repeat(MAX_TEXT_CHARS * 2)).unwrap();
        assert_eq!(
            extract_text(&path, FileKind::Text).unwrap().len(),
            MAX_TEXT_CHARS
        );
    }
}
