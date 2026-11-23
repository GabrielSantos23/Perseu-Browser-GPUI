use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::Duration;

use browser_core::APP_NAME;
use futures::channel::mpsc::UnboundedSender;
use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, FindWindowExW,
    HWND_MESSAGE, RegisterClassW, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_COPYDATA, WNDCLASSW,
};
use windows::core::{HSTRING, PCWSTR};

const COPYDATA_TAG: usize = 0x5342_5752;
const FORWARD_ATTEMPTS: u32 = 20;
const FORWARD_RETRY: Duration = Duration::from_millis(100);
const SEND_TIMEOUT_MS: u32 = 3_000;
const TARGET_SEPARATOR: char = '\n';

thread_local! {
    static LISTENER: RefCell<Option<UnboundedSender<Vec<String>>>> = const { RefCell::new(None) };
}

pub enum Launch {
    Primary,
    Forwarded,
}

/// Marks a sandboxed run (`PERSEU_DATA_ROOT`, the same switch
/// `paths` uses) so its mutex and listener window cannot collide with the
/// daily app's.
fn sandbox_tag() -> String {
    match std::env::var_os("PERSEU_DATA_ROOT") {
        Some(root) => {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            root.hash(&mut hasher);
            format!(".{:016x}", hasher.finish())
        }
        None => String::new(),
    }
}

fn mutex_name() -> HSTRING {
    HSTRING::from(format!("Local\\{APP_NAME}.instance{}", sandbox_tag()))
}

fn listener_class() -> HSTRING {
    HSTRING::from(format!("{APP_NAME}.ipc{}", sandbox_tag()))
}

/// Makes this process the running instance, or hands `targets` to the one
/// already running and reports that this process should exit.
pub fn claim(targets: &[String]) -> Launch {
    let created = unsafe { CreateMutexW(None, false, &mutex_name()) };
    let already_running = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if let Err(error) = created {
        tracing::warn!("could not check for a running instance: {error}");
        return Launch::Primary;
    }
    if !already_running {
        return Launch::Primary;
    }
    for _ in 0..FORWARD_ATTEMPTS {
        if let Some(listener) = find_listener()
            && forward(listener, targets)
        {
            return Launch::Forwarded;
        }
        std::thread::sleep(FORWARD_RETRY);
    }
    tracing::warn!("another instance is running but not answering; starting a second one");
    Launch::Primary
}

/// Starts receiving targets from later launches. Each launch arrives as one
/// batch, already validated; an empty batch means "come to the front".
pub fn listen(sender: UnboundedSender<Vec<String>>) {
    LISTENER.with(|listener| *listener.borrow_mut() = Some(sender));
    let class = listener_class();
    let result = unsafe {
        GetModuleHandleW(None).and_then(|module| {
            let class_info = WNDCLASSW {
                lpfnWndProc: Some(listener_proc),
                hInstance: module.into(),
                lpszClassName: PCWSTR(class.as_ptr()),
                ..WNDCLASSW::default()
            };
            RegisterClassW(&class_info);
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                &class,
                None,
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(module.into()),
                None,
            )
        })
    };
    if let Err(error) = result {
        tracing::error!("links from other apps will open new windows: {error}");
    }
}

/// Turns a command-line argument into a URL to open: http(s) URLs as given,
/// existing files as `file:///` URLs. Everything else is ignored.
pub fn accepted_target(argument: &str) -> Option<String> {
    let argument = argument.trim();
    let lower = argument.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Some(argument.to_string());
    }
    let path = std::path::absolute(Path::new(argument)).ok()?;
    path.is_file().then(|| file_url(&path))
}

fn file_url(path: &Path) -> String {
    let mut url = String::from("file:///");
    for c in path.to_string_lossy().chars() {
        match c {
            '\\' => url.push('/'),
            ' ' => url.push_str("%20"),
            '%' => url.push_str("%25"),
            '#' => url.push_str("%23"),
            '?' => url.push_str("%3F"),
            c => url.push(c),
        }
    }
    url
}

fn find_listener() -> Option<HWND> {
    unsafe { FindWindowExW(Some(HWND_MESSAGE), None, &listener_class(), None) }.ok()
}

fn forward(listener: HWND, targets: &[String]) -> bool {
    let payload: Vec<u16> = targets
        .join(&TARGET_SEPARATOR.to_string())
        .encode_utf16()
        .collect();
    let data = COPYDATASTRUCT {
        dwData: COPYDATA_TAG,
        cbData: (payload.len() * size_of::<u16>()) as u32,
        lpData: payload.as_ptr() as *mut _,
    };
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        SendMessageTimeoutW(
            listener,
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&data as *const COPYDATASTRUCT as isize),
            SMTO_ABORTIFHUNG,
            SEND_TIMEOUT_MS,
            None,
        )
        .0 != 0
    }
}

unsafe extern "system" fn listener_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_COPYDATA && lparam.0 != 0 {
        let data = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
        if data.dwData == COPYDATA_TAG {
            let text = if data.cbData == 0 || data.lpData.is_null() {
                String::new()
            } else {
                let units = unsafe {
                    std::slice::from_raw_parts(
                        data.lpData as *const u16,
                        data.cbData as usize / size_of::<u16>(),
                    )
                };
                String::from_utf16_lossy(units)
            };
            let targets: Vec<String> = text
                .split(TARGET_SEPARATOR)
                .filter_map(accepted_target)
                .collect();
            LISTENER.with(|listener| {
                if let Some(sender) = listener.borrow().as_ref() {
                    let _ = sender.unbounded_send(targets);
                }
            });
            return LRESULT(1);
        }
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_urls_pass_and_other_schemes_are_dropped() {
        assert_eq!(
            accepted_target(" https://example.com/a?b "),
            Some("https://example.com/a?b".to_string())
        );
        assert_eq!(accepted_target("HTTP://x.y").as_deref(), Some("HTTP://x.y"));
        assert_eq!(accepted_target("javascript:alert(1)"), None);
        assert_eq!(accepted_target("--flag"), None);
        assert_eq!(accepted_target(""), None);
    }

    #[test]
    fn crafted_arguments_cannot_become_flags() {
        assert_eq!(accepted_target("--remote-debugging-port=9222"), None);
        assert_eq!(accepted_target(r"--user-data-dir=C:\evil"), None);
        assert_eq!(accepted_target("\"--flag\""), None);
        assert_eq!(
            accepted_target("https://a.example/\" --flag").as_deref(),
            Some("https://a.example/\" --flag")
        );
    }

    #[test]
    fn existing_files_become_file_urls() {
        let dir = std::env::temp_dir().join(format!("perseu test {}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("page #1.html");
        std::fs::write(&file, "<p>hi</p>").unwrap();
        let url = accepted_target(&file.display().to_string()).unwrap();
        assert!(url.starts_with("file:///"));
        assert!(url.contains("perseu%20test%20"));
        assert!(url.ends_with("page%20%231.html"));
        assert!(!url.contains('\\'));
        assert_eq!(
            accepted_target(&dir.join("missing.html").display().to_string()),
            None
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
