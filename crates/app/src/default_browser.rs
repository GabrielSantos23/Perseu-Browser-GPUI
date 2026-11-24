use std::iter::once;

use anyhow::{Context as _, Result};
use browser_core::APP_NAME;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegGetValueW, RegSetKeyValueW,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, w};

const PROG_ID_SUFFIX: &str = "HTML";
const DESCRIPTION: &str = "A sidebar-first web browser";
const URL_SCHEMES: [&str; 2] = ["http", "https"];
const FILE_TYPES: [&str; 2] = [".htm", ".html"];
/// Where Windows records the https choice: recent Windows 11 builds write
/// `UserChoiceLatest\ProgId`, older ones `UserChoice`.
const USER_CHOICES: [&str; 2] = [
    r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoiceLatest\ProgId",
    r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice",
];
const MAX_PROG_ID_UNITS: usize = 256;
const SHELL_EXECUTE_ERROR_LIMIT: usize = 32;

fn prog_id() -> String {
    format!("{APP_NAME}{PROG_ID_SUFFIX}")
}

/// Lists this executable as a web browser for the current user, so it shows
/// up in Windows' Default apps. Windows only lets the user make it the default.
pub fn register() -> Result<()> {
    let exe = std::env::current_exe().context("locating the executable")?;
    let exe = exe.display().to_string();
    let icon = format!("\"{exe}\",0");
    let prog_id = prog_id();
    let client = format!(r"Software\Clients\StartMenuInternet\{APP_NAME}");
    let capabilities = format!(r"{client}\Capabilities");
    let class = format!(r"Software\Classes\{prog_id}");

    set(&client, None, APP_NAME)?;
    set(&format!(r"{client}\DefaultIcon"), None, &icon)?;
    set(
        &format!(r"{client}\shell\open\command"),
        None,
        &format!("\"{exe}\""),
    )?;
    set(&capabilities, Some("ApplicationName"), APP_NAME)?;
    set(&capabilities, Some("ApplicationDescription"), DESCRIPTION)?;
    set(&capabilities, Some("ApplicationIcon"), &icon)?;
    for scheme in URL_SCHEMES {
        set(
            &format!(r"{capabilities}\URLAssociations"),
            Some(scheme),
            &prog_id,
        )?;
    }
    for file_type in FILE_TYPES {
        set(
            &format!(r"{capabilities}\FileAssociations"),
            Some(file_type),
            &prog_id,
        )?;
    }
    set(
        r"Software\RegisteredApplications",
        Some(APP_NAME),
        &capabilities,
    )?;
    set(&class, None, &format!("{APP_NAME} Document"))?;
    set(&format!(r"{class}\DefaultIcon"), None, &icon)?;
    set(
        &format!(r"{class}\shell\open\command"),
        None,
        &format!("\"{exe}\" \"%1\""),
    )
}

/// Whether the user picked this app for https links.
pub fn is_default() -> bool {
    USER_CHOICES
        .iter()
        .find_map(|key| read_prog_id(key))
        .is_some_and(|chosen| chosen == prog_id())
}

fn read_prog_id(key: &str) -> Option<String> {
    let mut buffer = [0u16; MAX_PROG_ID_UNITS];
    let mut size = (buffer.len() * size_of::<u16>()) as u32;
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            &HSTRING::from("ProgId"),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if read.is_err() {
        return None;
    }
    let units = (size as usize / size_of::<u16>()).saturating_sub(1);
    Some(String::from_utf16_lossy(&buffer[..units.min(buffer.len())])).filter(|id| !id.is_empty())
}

/// Opens Windows' Default apps page, on this app's entry where supported.
/// Runs on its own thread: ShellExecute pumps window messages while it
/// waits, which would re-enter the UI in the middle of a click.
pub fn open_default_apps() {
    std::thread::spawn(|| {
        let page = HSTRING::from(format!(
            "ms-settings:defaultapps?registeredAppUser={APP_NAME}"
        ));
        let result = unsafe { ShellExecuteW(None, w!("open"), &page, None, None, SW_SHOWNORMAL) };
        if result.0 as usize <= SHELL_EXECUTE_ERROR_LIMIT {
            tracing::warn!("could not open Default apps (code {})", result.0 as usize);
        }
    });
}

fn set(key: &str, name: Option<&str>, value: &str) -> Result<()> {
    let data: Vec<u16> = value.encode_utf16().chain(once(0)).collect();
    let name = name.map(HSTRING::from);
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            name.as_ref().map_or(windows::core::PCWSTR::null(), |name| {
                windows::core::PCWSTR(name.as_ptr())
            }),
            REG_SZ.0,
            Some(data.as_ptr().cast()),
            (data.len() * size_of::<u16>()) as u32,
        )
    }
    .ok()
    .with_context(|| format!(r"writing HKCU\{key}"))
}
