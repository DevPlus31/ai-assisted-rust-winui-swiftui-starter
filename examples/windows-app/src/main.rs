//! Minimal WinUI 3 shell over the Rust engine.
//!
//! It shows the core ↔ shell wiring, not a finished product: open the engine
//! in the app-data folder, run engine work off the UI thread, render the
//! result. Replace the UI with your own.

// Release builds are GUI apps without a console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

// Only the Windows shell calls these; their tests run everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
mod organize;
#[cfg_attr(not(windows), allow(dead_code))]
mod summary;
#[cfg_attr(not(windows), allow(dead_code))]
mod text;

#[cfg(windows)]
mod shell;

#[cfg(windows)]
fn main() {
    if let Err(error) = windows_reactor::App::run_component::<shell::Shell>(()) {
        // Usually the Windows App SDK runtime is missing (unpackaged runs).
        eprintln!("app-starter: couldn't start WinUI: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("app-starter is the Windows sample shell; on macOS use examples/macos-app.");
    std::process::exit(1);
}
