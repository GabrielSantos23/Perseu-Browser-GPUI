use std::time::Duration;

use gpui::{
    AnyView, App, ClickEvent, Context, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, MouseUpEvent, ParentElement, Pixels, Point, StatefulInteractiveElement, Styled,
    Window, WindowControlArea, div, prelude::FluentBuilder,
};

use super::Shell;
use crate::actions;
use crate::context_menu::MenuEntry;
use crate::icons;
use crate::theme::Theme;
use crate::tooltip::{Side, tooltip, tooltip_with_keys};

#[derive(Default)]
struct NavState {
    loading: bool,
    can_go_back: bool,
    can_go_forward: bool,
    has_view: bool,
}

const RELOAD_HOLD: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReloadAction {
    Normal,
    Hard,
    EmptyCache,
}

type ClickHandler = fn(&mut Shell, &ClickEvent, &mut Window, &mut Context<Shell>);

impl Shell {
    pub(super) fn render_nav_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let buttons = if self.settings.site_title_in_sidebar {
            Vec::new()
        } else {
            self.render_nav_buttons(cx)
        };
        let theme = Theme::of(cx);

        div()
            .flex()
            .flex_row()
            .flex_shrink_0()
            .items_center()
            .when(!self.compact && !self.settings.hide_title_bar, |bar| {
                bar.w(self.sidebar_width)
            })
            .h_full()
            .pl(theme.metrics.frame_inset)
            .gap(theme.metrics.radius_sm / 2.)
            .children(buttons)
            .child(drag_area("nav-bar-drag"))
    }

    /// The top of the sidebar when the website name lives there: the
    /// sidebar toggle, then back, forward and reload.
    pub(super) fn render_sidebar_nav(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let buttons = self.render_nav_buttons(cx);
        let theme = Theme::of(cx);
        div()
            .flex()
            .flex_row()
            .flex_shrink_0()
            .items_center()
            .h(theme.metrics.title_bar_height)
            .px(theme.metrics.radius_sm)
            .when(self.compact, |row| row.mt(theme.metrics.frame_inset))
            .gap(theme.metrics.radius_sm / 2.)
            .child(nav_button(
                "sidebar-toggle",
                icons::SIDEBAR,
                true,
                |this, _, window, cx| this.toggle_compact(window, cx),
                tooltip_with_keys(Side::Right, "Toggle sidebar", actions::ToggleCompactMode),
                cx,
            ))
            .child(nav_button(
                "sidebar-search",
                icons::SEARCH,
                true,
                |this, _, window, cx| this.edit_current_address(window, cx),
                tooltip_with_keys(
                    Side::Right,
                    "Search or enter address",
                    actions::FocusAddressBar,
                ),
                cx,
            ))
            .child(drag_area("sidebar-nav-drag"))
            .children(buttons)
    }

    fn render_nav_buttons(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let runtime = self
            .active_runtime()
            .map(|runtime| NavState {
                loading: runtime.loading,
                can_go_back: runtime.can_go_back,
                can_go_forward: runtime.can_go_forward,
                has_view: runtime.view.is_some(),
            })
            .unwrap_or_default();
        let has_view = runtime.has_view;
        let reload = if runtime.loading {
            nav_button(
                "stop",
                icons::STOP,
                has_view,
                Self::stop,
                tooltip("Stop"),
                cx,
            )
            .into_any_element()
        } else {
            nav_button(
                "reload",
                icons::RELOAD,
                has_view,
                Self::reload,
                tooltip_with_keys(Side::Right, "Reload", actions::Reload),
                cx,
            )
            .when(has_view, |button| {
                button
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            this.start_reload_hold(event.position, window, cx)
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Right,
                        cx.listener(|this, event: &MouseUpEvent, window, cx| {
                            this.open_reload_menu(event.position, window, cx)
                        }),
                    )
            })
            .into_any_element()
        };
        vec![
            nav_button(
                "back",
                icons::BACK,
                runtime.can_go_back,
                Self::go_back,
                tooltip_with_keys(Side::Right, "Back", actions::GoBack),
                cx,
            )
            .into_any_element(),
            nav_button(
                "forward",
                icons::FORWARD,
                runtime.can_go_forward,
                Self::go_forward,
                tooltip_with_keys(Side::Right, "Forward", actions::GoForward),
                cx,
            )
            .into_any_element(),
            reload,
        ]
    }

    fn go_back(&mut self, _: &ClickEvent, _: &mut Window, _: &mut Context<Self>) {
        if let Some(view) = self.active_view() {
            view.go_back();
        }
    }

    fn go_forward(&mut self, _: &ClickEvent, _: &mut Window, _: &mut Context<Self>) {
        if let Some(view) = self.active_view() {
            view.go_forward();
        }
    }

    fn reload(&mut self, _: &ClickEvent, _: &mut Window, _: &mut Context<Self>) {
        if self.reload_hold.take().is_none() {
            return;
        }
        if let Some(view) = self.active_view() {
            view.reload();
        }
    }

    fn start_reload_hold(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reload_hold = Some(cx.spawn_in(window, async move |shell, cx| {
            cx.background_executor().timer(RELOAD_HOLD).await;
            shell
                .update_in(cx, |shell, window, cx| {
                    shell.reload_hold = None;
                    shell.open_reload_menu(position, window, cx);
                })
                .ok();
        }));
    }

    fn open_reload_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = vec![
            MenuEntry::item("Normal reload", ReloadAction::Normal),
            MenuEntry::item("Hard reload", ReloadAction::Hard),
            MenuEntry::item("Empty cache and hard reload", ReloadAction::EmptyCache),
        ];
        let below_bar = gpui::point(position.x, Theme::of(cx).metrics.title_bar_height);
        self.open_menu(entries, below_bar, window, cx, |shell, action, _, _| {
            let Some(view) = shell.active_view() else {
                return;
            };
            match action {
                ReloadAction::Normal => view.reload(),
                ReloadAction::Hard => view.hard_reload(),
                ReloadAction::EmptyCache => view.clear_cache_and_reload(),
            }
        });
    }

    fn stop(&mut self, _: &ClickEvent, _: &mut Window, _: &mut Context<Self>) {
        if let Some(view) = self.active_view() {
            view.stop();
        }
    }
}

fn drag_area(id: &'static str) -> impl IntoElement {
    div()
        .id(id)
        .flex_1()
        .h_full()
        .window_control_area(WindowControlArea::Drag)
}

fn nav_button(
    id: &'static str,
    glyph: icons::Icon,
    enabled: bool,
    on_click: ClickHandler,
    tip: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::of(cx);
    let hover_bg = theme.colors.surface_hover;
    let color = if enabled {
        theme.colors.text_muted
    } else {
        theme.colors.text_faint
    };

    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(theme.metrics.title_button_size)
        .rounded(theme.metrics.radius_md)
        .font_family(theme.fonts.icons.clone())
        .text_size(theme.metrics.nav_glyph)
        .text_color(color)
        .when(enabled, |button| {
            button
                .hover(move |style| style.bg(hover_bg))
                .on_click(cx.listener(on_click))
                .tooltip(tip)
        })
        .child(glyph)
}
