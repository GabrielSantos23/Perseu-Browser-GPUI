//! Chromium extensions hosted by the WebView2 runtime.
//!
//! The runtime loads unpacked extension folders into its shared profile
//! through `ICoreWebView2Profile7`; content scripts, MV3 service workers,
//! `chrome.storage` and `declarativeNetRequest` then run inside the runtime,
//! with no bridge into this app. Every view shares the profile, so a load
//! applies to them all, and a profile only exists once a view does.
//!
//! These calls wait for WebView2's asynchronous completions by pumping
//! messages (`wait_with_pump`), the way [`BrowserView::new`] waits for the
//! environment, so run them off the UI thread, like view creation does.

use std::path::Path;

use anyhow::{Result, anyhow};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2_13, ICoreWebView2BrowserExtension, ICoreWebView2BrowserExtensionList,
    ICoreWebView2Profile7,
};
use webview2_com::{
    BrowserExtensionEnableCompletedHandler, BrowserExtensionRemoveCompletedHandler,
    ProfileAddBrowserExtensionCompletedHandler, ProfileGetBrowserExtensionsCompletedHandler,
    take_pwstr, wait_with_pump,
};
use windows::core::{BOOL, HSTRING, Interface, PWSTR};

use crate::BrowserView;

/// An extension loaded into the profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionInfo {
    /// Runtime id, used in `chrome-extension://<id>/…` addresses.
    pub id: String,
    /// Name from the extension's manifest.
    pub name: String,
    /// Whether the runtime has it enabled right now.
    pub enabled: bool,
}

fn profile(view: &BrowserView) -> Result<ICoreWebView2Profile7> {
    Ok(unsafe {
        view.com_webview()
            .cast::<ICoreWebView2_13>()?
            .Profile()?
            .cast::<ICoreWebView2Profile7>()?
    })
}

/// The extensions currently in the profile.
pub fn installed(view: &BrowserView) -> Result<Vec<ExtensionInfo>> {
    Ok(installed_objects(view)?
        .into_iter()
        .map(|(info, _)| info)
        .collect())
}

fn installed_objects(
    view: &BrowserView,
) -> Result<Vec<(ExtensionInfo, ICoreWebView2BrowserExtension)>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let handler =
        ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |result, list| {
            let _ = tx.send(result.map(|()| {
                list.map(|list| read_list(&list).unwrap_or_default())
                    .unwrap_or_default()
            }));
            Ok(())
        }));
    unsafe { profile(view)?.GetBrowserExtensions(&handler)? };
    let extensions = wait_with_pump(rx)??;
    Ok(extensions)
}

fn read_list(
    list: &ICoreWebView2BrowserExtensionList,
) -> Result<Vec<(ExtensionInfo, ICoreWebView2BrowserExtension)>> {
    let mut count = 0u32;
    unsafe { list.Count(&mut count)? };
    let mut extensions = Vec::with_capacity(count as usize);
    for index in 0..count {
        let extension = unsafe { list.GetValueAtIndex(index)? };
        let info = read_extension(&extension)?;
        extensions.push((info, extension));
    }
    Ok(extensions)
}

fn read_extension(extension: &ICoreWebView2BrowserExtension) -> Result<ExtensionInfo> {
    let mut id = PWSTR::null();
    let mut name = PWSTR::null();
    let mut enabled = BOOL::default();
    unsafe {
        extension.Id(&mut id)?;
        extension.Name(&mut name)?;
        extension.IsEnabled(&mut enabled)?;
    }
    Ok(ExtensionInfo {
        id: take_pwstr(id),
        name: take_pwstr(name),
        enabled: enabled.as_bool(),
    })
}

/// Loads one unpacked extension folder into the profile, returning it.
pub fn add(view: &BrowserView, folder: &Path) -> Result<ExtensionInfo> {
    let (tx, rx) = std::sync::mpsc::channel();
    let handler =
        ProfileAddBrowserExtensionCompletedHandler::create(Box::new(move |result, extension| {
            let _ =
                tx.send(result.and_then(|()| extension.ok_or_else(windows::core::Error::empty)));
            Ok(())
        }));
    let path = HSTRING::from(folder.as_os_str());
    unsafe { profile(view)?.AddBrowserExtension(&path, &handler)? };
    let extension = wait_with_pump(rx)??;
    read_extension(&extension)
}

/// Enables or disables one loaded extension by its runtime id.
pub fn set_enabled(view: &BrowserView, id: &str, enabled: bool) -> Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    let handler = BrowserExtensionEnableCompletedHandler::create(Box::new(move |result| {
        let _ = tx.send(result);
        Ok(())
    }));
    unsafe { loaded(view, id)?.Enable(enabled, &handler)? };
    wait_with_pump(rx)??;
    Ok(())
}

/// Removes one loaded extension from the profile by its runtime id.
pub fn remove(view: &BrowserView, id: &str) -> Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    let handler = BrowserExtensionRemoveCompletedHandler::create(Box::new(move |result| {
        let _ = tx.send(result);
        Ok(())
    }));
    unsafe { loaded(view, id)?.Remove(&handler)? };
    wait_with_pump(rx)??;
    Ok(())
}

fn loaded(view: &BrowserView, id: &str) -> Result<ICoreWebView2BrowserExtension> {
    installed_objects(view)?
        .into_iter()
        .find(|(info, _)| info.id == id)
        .map(|(_, extension)| extension)
        .ok_or_else(|| anyhow!("extension {id} is not loaded"))
}
