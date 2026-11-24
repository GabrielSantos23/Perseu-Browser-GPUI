use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use browser_core::tabs::TabId;
use futures::StreamExt;
use futures::channel::mpsc::unbounded;
use gpui::{
    Animation, AnimationExt, AnyElement, Context, Image, ImageFormat, InteractiveElement,
    IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div, img,
    prelude::FluentBuilder, px,
};
use webview_host::BrowserView;

use super::Shell;
use super::identify::Identify;
use super::tab_list::favicon;
use crate::icons;
use crate::media::{self, MediaCommand, MediaInfo};
use crate::page;
use crate::theme::Theme;
use crate::tooltip::{Side, tooltip, tooltip_on};

const EXPAND: Duration = Duration::from_millis(240);
const IDENTIFY: &str = "Identify";
const LISTENING: &str = "Listening\u{2026}";
const MISSED: &str = "No match";
/// Zen's floating notes: a 90-frame sprite played over 3 s (MPL-2.0).
const NOTE_SPRITE: &str = include_str!("../../assets/zen/note-indicator.svg");
const NOTE_FRAMES: usize = 90;
const NOTE_LOOP: Duration = Duration::from_secs(3);
const NOTE_BOX: f32 = 36.;
/// How far the notes' box reaches down into the card they rise from.
const NOTE_OVERLAP: f32 = 8.;
const NOTE_COLOR: &str = "#9a9aa6";
const CONTROL_ROW: f32 = 28.;
const SITE_ROW: f32 = 16.;
const TITLE_ROW: f32 = 22.;
const CONTROL_BUTTON: f32 = 24.;
/// How long the player stays after its tab goes quiet (a pause, a gap).
const QUIET_HOLD: Duration = Duration::from_secs(10);

pub(super) struct MediaState {
    info: Option<MediaInfo>,
    commands: Sender<MediaCommand>,
    hovered: bool,
    hover_changes: usize,
    pub(super) identify: Identify,
    /// The hidden page that signs audio for Shazam, kept once started.
    pub(super) signer: Option<Rc<BrowserView>>,
    /// The tab that last went quiet, and when.
    quiet: Option<(TabId, Instant)>,
}

impl MediaState {
    /// Starts watching what the app's pages play and feeds it to the shell.
    pub(super) fn start(window: &mut Window, cx: &mut Context<Shell>) -> Self {
        let (updates, mut receiver) = unbounded();
        let commands = media::watch(updates);
        cx.spawn_in(window, async move |shell, cx| {
            while let Some(info) = receiver.next().await {
                let delivered = shell.update(cx, |shell, cx| {
                    shell.media.info = info;
                    cx.notify();
                });
                if delivered.is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            info: None,
            commands,
            hovered: false,
            hover_changes: 0,
            identify: Identify::Idle,
            signer: None,
            quiet: None,
        }
    }
}

impl Shell {
    fn audible_tab(&self) -> Option<TabId> {
        self.runtime
            .iter()
            .find(|(_, runtime)| runtime.playing_audio)
            .map(|(id, _)| *id)
            .or_else(|| {
                self.runtime
                    .iter()
                    .find(|(_, runtime)| runtime.muted)
                    .map(|(id, _)| *id)
            })
            .or_else(|| {
                self.media
                    .quiet
                    .filter(|(id, since)| {
                        since.elapsed() < QUIET_HOLD && self.runtime.contains_key(id)
                    })
                    .map(|(id, _)| id)
            })
    }

    /// Keeps the player for [`QUIET_HOLD`] after a tab stops playing.
    pub(super) fn on_audio_changed(
        &mut self,
        id: TabId,
        was_playing: bool,
        playing: bool,
        cx: &mut Context<Self>,
    ) {
        if playing {
            self.media.quiet = None;
            return;
        }
        if !was_playing {
            return;
        }
        let since = Instant::now();
        self.media.quiet = Some((id, since));
        cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(QUIET_HOLD).await;
            shell
                .update(cx, |shell, cx| {
                    if shell.media.quiet == Some((id, since)) {
                        shell.media.quiet = None;
                        cx.notify();
                    }
                })
                .ok();
        })
        .detach();
    }

    fn send_media(&self, command: MediaCommand) {
        if self.media.commands.send(command).is_err() {
            tracing::warn!("media controls stopped");
        }
    }

    /// Now-playing card at the bottom of the sidebar: a row of controls
    /// that grows on hover into site, title and controls rows.
    pub(super) fn render_media_player(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let tab = self.audible_tab();
        let info = self
            .media
            .info
            .clone()
            .filter(|info| !info.title.is_empty());
        if tab.is_none() && info.as_ref().is_none_or(|info| !info.playing) {
            return None;
        }
        let tab_entry = tab.and_then(|id| self.browser.tab(id));
        let title: SharedString = match (&info, tab_entry) {
            (Some(info), _) => info.title.clone().into(),
            (None, Some(tab)) => page::display_title(tab),
            (None, None) => return None,
        };
        let host = tab_entry
            .map(|tab| page::host(&tab.url))
            .unwrap_or_default();
        let artist = info
            .as_ref()
            .map(|info| info.artist.clone())
            .filter(|artist| !artist.is_empty());
        let playing = info.as_ref().map_or(tab.is_some(), |info| info.playing);
        let runtime = tab.and_then(|id| self.runtime.get(&id));
        let muted = runtime.is_some_and(|runtime| runtime.muted);
        let image = runtime.and_then(|runtime| runtime.favicon.clone());
        let has_session = info.is_some();
        let found = match &self.media.identify {
            Identify::Found(song) => Some(song.clone()),
            _ => None,
        };
        // A found song keeps both rows open, naming it in place of the page.
        let expanded = self.media.hovered || found.is_some();
        let changes = self.media.hover_changes;
        let pinned = found.is_some();

        let identify_label = match self.media.identify {
            Identify::Listening => LISTENING,
            Identify::Missed => MISSED,
            Identify::Idle | Identify::Found(_) => IDENTIFY,
        };
        let identify = media_button("media-identify", cx)
            .on_click(cx.listener(|this, _, window, cx| this.identify_song(window, cx)))
            .tooltip(tooltip_on(Side::Left, "Name the song with Shazam"));
        let title_link = div()
            .id("media-title")
            .flex_1()
            .min_w_0()
            .truncate()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                if let Some(id) = tab {
                    this.activate(id, window, cx);
                }
            }));
        let title_link = match (&found, artist) {
            (Some(song), _) => {
                let url = song.url.clone();
                title_link
                    .tooltip(tooltip("Open in Shazam"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(url) = url.clone() {
                            cx.stop_propagation();
                            this.open_tab(url, browser_core::tabs::OpenPosition::End, window, cx);
                        }
                    }))
            }
            (None, Some(artist)) => title_link.tooltip(tooltip(artist)),
            (None, None) => title_link,
        };
        let (host, title) = match &found {
            Some(song) => (song.artist.clone().into(), song.title.clone().into()),
            None => (host, title),
        };
        let leading: AnyElement = if expanded {
            icon_button("media-pip", icons::PICTURE_IN_PICTURE, tab.is_some(), cx)
                .on_click(cx.listener(move |this, _, _, _| {
                    if let Some(view) = tab
                        .and_then(|id| this.runtime.get(&id))
                        .and_then(|runtime| runtime.view.clone())
                    {
                        view.toggle_picture_in_picture();
                    }
                }))
                .tooltip(tooltip("Picture in picture"))
                .into_any_element()
        } else {
            let size = px(CONTROL_BUTTON);
            div()
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .size(size)
                .child(favicon(image.clone(), cx))
                .into_any_element()
        };
        let play_glyph = if playing { icons::PAUSE } else { icons::PLAY };
        let previous = icon_button("media-previous", icons::PREVIOUS, has_session, cx)
            .on_click(cx.listener(|this, _, _, _| this.send_media(MediaCommand::Previous)))
            .tooltip(tooltip("Previous"));
        let play = icon_button("media-play", play_glyph, has_session, cx)
            .on_click(cx.listener(|this, _, _, _| this.send_media(MediaCommand::TogglePlay)))
            .tooltip(tooltip(if playing { "Pause" } else { "Play" }));
        let next = icon_button("media-next", icons::NEXT, has_session, cx)
            .on_click(cx.listener(|this, _, _, _| this.send_media(MediaCommand::Next)))
            .tooltip(tooltip("Next"));
        let mute = icon_button(
            "media-mute",
            if muted { icons::MUTE } else { icons::VOLUME },
            tab.is_some(),
            cx,
        )
        .on_click(cx.listener(move |this, _, _, _| {
            if let Some(view) = tab
                .and_then(|id| this.runtime.get(&id))
                .and_then(|runtime| runtime.view.clone())
            {
                view.set_muted(!muted);
            }
        }))
        .tooltip(tooltip_on(
            Side::Left,
            if muted { "Unmute" } else { "Mute" },
        ));
        let title_favicon = favicon(image, cx);

        let theme = Theme::of(cx);
        let metrics = &theme.metrics;
        let site_row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(metrics.radius_sm)
            .pl(metrics.radius_md * 2.)
            .overflow_hidden()
            .text_size(metrics.text_sm)
            .text_color(theme.colors.text_muted)
            .child(div().flex_1().min_w_0().truncate().child(host))
            .when(found.is_none(), |row| {
                row.child(
                    identify
                        .px(metrics.radius_sm)
                        .font_weight(gpui::FontWeight::BOLD)
                        .child(identify_label),
                )
            });
        let title_row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(metrics.radius_md)
            .overflow_hidden()
            .font_weight(gpui::FontWeight::BOLD)
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .size(px(CONTROL_BUTTON))
                    .child(title_favicon),
            )
            .child(title_link.child(title))
            .when(found.is_some(), |row| {
                row.child(
                    media_button("media-song-dismiss", cx)
                        .font_family(theme.fonts.icons.clone())
                        .text_size(metrics.icon_glyph * 0.8)
                        .text_color(theme.colors.text_muted)
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss_song(cx)))
                        .tooltip(tooltip_on(Side::Left, "Back to the player"))
                        .child(icons::CLOSE),
                )
            });
        let controls = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .h(px(CONTROL_ROW))
            .child(leading)
            .child(previous)
            .child(play)
            .child(next)
            .child(mute);

        let notes = (playing && !muted && !crate::motion::reduced()).then(|| {
            div()
                .absolute()
                .top(px(NOTE_OVERLAP - NOTE_BOX))
                .left(px(0.))
                .size(px(NOTE_BOX))
                .overflow_hidden()
                .with_animation(
                    "media-notes",
                    Animation::new(NOTE_LOOP).repeat(),
                    |notes, delta| {
                        let frame = ((delta * NOTE_FRAMES as f32) as usize).min(NOTE_FRAMES - 1);
                        notes.children(note_frame(frame).map(|frame| img(frame).size_full()))
                    },
                )
        });

        Some(
            div()
                .id("media-player")
                .relative()
                .flex()
                .flex_col()
                .flex_shrink_0()
                .mx(metrics.radius_sm)
                .mt(metrics.radius_md)
                .mb(metrics.radius_sm / 2.)
                .px(metrics.radius_sm)
                .rounded(metrics.radius_frame)
                .bg(theme.colors.surface)
                .border_1()
                .border_color(theme.colors.border)
                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                    if this.media.hovered != *hovered {
                        this.media.hovered = *hovered;
                        this.media.hover_changes += 1;
                        cx.notify();
                    }
                }))
                .children(notes)
                .child(if pinned {
                    site_row.h(px(SITE_ROW)).into_any_element()
                } else {
                    expandable(site_row, "media-site", expanded, changes, px(SITE_ROW))
                })
                .child(if pinned {
                    title_row.h(px(TITLE_ROW)).into_any_element()
                } else {
                    expandable(
                        title_row,
                        "media-title-row",
                        expanded,
                        changes,
                        px(TITLE_ROW),
                    )
                })
                .child(controls)
                .into_any_element(),
        )
    }
}

/// A row that grows to `row` tall while the card is hovered. Before the
/// first hover it is simply collapsed, without animating.
fn expandable(
    row_element: gpui::Div,
    id: &'static str,
    expanded: bool,
    changes: usize,
    row: gpui::Pixels,
) -> AnyElement {
    if changes == 0 {
        return row_element.h(px(0.)).opacity(0.).into_any_element();
    }
    row_element
        .with_animation(
            (id, changes),
            Animation::new(EXPAND)
                .with_easing(crate::motion::curve(crate::motion::Easing::EaseOut)),
            move |element, delta| {
                let shown = if expanded { delta } else { 1.0 - delta };
                element.h(row * shown).opacity(shown)
            },
        )
        .into_any_element()
}

fn media_button(id: &'static str, cx: &Context<Shell>) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::of(cx);
    let hover_bg = theme.colors.surface_active;
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .h(theme.metrics.nav_button_size - theme.metrics.radius_md)
        .min_w(theme.metrics.nav_button_size - theme.metrics.radius_md)
        .rounded(theme.metrics.radius_sm)
        .hover(move |style| style.bg(hover_bg))
}

fn icon_button(
    id: &'static str,
    glyph: icons::Icon,
    enabled: bool,
    cx: &Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::of(cx);
    let hover_bg = theme.colors.surface_active;
    let button = div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(CONTROL_BUTTON))
        .rounded(theme.metrics.radius_md)
        .font_family(theme.fonts.icons.clone())
        .text_size(theme.metrics.nav_glyph * 0.9)
        .child(glyph);
    if enabled {
        button
            .text_color(theme.colors.text)
            .hover(move |style| style.bg(hover_bg))
    } else {
        button.text_color(theme.colors.text_faint)
    }
}

thread_local! {
    static NOTE_IMAGES: Vec<Arc<Image>> = note_images();
}

fn note_frame(frame: usize) -> Option<Arc<Image>> {
    NOTE_IMAGES.with(|frames| frames.get(frame).cloned())
}

/// Cuts Zen's sprite sheet into one SVG per frame: the shared note shapes
/// and that frame's notes.
fn note_images() -> Vec<Arc<Image>> {
    note_svgs()
        .into_iter()
        .map(|svg| Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes())))
        .collect()
}

fn note_svgs() -> Vec<String> {
    let Some((head, frames)) = NOTE_SPRITE.split_once("</defs>") else {
        return Vec::new();
    };
    let Some(defs) = head.find("<defs>").map(|start| &head[start..]) else {
        return Vec::new();
    };
    frames
        .split("<svg x=\"")
        .skip(1)
        .filter_map(|frame| {
            let body = frame.split_once('>')?.1;
            let notes = body.split_once("</svg>")?.0;
            Some(format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"36\" height=\"36\" viewBox=\"0 0 36 36\" fill=\"{NOTE_COLOR}\">{defs}</defs>{notes}</svg>"
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_note_sprite_cuts_into_its_frames() {
        let frames = note_svgs();
        assert_eq!(frames.len(), NOTE_FRAMES);
        assert!(
            frames
                .iter()
                .all(|frame| frame.contains("<use") && frame.contains("zen-note-shape"))
        );
    }
}
