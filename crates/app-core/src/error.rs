use std::fmt;

use crate::user_text::{TextKey, UserText};

/// Errors surfaced to both UI shells. [`CoreError::user_text`] is what to
/// show (translatable); `Display` gives its English text. Diagnostic
/// detail is kept separately via [`CoreError::detail`].
#[derive(Debug, PartialEq, Eq)]
pub enum CoreError {
    InvalidInput(UserText),
    NothingToUndo,
    /// Detail for logs.
    Storage(String),
    /// Detail for logs.
    Corrupt(String),
    /// Detail for logs.
    FolderUnreadable(String),
    /// The user stopped the work through [`Job::cancel`](crate::Job::cancel).
    Cancelled,
}

impl CoreError {
    pub(crate) fn invalid(key: TextKey) -> Self {
        Self::InvalidInput(key.into())
    }

    /// What to tell the user.
    pub fn user_text(&self) -> UserText {
        match self {
            Self::InvalidInput(text) => text.clone(),
            Self::NothingToUndo => TextKey::NothingToUndo.into(),
            Self::Storage(_) => TextKey::SaveFailed.into(),
            Self::Corrupt(_) => TextKey::SettingsSetAside.into(),
            Self::FolderUnreadable(_) => TextKey::FolderUnreadable.into(),
            Self::Cancelled => TextKey::Cancelled.into(),
        }
    }

    /// Diagnostic detail intended for logs, not for display.
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Storage(detail) | Self::Corrupt(detail) | Self::FolderUnreadable(detail) => {
                Some(detail)
            }
            Self::InvalidInput(_) | Self::NothingToUndo | Self::Cancelled => None,
        }
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.user_text().fmt(f)
    }
}

impl std::error::Error for CoreError {}

/// A per-file problem inside a batch operation. Batches never abort on one
/// bad file; failures are collected and shown to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFailure {
    pub path: std::path::PathBuf,
    pub message: UserText,
}

impl FileFailure {
    pub fn new(path: impl Into<std::path::PathBuf>, error: &std::io::Error) -> Self {
        Self::because(path, describe_io(error))
    }

    pub fn because(path: impl Into<std::path::PathBuf>, key: TextKey) -> Self {
        Self {
            path: path.into(),
            message: key.into(),
        }
    }
}

/// Short, user-facing description of an I/O error (no OS error codes).
pub fn describe_io(error: &std::io::Error) -> TextKey {
    use std::io::ErrorKind;
    match error.kind() {
        ErrorKind::NotFound => TextKey::FileMissing,
        ErrorKind::PermissionDenied => TextKey::NoPermission,
        ErrorKind::AlreadyExists => TextKey::FileExists,
        ErrorKind::StorageFull => TextKey::DiskFull,
        ErrorKind::InvalidData => TextKey::ContentsUnreadable,
        _ => TextKey::FileNotProcessed,
    }
}

/// Runs `work`, turning a panic into `None`. Used around third-party
/// parsers fed untrusted files: one hostile file must not take the app
/// down (windows-reactor treats a panic in background work as fatal).
pub fn contained<T>(work: impl FnOnce() -> T) -> Option<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).ok()
}
