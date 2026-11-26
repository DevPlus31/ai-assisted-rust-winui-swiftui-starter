//! Swift-facing API generated with UniFFI. Translation only — behaviour
//! lives in `app-core`. UniFFI turns Rust panics into Swift errors, so no
//! unwinding crosses into Swift.
//!
//! Calls may take a while (analysis, hashing); Swift must call them off the
//! main actor. The engine is internally locked, so calls are serialized.

mod records;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use app_core::analyze::{Category, FileInsight};
use app_core::{CoreError, Engine, Job, TextKey};

pub use records::*;

uniffi::setup_scaffolding!();

/// Every category, in display order.
#[uniffi::export]
pub fn file_categories() -> Vec<CategoryInfo> {
    Category::ALL.into_iter().map(Into::into).collect()
}

/// Errors carry a user-presentable message only: `message` in English,
/// `text` to translate (see [`UserTextRecord`]).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum AppError {
    #[error("{message}")]
    InvalidInput {
        message: String,
        text: UserTextRecord,
    },
    #[error("{message}")]
    NothingToUndo {
        message: String,
        text: UserTextRecord,
    },
    #[error("{message}")]
    Storage {
        message: String,
        text: UserTextRecord,
    },
    /// The user stopped the work with [`EngineJob::cancel`]. Not a failure:
    /// shells usually just return to the previous screen.
    #[error("{message}")]
    Cancelled {
        message: String,
        text: UserTextRecord,
    },
}

impl From<CoreError> for AppError {
    fn from(error: CoreError) -> Self {
        let user_text = error.user_text();
        let (message, text) = (user_text.to_string(), UserTextRecord::from(&user_text));
        match error {
            CoreError::InvalidInput(_) => Self::InvalidInput { message, text },
            CoreError::NothingToUndo => Self::NothingToUndo { message, text },
            CoreError::Storage(_) | CoreError::Corrupt(_) | CoreError::FolderUnreadable(_) => {
                Self::Storage { message, text }
            }
            CoreError::Cancelled => Self::Cancelled { message, text },
        }
    }
}

impl From<TextKey> for AppError {
    fn from(key: TextKey) -> Self {
        CoreError::InvalidInput(key.into()).into()
    }
}

/// Progress and cancellation for one long engine call. Create one per
/// call, pass it in, and poll [`EngineJob::progress`] from the main actor
/// while the call runs on a background task. Polling never blocks on the
/// engine, even while the call holds it.
#[derive(Debug, Default, uniffi::Object)]
pub struct EngineJob {
    job: Job,
}

#[uniffi::export]
impl EngineJob {
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks the call to stop soon; it then fails with
    /// [`AppError::Cancelled`] and changes nothing.
    pub fn cancel(&self) {
        self.job.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.job.is_cancelled()
    }

    pub fn progress(&self) -> ProgressRecord {
        self.job.progress().into()
    }
}

#[derive(Debug)]
struct State {
    engine: Engine,
    /// The current workspace: files from the last folder analysis. Plans,
    /// duplicate checks and collections work on these, so Swift never
    /// hands the engine arbitrary paths to move.
    workspace: Vec<FileInsight>,
    /// Folder `workspace` came from. Workspace calls must name the folder
    /// they expect, so a slow, superseded analysis can never make
    /// "Organize" act on a folder other than the one on screen.
    workspace_folder: Option<PathBuf>,
}

#[derive(Debug, uniffi::Object)]
pub struct AppEngine {
    state: Mutex<State>,
}

#[uniffi::export]
impl AppEngine {
    /// # Errors
    /// [`AppError::InvalidInput`] for a relative path,
    /// [`AppError::Storage`] when existing data can't be read.
    #[uniffi::constructor]
    pub fn open(data_directory: &str) -> Result<Self, AppError> {
        let engine = Engine::open(Path::new(data_directory))?;
        Ok(Self {
            state: Mutex::new(State {
                engine,
                workspace: Vec::new(),
                workspace_folder: None,
            }),
        })
    }

    pub fn startup_notices(&self) -> Vec<UserTextRecord> {
        self.lock()
            .engine
            .startup_notices()
            .iter()
            .map(Into::into)
            .collect()
    }

    /// Analyses a folder and makes it the current workspace. `job` reports
    /// progress and can cancel; a cancelled analysis keeps the previous
    /// workspace.
    ///
    /// # Errors
    /// [`AppError::Storage`] when the folder can't be read,
    /// [`AppError::Cancelled`] when `job` was cancelled.
    pub fn analyze_folder(
        &self,
        folder: &str,
        recursive: bool,
        job: &EngineJob,
    ) -> Result<AnalysisRecord, AppError> {
        let mut state = self.lock();
        let (insights, failures) =
            state
                .engine
                .analyze_folder(Path::new(folder), recursive, &job.job)?;
        let record = AnalysisRecord {
            files: insights.iter().map(Into::into).collect(),
            failures: failures.iter().map(Into::into).collect(),
        };
        state.workspace = insights;
        state.workspace_folder = Some(PathBuf::from(folder));
        Ok(record)
    }

    /// Plans organizing files of `folder` (which must be the folder last
    /// analysed) into their categories' destinations. `only_paths` limits
    /// the plan to a selection; empty means the whole folder.
    ///
    /// # Errors
    /// [`AppError::InvalidInput`] when `folder` is no longer the
    /// analysed workspace, or no valid destination is set.
    pub fn plan_workspace(
        &self,
        folder: &str,
        destinations: DestinationsRecord,
        only_paths: Vec<String>,
    ) -> Result<PlanRecord, AppError> {
        let wanted: Vec<PathBuf> = only_paths.into_iter().map(PathBuf::from).collect();
        let mut state = self.lock();
        let selected: Vec<FileInsight> = state
            .current(folder)?
            .iter()
            .filter(|i| wanted.is_empty() || wanted.contains(&i.path))
            .cloned()
            .collect();
        plan_record(&mut state.engine, &selected, destinations)
    }

    /// Analyses files dropped on the drop zone and plans them into their
    /// categories' destinations.
    ///
    /// # Errors
    /// [`AppError::InvalidInput`] when no valid destination is set.
    pub fn plan_dropped(
        &self,
        paths: Vec<String>,
        destinations: DestinationsRecord,
    ) -> Result<PlanRecord, AppError> {
        let mut state = self.lock();
        let paths: Vec<PathBuf> = paths
            .into_iter()
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .collect();
        let (insights, _) = state.engine.analyze(&paths);
        plan_record(&mut state.engine, &insights, destinations)
    }

    /// Runs the plan with id `plan_id`, except the moves at `skip` indexes.
    ///
    /// # Errors
    /// [`AppError::InvalidInput`] when the plan is no longer current.
    pub fn apply_plan(&self, plan_id: u64, skip: Vec<u32>) -> Result<ApplyRecord, AppError> {
        let skip: Vec<usize> = skip
            .into_iter()
            .filter_map(|i| usize::try_from(i).ok())
            .collect();
        let report = self.lock().engine.apply_plan(plan_id, &skip)?;
        Ok((&report).into())
    }

    /// # Errors
    /// [`AppError::NothingToUndo`] when there is no history.
    pub fn undo_last(&self) -> Result<ApplyRecord, AppError> {
        let report = self.lock().engine.undo_last()?;
        Ok((&report).into())
    }

    pub fn can_undo(&self) -> bool {
        self.lock().engine.can_undo()
    }

    /// # Errors
    /// [`AppError::InvalidInput`] when `folder` is no longer the
    /// analysed workspace.
    pub fn find_duplicates(&self, folder: &str) -> Result<Vec<DuplicateGroupRecord>, AppError> {
        let state = self.lock();
        let groups = state.engine.find_duplicates(state.current(folder)?);
        Ok(groups.iter().map(Into::into).collect())
    }

    /// Writes compressed/converted copies into `output_directory`.
    pub fn compress_images(
        &self,
        paths: Vec<String>,
        output_directory: &str,
        settings: CompressSettings,
    ) -> Vec<CompressResult> {
        let sources: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        self.lock()
            .engine
            .compress(&sources, Path::new(output_directory), settings.into())
            .iter()
            .map(Into::into)
            .collect()
    }

    pub fn rules(&self) -> Vec<RuleRecord> {
        self.lock()
            .engine
            .settings()
            .rules
            .iter()
            .map(|r| RuleRecord {
                id: r.id.clone(),
                name: r.name.clone(),
                enabled: r.enabled,
            })
            .collect()
    }

    /// # Errors
    /// [`AppError::InvalidInput`] for an unknown rule,
    /// [`AppError::Storage`] when saving fails.
    pub fn set_rule_enabled(&self, rule_id: &str, enabled: bool) -> Result<(), AppError> {
        let mut state = self.lock();
        let mut settings = state.engine.settings().clone();
        let rule = settings
            .rules
            .iter_mut()
            .find(|r| r.id == rule_id)
            .ok_or(AppError::from(TextKey::RuleMissing))?;
        rule.enabled = enabled;
        state.engine.update_settings(settings)?;
        Ok(())
    }

    pub fn collections(&self) -> Vec<CollectionRecord> {
        self.lock()
            .engine
            .settings()
            .collections
            .iter()
            .map(|c| CollectionRecord {
                id: c.id.clone(),
                name: c.name.clone(),
                icon: c.icon.clone(),
            })
            .collect()
    }

    /// # Errors
    /// [`AppError::InvalidInput`] when `folder` is no longer the
    /// analysed workspace.
    pub fn collection_members(
        &self,
        folder: &str,
        collection_id: &str,
    ) -> Result<Vec<FileRecord>, AppError> {
        let state = self.lock();
        let members = state
            .engine
            .collection_members(collection_id, state.current(folder)?);
        Ok(members.into_iter().map(Into::into).collect())
    }
}

impl State {
    fn current(&self, folder: &str) -> Result<&[FileInsight], AppError> {
        if self.workspace_folder.as_deref() == Some(Path::new(folder)) {
            Ok(&self.workspace)
        } else {
            Err(TextKey::WorkspaceChanged.into())
        }
    }
}

impl AppEngine {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn plan_record(
    engine: &mut Engine,
    insights: &[FileInsight],
    destinations: DestinationsRecord,
) -> Result<PlanRecord, AppError> {
    let pending = engine.plan(insights, &destinations.into())?;
    Ok(PlanRecord {
        id: pending.id,
        moves: pending.plan.moves.iter().map(Into::into).collect(),
        unchanged: pending.plan.unchanged as u64,
        missing_categories: pending.plan.missing.iter().map(|c| (*c).into()).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn engine(dir: &Path) -> AppEngine {
        AppEngine::open(&dir.join("data").to_string_lossy()).unwrap()
    }

    fn single(root: &Path) -> DestinationsRecord {
        DestinationsRecord {
            default_folder: Some(root.to_string_lossy().into_owned()),
            categories: vec![],
        }
    }

    #[test]
    fn dropped_files_go_to_their_category_folder_or_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let shot = dir.path().join("Screenshot 2026-01-01.png");
        image::RgbImage::new(8, 8).save(&shot).unwrap();
        let song = dir.path().join("song.mp3");
        fs::write(&song, b"ID3").unwrap();
        let shots = dir.path().join("Shots");
        let fyl = engine(dir.path());

        let plan = fyl
            .plan_dropped(
                vec![
                    shot.to_string_lossy().into_owned(),
                    song.to_string_lossy().into_owned(),
                ],
                DestinationsRecord {
                    default_folder: None,
                    categories: vec![CategoryFolder {
                        category: FileCategory::Screenshot,
                        folder: shots.to_string_lossy().into_owned(),
                    }],
                },
            )
            .unwrap();

        assert_eq!(plan.moves.len(), 1);
        assert!(Path::new(&plan.moves[0].to).starts_with(&shots));
        assert_eq!(plan.missing_categories, vec![FileCategory::Audio]);
        assert!(
            fyl.plan_dropped(
                vec![song.to_string_lossy().into_owned()],
                DestinationsRecord {
                    default_folder: None,
                    categories: vec![],
                },
            )
            .is_err()
        );
    }

    #[test]
    fn a_cancelled_analysis_keeps_the_previous_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        for folder in [&a, &b] {
            fs::create_dir(folder).unwrap();
            fs::write(folder.join("note.txt"), "hello").unwrap();
        }
        let fyl = engine(dir.path());
        let a_path = a.to_string_lossy().into_owned();
        let finished = EngineJob::new();
        fyl.analyze_folder(&a_path, false, &finished).unwrap();
        assert_eq!(finished.progress(), ProgressRecord { done: 1, total: 1 });

        let cancelled = EngineJob::new();
        cancelled.cancel();
        let result = fyl.analyze_folder(&b.to_string_lossy(), false, &cancelled);

        assert!(matches!(result, Err(AppError::Cancelled { .. })));
        assert!(
            fyl.find_duplicates(&a_path).is_ok(),
            "a is still the workspace"
        );
    }

    #[test]
    fn categories_are_listed_in_order() {
        let categories = file_categories();
        assert_eq!(categories.len(), Category::ALL.len());
        assert_eq!(categories[0].title, "Invoices");
    }

    #[test]
    fn workspace_flow_through_the_ffi_surface() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("Downloads");
        fs::create_dir(&inbox).unwrap();
        fs::write(
            inbox.join("receipt.txt"),
            "Thank you for your purchase. Order total $5. Paid with Visa",
        )
        .unwrap();
        fs::write(
            inbox.join("copy.txt"),
            "Thank you for your purchase. Order total $5. Paid with Visa",
        )
        .unwrap();
        let fyl = engine(dir.path());

        let analysis = fyl
            .analyze_folder(&inbox.to_string_lossy(), false, &EngineJob::new())
            .unwrap();
        assert_eq!(analysis.files.len(), 2);
        assert!(
            analysis
                .files
                .iter()
                .all(|f| f.category == FileCategory::Receipt)
        );
        let folder = inbox.to_string_lossy();
        assert_eq!(fyl.find_duplicates(&folder).unwrap().len(), 1);
        assert_eq!(
            fyl.collection_members(&folder, "paperwork").unwrap().len(),
            2
        );

        let root = dir.path().join("Organized");
        let plan = fyl.plan_workspace(&folder, single(&root), vec![]).unwrap();
        assert_eq!(plan.moves.len(), 2);
        let applied = fyl.apply_plan(plan.id, vec![]).unwrap();
        assert_eq!(applied.moved, 2);

        fyl.undo_last().unwrap();
        assert!(inbox.join("receipt.txt").exists());
        assert!(matches!(
            fyl.undo_last(),
            Err(AppError::NothingToUndo { .. })
        ));
    }

    #[test]
    fn selection_limits_the_plan() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "a").unwrap();
        fs::write(dir.path().join("b.txt"), "b").unwrap();
        let fyl = engine(dir.path());
        fyl.analyze_folder(&dir.path().to_string_lossy(), false, &EngineJob::new())
            .unwrap();

        let only_a = dir.path().join("a.txt").to_string_lossy().into_owned();
        let folder = dir.path().to_string_lossy();
        let plan = fyl
            .plan_workspace(&folder, single(&dir.path().join("out")), vec![only_a])
            .unwrap();

        assert_eq!(plan.moves.len(), 1);
    }

    #[test]
    fn rules_can_be_toggled_and_persist() {
        let dir = tempfile::tempdir().unwrap();
        let fyl = engine(dir.path());
        fyl.set_rule_enabled("photos", false).unwrap();
        assert!(fyl.set_rule_enabled("nope", true).is_err());

        let reopened = engine(dir.path());
        assert!(
            !reopened
                .rules()
                .iter()
                .find(|r| r.id == "photos")
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn dropped_files_are_planned_without_changing_the_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let dropped = dir.path().join("Screenshot 2026-01-01.png");
        image::RgbImage::new(8, 8).save(&dropped).unwrap();
        let fyl = engine(dir.path());

        let plan = fyl
            .plan_dropped(
                vec![dropped.to_string_lossy().into_owned()],
                single(&dir.path().join("out")),
            )
            .unwrap();

        assert_eq!(plan.moves.len(), 1);
        assert!(plan.moves[0].to.contains("Screenshots"));
        assert!(
            fyl.collection_members(&dir.path().to_string_lossy(), "screenshots")
                .is_err()
        );
    }

    #[test]
    fn workspace_calls_refuse_a_folder_that_is_not_the_analysed_one() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        fs::write(a.join("x.txt"), "x").unwrap();
        let fyl = engine(dir.path());
        fyl.analyze_folder(&a.to_string_lossy(), false, &EngineJob::new())
            .unwrap();

        // The UI now shows folder b, but the engine's workspace is still a.
        let result = fyl.plan_workspace(
            &b.to_string_lossy(),
            single(&dir.path().join("out")),
            vec![],
        );

        assert!(matches!(result, Err(AppError::InvalidInput { .. })));
        assert!(fyl.find_duplicates(&b.to_string_lossy()).is_err());
    }

    #[test]
    fn relative_data_directory_is_rejected() {
        assert!(matches!(
            AppEngine::open("data"),
            Err(AppError::InvalidInput { .. })
        ));
    }
}
