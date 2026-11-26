use anyhow::{anyhow, Result};
use dbus::{
    arg::{AppendAll, Iter, IterAppend, PropMap, ReadAll, RefArg, TypeMismatchError, Variant},
    blocking::Connection,
    message::{MatchRule, SignalArgs},
};
use percent_encoding::percent_decode;
use png::{BitDepth, ColorType, Decoder, Transformations};
use std::{
    collections::HashMap,
    env::temp_dir,
    fs::{self, File},
    io::BufReader,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug)]
pub struct OrgFreedesktopPortalRequestResponse {
    pub status: u32,
    pub results: PropMap,
}

impl AppendAll for OrgFreedesktopPortalRequestResponse {
    fn append(&self, i: &mut IterAppend) {
        RefArg::append(&self.status, i);
        RefArg::append(&self.results, i);
    }
}

impl ReadAll for OrgFreedesktopPortalRequestResponse {
    fn read(i: &mut Iter) -> Result<Self, TypeMismatchError> {
        Ok(OrgFreedesktopPortalRequestResponse {
            status: i.read()?,
            results: i.read()?,
        })
    }
}

impl SignalArgs for OrgFreedesktopPortalRequestResponse {
    const NAME: &'static str = "Response";
    const INTERFACE: &'static str = "org.freedesktop.portal.Request";
}

/// Decode any PNG the compositor hands us into tightly packed RGBA8.
///
/// The portals return whatever colour type they like — KDE writes plain RGB
/// (`colortype=2`, three bytes per pixel), GNOME writes RGBA. The original code
/// here assumed four bytes per pixel unconditionally, so an RGB capture was read
/// one byte off for every pixel, which is exactly the "garbled noise" users see
/// on KDE. Decode through `png` and convert explicitly instead.
fn decode_png_rgba(filename: &Path) -> Result<(Vec<u8>, u32, u32)> {
    let mut decoder = Decoder::new(BufReader::new(File::open(filename)?));
    // EXPAND turns palette images into RGB(A) and sub-byte greyscale into 8 bit,
    // so the match below only has to deal with the four true colour types.
    decoder.set_transformations(Transformations::EXPAND);

    let mut reader = decoder.read_info()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf)?;

    let width = info.width;
    let height = info.height;
    let stride = info.line_size;
    let channels: usize = match info.color_type {
        ColorType::Grayscale => 1,
        ColorType::GrayscaleAlpha => 2,
        ColorType::Rgb => 3,
        ColorType::Rgba => 4,
        ColorType::Indexed => return Err(anyhow!("palette was not expanded")),
    };
    // `EXPAND` handles palettes and low bit depths but keeps 16 bit samples
    // intact, so account for the wider samples here.
    let sample_bytes: usize = match info.bit_depth {
        BitDepth::Sixteen => 2,
        _ => 1,
    };

    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    for row in 0..height as usize {
        let row_start = row * stride;
        for col in 0..width as usize {
            let src = row_start + col * channels * sample_bytes;
            // PNG stores 16 bit samples big-endian, so the first byte of a
            // sample is its high byte — the only part an 8 bit surface keeps.
            let sample = |channel: usize| -> u8 {
                buf.get(src + channel * sample_bytes).copied().unwrap_or(0)
            };
            let dst = (row * width as usize + col) * 4;
            match channels {
                1 => {
                    let l = sample(0);
                    rgba[dst] = l;
                    rgba[dst + 1] = l;
                    rgba[dst + 2] = l;
                    rgba[dst + 3] = 255;
                }
                2 => {
                    let l = sample(0);
                    rgba[dst] = l;
                    rgba[dst + 1] = l;
                    rgba[dst + 2] = l;
                    rgba[dst + 3] = sample(1);
                }
                3 => {
                    rgba[dst] = sample(0);
                    rgba[dst + 1] = sample(1);
                    rgba[dst + 2] = sample(2);
                    rgba[dst + 3] = 255;
                }
                _ => {
                    rgba[dst] = sample(0);
                    rgba[dst + 1] = sample(1);
                    rgba[dst + 2] = sample(2);
                    rgba[dst + 3] = sample(3);
                }
            }
        }
    }

    Ok((rgba, width, height))
}

/// Crop a rectangle out of a decoded frame, clamping it to the frame bounds.
///
/// The requested rectangle comes from scaled display geometry (logical size times
/// the monitor scale factor), which does not always divide out to exactly the
/// pixel size the compositor captured. Clamping keeps the result non-black at the
/// edges, and the caller learns the real size back so it never has to guess.
fn crop_rgba(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    crop_width: i32,
    crop_height: i32,
) -> (Vec<u8>, u32, u32) {
    let left = x.clamp(0, width as i32);
    let top = y.clamp(0, height as i32);
    let right = (x.saturating_add(crop_width.max(0))).clamp(left, width as i32);
    let bottom = (y.saturating_add(crop_height.max(0))).clamp(top, height as i32);

    let out_width = (right - left) as u32;
    let out_height = (bottom - top) as u32;
    if out_width == 0 || out_height == 0 {
        return (rgba, width, height);
    }
    if out_width == width && out_height == height && left == 0 && top == 0 {
        return (rgba, width, height);
    }

    let mut out = vec![0u8; out_width as usize * out_height as usize * 4];
    for row in 0..out_height as usize {
        let src = ((top as usize + row) * width as usize + left as usize) * 4;
        let dst = row * out_width as usize * 4;
        let len = out_width as usize * 4;
        out[dst..dst + len].copy_from_slice(&rgba[src..src + len]);
    }

    (out, out_width, out_height)
}

/// Read a compositor-written screenshot file, crop it and return packed RGBA
/// together with the real pixel size of the cropped result.
fn png_to_rgba(filename: &Path, x: i32, y: i32, width: i32, height: i32) -> Result<(Vec<u8>, u32, u32)> {
    let (rgba, frame_width, frame_height) = decode_png_rgba(filename)?;
    Ok(crop_rgba(rgba, frame_width, frame_height, x, y, width, height))
}

fn portal_handle_token() -> String {
    // The portal rejects a call while another one with the same token is still
    // in flight, so two quick captures in a row must not share "1234".
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("pot_{}_{}", std::process::id(), nanos)
}

fn org_gnome_shell_screenshot(conn: &Connection) -> Result<(Vec<u8>, u32, u32)> {
    let proxy = conn.with_proxy(
        "org.gnome.Shell.Screenshot",
        "/org/gnome/Shell/Screenshot",
        Duration::from_secs(10),
    );

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?;

    let dirname = temp_dir().join("screenshot");

    fs::create_dir_all(&dirname)?;

    let mut path = dirname.join(timestamp.as_micros().to_string());
    path.set_extension("png");

    let filename = path.to_string_lossy().to_string();

    // Always grab the whole screen and crop locally: a compositor-side crop
    // would return a differently sized frame, and the caller needs to know the
    // real size it is cropping into.
    proxy.method_call(
        "org.gnome.Shell.Screenshot",
        "Screenshot",
        (false, false, &filename),
    )?;

    let result = png_to_rgba(&path, 0, 0, 0, 0);

    let _ = fs::remove_file(&path);

    result
}

fn org_freedesktop_portal_screenshot(conn: &Connection) -> Result<(Vec<u8>, u32, u32)> {
    let status: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
    let status_res = status.clone();
    let path: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let path_res = path.clone();
    let handle_token = portal_handle_token();

    let match_rule = MatchRule::new_signal("org.freedesktop.portal.Request", "Response");
    conn.add_match(
        match_rule,
        move |response: OrgFreedesktopPortalRequestResponse, _conn, _msg| {
            if let Ok(mut status) = status.lock() {
                *status = Some(response.status);
            }

            let uri = response.results.get("uri").and_then(|str| str.as_str());
            if let (Some(uri_str), Ok(mut path)) = (uri, path.lock()) {
                *path = uri_str.strip_prefix("file://").unwrap_or(uri_str).to_string();
            }

            true
        },
    )?;

    let proxy = conn.with_proxy(
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        Duration::from_millis(10000),
    );

    let mut options: PropMap = HashMap::new();
    options.insert(String::from("handle_token"), Variant(Box::new(handle_token)));
    options.insert(String::from("modal"), Variant(Box::new(true)));
    options.insert(String::from("interactive"), Variant(Box::new(false)));

    proxy.method_call(
        "org.freedesktop.portal.Screenshot",
        "Screenshot",
        ("", options),
    )?;

    // wait 60 seconds for user interaction
    for _ in 0..60 {
        let result = conn.process(Duration::from_millis(1000))?;
        let status = status_res
            .lock()
            .map_err(|_| anyhow!("Get status lock failed"))?;

        if result && status.is_some() {
            break;
        }
    }

    let status = status_res
        .lock()
        .map_err(|_| anyhow!("Get status lock failed"))?;
    let status = *status;

    let path = path_res
        .lock()
        .map_err(|_| anyhow!("Get path lock failed"))?;
    let path = &*path;

    if status.ne(&Some(0)) || path.is_empty() {
        if !path.is_empty() {
            fs::remove_file(path)?;
        }
        return Err(anyhow!("Screenshot failed or canceled",));
    }

    let filename = percent_decode(path.as_bytes()).decode_utf8()?.to_string();
    let result = png_to_rgba(Path::new(&filename), 0, 0, 0, 0);

    let _ = fs::remove_file(&filename);

    result
}

/// Capture the whole screen and return packed RGBA plus the frame's real pixel
/// size.
///
/// Wayland's screenshot APIs hand back a full-screen image; there is no
/// "capture this rectangle" call. Cropping therefore happens after the fact,
/// and the caller must use the returned size rather than a size derived from
/// logical geometry times a scale factor — those never agree exactly (1646x1029
/// at scale 1.75 is 2880.5x1800.75, while the compositor writes 2881x1801).
pub fn wayland_capture_full() -> Result<(Vec<u8>, u32, u32)> {
    let conn = Connection::new_session()?;

    // Both backends are asked for the entire screen: (0, 0) with a zero size
    // makes GNOME's ScreenshotArea a no-op crop, and the portal ignores the
    // rectangle altogether.
    org_gnome_shell_screenshot(&conn).or_else(|e| {
        org_freedesktop_portal_screenshot(&conn).map_err(|_| e)
    })
}

/// Crop packed RGBA down to `(x, y, width, height)`, clamped to the frame.
pub fn crop_rgba_frame(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    crop_width: i32,
    crop_height: i32,
) -> (Vec<u8>, u32, u32) {
    crop_rgba(rgba, width, height, x, y, crop_width, crop_height)
}
