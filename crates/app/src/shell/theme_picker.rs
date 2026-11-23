use gpui::{
    AnyWindowHandle, AppContext, Bounds, Context, Entity, Pixels, Point, Subscription, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, point, size,
};
use raw_window_handle::HasWindowHandle;

use super::Shell;
use super::menus::SpaceAction;
use crate::native;
use crate::theme::Theme;
use crate::theme_picker::{SpaceLook, ThemePicker, ThemePickerEvent, picker_height, picker_width};

pub(super) struct OpenThemePicker {
    window: AnyWindowHandle,
    _events: Subscription,
}

impl Shell {
    /// Opens the space theme picker beside the sidebar, its bottom level
    /// with `anchor`, a click position in the main window.
    pub(super) fn open_theme_picker(
        &mut self,
        anchor: Point<Pixels>,
        show_icons: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(open) = self.theme_picker.take() {
            self.dismiss_popup(open.window, window, cx, |_, _, _| {});
            return;
        }
        let metrics = &Theme::of(cx).metrics;
        let can_delete = self.browser.spaces().len() > 1;
        let picker_size = size(
            picker_width(),
            picker_height(self.settings.custom_colors.len(), can_delete, cx),
        );
        let window_bounds = window.bounds();
        let max_x = (window_bounds.size.width - picker_size.width - metrics.frame_inset)
            .max(metrics.frame_inset);
        let sidebar_left = if self.compact {
            metrics.frame_inset
        } else {
            Pixels::ZERO
        };
        let x = (sidebar_left + self.sidebar_width + metrics.radius_md)
            .clamp(metrics.frame_inset, max_x);
        let max_y = (window_bounds.size.height - picker_size.height - metrics.frame_inset)
            .max(metrics.frame_inset);
        let y = (anchor.y + metrics.row_height / 2. - picker_size.height)
            .clamp(metrics.frame_inset, max_y);
        let origin = window_bounds.origin + point(x, y);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(origin, picker_size))),
            titlebar: None,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            ..WindowOptions::default()
        };
        let space = self.browser.space();
        let look = SpaceLook {
            name: space.name.clone(),
            icon: space.icon.clone(),
            icon_color: space.icon_color,
            show_icons,
            theme: space.theme,
            intensity: space.intensity,
            texture: space.texture,
            texture_kind: space.texture_kind,
            state: space.theme_state.clone(),
            custom_colors: self.settings.custom_colors.clone(),
            can_delete,
        };
        let mut picker: Option<Entity<ThemePicker>> = None;
        let opened = cx.open_window(options, |popup, cx| {
            if let Ok(handle) = popup.window_handle() {
                native::remove_frame_shadow_and_border(handle.as_raw());
            }
            let view = cx.new(|cx| ThemePicker::new(look, popup, cx));
            picker = Some(view.clone());
            view
        });
        let (handle, view) = match (opened, picker) {
            (Ok(handle), Some(view)) => (handle, view),
            (Err(error), _) => {
                tracing::error!("failed to open theme picker: {error:#}");
                return;
            }
            (Ok(_), None) => return,
        };
        let events = cx.subscribe_in(&view, window, |shell, _, event, window, cx| {
            match event {
                ThemePickerEvent::Changed {
                    theme,
                    intensity,
                    texture,
                    texture_kind,
                    state,
                } => {
                    let space = shell.browser.space_mut();
                    space.theme = *theme;
                    space.intensity = *intensity;
                    space.texture = *texture;
                    space.texture_kind = *texture_kind;
                    space.theme_state = Some(state.clone());
                }
                ThemePickerEvent::CustomColors(colors) => {
                    let colors = colors.clone();
                    shell.update_settings(|settings| settings.custom_colors = colors, cx);
                }
                ThemePickerEvent::Renamed(name) => shell.browser.space_mut().name = name.clone(),
                ThemePickerEvent::SetIcon { name, color } => {
                    let space = shell.browser.space_mut();
                    space.icon = name.clone();
                    space.icon_color = *color;
                }
                ThemePickerEvent::Delete => {
                    let index = shell.browser.active_space_index();
                    if let Some(open) = shell.theme_picker.take() {
                        shell.dismiss_popup(open.window, window, cx, move |shell, window, cx| {
                            shell.run_space_action(index, SpaceAction::Delete, window, cx)
                        });
                    }
                }
                ThemePickerEvent::Dismiss => {
                    if let Some(open) = shell.theme_picker.take() {
                        shell.dismiss_popup(open.window, window, cx, |_, _, _| {});
                    }
                }
            }
            cx.notify();
        });
        self.theme_picker = Some(OpenThemePicker {
            window: handle.into(),
            _events: events,
        });
    }
}
