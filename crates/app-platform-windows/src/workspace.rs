//! The engine handle a Windows shell shares between its UI thread and
//! background jobs.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use app_core::{Engine, UserText};

/// Shared, thread-safe engine handle (cheap to clone into jobs).
#[derive(Debug, Clone)]
pub struct Workspace {
    engine: Arc<Mutex<Engine>>,
}

impl Workspace {
    /// Opens the engine in `data_dir` and returns it with the notices the
    /// engine raised on startup (e.g. a settings file it had to set aside).
    ///
    /// # Errors
    /// A user-facing message when the data folder is missing or unreadable.
    pub fn open(data_dir: Option<PathBuf>) -> Result<(Self, Vec<UserText>), String> {
        let dir = data_dir.ok_or("The app couldn't find a folder for its settings.")?;
        let engine = Engine::open(&dir).map_err(|e| e.to_string())?;
        let notices = engine.startup_notices().to_vec();
        Ok((
            Self {
                engine: Arc::new(Mutex::new(engine)),
            },
            notices,
        ))
    }

    /// Runs `f` with exclusive access to the engine. A job that panicked
    /// while holding the lock doesn't poison it for everyone else.
    pub fn with<T>(&self, f: impl FnOnce(&mut Engine) -> T) -> T {
        f(&mut self.engine.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_a_data_folder() {
        assert!(Workspace::open(None).is_err());
    }

    #[test]
    fn clones_share_one_engine() {
        let dir = tempfile::tempdir().unwrap();
        let (workspace, notices) = Workspace::open(Some(dir.path().join("data"))).unwrap();
        assert!(notices.is_empty());

        let clone = workspace.clone();
        let from_clone = clone.with(|engine| std::ptr::from_mut(engine) as usize);
        let from_original = workspace.with(|engine| std::ptr::from_mut(engine) as usize);
        assert_eq!(from_clone, from_original);
    }
}
