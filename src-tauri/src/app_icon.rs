//! Custom app icon, ported from teams-for-linux's App Icon menu: picked from
//! the tray menu, stored in config.json, applied live to the tray and window,
//! and optionally copied into the user icon theme. The desktop resolves
//! taskbar, launcher and (on Wayland) window icons by name from the icon
//! theme, never from the running window, so a custom icon only shows there
//! once a copy sits in the per-user hicolor theme under that name.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use gtk::prelude::*;
use image::RgbaImage;
use tauri::{AppHandle, Manager};

use crate::config;

const SIZES: [u32; 10] = [16, 22, 24, 32, 48, 64, 96, 128, 256, 512];
const DEFAULT_NAME: &str = "outlook-for-linux";

/// Decoded through gdk-pixbuf, so anything the desktop can open works,
/// SVG included (rendered at 512 px through the librsvg loader).
pub fn load_image(path: &str) -> Option<RgbaImage> {
    use gtk::gdk_pixbuf::Pixbuf;
    if path.is_empty() {
        return None;
    }
    let svg = Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("svgz"));
    let pixbuf = if svg { Pixbuf::from_file_at_scale(path, 512, 512, true) } else { Pixbuf::from_file(path) }.ok()?;
    let pixbuf = if pixbuf.has_alpha() { pixbuf } else { pixbuf.add_alpha(false, 0, 0, 0).ok()? };
    let (w, h, stride) = (pixbuf.width() as usize, pixbuf.height() as usize, pixbuf.rowstride() as usize);
    let bytes = pixbuf.read_pixel_bytes();
    let mut rgba = Vec::with_capacity(w * h * 4);
    for row in bytes.chunks(stride).take(h) {
        rgba.extend_from_slice(row.get(..w * 4)?);
    }
    RgbaImage::from_raw(w as u32, h as u32, rgba)
}

pub fn to_tauri(image: RgbaImage) -> tauri::image::Image<'static> {
    let (w, h) = image.dimensions();
    tauri::image::Image::new_owned(image.into_raw(), w, h)
}

/// Scaled to fit a transparent square of `size`.
fn resized(image: &RgbaImage, size: u32) -> RgbaImage {
    let (w, h) = image.dimensions();
    let (nw, nh) = if w >= h {
        (size, (size * h / w).max(1))
    } else {
        ((size * w / h).max(1), size)
    };
    let scaled = image::imageops::resize(image, nw, nh, image::imageops::FilterType::Lanczos3);
    let mut canvas = RgbaImage::new(size, size);
    image::imageops::overlay(&mut canvas, &scaled, ((size - nw) / 2).into(), ((size - nh) / 2).into());
    canvas
}

/// Red badge at the top right, matching the bundled tray-unread.png.
pub fn with_unread_dot(image: &RgbaImage) -> RgbaImage {
    let mut out = resized(image, 128);
    let (cx, cy, r) = (100.0_f32, 28.0_f32, 22.0_f32);
    for (x, y, px) in out.enumerate_pixels_mut() {
        let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
        if d <= r {
            *px = if d > r - 3.0 { image::Rgba([255, 255, 255, 255]) } else { image::Rgba([232, 17, 35, 255]) };
        }
    }
    out
}

// Icon theme copy.

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share"))
}

fn hicolor_dir() -> PathBuf {
    data_home().join("icons/hicolor")
}

fn manifest_file() -> PathBuf {
    data_home().join(DEFAULT_NAME).join("theme-icon.json")
}

pub fn supported() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("FLATPAK_ID").is_none() && std::env::var_os("SNAP").is_none()
}

fn read_desktop_files() -> Vec<(String, String)> {
    let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    let mut files = Vec::new();
    for dir in std::iter::once(data_home()).chain(dirs.split(':').map(PathBuf::from)) {
        let Ok(entries) = fs::read_dir(dir.join("applications")) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".desktop") {
                continue;
            }
            if let Ok(text) = fs::read_to_string(entry.path()) {
                files.push((name, text));
            }
        }
    }
    files
}

fn icon_from_desktop(text: &str) -> Option<String> {
    let icon = text.lines().find_map(|l| l.strip_prefix("Icon="))?.trim();
    let valid = !icon.is_empty() && icon.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c));
    valid.then(|| icon.to_string())
}

/// `Icon=` of the desktop entry the shell matches this window to: the
/// packaged entry by file name, otherwise whichever launches this binary
/// (AppImage integrations install one under their own icon name).
fn icon_name() -> String {
    let files = read_desktop_files();
    let exe = std::env::var("APPIMAGE")
        .ok()
        .or_else(|| std::env::current_exe().ok().map(|p| p.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let packaged = format!("{DEFAULT_NAME}.desktop");
    files
        .iter()
        .find(|(name, _)| *name == packaged)
        .or_else(|| (!exe.is_empty()).then(|| files.iter().find(|(_, text)| text.contains(&exe))).flatten())
        .and_then(|(_, text)| icon_from_desktop(text))
        .unwrap_or_else(|| DEFAULT_NAME.into())
}

// Written paths per icon name, so only files this app wrote are ever removed.
type Manifest = BTreeMap<String, Vec<PathBuf>>;

fn read_manifest() -> Manifest {
    fs::read(manifest_file())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn write_manifest(manifest: &Manifest) -> std::io::Result<()> {
    let file = manifest_file();
    if manifest.is_empty() {
        let _ = fs::remove_file(&file);
        return Ok(());
    }
    fs::create_dir_all(file.parent().unwrap_or(Path::new(".")))?;
    fs::write(file, serde_json::to_vec(manifest)?)
}

fn refresh() {
    // GTK desktops watch the theme directory's mtime; KDE listens for the signal.
    let now = std::time::SystemTime::now();
    if let Err(e) = fs::File::open(hicolor_dir()).and_then(|f| f.set_modified(now)) {
        eprintln!("could not touch the icon theme directory: {e}");
    }
    let signal = || -> zbus::Result<()> {
        zbus::blocking::Connection::session()?.emit_signal(
            None::<&str>,
            "/KIconLoader",
            "org.kde.KIconLoader",
            "iconChanged",
            &0_i32,
        )
    };
    if let Err(e) = signal() {
        eprintln!("could not send the icon change signal: {e}");
    }
}

pub fn installed() -> bool {
    supported() && read_manifest().contains_key(&icon_name())
}

fn write_sizes(image: &RgbaImage, name: &str, pending: &mut Vec<(PathBuf, PathBuf)>) -> Result<(), Box<dyn std::error::Error>> {
    let pid = std::process::id();
    for size in SIZES {
        let file = hicolor_dir().join(format!("{size}x{size}/apps/{name}.png"));
        let temp = file.with_extension(format!("png.tmp-{pid}"));
        fs::create_dir_all(file.parent().unwrap_or(Path::new(".")))?;
        resized(image, size).save_with_format(&temp, image::ImageFormat::Png)?;
        pending.push((temp, file));
    }
    for (temp, file) in pending.iter() {
        fs::rename(temp, file)?;
    }
    let mut manifest = read_manifest();
    manifest.insert(name.to_string(), pending.iter().map(|(_, f)| f.clone()).collect());
    write_manifest(&manifest)?;
    Ok(())
}

/// Every size goes to a temp name first and is renamed over the real file
/// only once all of them are written, so a failed replacement leaves the
/// previous icon set whole.
fn install(image: &RgbaImage) -> bool {
    if !supported() {
        return false;
    }
    let mut pending = Vec::new();
    if let Err(e) = write_sizes(image, &icon_name(), &mut pending) {
        eprintln!("could not write the icon theme, leaving it untouched: {e}");
        for (temp, _) in &pending {
            let _ = fs::remove_file(temp);
        }
        return false;
    }
    refresh();
    true
}

fn remove() -> bool {
    if !supported() {
        return false;
    }
    let mut manifest = read_manifest();
    let Some(files) = manifest.remove(&icon_name()) else { return false };
    for file in &files {
        let _ = fs::remove_file(file);
    }
    if let Err(e) = write_manifest(&manifest) {
        eprintln!("could not update the icon theme manifest: {e}");
        return false;
    }
    refresh();
    true
}

// Settings actions. All run on the main thread (GTK dialogs need it).

/// Parent for dialogs: the Settings window while it is open, so they open
/// in front of it, else the main window.
pub(crate) fn dialog_parent(app: &AppHandle) -> Option<gtk::Window> {
    crate::settings::window().or_else(|| app.get_window("main").and_then(|w| w.gtk_window().ok()).map(|w| w.upcast()))
}

pub(crate) fn message(app: &AppHandle, title: &str, kind: gtk::MessageType, buttons: gtk::ButtonsType, text: &str, secondary: &str) -> gtk::ResponseType {
    let mut builder = gtk::MessageDialog::builder()
        .title(title)
        .message_type(kind)
        .buttons(buttons)
        .text(text)
        .secondary_text(secondary);
    if let Some(parent) = dialog_parent(app) {
        builder = builder.transient_for(&parent);
    }
    let dialog = builder.build();
    let response = dialog.run();
    dialog.close();
    response
}

pub fn choose(app: &AppHandle) {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Images (PNG, SVG)"));
    filter.add_pattern("*.png");
    filter.add_pattern("*.svg");
    let dialog = gtk::FileChooserNative::new(
        Some("Choose App Icon"),
        dialog_parent(app).as_ref(),
        gtk::FileChooserAction::Open,
        Some("Open"),
        Some("Cancel"),
    );
    dialog.add_filter(filter);
    if dialog.run() != gtk::ResponseType::Accept {
        return;
    }
    let Some(path) = dialog.filename() else { return };
    let path = path.to_string_lossy().into_owned();
    let Some(image) = load_image(&path) else {
        message(app, "App Icon", gtk::MessageType::Error, gtk::ButtonsType::Ok, "That file could not be read as an image.", &path);
        return;
    };
    config::update(app, |c| c.app_icon = path);
    crate::apply_icon(app);
    if supported() {
        let yes = message(
            app,
            "App Icon",
            gtk::MessageType::Question,
            gtk::ButtonsType::YesNo,
            "Also use it for the taskbar and launcher?",
            "This copies the icon into your icon theme under ~/.local/share/icons. It shows on the next start, on some desktops after you log in again. You can change this later under Settings.",
        ) == gtk::ResponseType::Yes;
        if yes {
            install(&image);
        } else {
            remove();
        }
    }
}

pub fn toggle_theme(app: &AppHandle) {
    if installed() {
        remove();
    } else if let Some(image) = load_image(&config::load(app).app_icon) {
        if install(&image) {
            message(
                app,
                "App Icon",
                gtk::MessageType::Info,
                gtk::ButtonsType::Ok,
                "The taskbar and launcher pick up the icon on the next start.",
                "Some desktops only refresh it after you log in again.",
            );
        }
    }
}

pub fn reset(app: &AppHandle) {
    config::update(app, |c| c.app_icon.clear());
    crate::apply_icon(app);
    remove();
}

#[cfg(test)]
mod tests {
    use super::{icon_from_desktop, load_image, resized, with_unread_dot};

    #[test]
    fn svg_decodes_through_pixbuf() {
        let svg = load_image(concat!(env!("CARGO_MANIFEST_DIR"), "/icons/icon.svg")).expect("librsvg pixbuf loader");
        assert_eq!(svg.dimensions(), (512, 512));
        assert_eq!(svg.get_pixel(256, 256).0[3], 255);
        let png = load_image(concat!(env!("CARGO_MANIFEST_DIR"), "/icons/32x32.png")).expect("png");
        assert_eq!(png.dimensions(), (32, 32));
        assert!(load_image("/nonexistent.svg").is_none());
    }

    #[test]
    fn icon_name_from_desktop_entry() {
        assert_eq!(icon_from_desktop("[Desktop Entry]\nIcon=outlook-for-linux\nName=x\n").as_deref(), Some("outlook-for-linux"));
        assert_eq!(icon_from_desktop("Icon=appimagekit_abc-outlook.for.linux\n").as_deref(), Some("appimagekit_abc-outlook.for.linux"));
        // Paths and odd characters never become a theme name.
        assert_eq!(icon_from_desktop("Icon=/usr/share/pixmaps/x.png\n"), None);
        assert_eq!(icon_from_desktop("Name=no icon\n"), None);
    }

    #[test]
    fn resize_keeps_aspect_inside_square() {
        let wide = image::RgbaImage::from_pixel(200, 100, image::Rgba([0, 0, 255, 255]));
        let out = resized(&wide, 64);
        assert_eq!(out.dimensions(), (64, 64));
        assert_eq!(out.get_pixel(32, 32).0[3], 255);
        assert_eq!(out.get_pixel(32, 2).0[3], 0);
        let badged = with_unread_dot(&wide);
        assert_eq!(badged.get_pixel(100, 28).0, [232, 17, 35, 255]);
    }
}
