use std::rc::Rc;
use std::sync::Arc;

use browser_core::omnibox::display_host;
use browser_core::tabs::Tab;
use gpui::{Image, ImageFormat, SharedString};
use webview_host::{BrowserView, SecurityState};

const NEW_TAB_TITLE: &str = "New tab";

#[derive(Default)]
pub struct TabRuntime {
    pub view: Option<Rc<BrowserView>>,
    pub view_pending: bool,
    pub focus_when_ready: bool,
    pub favicon: Option<Arc<Image>>,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub security: Option<SecurityState>,
    pub windows_opened: u32,
    pub blocked_popups: Vec<String>,
    pub playing_audio: bool,
    pub muted: bool,
    pub zoom: Option<f64>,
    /// An extension's pending window this tab's view takes over instead
    /// of loading its URL.
    pub adopt: Option<u64>,
}

impl TabRuntime {
    pub fn set_favicon(&mut self, png: Option<&[u8]>) {
        self.favicon =
            png.map(|bytes| Arc::new(Image::from_bytes(ImageFormat::Png, bytes.to_vec())));
    }
}

pub fn is_blank(url: &str) -> bool {
    display_host(url).is_empty()
}

pub fn host(url: &str) -> SharedString {
    SharedString::from(display_host(url).to_string())
}

pub fn display_title(tab: &Tab) -> SharedString {
    if is_blank(&tab.url) {
        SharedString::new_static(NEW_TAB_TITLE)
    } else if tab.title.trim().is_empty() {
        host(&tab.url)
    } else {
        SharedString::from(tab.title.clone())
    }
}
