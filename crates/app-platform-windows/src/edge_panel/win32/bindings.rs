windows_link::link!("gdi32.dll" "system" fn CreateRoundRectRgn(x1 : i32, y1 : i32, x2 : i32, y2 : i32, w : i32, h : i32) -> HRGN);
windows_link::link!("user32.dll" "system" fn EnumThreadWindows(dwthreadid : u32, lpfn : WNDENUMPROC, lparam : LPARAM) -> BOOL);
windows_link::link!("kernel32.dll" "system" fn GetCurrentThreadId() -> u32);
windows_link::link!("api-ms-win-shcore-scaling-l1-1-1.dll" "system" fn GetDpiForMonitor(hmonitor : HMONITOR, dpitype : MONITOR_DPI_TYPE, dpix : *mut u32, dpiy : *mut u32) -> HRESULT);
windows_link::link!("user32.dll" "system" fn GetDpiForWindow(hwnd : HWND) -> u32);
windows_link::link!("user32.dll" "system" fn GetMonitorInfoW(hmonitor : HMONITOR, lpmi : *mut MONITORINFO) -> BOOL);
windows_link::link!("kernel32.dll" "system" fn GetUserPreferredUILanguages(dwflags : u32, pulnumlanguages : *mut u32, pwszlanguagesbuffer : *mut u16, pcchlanguagesbuffer : *mut u32) -> BOOL);
#[cfg(any(
    target_arch = "aarch64",
    target_arch = "arm64ec",
    target_arch = "x86_64"
))]
windows_link::link!("user32.dll" "system" fn GetWindowLongPtrW(hwnd : HWND, nindex : i32) -> isize);
#[cfg(target_pointer_width = "32")]
pub use GetWindowLongW as GetWindowLongPtrW;
windows_link::link!("user32.dll" "system" fn GetWindowLongW(hwnd : HWND, nindex : i32) -> i32);
windows_link::link!("user32.dll" "system" fn GetWindowTextW(hwnd : HWND, lpstring : PWSTR, nmaxcount : i32) -> i32);
windows_link::link!("user32.dll" "system" fn IsIconic(hwnd : HWND) -> BOOL);
windows_link::link!("user32.dll" "system" fn IsWindow(hwnd : HWND) -> BOOL);
windows_link::link!("user32.dll" "system" fn MonitorFromPoint(pt : POINT, dwflags : u32) -> HMONITOR);
windows_link::link!("user32.dll" "system" fn SetForegroundWindow(hwnd : HWND) -> BOOL);
#[cfg(any(
    target_arch = "aarch64",
    target_arch = "arm64ec",
    target_arch = "x86_64"
))]
windows_link::link!("user32.dll" "system" fn SetWindowLongPtrW(hwnd : HWND, nindex : i32, dwnewlong : isize) -> isize);
#[cfg(target_pointer_width = "32")]
pub use SetWindowLongW as SetWindowLongPtrW;
windows_link::link!("user32.dll" "system" fn SetWindowLongW(hwnd : HWND, nindex : i32, dwnewlong : i32) -> i32);
windows_link::link!("user32.dll" "system" fn SetWindowPos(hwnd : HWND, hwndinsertafter : HWND, x : i32, y : i32, cx : i32, cy : i32, uflags : u32) -> BOOL);
windows_link::link!("user32.dll" "system" fn SetWindowRgn(hwnd : HWND, hrgn : HRGN, bredraw : BOOL) -> i32);
windows_link::link!("user32.dll" "system" fn ShowWindow(hwnd : HWND, ncmdshow : i32) -> BOOL);
pub type BOOL = i32;
pub const GWL_EXSTYLE: i32 = -20;
pub const GWL_STYLE: i32 = -16;
pub type HMONITOR = *mut core::ffi::c_void;
pub type HRESULT = i32;
pub type HRGN = *mut core::ffi::c_void;
pub type HWND = *mut core::ffi::c_void;
pub const HWND_TOPMOST: HWND = -1 as _;
pub type LPARAM = isize;
pub const MDT_EFFECTIVE_DPI: MONITOR_DPI_TYPE = 0;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MONITORINFO {
    pub cbSize: u32,
    pub rcMonitor: RECT,
    pub rcWork: RECT,
    pub dwFlags: u32,
}
pub const MONITOR_DEFAULTTOPRIMARY: i32 = 1;
pub type MONITOR_DPI_TYPE = i32;
pub const MUI_LANGUAGE_NAME: i32 = 8;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}
pub type PWSTR = *mut u16;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
pub const SWP_FRAMECHANGED: i32 = 32;
pub const SWP_NOACTIVATE: i32 = 16;
pub const SWP_SHOWWINDOW: i32 = 64;
pub const SW_HIDE: i32 = 0;
pub const SW_RESTORE: i32 = 9;
pub const SW_SHOW: i32 = 5;
pub const SW_SHOWNOACTIVATE: i32 = 4;
pub type WNDENUMPROC = Option<unsafe extern "system" fn(param0: HWND, param1: LPARAM) -> BOOL>;
pub const WS_CAPTION: i32 = 12582912;
pub const WS_EX_NOACTIVATE: i32 = 134217728;
pub const WS_EX_TOOLWINDOW: i32 = 128;
pub const WS_EX_TOPMOST: i32 = 8;
pub const WS_MAXIMIZEBOX: i32 = 65536;
pub const WS_MINIMIZEBOX: i32 = 131072;
pub const WS_POPUP: u32 = 2147483648;
pub const WS_SYSMENU: i32 = 524288;
pub const WS_THICKFRAME: i32 = 262144;
pub const WS_VISIBLE: i32 = 268435456;
