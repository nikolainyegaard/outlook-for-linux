//! Settings window (Edit > Settings), laid out like Outlook's: categories
//! with icons and a search box on the left, the category's entries in the
//! middle, the selected page on the right. Under Profiles the middle column
//! lists the profiles themselves, each with an avatar and status, plus Add
//! profile; under General it lists fixed pages. Every control applies at
//! once, there is no OK/Cancel. Pages are rebuilt from config after a
//! change, so widgets are created in their final state and never fire
//! handlers while being set. Styling follows the system theme (Breeze
//! light and dark): only theme colours are referenced in the CSS below.

use std::cell::RefCell;

use gtk::prelude::*;
use tauri::{AppHandle, Manager};

use crate::{app_icon, config, profiles};

type Build = fn(&AppHandle, &gtk::Box) -> gtk::Widget;

/// Name and icon candidates, first one the icon theme has wins. The
/// Profiles category's middle column lists the profiles themselves.
const CATEGORIES: [(&str, &[&str]); 2] = [
    ("General", &["preferences-system-symbolic", "preferences-system", "configure"]),
    ("Profiles", &["avatar-default-symbolic", "user-identity", "system-users"]),
];
const GENERAL: i32 = 0;
const PROFILES: i32 = 1;

/// The fixed pages under General.
struct Page {
    title: &'static str,
    /// Extra words the search box matches besides the title.
    keywords: &'static str,
    build: Build,
}

const PAGES: [Page; 3] = [
    Page { title: "Notifications", keywords: "toast sound preview badge mute alert new mail", build: build_notifications },
    Page { title: "Background profiles", keywords: "unload memory ram timeout minutes", build: build_unload },
    Page { title: "App icon", keywords: "icon picture image taskbar launcher tray logo", build: build_icon },
];

const ADD_KEY: &str = "add";

/// (minutes or never, label, hint on the right)
const UNLOAD_CHOICES: [(Option<u32>, &str, &str); 6] = [
    (Some(0), "Immediately", "Least memory"),
    (Some(5), "After 5 minutes", ""),
    (Some(15), "After 15 minutes", "Default"),
    (Some(30), "After 30 minutes", ""),
    (Some(60), "After 1 hour", ""),
    (None, "Never", "Most memory"),
];

/// Avatar fills, picked per profile from its id. All carry white initials
/// at better than 4.5:1.
const AVATAR_COLOURS: [&str; 6] = ["#2a7ab8", "#9b4fb0", "#1e7a45", "#b4502a", "#22777a", "#a8335f"];

// Navigation columns sit on the window background, the page on the view
// (base) colour, like Outlook's grey navigation next to a white page. The
// selected row gets a light accent tint instead of the theme's solid
// selection fill, with no edge shadow.
const CSS: &str = "
list.settings-nav, list.settings-nav row { background-color: transparent; background-image: none; border: none; box-shadow: none; outline: none; }
list.settings-nav row { border-radius: 6px; margin: 1px 0; padding: 7px 10px; min-height: 0; }
list.settings-nav row:hover { background-color: alpha(@theme_fg_color, 0.07); }
list.settings-nav row:selected { background-color: alpha(@theme_selected_bg_color, 0.22); color: @theme_fg_color; }
list.settings-nav row:selected label { color: @theme_fg_color; }
list.settings-nav row:selected label.settings-row-title { font-weight: 600; }
list.settings-nav row label.settings-add { color: @link_color; font-weight: 500; }
.settings-page { background-color: @theme_base_color; }
.settings-title { font-size: 22px; font-weight: 600; }
.settings-page-title { font-size: 22px; font-weight: 600; }
.settings-section { font-size: 14px; font-weight: 600; }
.settings-small { font-size: 12px; }
.settings-avatar { border-radius: 999px; color: #ffffff; font-weight: 600; min-width: 30px; min-height: 30px; padding: 0; }
.settings-avatar.large { min-width: 52px; min-height: 52px; font-size: 20px; }
.settings-avatar-add { border-radius: 999px; border: 1px dashed @link_color; color: @link_color; min-width: 28px; min-height: 28px; font-weight: 600; }
frame.settings-group > border { border-radius: 8px; }
list.settings-group, list.settings-group row { background-color: transparent; background-image: none; }
list.settings-group row { padding: 0 12px; min-height: 40px; border-bottom: 1px solid alpha(@theme_fg_color, 0.12); }
list.settings-group row:last-child { border-bottom: none; }
";

/// The three navigation widgets, so pages can re-list profiles after a
/// rename, add or remove.
#[derive(Clone)]
struct Nav {
    categories: gtk::ListBox,
    pages: gtk::ListBox,
    search: gtk::SearchEntry,
}

thread_local! {
    static WINDOW: RefCell<Option<gtk::Window>> = const { RefCell::new(None) };
    static NAV: RefCell<Option<Nav>> = const { RefCell::new(None) };
    static STYLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The Settings window while it is open.
pub fn window() -> Option<gtk::Window> {
    WINDOW.with(|w| w.borrow().clone())
}

/// Close Settings if it is open. Called before the main window hides.
pub fn close() {
    if let Some(w) = window() {
        w.close();
    }
}

fn unload_id(value: Option<u32>) -> String {
    value.map_or("never".into(), |m| m.to_string())
}

fn parse_unload_id(id: &str) -> Option<u32> {
    id.parse().ok()
}

/// Screen-wide, once: the selectors are scoped by the settings-* classes,
/// so nothing else in the app is affected. Per-widget providers would not
/// reach the rows inside the lists.
fn install_css() {
    if STYLED.get() {
        return;
    }
    let provider = gtk::CssProvider::new();
    if provider.load_from_data(CSS.as_bytes()).is_ok() {
        if let Some(screen) = gtk::gdk::Screen::default() {
            gtk::StyleContext::add_provider_for_screen(&screen, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
            STYLED.set(true);
        }
    }
}

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    for class in classes {
        label.style_context().add_class(class);
    }
    label
}

fn markup(text: &str, classes: &[&str]) -> gtk::Label {
    let label = label("", classes);
    label.set_markup(text);
    label
}

fn escape(text: &str) -> String {
    gtk::glib::markup_escape_text(text).to_string()
}

fn hint(text: &str) -> gtk::Label {
    let label = label(text, &["dim-label", "settings-small"]);
    label.set_line_wrap(true);
    label.set_max_width_chars(60);
    label
}

fn section(text: &str) -> gtk::Label {
    label(text, &["settings-section"])
}

fn category_icon(names: &[&str]) -> gtk::Image {
    let theme = gtk::IconTheme::default();
    let name = names
        .iter()
        .find(|n| theme.as_ref().is_some_and(|t| t.has_icon(n)))
        .unwrap_or(&names[0]);
    gtk::Image::from_icon_name(Some(name), gtk::IconSize::Menu)
}

/// Initials in a coloured circle; the colour follows the id, so it stays
/// put across renames.
fn avatar(profile: &config::Profile, large: bool) -> gtk::Label {
    let initial: String = profile.name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into());
    let index = profile.id.bytes().fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(usize::from(b))) % AVATAR_COLOURS.len();
    let label = gtk::Label::new(None);
    // A per-widget provider paints the whole circle in the profile's colour.
    let provider = gtk::CssProvider::new();
    let _ = provider.load_from_data(format!("label {{ background-color: {}; }}", AVATAR_COLOURS[index]).as_bytes());
    label.set_markup(&escape(&initial));
    let context = label.style_context();
    context.add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    context.add_class("settings-avatar");
    if large {
        context.add_class("large");
    }
    label.set_valign(gtk::Align::Center);
    label
}

/// Dot colour and wording, same as the menubar dropdown.
fn status(app: &AppHandle, id: &str) -> (&'static str, String) {
    if profiles::active_id(app) == id {
        (profiles::DOT_SELECTED, "Selected".into())
    } else if profiles::is_loaded(app, id) {
        let unread = profiles::unread(app, id);
        let text = if unread > 0 { format!("Loaded, {unread} unread") } else { "Loaded in the background".into() };
        (profiles::DOT_LOADED, text)
    } else {
        (profiles::DOT_UNLOADED, "Unloaded".into())
    }
}

fn status_label(app: &AppHandle, id: &str, classes: &[&str]) -> gtk::Label {
    let (colour, text) = status(app, id);
    markup(&format!("<span foreground=\"{colour}\">\u{25cf}</span> {}", escape(&text)), classes)
}

fn profile_key(id: &str) -> String {
    format!("profile:{id}")
}

fn profile_entry(app: &AppHandle, profile: &config::Profile) -> (String, gtk::Widget) {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    content.pack_start(&avatar(profile, false), false, false, 0);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
    text.set_valign(gtk::Align::Center);
    let name = label(&profile.name, &["settings-row-title"]);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.pack_start(&name, false, false, 0);
    text.pack_start(&status_label(app, &profile.id, &["dim-label", "settings-small"]), false, false, 0);
    content.pack_start(&text, true, true, 0);
    (profile_key(&profile.id), content.upcast())
}

fn add_entry() -> (String, gtk::Widget) {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let plus = gtk::Label::new(Some("+"));
    plus.style_context().add_class("settings-avatar-add");
    plus.set_valign(gtk::Align::Center);
    content.pack_start(&plus, false, false, 0);
    content.pack_start(&label("Add profile", &["settings-add"]), true, true, 0);
    (ADD_KEY.into(), content.upcast())
}

fn page_entry(index: usize) -> (String, gtk::Widget) {
    let text = label(PAGES[index].title, &["settings-row-title"]);
    text.set_margin_top(2);
    text.set_margin_bottom(2);
    (format!("page:{index}"), text.upcast())
}

fn profile_entries(app: &AppHandle) -> Vec<(String, gtk::Widget)> {
    let mut entries: Vec<_> = config::load(app).profiles.iter().map(|p| profile_entry(app, p)).collect();
    entries.push(add_entry());
    entries
}

fn general_entries() -> Vec<(String, gtk::Widget)> {
    (0..PAGES.len()).map(page_entry).collect()
}

/// Select the Profiles category and list the profiles, opening `select`
/// (a profile id) or the first profile.
fn show_profiles(app: &AppHandle, select: Option<&str>) {
    let Some(nav) = NAV.with(|n| n.borrow().clone()) else { return };
    if !nav.search.text().is_empty() {
        nav.search.set_text("");
    }
    nav.categories.select_row(nav.categories.row_at_index(PROFILES).as_ref());
    fill(&nav.pages, profile_entries(app), select.map(profile_key).as_deref());
}

pub fn open(app: &AppHandle) {
    open_at(app, None);
}

/// Open Settings on `key`: a middle-column row key (`profile:<id>`,
/// `page:<index>`), or the first profile when None.
pub fn open_at(app: &AppHandle, key: Option<&str>) {
    if let Some(existing) = window() {
        existing.present();
        if let Some(key) = key {
            select_key(app, key);
        }
        return;
    }
    install_css();

    let win = gtk::Window::new(gtk::WindowType::Toplevel);
    win.set_title("Settings");
    win.set_default_size(900, 600);
    // Not modal: KWin routes activation and close requests for a window
    // to its modal child, so a modal Settings swallowed the taskbar Close
    // meant for Outlook. Non-modal also lets mail be used meanwhile.
    win.set_type_hint(gtk::gdk::WindowTypeHint::Dialog);
    if let Some(parent) = app.get_window("main").and_then(|w| w.gtk_window().ok()) {
        win.set_transient_for(Some(&parent));
    }
    win.connect_key_press_event(|w, event| {
        if event.keyval() == gtk::gdk::keys::constants::Escape {
            w.close();
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });
    win.connect_destroy(|_| {
        WINDOW.with(|w| *w.borrow_mut() = None);
        NAV.with(|n| *n.borrow_mut() = None);
    });

    // Left: title, search, categories.
    let left = gtk::Box::new(gtk::Orientation::Vertical, 14);
    left.set_border_width(16);
    left.set_size_request(210, -1);
    let title = label("Settings", &["settings-title"]);
    title.set_margin_start(8);
    left.pack_start(&title, false, false, 0);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search settings"));
    left.pack_start(&search, false, false, 0);
    let categories = gtk::ListBox::new();
    categories.style_context().add_class("settings-nav");
    for (name, icons) in CATEGORIES {
        let row = gtk::ListBoxRow::new();
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        content.pack_start(&category_icon(icons), false, false, 0);
        content.pack_start(&label(name, &["settings-row-title"]), true, true, 0);
        row.add(&content);
        categories.add(&row);
    }
    left.pack_start(&categories, false, false, 0);

    // Middle: the category's entries, or the search hits.
    let pages = gtk::ListBox::new();
    pages.style_context().add_class("settings-nav");
    let none = label("No settings found", &["dim-label"]);
    none.set_margin_top(12);
    none.set_xalign(0.5);
    none.show();
    pages.set_placeholder(Some(&none));
    // GtkListBox ignores border-width, so the inset is margins.
    pages.set_margin_top(16);
    pages.set_margin_bottom(16);
    pages.set_margin_start(12);
    pages.set_margin_end(12);
    pages.set_size_request(230, -1);
    pages.set_valign(gtk::Align::Start);

    // Right: the selected page. The shared heading is for General pages;
    // a profile page draws its own header with the avatar.
    let page = gtk::Box::new(gtk::Orientation::Vertical, 18);
    page.set_margin_top(28);
    page.set_margin_bottom(28);
    page.set_margin_start(36);
    page.set_margin_end(36);
    let heading = label("", &["settings-page-title"]);
    page.pack_start(&heading, false, false, 0);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.pack_start(&body, true, true, 0);
    let scroller = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).child(&page).build();
    scroller.style_context().add_class("settings-page");

    let layout = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    layout.pack_start(&left, false, false, 0);
    layout.pack_start(&gtk::Separator::new(gtk::Orientation::Vertical), false, false, 0);
    layout.pack_start(&pages, false, false, 0);
    layout.pack_start(&gtk::Separator::new(gtk::Orientation::Vertical), false, false, 0);
    layout.pack_start(&scroller, true, true, 0);
    win.add(&layout);

    let (a, b, h) = (app.clone(), body.clone(), heading.clone());
    pages.connect_row_selected(move |_, row| {
        let Some(row) = row else { return };
        let key = row.widget_name().to_string();
        if key == ADD_KEY {
            // Deferred: the dialog runs a nested loop, and the list is
            // refilled afterwards, which must not happen inside this signal.
            let a = a.clone();
            gtk::glib::idle_add_local_once(move || {
                let before = config::load(&a).profiles.len();
                profiles::add_profile(&a);
                let added = config::load(&a).profiles.len() > before;
                show_profiles(&a, added.then(|| profiles::active_id(&a)).as_deref());
            });
        } else if let Some(id) = key.strip_prefix("profile:") {
            h.hide();
            for child in b.children() {
                b.remove(&child);
            }
            b.pack_start(&profile_page(&a, id), true, true, 0);
            b.show_all();
        } else if let Some(page) = key.strip_prefix("page:").and_then(|i| i.parse::<usize>().ok()).and_then(|i| PAGES.get(i)) {
            h.set_text(page.title);
            h.show();
            refresh(&a, &b, page.build);
        }
    });
    let (a, list, search_box) = (app.clone(), pages.clone(), search.clone());
    categories.connect_row_selected(move |_, row| {
        let Some(row) = row else { return };
        if !search_box.text().is_empty() {
            search_box.set_text("");
        }
        let entries = if row.index() == PROFILES { profile_entries(&a) } else { general_entries() };
        fill(&list, entries, None);
    });
    let (a, list, cats) = (app.clone(), pages.clone(), categories.clone());
    search.connect_search_changed(move |entry| {
        let query = entry.text().trim().to_lowercase();
        if query.is_empty() {
            let profiles = cats.selected_row().is_some_and(|r| r.index() == PROFILES);
            fill(&list, if profiles { profile_entries(&a) } else { general_entries() }, None);
            return;
        }
        cats.unselect_all();
        let all_profiles = "profiles accounts".contains(&query);
        let mut entries: Vec<_> = config::load(&a)
            .profiles
            .iter()
            .filter(|p| all_profiles || p.name.to_lowercase().contains(&query))
            .map(|p| profile_entry(&a, p))
            .collect();
        entries.extend(
            PAGES
                .iter()
                .enumerate()
                .filter(|(_, p)| p.title.to_lowercase().contains(&query) || p.keywords.contains(&query))
                .map(|(i, _)| page_entry(i)),
        );
        fill(&list, entries, None);
    });

    NAV.with(|n| *n.borrow_mut() = Some(Nav { categories: categories.clone(), pages: pages.clone(), search: search.clone() }));
    categories.select_row(categories.row_at_index(GENERAL).as_ref());
    WINDOW.with(|w| *w.borrow_mut() = Some(win.clone()));
    win.show_all();
    if let Some(key) = key {
        select_key(app, key);
    }
}

fn select_key(app: &AppHandle, key: &str) {
    let Some(nav) = NAV.with(|n| n.borrow().clone()) else { return };
    if let Some(id) = key.strip_prefix("profile:") {
        show_profiles(app, Some(id));
        return;
    }
    nav.categories.select_row(nav.categories.row_at_index(GENERAL).as_ref());
    let row = nav.pages.children().into_iter().filter_map(|c| c.downcast::<gtk::ListBoxRow>().ok()).find(|r| r.widget_name() == key);
    nav.pages.select_row(row.as_ref());
}

/// Replace the middle column with `entries` (row key, row content) and
/// open the one keyed `select`, or the first.
fn fill(list: &gtk::ListBox, entries: Vec<(String, gtk::Widget)>, select: Option<&str>) {
    for child in list.children() {
        list.remove(&child);
    }
    let mut target = None;
    for (key, content) in entries {
        let row = gtk::ListBoxRow::new();
        row.add(&content);
        row.set_widget_name(&key);
        if select == Some(key.as_str()) {
            target = Some(row.clone());
        }
        list.add(&row);
    }
    list.show_all();
    list.select_row(target.or_else(|| list.row_at_index(0)).as_ref());
}

fn refresh(app: &AppHandle, container: &gtk::Box, build: Build) {
    for child in container.children() {
        container.remove(&child);
    }
    let content = build(app, container);
    container.pack_start(&content, true, true, 0);
    container.show_all();
}

/// A titled block above a thin rule, the way Outlook separates groups.
fn block(outer: &gtk::Box, content: &impl IsA<gtk::Widget>) {
    outer.pack_start(&gtk::Separator::new(gtk::Orientation::Horizontal), false, false, 0);
    outer.pack_start(content, false, false, 0);
}

fn build_icon(app: &AppHandle, container: &gtk::Box) -> gtk::Widget {
    let config = config::load(app);
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 18);
    outer.pack_start(&hint("The icon in the tray, with the unread badge drawn on top. PNG and SVG both work."), false, false, 0);

    let current = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    let preview = if config.app_icon.is_empty() {
        let image = gtk::Image::from_icon_name(Some("outlook-for-linux"), gtk::IconSize::Dialog);
        image.set_pixel_size(48);
        image
    } else {
        gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(&config.app_icon, 48, 48, true)
            .map(|p| gtk::Image::from_pixbuf(Some(&p)))
            .unwrap_or_else(|_| gtk::Image::from_icon_name(Some("image-missing"), gtk::IconSize::Dialog))
    };
    current.pack_start(&preview, false, false, 0);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_valign(gtk::Align::Center);
    text.pack_start(&section(if config.app_icon.is_empty() { "Bundled icon" } else { "Custom icon" }), false, false, 0);
    if !config.app_icon.is_empty() {
        let path = label(&config.app_icon, &["dim-label", "settings-small"]);
        path.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        text.pack_start(&path, false, false, 0);
    }
    current.pack_start(&text, true, true, 0);
    let choose = gtk::Button::with_label("Choose\u{2026}");
    choose.set_valign(gtk::Align::Center);
    let (a, c) = (app.clone(), container.clone());
    choose.connect_clicked(move |_| {
        let (a, c) = (a.clone(), c.clone());
        gtk::glib::idle_add_local_once(move || {
            app_icon::choose(&a);
            refresh(&a, &c, build_icon);
        });
    });
    current.pack_start(&choose, false, false, 0);
    if !config.app_icon.is_empty() {
        let reset = gtk::Button::with_label("Reset");
        reset.set_valign(gtk::Align::Center);
        let (a, c) = (app.clone(), container.clone());
        reset.connect_clicked(move |_| {
            let (a, c) = (a.clone(), c.clone());
            gtk::glib::idle_add_local_once(move || {
                app_icon::reset(&a);
                refresh(&a, &c, build_icon);
            });
        });
        current.pack_start(&reset, false, false, 0);
    }
    block(&outer, &current);

    if app_icon::supported() {
        let theme_block = gtk::Box::new(gtk::Orientation::Vertical, 8);
        theme_block.pack_start(&section("Taskbar and launcher"), false, false, 0);
        let theme = gtk::CheckButton::with_label("Also use it for the taskbar and launcher");
        theme.set_sensitive(!config.app_icon.is_empty());
        theme.set_active(!config.app_icon.is_empty() && app_icon::installed());
        let (a, c) = (app.clone(), container.clone());
        theme.connect_toggled(move |_| {
            let (a, c) = (a.clone(), c.clone());
            gtk::glib::idle_add_local_once(move || {
                app_icon::toggle_theme(&a);
                refresh(&a, &c, build_icon);
            });
        });
        theme_block.pack_start(&theme, false, false, 0);
        let theme_hint = hint("Copies the icon into your icon theme under ~/.local/share/icons. It shows on the next start, on some desktops after you log in again.");
        theme_hint.set_margin_start(26);
        theme_block.pack_start(&theme_hint, false, false, 0);
        block(&outer, &theme_block);
    }
    outer.upcast()
}

/// A check button with its explanation indented under the label.
fn check(outer: &gtk::Box, text: &str, explanation: &str, active: bool, sensitive: bool, on_toggle: impl Fn(bool) + 'static) -> gtk::CheckButton {
    let button = gtk::CheckButton::with_label(text);
    button.set_active(active);
    button.set_sensitive(sensitive);
    button.connect_toggled(move |b| on_toggle(b.is_active()));
    outer.pack_start(&button, false, false, 0);
    if !explanation.is_empty() {
        let hint = hint(explanation);
        hint.set_margin_start(26);
        hint.set_margin_bottom(6);
        outer.pack_start(&hint, false, false, 0);
    }
    button
}

fn build_notifications(app: &AppHandle, container: &gtk::Box) -> gtk::Widget {
    let n = config::load(app).notifications;
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 8);
    outer.pack_start(&hint("New mail is announced with a desktop notification showing the sender, the subject and the first line. Clicking it opens the message."), false, false, 0);
    let section_title = section("Desktop notifications");
    section_title.set_margin_top(8);
    outer.pack_start(&section_title, false, false, 0);
    let (a, c) = (app.clone(), container.clone());
    check(&outer, "Show desktop notifications", "", n.enabled, true, move |on| {
        config::update(&a, |c| c.notifications.enabled = on);
        let (a, c) = (a.clone(), c.clone());
        gtk::glib::idle_add_local_once(move || refresh(&a, &c, build_notifications));
    });
    let a = app.clone();
    check(&outer, "Only while the window is hidden or not focused", "No notifications while you are looking at the inbox.", n.only_when_hidden, n.enabled, move |on| {
        config::update(&a, |c| c.notifications.only_when_hidden = on);
    });
    let a = app.clone();
    check(&outer, "Include a preview of the message", "Off shows the sender and subject only, for shared screens.", n.preview, n.enabled, move |on| {
        config::update(&a, |c| c.notifications.preview = on);
    });
    let a = app.clone();
    check(&outer, "Play the desktop's notification sound", "Which sound, and whether there is one, is set in your desktop's notification settings.", n.sound, n.enabled, move |on| {
        config::update(&a, |c| c.notifications.sound = on);
    });
    outer.pack_start(&hint("Each profile has its own switch on its page under Profiles."), false, false, 0);
    let tray_title = section("Tray icon");
    tray_title.set_margin_top(12);
    outer.pack_start(&tray_title, false, false, 0);
    let a = app.clone();
    check(&outer, "Show a red dot on the tray icon while there is unread mail", "The tooltip shows the count either way.", n.tray_badge, true, move |on| {
        config::update(&a, |c| c.notifications.tray_badge = on);
        crate::refresh_tray_badge(&a);
    });
    outer.upcast()
}

fn build_unload(app: &AppHandle, _container: &gtk::Box) -> gtk::Widget {
    let config = config::load(app);
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 12);
    outer.pack_start(&hint("A profile you switch away from stays loaded for a while, so switching back is instant. After that it is unloaded: its memory is freed and it stops notifying until you select it again."), false, false, 0);
    let title = section("Unload a profile you switched away from");
    title.set_margin_top(8);
    outer.pack_start(&title, false, false, 0);

    let current = config.unload_after_minutes;
    let mut choices: Vec<(Option<u32>, String, &str)> = UNLOAD_CHOICES.iter().map(|(v, t, h)| (*v, (*t).to_string(), *h)).collect();
    if let Some(minutes) = current.filter(|_| !UNLOAD_CHOICES.iter().any(|(v, _, _)| *v == current)) {
        choices.push((current, format!("After {minutes} minutes"), "Set in config.json"));
    }
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.style_context().add_class("settings-group");
    let mut leader: Option<gtk::RadioButton> = None;
    for (value, text, side) in choices {
        let radio = match &leader {
            Some(first) => gtk::RadioButton::with_label_from_widget(first, &text),
            None => gtk::RadioButton::with_label(&text),
        };
        radio.set_active(value == current);
        let (a, id) = (app.clone(), unload_id(value));
        radio.connect_toggled(move |r| {
            if r.is_active() {
                config::update(&a, |c| c.unload_after_minutes = parse_unload_id(&id));
            }
        });
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.pack_start(&radio, true, true, 0);
        if !side.is_empty() {
            row.pack_start(&label(side, &["dim-label", "settings-small"]), false, false, 0);
        }
        list.add(&row);
        leader.get_or_insert(radio);
    }
    let frame = gtk::Frame::new(None);
    frame.style_context().add_class("settings-group");
    frame.add(&list);
    frame.set_halign(gtk::Align::Start);
    frame.set_size_request(420, -1);
    outer.pack_start(&frame, false, false, 0);
    outer.pack_start(&hint("Profiles marked Keep loaded are exempt. Set it on each profile's page under Profiles."), false, false, 0);
    outer.upcast()
}

/// One profile's page. Actions that re-list the profiles run deferred, so
/// the widget whose signal fired is not destroyed inside its own handler.
fn profile_page(app: &AppHandle, id: &str) -> gtk::Widget {
    let config = config::load(app);
    let Some(profile) = config.profile(id).cloned() else {
        return label("This profile no longer exists.", &["dim-label"]).upcast();
    };
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 18);

    // Header: big avatar, name, status, and Switch for the others.
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    header.pack_start(&avatar(&profile, true), false, false, 0);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_valign(gtk::Align::Center);
    let name_title = label(&profile.name, &["settings-page-title"]);
    name_title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.pack_start(&name_title, false, false, 0);
    text.pack_start(&status_label(app, &profile.id, &["dim-label"]), false, false, 0);
    header.pack_start(&text, true, true, 0);
    if profiles::active_id(app) != profile.id {
        let switch = gtk::Button::with_label("Switch to this profile");
        switch.style_context().add_class("suggested-action");
        switch.set_valign(gtk::Align::Center);
        let (a, pid) = (app.clone(), profile.id.clone());
        switch.connect_clicked(move |_| {
            let (a, pid) = (a.clone(), pid.clone());
            gtk::glib::idle_add_local_once(move || {
                profiles::activate(&a, &pid);
                show_profiles(&a, Some(&pid));
            });
        });
        header.pack_start(&switch, false, false, 0);
    }
    outer.pack_start(&header, false, false, 0);

    // Name
    let name_block = gtk::Box::new(gtk::Orientation::Vertical, 8);
    name_block.pack_start(&section("Name"), false, false, 0);
    let name_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let name = gtk::Entry::new();
    name.set_text(&profile.name);
    name.set_width_chars(28);
    let (a, pid, previous) = (app.clone(), profile.id.clone(), profile.name.clone());
    name.connect_activate(move |entry| {
        let text = entry.text().trim().to_string();
        if text.is_empty() {
            entry.set_text(&previous);
        } else if text != previous {
            let (a, pid) = (a.clone(), pid.clone());
            gtk::glib::idle_add_local_once(move || {
                profiles::rename(&a, &pid, text);
                show_profiles(&a, Some(&pid));
            });
        }
    });
    name_row.pack_start(&name, false, false, 0);
    name_row.pack_start(&label("Saved when you press Enter", &["dim-label", "settings-small"]), false, false, 0);
    name_block.pack_start(&name_row, false, false, 0);
    block(&outer, &name_block);

    // Notifications
    let notify = gtk::Box::new(gtk::Orientation::Vertical, 8);
    notify.pack_start(&section("Notifications"), false, false, 0);
    let (a, pid) = (app.clone(), profile.id.clone());
    check(&notify, "Show notifications for this profile", "Applies while this profile is loaded, selected or in the background. The global switches are under General.", profile.notifications, true, move |on| {
        profiles::set_notifications(&a, &pid, on);
    });
    block(&outer, &notify);

    // In the background
    let background = gtk::Box::new(gtk::Orientation::Vertical, 8);
    background.pack_start(&section("In the background"), false, false, 0);
    let keep = gtk::CheckButton::with_label("Keep loaded");
    keep.set_active(profile.keep_awake);
    let (a, pid) = (app.clone(), profile.id.clone());
    keep.connect_toggled(move |check| profiles::set_keep_awake(&a, &pid, check.is_active()));
    background.pack_start(&keep, false, false, 0);
    let keep_hint = hint("Never unload this profile when you switch away, so it keeps notifying. Uses as much memory as the selected profile. The timeout for the others is under General.");
    keep_hint.set_margin_start(26);
    background.pack_start(&keep_hint, false, false, 0);
    block(&outer, &background);

    // Remove
    let remove_row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    let remove_text = gtk::Box::new(gtk::Orientation::Vertical, 3);
    remove_text.pack_start(&section("Remove profile"), false, false, 0);
    remove_text.pack_start(&hint("Deletes its sign-in and local data from this computer. The account itself is not affected."), false, false, 0);
    remove_row.pack_start(&remove_text, true, true, 0);
    let remove = gtk::Button::with_label("Remove");
    remove.style_context().add_class("destructive-action");
    remove.set_valign(gtk::Align::Center);
    remove.set_sensitive(config.profiles.len() > 1);
    if config.profiles.len() < 2 {
        remove.set_tooltip_text(Some("The last profile cannot be removed"));
    }
    let (a, pid, profile_name) = (app.clone(), profile.id.clone(), profile.name.clone());
    remove.connect_clicked(move |_| {
        let (a, pid, profile_name) = (a.clone(), pid.clone(), profile_name.clone());
        gtk::glib::idle_add_local_once(move || {
            let confirmed = app_icon::message(
                &a,
                "Remove Profile",
                gtk::MessageType::Question,
                gtk::ButtonsType::YesNo,
                &format!("Remove the profile \"{profile_name}\"?"),
                "This deletes its sign-in and local data from this computer. The account itself is not affected.",
            ) == gtk::ResponseType::Yes;
            if confirmed {
                profiles::remove(&a, &pid);
                show_profiles(&a, None);
            }
        });
    });
    remove_row.pack_start(&remove, false, false, 0);
    block(&outer, &remove_row);
    outer.upcast()
}
