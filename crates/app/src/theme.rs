use browser_core::browser::SpaceTheme;
use browser_core::settings::DEFAULT_ACCENT;
use gpui::{
    App, Background, Global, Hsla, Pixels, SharedString, linear_color_stop, linear_gradient, px,
    rgb, rgba,
};
use gpui_kit::component::theme::ThemeMode;

pub struct Theme {
    pub colors: Colors,
    pub metrics: Metrics,
    pub fonts: Fonts,
}

pub struct Colors {
    /// Dark text on light surfaces, chosen when the space colors are bright.
    pub light: bool,
    pub canvas: Hsla,
    pub surface: Hsla,
    pub surface_raised: Hsla,
    pub surface_hover: Hsla,
    pub surface_active: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_faint: Hsla,
    pub accent: Hsla,
    pub danger: Hsla,
    pub warning: Hsla,
    pub success: Hsla,
    pub shadow: Hsla,
    pub scrim: Hsla,
}

pub struct SpacePalette {
    pub accent: Hsla,
    pub sidebar: Background,
    pub overlay: Option<Background>,
    pub frame_border: Hsla,
}

pub struct Metrics {
    pub title_bar_height: Pixels,
    pub title_button_size: Pixels,
    pub favorites_bar_height: Pixels,
    pub window_control_width: Pixels,
    pub sidebar_width: Pixels,
    pub sidebar_min_width: Pixels,
    pub sidebar_max_width: Pixels,
    pub resize_handle_width: Pixels,
    pub frame_inset: Pixels,
    pub url_field_height: Pixels,
    pub row_height: Pixels,
    pub favicon_size: Pixels,
    pub nav_button_size: Pixels,
    pub nav_glyph: Pixels,
    pub loading_bar_height: Pixels,
    pub essential_tile_height: Pixels,
    pub essential_icon_size: Pixels,
    pub essentials_per_row: u16,
    pub bottom_bar_height: Pixels,
    pub space_slide_distance: Pixels,
    pub command_bar_text: Pixels,
    pub command_bar_min_width: Pixels,
    pub command_bar_max_width: Pixels,
    pub command_bar_height: Pixels,
    pub command_bar_top_offset: Pixels,
    pub menu_width: Pixels,
    pub site_menu_width: Pixels,
    pub favorite_max_width: Pixels,
    pub title_center_inset: Pixels,
    pub title_center_max_width: Pixels,
    pub toast_width: Pixels,
    pub toast_height: Pixels,
    pub radius_sm: Pixels,
    pub radius_md: Pixels,
    pub radius_frame: Pixels,
    pub radius_popover: Pixels,
    pub radius_peek: Pixels,
    pub peek_margin: Pixels,
    pub text_sm: Pixels,
    pub text_md: Pixels,
    pub text_title: Pixels,
    pub icon_glyph: Pixels,
}

pub struct Fonts {
    pub ui: SharedString,
    pub ui_weight: gpui::FontWeight,
    pub icons: SharedString,
    pub icons_solid: SharedString,
    pub system_icons: SharedString,
}

impl Global for Theme {}

const MIN_LAYER_ALPHA: f32 = 0.2;
const MAX_LAYER_ALPHA: f32 = 0.88;
const BASE_GRADIENT_ANGLE: f32 = 122.;
const BASE_GRADIENT_END: f32 = 0.75;
const OVERLAY_ANGLE: f32 = 180.;
const OVERLAY_MAX_ALPHA: f32 = 0.9;
const FRAME_BORDER_STRENGTH: f32 = 0.45;
const SELECTION_STRENGTH: f32 = 0.4;
pub const ACCENT_PRESETS: [u32; 8] = [
    DEFAULT_ACCENT,
    0x4CC3B5,
    0x6BBF59,
    0xE8B04B,
    0xF28C6B,
    0xEF6F8E,
    0xC58CFF,
    0x5AA9F0,
];
const DARK_CANVAS: u32 = 0x17171CFF;
const OVERLAY_AVERAGE: f32 = OVERLAY_MAX_ALPHA / 2.;

/// A theme color laid over the dark window color at the alpha `intensity`
/// gives; always the dark one, so the space looks the same in both schemes.
fn layer(color: u32, intensity: f32) -> Hsla {
    let strength = intensity.clamp(0.0, 1.0);
    let alpha = MIN_LAYER_ALPHA + (MAX_LAYER_ALPHA - MIN_LAYER_ALPHA) * strength * strength;
    tint(hex(DARK_CANVAS), rgb(color).into(), alpha)
}

/// The sidebar's color averaged over its area.
fn average_background(space: SpaceTheme, intensity: f32) -> Hsla {
    match space {
        SpaceTheme::Solid(color) => layer(color, intensity),
        SpaceTheme::Gradient(start, end) => mix(layer(start, intensity), layer(end, intensity)),
        SpaceTheme::Triple(start, end, third) => {
            mix(layer(start, intensity), layer(end, intensity))
                .blend(layer(third, intensity).opacity(OVERLAY_AVERAGE))
        }
    }
}

const ICONS_REGULAR: &str = "uicons-regular-rounded";
const ICONS_SOLID: &str = "uicons-solid-rounded";
const FLUENT_ICONS: &str = "Segoe Fluent Icons";
const MDL2_ICONS: &str = "Segoe MDL2 Assets";

fn hex(value: u32) -> Hsla {
    rgba(value).into()
}

pub fn to_rgba8(color: Hsla) -> (u8, u8, u8, u8) {
    let rgba = gpui::Rgba::from(color);
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
    (byte(rgba.r), byte(rgba.g), byte(rgba.b), byte(rgba.a))
}

pub fn tint(base: Hsla, accent: Hsla, strength: f32) -> Hsla {
    base.blend(accent.opacity(strength))
}

impl Colors {
    pub fn dark(accent: Hsla) -> Self {
        Self {
            light: false,
            canvas: hex(DARK_CANVAS),
            surface: hex(0x1F1F26FF),
            surface_raised: hex(0x272730FF),
            surface_hover: hex(0xFFFFFF0F),
            surface_active: hex(0xFFFFFF1F),
            border: hex(0xFFFFFF14),
            text: hex(0xECECF1FF),
            text_muted: hex(0x9A9AA6FF),
            text_faint: hex(0x6B6B76FF),
            accent,
            danger: hex(0xFF6B6BFF),
            warning: hex(0xF5B84BFF),
            success: hex(0x4CC38AFF),
            shadow: hex(0x00000066),
            scrim: hex(0x0000008C),
        }
    }

    pub fn light(accent: Hsla) -> Self {
        Self {
            light: true,
            canvas: hex(0xF3F3F6FF),
            surface: hex(0xFFFFFFFF),
            surface_raised: hex(0xF8F8FBFF),
            surface_hover: hex(0x00000012),
            surface_active: hex(0x00000021),
            border: hex(0x0000001A),
            text: hex(0x17171CFF),
            text_muted: hex(0x55555FFF),
            text_faint: hex(0x80808AFF),
            accent,
            danger: hex(0xC92A2AFF),
            warning: hex(0x9A6200FF),
            success: hex(0x1B7F4EFF),
            shadow: hex(0x00000033),
            scrim: hex(0x00000066),
        }
    }
}

/// Whether dark text reads better than light text on `background`, by WCAG
/// contrast ratio.
fn wants_dark_text(background: Hsla) -> bool {
    let rgba = gpui::Rgba::from(background);
    let linear = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * linear(rgba.r) + 0.7152 * linear(rgba.g) + 0.0722 * linear(rgba.b);
    (luminance + 0.05) / 0.05 > 1.05 / (luminance + 0.05)
}

fn mix(a: Hsla, b: Hsla) -> Hsla {
    let (a, b) = (gpui::Rgba::from(a), gpui::Rgba::from(b));
    gpui::Rgba {
        r: (a.r + b.r) / 2.,
        g: (a.g + b.g) / 2.,
        b: (a.b + b.b) / 2.,
        a: 1.,
    }
    .into()
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            title_bar_height: px(30.),
            title_button_size: px(24.),
            favorites_bar_height: px(28.),
            window_control_width: px(46.),
            sidebar_width: px(260.),
            sidebar_min_width: px(200.),
            sidebar_max_width: px(480.),
            resize_handle_width: px(6.),
            frame_inset: px(8.),
            url_field_height: px(34.),
            row_height: px(32.),
            favicon_size: px(16.),
            nav_button_size: px(28.),
            nav_glyph: px(12.),
            loading_bar_height: px(2.),
            essential_tile_height: px(44.),
            essential_icon_size: px(20.),
            essentials_per_row: 4,
            bottom_bar_height: px(40.),
            space_slide_distance: px(24.),
            command_bar_text: px(16.),
            command_bar_min_width: px(420.),
            command_bar_max_width: px(680.),
            command_bar_height: px(520.),
            command_bar_top_offset: px(56.),
            menu_width: px(240.),
            site_menu_width: px(280.),
            favorite_max_width: px(160.),
            title_center_inset: px(150.),
            title_center_max_width: px(480.),
            toast_width: px(220.),
            toast_height: px(40.),
            radius_sm: px(4.),
            radius_md: px(8.),
            radius_frame: px(10.),
            radius_popover: px(12.),
            radius_peek: px(14.),
            peek_margin: px(40.),
            text_sm: px(11.),
            text_md: px(13.),
            text_title: px(18.),
            icon_glyph: px(10.),
        }
    }
}

impl Fonts {
    fn detect(cx: &App) -> Self {
        let bundled = vec![
            std::borrow::Cow::Borrowed(
                &include_bytes!("../assets/fonts/uicons-regular-rounded.ttf")[..],
            ),
            std::borrow::Cow::Borrowed(
                &include_bytes!("../assets/fonts/uicons-solid-rounded.ttf")[..],
            ),
        ];
        if let Err(error) = cx.text_system().add_fonts(bundled) {
            tracing::error!("failed to load the icon fonts: {error:#}");
        }
        let system_icons = if cx
            .text_system()
            .all_font_names()
            .iter()
            .any(|name| name == FLUENT_ICONS)
        {
            FLUENT_ICONS
        } else {
            MDL2_ICONS
        };
        Self {
            ui: "Segoe UI".into(),
            ui_weight: gpui::FontWeight::SEMIBOLD,
            system_icons: system_icons.into(),
            icons: ICONS_REGULAR.into(),
            icons_solid: ICONS_SOLID.into(),
        }
    }
}

pub fn apply_to_components(cx: &mut App) {
    let colors = &Theme::of(cx).colors;
    let (accent, text, muted, border) =
        (colors.accent, colors.text, colors.text_muted, colors.border);
    let (surface, raised, hover, active) = (
        colors.surface,
        colors.surface_raised,
        colors.surface_hover,
        colors.surface_active,
    );
    let on_accent = if wants_dark_text(accent) {
        hex(0x17171CFF)
    } else {
        hex(0xFFFFFFFF)
    };
    gpui_kit::component::Theme::update(cx, |kit| {
        kit.selection = accent.opacity(SELECTION_STRENGTH);
        kit.primary = accent;
        kit.primary_hover = accent.opacity(0.9);
        kit.primary_active = accent.opacity(0.8);
        kit.primary_foreground = on_accent;
        kit.ring = accent;
        kit.foreground = text;
        kit.muted_foreground = muted;
        kit.border = border;
        kit.input = border;
        kit.background = surface;
        kit.popover = raised;
        kit.popover_foreground = text;
        kit.secondary = hover;
        kit.secondary_hover = active;
        kit.secondary_active = active;
        kit.secondary_foreground = text;
        kit.accent = hover;
        kit.accent_foreground = text;
        kit.switch = active;
        kit.switch_thumb = hex(0xFFFFFFFF);
    });
}

impl Theme {
    pub fn init(cx: &mut App) {
        let theme = Self {
            colors: Colors::dark(rgb(DEFAULT_ACCENT).into()),
            metrics: Metrics::default(),
            fonts: Fonts::detect(cx),
        };
        cx.set_global(theme);
    }

    pub fn of(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    /// Switches between the dark and light colors so text and icons keep
    /// contrast with the space's sidebar.
    pub fn sync_scheme(space: SpaceTheme, intensity: f32, cx: &mut App) {
        let light = wants_dark_text(average_background(space, intensity));
        if Self::of(cx).colors.light == light {
            return;
        }
        let accent = Self::of(cx).colors.accent;
        let (colors, mode) = if light {
            (Colors::light(accent), ThemeMode::Light)
        } else {
            (Colors::dark(accent), ThemeMode::Dark)
        };
        cx.global_mut::<Self>().colors = colors;
        gpui_kit::component::Theme::change(mode, None, cx);
        apply_to_components(cx);
        cx.refresh_windows();
    }

    pub fn set_accent(accent: u32, cx: &mut App) {
        cx.global_mut::<Self>().colors.accent = rgb(accent).into();
        apply_to_components(cx);
        cx.refresh_windows();
    }

    /// Colors for a space. `intensity` (0 to 1, 0.5 by default) scales the
    /// sidebar tint; `angle` is the gradient direction in degrees.
    /// Colors for a space, layered the way Zen does: each theme color sits
    /// over the window color at an alpha that grows with the square of
    /// `intensity` (0 to 1). A third color fades in from the top as an overlay.
    pub fn space_palette(&self, space: SpaceTheme, intensity: f32) -> SpacePalette {
        let accent: Hsla = rgb(space.accent()).into();
        let layer = |color: u32| layer(color, intensity);
        let base = |start: u32, end: u32| {
            linear_gradient(
                BASE_GRADIENT_ANGLE,
                linear_color_stop(layer(start), 0.),
                linear_color_stop(layer(end), BASE_GRADIENT_END),
            )
        };
        let (sidebar, overlay) = match space {
            SpaceTheme::Solid(color) => (layer(color).into(), None),
            SpaceTheme::Gradient(start, end) => (base(start, end), None),
            SpaceTheme::Triple(start, end, third) => (
                base(start, end),
                Some(linear_gradient(
                    OVERLAY_ANGLE,
                    linear_color_stop(layer(third).opacity(0.), 0.),
                    linear_color_stop(layer(third).opacity(OVERLAY_MAX_ALPHA), 1.),
                )),
            ),
        };
        SpacePalette {
            accent,
            sidebar,
            overlay,
            frame_border: accent.opacity(FRAME_BORDER_STRENGTH),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tint_moves_base_toward_accent_by_strength() {
        let base = hex(0x000000FF);
        let accent = hex(0xFFFFFFFF);
        let tinted = gpui::Rgba::from(tint(base, accent, 0.25));
        assert!((tinted.r - 0.25).abs() < 0.01);
        assert!((tinted.a - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn bright_spaces_switch_to_dark_text() {
        assert!(wants_dark_text(average_background(
            SpaceTheme::Solid(0xFFE000),
            1.0
        )));
        assert!(!wants_dark_text(average_background(
            SpaceTheme::Solid(0xFFE000),
            0.0
        )));
        assert!(!wants_dark_text(average_background(
            SpaceTheme::Solid(0x3D3DFF),
            1.0
        )));
    }

    #[test]
    fn full_strength_tint_returns_accent() {
        let accent: Hsla = rgb(DEFAULT_ACCENT).into();
        assert_eq!(tint(hex(0x17171CFF), accent, 1.0), accent);
    }
}
