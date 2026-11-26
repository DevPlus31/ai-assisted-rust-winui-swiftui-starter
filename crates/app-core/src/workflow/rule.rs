//! User-editable automation rules ("workflows") and smart-collection
//! filters share one condition language.

use serde::{Deserialize, Serialize};

use crate::analyze::{Category, FileInsight};
use crate::date::CivilDate;
use crate::files::{FileKind, extension_of};

const BYTES_PER_MB: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Condition {
    CategoryIs(Vec<Category>),
    KindIs(Vec<FileKind>),
    /// Lower-case extensions without the dot.
    ExtensionIs(Vec<String>),
    /// Case-insensitive substring of the file name.
    NameContains(String),
    /// Not modified for at least this many days.
    OlderThanDays(u32),
    LargerThanMb(u64),
}

impl Condition {
    pub fn matches(&self, insight: &FileInsight, today: CivilDate) -> bool {
        match self {
            Self::CategoryIs(categories) => categories.contains(&insight.category),
            Self::KindIs(kinds) => kinds.contains(&insight.kind),
            Self::ExtensionIs(extensions) => {
                let ext = extension_of(&insight.path);
                extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext))
            }
            Self::NameContains(needle) => {
                insight.name.to_lowercase().contains(&needle.to_lowercase())
            }
            Self::OlderThanDays(days) => insight
                .modified
                .is_some_and(|m| today.days_since(m) >= i64::from(*days)),
            Self::LargerThanMb(mb) => insight.size > mb.saturating_mul(BYTES_PER_MB),
        }
    }
}

/// Every condition must match (an empty list matches all files).
pub fn all_match(conditions: &[Condition], insight: &FileInsight, today: CivilDate) -> bool {
    conditions.iter().all(|c| c.matches(insight, today))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Destination {
    /// Leave the file in its folder (rename only).
    Stay,
    /// `<root>/<Category folder>` — e.g. `Invoices`, `Screenshots`.
    CategoryFolder,
    /// `<root>/<relative path>`; validated to stay inside the root.
    Folder(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub conditions: Vec<Condition>,
    pub destination: Destination,
    /// Apply the smart name (only when classification is confident).
    pub rename: bool,
}

impl Rule {
    fn new(
        id: &str,
        name: &str,
        conditions: Vec<Condition>,
        destination: Destination,
        rename: bool,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            enabled: true,
            conditions,
            destination,
            rename,
        }
    }
}

/// Sensible starting workflows; the first matching enabled rule wins.
pub fn default_rules() -> Vec<Rule> {
    vec![
        Rule::new(
            "documents",
            "File important documents",
            vec![Condition::CategoryIs(vec![
                Category::Invoice,
                Category::Receipt,
                Category::Contract,
                Category::Resume,
                Category::BankStatement,
                Category::TaxDocument,
            ])],
            Destination::CategoryFolder,
            true,
        ),
        Rule::new(
            "screenshots",
            "Collect screenshots",
            vec![Condition::CategoryIs(vec![Category::Screenshot])],
            Destination::CategoryFolder,
            true,
        ),
        Rule::new(
            "photos",
            "Date and file photos",
            vec![Condition::CategoryIs(vec![Category::Photo])],
            Destination::CategoryFolder,
            true,
        ),
        Rule::new(
            "by-type",
            "Sort everything else by type",
            vec![],
            Destination::CategoryFolder,
            false,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn insight(
        name: &str,
        category: Category,
        size: u64,
        modified: Option<CivilDate>,
    ) -> FileInsight {
        FileInsight {
            path: PathBuf::from("/in").join(name),
            name: name.into(),
            size,
            kind: FileKind::Other,
            category,
            confidence: 0.9,
            reason: String::new(),
            date: modified,
            modified,
            modified_at: None,
            suggested_name: name.into(),
            dimensions: None,
        }
    }

    #[test]
    fn conditions_match_insights() {
        let today = CivilDate::new(2026, 10, 18).unwrap();
        let file = insight(
            "Big Report.PDF",
            Category::Document,
            5 * BYTES_PER_MB,
            CivilDate::new(2026, 1, 1),
        );

        assert!(Condition::ExtensionIs(vec!["pdf".into()]).matches(&file, today));
        assert!(Condition::NameContains("report".into()).matches(&file, today));
        assert!(Condition::LargerThanMb(4).matches(&file, today));
        assert!(!Condition::LargerThanMb(5).matches(&file, today));
        assert!(Condition::OlderThanDays(90).matches(&file, today));
        assert!(!Condition::OlderThanDays(365).matches(&file, today));
        assert!(!Condition::CategoryIs(vec![Category::Invoice]).matches(&file, today));
        assert!(all_match(&[], &file, today));
    }

    #[test]
    fn unknown_modification_date_is_never_old() {
        let today = CivilDate::new(2026, 10, 18).unwrap();
        let file = insight("a", Category::Other, 1, None);
        assert!(!Condition::OlderThanDays(1).matches(&file, today));
    }

    #[test]
    fn rules_round_trip_through_json() {
        let rules = default_rules();
        let json = serde_json::to_string(&rules).unwrap();
        assert_eq!(serde_json::from_str::<Vec<Rule>>(&json).unwrap(), rules);
    }
}
