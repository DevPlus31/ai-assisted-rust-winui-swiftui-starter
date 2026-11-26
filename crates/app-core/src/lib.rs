//! Shared Rust core engine.
//!
//! Pure Rust with no UI or FFI code, shared by the macOS shell (via
//! `app-ffi`) and the Windows shell (linked directly).
//!
//! Privacy and store-policy guarantees enforced here:
//! - everything runs locally: no networking dependencies, no cloud AI,
//!   no code loaded at runtime;
//! - the engine never picks locations on its own; shells pass folders the
//!   user granted (macOS sandbox) and an app-data directory;
//! - file changes are always planned first, never overwrite, and can be undone.

pub mod analyze;
pub mod collections;
pub mod compress;
pub mod date;
pub mod dedupe;
pub mod edge;
mod engine;
mod error;
pub mod files;
mod imaging;
mod job;
pub mod naming;
mod store;
pub mod user_text;
pub mod workflow;

pub use engine::{Engine, JOURNAL_FILE, PendingPlan, SETTINGS_FILE, Settings};
pub use error::{CoreError, FileFailure};
pub use job::{Job, Progress};
pub use store::{JsonStore, Loaded};
pub use user_text::{TextKey, UserText};
