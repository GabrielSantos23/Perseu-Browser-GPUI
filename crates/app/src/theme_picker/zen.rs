// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//
// Ported from Zen Browser's ZenGradientGenerator.mjs and theme-picker.inc
// (https://github.com/zen-browser/desktop).

use std::f32::consts::PI;

/// Side of the square color pad, in logical pixels.
pub const PAD: f32 = 342.;
pub const MAX_DOTS: usize = 3;
const COLOR_PADDING: f32 = 30.;
const DOT_HALF_SIZE: f32 = 29.;
const INITIAL_RECT: f32 = 364.;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DotKind {
    /// Placed by clicking: lightness follows the distance from the center.
    Free,
    /// From a preset: lightness is fixed and the distance sets saturation.
    Lightness,
    /// From the gray presets: no saturation, distance sets lightness.
    BlackWhite,
}

impl DotKind {
    fn code(self) -> char {
        match self {
            Self::Free => 'f',
            Self::Lightness => 'l',
            Self::BlackWhite => 'b',
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        match code {
            "f" => Some(Self::Free),
            "l" => Some(Self::Lightness),
            "b" => Some(Self::BlackWhite),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dot {
    pub x: f32,
    pub y: f32,
    pub kind: DotKind,
}

pub struct Harmony {
    pub name: &'static str,
    pub angles: &'static [f32],
}

pub const HARMONIES: [Harmony; 6] = [
    Harmony {
        name: "complementary",
        angles: &[180.],
    },
    Harmony {
        name: "singleAnalogous",
        angles: &[310.],
    },
    Harmony {
        name: "splitComplementary",
        angles: &[150., 210.],
    },
    Harmony {
        name: "analogous",
        angles: &[50., 310.],
    },
    Harmony {
        name: "triadic",
        angles: &[120., 240.],
    },
    Harmony {
        name: "floating",
        angles: &[],
    },
];

#[derive(Clone, Copy, PartialEq)]
pub enum Action {
    Add,
    Remove,
    Update,
}

fn harmony_named(name: &str) -> Option<&'static Harmony> {
    HARMONIES.iter().find(|harmony| harmony.name == name)
}

fn harmony_with(angles: usize) -> Option<&'static Harmony> {
    HARMONIES
        .iter()
        .find(|harmony| harmony.angles.len() == angles)
}

/// Which harmony lays out `dot_count` dots (after the action) — Zen's
/// getColorHarmonyType.
fn harmony_for(dot_count: usize, action: Action, current: &str) -> Option<&'static Harmony> {
    if let Some(selected) = harmony_named(current) {
        return match action {
            Action::Remove if dot_count > 0 => {
                selected.angles.len().checked_sub(1).and_then(harmony_with)
            }
            Action::Remove => harmony_named("floating"),
            Action::Add => harmony_with(selected.angles.len() + 1),
            Action::Update => Some(selected),
        };
    }
    match action {
        Action::Remove => {
            let harmony = harmony_with(dot_count)?;
            if harmony.name == "analogous" && dot_count == 1 {
                harmony_named("singleAnalogous")
            } else {
                Some(harmony)
            }
        }
        Action::Add | Action::Update => HARMONIES
            .iter()
            .find(|harmony| harmony.angles.len() + 1 == dot_count),
    }
}

/// New dot positions around the primary dot (index 0) for `action` —
/// Zen's calculateCompliments. Also returns the harmony now in use.
pub fn compliments(dots: &[Dot], action: Action, current: &str) -> (Vec<Dot>, String) {
    let Some(primary) = dots.first().copied() else {
        return (Vec::new(), current.to_string());
    };
    let delta: isize = match action {
        Action::Add => 1,
        Action::Remove => -1,
        Action::Update => 0,
    };
    let count = (dots.len() as isize + delta).max(0) as usize;
    let Some(harmony) = harmony_for(count, action, current) else {
        return (dots.to_vec(), current.to_string());
    };
    if harmony.angles.is_empty() {
        return (dots.to_vec(), harmony.name.to_string());
    }
    let center = PAD / 2.;
    let base_angle = (primary.y - center).atan2(primary.x - center).to_degrees();
    let distance = (primary.x - center).hypot(primary.y - center).min(PAD / 2.);
    let mut updated = vec![primary];
    for offset in harmony.angles {
        let radians = ((base_angle + offset).rem_euclid(360.)) * PI / 180.;
        updated.push(Dot {
            x: center + distance * radians.cos(),
            y: center + distance * radians.sin(),
            kind: primary.kind,
        });
    }
    (updated, harmony.name.to_string())
}

/// Keeps a pad point inside the circle the dots live in.
pub fn clamp_to_circle(x: f32, y: f32) -> (f32, f32) {
    let center = PAD / 2.;
    let radius = PAD / 2.;
    let distance = (x - center).hypot(y - center);
    if distance <= radius {
        return (x, y);
    }
    let angle = (y - center).atan2(x - center);
    (center + angle.cos() * radius, center + angle.sin() * radius)
}

/// The color of a dot — Zen's getColorFromPosition. `lightness` (0–100) is
/// used by preset dots.
pub fn color_at(dot: Dot, lightness: f32) -> u32 {
    let width = PAD + COLOR_PADDING * 2.;
    let center = width / 2.;
    let radius = (width - COLOR_PADDING) / 2.;
    let x = dot.x + DOT_HALF_SIZE;
    let y = dot.y + DOT_HALF_SIZE;
    let distance = (x - center).hypot(y - center) - 0.2;
    let hue = (y - center).atan2(x - center).to_degrees().rem_euclid(360.);
    let near = 1. - (distance / radius).min(1.);
    let (r, g, b) = match dot.kind {
        DotKind::Lightness => hsb_to_rgb(hue, near, lightness / 100.),
        DotKind::Free => {
            let saturation = 90. + (1. - near) * 10.;
            let lightness = ((1. - near) * 100.).round();
            hsl_to_rgb(hue / 360., saturation / 100., lightness / 100.)
        }
        DotKind::BlackWhite => {
            let lightness = ((1. - near) * 100.).round();
            hsl_to_rgb(hue / 360., 0., lightness / 100.)
        }
    };
    (r << 16) | (g << 8) | b
}

/// Where a color sits on the pad when no saved dots exist — Zen's
/// calculateInitialPosition.
pub fn initial_position(color: u32) -> (f32, f32) {
    let (hue, saturation, _) = rgb_to_hsl(color);
    let center = INITIAL_RECT / 2.;
    let radius = INITIAL_RECT / 2.;
    let angle = hue / 360. * 2. * PI;
    let scale = PAD / INITIAL_RECT;
    (
        (center + radius * saturation * angle.cos()) * scale,
        (center + radius * saturation * angle.sin()) * scale,
    )
}

fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0. {
        t += 1.;
    }
    if t > 1. {
        t -= 1.;
    }
    if t < 1. / 6. {
        return p + (q - p) * 6. * t;
    }
    if t < 1. / 2. {
        return q;
    }
    if t < 2. / 3. {
        return p + (q - p) * (2. / 3. - t) * 6.;
    }
    p
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u32, u32, u32) {
    let (r, g, b) = if s == 0. {
        (l, l, l)
    } else {
        let q = if l < 0.5 { l * (1. + s) } else { l + s - l * s };
        let p = 2. * l - q;
        (
            hue_to_rgb(p, q, h + 1. / 3.),
            hue_to_rgb(p, q, h),
            hue_to_rgb(p, q, h - 1. / 3.),
        )
    };
    let channel = |value: f32| (value * 255.).round().clamp(0., 255.) as u32;
    (channel(r), channel(g), channel(b))
}

fn hsb_to_rgb(hue: f32, saturation: f32, brightness: f32) -> (u32, u32, u32) {
    let c = brightness * saturation;
    let x = c * (1. - ((hue / 60.) % 2. - 1.).abs());
    let m = brightness - c;
    let sector = ((hue.rem_euclid(360.)) / 60.).floor() as usize % 6;
    let (r, g, b) = [
        (c, x, 0.),
        (x, c, 0.),
        (0., c, x),
        (0., x, c),
        (x, 0., c),
        (c, 0., x),
    ][sector];
    let channel = |value: f32| ((value + m) * 255.).round().clamp(0., 255.) as u32;
    (channel(r), channel(g), channel(b))
}

/// Hue in degrees, saturation and lightness in 0–1.
pub fn rgb_to_hsl(color: u32) -> (f32, f32, f32) {
    let r = ((color >> 16) & 0xFF) as f32 / 255.;
    let g = ((color >> 8) & 0xFF) as f32 / 255.;
    let b = (color & 0xFF) as f32 / 255.;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.;
    if max == min {
        return (0., 0., l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2. - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        (g - b) / d + if g < b { 6. } else { 0. }
    } else if max == g {
        (b - r) / d + 2.
    } else {
        (r - g) / d + 4.
    };
    (h * 60., s, l)
}

/// A swatch in the preset pages.
pub struct Preset {
    pub lightness: Option<f32>,
    pub analogous: bool,
    pub x: f32,
    pub y: f32,
    pub colors: [u32; 3],
    pub kind: DotKind,
}

const fn preset(lightness: f32, x: f32, y: f32, color: u32) -> Preset {
    Preset {
        lightness: Some(lightness),
        analogous: false,
        x,
        y,
        colors: [color, color, color],
        kind: DotKind::Lightness,
    }
}

const fn analogous(lightness: f32, x: f32, y: f32, colors: [u32; 3]) -> Preset {
    Preset {
        lightness: Some(lightness),
        analogous: true,
        x,
        y,
        colors,
        kind: DotKind::Lightness,
    }
}

const fn gray(x: f32, color: u32) -> Preset {
    Preset {
        lightness: None,
        analogous: false,
        x,
        y: 180.,
        colors: [color, color, color],
        kind: DotKind::BlackWhite,
    }
}

pub const PRESETS_PER_PAGE: usize = 9;

/// Zen's preset pages, nine swatches each.
pub const PRESETS: [Preset; 63] = [
    preset(95., 330., 248., 0xF2EAE4),
    preset(95., 282., 127., 0xF29BBB),
    preset(65., 255., 75., 0xA6729D),
    preset(95., 244., 165., 0xF25E6B),
    preset(100., 234., 190., 0xFF8558),
    preset(97., 217., 220., 0xF8D558),
    preset(91., 135., 191., 0x34E895),
    preset(85., 84., 145., 0x6CBAD9),
    preset(54., 99., 54., 0x666789),
    analogous(95., 330., 248., [0xF2EAE4, 0xEEF2E4, 0xF2E4EA]),
    analogous(95., 282., 127., [0xF29BBB, 0xF2C49B, 0xE09BF2]),
    analogous(65., 255., 75., [0xA6729D, 0xA67272, 0x8472A6]),
    analogous(95., 244., 165., [0xF25E6B, 0xF2CC5E, 0xF25EE6]),
    analogous(100., 234., 190., [0xFF8558, 0xEEFF58, 0xFF58B6]),
    analogous(97., 217., 220., [0xF8D558, 0x96F858, 0xF85860]),
    analogous(91., 135., 191., [0x34E895, 0x34A5E8, 0x69E834]),
    analogous(85., 84., 145., [0x6CBAD9, 0x796CD9, 0x6CD99D]),
    analogous(54., 99., 54., [0x666789, 0x826689, 0x668489]),
    preset(100., 349., 219., 0xFEFBFA),
    preset(100., 323., 94., 0xFFE9F3),
    preset(95., 274., 47., 0xF1D1EC),
    preset(100., 319., 154., 0xFFCBD1),
    preset(100., 319., 219., 0xFFE0D3),
    preset(100., 288., 292., 0xFFF9E5),
    preset(99., 38., 265., 0xDEFDEB),
    preset(99., 17., 125., 0xDCF3FC),
    preset(90., 88., 36., 0xC5C6E5),
    analogous(100., 349., 219., [0xFEFBFA, 0xFEFEFA, 0xFEFAFC]),
    analogous(100., 323., 94., [0xFFE9F3, 0xFFF1E9, 0xF9E9FF]),
    analogous(95., 274., 47., [0xF1D1EC, 0xF1D1D1, 0xDBD1F1]),
    analogous(100., 319., 154., [0xFFCBD1, 0xFFF0CB, 0xFFCBFC]),
    analogous(100., 319., 219., [0xFFE0D3, 0xF9FFD3, 0xFFD3EB]),
    analogous(100., 288., 292., [0xFFF9E5, 0xEFFFE5, 0xFFE5E7]),
    analogous(99., 38., 265., [0xDEFDEB, 0xDEF5FD, 0xEBFDDE]),
    analogous(99., 17., 125., [0xDCF3FC, 0xE0DCFC, 0xDCFCEA]),
    analogous(90., 88., 36., [0xC5C6E5, 0xDFC5E5, 0xC5E1E5]),
    preset(35., 188., 53., 0x4E3A5A),
    preset(41., 244., 111., 0x693558),
    preset(56., 255., 170., 0x8F4042),
    preset(69., 233., 199., 0xB16D40),
    preset(84., 232., 226., 0xD7B35D),
    preset(80., 250., 299., 0xCDCCA4),
    preset(66., 85., 275., 0x7AA982),
    preset(38., 112., 197., 0x226149),
    preset(42., 117., 136., 0x26456B),
    analogous(35., 188., 53., [0x4E3A5A, 0x5A3A4B, 0x3A415A]),
    analogous(41., 244., 111., [0x693558, 0x693D35, 0x4F3569]),
    analogous(56., 255., 170., [0x8F4042, 0x8F8040, 0x8F4084]),
    analogous(69., 233., 199., [0xB16D40, 0x97B140, 0xB14071]),
    analogous(84., 232., 226., [0xD7B35D, 0x95D75D, 0xD75D6D]),
    analogous(80., 250., 299., [0xCDCCA4, 0xACCDA4, 0xCDAAA4]),
    analogous(66., 85., 275., [0x7AA982, 0x7AA9A9, 0x99A97A]),
    analogous(38., 112., 197., [0x226149, 0x224461, 0x2F6122]),
    analogous(42., 117., 136., [0x26456B, 0x40266B, 0x266B57]),
    gray(340., 0xE0E0E0),
    gray(337.5, 0xE0E0E0),
    gray(315., 0xC0C0C0),
    gray(292.5, 0xA0A0A0),
    gray(270., 0x808080),
    gray(247.5, 0x606060),
    gray(225., 0x404040),
    gray(202.5, 0x202020),
    gray(180., 0x000000),
];

/// The picker's state as saved with the space: lightness, harmony and dots.
pub fn encode(dots: &[Dot], lightness: f32, harmony: &str) -> String {
    let mut text = format!("1;{lightness};{harmony}");
    for dot in dots {
        text.push_str(&format!(";{:.1},{:.1},{}", dot.x, dot.y, dot.kind.code()));
    }
    text
}

pub fn decode(text: &str) -> Option<(Vec<Dot>, f32, String)> {
    let mut parts = text.split(';');
    if parts.next()? != "1" {
        return None;
    }
    let lightness = parts.next()?.parse().ok()?;
    let harmony = parts.next()?.to_string();
    let dots = parts
        .map(|part| {
            let mut fields = part.split(',');
            Some(Dot {
                x: fields.next()?.parse().ok()?,
                y: fields.next()?.parse().ok()?,
                kind: DotKind::from_code(fields.next()?)?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some((dots, lightness, harmony))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harmonies_place_dots_around_the_primary() {
        let primary = Dot {
            x: PAD / 2. + 100.,
            y: PAD / 2.,
            kind: DotKind::Free,
        };
        let (dots, harmony) = compliments(&[primary], Action::Add, "");
        assert_eq!(harmony, "complementary");
        assert_eq!(dots.len(), 2);
        assert!((dots[1].x - (PAD / 2. - 100.)).abs() < 0.01);
        let (dots, harmony) = compliments(&dots, Action::Add, &harmony);
        assert_eq!(harmony, "splitComplementary");
        assert_eq!(dots.len(), 3);
        let (dots, harmony) = compliments(&dots[..2], Action::Remove, "");
        assert_eq!(harmony, "complementary");
        assert_eq!(dots.len(), 2);
        let (dots, harmony) = compliments(&dots[..1], Action::Remove, "");
        assert_eq!(harmony, "floating");
        assert_eq!(dots.len(), 1);
    }

    #[test]
    fn center_is_dark_and_edge_is_light_for_free_dots() {
        let center = Dot {
            x: PAD / 2. + 1.,
            y: PAD / 2. + 1.,
            kind: DotKind::Free,
        };
        assert_eq!(color_at(center, 50.), 0x000000);
        let preset = Dot {
            x: 244.,
            y: 165.,
            kind: DotKind::Lightness,
        };
        let (_, saturation, _) = rgb_to_hsl(color_at(preset, 95.));
        assert!(saturation > 0.3);
    }

    #[test]
    fn state_round_trips_through_text() {
        let dots = vec![
            Dot {
                x: 12.5,
                y: 40.,
                kind: DotKind::Free,
            },
            Dot {
                x: 300.,
                y: 2.,
                kind: DotKind::BlackWhite,
            },
        ];
        let text = encode(&dots, 95., "complementary");
        assert_eq!(decode(&text), Some((dots, 95., "complementary".into())));
        assert_eq!(decode("garbage"), None);
    }

    #[test]
    fn dots_stay_inside_the_circle() {
        let (x, y) = clamp_to_circle(PAD * 2., PAD / 2.);
        assert!((x - PAD).abs() < 0.01 && (y - PAD / 2.).abs() < 0.01);
    }
}
