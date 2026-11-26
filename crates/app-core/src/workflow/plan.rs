//! Turns insights + rules into a reviewable plan. Planning touches nothing
//! on disk; the user always sees exactly what will happen first.

use std::collections::{BTreeSet, HashSet};
use std::path::{Component, Path, PathBuf};

use super::destinations::Destinations;
use super::rule::{Destination, Rule, all_match};
use crate::analyze::{Category, FileInsight};
use crate::date::CivilDate;
use crate::naming::unique_path;

/// Below this classification confidence, files are moved by type but keep
/// their original names.
pub const RENAME_MIN_CONFIDENCE: f32 = 0.6;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlannedMove {
    pub from: PathBuf,
    pub to: PathBuf,
    pub rule_name: String,
    /// Size and modification time at analysis; apply refuses to move a
    /// file that changed since the user reviewed the plan.
    pub expected_size: u64,
    pub expected_modified: Option<std::time::SystemTime>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Plan {
    pub moves: Vec<PlannedMove>,
    /// Files no rule changed.
    pub unchanged: usize,
    /// Categories of files left in place because they have no destination
    /// (sorted, no repeats). Their files are not counted in `unchanged`.
    #[serde(default)]
    pub missing: Vec<Category>,
}

/// Builds the plan. Each file goes to its category's destination.
/// Target names never collide with existing files or with each other
/// (comparison is case-insensitive, as on APFS and NTFS defaults).
pub fn plan(
    insights: &[FileInsight],
    rules: &[Rule],
    destinations: &Destinations,
    today: CivilDate,
) -> Plan {
    let mut reserved: HashSet<String> = HashSet::new();
    let mut missing: BTreeSet<Category> = BTreeSet::new();
    let mut result = Plan::default();
    for insight in insights {
        let Some(rule) = rules
            .iter()
            .find(|r| r.enabled && all_match(&r.conditions, insight, today))
        else {
            result.unchanged += 1;
            continue;
        };
        let Some(directory) = target_directory(insight, &rule.destination, destinations) else {
            if matches!(rule.destination, Destination::Stay) {
                result.unchanged += 1;
            } else {
                missing.insert(insight.category);
            }
            continue;
        };
        let Some(original_name) = insight.path.file_name() else {
            result.unchanged += 1;
            continue;
        };
        // Names that aren't valid Unicode are never rewritten (a lossy
        // conversion would corrupt them); they are only moved.
        let renamable = original_name.to_str().is_some();
        let candidate = if rule.rename && renamable && insight.confidence >= RENAME_MIN_CONFIDENCE {
            directory.join(&insight.suggested_name)
        } else {
            directory.join(original_name)
        };
        // Exact same path: nothing to do. A case-only difference is a rename.
        if candidate == insight.path {
            result.unchanged += 1;
            continue;
        }
        let to = unique_path(&candidate, |p| {
            reserved.contains(&key(p)) || (p.exists() && !same_path(p, &insight.path))
        });
        reserved.insert(key(&to));
        result.moves.push(PlannedMove {
            from: insight.path.clone(),
            to,
            rule_name: rule.name.clone(),
            expected_size: insight.size,
            expected_modified: insight.modified_at,
        });
    }
    Plan {
        missing: missing.into_iter().collect(),
        ..result
    }
}

fn target_directory(
    insight: &FileInsight,
    destination: &Destination,
    destinations: &Destinations,
) -> Option<PathBuf> {
    match destination {
        Destination::Stay => insight.path.parent().map(Path::to_path_buf),
        Destination::CategoryFolder => destinations.category_folder(insight.category),
        Destination::Folder(relative) => {
            let base = destinations.base_for(insight.category)?;
            safe_relative(relative).map(|r| base.join(r))
        }
    }
}

/// Accepts only plain relative paths: no `..`, no absolute paths, no drive
/// prefixes — a rule can never move files outside the chosen root.
fn safe_relative(relative: &str) -> Option<PathBuf> {
    let path = Path::new(relative);
    let plain = path.components().all(|c| matches!(c, Component::Normal(_)));
    (plain && !relative.trim().is_empty()).then(|| path.to_path_buf())
}

fn key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

fn same_path(a: &Path, b: &Path) -> bool {
    key(a) == key(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze::Category;
    use crate::files::FileKind;
    use crate::workflow::rule::{Condition, default_rules};

    fn today() -> CivilDate {
        CivilDate::new(2026, 10, 18).unwrap()
    }

    fn org() -> Destinations {
        Destinations::single("/org")
    }

    fn insight(path: &Path, category: Category, confidence: f32, suggested: &str) -> FileInsight {
        FileInsight {
            path: path.to_path_buf(),
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            size: 1,
            kind: FileKind::Pdf,
            category,
            confidence,
            reason: String::new(),
            date: None,
            modified: None,
            modified_at: None,
            suggested_name: suggested.into(),
            dimensions: None,
        }
    }

    #[test]
    fn sorts_into_category_folders_and_renames_confident_files() {
        let root = Path::new("/org");
        let invoice = insight(
            Path::new("/in/scan.pdf"),
            Category::Invoice,
            0.9,
            "2026-01-02 Invoice.pdf",
        );
        let unsure = insight(
            Path::new("/in/misc.pdf"),
            Category::Document,
            0.5,
            "Document.pdf",
        );

        let plan = plan(
            &[invoice, unsure],
            &default_rules(),
            &Destinations::single(root),
            today(),
        );

        let targets: Vec<_> = plan.moves.iter().map(|m| m.to.clone()).collect();
        assert_eq!(
            targets,
            vec![
                PathBuf::from("/org/Invoices/2026-01-02 Invoice.pdf"),
                PathBuf::from("/org/Documents/misc.pdf")
            ]
        );
        assert_eq!(plan.moves[0].rule_name, "File important documents");
    }

    #[test]
    fn colliding_targets_get_distinct_names() {
        let a = insight(
            Path::new("/in/a.pdf"),
            Category::Invoice,
            0.9,
            "Invoice.pdf",
        );
        let b = insight(
            Path::new("/in/b.pdf"),
            Category::Invoice,
            0.9,
            "invoice.pdf",
        );

        let plan = plan(&[a, b], &default_rules(), &org(), today());

        assert_eq!(plan.moves[0].to, PathBuf::from("/org/Invoices/Invoice.pdf"));
        assert_eq!(
            plan.moves[1].to,
            PathBuf::from("/org/Invoices/invoice (2).pdf")
        );
    }

    #[test]
    fn existing_files_are_never_targets() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Invoices")).unwrap();
        std::fs::write(dir.path().join("Invoices/x.pdf"), b"keep me").unwrap();
        let file = insight(&dir.path().join("new.pdf"), Category::Invoice, 0.9, "x.pdf");

        let plan = plan(
            &[file],
            &default_rules(),
            &Destinations::single(dir.path()),
            today(),
        );

        assert_eq!(plan.moves[0].to, dir.path().join("Invoices/x (2).pdf"));
    }

    #[test]
    fn files_already_in_place_are_unchanged() {
        let file = insight(
            Path::new("/org/Invoices/a.pdf"),
            Category::Invoice,
            0.9,
            "a.pdf",
        );
        let plan = plan(&[file], &default_rules(), &org(), today());
        assert!(plan.moves.is_empty());
        assert_eq!(plan.unchanged, 1);
    }

    #[test]
    fn folder_destinations_cannot_escape_the_root() {
        let mut rule = default_rules().remove(3);
        rule.destination = Destination::Folder("../../etc".into());
        rule.conditions = vec![Condition::CategoryIs(vec![Category::Invoice])];
        let file = insight(Path::new("/in/a.pdf"), Category::Invoice, 0.9, "a.pdf");

        let plan = plan(&[file], &[rule], &org(), today());

        assert!(plan.moves.is_empty());
        assert_eq!(safe_relative("Work/2026"), Some(PathBuf::from("Work/2026")));
        assert_eq!(safe_relative("/abs"), None);
    }

    #[test]
    fn each_category_goes_to_its_own_folder() {
        let destinations = Destinations::default()
            .with(Category::Invoice, "/finance")
            .with(Category::Photo, "/pictures");
        let invoice = insight(
            Path::new("/in/scan.pdf"),
            Category::Invoice,
            0.9,
            "Invoice.pdf",
        );
        let photo = insight(Path::new("/in/IMG_1.jpg"), Category::Photo, 0.4, "x.jpg");
        let video = insight(Path::new("/in/a.mov"), Category::Video, 0.9, "a.mov");
        let audio = insight(Path::new("/in/b.mp3"), Category::Audio, 0.9, "b.mp3");
        let video2 = insight(Path::new("/in/c.mov"), Category::Video, 0.9, "c.mov");

        let plan = plan(
            &[invoice, photo, video, audio, video2],
            &default_rules(),
            &destinations,
            today(),
        );

        let targets: Vec<_> = plan.moves.iter().map(|m| m.to.clone()).collect();
        assert_eq!(
            targets,
            vec![
                PathBuf::from("/finance/Invoice.pdf"),
                PathBuf::from("/pictures/IMG_1.jpg")
            ]
        );
        assert_eq!(plan.missing, vec![Category::Audio, Category::Video]);
        assert_eq!(plan.unchanged, 0);
    }

    #[test]
    fn a_default_folder_catches_categories_without_their_own() {
        let destinations = org().with(Category::Photo, "/pictures");
        let photo = insight(Path::new("/in/IMG_1.jpg"), Category::Photo, 0.4, "x.jpg");
        let video = insight(Path::new("/in/a.mov"), Category::Video, 0.9, "a.mov");

        let plan = plan(&[photo, video], &default_rules(), &destinations, today());

        assert_eq!(plan.moves[0].to, PathBuf::from("/pictures/IMG_1.jpg"));
        assert_eq!(plan.moves[1].to, PathBuf::from("/org/Videos/a.mov"));
        assert!(plan.missing.is_empty());
    }

    #[test]
    fn disabled_rules_are_skipped() {
        let mut rules = default_rules();
        for rule in &mut rules {
            rule.enabled = false;
        }
        let file = insight(Path::new("/in/a.pdf"), Category::Invoice, 0.9, "a.pdf");
        assert_eq!(plan(&[file], &rules, &org(), today()).unchanged, 1);
    }
}
