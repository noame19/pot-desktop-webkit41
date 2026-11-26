use crate::{
    linux::wayland_screenshot::{crop_rgba_frame, wayland_capture_full},
    DisplayInfo, Image,
};
use anyhow::Result;

/// Logical bounding box of the whole desktop.
///
/// Used to map logical desktop coordinates onto the captured frame. The frame
/// size is whatever the compositor produced, so the ratio between it and this
/// box is the only trustworthy scale: `display_info.scale_factor` disagrees
/// across sources (KDE's kscreen says 1.75, GDK says 2, and the real capture is
/// 2881 logical-1646ths), while the frame is ground truth.
fn desktop_bounds(display_info: &DisplayInfo) -> (i32, i32, u32, u32) {
    match DisplayInfo::all() {
        Ok(screens) if !screens.is_empty() => {
            let left = screens.iter().map(|s| s.x).min().unwrap_or(display_info.x);
            let top = screens.iter().map(|s| s.y).min().unwrap_or(display_info.y);
            let right = screens
                .iter()
                .map(|s| s.x + s.width as i32)
                .max()
                .unwrap_or(display_info.x + display_info.width as i32);
            let bottom = screens
                .iter()
                .map(|s| s.y + s.height as i32)
                .max()
                .unwrap_or(display_info.y + display_info.height as i32);
            (left, top, (right - left).max(1) as u32, (bottom - top).max(1) as u32)
        }
        _ => (
            display_info.x,
            display_info.y,
            display_info.width.max(1),
            display_info.height.max(1),
        ),
    }
}

/// Capture the whole desktop and hand back the frame verbatim.
///
/// The frame's size is exactly what the compositor produced and is reported back
/// unchanged. It is *not* `display_info.width * scale_factor`: 1646 * 1.75 is
/// 2880.5, KDE rounds up to 2881, and using the arithmetic leaves every row one
/// pixel short of the real stride — which smears the image.
pub fn wayland_capture_screen(display_info: &DisplayInfo) -> Result<Image> {
    let (rgba, frame_width, frame_height) = wayland_capture_full()?;

    let (desk_x, desk_y, desk_width, desk_height) = desktop_bounds(display_info);
    let scale_x = frame_width as f32 / desk_width as f32;
    let scale_y = frame_height as f32 / desk_height as f32;

    // Multi-monitor: the portal returns the stitched desktop, so cut out the
    // monitor that was asked for. On a single monitor the crop is a no-op that
    // `crop_rgba_frame` short-circuits.
    let (rgba, width, height) = crop_rgba_frame(
        rgba,
        frame_width,
        frame_height,
        ((display_info.x - desk_x) as f32 * scale_x).round() as i32,
        ((display_info.y - desk_y) as f32 * scale_y).round() as i32,
        (display_info.width as f32 * scale_x).round() as i32,
        (display_info.height as f32 * scale_y).round() as i32,
    );

    Ok(Image::from_rgba8(rgba, width, height))
}

pub fn wayland_capture_screen_area(
    display_info: &DisplayInfo,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<Image> {
    let (rgba, frame_width, frame_height) = wayland_capture_full()?;

    // The area is expressed in logical pixels relative to the monitor, so shift
    // it into desktop space and scale it into frame pixels before cropping.
    let (desk_x, desk_y, desk_width, desk_height) = desktop_bounds(display_info);
    let scale_x = frame_width as f32 / desk_width as f32;
    let scale_y = frame_height as f32 / desk_height as f32;

    let (rgba, width, height) = crop_rgba_frame(
        rgba,
        frame_width,
        frame_height,
        (((display_info.x + x) - desk_x) as f32 * scale_x).round() as i32,
        (((display_info.y + y) - desk_y) as f32 * scale_y).round() as i32,
        (width as f32 * scale_x).round() as i32,
        (height as f32 * scale_y).round() as i32,
    );

    Ok(Image::from_rgba8(rgba, width, height))
}
