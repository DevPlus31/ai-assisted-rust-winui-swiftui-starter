//! Executes a plan and records an undoable batch.
//!
//! Safety rules:
//! - **never overwrite.** Moves use `hard_link` + `remove_file` (the link
//!   fails atomically if the name is taken) and copies use `create_new`.
//!   `fs::rename` is used only for case-only renames of the same file;
//! - a file that changed after the preview is left alone;
//! - cross-volume moves (or file systems without hard links) copy, verify
//!   the content hash, then delete the source;
//! - if the source can't be removed (e.g. macOS sandbox granted access to a
//!   dropped file but not its folder) the copy is kept and recorded as
//!   `Copied`, so undo deletes only the copy;
//! - undo deletes a copy only while it is still byte-identical to its
//!   original; undo failures stay in the journal for another try.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::plan::{Plan, PlannedMove};
use crate::dedupe::same_content;
use crate::error::FileFailure;
use crate::naming::unique_path;
use crate::user_text::{TextKey, UserText};

/// Name collisions tolerated per file before giving up (each retry picks a
/// new " (n)" name).
const MAX_NAME_ATTEMPTS: usize = 16;
/// Journal checkpoints while a long batch runs (crash safety for undo).
pub const CHECKPOINT_EVERY: usize = 25;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    Moved,
    /// Original left in place; `to` is a copy.
    Copied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedMove {
    pub from: PathBuf,
    pub to: PathBuf,
    pub mode: Mode,
}

/// One undoable unit of work.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Batch {
    pub applied: Vec<AppliedMove>,
    /// Directories this batch created, removed again on undo if empty.
    pub created_dirs: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApplyReport {
    pub batch: Batch,
    pub failures: Vec<FileFailure>,
    /// Non-fatal problem worth telling the user about.
    pub warning: Option<UserText>,
}

/// Applies `plan`, calling `checkpoint` with the work done so far every
/// [`CHECKPOINT_EVERY`] moves so the caller can persist undo information
/// before a crash could lose it.
pub fn apply(plan: &Plan, checkpoint: &mut dyn FnMut(&Batch)) -> ApplyReport {
    let mut report = ApplyReport::default();
    for planned in &plan.moves {
        if let Some(failure) = changed_since_preview(planned) {
            report.failures.push(failure);
            continue;
        }
        match move_one(&planned.from, &planned.to, &mut report.batch.created_dirs) {
            Ok(applied) => {
                report.batch.applied.push(applied);
                if report.batch.applied.len() % CHECKPOINT_EVERY == 0 {
                    checkpoint(&report.batch);
                }
            }
            Err(error) => report
                .failures
                .push(FileFailure::new(&planned.from, &error)),
        }
    }
    report
}

fn changed_since_preview(planned: &PlannedMove) -> Option<FileFailure> {
    let metadata = fs::metadata(&planned.from).ok()?; // missing: reported by the move
    let changed = metadata.len() != planned.expected_size
        || (planned.expected_modified.is_some()
            && metadata.modified().ok() != planned.expected_modified);
    changed.then(|| FileFailure::because(&planned.from, TextKey::FileChangedSincePreview))
}

/// Reverses a batch (last change first). Entries that could not be
/// restored are returned in `remaining` so they stay undoable.
pub fn undo(batch: &Batch) -> (ApplyReport, Batch) {
    let mut report = ApplyReport::default();
    let mut remaining = Batch {
        applied: Vec::new(),
        created_dirs: batch.created_dirs.clone(),
    };
    let mut renamed = 0;
    for applied in batch.applied.iter().rev() {
        let result = match applied.mode {
            Mode::Moved => move_one(&applied.to, &applied.from, &mut report.batch.created_dirs),
            Mode::Copied => remove_copy(applied),
        };
        match result {
            Ok(restored) => {
                if applied.mode == Mode::Moved && restored.to != applied.from {
                    renamed += 1;
                }
                report.batch.applied.push(restored);
            }
            Err(error) => {
                report.failures.push(FileFailure::new(&applied.to, &error));
                remaining.applied.insert(0, applied.clone());
            }
        }
    }
    for dir in batch.created_dirs.iter().rev() {
        // Only succeeds when empty — user files are never removed here.
        let _ = fs::remove_dir(dir);
    }
    if renamed > 0 {
        report.warning =
            Some(UserText::new(TextKey::RestoredWithNewName).arg("count", renamed.to_string()));
    }
    if remaining.applied.is_empty() {
        remaining.created_dirs.clear();
    }
    (report, remaining)
}

fn move_one(from: &Path, to: &Path, created_dirs: &mut Vec<PathBuf>) -> io::Result<AppliedMove> {
    if !from.is_file() {
        return Err(io::Error::from(io::ErrorKind::NotFound));
    }
    if let Some(parent) = to.parent() {
        create_dirs(parent, created_dirs)?;
    }
    let case_only = to.to_string_lossy().to_lowercase() == from.to_string_lossy().to_lowercase();
    if case_only && !exact_name_exists(to) {
        // Same file, different letter case: a plain rename is the only way
        // and cannot clobber another file.
        fs::rename(from, to)?;
        return Ok(AppliedMove {
            from: from.into(),
            to: to.into(),
            mode: Mode::Moved,
        });
    }
    for _ in 0..MAX_NAME_ATTEMPTS {
        let target = unique_path(to, Path::exists);
        match fs::hard_link(from, &target) {
            Ok(()) => {
                if fs::remove_file(from).is_ok() {
                    return Ok(AppliedMove {
                        from: from.into(),
                        to: target,
                        mode: Mode::Moved,
                    });
                }
                // Source folder not writable: drop the link and make a real
                // copy instead, so the two files are independent.
                fs::remove_file(&target)?;
                return copy_then_remove(from, &target);
            }
            // Someone took the name a moment ago: pick another.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            // Other volume, no hard-link support (FAT/exFAT, some shares), …
            Err(_) => return copy_then_remove(from, &target),
        }
    }
    Err(io::Error::from(io::ErrorKind::AlreadyExists))
}

fn copy_then_remove(from: &Path, to: &Path) -> io::Result<AppliedMove> {
    let source_meta = fs::metadata(from)?;
    copy_new(from, to)?;
    if !same_content(from, to)? {
        let _ = fs::remove_file(to);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "copy verification failed",
        ));
    }
    if let Ok(modified) = source_meta.modified() {
        let _ = fs::File::options()
            .write(true)
            .open(to)
            .and_then(|f| f.set_modified(modified));
    }
    let mode = match fs::remove_file(from) {
        Ok(()) => Mode::Moved,
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => Mode::Copied,
        Err(e) => {
            let _ = fs::remove_file(to);
            return Err(e);
        }
    };
    Ok(AppliedMove {
        from: from.into(),
        to: to.into(),
        mode,
    })
}

/// Copies without ever replacing an existing file.
fn copy_new(from: &Path, to: &Path) -> io::Result<()> {
    let mut source = fs::File::open(from)?;
    let mut target = fs::File::create_new(to)?;
    let result = io::copy(&mut source, &mut target).and_then(|_| target.sync_all());
    if result.is_err() {
        let _ = fs::remove_file(to);
    }
    result
}

fn remove_copy(applied: &AppliedMove) -> io::Result<AppliedMove> {
    if !applied.from.exists() {
        // The copy is now the only version — keep it.
        return Err(io::Error::from(io::ErrorKind::NotFound));
    }
    if !same_content(&applied.from, &applied.to)? {
        // The copy was edited since; deleting it would lose work.
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "copy was modified",
        ));
    }
    fs::remove_file(&applied.to)?;
    Ok(AppliedMove {
        from: applied.to.clone(),
        to: applied.from.clone(),
        mode: Mode::Copied,
    })
}

fn exact_name_exists(path: &Path) -> bool {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return false;
    };
    fs::read_dir(parent).is_ok_and(|entries| entries.flatten().any(|e| e.file_name() == name))
}

fn create_dirs(dir: &Path, created: &mut Vec<PathBuf>) -> io::Result<()> {
    let missing: Vec<PathBuf> = dir
        .ancestors()
        .take_while(|d| !d.exists())
        .map(Path::to_path_buf)
        .collect();
    fs::create_dir_all(dir)?;
    created.extend(missing.into_iter().rev());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_of(moves: &[(&Path, &Path)]) -> Plan {
        Plan {
            moves: moves
                .iter()
                .map(|(from, to)| {
                    let meta = fs::metadata(from).ok();
                    PlannedMove {
                        from: from.into(),
                        to: to.into(),
                        rule_name: "t".into(),
                        expected_size: meta.as_ref().map_or(0, std::fs::Metadata::len),
                        expected_modified: meta.and_then(|m| m.modified().ok()),
                    }
                })
                .collect(),
            ..Plan::default()
        }
    }

    fn apply_all(plan: &Plan) -> ApplyReport {
        apply(plan, &mut |_| {})
    }

    #[test]
    fn applies_and_undoes_moves_including_created_folders() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("in/a.pdf");
        fs::create_dir(dir.path().join("in")).unwrap();
        fs::write(&from, b"A").unwrap();
        let to = dir.path().join("org/Invoices/2026 a.pdf");

        let report = apply_all(&plan_of(&[(&from, &to)]));

        assert!(report.failures.is_empty());
        assert_eq!(fs::read(&to).unwrap(), b"A");
        assert!(!from.exists());

        let (undone, _) = undo(&report.batch);

        assert!(undone.failures.is_empty());
        assert_eq!(fs::read(&from).unwrap(), b"A");
        assert!(
            !dir.path().join("org").exists(),
            "created folders are cleaned up"
        );
    }

    #[test]
    fn never_overwrites_a_file_that_appeared_after_planning() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a.txt");
        let to = dir.path().join("b.txt");
        fs::write(&from, b"new").unwrap();
        fs::write(&to, b"existing").unwrap();

        let report = apply_all(&plan_of(&[(&from, &to)]));

        assert_eq!(fs::read(&to).unwrap(), b"existing");
        assert_eq!(report.batch.applied[0].to, dir.path().join("b (2).txt"));
    }

    #[test]
    fn missing_sources_are_reported_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("ok.txt");
        fs::write(&ok, b"1").unwrap();
        let gone = dir.path().join("gone.txt");

        let report = apply_all(&plan_of(&[
            (&gone, &dir.path().join("x.txt")),
            (&ok, &dir.path().join("y.txt")),
        ]));

        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.batch.applied.len(), 1);
    }

    #[test]
    fn undo_restores_next_to_a_new_file_with_the_old_name() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a.txt");
        let to = dir.path().join("moved.txt");
        fs::write(&from, b"original").unwrap();
        let report = apply_all(&plan_of(&[(&from, &to)]));
        fs::write(&from, b"someone else's new file").unwrap();

        let (undone, _) = undo(&report.batch);

        assert_eq!(fs::read(&from).unwrap(), b"someone else's new file");
        assert_eq!(fs::read(dir.path().join("a (2).txt")).unwrap(), b"original");
        assert!(undone.failures.is_empty());
    }

    #[test]
    fn undo_of_a_copy_keeps_it_when_the_original_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join("copy.txt");
        fs::write(&copy, b"only version").unwrap();
        let batch = Batch {
            applied: vec![AppliedMove {
                from: dir.path().join("gone.txt"),
                to: copy.clone(),
                mode: Mode::Copied,
            }],
            created_dirs: vec![],
        };

        let (undone, remaining) = undo(&batch);

        assert_eq!(undone.failures.len(), 1);
        assert!(copy.exists());
        assert_eq!(
            remaining.applied, batch.applied,
            "failed undo stays undoable"
        );
    }

    #[test]
    fn undo_keeps_a_copy_that_was_edited() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("o.txt");
        let copy = dir.path().join("c.txt");
        fs::write(&original, b"v1").unwrap();
        fs::write(&copy, b"v1 plus my edits").unwrap();
        let batch = Batch {
            applied: vec![AppliedMove {
                from: original,
                to: copy.clone(),
                mode: Mode::Copied,
            }],
            created_dirs: vec![],
        };

        let (undone, remaining) = undo(&batch);

        assert_eq!(undone.failures.len(), 1);
        assert!(copy.exists());
        assert_eq!(remaining.applied.len(), 1);
    }

    #[test]
    fn files_changed_after_the_preview_are_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a.txt");
        fs::write(&from, b"before").unwrap();
        let plan = plan_of(&[(&from, &dir.path().join("b.txt"))]);
        fs::write(&from, b"after, and longer").unwrap();

        let report = apply_all(&plan);

        assert!(report.batch.applied.is_empty());
        assert_eq!(
            report.failures[0].message.key,
            TextKey::FileChangedSincePreview
        );
        assert!(from.exists());
    }

    #[test]
    fn checkpoints_report_progress_during_long_batches() {
        let dir = tempfile::tempdir().unwrap();
        let names: Vec<_> = (0..=CHECKPOINT_EVERY * 2)
            .map(|i| dir.path().join(format!("{i}.txt")))
            .collect();
        for name in &names {
            fs::write(name, b"x").unwrap();
        }
        let targets: Vec<_> = (0..names.len())
            .map(|i| dir.path().join(format!("out/{i}.txt")))
            .collect();
        let pairs: Vec<_> = names
            .iter()
            .zip(&targets)
            .map(|(a, b)| (a.as_path(), b.as_path()))
            .collect();
        let mut checkpoints = Vec::new();

        apply(&plan_of(&pairs), &mut |batch| {
            checkpoints.push(batch.applied.len());
        });

        assert_eq!(checkpoints, vec![CHECKPOINT_EVERY, CHECKPOINT_EVERY * 2]);
    }

    #[test]
    fn undo_reports_files_restored_under_a_new_name() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a.txt");
        fs::write(&from, b"original").unwrap();
        let report = apply_all(&plan_of(&[(&from, &dir.path().join("moved.txt"))]));
        fs::write(&from, b"new file").unwrap();

        let (undone, remaining) = undo(&report.batch);

        assert_eq!(undone.warning.unwrap().key, TextKey::RestoredWithNewName);
        assert!(remaining.applied.is_empty());
    }

    #[test]
    fn case_only_renames_work() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("photo.JPG");
        fs::write(&from, b"x").unwrap();
        let to = dir.path().join("photo.jpg");

        let report = apply_all(&plan_of(&[(&from, &to)]));

        assert!(report.failures.is_empty());
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("photo.jpg")]);
    }
}
