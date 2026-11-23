// Icons are from Uicons by Flaticon (https://www.flaticon.com/uicons),
// used under the Flaticon License, which asks for this credit.

use gpui::{
    AnyElement, App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, rgb,
};

use crate::theme::Theme;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Regular,
    Solid,
    System,
}

/// A glyph from the bundled icon fonts (regular or solid) or, for the window
/// buttons, the Windows system icon font.
#[derive(Clone, Copy, PartialEq, Eq, IntoElement)]
pub struct Icon {
    glyph: &'static str,
    family: Family,
}

impl Icon {
    const fn regular(glyph: &'static str) -> Self {
        Self {
            glyph,
            family: Family::Regular,
        }
    }

    const fn solid(glyph: &'static str) -> Self {
        Self {
            glyph,
            family: Family::Solid,
        }
    }

    const fn system(glyph: &'static str) -> Self {
        Self {
            glyph,
            family: Family::System,
        }
    }
}

impl RenderOnce for Icon {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let fonts = &Theme::of(cx).fonts;
        let family = match self.family {
            Family::Regular => fonts.icons.clone(),
            Family::Solid => fonts.icons_solid.clone(),
            Family::System => fonts.system_icons.clone(),
        };
        div()
            .font_family(family)
            .font_weight(gpui::FontWeight::NORMAL)
            .child(self.glyph)
    }
}

pub const MINIMIZE: Icon = Icon::system("\u{E921}");
pub const MAXIMIZE: Icon = Icon::system("\u{E922}");
pub const RESTORE: Icon = Icon::system("\u{E923}");
pub const WINDOW_CLOSE: Icon = Icon::system("\u{E8BB}");
pub const CLOSE: Icon = Icon::regular("\u{F4F6}");
pub const BACK: Icon = Icon::regular("\u{F197}");
pub const FORWARD: Icon = Icon::regular("\u{F19B}");
pub const RELOAD: Icon = Icon::regular("\u{FB6A}");
pub const STOP: Icon = Icon::regular("\u{F4F6}");
pub const GLOBE: Icon = Icon::regular("\u{F705}");
pub const ADD: Icon = Icon::regular("\u{FAAC}");
pub const PIN: Icon = Icon::regular("\u{FD95}");
pub const COMMAND: Icon = Icon::regular("\u{F498}");
pub const SEARCH: Icon = Icon::regular("\u{FBBA}");
pub const UNSPLIT: Icon = Icon::regular("\u{FC91}");
pub const OPEN_IN_TAB: Icon = Icon::regular("\u{F1B3}");
pub const SETTINGS: Icon = Icon::regular("\u{FBD2}");
pub const DOWNLOAD: Icon = Icon::regular("\u{F591}");
pub const FOLDER: Icon = Icon::regular("\u{F6B5}");
pub const LOCK: Icon = Icon::regular("\u{F8D5}");
pub const WARNING: Icon = Icon::solid("\u{FDEB}");
pub const CHECK: Icon = Icon::regular("\u{F3C8}");
pub const VOLUME: Icon = Icon::regular("\u{FED0}");
pub const MUTE: Icon = Icon::regular("\u{FECF}");
pub const MINUS: Icon = Icon::regular("\u{F94E}");
pub const SLIDERS: Icon = Icon::regular("\u{FBD1}");
pub const PUZZLE: Icon = Icon::regular("\u{FAE9}");
pub const LINK: Icon = Icon::regular("\u{F8B5}");
pub const CHEVRON_DOWN: Icon = Icon::regular("\u{F153}");
pub const SIDEBAR: Icon = Icon::regular("\u{FC08}");
pub const CHEVRON_UP: Icon = Icon::regular("\u{F156}");
pub const ZOOM_IN: Icon = Icon::regular("\u{FF2D}");
pub const ZOOM_OUT: Icon = Icon::regular("\u{FF2E}");
pub const APPS: Icon = Icon::regular("\u{F16C}");
pub const PLAY: Icon = Icon::solid("\u{FA7F}");
pub const PAUSE: Icon = Icon::solid("\u{F9FC}");
pub const PREVIOUS: Icon = Icon::solid("\u{FCC9}");
pub const NEXT: Icon = Icon::solid("\u{FCCA}");
pub const PICTURE_IN_PICTURE: Icon = Icon::regular("\u{FF0C}");
pub const CHEVRON_LEFT: Icon = Icon::regular("\u{F154}");
pub const CHEVRON_RIGHT: Icon = Icon::regular("\u{F155}");
pub const HARMONY: Icon = Icon::regular("\u{F411}");
pub const SCHEME_AUTO: Icon = Icon::solid("\u{FC5D}");
pub const SCHEME_LIGHT: Icon = Icon::solid("\u{FCEF}");
pub const SCHEME_DARK: Icon = Icon::solid("\u{F962}");

pub const PALETTE: Icon = Icon::regular("\u{F9ED}");
pub const INCOGNITO: Icon = Icon::regular("\u{F804}");
pub const HISTORY: Icon = Icon::regular("\u{FDAA}");
pub const TRASH: Icon = Icon::regular("\u{FE17}");
pub const ARROW_BACK: Icon = Icon::regular("\u{F19D}");
pub const KEYBOARD: Icon = Icon::regular("\u{F84C}");
pub const SHIELD: Icon = Icon::regular("\u{FBE8}");
pub const INFO: Icon = Icon::regular("\u{F80B}");
pub const BROWSERS: Icon = Icon::regular("\u{F2F6}");

/// Icons offered for spaces, by name, as shown in the space icon picker.
pub const SPACE_ICONS: &[(&str, Icon)] = &[
    ("incognito", INCOGNITO),
    ("home", Icon::solid("\u{F7AD}")),
    ("briefcase", Icon::solid("\u{F2E7}")),
    ("rocket", Icon::solid("\u{FB41}")),
    ("flask", Icon::solid("\u{F68E}")),
    ("book-alt", Icon::solid("\u{F272}")),
    ("gamepad", Icon::solid("\u{F6D2}")),
    ("plane-alt", Icon::solid("\u{FA6B}")),
    ("music-alt", Icon::solid("\u{F985}")),
    ("shopping-cart", Icon::solid("\u{FBD6}")),
    ("heart", Icon::solid("\u{F794}")),
    ("star", Icon::solid("\u{FCC1}")),
    ("palette", Icon::solid("\u{F9DC}")),
    ("camera", Icon::solid("\u{F345}")),
    ("coffee", Icon::solid("\u{F487}")),
    ("graduation-cap", Icon::solid("\u{F704}")),
    ("leaf", Icon::solid("\u{F87F}")),
    ("fire-flame-curved", Icon::solid("\u{F677}")),
    ("moon", Icon::solid("\u{F962}")),
    ("sun", Icon::solid("\u{FCEF}")),
    ("bolt", Icon::solid("\u{F26A}")),
    ("film", Icon::solid("\u{F66D}")),
    ("gift", Icon::solid("\u{F6E9}")),
    ("paw", Icon::solid("\u{F9FF}")),
    ("wallet", Icon::solid("\u{FEB4}")),
    ("user", Icon::solid("\u{FE78}")),
    ("envelope", Icon::solid("\u{F5D1}")),
    ("calendar", Icon::solid("\u{F336}")),
    ("bookmark", Icon::solid("\u{F289}")),
    ("flag", Icon::solid("\u{F688}")),
    ("bulb", Icon::solid("\u{F301}")),
    ("terminal", Icon::solid("\u{FD4E}")),
    ("pencil", Icon::solid("\u{FA17}")),
    ("headphones", Icon::solid("\u{F787}")),
    ("globe", Icon::solid("\u{F6F9}")),
    ("ghost", Icon::solid("\u{F6E4}")),
    ("crown", Icon::solid("\u{F4EF}")),
    ("gem", Icon::solid("\u{F6E2}")),
    ("pizza-slice", Icon::solid("\u{FA67}")),
    ("basketball", Icon::solid("\u{F21D}")),
    ("mountain", Icon::solid("\u{F96E}")),
    ("umbrella", Icon::solid("\u{FE3F}")),
    ("cat", Icon::solid("\u{F388}")),
    ("dog", Icon::solid("\u{F570}")),
    ("code-branch", Icon::solid("\u{F47A}")),
    ("chart-histogram", Icon::solid("\u{F39F}")),
    ("cloud", Icon::solid("\u{F471}")),
    ("cube", Icon::solid("\u{F4F6}")),
    ("map-marker", Icon::solid("\u{F8EA}")),
];

/// A space's icon: one of [`SPACE_ICONS`] by name, an emoji saved by an older
/// version, or a dot for "no icon".
pub fn space_icon_view(name: &str, color: Option<u32>) -> AnyElement {
    let view = div().flex().items_center().justify_center();
    let view = match color {
        Some(color) => view.text_color(rgb(color)),
        None => view,
    };
    match space_icon(name) {
        Some(icon) => view.child(icon),
        None if name.is_empty() => view.child(NO_ICON),
        None => view.child(SharedString::from(name.to_string())),
    }
    .into_any_element()
}

const NO_ICON: &str = "\u{2022}";

/// The space icon with this name, if it is one of [`SPACE_ICONS`].
pub fn space_icon(name: &str) -> Option<Icon> {
    SPACE_ICONS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, icon)| *icon)
}
