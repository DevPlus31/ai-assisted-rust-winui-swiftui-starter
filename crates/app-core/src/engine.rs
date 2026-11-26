//! The app engine: one object both shells drive.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::analyze::{
    Classifier, FileInsight, HeuristicClassifier, analyze_files, analyze_files_in,
};
use crate::collections::{SmartCollection, default_collections};
use crate::compress::{CompressOptions, CompressOutcome, compress_images};
use crate::date::CivilDate;
use crate::dedupe::{DuplicateGroup, find_exact_duplicates, find_similar_images};
use crate::error::{CoreError, FileFailure};
use crate::files::{FileKind, scan_folder};
use crate::job::Job;
use crate::store::JsonStore;
use crate::user_text::{TextKey, UserText};
use crate::workflow::{self, ApplyReport, Batch, Destinations, Plan, Rule, default_rules};

/// Undo history depth.
const MAX_UNDO_BATCHES: usize = 20;
pub const SETTINGS_FILE: &str = "settings.json";
pub const JOURNAL_FILE: &str = "undo-journal.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub rules: Vec<Rule>,
    pub collections: Vec<SmartCollection>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            rules: default_rules(),
            collections: default_collections(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
struct Journal {
    batches: Vec<Batch>,
}

/// A plan the engine produced and will agree to execute. Shells reference
/// it by id, so they can never ask the engine to move arbitrary paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPlan {
    pub id: u64,
    pub plan: Plan,
}

pub struct Engine {
    settings_store: JsonStore<Settings>,
    journal_store: JsonStore<Journal>,
    settings: Settings,
    journal: Journal,
    pending: Option<PendingPlan>,
    next_plan_id: u64,
    classifier: Box<dyn Classifier>,
    notices: Vec<UserText>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("rules", &self.settings.rules.len())
            .field("undo_batches", &self.journal.batches.len())
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// Opens the app's data in `data_dir` (must be absolute). Corrupt files
    /// are set aside and reported via [`Engine::startup_notices`].
    ///
    /// # Errors
    /// [`CoreError::InvalidInput`] for a relative path; [`CoreError::Storage`]
    /// when existing data can't be read.
    pub fn open(data_dir: &Path) -> Result<Self, CoreError> {
        if !data_dir.is_absolute() {
            return Err(CoreError::invalid(TextKey::DataDirNotAbsolute));
        }
        let settings_store = JsonStore::new(data_dir, SETTINGS_FILE);
        let journal_store = JsonStore::new(data_dir, JOURNAL_FILE);
        let settings = settings_store.open()?;
        let journal = journal_store.open()?;
        let mut notices = Vec::new();
        if settings.recovered {
            notices.push(TextKey::RulesReset.into());
        }
        if journal.recovered {
            notices.push(TextKey::UndoHistoryCleared.into());
        }
        Ok(Self {
            settings_store,
            journal_store,
            settings: settings.value,
            journal: journal.value,
            pending: None,
            next_plan_id: 1,
            classifier: Box::new(HeuristicClassifier),
            notices,
        })
    }

    pub fn startup_notices(&self) -> &[UserText] {
        &self.notices
    }

    /// Analyses the files in `folder`, reporting progress to `job`. Pass
    /// a fresh [`Job::new`] when nothing needs to watch or cancel it.
    ///
    /// # Errors
    /// [`CoreError::FolderUnreadable`] when the folder can't be listed;
    /// [`CoreError::Cancelled`] when `job` was cancelled.
    pub fn analyze_folder(
        &self,
        folder: &Path,
        recursive: bool,
        job: &Job,
    ) -> Result<(Vec<FileInsight>, Vec<FileFailure>), CoreError> {
        let paths = scan_folder(folder, recursive)?;
        if job.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        analyze_files_in(&paths, self.classifier.as_ref(), job)
    }

    pub fn analyze(&self, paths: &[PathBuf]) -> (Vec<FileInsight>, Vec<FileFailure>) {
        analyze_files(paths, self.classifier.as_ref())
    }

    /// Plans organizing `insights` into their categories' `destinations`
    /// with the enabled rules and remembers the plan (replacing any earlier
    /// unapplied one).
    ///
    /// # Errors
    /// [`CoreError::InvalidInput`] when no destination is set or one isn't
    /// an absolute path.
    pub fn plan(
        &mut self,
        insights: &[FileInsight],
        destinations: &Destinations,
    ) -> Result<PendingPlan, CoreError> {
        destinations.validate()?;
        let plan = workflow::plan(insights, &self.settings.rules, destinations, today());
        let pending = PendingPlan {
            id: self.next_plan_id,
            plan,
        };
        self.next_plan_id += 1;
        self.pending = Some(pending.clone());
        Ok(pending)
    }

    /// Executes the remembered plan, minus the moves the user unticked.
    ///
    /// # Errors
    /// [`CoreError::InvalidInput`] when `plan_id` is not the current plan.
    pub fn apply_plan(&mut self, plan_id: u64, skip: &[usize]) -> Result<ApplyReport, CoreError> {
        let pending = self
            .pending
            .take_if(|p| p.id == plan_id)
            .ok_or_else(|| CoreError::invalid(TextKey::PreviewOutOfDate))?;
        let selected = Plan {
            moves: pending
                .plan
                .moves
                .into_iter()
                .enumerate()
                .filter(|(index, _)| !skip.contains(index))
                .map(|(_, m)| m)
                .collect(),
            ..pending.plan
        };
        // Record the batch while it runs, so a crash or force-quit part-way
        // through still leaves everything moved so far undoable.
        self.journal.batches.push(Batch::default());
        let slot = self.journal.batches.len() - 1;
        let journal = &mut self.journal;
        let store = &self.journal_store;
        let mut report = workflow::apply(&selected, &mut |partial| {
            journal.batches[slot] = partial.clone();
            let _ = store.save(journal); // best effort; the final save reports
        });
        if report.batch.applied.is_empty() {
            self.journal.batches.pop();
        } else {
            self.journal.batches[slot] = report.batch.clone();
            let overflow = self.journal.batches.len().saturating_sub(MAX_UNDO_BATCHES);
            self.journal.batches.drain(..overflow);
        }
        if self.journal_store.save(&self.journal).is_err() && !report.batch.applied.is_empty() {
            report.warning = Some(TextKey::UndoNotSavedAfterApply.into());
        }
        Ok(report)
    }

    pub fn can_undo(&self) -> bool {
        !self.journal.batches.is_empty()
    }

    /// # Errors
    /// [`CoreError::NothingToUndo`] when the history is empty.
    pub fn undo_last(&mut self) -> Result<ApplyReport, CoreError> {
        let batch = self.journal.batches.pop().ok_or(CoreError::NothingToUndo)?;
        let (mut report, remaining) = workflow::undo(&batch);
        if !remaining.applied.is_empty() {
            // Keep what couldn't be restored, so the user can retry.
            self.journal.batches.push(remaining);
        }
        if self.journal_store.save(&self.journal).is_err() {
            report.warning = Some(TextKey::UndoHistoryNotSaved.into());
        }
        Ok(report)
    }

    /// Exact duplicates among all files plus visually similar photos.
    pub fn find_duplicates(&self, insights: &[FileInsight]) -> Vec<DuplicateGroup> {
        let all: Vec<PathBuf> = insights.iter().map(|i| i.path.clone()).collect();
        let exact = find_exact_duplicates(&all);
        let images: Vec<PathBuf> = insights
            .iter()
            .filter(|i| i.kind == FileKind::Image)
            .filter(|i| !exact.iter().any(|g| g.paths[1..].contains(&i.path)))
            .map(|i| i.path.clone())
            .collect();
        exact
            .into_iter()
            .chain(find_similar_images(&images))
            .collect()
    }

    pub fn compress(
        &self,
        sources: &[PathBuf],
        output_dir: &Path,
        options: CompressOptions,
    ) -> Vec<CompressOutcome> {
        compress_images(sources, output_dir, options)
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Validates and persists new rules/collections.
    ///
    /// # Errors
    /// [`CoreError::InvalidInput`] for blank names or duplicate ids;
    /// [`CoreError::Storage`] when saving fails (old settings stay active).
    pub fn update_settings(&mut self, settings: Settings) -> Result<(), CoreError> {
        validate(&settings)?;
        self.settings_store.save(&settings)?;
        self.settings = settings;
        self.pending = None; // plans made with old rules are stale
        Ok(())
    }

    pub fn collection_members<'a>(
        &self,
        collection_id: &str,
        insights: &'a [FileInsight],
    ) -> Vec<&'a FileInsight> {
        self.settings
            .collections
            .iter()
            .find(|c| c.id == collection_id)
            .map(|c| c.filter(insights, today()))
            .unwrap_or_default()
    }
}

fn validate(settings: &Settings) -> Result<(), CoreError> {
    for rule in &settings.rules {
        if let workflow::Destination::Folder(relative) = &rule.destination
            && !is_portable_relative_folder(relative)
        {
            return Err(CoreError::InvalidInput(
                UserText::new(TextKey::FolderNameNotPortable).arg("folder", relative.clone()),
            ));
        }
    }
    let rules = settings
        .rules
        .iter()
        .map(|r| (r.id.as_str(), r.name.as_str()));
    let collections = settings
        .collections
        .iter()
        .map(|c| (c.id.as_str(), c.name.as_str()));
    validate_entries(rules)?;
    validate_entries(collections)
}

/// Names must be non-blank; ids unique within their list.
fn validate_entries<'a>(
    entries: impl Iterator<Item = (&'a str, &'a str)>,
) -> Result<(), CoreError> {
    let mut ids = std::collections::HashSet::new();
    for (id, name) in entries {
        if name.trim().is_empty() {
            return Err(CoreError::invalid(TextKey::NameRequired));
        }
        if id.trim().is_empty() || !ids.insert(id) {
            return Err(CoreError::invalid(TextKey::IdsNotUnique));
        }
    }
    Ok(())
}

/// Plain relative folders whose every component is a valid, portable name
/// (no `..`, no reserved characters or device names, no trailing dots).
fn is_portable_relative_folder(relative: &str) -> bool {
    let components: Vec<&str> = relative.split(['/', '\\']).collect();
    !relative.trim().is_empty()
        && components.iter().all(|component| {
            !component.is_empty()
                && *component != "."
                && *component != ".."
                && crate::naming::sanitize_stem(component) == *component
        })
}

fn today() -> CivilDate {
    CivilDate::today()
        .or_else(|| CivilDate::new(2000, 1, 1))
        .expect("2000-01-01 is a valid date")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup() -> (tempfile::TempDir, Engine) {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(&dir.path().join("data")).unwrap();
        (dir, engine)
    }

    #[test]
    fn analysis_reports_progress_for_every_file() {
        let (dir, engine) = setup();
        let inbox = dir.path().join("inbox");
        fs::create_dir(&inbox).unwrap();
        for i in 0..5 {
            fs::write(inbox.join(format!("note-{i}.txt")), "hello").unwrap();
        }
        let job = Job::new();

        let (insights, _) = engine.analyze_folder(&inbox, false, &job).unwrap();

        assert_eq!(insights.len(), 5);
        assert_eq!(job.progress(), crate::Progress { done: 5, total: 5 });
    }

    #[test]
    fn a_cancelled_analysis_returns_cancelled_and_no_files() {
        let (dir, engine) = setup();
        fs::write(dir.path().join("note.txt"), "hello").unwrap();
        let job = Job::new();
        job.cancel();

        let result = engine.analyze_folder(dir.path(), false, &job);

        assert_eq!(result.unwrap_err(), CoreError::Cancelled);
    }

    #[test]
    fn organizes_a_messy_folder_and_undoes_it() {
        let (dir, mut engine) = setup();
        let inbox = dir.path().join("Desktop");
        fs::create_dir(&inbox).unwrap();
        fs::write(
            inbox.join("scan_01.txt"),
            "Invoice number 9. Bill to: Sam. Amount due 2026-05-04",
        )
        .unwrap();
        fs::write(inbox.join("notes.md"), "# Ideas").unwrap();
        let root = dir.path().join("Organized");

        let (insights, failures) = engine.analyze_folder(&inbox, false, &Job::new()).unwrap();
        assert!(failures.is_empty());
        let pending = engine
            .plan(&insights, &Destinations::single(&root))
            .unwrap();
        let report = engine.apply_plan(pending.id, &[]).unwrap();

        assert!(report.failures.is_empty());
        assert!(root.join("Invoices/2026-05-04 Invoice.txt").exists());
        assert!(root.join("Documents/notes.md").exists());
        assert!(engine.can_undo());

        // Undo survives a restart.
        let mut reopened = Engine::open(&dir.path().join("data")).unwrap();
        reopened.undo_last().unwrap();
        assert!(inbox.join("scan_01.txt").exists());
        assert!(inbox.join("notes.md").exists());
        assert!(!root.exists());
        assert_eq!(reopened.undo_last().unwrap_err(), CoreError::NothingToUndo);
    }

    #[test]
    fn only_the_current_plan_can_be_applied_and_skips_are_honoured() {
        let (dir, mut engine) = setup();
        let inbox = dir.path().join("in");
        fs::create_dir(&inbox).unwrap();
        fs::write(inbox.join("a.txt"), "a").unwrap();
        fs::write(inbox.join("b.txt"), "b").unwrap();
        let (insights, _) = engine.analyze_folder(&inbox, false, &Job::new()).unwrap();
        let out = Destinations::single(dir.path().join("out"));
        let first = engine.plan(&insights, &out).unwrap();
        let second = engine.plan(&insights, &out).unwrap();

        assert!(matches!(
            engine.apply_plan(first.id, &[]),
            Err(CoreError::InvalidInput(_))
        ));
        let report = engine.apply_plan(second.id, &[0]).unwrap();

        assert_eq!(report.batch.applied.len(), 1);
        assert!(inbox.join("a.txt").exists(), "skipped move left in place");
        assert!(
            matches!(
                engine.apply_plan(second.id, &[]),
                Err(CoreError::InvalidInput(_))
            ),
            "plans run once"
        );
    }

    #[test]
    fn planning_needs_a_valid_destination() {
        let (dir, mut engine) = setup();
        fs::write(dir.path().join("a.txt"), "a").unwrap();
        let (insights, _) = engine
            .analyze_folder(dir.path(), false, &Job::new())
            .unwrap();

        assert!(matches!(
            engine.plan(&insights, &Destinations::default()),
            Err(CoreError::InvalidInput(_))
        ));
        assert!(matches!(
            engine.plan(&insights, &Destinations::single("relative")),
            Err(CoreError::InvalidInput(_))
        ));
    }

    #[test]
    fn settings_are_validated_and_persisted() {
        let (dir, mut engine) = setup();
        let mut settings = engine.settings().clone();
        settings.rules[0].name = "  ".into();
        assert!(engine.update_settings(settings.clone()).is_err());
        let mut duplicate_ids = engine.settings().clone();
        let first_id = duplicate_ids.rules[0].id.clone();
        duplicate_ids.rules[1].id = first_id;
        assert!(engine.update_settings(duplicate_ids).is_err());

        settings.rules[0].name = "Paperwork".into();
        settings.rules[1].enabled = false;
        engine.update_settings(settings.clone()).unwrap();

        assert_eq!(
            Engine::open(&dir.path().join("data")).unwrap().settings(),
            &settings
        );
    }

    #[test]
    fn rule_folders_must_be_portable() {
        assert!(is_portable_relative_folder("Work/2026 Taxes"));
        assert!(!is_portable_relative_folder("../escape"));
        assert!(!is_portable_relative_folder("Bad:Name"));
        assert!(!is_portable_relative_folder("CON"));
        assert!(!is_portable_relative_folder("trailing."));
        assert!(!is_portable_relative_folder("/abs"));
    }

    #[test]
    fn corrupt_settings_restore_defaults_with_a_notice() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(SETTINGS_FILE), "{oops").unwrap();

        let engine = Engine::open(dir.path()).unwrap();

        assert_eq!(engine.settings(), &Settings::default());
        assert_eq!(engine.startup_notices().len(), 1);
    }

    #[test]
    fn relative_data_dirs_are_rejected() {
        assert!(matches!(
            Engine::open(Path::new("data")),
            Err(CoreError::InvalidInput(_))
        ));
    }

    #[test]
    fn duplicates_include_exact_copies() {
        let (dir, engine) = setup();
        fs::write(dir.path().join("a.txt"), "same").unwrap();
        fs::write(dir.path().join("b.txt"), "same").unwrap();
        let (insights, _) = engine
            .analyze_folder(dir.path(), false, &Job::new())
            .unwrap();

        let groups = engine.find_duplicates(&insights);

        assert_eq!(groups.len(), 1);
        assert!(groups[0].exact);
    }
}
