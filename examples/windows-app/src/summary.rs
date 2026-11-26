//! What the shell shows after analysing a folder. Platform-neutral, so it
//! is unit-tested on every OS; the WinUI code only renders a [`Summary`].

use std::path::{Path, PathBuf};

use app_core::analyze::{Category, FileInsight};
use app_core::{CoreError, FileFailure, Job, TextKey, UserText};
use app_platform_windows::Workspace;

/// Files of one category found in the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryCount {
    /// Plural title, e.g. "Invoices".
    pub title: &'static str,
    pub files: usize,
    pub bytes: u64,
}

/// The result of analysing one folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub folder: PathBuf,
    pub files: usize,
    /// Non-empty categories, largest first.
    pub categories: Vec<CategoryCount>,
    /// Files the engine couldn't read.
    pub unreadable: usize,
}

/// A finished analysis: what to show, plus the files to plan with.
#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    pub summary: Summary,
    pub insights: Vec<FileInsight>,
}

/// Why an analysis didn't finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stopped {
    /// The user cancelled it; not an error.
    Cancelled,
    /// What to tell the user (translate with `text::Translations`).
    Failed(UserText),
}

/// Analyses the top level of `folder` (as the user typed it), reporting
/// progress to `job`.
///
/// # Errors
/// [`Stopped::Failed`] when the path isn't a full folder path or the folder
/// can't be read; [`Stopped::Cancelled`] when `job` was cancelled.
pub fn analyze(workspace: &Workspace, folder: &str, job: &Job) -> Result<Analysis, Stopped> {
    let folder = parse_folder(folder).map_err(Stopped::Failed)?;
    let (insights, failures) = workspace
        .with(|engine| engine.analyze_folder(&folder, false, job))
        .map_err(|error| match error {
            CoreError::Cancelled => Stopped::Cancelled,
            other => Stopped::Failed(other.user_text()),
        })?;
    Ok(Analysis {
        summary: summarize(folder, &insights, &failures),
        insights,
    })
}

fn parse_folder(input: &str) -> Result<PathBuf, UserText> {
    let trimmed = input.trim().trim_matches('"');
    if trimmed.is_empty() {
        return Err(TextKey::FolderRequired.into());
    }
    let path = Path::new(trimmed);
    if !path.is_absolute() {
        return Err(UserText::new(TextKey::FolderNotAbsolute).arg("folder", trimmed));
    }
    Ok(path.to_path_buf())
}

/// Groups `insights` by category, largest categories first.
pub fn summarize(folder: PathBuf, insights: &[FileInsight], failures: &[FileFailure]) -> Summary {
    let mut categories: Vec<CategoryCount> = Category::ALL
        .iter()
        .map(|&category| {
            let matching = insights.iter().filter(|i| i.category == category);
            CategoryCount {
                title: category.folder(),
                files: matching.clone().count(),
                bytes: matching.map(|i| i.size).sum(),
            }
        })
        .filter(|count| count.files > 0)
        .collect();
    // Stable sort keeps the engine's category order for ties.
    categories.sort_by_key(|count| std::cmp::Reverse(count.files));
    Summary {
        folder,
        files: insights.len(),
        categories,
        unreadable: failures.len(),
    }
}

/// Human-readable size, e.g. `1.4 MB`.
#[allow(clippy::cast_precision_loss)] // display only
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1000.0;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::open(Some(dir.path().join("data"))).unwrap();
        (dir, workspace)
    }

    #[test]
    fn groups_files_by_category_largest_first() {
        let (dir, workspace) = workspace();
        let folder = dir.path().join("inbox");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("a.zip"), b"PK\x05\x06").unwrap();
        std::fs::write(folder.join("b.mp3"), b"ID3").unwrap();
        std::fs::write(folder.join("c.mp3"), b"ID3").unwrap();

        let job = Job::new();
        let analysis = analyze(&workspace, folder.to_str().unwrap(), &job).unwrap();
        let summary = analysis.summary;

        assert_eq!(summary.files, 3);
        assert_eq!(summary.unreadable, 0);
        assert_eq!(summary.categories.len(), 2);
        assert_eq!(summary.categories[0].files, 2);
        assert_eq!(summary.categories[1].files, 1);
        let counted: usize = summary.categories.iter().map(|c| c.files).sum();
        assert_eq!(counted, summary.files);
        assert_eq!(analysis.insights.len(), 3);
        assert_eq!(job.progress().done, 3);
    }

    #[test]
    fn a_cancelled_job_stops_without_an_error_message() {
        let (dir, workspace) = workspace();
        let job = Job::new();
        job.cancel();
        assert_eq!(
            analyze(&workspace, dir.path().to_str().unwrap(), &job),
            Err(Stopped::Cancelled)
        );
    }

    #[test]
    fn accepts_a_quoted_path_as_pasted_from_explorer() {
        let (dir, workspace) = workspace();
        let quoted = format!("  \"{}\"  ", dir.path().display());
        assert!(analyze(&workspace, &quoted, &Job::new()).is_ok());
    }

    #[test]
    fn rejects_empty_and_relative_paths_with_a_message() {
        let (_dir, workspace) = workspace();
        let failed = |folder: &str| match analyze(&workspace, folder, &Job::new()) {
            Err(Stopped::Failed(text)) => text,
            other => panic!("expected a failure, got {other:?}"),
        };
        assert_eq!(failed("   ").key, TextKey::FolderRequired);
        assert_eq!(
            failed("Downloads").to_string(),
            "“Downloads” isn't a full folder path."
        );
    }

    #[test]
    fn reports_a_missing_folder_as_an_error() {
        let (dir, workspace) = workspace();
        let missing = dir.path().join("missing");
        assert!(matches!(
            analyze(&workspace, missing.to_str().unwrap(), &Job::new()),
            Err(Stopped::Failed(_))
        ));
    }

    #[test]
    fn formats_sizes_in_decimal_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(999), "999 B");
        assert_eq!(format_size(1_000), "1.0 KB");
        assert_eq!(format_size(1_400_000), "1.4 MB");
        assert_eq!(format_size(3_000_000_000_000_000), "3000.0 TB");
    }
}
