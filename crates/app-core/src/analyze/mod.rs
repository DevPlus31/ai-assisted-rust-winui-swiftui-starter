//! Understanding files: type, contents, category, date and a better name.

mod classify;
mod image_facts;
mod text;

use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

pub use classify::{Category, Classification, Classifier, Evidence, HeuristicClassifier};
pub use image_facts::ImageFacts;

use crate::date::CivilDate;
use crate::error::{CoreError, FileFailure};
use crate::files::{FileKind, detect_kind};
use crate::job::Job;
use crate::naming::suggest_name;

#[derive(Debug, Clone, PartialEq)]
pub struct FileInsight {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub kind: FileKind,
    pub category: Category,
    pub confidence: f32,
    pub reason: String,
    /// Best date for the file: photo capture, date in the document, or
    /// last modification.
    pub date: Option<CivilDate>,
    pub modified: Option<CivilDate>,
    /// Exact modification time, used to detect changes before applying.
    pub modified_at: Option<std::time::SystemTime>,
    pub suggested_name: String,
    /// Image dimensions, when the file is an image.
    pub dimensions: Option<(u32, u32)>,
}

/// Analyses one file. Reads at most a few hundred KB per file (see
/// `text` limits) and never modifies anything. Parser panics on hostile
/// files are contained and reported as an unreadable file.
///
/// # Errors
/// A [`FileFailure`] when the file can't be read or parsed.
pub fn analyze_file(path: &Path, classifier: &dyn Classifier) -> Result<FileInsight, FileFailure> {
    crate::error::contained(|| analyze_unguarded(path, classifier)).unwrap_or_else(|| {
        Err(FileFailure::because(
            path,
            crate::TextKey::ContentsUnreadable,
        ))
    })
}

fn analyze_unguarded(path: &Path, classifier: &dyn Classifier) -> Result<FileInsight, FileFailure> {
    let metadata = fs::metadata(path).map_err(|e| FileFailure::new(path, &e))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let kind = detect_kind(path);
    let text = text::extract_text(path, kind);
    let image = (kind == FileKind::Image)
        .then(|| image_facts::read_image_facts(path))
        .flatten();

    let classification = classifier.classify(&Evidence {
        file_name: &name,
        kind,
        text: text.as_deref(),
        image: image.as_ref(),
    });
    let modified_at = metadata.modified().ok();
    let modified = modified_at.and_then(CivilDate::from_system_time);
    // Capture date, then a date in the contents, then one in the name
    // ("Screenshot 2026-02-01 at …"), then the last modification.
    let date = image
        .as_ref()
        .and_then(|facts| facts.taken)
        .or_else(|| text.as_deref().and_then(CivilDate::find_in_text))
        .or_else(|| CivilDate::find_in_text(&name))
        .or(modified);

    Ok(FileInsight {
        suggested_name: suggest_name(path, classification.category, date),
        path: path.to_path_buf(),
        name,
        size: metadata.len(),
        kind,
        category: classification.category,
        confidence: classification.confidence,
        reason: classification.reason,
        date,
        modified,
        modified_at,
        dimensions: image.map(|facts| (facts.width, facts.height)),
    })
}

/// Analyses many files in parallel. Results keep the input order.
pub fn analyze_files(
    paths: &[PathBuf],
    classifier: &dyn Classifier,
) -> (Vec<FileInsight>, Vec<FileFailure>) {
    let job = Job::new();
    analyze_files_in(paths, classifier, &job).unwrap_or_default()
}

/// [`analyze_files`] that reports progress to `job` and stops early when
/// it is cancelled.
///
/// # Errors
/// [`CoreError::Cancelled`] when `job` was cancelled before every file
/// was analysed.
pub fn analyze_files_in(
    paths: &[PathBuf],
    classifier: &dyn Classifier,
    job: &Job,
) -> Result<(Vec<FileInsight>, Vec<FileFailure>), CoreError> {
    job.start(paths.len());
    // Workers check the flag before each file; collecting into `Option`
    // stops handing out new files as soon as one sees it.
    let results: Vec<_> = paths
        .par_iter()
        .map(|path| {
            if job.is_cancelled() {
                return None;
            }
            let result = analyze_file(path, classifier);
            job.advance();
            Some(result)
        })
        .collect::<Option<_>>()
        .ok_or(CoreError::Cancelled)?;
    let mut insights = Vec::with_capacity(results.len());
    let mut failures = Vec::new();
    for result in results {
        match result {
            Ok(insight) => insights.push(insight),
            Err(failure) => failures.push(failure),
        }
    }
    Ok((insights, failures))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_date_from_the_file_name_before_the_modified_date() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Screenshot 2026-02-01 at 09.30.12.png");
        image::RgbImage::new(8, 8).save(&path).unwrap();

        let insight = analyze_file(&path, &HeuristicClassifier).unwrap();

        assert_eq!(insight.category, Category::Screenshot);
        assert_eq!(insight.date, CivilDate::new(2026, 2, 1));
        assert_eq!(insight.suggested_name, "2026-02-01 Screenshot.png");
    }

    #[test]
    fn analyses_a_text_invoice_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scan_0042.txt");
        fs::write(
            &path,
            "Invoice number 7\nBill to: Jo\nAmount due on 2026-02-01",
        )
        .unwrap();

        let insight = analyze_file(&path, &HeuristicClassifier).unwrap();

        assert_eq!(insight.category, Category::Invoice);
        assert_eq!(insight.date, CivilDate::new(2026, 2, 1));
        assert_eq!(insight.suggested_name, "2026-02-01 Invoice.txt");
        assert_eq!(insight.size, 53);
    }

    #[test]
    fn batch_collects_failures_without_aborting() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("a.txt");
        fs::write(&ok, "hello").unwrap();
        let missing = dir.path().join("gone.txt");

        let (insights, failures) = analyze_files(&[ok, missing.clone()], &HeuristicClassifier);

        assert_eq!(insights.len(), 1);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].path, missing);
    }
}
