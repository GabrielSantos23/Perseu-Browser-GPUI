use std::time::Duration;

use gpui::{
    AnyWindowHandle, AppContext, Bounds, ClipboardItem, Context, Pixels, SharedString, Task,
    Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, point, size,
};
use raw_window_handle::HasWindowHandle;

use super::Shell;
use crate::native;
use crate::page;
use crate::theme::Theme;
use crate::toast::Toast;

const TOAST_DURATION: Duration = Duration::from_millis(1800);
const LINK_COPIED: &str = "Link copied";

pub(super) struct OpenToast {
    window: AnyWindowHandle,
    _close: Task<()>,
}

impl Shell {
    pub(super) fn copy_active_url(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.active_url().to_string();
        if !page::is_blank(&url) {
            self.copy_link(url, window, cx);
        }
    }

    pub(super) fn copy_link(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(url));
        self.show_toast(LINK_COPIED, window, cx);
    }

    /// Shows a short message in the top-right corner of the page without
    /// taking focus from it.
    pub(super) fn show_toast(
        &mut self,
        message: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_toast(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(self.toast_bounds(window, cx))),
            titlebar: None,
            focus: false,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            ..WindowOptions::default()
        };
        let message = message.into();
        let opened = cx.open_window(options, |popup, cx| {
            if let Ok(handle) = popup.window_handle() {
                native::remove_frame_shadow_and_border(handle.as_raw());
            }
            cx.new(|_| Toast::new(message))
        });
        let handle: AnyWindowHandle = match opened {
            Ok(handle) => handle.into(),
            Err(error) => {
                tracing::error!("failed to show toast: {error:#}");
                return;
            }
        };
        let close = cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(TOAST_DURATION).await;
            shell
                .update(cx, |shell, cx| {
                    if shell
                        .toast
                        .as_ref()
                        .is_some_and(|open| open.window == handle)
                    {
                        shell.close_toast(cx);
                    }
                })
                .ok();
        });
        self.toast = Some(OpenToast {
            window: handle,
            _close: close,
        });
    }

    fn close_toast(&mut self, cx: &mut Context<Self>) {
        if let Some(open) = self.toast.take() {
            cx.defer(move |cx| {
                open.window
                    .update(cx, |_, popup, _| popup.remove_window())
                    .ok();
            });
        }
    }

    fn toast_bounds(&self, window: &Window, cx: &Context<Self>) -> Bounds<Pixels> {
        let metrics = &Theme::of(cx).metrics;
        let window_bounds = window.bounds();
        let content = self.content_bounds.get();
        let right = if content.size.width > Pixels::ZERO {
            content.origin.x + content.size.width
        } else {
            window_bounds.size.width
        };
        let top = content.origin.y.max(metrics.title_bar_height);
        Bounds::new(
            point(
                window_bounds.origin.x + right - metrics.toast_width - metrics.frame_inset,
                window_bounds.origin.y + top + metrics.frame_inset,
            ),
            size(metrics.toast_width, metrics.toast_height),
        )
    }
}
