use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRoundRectRgn, DeleteObject, RGN_AND, RGN_DIFF, SetWindowRgn,
};

use crate::ContentBounds;

pub(crate) fn clip(hwnd: HWND, bounds: ContentBounds) {
    let scale = bounds.scale_factor;
    let width = (bounds.width * scale).round() as i32;
    let height = (bounds.height * scale).round() as i32;
    let diameter = (bounds.corner_radius * 2.0 * scale).round() as i32;
    let cutout = (bounds.cutout_width * scale).round() as i32;
    let cutout_top = (bounds.cutout_top * scale).round() as i32;
    unsafe {
        let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, diameter, diameter);
        if cutout > 0 {
            let overlay =
                CreateRoundRectRgn(-diameter, 0, cutout + 1, height + 1, diameter, diameter);
            CombineRgn(Some(region), Some(region), Some(overlay), RGN_DIFF);
            let _ = DeleteObject(overlay.into());
        }
        if cutout_top > 0 {
            let below = CreateRoundRectRgn(
                0,
                cutout_top,
                width + 1,
                height + 1 + diameter,
                diameter,
                diameter,
            );
            CombineRgn(Some(region), Some(region), Some(below), RGN_AND);
            let _ = DeleteObject(below.into());
        }
        SetWindowRgn(hwnd, Some(region), true);
    }
}
