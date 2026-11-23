use browser_core::omnibox::{BLANK_PAGE, display_host};
use browser_core::security::registrable_domain;
use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement, IntoElement, ParentElement, Pixels,
    SharedString, Size, StatefulInteractiveElement, Styled, div, prelude::FluentBuilder,
};
use webview_host::SecurityState;

use super::tab_list::favicon;
use super::{Shell, URL_PLACEHOLDER};
use crate::actions;
use crate::icons;
use crate::page;
use crate::site_menu::SiteMenuEvent;
use crate::theme::Theme;
use crate::title_bar::TitleBar;
use crate::tooltip::{Side, tooltip, tooltip_with_keys};

impl Shell {
    /// The page's address in the middle of the title bar: copy link, the
    /// site (click to type an address) and the site menu.
    pub(super) fn render_site_title(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let hover_bg = theme.colors.surface_hover;
        let url = self.active_url();
        let address = div()
            .id("site-title")
            .flex()
            .flex_row()
            .items_center()
            .min_w_0()
            .gap(theme.metrics.radius_md)
            .h(theme.metrics.title_button_size)
            .px(theme.metrics.radius_md)
            .rounded(theme.metrics.radius_md)
            .cursor_pointer()
            .hover(move |style| style.bg(hover_bg))
            .on_click(cx.listener(|this, _, window, cx| this.edit_current_address(window, cx)))
            .tooltip(tooltip_with_keys(
                Side::Right,
                "Edit address",
                actions::FocusAddressBar,
            ));

        let in_sidebar = self.settings.site_title_in_sidebar;
        let address = address
            .occlude()
            .when(in_sidebar, |address| address.flex_1());
        if url.is_empty() || url.eq_ignore_ascii_case(BLANK_PAGE) {
            return address
                .text_color(theme.colors.text_muted)
                .child(div().min_w_0().truncate().child(URL_PLACEHOLDER))
                .into_any_element();
        }
        if page::is_blank(url) {
            return address
                .max_w(theme.metrics.title_center_max_width)
                .text_color(theme.colors.text_muted)
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .child(SharedString::from(url.to_string())),
                )
                .into_any_element();
        }

        let runtime = self.active_runtime();
        let insecure = url
            .get(..7)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http://"))
            || runtime.and_then(|runtime| runtime.security) == Some(SecurityState::Insecure);
        let host = display_host(url);
        let domain = registrable_domain(host).unwrap_or(host);
        let prefix = &host[..host.len() - domain.len()];
        let address = address
            .child(favicon(
                runtime.and_then(|runtime| runtime.favicon.clone()),
                cx,
            ))
            .children(insecure.then(|| {
                div()
                    .flex_shrink_0()
                    .font_family(theme.fonts.icons.clone())
                    .text_size(theme.metrics.icon_glyph)
                    .text_color(theme.colors.warning)
                    .child(icons::WARNING)
            }))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(
                        div()
                            .flex_shrink(1.)
                            .min_w_0()
                            .truncate()
                            .text_color(theme.colors.text_muted)
                            .child(SharedString::from(prefix.to_string())),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_color(theme.colors.text)
                            .child(SharedString::from(domain.to_string())),
                    ),
            );

        let copy_url = url.to_string();
        div()
            .occlude()
            .flex()
            .flex_row()
            .items_center()
            .min_w_0()
            .map(|title| {
                if in_sidebar {
                    title.flex_1()
                } else {
                    title.max_w(theme.metrics.title_center_max_width)
                }
            })
            .gap(theme.metrics.radius_sm)
            .child(
                title_button("copy-link", icons::LINK, cx)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.copy_link(copy_url.clone(), window, cx)
                    }))
                    .tooltip(tooltip_with_keys(
                        Side::Right,
                        "Copy link",
                        actions::CopyUrl,
                    )),
            )
            .child(address)
            .children(self.render_zoom_badge(cx))
            .children(self.render_store_badge(cx))
            .children(self.has_extension_menus().then(|| {
                title_button("extensions-button", icons::PUZZLE, cx)
                    .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                        this.open_extensions_menu(event.position(), window, cx)
                    }))
                    .tooltip(tooltip("Extensions"))
            }))
            .child(
                title_button("site-menu-button", icons::SLIDERS, cx)
                    .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                        this.open_site_menu(event.position(), window, cx)
                    }))
                    .tooltip(tooltip("Site settings")),
            )
            .into_any_element()
    }

    /// The website name under the sidebar's top row, when Settings puts it
    /// in the sidebar instead of the title bar.
    pub(super) fn render_sidebar_site_title(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.settings.site_title_in_sidebar {
            return None;
        }
        let title = self.render_site_title(cx);
        let theme = Theme::of(cx);
        Some(
            div()
                .flex()
                .flex_row()
                .flex_shrink_0()
                .items_center()
                .h(theme.metrics.url_field_height)
                .px(theme.metrics.radius_sm)
                .rounded(theme.metrics.radius_md)
                .bg(theme.colors.surface_hover)
                .child(div().flex().flex_1().min_w_0().child(title))
                .into_any_element(),
        )
    }

    /// The hidden title bar, sliding down over the page while revealed. Its
    /// background is the window's own background, offset to line up, so it
    /// blends with the sidebar beside it. Below its bottom corners it draws
    /// the page's rounded top corners, so the page looks tucked under it.
    pub(super) fn render_title_overlay(
        &self,
        title_bar: TitleBar,
        favorites_bar: Option<AnyElement>,
        viewport: Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let fraction = self.title_reveal_fraction();
        if fraction <= 0.0 {
            return None;
        }
        let height = self.title_overlay_height(cx);
        let theme = Theme::of(cx);
        let radius = theme.metrics.radius_frame;
        let inset = theme.metrics.frame_inset;
        let surface = theme.colors.surface;
        let left = self.title_overlay_left();
        let top = -height * (1.0 - fraction);
        let palette = self.palette(cx);
        let page_left = if self.compact { inset } else { Pixels::ZERO };
        let backdrop = |x: Pixels, y: Pixels| {
            div()
                .absolute()
                .left(-x)
                .top(-y)
                .w(viewport.width)
                .h(viewport.height)
                .bg(palette.sidebar)
                .children(self.space_backdrop(cx))
        };
        let corner = |x: Pixels, from_right: bool| {
            let circle = div()
                .absolute()
                .top_0()
                .size(radius * 2.)
                .rounded_full()
                .bg(surface)
                .border_1()
                .border_color(palette.frame_border);
            let circle = if from_right {
                circle.left(-radius)
            } else {
                circle.left_0()
            };
            div()
                .absolute()
                .top(height)
                .size(radius)
                .overflow_hidden()
                .child(backdrop(x, top + height))
                .child(circle)
        };
        let right_corner_x = left + (viewport.width - left - inset - radius);
        Some(
            div()
                .absolute()
                .left(left)
                .right_0()
                .top(top)
                .h(height)
                .child(
                    div()
                        .absolute()
                        .size_full()
                        .overflow_hidden()
                        .child(backdrop(left, top)),
                )
                .child(title_bar)
                .children(favorites_bar)
                .child(corner(left + page_left, false).left(page_left))
                .child(corner(right_corner_x, true).right(inset))
                .into_any_element(),
        )
    }
}

impl Shell {
    /// "Add extension" on a Chrome Web Store extension page: the store's own
    /// button only works in Chrome.
    fn render_store_badge(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = browser_core::webstore::extension_id(self.active_url())?;
        let theme = Theme::of(cx);
        let accent = self.palette(cx).accent;
        Some(
            div()
                .id("store-badge")
                .flex()
                .flex_row()
                .flex_shrink_0()
                .items_center()
                .gap(theme.metrics.radius_sm)
                .h(theme.metrics.title_button_size)
                .px(theme.metrics.radius_md)
                .rounded(theme.metrics.radius_md)
                .bg(accent.opacity(0.18))
                .hover(move |style| style.bg(accent.opacity(0.3)))
                .text_size(theme.metrics.text_sm)
                .text_color(theme.colors.text)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.install_from_webstore(id.clone(), window, cx)
                }))
                .tooltip(tooltip("Download and add this extension"))
                .child(
                    div()
                        .text_size(theme.metrics.icon_glyph)
                        .child(icons::PUZZLE),
                )
                .child("Add extension")
                .into_any_element(),
        )
    }

    /// The page zoom, shown beside the address while it isn't 100%; a click
    /// resets it.
    fn render_zoom_badge(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let factor = self.active_zoom();
        if (factor - 1.0).abs() < 0.001 {
            return None;
        }
        let theme = Theme::of(cx);
        let hover_bg = theme.colors.surface_active;
        let glyph = if factor > 1.0 {
            icons::ZOOM_IN
        } else {
            icons::ZOOM_OUT
        };
        Some(
            div()
                .id("zoom-badge")
                .flex()
                .flex_row()
                .flex_shrink_0()
                .items_center()
                .gap(theme.metrics.radius_sm)
                .h(theme.metrics.title_button_size)
                .px(theme.metrics.radius_md)
                .rounded(theme.metrics.radius_md)
                .bg(theme.colors.surface_hover)
                .hover(move |style| style.bg(hover_bg))
                .text_size(theme.metrics.text_sm)
                .text_color(theme.colors.text_muted)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.on_site_menu_event(&SiteMenuEvent::ZoomReset, window, cx)
                }))
                .tooltip(tooltip("Reset zoom"))
                .child(div().text_size(theme.metrics.icon_glyph).child(glyph))
                .child(format!("{}%", browser_core::zoom::percent(factor)))
                .into_any_element(),
        )
    }
}

fn title_button(
    id: &'static str,
    glyph: icons::Icon,
    cx: &Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::of(cx);
    let hover_bg = theme.colors.surface_hover;
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(theme.metrics.title_button_size)
        .rounded(theme.metrics.radius_md)
        .font_family(theme.fonts.icons.clone())
        .text_size(theme.metrics.icon_glyph)
        .text_color(theme.colors.text_muted)
        .hover(move |style| style.bg(hover_bg))
        .child(glyph)
}
