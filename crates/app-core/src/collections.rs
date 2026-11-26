//! Smart collections: saved, live filters over analysed files. They are
//! virtual — nothing moves until the user runs a workflow on them.

use serde::{Deserialize, Serialize};

use crate::analyze::{Category, FileInsight};
use crate::date::CivilDate;
use crate::files::FileKind;
use crate::workflow::{Condition, all_match};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartCollection {
    pub id: String,
    pub name: String,
    /// Platform-neutral icon hint (shells map it to SF Symbols / Segoe icons).
    pub icon: String,
    pub conditions: Vec<Condition>,
}

impl SmartCollection {
    pub fn filter<'a>(
        &self,
        insights: &'a [FileInsight],
        today: CivilDate,
    ) -> Vec<&'a FileInsight> {
        insights
            .iter()
            .filter(|i| all_match(&self.conditions, i, today))
            .collect()
    }
}

fn collection(id: &str, name: &str, icon: &str, conditions: Vec<Condition>) -> SmartCollection {
    SmartCollection {
        id: id.into(),
        name: name.into(),
        icon: icon.into(),
        conditions,
    }
}

pub fn default_collections() -> Vec<SmartCollection> {
    vec![
        collection(
            "paperwork",
            "Invoices & Receipts",
            "receipt",
            vec![Condition::CategoryIs(vec![
                Category::Invoice,
                Category::Receipt,
            ])],
        ),
        collection(
            "screenshots",
            "Screenshots",
            "screenshot",
            vec![Condition::CategoryIs(vec![Category::Screenshot])],
        ),
        collection(
            "photos",
            "Photos",
            "photo",
            vec![Condition::CategoryIs(vec![Category::Photo])],
        ),
        collection(
            "large",
            "Large Files",
            "large",
            vec![Condition::LargerThanMb(100)],
        ),
        collection(
            "stale-installers",
            "Old Installers",
            "installer",
            vec![
                Condition::KindIs(vec![FileKind::Installer]),
                Condition::OlderThanDays(30),
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn insight(category: Category, kind: FileKind, size: u64) -> FileInsight {
        FileInsight {
            path: PathBuf::from("/x"),
            name: "x".into(),
            size,
            kind,
            category,
            confidence: 0.9,
            reason: String::new(),
            date: None,
            modified: CivilDate::new(2026, 1, 1),
            modified_at: None,
            suggested_name: "x".into(),
            dimensions: None,
        }
    }

    #[test]
    fn default_collections_filter_as_expected() {
        let today = CivilDate::new(2026, 10, 18).unwrap();
        let files = [
            insight(Category::Receipt, FileKind::Pdf, 10),
            insight(Category::Installer, FileKind::Installer, 500 * 1024 * 1024),
        ];
        let by_id = |id: &str| {
            default_collections()
                .into_iter()
                .find(|c| c.id == id)
                .unwrap()
        };

        assert_eq!(by_id("paperwork").filter(&files, today).len(), 1);
        assert_eq!(by_id("large").filter(&files, today).len(), 1);
        assert_eq!(by_id("stale-installers").filter(&files, today).len(), 1);
        assert!(by_id("photos").filter(&files, today).is_empty());
    }
}
