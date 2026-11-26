//! Native Windows layer for app shells.
//!
//! Everything a Windows UI needs from the platform except the UI itself:
//! - [`data_dir`]: per-user folders (MSIX-aware app data, inbox, destination);
//! - [`prefs`] and [`destinations`]: shell preferences saved next to the engine;
//! - [`workspace`]: the shared, thread-safe [`app_core::Engine`] handle;
//! - [`edge_panel`]: the edge panel's behaviour, plus Win32 window glue on Windows.
//!
//! Only `edge_panel::win32` is Windows-specific, so the rest is unit-tested
//! on any OS.

pub mod data_dir;
pub mod destinations;
pub mod edge_panel;
pub mod prefs;
pub mod workspace;

pub use destinations::DestinationFields;
pub use prefs::Preferences;
pub use workspace::Workspace;
