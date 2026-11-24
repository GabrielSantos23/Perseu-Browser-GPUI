use std::collections::{BTreeMap, HashSet};
use std::sync::RwLock;

use gpui::{Action, App, KeyBinding, Keystroke, Modifiers, NoAction, actions};
use webview_host::KeyChord;

actions!(
    perseu,
    [
        NewTab,
        CloseTab,
        ReopenClosedTab,
        NextTab,
        PreviousTab,
        SelectTab1,
        SelectTab2,
        SelectTab3,
        SelectTab4,
        SelectTab5,
        SelectTab6,
        SelectTab7,
        SelectTab8,
        SelectLastTab,
        FocusAddressBar,
        Reload,
        HardReload,
        GoBack,
        GoForward,
        CopyUrl,
        SwitchSpace1,
        SwitchSpace2,
        SwitchSpace3,
        SwitchSpace4,
        SwitchSpace5,
        SwitchSpace6,
        SwitchSpace7,
        SwitchSpace8,
        SwitchSpace9,
        ToggleCompactMode,
        ToggleFullscreen,
        ZoomIn,
        ZoomOut,
        ResetZoom,
        NewPrivateWindow,
        ShowHistory,
    ]
);

/// Zoom keys, fixed rather than editable: their keys are not letters or
/// digits, which the shortcut editor accepts.
const ZOOM_KEYS: &[(&str, ActionFactory)] = &[
    ("ctrl-=", || Box::new(ZoomIn)),
    ("ctrl-shift-=", || Box::new(ZoomIn)),
    ("ctrl-+", || Box::new(ZoomIn)),
    ("ctrl-shift-+", || Box::new(ZoomIn)),
    ("ctrl--", || Box::new(ZoomOut)),
    ("ctrl-shift--", || Box::new(ZoomOut)),
    ("ctrl-0", || Box::new(ResetZoom)),
];

type ActionFactory = fn() -> Box<dyn Action>;

const SHORTCUTS: &[(&str, ActionFactory)] = &[
    ("ctrl-t", || Box::new(NewTab)),
    ("ctrl-w", || Box::new(CloseTab)),
    ("ctrl-shift-t", || Box::new(ReopenClosedTab)),
    ("ctrl-tab", || Box::new(NextTab)),
    ("ctrl-shift-tab", || Box::new(PreviousTab)),
    ("ctrl-1", || Box::new(SelectTab1)),
    ("ctrl-2", || Box::new(SelectTab2)),
    ("ctrl-3", || Box::new(SelectTab3)),
    ("ctrl-4", || Box::new(SelectTab4)),
    ("ctrl-5", || Box::new(SelectTab5)),
    ("ctrl-6", || Box::new(SelectTab6)),
    ("ctrl-7", || Box::new(SelectTab7)),
    ("ctrl-8", || Box::new(SelectTab8)),
    ("ctrl-9", || Box::new(SelectLastTab)),
    ("ctrl-l", || Box::new(FocusAddressBar)),
    ("ctrl-r", || Box::new(Reload)),
    ("f5", || Box::new(Reload)),
    ("ctrl-shift-r", || Box::new(HardReload)),
    ("shift-f5", || Box::new(HardReload)),
    ("alt-left", || Box::new(GoBack)),
    ("alt-right", || Box::new(GoForward)),
    ("ctrl-shift-c", || Box::new(CopyUrl)),
    ("ctrl-alt-1", || Box::new(SwitchSpace1)),
    ("ctrl-alt-2", || Box::new(SwitchSpace2)),
    ("ctrl-alt-3", || Box::new(SwitchSpace3)),
    ("ctrl-alt-4", || Box::new(SwitchSpace4)),
    ("ctrl-alt-5", || Box::new(SwitchSpace5)),
    ("ctrl-alt-6", || Box::new(SwitchSpace6)),
    ("ctrl-alt-7", || Box::new(SwitchSpace7)),
    ("ctrl-alt-8", || Box::new(SwitchSpace8)),
    ("ctrl-alt-9", || Box::new(SwitchSpace9)),
    ("ctrl-s", || Box::new(ToggleCompactMode)),
    ("f11", || Box::new(ToggleFullscreen)),
    ("ctrl-shift-n", || Box::new(NewPrivateWindow)),
    ("ctrl-h", || Box::new(ShowHistory)),
];

const NAMED_KEYS: [&str; 3] = ["tab", "left", "right"];

static ACTIVE_KEYS: RwLock<Vec<String>> = RwLock::new(Vec::new());

pub struct ShortcutRow {
    pub default: &'static str,
    pub label: String,
    pub keys: String,
}

pub fn shortcut_rows(overrides: &BTreeMap<String, String>) -> Vec<ShortcutRow> {
    SHORTCUTS
        .iter()
        .map(|(default, action)| ShortcutRow {
            default,
            label: humanize(action().name()),
            keys: effective_keys(default, overrides),
        })
        .collect()
}

pub fn bind_shortcuts(cx: &mut App, overrides: &BTreeMap<String, String>) {
    let wanted: Vec<String> = SHORTCUTS
        .iter()
        .map(|(default, _)| effective_keys(default, overrides))
        .collect();
    let previous = std::mem::replace(
        &mut *ACTIVE_KEYS
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        wanted.clone(),
    );
    let unbound = previous
        .iter()
        .filter(|keys| !wanted.contains(keys))
        .map(|keys| (keys, Box::new(NoAction) as Box<dyn Action>));
    let bound = SHORTCUTS
        .iter()
        .zip(&wanted)
        .map(|((_, action), keys)| (keys, action()));
    let zoom = ZOOM_KEYS.iter().map(|(keys, action)| (*keys, action()));
    let bindings: Vec<KeyBinding> = unbound
        .map(|(keys, action)| (keys.as_str(), action))
        .chain(bound.map(|(keys, action)| (keys.as_str(), action)))
        .chain(zoom)
        .filter_map(
            |(keys, action)| match KeyBinding::load(keys, action, None, false, None, cx) {
                Ok(binding) => Some(binding),
                Err(error) => {
                    tracing::error!("invalid shortcut {keys}: {error}");
                    None
                }
            },
        )
        .collect();
    cx.bind_keys(bindings);
}

pub fn is_browser_shortcut(chord: &KeyChord) -> bool {
    let keys = chord_keys(chord);
    if ZOOM_KEYS.iter().any(|(zoom, _)| *zoom == keys) {
        return true;
    }
    ACTIVE_KEYS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .contains(&keys)
}

pub fn keystroke_for(chord: &KeyChord) -> Keystroke {
    Keystroke {
        modifiers: Modifiers {
            control: chord.ctrl,
            alt: chord.alt,
            shift: chord.shift,
            ..Modifiers::default()
        },
        key: chord.key.clone(),
        key_char: None,
    }
}

/// Checks typed keys such as `"Ctrl+Shift+K"` and returns them in keymap
/// form (`"ctrl-shift-k"`). Pages keep plain keys, so a shortcut needs Ctrl
/// or Alt unless it is a function key, and its key must be one the page
/// filter can see: a letter, digit, Tab, Left, Right or F1–F24.
pub fn normalize_shortcut(typed: &str) -> Result<String, &'static str> {
    let lowered = typed.trim().to_ascii_lowercase();
    let parts: Vec<&str> = lowered
        .split(['-', '+'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    let (key, modifiers) = parts.split_last().ok_or("Type a shortcut such as ctrl-k")?;
    let mut chord = KeyChord {
        ctrl: false,
        shift: false,
        alt: false,
        key: key.to_string(),
    };
    for modifier in modifiers {
        match *modifier {
            "ctrl" | "control" => chord.ctrl = true,
            "alt" => chord.alt = true,
            "shift" => chord.shift = true,
            _ => return Err("Use only Ctrl, Alt and Shift as modifiers"),
        }
    }
    let is_function_key = key
        .strip_prefix('f')
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=24).contains(&number));
    let is_plain_key = key.len() == 1 && key.chars().all(|c| c.is_ascii_alphanumeric());
    if !(is_function_key || is_plain_key || NAMED_KEYS.contains(key)) {
        return Err("Use a letter, digit, Tab, Left, Right or F1–F24");
    }
    if !(chord.ctrl || chord.alt || is_function_key) {
        return Err("Add Ctrl or Alt so typing in pages still works");
    }
    Ok(chord_keys(&chord))
}

/// Applies an edit to one shortcut and returns the resulting overrides, or
/// why it was refused. Empty input restores the default.
pub fn edit_shortcut(
    overrides: &BTreeMap<String, String>,
    default: &str,
    typed: &str,
) -> Result<BTreeMap<String, String>, String> {
    let mut edited = overrides.clone();
    if typed.trim().is_empty() {
        edited.remove(default);
        return Ok(edited);
    }
    let keys = normalize_shortcut(typed).map_err(str::to_string)?;
    if keys == default {
        edited.remove(default);
    } else {
        edited.insert(default.to_string(), keys.clone());
    }
    let mut seen = HashSet::new();
    for (bound, _) in SHORTCUTS {
        let effective = effective_keys(bound, &edited);
        if !seen.insert(effective.clone()) {
            return Err(format!("{effective} is already used by another shortcut"));
        }
    }
    Ok(edited)
}

fn effective_keys(default: &str, overrides: &BTreeMap<String, String>) -> String {
    overrides
        .get(default)
        .cloned()
        .unwrap_or_else(|| default.to_string())
}

fn humanize(action_name: &str) -> String {
    let name = action_name.rsplit("::").next().unwrap_or(action_name);
    let mut label = String::new();
    for (index, c) in name.chars().enumerate() {
        if index > 0 && (c.is_ascii_uppercase() || c.is_ascii_digit()) {
            label.push(' ');
        }
        if index > 0 {
            label.push(c.to_ascii_lowercase());
        } else {
            label.push(c);
        }
    }
    label
}

fn chord_keys(chord: &KeyChord) -> String {
    let mut keys = String::new();
    for (held, name) in [
        (chord.ctrl, "ctrl-"),
        (chord.alt, "alt-"),
        (chord.shift, "shift-"),
    ] {
        if held {
            keys.push_str(name);
        }
    }
    keys.push_str(&chord.key);
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(ctrl: bool, shift: bool, key: &str) -> KeyChord {
        KeyChord {
            ctrl,
            shift,
            alt: false,
            key: key.to_string(),
        }
    }

    fn activate_defaults() {
        *ACTIVE_KEYS.write().unwrap() =
            SHORTCUTS.iter().map(|(keys, _)| keys.to_string()).collect();
    }

    #[test]
    fn browser_shortcuts_are_recognised() {
        activate_defaults();
        assert!(is_browser_shortcut(&chord(true, false, "t")));
        assert!(is_browser_shortcut(&chord(true, true, "t")));
        assert!(is_browser_shortcut(&chord(true, true, "tab")));
        assert!(is_browser_shortcut(&chord(false, false, "f5")));
    }

    #[test]
    fn page_shortcuts_are_left_alone() {
        activate_defaults();
        assert!(!is_browser_shortcut(&chord(true, false, "c")));
        assert!(!is_browser_shortcut(&chord(true, false, "f")));
        assert!(!is_browser_shortcut(&chord(false, false, "t")));
    }

    #[test]
    fn every_shortcut_uses_canonical_modifier_order() {
        for (keys, _) in SHORTCUTS {
            assert_eq!(normalize_shortcut(keys).as_deref(), Ok(*keys));
        }
    }

    #[test]
    fn typed_shortcuts_are_normalized() {
        assert_eq!(
            normalize_shortcut(" Shift+Ctrl+K ").as_deref(),
            Ok("ctrl-shift-k")
        );
        assert_eq!(
            normalize_shortcut("alt-shift-3").as_deref(),
            Ok("alt-shift-3")
        );
        assert_eq!(normalize_shortcut("F11").as_deref(), Ok("f11"));
        assert!(normalize_shortcut("k").is_err());
        assert!(normalize_shortcut("shift-k").is_err());
        assert!(normalize_shortcut("ctrl-space").is_err());
        assert!(normalize_shortcut("win-k").is_err());
        assert!(normalize_shortcut("f25").is_err());
        assert!(normalize_shortcut("").is_err());
    }

    #[test]
    fn edits_override_reset_and_refuse_duplicates() {
        let none = BTreeMap::new();
        let edited = edit_shortcut(&none, "ctrl-s", "ctrl-shift-s").unwrap();
        assert_eq!(
            edited.get("ctrl-s").map(String::as_str),
            Some("ctrl-shift-s")
        );
        assert!(edit_shortcut(&edited, "ctrl-s", "").unwrap().is_empty());
        assert!(
            edit_shortcut(&edited, "ctrl-s", "ctrl-s")
                .unwrap()
                .is_empty()
        );
        assert!(edit_shortcut(&none, "ctrl-s", "ctrl-t").is_err());
    }

    #[test]
    fn labels_come_from_action_names() {
        assert_eq!(humanize("perseu::SwitchSpace3"), "Switch space 3");
        assert_eq!(humanize("perseu::NewTab"), "New tab");
    }
}
