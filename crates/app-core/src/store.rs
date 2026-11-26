//! Crash-safe JSON persistence for the app's own data (settings, undo
//! journal) inside the directory the platform shell provides:
//! - macOS: the App Sandbox container (Application Support).
//! - Windows: the package's local app data folder (MSIX-redirected).

use std::fs;
use std::io::Write;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::CoreError;

/// Outcome of [`JsonStore::open`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded<T> {
    pub value: T,
    /// `true` when a corrupt file was renamed aside (never deleted) and
    /// defaults were used instead.
    pub recovered: bool,
}

#[derive(Debug, Clone)]
pub struct JsonStore<T> {
    file: PathBuf,
    _value: PhantomData<fn() -> T>,
}

impl<T: Serialize + DeserializeOwned + Default> JsonStore<T> {
    pub fn new(directory: &Path, file_name: &str) -> Self {
        Self {
            file: directory.join(file_name),
            _value: PhantomData,
        }
    }

    /// Loads the value; a missing file yields the default.
    ///
    /// # Errors
    /// [`CoreError::Storage`] when the file can't be read and
    /// [`CoreError::Corrupt`] when its contents are invalid.
    pub fn load(&self) -> Result<T, CoreError> {
        let bytes = match fs::read(&self.file) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
            Err(e) => return Err(CoreError::Storage(e.to_string())),
        };
        serde_json::from_slice(&bytes).map_err(|e| CoreError::Corrupt(e.to_string()))
    }

    /// Start-up load: a corrupt file is renamed to
    /// `<name>.corrupt-<unix time>.json` and defaults are returned, so one bad
    /// file never leaves the app unusable.
    ///
    /// # Errors
    /// [`CoreError::Storage`] when the file exists but can't be read or moved.
    pub fn open(&self) -> Result<Loaded<T>, CoreError> {
        match self.load() {
            Ok(value) => Ok(Loaded {
                value,
                recovered: false,
            }),
            Err(CoreError::Corrupt(_)) => {
                fs::rename(&self.file, self.quarantine_path())
                    .map_err(|e| CoreError::Storage(e.to_string()))?;
                Ok(Loaded {
                    value: T::default(),
                    recovered: true,
                })
            }
            Err(error) => Err(error),
        }
    }

    /// Atomic replace: unique temp file, fsync, rename, fsync directory.
    ///
    /// # Errors
    /// [`CoreError::Storage`] on any I/O failure.
    pub fn save(&self, value: &T) -> Result<(), CoreError> {
        let storage = |e: std::io::Error| CoreError::Storage(e.to_string());
        let directory = self
            .file
            .parent()
            .ok_or_else(|| CoreError::Storage("data file has no parent directory".into()))?;
        fs::create_dir_all(directory).map_err(storage)?;
        let json =
            serde_json::to_vec_pretty(value).map_err(|e| CoreError::Storage(e.to_string()))?;
        // Dropped (and deleted) automatically if any step below fails.
        let mut temp = tempfile::NamedTempFile::new_in(directory).map_err(storage)?;
        temp.write_all(&json).map_err(storage)?;
        temp.as_file().sync_all().map_err(storage)?;
        temp.persist(&self.file).map_err(|e| storage(e.error))?;
        sync_directory(directory).map_err(storage)
    }

    fn quarantine_path(&self) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        let stem = self
            .file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("data");
        self.file
            .with_file_name(format!("{stem}.corrupt-{stamp}.json"))
    }
}

/// Makes the rename itself durable (POSIX); NTFS journals renames.
#[cfg(unix)]
fn sync_directory(directory: &Path) -> std::io::Result<()> {
    fs::File::open(directory)?.sync_all()
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)] // keeps one signature across platforms
fn sync_directory(_directory: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    type Store = JsonStore<Vec<String>>;

    #[test]
    fn missing_file_loads_default() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Store::new(dir.path(), "x.json").load().unwrap().is_empty());
    }

    #[test]
    fn save_then_load_round_trips_without_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(&dir.path().join("nested"), "x.json");

        store.save(&vec!["a".into()]).unwrap();

        assert_eq!(store.load().unwrap(), vec!["a".to_owned()]);
        assert_eq!(fs::read_dir(dir.path().join("nested")).unwrap().count(), 1);
    }

    #[test]
    fn open_quarantines_corrupt_data_and_starts_fresh() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("x.json"), b"{not json").unwrap();
        let store = Store::new(dir.path(), "x.json");

        let loaded = store.open().unwrap();

        assert!(loaded.recovered);
        assert!(loaded.value.is_empty());
        let kept: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| fs::read(entry.unwrap().path()).unwrap())
            .collect();
        assert_eq!(
            kept,
            vec![b"{not json".to_vec()],
            "bad data is kept, not deleted"
        );
        store.save(&loaded.value).unwrap();
    }

    #[test]
    fn storage_errors_do_not_leak_into_the_display_message() {
        let err = CoreError::Storage("/Users/someone/secret/path: denied".into());
        assert!(!err.to_string().contains("/Users"));
        assert!(err.detail().unwrap().contains("denied"));
    }
}
