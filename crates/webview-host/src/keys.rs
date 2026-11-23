use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, VIRTUAL_KEY, VK_ADD, VK_CONTROL, VK_ESCAPE, VK_F1, VK_F24,
    VK_LEFT, VK_MENU, VK_OEM_MINUS, VK_OEM_PLUS, VK_RIGHT, VK_SHIFT, VK_SUBTRACT, VK_TAB,
};

use crate::KeyChord;

const KEY_DOWN_MASK: i16 = i16::MIN;

pub(crate) fn chord_for(virtual_key: u32) -> Option<KeyChord> {
    let key = key_name(virtual_key)?;
    Some(KeyChord {
        ctrl: is_down(VK_CONTROL),
        shift: is_down(VK_SHIFT),
        alt: is_down(VK_MENU),
        key,
    })
}

/// Ctrl, Shift and Alt as physically held right now. Mouse clicks reach the
/// page's process, so this thread's key state can lag behind.
pub(crate) fn held_modifiers() -> (bool, bool, bool) {
    let held = |key: VIRTUAL_KEY| unsafe { GetAsyncKeyState(key.0.into()) } & KEY_DOWN_MASK != 0;
    (held(VK_CONTROL), held(VK_SHIFT), held(VK_MENU))
}

fn is_down(key: VIRTUAL_KEY) -> bool {
    unsafe { GetKeyState(key.0.into()) & KEY_DOWN_MASK != 0 }
}

fn key_name(virtual_key: u32) -> Option<String> {
    let code = u16::try_from(virtual_key).ok()?;
    if code == VK_TAB.0 {
        return Some("tab".to_string());
    }
    for (named, name) in [
        (VK_ESCAPE, "escape"),
        (VK_LEFT, "left"),
        (VK_RIGHT, "right"),
    ] {
        if code == named.0 {
            return Some(name.to_string());
        }
    }
    if [VK_OEM_PLUS, VK_ADD].iter().any(|key| key.0 == code) {
        return Some("=".to_string());
    }
    if [VK_OEM_MINUS, VK_SUBTRACT].iter().any(|key| key.0 == code) {
        return Some("-".to_string());
    }
    if (VK_F1.0..=VK_F24.0).contains(&code) {
        return Some(format!("f{}", code - VK_F1.0 + 1));
    }
    match u8::try_from(code).ok()? {
        byte @ (b'0'..=b'9' | b'A'..=b'Z') => {
            Some(char::from(byte).to_ascii_lowercase().to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_letters_digits_tab_and_function_keys() {
        assert_eq!(key_name(u32::from(b'T')).as_deref(), Some("t"));
        assert_eq!(key_name(u32::from(b'7')).as_deref(), Some("7"));
        assert_eq!(key_name(u32::from(VK_TAB.0)).as_deref(), Some("tab"));
        assert_eq!(key_name(u32::from(VK_ESCAPE.0)).as_deref(), Some("escape"));
        assert_eq!(key_name(u32::from(VK_LEFT.0)).as_deref(), Some("left"));
        assert_eq!(key_name(u32::from(VK_F1.0) + 4).as_deref(), Some("f5"));
    }

    #[test]
    fn ignores_other_keys() {
        assert_eq!(key_name(0x20), None);
        assert_eq!(key_name(u32::from(VK_SHIFT.0)), None);
    }
}
