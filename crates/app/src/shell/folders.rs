use browser_core::tabs::Section;
use gpui::{
    Animation, AnimationExt as _, AnyElement, Context, DragMoveEvent, ElementId, Entity,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseUpEvent, ParentElement,
    Pixels, Point, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder, px,
};
use gpui_kit::component::input::{Input, InputEvent, InputState};

use super::tab_list::{DraggedTab, favicon};
use super::{ESCAPE_KEY, Shell};
use crate::context_menu::MenuEntry;
use crate::icons;
use crate::theme::Theme;

const FOLDER_PREVIEW: usize = 3;
const FOLDER_SLIDE: std::time::Duration = std::time::Duration::from_millis(180);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FolderAction {
    Rename,
    Delete,
}

impl Shell {
    fn folder_key(&self, name: &str) -> String {
        format!("{}:{name}", self.browser.space().id.value())
    }

    /// The pinned section: folders (with their tabs unless collapsed; a
    /// collapsed folder still shows its active tab), then loose pinned tabs.
    pub(super) fn render_pinned(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let tabs = &self.browser.space().tabs;
        let active = self.browser.active_tab_id();
        let mut rows = Vec::new();
        for folder in tabs.folders() {
            let header = self.render_folder_row(&folder, cx);
            let key = self.folder_key(&folder);
            let collapsed = self.collapsed_folders.contains(&key);
            let sliding = self
                .folder_toggled
                .get(&key)
                .is_some_and(|at| at.elapsed() < FOLDER_SLIDE);
            let scope = self.list_scope(&format!("folder:{folder}"));
            let row = f32::from(Theme::of(cx).metrics.row_height);
            let in_folder: Vec<_> = tabs
                .section(Section::Pinned)
                .filter(|member| member.folder() == Some(folder.as_str()))
                .collect();
            let closed_rows = in_folder
                .iter()
                .filter(|member| Some(member.id) == active)
                .count();
            let open_rows = in_folder.len();
            let members: Vec<AnyElement> = in_folder
                .into_iter()
                .filter(|member| !collapsed || sliding || Some(member.id) == active)
                .filter_map(|member| {
                    self.render_strip_entry(member, true, cx)
                        .map(|element| (member.id, element))
                })
                .enumerate()
                .map(|(index, (id, element))| {
                    self.animated_row(id, scope, index as f32 * row, element)
                })
                .collect();
            let theme = Theme::of(cx);
            rows.push(
                div()
                    .flex()
                    .flex_col()
                    .flex_shrink_0()
                    .my(theme.metrics.radius_sm / 2.)
                    .p(theme.metrics.radius_sm)
                    .rounded(theme.metrics.radius_md * 1.5)
                    .bg(theme.colors.surface_hover)
                    .child(header)
                    .when(!members.is_empty(), |card| {
                        let list = div()
                            .flex()
                            .flex_col()
                            .pl(theme.metrics.radius_md * 1.5)
                            .children(members);
                        if !sliding {
                            return card.child(list);
                        }
                        let (closed, open) = (closed_rows as f32 * row, open_rows as f32 * row);
                        let (from, to) = if collapsed {
                            (open, closed)
                        } else {
                            (closed, open)
                        };
                        let ease = crate::motion::curve(crate::motion::Easing::EaseOut);
                        card.child(list.overflow_hidden().with_animation(
                            ElementId::Name(format!("folder-slide-{key}-{collapsed}").into()),
                            Animation::new(FOLDER_SLIDE),
                            move |list, delta| list.h(px(from + (to - from) * ease(delta))),
                        ))
                    })
                    .into_any_element(),
            );
        }
        let loose = self.render_gap_list(Section::Pinned, cx);
        rows.push(
            div()
                .id("loose-pinned")
                .flex()
                .flex_col()
                .on_drag_move(
                    cx.listener(|this, event: &DragMoveEvent<DraggedTab>, _, cx| {
                        this.on_tab_drag_move(Section::Pinned, event, cx)
                    }),
                )
                .on_drop(cx.listener(|this, dragged: &DraggedTab, _, cx| {
                    this.drop_into_gap(dragged.id, Section::Pinned, cx)
                }))
                .children(loose)
                .into_any_element(),
        );
        rows
    }

    /// The inline name field shown while a folder is being renamed.
    pub(super) fn folder_rename_field(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex_1()
            .min_w(Theme::of(cx).metrics.favorite_max_width / 2.)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == ESCAPE_KEY {
                    this.stop_renaming_folder(window, cx);
                }
            }))
            .child(
                Input::new(&self.folder_name_input)
                    .appearance(false)
                    .px_0()
                    .aria_label("Folder name"),
            )
            .into_any_element()
    }

    /// Makes an empty folder and starts naming it.
    pub(super) fn create_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tabs = &mut self.browser.space_mut().tabs;
        let name = tabs.new_folder_name();
        tabs.add_folder(name.clone());
        self.start_folder_rename(name, window, cx);
    }

    /// Right-click menu for empty space in the sidebar or favorites bar.
    pub(super) fn open_empty_area_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = vec![
            MenuEntry::item("New folder", true),
            MenuEntry::item("New tab", false),
        ];
        self.open_menu(entries, position, window, cx, |this, folder, window, cx| {
            if folder {
                this.create_folder(window, cx);
            } else {
                this.new_tab(window, cx);
            }
        });
    }

    fn render_folder_row(&self, name: &str, cx: &mut Context<Self>) -> AnyElement {
        let owned = name.to_string();
        let label: AnyElement = if self.renaming_folder.as_deref() == Some(name) {
            self.folder_rename_field(cx)
        } else {
            div()
                .min_w_0()
                .truncate()
                .child(SharedString::from(owned.clone()))
                .into_any_element()
        };
        let theme = Theme::of(cx);
        let hover_bg = theme.colors.surface_hover;
        let drop_marker = self.palette(cx).accent;
        let collapsed = self.collapsed_folders.contains(&self.folder_key(name));
        let key = self.folder_key(name);
        let members: Vec<_> = self
            .browser
            .space()
            .tabs
            .section(Section::Pinned)
            .filter(|tab| tab.folder() == Some(name))
            .map(|tab| {
                self.runtime
                    .get(&tab.id)
                    .and_then(|runtime| runtime.favicon.clone())
            })
            .collect();
        let preview = collapsed.then(|| {
            div()
                .flex()
                .flex_row()
                .flex_shrink_0()
                .items_center()
                .gap(theme.metrics.radius_sm)
                .text_size(theme.metrics.text_sm)
                .text_color(theme.colors.text_muted)
                .children(
                    members
                        .iter()
                        .take(FOLDER_PREVIEW)
                        .map(|image| favicon(image.clone(), cx)),
                )
                .child(members.len().to_string())
        });
        let menu_name = owned.clone();
        let drop_name = owned.clone();
        div()
            .id(ElementId::Name(format!("folder-{name}").into()))
            .flex()
            .flex_row()
            .flex_shrink_0()
            .items_center()
            .gap(theme.metrics.radius_md)
            .h(theme.metrics.row_height)
            .px(theme.metrics.radius_md)
            .rounded(theme.metrics.radius_md)
            .text_color(theme.colors.text)
            .hover(move |style| style.bg(hover_bg))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.collapsed_folders.remove(&key) {
                    this.collapsed_folders.insert(key.clone());
                }
                this.folder_toggled
                    .insert(key.clone(), std::time::Instant::now());
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(FOLDER_SLIDE).await;
                    this.update(cx, |_, cx| cx.notify()).ok();
                })
                .detach();
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseUpEvent, window, cx| {
                    cx.stop_propagation();
                    this.open_folder_menu(menu_name.clone(), event.position, window, cx)
                }),
            )
            .drag_over::<DraggedTab>(move |style, _, _, _| style.bg(drop_marker.opacity(0.15)))
            .on_drop(cx.listener(move |this, dragged: &DraggedTab, _, cx| {
                this.drop_tab_at_end(dragged.id, Section::Pinned, cx);
                this.browser
                    .space_mut()
                    .tabs
                    .set_folder(dragged.id, Some(drop_name.clone()));
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .size(theme.metrics.favicon_size)
                    .text_size(theme.metrics.nav_glyph)
                    .child(icons::FOLDER),
            )
            .child(label)
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(theme.metrics.icon_glyph)
                    .text_color(theme.colors.text_muted)
                    .child(if collapsed {
                        icons::CHEVRON_DOWN
                    } else {
                        icons::CHEVRON_UP
                    }),
            )
            .child(div().flex_1())
            .children(preview)
            .into_any_element()
    }

    pub(super) fn open_folder_menu(
        &mut self,
        name: String,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = vec![
            MenuEntry::item("Rename folder", FolderAction::Rename),
            MenuEntry::Separator,
            MenuEntry::danger("Delete folder (keeps tabs)", FolderAction::Delete),
        ];
        self.open_menu(
            entries,
            position,
            window,
            cx,
            move |this, action, window, cx| match action {
                FolderAction::Rename => this.start_folder_rename(name.clone(), window, cx),
                FolderAction::Delete => {
                    this.browser.space_mut().tabs.remove_folder(&name);
                    cx.notify();
                }
            },
        );
    }

    pub(super) fn start_folder_rename(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = self.folder_key(&name);
        self.collapsed_folders.remove(&key);
        self.take_keyboard_from_pages();
        self.folder_name_input.update(cx, |input, cx| {
            input.set_value(name.clone(), window, cx);
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        self.renaming_folder = Some(name);
        cx.notify();
    }

    pub(super) fn on_folder_name_event(
        &mut self,
        _input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
            return;
        }
        let Some(from) = self.renaming_folder.clone() else {
            return;
        };
        let to = self.folder_name_input.read(cx).value().trim().to_string();
        if !to.is_empty() && to != from {
            self.browser.space_mut().tabs.rename_folder(&from, &to);
        }
        self.stop_renaming_folder(window, cx);
    }

    fn stop_renaming_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.renaming_folder.take().is_some() {
            window.focus(&self.focus_handle, cx);
            cx.notify();
        }
    }
}
