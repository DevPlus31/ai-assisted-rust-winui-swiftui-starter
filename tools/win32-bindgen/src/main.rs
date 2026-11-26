//! `cargo run --manifest-path tools/win32-bindgen/Cargo.toml`
//!
//! Generates only the Win32 APIs the edge panel needs, as raw `--sys`
//! bindings that depend on nothing but `windows-link`.

fn main() {
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/../../crates/app-platform-windows/src/edge_panel/win32/bindings.rs");
    windows_bindgen::bindgen([
        "--out",
        out,
        "--sys",
        "--flat",
        "--filter",
        // Finding and styling our own window
        "EnumThreadWindows",
        "GetCurrentThreadId",
        "GetWindowTextW",
        "IsWindow",
        "GetWindowLongPtrW",
        "SetWindowLongPtrW",
        "GWL_STYLE",
        "GWL_EXSTYLE",
        "WS_POPUP",
        "WS_VISIBLE",
        "WS_CAPTION",
        "WS_THICKFRAME",
        "WS_SYSMENU",
        "WS_MINIMIZEBOX",
        "WS_MAXIMIZEBOX",
        "WS_EX_TOOLWINDOW",
        "WS_EX_NOACTIVATE",
        "WS_EX_TOPMOST",
        // Placing, showing and shaping it
        "SetWindowPos",
        "HWND_TOPMOST",
        "SWP_NOACTIVATE",
        "SWP_FRAMECHANGED",
        "SWP_SHOWWINDOW",
        "ShowWindow",
        "SW_HIDE",
        "SW_SHOWNOACTIVATE",
        "SW_RESTORE",
        "SW_SHOW",
        "IsIconic",
        "SetForegroundWindow",
        "SetWindowRgn",
        "CreateRoundRectRgn",
        // Display geometry
        "MonitorFromPoint",
        "GetMonitorInfoW",
        "MONITORINFO",
        "MONITOR_DEFAULTTOPRIMARY",
        "GetDpiForWindow",
        "GetDpiForMonitor",
        "MDT_EFFECTIVE_DPI",
        // The user's UI languages (picks the shell's translation file)
        "GetUserPreferredUILanguages",
        "MUI_LANGUAGE_NAME",
    ]);
}
