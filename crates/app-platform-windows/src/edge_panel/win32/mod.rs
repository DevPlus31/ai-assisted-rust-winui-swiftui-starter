//! Win32 glue that windows-reactor 0.100 doesn't expose: turning one of
//! its windows into a borderless, always-on-top tool window that never
//! takes focus, and placing/shaping it on a screen edge; plus the user's
//! UI languages ([`preferred_ui_languages`]).
//!
//! All `unsafe` in this crate lives here. Every call targets a window owned
//! by this thread, found by its unique title right after reactor created it.
#![allow(unsafe_code)]

#[allow(
    non_snake_case,
    non_camel_case_types,
    dead_code,
    clippy::all,
    clippy::pedantic
)]
mod bindings;

use app_core::edge::{DisplayMetrics, EdgePlacement, Rect};
#[allow(clippy::wildcard_imports)] // small generated module, by design
use bindings::*;

/// Win32 default DPI; scale = window DPI / 96.
const BASE_DPI: f64 = 96.0;

/// A top-level window owned by the UI thread. Not `Send`: only use it on
/// the thread that created it (reactor's UI thread).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window(HWND);

impl Window {
    /// Finds this thread's top-level window titled exactly `title`.
    pub fn find_own(title: &str) -> Option<Self> {
        struct Search {
            title: Vec<u16>,
            found: HWND,
        }
        unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
            // SAFETY: `lparam` is the `&mut Search` passed below and outlives
            // the synchronous EnumThreadWindows call.
            let search = unsafe { &mut *(lparam as *mut Search) };
            let mut buffer = [0u16; 128];
            // SAFETY: hwnd comes from the enumeration; buffer length is passed.
            let capacity = i32::try_from(buffer.len()).unwrap_or(i32::MAX);
            let length = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), capacity) };
            if usize::try_from(length).is_ok_and(|n| buffer[..n] == search.title[..]) {
                search.found = hwnd;
                return 0; // stop enumerating
            }
            1
        }
        let mut search = Search {
            title: title.encode_utf16().collect(),
            found: std::ptr::null_mut(),
        };
        // SAFETY: callback and pointer are valid for the duration of the call.
        unsafe {
            EnumThreadWindows(GetCurrentThreadId(), Some(visit), &raw mut search as LPARAM);
        }
        (!search.found.is_null()).then_some(Self(search.found))
    }

    pub fn is_alive(self) -> bool {
        // SAFETY: IsWindow accepts any handle value.
        unsafe { IsWindow(self.0) != 0 }
    }

    /// Borderless popup, no taskbar button / Alt+Tab entry, never takes
    /// focus, above other windows. Keeps the rest of the original style
    /// (e.g. `WS_CLIPCHILDREN`, which the XAML content relies on).
    // Window styles are bit masks; the app ships 64-bit only (x64, ARM64),
    // where every style value fits in `isize`.
    #[allow(clippy::cast_possible_wrap)]
    pub fn make_edge_panel(self) {
        let frame_bits =
            (WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX) as isize;
        let extended_bits = (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST) as isize;
        // SAFETY: self.0 is a live window on this thread (see find_own).
        unsafe {
            let style = GetWindowLongPtrW(self.0, GWL_STYLE);
            SetWindowLongPtrW(self.0, GWL_STYLE, (style & !frame_bits) | WS_POPUP as isize);
            let extended = GetWindowLongPtrW(self.0, GWL_EXSTYLE);
            SetWindowLongPtrW(self.0, GWL_EXSTYLE, extended | extended_bits);
        }
    }

    /// Places the window at `frame` (logical px, scaled by the target
    /// display's `scale`) above all others without activating it, and clips
    /// it to a rounded shape whose corners on the attached edge are pushed
    /// off-window so they stay square.
    pub fn place(self, frame: Rect, scale: f64, placement: EdgePlacement, radius: f64) {
        let physical = frame.scaled(scale);
        let (x, y, width, height) = (
            round(physical.x),
            round(physical.y),
            round(physical.width),
            round(physical.height),
        );
        let r = round(radius * scale).max(1);
        let (left, top, right, bottom) = match placement {
            EdgePlacement::Top => (0, -r, width, height),
            EdgePlacement::Left => (-r, 0, width, height),
            EdgePlacement::Right => (0, 0, width + r, height),
            EdgePlacement::Bottom | EdgePlacement::Off => (0, 0, width, height),
        };
        let flags = (SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_SHOWWINDOW).cast_unsigned();
        // SAFETY: live window on this thread. SetWindowRgn takes ownership
        // of the region (we must not free it), even on failure.
        unsafe {
            SetWindowPos(self.0, HWND_TOPMOST, x, y, width, height, flags);
            let region = CreateRoundRectRgn(left, top, right + 1, bottom + 1, 2 * r, 2 * r);
            if !region.is_null() {
                SetWindowRgn(self.0, region, 1);
            }
        }
    }

    pub fn hide(self) {
        // SAFETY: live window on this thread.
        unsafe {
            ShowWindow(self.0, SW_HIDE);
        }
    }

    pub fn show_without_activating(self) {
        // SAFETY: live window on this thread.
        unsafe {
            ShowWindow(self.0, SW_SHOWNOACTIVATE);
        }
    }

    /// Brings the window forward (restoring it only if minimized, so a
    /// maximized window stays maximized). Windows may decline to change the
    /// foreground window; it then flashes in the taskbar instead.
    pub fn bring_forward(self) {
        // SAFETY: live window on this thread.
        unsafe {
            let command = if IsIconic(self.0) != 0 {
                SW_RESTORE
            } else {
                SW_SHOW
            };
            ShowWindow(self.0, command);
            SetForegroundWindow(self.0);
        }
    }
}

/// The primary display (where the taskbar lives) in logical pixels, with
/// its own scale factor. The scale comes from that monitor, not from the
/// window, which may still be on another display when this is called.
pub fn primary_display() -> Option<(DisplayMetrics, f64)> {
    let mut info = MONITORINFO {
        cbSize: u32::try_from(size_of::<MONITORINFO>()).unwrap_or(0),
        ..MONITORINFO::default()
    };
    let (mut dpi_x, mut dpi_y) = (0u32, 0u32);
    // SAFETY: plain value arguments; `info` and the DPI values are
    // correctly sized out-params that outlive the calls.
    let (ok, dpi_ok) = unsafe {
        let monitor = MonitorFromPoint(
            POINT { x: 0, y: 0 },
            MONITOR_DEFAULTTOPRIMARY.cast_unsigned(),
        );
        let ok = GetMonitorInfoW(monitor, &raw mut info);
        let dpi_ok = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &raw mut dpi_x, &raw mut dpi_y);
        (ok, dpi_ok)
    };
    if ok == 0 {
        return None;
    }
    let scale = if dpi_ok >= 0 && dpi_x > 0 {
        f64::from(dpi_x) / BASE_DPI
    } else {
        1.0
    };
    let display = DisplayMetrics {
        bounds: logical(info.rcMonitor, scale),
        work_area: logical(info.rcWork, scale),
        notch_height: 0.0,
        notch_width: None,
    };
    Some((display, scale))
}

/// The user's preferred UI languages as BCP-47 tags (`de-DE`, `en-US`),
/// most preferred first. Empty when Windows can't say.
pub fn preferred_ui_languages() -> Vec<String> {
    let flags = MUI_LANGUAGE_NAME.cast_unsigned();
    let (mut count, mut length) = (0u32, 0u32);
    // SAFETY: a null buffer asks only for the required length (in UTF-16
    // units, including the terminators), written to `length`.
    let sized = unsafe {
        GetUserPreferredUILanguages(flags, &raw mut count, std::ptr::null_mut(), &raw mut length)
    };
    if sized == 0 || length == 0 {
        return Vec::new();
    }
    let mut buffer = vec![0u16; length as usize];
    // SAFETY: `buffer` holds exactly the `length` units requested above.
    let filled = unsafe {
        GetUserPreferredUILanguages(flags, &raw mut count, buffer.as_mut_ptr(), &raw mut length)
    };
    if filled == 0 {
        return Vec::new();
    }
    split_multi_sz(&buffer)
}

/// Splits a Win32 multi-string ("a\0b\0\0") into its parts.
fn split_multi_sz(buffer: &[u16]) -> Vec<String> {
    buffer
        .split(|&unit| unit == 0)
        .filter(|part| !part.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

fn logical(rect: RECT, scale: f64) -> Rect {
    let s = if scale > 0.0 { scale } else { 1.0 };
    Rect::new(
        f64::from(rect.left) / s,
        f64::from(rect.top) / s,
        f64::from(rect.right - rect.left) / s,
        f64::from(rect.bottom - rect.top) / s,
    )
}

#[allow(clippy::cast_possible_truncation)] // screen coordinates fit in i32
fn round(value: f64) -> i32 {
    value.round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_multi_string() {
        let buffer: Vec<u16> = "de-DE\0en-US\0\0".encode_utf16().collect();
        assert_eq!(split_multi_sz(&buffer), ["de-DE", "en-US"]);
        assert!(split_multi_sz(&[0, 0]).is_empty());
    }

    #[test]
    fn reports_at_least_one_ui_language() {
        assert!(!preferred_ui_languages().is_empty());
    }
}
