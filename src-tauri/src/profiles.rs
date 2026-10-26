//! Profiles: one OWA webview per profile inside the main window, each with
//! its own WebKit data directory (cookies, storage, cache), so each profile
//! is its own signed-in account. Only the selected profile is shown. A
//! deselected one stays loaded for `unload_after_minutes`, then its webview
//! is closed and the memory goes with it; selecting it again reloads OWA
//! from the kept cookies. The switcher is a GTK item on the menubar.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use gtk::glib;
use gtk::prelude::*;
use tauri::{AppHandle, Manager, Webview, Window};

use crate::config::{self, Config, Profile};

pub(crate) const DOT_SELECTED: &str = "#27ae60";
pub(crate) const DOT_LOADED: &str = "#f39c12";
pub(crate) const DOT_UNLOADED: &str = "#7f8c8d";

#[derive(Default)]
pub struct State {
    active: String,
    /// When each deselected, still loaded profile was last shown.
    hidden_since: HashMap<String, Instant>,
    /// Last unread count reported by each profile's title watcher.
    unread: HashMap<String, u32>,
}

thread_local! {
    /// Menubar item, its label, and its one persistent dropdown menu.
    static SWITCHER: RefCell<Option<(gtk::MenuItem, gtk::Label, gtk::Menu)>> = const { RefCell::new(None) };
    /// A refresh arrived while the dropdown was open; redo it once it closes.
    static DIRTY: Cell<bool> = const { Cell::new(false) };
}

fn with_state<T>(app: &AppHandle, f: impl FnOnce(&mut State) -> T) -> T {
    let state = app.state::<Mutex<State>>();
    let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);
    f(&mut guard)
}

pub fn webview_label(id: &str) -> String {
    format!("profile-{id}")
}

/// The default profile keeps the directory Tauri used before profiles
/// existed, so the current sign-in carries over.
pub fn data_dir(app: &AppHandle, id: &str) -> Option<PathBuf> {
    let base = app.path().app_local_data_dir().ok()?;
    Some(if id == config::DEFAULT_PROFILE { base } else { base.join("profiles").join(id) })
}

pub fn active_id(app: &AppHandle) -> String {
    with_state(app, |s| s.active.clone())
}

pub fn is_loaded(app: &AppHandle, id: &str) -> bool {
    app.get_webview(&webview_label(id)).is_some()
}

/// Last unread count the profile reported; 0 when unknown.
pub fn unread(app: &AppHandle, id: &str) -> u32 {
    with_state(app, |s| s.unread.get(id).copied().unwrap_or(0))
}

pub fn active_webview(app: &AppHandle) -> Option<Webview> {
    app.get_webview(&webview_label(&active_id(app)))
}

/// Called once in setup, before the main window is opened.
pub fn init(app: &AppHandle) {
    let active = config::load(app).active_profile;
    app.manage(Mutex::new(State { active, ..State::default() }));
    let handle = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(60));
        let app = handle.clone();
        let _ = handle.run_on_main_thread(move || unload_expired(&app));
    });
}

fn title(config: &Config, id: &str) -> String {
    match config.profile(id) {
        Some(p) if config.profiles.len() > 1 => format!("Outlook: {}", p.name),
        _ => "Outlook".into(),
    }
}

/// Prefix desktop notifications with the profile name once there is more
/// than one profile to tell apart.
pub fn notification_title(app: &AppHandle, id: &str, summary: String) -> String {
    let config = config::load(app);
    match config.profile(id) {
        Some(p) if config.profiles.len() > 1 => format!("{}: {summary}", p.name),
        _ => summary,
    }
}

/// Show profile `id`, loading it if needed, and hide the one shown before.
pub fn activate(app: &AppHandle, id: &str) {
    let config = config::load(app);
    if config.profile(id).is_none() {
        return;
    }
    let Some(window) = app.get_window("main") else { return };
    let previous = active_id(app);
    let previous_webview = (previous != id).then(|| app.get_webview(&webview_label(&previous))).flatten();
    // Hide first so the two never share the window for a frame.
    if let Some(old) = &previous_webview {
        let _ = old.hide();
    }
    let webview = match app.get_webview(&webview_label(id)) {
        Some(w) => w,
        None => match load_webview(app, &window, id) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("could not load profile {id}: {e}");
                if let Some(old) = &previous_webview {
                    let _ = old.show();
                }
                return;
            }
        },
    };
    let _ = webview.show();
    let _ = webview.set_focus();
    let unread = with_state(app, |s| {
        s.active = id.to_string();
        s.hidden_since.remove(id);
        if previous_webview.is_some() {
            s.hidden_since.insert(previous.clone(), Instant::now());
        }
        s.unread.get(id).copied().unwrap_or(0)
    });
    if previous != id {
        config::update(app, |c| c.active_profile = id.to_string());
        if config.unload_after() == Some(Duration::ZERO) && !keeps_awake(&config, &previous) {
            unload(app, &previous);
        }
    }
    crate::update_tray(app, unread);
    let _ = window.set_title(&title(&config, id));
    refresh_switcher(app);
}

/// Hide every loaded profile but the active one. Needed after the main
/// window's show(), which shows all children.
pub fn hide_inactive(app: &AppHandle) {
    let active = active_id(app);
    for profile in config::load(app).profiles {
        if profile.id != active {
            if let Some(w) = app.get_webview(&webview_label(&profile.id)) {
                let _ = w.hide();
            }
        }
    }
}

fn load_webview(app: &AppHandle, window: &Window, id: &str) -> tauri::Result<Webview> {
    let mut builder = crate::owa_webview(&webview_label(id), crate::OWA_URL);
    if let Some(dir) = data_dir(app, id) {
        builder = builder.data_directory(dir);
    }
    let webview = window.add_child(builder, tauri::LogicalPosition::new(0.0, 0.0), tauri::LogicalSize::new(1280.0, 800.0))?;
    crate::configure_webview(&webview, Some(id))?;
    Ok(webview)
}

fn keeps_awake(config: &Config, id: &str) -> bool {
    config.profile(id).is_some_and(|p| p.keep_awake)
}

/// Close the child webview; the data directory stays, so the next load is
/// signed in. The web process is killed first: WebKit otherwise parks a
/// closed page's process in its process cache for up to half an hour, with
/// the page's memory still in it, which is exactly what unloading is for.
fn unload(app: &AppHandle, id: &str) {
    if let Some(webview) = app.get_webview(&webview_label(id)) {
        #[cfg(target_os = "linux")]
        let _ = webview.with_webview(|w| {
            use webkit2gtk::WebViewExt;
            w.inner().terminate_web_process();
        });
        let _ = webview.close();
    }
    with_state(app, |s| {
        s.hidden_since.remove(id);
    });
}

fn unload_expired(app: &AppHandle) {
    let config = config::load(app);
    let Some(limit) = config.unload_after() else { return };
    let due: Vec<String> = with_state(app, |s| {
        s.hidden_since
            .iter()
            .filter(|(id, since)| **id != s.active && since.elapsed() >= limit && !keeps_awake(&config, id))
            .map(|(id, _)| id.clone())
            .collect()
    });
    if due.is_empty() {
        return;
    }
    for id in &due {
        unload(app, id);
    }
    refresh_switcher(app);
}

/// Title watcher callback: the tray follows the selected profile only, the
/// dropdown shows the rest.
pub fn report_unread(app: &AppHandle, id: &str, count: u32) {
    let is_active = with_state(app, |s| {
        s.unread.insert(id.to_string(), count);
        s.active == id
    });
    if is_active {
        crate::update_tray(app, count);
    }
    refresh_switcher(app);
}

/// Append the right-justified "Profile: … ▾" item to muda's menubar.
pub fn install_switcher(app: &AppHandle, menubar: &gtk::MenuBar) {
    let item = gtk::MenuItem::new();
    let label = gtk::Label::new(None);
    item.add(&label);
    #[allow(deprecated)]
    item.set_right_justified(true);
    // One menu for the item's lifetime, entries replaced in place. Never
    // call show on the menu itself: GTK does that on popup, and is_visible
    // is how refresh_switcher knows the dropdown is open.
    let menu = gtk::Menu::new();
    item.set_submenu(Some(&menu));
    let on_close = app.clone();
    menu.connect_hide(move |_| {
        if DIRTY.get() {
            let app = on_close.clone();
            glib::idle_add_local_once(move || refresh_switcher(&app));
        }
    });
    item.show_all();
    menubar.append(&item);
    SWITCHER.with(|s| *s.borrow_mut() = Some((item, label, menu)));
    refresh_switcher(app);
}

/// Rebuild the label and dropdown from config and state. Cheap, and called
/// after every change that shows there (switch, unload, unread count).
/// Entries are only replaced while the dropdown is closed: replacing them
/// under the pointer destroys the menu holding GTK's grab, and the app
/// stops taking input.
pub fn refresh_switcher(app: &AppHandle) {
    let Some((_, label, menu)) = SWITCHER.with(|s| s.borrow().clone()) else { return };
    let config = config::load(app);
    let (active, unread) = with_state(app, |s| (s.active.clone(), s.unread.clone()));
    let active_name = config.profile(&active).map_or(active.as_str(), |p| p.name.as_str());
    label.set_text(&format!("Profile: {active_name} ▾"));

    if menu.is_visible() {
        DIRTY.set(true);
        return;
    }
    DIRTY.set(false);
    for child in menu.children() {
        menu.remove(&child);
    }
    for profile in &config.profiles {
        let selected = profile.id == active;
        let loaded = app.get_webview(&webview_label(&profile.id)).is_some();
        let colour = if selected { DOT_SELECTED } else if loaded { DOT_LOADED } else { DOT_UNLOADED };
        let mut text = glib::markup_escape_text(&profile.name).to_string();
        if let Some(n) = unread.get(&profile.id).filter(|n| config.notifications.switcher_unread && **n > 0 && !selected && loaded) {
            text.push_str(&format!(" ({n})"));
        }
        let entry = gtk::CheckMenuItem::new();
        entry.set_draw_as_radio(true);
        // Before connect_activate: set_active fires the signal too.
        entry.set_active(selected);
        let text_label = gtk::Label::new(None);
        text_label.set_markup(&format!("<span foreground=\"{colour}\">\u{25cf}</span>  {text}"));
        text_label.set_xalign(0.0);
        entry.add(&text_label);
        let (app, id) = (app.clone(), profile.id.clone());
        // Deferred: the handler would otherwise replace this very menu while
        // GTK is still delivering its activation.
        entry.connect_activate(move |_| {
            let (app, id) = (app.clone(), id.clone());
            glib::idle_add_local_once(move || activate(&app, &id));
        });
        entry.show_all();
        menu.append(&entry);
    }
    let separator = gtk::SeparatorMenuItem::new();
    separator.show();
    menu.append(&separator);
    let add = gtk::MenuItem::with_label("Add Profile\u{2026}");
    let app = app.clone();
    add.connect_activate(move |_| {
        let app = app.clone();
        glib::idle_add_local_once(move || add_profile(&app));
    });
    add.show_all();
    menu.append(&add);
}

pub fn add_profile(app: &AppHandle) {
    let dialog = gtk::Dialog::with_buttons(
        Some("Add Profile"),
        crate::app_icon::dialog_parent(app).as_ref(),
        gtk::DialogFlags::MODAL,
        &[("Cancel", gtk::ResponseType::Cancel), ("Add", gtk::ResponseType::Accept)],
    );
    dialog.set_default_response(gtk::ResponseType::Accept);
    let content = dialog.content_area();
    content.set_border_width(12);
    content.set_spacing(6);
    let hint = gtk::Label::new(Some("A profile is a separate sign-in with its own cookies and storage."));
    hint.set_xalign(0.0);
    content.add(&hint);
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("Name, for example Personal"));
    entry.set_activates_default(true);
    content.add(&entry);
    content.show_all();
    let response = dialog.run();
    let name = entry.text().trim().to_string();
    dialog.close();
    if response != gtk::ResponseType::Accept || name.is_empty() {
        return;
    }
    let existing: Vec<String> = config::load(app).profiles.into_iter().map(|p| p.id).collect();
    let id = unique_id(&name, &existing);
    config::update(app, |c| c.profiles.push(Profile::new(id.clone(), name)));
    activate(app, &id);
}

pub fn rename(app: &AppHandle, id: &str, name: String) {
    let config = config::update(app, |c| {
        if let Some(p) = c.profiles.iter_mut().find(|p| p.id == id) {
            p.name = name;
        }
    });
    if let Some(window) = app.get_window("main") {
        let _ = window.set_title(&title(&config, &active_id(app)));
    }
    refresh_switcher(app);
}

pub fn set_notifications(app: &AppHandle, id: &str, notifications: bool) {
    config::update(app, |c| {
        if let Some(p) = c.profiles.iter_mut().find(|p| p.id == id) {
            p.notifications = notifications;
        }
    });
}

pub fn set_keep_awake(app: &AppHandle, id: &str, keep_awake: bool) {
    config::update(app, |c| {
        if let Some(p) = c.profiles.iter_mut().find(|p| p.id == id) {
            p.keep_awake = keep_awake;
        }
    });
}

/// Delete a profile and its sign-in. The last profile cannot be removed.
/// The default profile shares its directory with `profiles/`, so only its
/// own entries are deleted there.
pub fn remove(app: &AppHandle, id: &str) {
    let config = config::load(app);
    if config.profiles.len() < 2 || config.profile(id).is_none() {
        return;
    }
    if active_id(app) == id {
        if let Some(other) = config.profiles.iter().find(|p| p.id != id) {
            activate(app, &other.id);
        }
    }
    unload(app, id);
    with_state(app, |s| {
        s.unread.remove(id);
    });
    if let Some(dir) = data_dir(app, id) {
        let result = if id == config::DEFAULT_PROFILE {
            std::fs::read_dir(&dir).map(|entries| {
                for entry in entries.flatten() {
                    if entry.file_name() != "profiles" {
                        let path = entry.path();
                        let _ = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
                    }
                }
            })
        } else {
            std::fs::remove_dir_all(&dir)
        };
        if let Err(e) = result {
            eprintln!("could not delete profile data: {e}");
        }
    }
    let config = config::update(app, |c| c.profiles.retain(|p| p.id != id));
    if let Some(window) = app.get_window("main") {
        let _ = window.set_title(&title(&config, &active_id(app)));
    }
    refresh_switcher(app);
}

/// Directory-safe id from a display name: lowercase ASCII letters and
/// digits, runs of anything else become one dash.
fn slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() { "profile".into() } else { out.into() }
}

/// Slug plus a random suffix. Ids are never reused: Tauri keeps a
/// profile's WebContext (and its in-memory cookie jar) alive for the life
/// of the app, so a removed profile re-added under the same directory
/// would come back signed in as the old account.
fn unique_id(name: &str, existing: &[String]) -> String {
    let base = slug(name);
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos())
        ^ std::process::id();
    loop {
        let candidate = format!("{base}-{:04x}", seed & 0xffff);
        if !existing.contains(&candidate) {
            return candidate;
        }
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_directory_safe_and_unique() {
        assert_eq!(slug("Work (Bravo) / 2026"), "work-bravo-2026");
        assert_eq!(slug("Æø"), "profile");
        let id = unique_id("Personal", &[]);
        assert!(id.starts_with("personal-") && id.len() == "personal-".len() + 4, "{id}");
        assert_ne!(unique_id("Personal", &[id.clone()]), id);
    }
}
