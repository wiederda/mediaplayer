//! Plattformspezifische Anpassungen des nativen Fensters.

/// Färbt die Windows-Titelleiste dunkel, damit sie zum dunklen Programm passt.
/// Auf allen anderen Systemen passiert nichts.
#[cfg(windows)]
pub fn enable_dark_titlebar(window: impl raw_window_handle::HasWindowHandle) {
    use raw_window_handle::RawWindowHandle;

    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let hwnd = win32.hwnd.get() as *mut core::ffi::c_void;

    // Windows 10 ab Build 1903 verwendet Attribut 20, ältere Builds Attribut 19.
    for attribute in [20_u32, 19_u32] {
        let value: i32 = 1;
        let result = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                attribute,
                &value as *const i32 as *const core::ffi::c_void,
                std::mem::size_of::<i32>() as u32,
            )
        };
        if result == 0 {
            break;
        }
    }
}

#[cfg(windows)]
#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(
        hwnd: *mut core::ffi::c_void,
        attribute: u32,
        value: *const core::ffi::c_void,
        size: u32,
    ) -> i32;
}

#[cfg(not(windows))]
pub fn enable_dark_titlebar(_window: impl raw_window_handle::HasWindowHandle) {}
