use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use browser_core::browser::TextureKind;
use gpui::{Image, ImageFormat};

const SIZE: usize = 768;
const HEADER: usize = 54;

thread_local! {
    static CACHE: RefCell<HashMap<u8, Arc<Image>>> = RefCell::new(HashMap::new());
}

/// A tileless gray noise image for `kind`, made once and cached. Shown at a
/// low opacity over a space's colors, like a paper grain.
pub fn texture(kind: TextureKind) -> Option<Arc<Image>> {
    let key = match kind {
        TextureKind::None => return None,
        TextureKind::Grain => 1,
        TextureKind::Sand => 2,
        TextureKind::Denim => 3,
        TextureKind::Tweed => 4,
    };
    Some(CACHE.with(|cache| {
        cache
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| Arc::new(Image::from_bytes(ImageFormat::Bmp, bitmap(kind))))
            .clone()
    }))
}

fn bitmap(kind: TextureKind) -> Vec<u8> {
    let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
    let mut noise = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 40) as f32 / (1u64 << 24) as f32 - 0.5
    };
    let mut coarse = vec![0.0f32; (SIZE / 2 + 1) * (SIZE / 2 + 1)];
    for value in &mut coarse {
        *value = noise();
    }
    let row_bytes = SIZE * 3;
    let mut bytes = Vec::with_capacity(HEADER + row_bytes * SIZE);
    header(&mut bytes, row_bytes * SIZE);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (fx, fy) = (x as f32, y as f32);
            let level = match kind {
                TextureKind::None | TextureKind::Grain => noise() * 110.,
                TextureKind::Sand => {
                    coarse[(y / 2) * (SIZE / 2 + 1) + x / 2] * 150. + noise() * 40.
                }
                TextureKind::Denim => ((fx + fy) * 0.9).sin() * 45. + noise() * 80.,
                TextureKind::Tweed => ((fx * 0.8).sin() * (fy * 0.8).sin()) * 60. + noise() * 70.,
            };
            let gray = (128. + level).clamp(0., 255.) as u8;
            bytes.extend_from_slice(&[gray, gray, gray]);
        }
    }
    bytes
}

fn header(bytes: &mut Vec<u8>, pixels: usize) {
    let push32 = |bytes: &mut Vec<u8>, value: u32| bytes.extend_from_slice(&value.to_le_bytes());
    bytes.extend_from_slice(b"BM");
    push32(bytes, (HEADER + pixels) as u32);
    push32(bytes, 0);
    push32(bytes, HEADER as u32);
    push32(bytes, 40);
    push32(bytes, SIZE as u32);
    push32(bytes, SIZE as u32);
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&24u16.to_le_bytes());
    push32(bytes, 0);
    push32(bytes, pixels as u32);
    push32(bytes, 2835);
    push32(bytes, 2835);
    push32(bytes, 0);
    push32(bytes, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmaps_have_a_valid_header_and_size() {
        let bytes = bitmap(TextureKind::Denim);
        assert_eq!(&bytes[..2], b"BM");
        assert_eq!(bytes.len(), HEADER + SIZE * SIZE * 3);
        let declared = u32::from_le_bytes(bytes[2..6].try_into().unwrap()) as usize;
        assert_eq!(declared, bytes.len());
        assert!(texture(TextureKind::None).is_none());
    }
}
