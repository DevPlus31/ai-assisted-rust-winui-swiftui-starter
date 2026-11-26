//! Plan → preview → apply → undo, as the shell shows it. Platform-neutral,
//! so it is unit-tested on every OS.
//!
//! The engine only ever executes a plan it produced itself, by id; the
//! shell can untick moves but never hand it paths to move.

use std::path::{Path, PathBuf};

use app_core::analyze::FileInsight;
use app_core::workflow::{ApplyReport, Destinations, Mode};
use app_core::{TextKey, UserText};
use app_platform_windows::Workspace;

/// One proposed move, ready to display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveRow {
    /// File name today.
    pub from_name: String,
    /// File name after the move (may be a better name the engine suggested).
    pub to_name: String,
    /// Folder it goes to.
    pub to_folder: String,
}

/// A plan the user reviews before anything changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub plan_id: u64,
    pub moves: Vec<MoveRow>,
    /// Files no rule would change.
    pub unchanged: usize,
    /// Plural titles of categories left in place for lack of a destination.
    pub missing: Vec<&'static str>,
}

/// What an apply or undo did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub moved: usize,
    /// Files copied because the original's folder isn't writable.
    pub copied: usize,
    /// File name and what went wrong, per file that failed.
    pub failures: Vec<(String, UserText)>,
    pub warning: Option<UserText>,
}

/// Plans filing `insights` into `destination` (as the user typed it), one
/// subfolder per category.
///
/// # Errors
/// What to tell the user when `destination` is blank or isn't a full
/// folder path (the engine checks the path).
pub fn plan(
    workspace: &Workspace,
    insights: &[FileInsight],
    destination: &str,
) -> Result<Preview, UserText> {
    let destination = destination.trim().trim_matches('"');
    if destination.is_empty() {
        return Err(TextKey::ChooseDestination.into());
    }
    let destinations = Destinations::single(PathBuf::from(destination));
    let pending = workspace
        .with(|engine| engine.plan(insights, &destinations))
        .map_err(|e| e.user_text())?;
    Ok(Preview {
        plan_id: pending.id,
        moves: pending
            .plan
            .moves
            .iter()
            .map(|planned| MoveRow {
                from_name: file_name(&planned.from),
                to_name: file_name(&planned.to),
                to_folder: planned
                    .to
                    .parent()
                    .map(|folder| folder.display().to_string())
                    .unwrap_or_default(),
            })
            .collect(),
        unchanged: pending.plan.unchanged,
        missing: pending.plan.missing.iter().map(|c| c.folder()).collect(),
    })
}

/// Runs plan `plan_id` except the moves at `skip` (indexes into
/// [`Preview::moves`]).
///
/// # Errors
/// What to tell the user when the preview is out of date.
pub fn apply(workspace: &Workspace, plan_id: u64, skip: &[usize]) -> Result<Outcome, UserText> {
    workspace
        .with(|engine| engine.apply_plan(plan_id, skip))
        .map(|report| outcome(&report))
        .map_err(|e| e.user_text())
}

/// Puts the files of the last apply back.
///
/// # Errors
/// What to tell the user when there is nothing to undo.
pub fn undo(workspace: &Workspace) -> Result<Outcome, UserText> {
    workspace
        .with(app_core::Engine::undo_last)
        .map(|report| outcome(&report))
        .map_err(|e| e.user_text())
}

pub fn can_undo(workspace: &Workspace) -> bool {
    workspace.with(|engine| engine.can_undo())
}

fn outcome(report: &ApplyReport) -> Outcome {
    let count = |mode| {
        report
            .batch
            .applied
            .iter()
            .filter(|a| a.mode == mode)
            .count()
    };
    Outcome {
        moved: count(Mode::Moved),
        copied: count(Mode::Copied),
        failures: report
            .failures
            .iter()
            .map(|f| (file_name(&f.path), f.message.clone()))
            .collect(),
        warning: report.warning.clone(),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use app_core::Job;

    use super::*;
    use crate::summary;

    struct Fixture {
        dir: tempfile::TempDir,
        workspace: Workspace,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let (workspace, _) = Workspace::open(Some(dir.path().join("data"))).unwrap();
            fs::create_dir(dir.path().join("inbox")).unwrap();
            Self { dir, workspace }
        }

        fn inbox(&self) -> std::path::PathBuf {
            self.dir.path().join("inbox")
        }

        fn destination(&self) -> String {
            self.dir.path().join("Organized").display().to_string()
        }

        fn preview(&self) -> Preview {
            let folder = self.inbox().display().to_string();
            let analysis = summary::analyze(&self.workspace, &folder, &Job::new()).unwrap();
            plan(&self.workspace, &analysis.insights, &self.destination()).unwrap()
        }
    }

    #[test]
    fn previews_then_applies_then_undoes() {
        let fixture = Fixture::new();
        fs::write(fixture.inbox().join("song.mp3"), b"ID3").unwrap();

        let preview = fixture.preview();
        assert_eq!(preview.moves.len(), 1);
        assert_eq!(preview.moves[0].from_name, "song.mp3");
        assert!(
            preview.moves[0]
                .to_folder
                .starts_with(&fixture.destination())
        );
        assert!(
            fixture.inbox().join("song.mp3").exists(),
            "preview moves nothing"
        );

        let applied = apply(&fixture.workspace, preview.plan_id, &[]).unwrap();
        assert_eq!((applied.moved, applied.copied), (1, 0));
        assert!(!fixture.inbox().join("song.mp3").exists());
        assert!(can_undo(&fixture.workspace));

        let undone = undo(&fixture.workspace).unwrap();
        assert_eq!(undone.moved, 1);
        assert!(fixture.inbox().join("song.mp3").exists());
        assert!(!can_undo(&fixture.workspace));
    }

    #[test]
    fn unticked_moves_are_left_alone() {
        let fixture = Fixture::new();
        fs::write(fixture.inbox().join("a.mp3"), b"ID3").unwrap();
        fs::write(fixture.inbox().join("b.mp3"), b"ID3").unwrap();
        let preview = fixture.preview();
        assert_eq!(preview.moves.len(), 2);

        let applied = apply(&fixture.workspace, preview.plan_id, &[0]).unwrap();

        assert_eq!(applied.moved, 1);
        let skipped = &preview.moves[0].from_name;
        assert!(fixture.inbox().join(skipped).exists());
    }

    #[test]
    fn a_stale_preview_is_refused() {
        let fixture = Fixture::new();
        fs::write(fixture.inbox().join("song.mp3"), b"ID3").unwrap();
        let first = fixture.preview();
        let _second = fixture.preview();

        assert!(apply(&fixture.workspace, first.plan_id, &[]).is_err());
        assert!(fixture.inbox().join("song.mp3").exists());
    }

    #[test]
    fn rejects_a_relative_destination() {
        let fixture = Fixture::new();
        assert_eq!(
            plan(&fixture.workspace, &[], "Organized").unwrap_err().key,
            TextKey::FolderNotAbsolute
        );
        assert_eq!(
            plan(&fixture.workspace, &[], "  ").unwrap_err().key,
            TextKey::ChooseDestination
        );
    }

    #[test]
    fn undo_without_history_explains_why() {
        let fixture = Fixture::new();
        assert_eq!(
            undo(&fixture.workspace).unwrap_err().to_string(),
            "Nothing to undo."
        );
    }
}
