use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

use raw_window_handle::RawWindowHandle;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Dwm::{
    DWMNCRP_DISABLED, DWMWA_NCRENDERING_POLICY, DwmSetWindowAttribute,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, IsIconic, SPI_GETCLIENTAREAANIMATION, SW_RESTORE,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetForegroundWindow, ShowWindow, SystemParametersInfoW,
};

fn hwnd(handle: RawWindowHandle) -> Option<HWND> {
    match handle {
        RawWindowHandle::Win32(handle) => Some(HWND(handle.hwnd.get() as *mut _)),
        _ => None,
    }
}

// Call only while no GPUI state is borrowed: Windows delivers activation messages synchronously.
pub fn bring_to_front(handle: RawWindowHandle) {
    if let Some(hwnd) = hwnd(handle) {
        let _ = unsafe { SetForegroundWindow(hwnd) };
    }
}

pub fn restore_and_focus(handle: RawWindowHandle) {
    if let Some(hwnd) = hwnd(handle) {
        unsafe {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

pub fn animations_disabled() -> bool {
    let mut enabled = windows::core::BOOL(1);
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut enabled as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_ok() && !enabled.as_bool()
}

pub fn cursor_position() -> Option<(f32, f32)> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.ok()?;
    Some((point.x as f32, point.y as f32))
}

pub fn remove_frame_shadow_and_border(handle: RawWindowHandle) {
    let Some(hwnd) = hwnd(handle) else {
        return;
    };
    let policy = DWMNCRP_DISABLED;
    let result = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_NCRENDERING_POLICY,
            std::ptr::from_ref(&policy).cast(),
            size_of_val(&policy) as u32,
        )
    };
    if let Err(error) = result {
        tracing::debug!("could not disable window frame rendering: {error}");
    }
}

const EXPLORER: &str = "explorer.exe";
const SHELL_EXECUTE_ERROR_LIMIT: usize = 32;
const WEBVIEW2_DOWNLOAD: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

pub fn launch_uri(uri: &str) {
    let uri = windows::core::HSTRING::from(uri);
    let result = unsafe {
        windows::Win32::UI::Shell::ShellExecuteW(
            None,
            windows::core::w!("open"),
            &uri,
            None,
            None,
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    };
    if result.0 as usize <= SHELL_EXECUTE_ERROR_LIMIT {
        tracing::warn!("no app opened the link (code {})", result.0 as usize);
    }
}

/// Explains that the WebView2 Runtime is missing and offers its download page.
pub fn offer_runtime_install() {
    use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONWARNING, MB_YESNO, MessageBoxW};
    let text = windows::core::HSTRING::from(format!(
        "{} needs the Microsoft Edge WebView2 Runtime, which is not installed.\n\nOpen the download page now?",
        browser_core::APP_NAME
    ));
    let title = windows::core::HSTRING::from(browser_core::APP_NAME);
    let choice = unsafe { MessageBoxW(None, &text, &title, MB_YESNO | MB_ICONWARNING) };
    if choice == IDYES {
        launch_uri(WEBVIEW2_DOWNLOAD);
    }
}

pub fn open_path(path: &Path) {
    if let Err(error) = Command::new(EXPLORER).arg(path).spawn() {
        tracing::warn!("could not open {}: {error}", path.display());
    }
}

pub fn show_in_folder(path: &Path) {
    // Explorer only parses /select when the path is quoted inside the same argument.
    let argument = format!("/select,\"{}\"", path.display());
    if let Err(error) = Command::new(EXPLORER).raw_arg(argument).spawn() {
        tracing::warn!("could not show {}: {error}", path.display());
    }
}
