//! Plain data types crossing the Swift boundary, with conversions from
//! `app-core` types. No behaviour lives here.

use app_core::analyze::{Category, FileInsight};
use app_core::compress::{CompressOptions, CompressOutcome, OutputFormat};
use app_core::dedupe::DuplicateGroup;
use app_core::workflow::{ApplyReport, Destinations, Mode, PlannedMove};
use app_core::{FileFailure, UserText};

fn path_string(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FileCategory {
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

impl From<Category> for FileCategory {
    fn from(category: Category) -> Self {
        match category {
            Category::Invoice => Self::Invoice,
            Category::Receipt => Self::Receipt,
            Category::Contract => Self::Contract,
            Category::Resume => Self::Resume,
            Category::BankStatement => Self::BankStatement,
            Category::TaxDocument => Self::TaxDocument,
            Category::Screenshot => Self::Screenshot,
            Category::Photo => Self::Photo,
            Category::Image => Self::Image,
            Category::Document => Self::Document,
            Category::Spreadsheet => Self::Spreadsheet,
            Category::Presentation => Self::Presentation,
            Category::Archive => Self::Archive,
            Category::Audio => Self::Audio,
            Category::Video => Self::Video,
            Category::Installer => Self::Installer,
            Category::Code => Self::Code,
            Category::Other => Self::Other,
        }
    }
}

impl From<FileCategory> for Category {
    fn from(category: FileCategory) -> Self {
        match category {
            FileCategory::Invoice => Self::Invoice,
            FileCategory::Receipt => Self::Receipt,
            FileCategory::Contract => Self::Contract,
            FileCategory::Resume => Self::Resume,
            FileCategory::BankStatement => Self::BankStatement,
            FileCategory::TaxDocument => Self::TaxDocument,
            FileCategory::Screenshot => Self::Screenshot,
            FileCategory::Photo => Self::Photo,
            FileCategory::Image => Self::Image,
            FileCategory::Document => Self::Document,
            FileCategory::Spreadsheet => Self::Spreadsheet,
            FileCategory::Presentation => Self::Presentation,
            FileCategory::Archive => Self::Archive,
            FileCategory::Audio => Self::Audio,
            FileCategory::Video => Self::Video,
            FileCategory::Installer => Self::Installer,
            FileCategory::Code => Self::Code,
            FileCategory::Other => Self::Other,
        }
    }
}

/// A category as shown in the Destinations list.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CategoryInfo {
    pub category: FileCategory,
    /// Plural title, also the subfolder name in the default folder.
    pub title: String,
}

impl From<Category> for CategoryInfo {
    fn from(category: Category) -> Self {
        Self {
            category: category.into(),
            title: category.folder().to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CategoryFolder {
    pub category: FileCategory,
    pub folder: String,
}

/// Where files go: a folder per category, plus an optional default folder
/// that gets a subfolder per remaining category.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DestinationsRecord {
    pub default_folder: Option<String>,
    pub categories: Vec<CategoryFolder>,
}

impl From<DestinationsRecord> for Destinations {
    fn from(record: DestinationsRecord) -> Self {
        let base = Self {
            default_root: record.default_folder.map(Into::into),
            ..Self::default()
        };
        record
            .categories
            .into_iter()
            .fold(base, |destinations, entry| {
                destinations.with(entry.category.into(), entry.folder)
            })
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FileRecord {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub category: FileCategory,
    pub category_label: String,
    pub confidence: f32,
    pub reason: String,
    /// ISO `YYYY-MM-DD`.
    pub date: Option<String>,
    pub suggested_name: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl From<&FileInsight> for FileRecord {
    fn from(insight: &FileInsight) -> Self {
        Self {
            path: path_string(&insight.path),
            name: insight.name.clone(),
            size: insight.size,
            category: insight.category.into(),
            category_label: insight.category.label().to_owned(),
            confidence: insight.confidence,
            reason: insight.reason.clone(),
            date: insight.date.map(app_core::date::CivilDate::iso),
            suggested_name: insight.suggested_name.clone(),
            width: insight.dimensions.map(|(w, _)| w),
            height: insight.dimensions.map(|(_, h)| h),
        }
    }
}

/// A sentence for the user. Show `text` (English), or translate: look up
/// `key` (e.g. in a String Catalog), fall back to `template`, then replace
/// each `{name}` with its argument. `localized(_:)` in the Swift package
/// does this.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct UserTextRecord {
    pub key: String,
    pub template: String,
    pub args: Vec<TextArgRecord>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TextArgRecord {
    pub name: String,
    pub value: String,
}

impl From<&UserText> for UserTextRecord {
    fn from(text: &UserText) -> Self {
        Self {
            key: text.key.id().to_owned(),
            template: text.key.template().to_owned(),
            args: text
                .args
                .iter()
                .map(|(name, value)| TextArgRecord {
                    name: (*name).to_owned(),
                    value: value.clone(),
                })
                .collect(),
            text: text.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FailureRecord {
    pub path: String,
    pub message: UserTextRecord,
}

impl From<&FileFailure> for FailureRecord {
    fn from(failure: &FileFailure) -> Self {
        Self {
            path: path_string(&failure.path),
            message: (&failure.message).into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AnalysisRecord {
    pub files: Vec<FileRecord>,
    pub failures: Vec<FailureRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MoveRecord {
    pub from: String,
    pub to: String,
    pub rule_name: String,
}

impl From<&PlannedMove> for MoveRecord {
    fn from(planned: &PlannedMove) -> Self {
        Self {
            from: path_string(&planned.from),
            to: path_string(&planned.to),
            rule_name: planned.rule_name.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PlanRecord {
    pub id: u64,
    pub moves: Vec<MoveRecord>,
    pub unchanged: u64,
    /// Categories whose files stayed put because they have no destination.
    pub missing_categories: Vec<FileCategory>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ApplyRecord {
    pub moved: u64,
    /// Files copied because the original's folder isn't writable.
    pub copied: u64,
    pub failures: Vec<FailureRecord>,
    pub warning: Option<UserTextRecord>,
}

impl From<&ApplyReport> for ApplyRecord {
    fn from(report: &ApplyReport) -> Self {
        let count = |mode| {
            report
                .batch
                .applied
                .iter()
                .filter(|a| a.mode == mode)
                .count() as u64
        };
        Self {
            moved: count(Mode::Moved),
            copied: count(Mode::Copied),
            failures: report.failures.iter().map(Into::into).collect(),
            warning: report.warning.as_ref().map(Into::into),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DuplicateGroupRecord {
    pub paths: Vec<String>,
    pub size: u64,
    pub exact: bool,
    pub reclaimable_bytes: u64,
}

impl From<&DuplicateGroup> for DuplicateGroupRecord {
    fn from(group: &DuplicateGroup) -> Self {
        Self {
            paths: group.paths.iter().map(|p| path_string(p)).collect(),
            size: group.size,
            exact: group.exact,
            reclaimable_bytes: group.reclaimable_bytes(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ImageFormat {
    Jpeg { quality: u8 },
    Png,
    WebpLossless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct CompressSettings {
    pub format: ImageFormat,
    pub max_dimension: Option<u32>,
}

impl From<CompressSettings> for CompressOptions {
    fn from(settings: CompressSettings) -> Self {
        let format = match settings.format {
            ImageFormat::Jpeg { quality } => OutputFormat::Jpeg { quality },
            ImageFormat::Png => OutputFormat::Png,
            ImageFormat::WebpLossless => OutputFormat::WebpLossless,
        };
        Self {
            format,
            max_dimension: settings.max_dimension,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum CompressResult {
    Written {
        source: String,
        output: String,
        before: u64,
        after: u64,
    },
    NotSmaller {
        source: String,
    },
    Failed {
        path: String,
        message: UserTextRecord,
    },
}

impl From<&CompressOutcome> for CompressResult {
    fn from(outcome: &CompressOutcome) -> Self {
        match outcome {
            CompressOutcome::Written {
                source,
                output,
                before,
                after,
            } => Self::Written {
                source: path_string(source),
                output: path_string(output),
                before: *before,
                after: *after,
            },
            CompressOutcome::NotSmaller { source } => Self::NotSmaller {
                source: path_string(source),
            },
            CompressOutcome::Failed(failure) => Self::Failed {
                path: path_string(&failure.path),
                message: (&failure.message).into(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RuleRecord {
    pub id: String,
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CollectionRecord {
    pub id: String,
    pub name: String,
    pub icon: String,
}

/// How far an [`crate::EngineJob`] has got. `total` is 0 while the engine
/// is still counting the work; show an indeterminate indicator then.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct ProgressRecord {
    pub done: u64,
    pub total: u64,
}

impl From<app_core::Progress> for ProgressRecord {
    fn from(progress: app_core::Progress) -> Self {
        Self {
            done: progress.done,
            total: progress.total,
        }
    }
}
