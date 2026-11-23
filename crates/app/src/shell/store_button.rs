//! "Add to perseu" on Chrome Web Store listings, in place of the
//! store's "Add to Chrome", which refuses this browser, and the card that
//! confirms an extension before it installs.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use browser_core::extensions::Review;
use browser_core::tabs::TabId;
use gpui::{
    AnyElement, Context, FontWeight, Image, ImageFormat, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div, img,
    prelude::FluentBuilder,
};

use super::Shell;
use super::site_guard::notice_button;
use crate::icons;
use crate::theme::Theme;

const STORE_POLL: Duration = Duration::from_millis(400);

/// A downloaded store extension waiting for the user to add it.
pub(super) struct StoreOffer {
    id: String,
    folder: PathBuf,
    review: Review,
    icon: Option<Arc<Image>>,
    expanded: bool,
}

impl Shell {
    /// Keeps the button on tab `id` while it shows a store listing: "Add",
    /// "Adding…" or, once installed, "Remove".
    pub(super) fn watch_store_listing(&mut self, id: TabId, cx: &mut Context<Self>) {
        if !self.store_watch.insert(id) {
            return;
        }
        let clicked: Rc<RefCell<Option<String>>> = Rc::default();
        cx.spawn(async move |shell, cx| {
            loop {
                cx.background_executor().timer(STORE_POLL).await;
                let watching = shell.update(cx, |shell, cx| {
                    let listing = shell
                        .browser
                        .tab(id)
                        .and_then(|tab| browser_core::webstore::extension_id(&tab.url));
                    let Some(listing) = listing else {
                        shell.store_watch.remove(&id);
                        return false;
                    };
                    let installed = shell.store_installed(&listing);
                    if clicked.borrow_mut().take().as_deref() == Some(listing.as_str()) {
                        shell.in_main_window(cx, move |shell, window, cx| {
                            if installed {
                                shell.uninstall_extension(listing, window, cx);
                            } else {
                                shell.install_from_webstore(listing, window, cx);
                            }
                        });
                        return true;
                    }
                    let (kind, label) = if installed {
                        ("remove", "Remove from perseu")
                    } else if shell.store_adding.as_deref() == Some(listing.as_str()) {
                        ("adding", "Adding…")
                    } else {
                        ("add", "Add to perseu")
                    };
                    if let Some(view) = shell.runtime.get(&id).and_then(|r| r.view.clone()) {
                        let clicked = clicked.clone();
                        view.evaluate_string(
                            &browser_core::webstore::store_button_script(&listing, kind, label),
                            move |id| {
                                if !id.is_empty() {
                                    *clicked.borrow_mut() = Some(id);
                                }
                            },
                        );
                    }
                    true
                });
                if !matches!(watching, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Store installs land in a folder named after their store id.
    fn store_installed(&self, id: &str) -> bool {
        self.extensions
            .installed()
            .iter()
            .any(|item| item.folder == id)
    }

    /// Shows the card for a downloaded store package, bringing the sidebar
    /// out even when it hides itself.
    pub(super) fn offer_extension(
        &mut self,
        id: String,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match browser_core::extensions::review(&folder) {
            Ok(review) => {
                let icon = review
                    .icon
                    .as_ref()
                    .and_then(|path| std::fs::read(path).ok())
                    .map(|bytes| Arc::new(Image::from_bytes(ImageFormat::Png, bytes)));
                if let Some(previous) = self.store_offer.take() {
                    discard(&previous.folder);
                }
                self.store_offer = Some(StoreOffer {
                    id,
                    folder,
                    review,
                    icon,
                    expanded: false,
                });
                self.reveal_sidebar(cx);
                cx.notify();
            }
            Err(error) => {
                self.store_adding = None;
                discard(&folder);
                self.show_toast(
                    format!("Could not add the extension: {error:#}"),
                    window,
                    cx,
                );
            }
        }
    }

    fn answer_offer(&mut self, add: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.store_adding = None;
        let Some(offer) = self.store_offer.take() else {
            return;
        };
        if add {
            self.install_extension(offer.folder.clone(), window, cx);
        }
        discard(&offer.folder);
        cx.notify();
    }

    pub(super) fn render_store_offer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let offer = self.store_offer.as_ref()?;
        let theme = Theme::of(cx);
        let metrics = &theme.metrics;
        let manifest = &offer.review.manifest;
        let access = &offer.review.access;
        let shown = if offer.expanded {
            access.len()
        } else {
            access.len().min(1)
        };
        let more = access.len() - shown;
        let icon_size = metrics.essential_icon_size * 1.5;
        let icon: AnyElement = match &offer.icon {
            Some(image) => img(image.clone()).size(icon_size).into_any_element(),
            None => div()
                .flex()
                .items_center()
                .justify_center()
                .size(icon_size)
                .font_family(theme.fonts.icons.clone())
                .text_size(metrics.icon_glyph * 1.5)
                .child(icons::PUZZLE)
                .into_any_element(),
        };
        let muted = theme.colors.text_muted;
        let text = theme.colors.text;
        Some(
            div()
                .id(SharedString::from(format!("store-offer-{}", offer.id)))
                .absolute()
                .left(metrics.frame_inset)
                .right(metrics.frame_inset)
                .bottom(metrics.bottom_bar_height + metrics.radius_md)
                .flex()
                .flex_col()
                .gap(metrics.radius_md * 1.5)
                .p(metrics.radius_md * 2.)
                .rounded(metrics.radius_popover)
                .bg(theme.colors.surface_raised)
                .border_1()
                .border_color(theme.colors.border)
                .shadow_lg()
                .text_size(metrics.text_sm)
                .text_color(text)
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(metrics.radius_md * 1.5)
                        .child(icon)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .min_w_0()
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(metrics.text_md)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(manifest.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_color(muted)
                                        .child(format!("Version {}", manifest.version)),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(metrics.radius_sm)
                        .when(access.is_empty(), |list| {
                            list.child("Needs no special access")
                        })
                        .children(access.iter().take(shown).map(|line| div().child(*line)))
                        .when(more > 0, |list| {
                            list.child(
                                div()
                                    .id("store-offer-more")
                                    .text_color(muted)
                                    .hover(move |style| style.text_color(text))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(offer) = this.store_offer.as_mut() {
                                            offer.expanded = true;
                                        }
                                        cx.notify();
                                    }))
                                    .child(format!("and {more} more")),
                            )
                        }),
                )
                .child(
                    div()
                        .text_color(muted)
                        .child("Some features may not work in perseu"),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(metrics.radius_md)
                        .child(
                            notice_button("store-offer-cancel", "Cancel", false, cx)
                                .flex_1()
                                .justify_center()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.answer_offer(false, window, cx)
                                })),
                        )
                        .child(
                            notice_button("store-offer-add", "Add", true, cx)
                                .flex_1()
                                .justify_center()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.answer_offer(true, window, cx)
                                })),
                        ),
                )
                .into_any_element(),
        )
    }
}

fn discard(folder: &Path) {
    if let Err(error) = std::fs::remove_dir_all(folder) {
        tracing::warn!("could not delete {}: {error}", folder.display());
    }
}
