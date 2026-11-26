//! Category classification from local evidence.
//!
//! [`Classifier`] is the extension point for smarter models (e.g. an
//! on-device ONNX model later). [`HeuristicClassifier`] is deterministic and
//! explainable: every result carries a human-readable reason.

use super::image_facts::ImageFacts;
use crate::files::FileKind;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Category {
    Invoice,
    Receipt,
    Contract,
    Resume,
    BankStatement,
    TaxDocument,
    Screenshot,
    Photo,
    Image,
    Document,
    Spreadsheet,
    Presentation,
    Archive,
    Audio,
    Video,
    Installer,
    Code,
    Other,
}

impl Category {
    pub const ALL: [Self; 18] = [
        Self::Invoice,
        Self::Receipt,
        Self::Contract,
        Self::Resume,
        Self::BankStatement,
        Self::TaxDocument,
        Self::Screenshot,
        Self::Photo,
        Self::Image,
        Self::Document,
        Self::Spreadsheet,
        Self::Presentation,
        Self::Archive,
        Self::Audio,
        Self::Video,
        Self::Installer,
        Self::Code,
        Self::Other,
    ];

    /// Singular label used in suggested file names.
    pub fn label(self) -> &'static str {
        match self {
            Self::Invoice => "Invoice",
            Self::Receipt => "Receipt",
            Self::Contract => "Contract",
            Self::Resume => "Resume",
            Self::BankStatement => "Bank Statement",
            Self::TaxDocument => "Tax Document",
            Self::Screenshot => "Screenshot",
            Self::Photo => "Photo",
            Self::Image => "Image",
            Self::Document => "Document",
            Self::Spreadsheet => "Spreadsheet",
            Self::Presentation => "Presentation",
            Self::Archive => "Archive",
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Installer => "Installer",
            Self::Code => "Code",
            Self::Other => "Other",
        }
    }

    /// Folder name used when sorting into category folders.
    pub fn folder(self) -> &'static str {
        match self {
            Self::Invoice => "Invoices",
            Self::Receipt => "Receipts",
            Self::Contract => "Contracts",
            Self::Resume => "Resumes",
            Self::BankStatement => "Bank Statements",
            Self::TaxDocument => "Taxes",
            Self::Screenshot => "Screenshots",
            Self::Photo => "Photos",
            Self::Image => "Images",
            Self::Document => "Documents",
            Self::Spreadsheet => "Spreadsheets",
            Self::Presentation => "Presentations",
            Self::Archive => "Archives",
            Self::Audio => "Audio",
            Self::Video => "Videos",
            Self::Installer => "Installers",
            Self::Code => "Code",
            Self::Other => "Other",
        }
    }

    /// Document categories whose names benefit from a date prefix.
    pub fn is_dated_document(self) -> bool {
        matches!(
            self,
            Self::Invoice
                | Self::Receipt
                | Self::Contract
                | Self::BankStatement
                | Self::TaxDocument
        )
    }
}

/// Everything known about a file before classification.
#[derive(Debug)]
pub struct Evidence<'a> {
    pub file_name: &'a str,
    pub kind: FileKind,
    pub text: Option<&'a str>,
    pub image: Option<&'a ImageFacts>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Classification {
    pub category: Category,
    /// 0.0–1.0. UIs may ask before acting below ~0.6.
    pub confidence: f32,
    pub reason: String,
}

pub trait Classifier: Send + Sync {
    fn classify(&self, evidence: &Evidence<'_>) -> Classification;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct HeuristicClassifier;

/// Keyword lists (lower-case, matched on word boundaries). Includes common
/// German/French/Spanish terms for documents users receive most often.
const DOCUMENT_RULES: [(Category, &[&str]); 6] = [
    (
        Category::Invoice,
        &[
            "invoice",
            "invoice number",
            "invoice no",
            "bill to",
            "amount due",
            "payment terms",
            "due date",
            "vat",
            "rechnung",
            "facture",
            "factura",
        ],
    ),
    (
        Category::Receipt,
        &[
            "receipt",
            "thank you for your purchase",
            "order total",
            "paid with",
            "change due",
            "cashier",
            "quittung",
            "kassenbon",
            "reçu",
            "recibo",
        ],
    ),
    (
        Category::Contract,
        &[
            "agreement",
            "contract",
            "hereinafter",
            "the parties",
            "terms and conditions",
            "in witness whereof",
            "signature",
            "vertrag",
            "contrat",
            "contrato",
        ],
    ),
    (
        Category::Resume,
        &[
            "resume",
            "résumé",
            "curriculum vitae",
            "cv",
            "work experience",
            "education",
            "skills",
            "references",
            "lebenslauf",
        ],
    ),
    (
        Category::BankStatement,
        &[
            "bank statement",
            "account statement",
            "opening balance",
            "closing balance",
            "iban",
            "statement period",
            "kontoauszug",
            "relevé",
        ],
    ),
    (
        Category::TaxDocument,
        &[
            "tax return",
            "form w-2",
            "1099",
            "tax year",
            "irs",
            "hmrc",
            "taxable income",
            "steuer",
            "steuererklärung",
            "impôt",
        ],
    ),
];
const NAME_WEIGHT: u32 = 3;
const MIN_DOCUMENT_SCORE: u32 = 3;
const SCREENSHOT_NAMES: [&str; 5] = [
    "screenshot",
    "screen shot",
    "bildschirmfoto",
    "capture d",
    "captura de pantalla",
];

impl Classifier for HeuristicClassifier {
    fn classify(&self, evidence: &Evidence<'_>) -> Classification {
        let name = evidence.file_name.to_lowercase();
        match evidence.kind {
            FileKind::Image => classify_image(&name, evidence.image),
            FileKind::Pdf | FileKind::WordDocument | FileKind::Text | FileKind::Spreadsheet => {
                classify_document(&name, evidence.text).unwrap_or_else(|| by_kind(evidence.kind))
            }
            kind => by_kind(kind),
        }
    }
}

fn classify_image(name: &str, facts: Option<&ImageFacts>) -> Classification {
    if SCREENSHOT_NAMES.iter().any(|k| name.contains(k)) {
        return result(
            Category::Screenshot,
            0.95,
            "File name says it is a screenshot",
        );
    }
    match facts {
        Some(f) if f.from_camera => {
            result(Category::Photo, 0.9, "Camera details in photo metadata")
        }
        Some(f) if f.taken.is_some() => {
            result(Category::Photo, 0.75, "Capture date in photo metadata")
        }
        Some(f) if f.looks_like_screen_capture() => result(
            Category::Screenshot,
            0.6,
            "Screen-sized image without camera metadata",
        ),
        _ => result(Category::Image, 0.5, "Image file"),
    }
}

fn classify_document(name: &str, text: Option<&str>) -> Option<Classification> {
    let body = text.map(str::to_lowercase).unwrap_or_default();
    DOCUMENT_RULES
        .iter()
        .filter_map(|(category, keywords)| {
            let hits: Vec<&str> = keywords
                .iter()
                .copied()
                .filter(|k| contains_word(name, k) || contains_word(&body, k))
                .collect();
            let score: u32 = hits
                .iter()
                .map(|k| {
                    if contains_word(name, k) {
                        NAME_WEIGHT
                    } else {
                        1
                    }
                })
                .sum();
            (score >= MIN_DOCUMENT_SCORE).then_some((*category, score, hits))
        })
        .max_by_key(|(_, score, _)| *score)
        .map(|(category, score, hits)| {
            let capped = u8::try_from(score.min(10)).unwrap_or(10);
            let confidence = (0.45 + 0.1 * f32::from(capped)).min(0.97);
            let shown: Vec<&str> = hits.into_iter().take(3).collect();
            result(
                category,
                confidence,
                &format!("Mentions “{}”", shown.join("”, “")),
            )
        })
}

fn by_kind(kind: FileKind) -> Classification {
    let category = match kind {
        FileKind::Image => Category::Image,
        FileKind::Pdf | FileKind::WordDocument | FileKind::Text => Category::Document,
        FileKind::Spreadsheet => Category::Spreadsheet,
        FileKind::Presentation => Category::Presentation,
        FileKind::Archive => Category::Archive,
        FileKind::Audio => Category::Audio,
        FileKind::Video => Category::Video,
        FileKind::Installer => Category::Installer,
        FileKind::Code => Category::Code,
        FileKind::Other => Category::Other,
    };
    result(category, 0.5, "File type")
}

fn result(category: Category, confidence: f32, reason: &str) -> Classification {
    Classification {
        category,
        confidence,
        reason: reason.to_owned(),
    }
}

/// Word-boundary match so "cv" doesn't match "cvs" and "vat" not "private".
fn contains_word(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(start, _)| {
        let before = haystack[..start].chars().next_back();
        let after = haystack[start + needle.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(name: &str, kind: FileKind, text: Option<&str>) -> Classification {
        HeuristicClassifier.classify(&Evidence {
            file_name: name,
            kind,
            text,
            image: None,
        })
    }

    #[test]
    fn recognises_invoices_from_content() {
        let text = "ACME Ltd\nInvoice Number: 1042\nBill To: Jane\nAmount due: €120\nVAT 20%";
        let c = classify("scan_0042.pdf", FileKind::Pdf, Some(text));
        assert_eq!(c.category, Category::Invoice);
        assert!(c.confidence > 0.8);
        assert!(c.reason.contains("invoice"));
    }

    #[test]
    fn file_name_alone_can_classify() {
        assert_eq!(
            classify("Rechnung_März.pdf", FileKind::Pdf, None).category,
            Category::Invoice
        );
        assert_eq!(
            classify("My CV 2026.docx", FileKind::WordDocument, None).category,
            Category::Resume
        );
    }

    #[test]
    fn weak_evidence_falls_back_to_file_type() {
        let c = classify(
            "notes.txt",
            FileKind::Text,
            Some("private thoughts about cvs"),
        );
        assert_eq!(c.category, Category::Document);
        assert!((c.confidence - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn word_boundaries_are_respected() {
        assert!(contains_word("my cv.pdf", "cv"));
        assert!(!contains_word("cvs receipt", "cv"));
        assert!(!contains_word("private", "vat"));
        assert!(contains_word("résumé final", "résumé"));
    }

    #[test]
    fn screenshots_and_photos() {
        let named = HeuristicClassifier.classify(&Evidence {
            file_name: "Screenshot 2026-10-18 at 10.00.00.png",
            kind: FileKind::Image,
            text: None,
            image: None,
        });
        assert_eq!(named.category, Category::Screenshot);

        let camera = ImageFacts {
            width: 4032,
            height: 3024,
            from_camera: true,
            taken: None,
        };
        let photo = HeuristicClassifier.classify(&Evidence {
            file_name: "IMG_1234.JPG",
            kind: FileKind::Image,
            text: None,
            image: Some(&camera),
        });
        assert_eq!(photo.category, Category::Photo);
    }

    #[test]
    fn non_documents_use_their_kind() {
        assert_eq!(
            classify("setup.exe", FileKind::Installer, None).category,
            Category::Installer
        );
    }
}
