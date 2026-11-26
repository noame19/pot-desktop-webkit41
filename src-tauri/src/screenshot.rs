use log::info;

/// Capture the screen whose top-left display position is `(x, y)` and write it to
/// `<cache>/pot_screenshot.png`.
///
/// Everything that can fail is reported back to the frontend instead of
/// panicking: on Wayland the capture goes through a desktop portal, which the
/// user can refuse, and a panic in a command handler takes down the whole
/// process.
#[tauri::command]
pub fn screenshot(x: i32, y: i32) -> Result<(), String> {
    use crate::APP;
    use dirs::cache_dir;
    use screenshots::Screen;
    use std::fs;

    info!("Screenshot screen with position: x={}, y={}", x, y);

    let screens = Screen::all().map_err(|e| format!("Enumerate screens failed: {e}"))?;
    // Tauri reports the monitor position in physical pixels, display-info
    // reports it in logical ones, and on a scaled Wayland desktop the two do not
    // always agree. Fall back to the first screen rather than failing: the
    // capture covers the whole desktop anyway and is cropped to that screen.
    let screen = screens
        .iter()
        .find(|screen| screen.display_info.x == x && screen.display_info.y == y)
        .or_else(|| screens.first())
        .ok_or_else(|| "No screen available".to_string())?;

    let image = screen
        .capture()
        .map_err(|e| format!("Capture screen failed: {e}"))?;

    let handle = APP.get().ok_or_else(|| "App is not ready".to_string())?;
    let mut app_cache_dir_path = cache_dir().ok_or_else(|| "Get Cache Dir Failed".to_string())?;
    app_cache_dir_path.push(&handle.config().tauri.bundle.identifier);
    if !app_cache_dir_path.exists() {
        fs::create_dir_all(&app_cache_dir_path).map_err(|e| format!("Create Cache Dir Failed: {e}"))?;
    }
    app_cache_dir_path.push("pot_screenshot.png");

    let buffer = image
        .to_png(screenshots::Compression::Fast)
        .map_err(|e| format!("Encode screenshot failed: {e}"))?;
    fs::write(&app_cache_dir_path, buffer).map_err(|e| format!("Write screenshot failed: {e}"))?;

    info!(
        "Saved screenshot {}x{} to {}",
        image.width(),
        image.height(),
        app_cache_dir_path.display()
    );
    Ok(())
}
