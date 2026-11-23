use std::time::{Duration, Instant};

use browser_core::omnibox::display_host;
use browser_core::permissions::Decision;
use browser_core::security::origin_of;
use browser_core::tabs::TabId;
use browser_core::zoom;
use gpui::{
    AnyWindowHandle, AppContext, Bounds, Context, Entity, Pixels, Point, SharedString,
    Subscription, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
    point, size,
};
use raw_window_handle::HasWindowHandle;
use webview_host::SecurityState;

use super::Shell;
use crate::native;
use crate::site_menu::{
    SiteInfo, SiteMenu, SiteMenuEvent, SitePermission, SiteSecurity, site_menu_height,
};
use crate::theme::Theme;

/// A click on the menu button closes the menu (by taking focus) just before
/// it lands; a click this soon after that counts as "close", not "reopen".
const REOPEN_GUARD: Duration = Duration::from_millis(300);

pub(super) struct OpenSiteMenu {
    window: AnyWindowHandle,
    view: Entity<SiteMenu>,
    _events: Subscription,
}

impl Shell {
    /// Remembers a tab's zoom for its site, as Chrome does.
    pub(super) fn remember_zoom(&mut self, tab: TabId, factor: f64) {
        let Some(origin) = self.browser.tab(tab).and_then(|tab| origin_of(&tab.url)) else {
            return;
        };
        if (factor - 1.0).abs() < f64::EPSILON {
            self.site_zoom.remove(&origin);
        } else {
            self.site_zoom.insert(origin, factor);
        }
    }

    /// Gives a tab its site's zoom when it opens or moves to another site.
    pub(super) fn restore_zoom(&mut self, tab: TabId, previous_url: &str, url: &str) {
        let runtime = self.runtime.get(&tab);
        let fresh = runtime.is_none_or(|runtime| runtime.zoom.is_none());
        let origin = origin_of(url);
        if !fresh && origin == origin_of(previous_url) {
            return;
        }
        let factor = origin
            .and_then(|origin| self.site_zoom.get(&origin).copied())
            .unwrap_or(1.0);
        let current = runtime.and_then(|runtime| runtime.zoom).unwrap_or(1.0);
        if (factor - current).abs() < f64::EPSILON {
            return;
        }
        if let Some(view) = runtime.and_then(|runtime| runtime.view.clone()) {
            view.set_zoom(factor);
        }
    }

    pub(super) fn active_zoom(&self) -> f64 {
        self.active_runtime()
            .and_then(|runtime| runtime.zoom)
            .unwrap_or(1.0)
    }

    fn site_info(&self) -> SiteInfo {
        let url = self.active_url();
        let plain_http = url
            .get(..7)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http://"));
        let security = match self.active_runtime().and_then(|runtime| runtime.security) {
            _ if plain_http => SiteSecurity::NotSecure,
            Some(SecurityState::Insecure) => SiteSecurity::NotSecure,
            Some(SecurityState::Secure) => SiteSecurity::Secure,
            _ => SiteSecurity::Unknown,
        };
        let origin = origin_of(url).unwrap_or_default();
        let permissions = self
            .permissions
            .sites()
            .find(|(site, _)| *site == origin)
            .map(|(_, decisions)| {
                decisions
                    .iter()
                    .map(|(key, decision)| SitePermission {
                        key: key.clone(),
                        allowed: *decision == Decision::Allow,
                    })
                    .collect()
            })
            .unwrap_or_default();
        SiteInfo {
            host: SharedString::from(display_host(url).to_string()),
            security,
            zoom_percent: zoom::percent(self.active_zoom()),
            permissions,
        }
    }

    /// Opens the site menu under the title bar, centered on `anchor`, the
    /// click position in the main window.
    pub(super) fn open_site_menu(
        &mut self,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let just_closed = self
            .site_menu_closed
            .take()
            .is_some_and(|closed| closed.elapsed() < REOPEN_GUARD);
        if self.close_site_menu(window, cx) || just_closed {
            return;
        }
        let info = self.site_info();
        let metrics = &Theme::of(cx).metrics;
        let width = metrics.site_menu_width;
        let height = site_menu_height(&info, cx);
        let window_bounds = window.bounds();
        let max_x =
            (window_bounds.size.width - width - metrics.frame_inset).max(metrics.frame_inset);
        let x = (anchor.x - width / 2.).clamp(metrics.frame_inset, max_x);
        let y = (anchor.y + metrics.title_button_size / 2. + metrics.radius_sm)
            .max(metrics.title_bar_height);
        let origin = window_bounds.origin + point(x, y);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                origin,
                size(width, height),
            ))),
            titlebar: None,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            ..WindowOptions::default()
        };
        let mut menu: Option<Entity<SiteMenu>> = None;
        let opened = cx.open_window(options, |popup, cx| {
            if let Ok(handle) = popup.window_handle() {
                native::remove_frame_shadow_and_border(handle.as_raw());
            }
            let view = cx.new(|cx| SiteMenu::new(info, popup, cx));
            menu = Some(view.clone());
            view
        });
        let (handle, view) = match (opened, menu) {
            (Ok(handle), Some(view)) => (handle, view),
            (Err(error), _) => {
                tracing::error!("failed to open site menu: {error:#}");
                return;
            }
            (Ok(_), None) => return,
        };
        let events = cx.subscribe_in(&view, window, |shell, _, event, window, cx| {
            shell.on_site_menu_event(event, window, cx)
        });
        self.site_menu = Some(OpenSiteMenu {
            window: handle.into(),
            view,
            _events: events,
        });
    }

    fn close_site_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(open) = self.site_menu.take() else {
            return false;
        };
        self.dismiss_popup(open.window, window, cx, |_, _, _| {});
        self.site_menu_closed = Some(Instant::now());
        true
    }

    pub(super) fn on_site_menu_event(
        &mut self,
        event: &SiteMenuEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self.active_zoom();
        let zoom_to = match event {
            SiteMenuEvent::ZoomIn => Some(zoom::zoom_in(current)),
            SiteMenuEvent::ZoomOut => Some(zoom::zoom_out(current)),
            SiteMenuEvent::ZoomReset => Some(1.0),
            _ => None,
        };
        if let Some(factor) = zoom_to {
            if let Some(view) = self.active_view() {
                view.set_zoom(factor);
            }
            if let Some(tab) = self.browser.active_tab_id() {
                self.update_runtime(tab, |runtime| runtime.zoom = Some(factor));
            }
            if let Some(open) = &self.site_menu {
                open.view
                    .update(cx, |menu, cx| menu.set_zoom(zoom::percent(factor), cx));
            }
            return;
        }
        let url = self.active_url().to_string();
        let origin = origin_of(&url);
        let view = self.active_view();
        let message = match event {
            SiteMenuEvent::CopyLink => {
                self.close_site_menu(window, cx);
                self.copy_link(url, window, cx);
                return;
            }
            SiteMenuEvent::ClearData => {
                if let (Some(view), Some(origin)) = (view, &origin) {
                    view.clear_site_data(origin);
                }
                Some("Site data cleared")
            }
            SiteMenuEvent::ResetPermission(key) => {
                if let Some(origin) = &origin {
                    self.permissions.revoke(origin, key);
                    self.save_permissions();
                }
                Some("Permission reset")
            }
            _ => None,
        };
        self.close_site_menu(window, cx);
        if let Some(message) = message {
            self.show_toast(message, window, cx);
        }
        cx.notify();
    }
}
