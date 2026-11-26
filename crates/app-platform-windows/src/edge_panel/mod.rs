//! The Windows edge panel: a pill on a screen edge that springs open into a
//! drop panel. `model` is platform-neutral; the Win32 glue only compiles on
//! Windows.

pub mod model;
#[cfg(windows)]
pub mod win32;
