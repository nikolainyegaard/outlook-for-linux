// Prevents an extra console window on Windows in release; harmless on Linux.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_icon;
mod config;
mod ics;
mod profiles;
mod settings;
mod update;
mod webauthn;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
#[cfg(target_os = "linux")]
use std::{cell::RefCell, collections::HashMap};
use tauri::menu::{AboutMetadataBuilder, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::webview::{NewWindowFeatures, NewWindowResponse, WebviewBuilder};
use tauri::{AppHandle, Manager, Url, Webview, WebviewUrl, WebviewWindowBuilder, Window, Wry};

pub(crate) const OWA_URL: &str = "https://outlook.office.com/mail/";
const COMPOSE_URL: &str = "https://outlook.office.com/mail/deeplink/compose";
const REPO_URL: &str = "https://github.com/nikolainyegaard/outlook-for-linux";

const TRAY_UNREAD_ICON: &[u8] = include_bytes!("../icons/tray-unread.png");
static TRAY_SHOWS_UNREAD: AtomicBool = AtomicBool::new(false);

// Microsoft's login page offers passkey sign-in only to browsers on its FIDO
// support matrix, decided by user-agent sniffing. WebKitGTK's default UA
// (Safari "Version/60.5" on Linux) fails that check, so the passkey option
// never appears. Present a current Chrome on Linux instead; OWA itself also
// renders fine with it.
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

// Tauri injects its IPC internals into the main frame only, so the polyfill
// goes there too; it self-guards to the Microsoft login origins.
const WEBAUTHN_POLYFILL: &str = include_str!("webauthn_polyfill.js");
// Writes the Inbox unread count into the title, which OWA itself no longer
// does; the title watcher below drives the tray badge and notifications.
const UNREAD_TITLE: &str = include_str!("unread_title.js");
// Debug probe: log per page whether the polyfill installed.
#[cfg(debug_assertions)]
const PROBE: &str = "console.log('[probe] ' + location.origin + ' PublicKeyCredential=' + typeof PublicKeyCredential + ' notifPermission=' + (typeof Notification === 'undefined' ? 'n/a' : Notification.permission) + ' UA=' + navigator.userAgent)";

static COMPOSE_SEQ: AtomicUsize = AtomicUsize::new(0);

/// Translate a mailto: URL into OWA's compose deeplink.
fn mailto_to_compose(mailto: &str) -> Option<String> {
    let url = Url::parse(mailto).ok()?;
    if url.scheme() != "mailto" {
        return None;
    }
    let mut compose = Url::parse(COMPOSE_URL).expect("COMPOSE_URL parses");
    {
        let mut q = compose.query_pairs_mut();
        let to = percent_encoding::percent_decode_str(url.path())
            .decode_utf8()
            .ok()?;
        if !to.is_empty() {
            q.append_pair("to", &to);
        }
        for (key, value) in url.query_pairs() {
            let key = key.to_ascii_lowercase();
            if matches!(key.as_str(), "to" | "cc" | "bcc" | "subject" | "body") {
                q.append_pair(&key, &value);
            }
        }
    }
    Some(compose.into())
}

fn open_compose(app: &AppHandle, mailto: &str) {
    if let Some(url) = mailto_to_compose(mailto) {
        open_compose_url(app, &url);
    }
}

/// A launch argument that is an .ics file, from a double-click or a
/// "open with": the first event goes into a calendar compose window.
fn open_ics_from_args(app: &AppHandle, argv: &[String]) -> bool {
    let Some(path) = argv.iter().find_map(|a| ics::path_from_arg(a)) else { return false };
    match ics::compose_url_for(&path) {
        Ok(url) => open_compose_url(app, &url),
        Err(e) => {
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                app_icon::message(&handle, "Open Calendar File", gtk::MessageType::Error, gtk::ButtonsType::Ok, "This calendar file cannot be opened.", &e);
            });
        }
    }
    true
}

fn open_compose_url(app: &AppHandle, url: &str) {
    let n = COMPOSE_SEQ.fetch_add(1, Ordering::Relaxed);
    if let Err(e) = open_compose_window(app, &format!("compose-{n}"), url) {
        eprintln!("failed to open compose window: {e}");
    }
}

/// Unread count from the page title: OWA puts "(N)" in document.title when
/// there is unread mail. First "(N)" group wins, 0 otherwise.
// The "(N) " prefix is written by unread_title.js. None means the count is
// not (yet) known, which is distinct from a known zero: only transitions
// between known counts may raise a notification.
fn parse_unread(title: &str) -> Option<u32> {
    title
        .strip_prefix('(')?
        .split(')')
        .next()?
        .trim()
        .parse()
        .ok()
}

fn main_window(app: &AppHandle) -> Option<Window> {
    app.get_window("main")
}

fn main_webview(app: &AppHandle) -> Option<Webview> {
    profiles::active_webview(app)
}

fn show_main(app: &AppHandle) {
    if let Some(w) = main_window(app) {
        let _ = w.show();
        // Window::show is gtk_widget_show_all on Linux: it brings every
        // hidden background profile along, stacked under the active one.
        profiles::hide_inactive(app);
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn hide_main(app: &AppHandle) {
    if let Some(w) = main_window(app) {
        let _ = w.hide();
    }
}

fn refresh_main(app: &AppHandle) {
    if let Some(w) = main_webview(app) {
        let _ = w.eval("window.location.reload()");
    }
}

/// Edit menu: muda's predefined Cut/Copy/Paste items synthesise key presses
/// through libxdo on Linux, which does nothing on Wayland. Drive WebKit's
/// editing commands directly instead.
#[cfg(target_os = "linux")]
fn editing_command(app: &AppHandle, command: &'static str) {
    if let Some(w) = main_webview(app) {
        let _ = w.with_webview(move |webview| {
            use webkit2gtk::WebViewExt;
            webview.inner().execute_editing_command(command);
        });
    }
}

#[cfg(target_os = "linux")]
fn zoom(app: &AppHandle, change: fn(f64) -> f64) {
    if let Some(w) = main_webview(app) {
        let _ = w.with_webview(move |webview| {
            use webkit2gtk::WebViewExt;
            let wv = webview.inner();
            wv.set_zoom_level(change(wv.zoom_level()).clamp(0.5, 3.0));
        });
    }
}

/// Tray image for the unread state: the custom icon when one is configured
/// (badged by us), the bundled ones otherwise.
fn tray_image(app: &AppHandle, unread: bool) -> Option<tauri::image::Image<'_>> {
    if let Some(custom) = app_icon::load_image(&config::load(app).app_icon) {
        return Some(app_icon::to_tauri(if unread { app_icon::with_unread_dot(&custom) } else { custom }));
    }
    if unread {
        tauri::image::Image::from_bytes(TRAY_UNREAD_ICON).ok()
    } else {
        app.default_window_icon().cloned()
    }
}

/// Re-apply the configured icon to the tray and the main window. The window
/// icon only takes on X11; on Wayland KWin reads it from the desktop file,
/// which is what the icon theme copy in app_icon is for.
fn apply_icon(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id("tray") {
        let _ = tray.set_icon(tray_image(app, TRAY_SHOWS_UNREAD.load(Ordering::Relaxed)));
    }
    if let (Some(window), Some(icon)) = (main_window(app), tray_image(app, false)) {
        let _ = window.set_icon(icon);
    }
}

pub(crate) fn update_tray(app: &AppHandle, unread: u32) {
    let Some(tray) = app.tray_by_id("tray") else { return };
    let tooltip = if unread > 0 { format!("Outlook, {unread} unread") } else { "Outlook".into() };
    let _ = tray.set_tooltip(Some(tooltip));
    let show_unread = unread > 0 && config::load(app).notifications.tray_badge;
    // Only swap the icon when the unread state flips.
    if TRAY_SHOWS_UNREAD.swap(show_unread, Ordering::Relaxed) != show_unread {
        let _ = tray.set_icon(tray_image(app, show_unread));
    }
}

/// Re-apply the badge from the selected profile's last count, after the
/// tray badge setting changed.
pub(crate) fn refresh_tray_badge(app: &AppHandle) {
    update_tray(app, profiles::unread(app, &profiles::active_id(app)));
}

#[cfg(target_os = "linux")]
thread_local! {
    /// Web notifications whose desktop toast is still showing, by WebKit
    /// id, so a click on the toast can be delivered back to the page.
    /// Main thread only.
    static TOASTS: RefCell<HashMap<u64, webkit2gtk::Notification>> = RefCell::new(HashMap::new());
}

/// The desktop toast closed, clicked or not. On a click, bring the window
/// and the profile forward and tell WebKit, which runs the page's onclick
/// (unread_title.js opens the message there).
#[cfg(target_os = "linux")]
fn toast_done(app: &AppHandle, id: u64, clicked: bool, profile: Option<&str>) {
    use webkit2gtk::NotificationExt;
    let notification = TOASTS.with(|t| t.borrow_mut().remove(&id));
    if !clicked {
        return;
    }
    show_main(app);
    if let Some(profile) = profile {
        profiles::activate(app, profile);
    }
    if let Some(notification) = notification {
        notification.clicked();
    }
}

/// Main window menubar. Accelerators declared here are registered by muda on
/// the GTK window, where they run before the webview sees the key; this
/// shadows OWA's own Ctrl+R (reply) and Ctrl+Q (mark as read), same keys as
/// teams-for-linux. Edit items carry no accelerator on purpose: Ctrl+C/V/X/A
/// keep going straight to the page.
fn menubar(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let file = Submenu::with_items(
        app,
        "File",
        true,
        &[
            &MenuItem::with_id(app, "new-message", "New Message", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "hide", "Hide", true, Some("CmdOrCtrl+H"))?,
            &MenuItem::with_id(app, "quit", "Quit", true, Some("CmdOrCtrl+Q"))?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &MenuItem::with_id(app, "edit-cut", "Cut", true, None::<&str>)?,
            &MenuItem::with_id(app, "edit-copy", "Copy", true, None::<&str>)?,
            &MenuItem::with_id(app, "edit-paste", "Paste", true, None::<&str>)?,
            &MenuItem::with_id(app, "edit-select-all", "Select All", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "settings", "Settings\u{2026}", true, Some("CmdOrCtrl+Comma"))?,
        ],
    )?;
    let view = Submenu::with_items(
        app,
        "View",
        true,
        &[
            &MenuItem::with_id(app, "refresh", "Refresh", true, Some("CmdOrCtrl+R"))?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "zoom-in", "Zoom In", true, Some("CmdOrCtrl+Equal"))?,
            &MenuItem::with_id(app, "zoom-out", "Zoom Out", true, Some("CmdOrCtrl+Minus"))?,
            &MenuItem::with_id(app, "zoom-reset", "Actual Size", true, Some("CmdOrCtrl+0"))?,
        ],
    )?;
    let about = AboutMetadataBuilder::new()
        .name(Some("Outlook for Linux"))
        .version(Some(env!("CARGO_PKG_VERSION")))
        .website(Some(REPO_URL))
        .website_label(Some("GitHub"))
        .build();
    let help = Submenu::with_items(
        app,
        "Help",
        true,
        &[
            &MenuItem::with_id(app, "github", "GitHub Repository", true, None::<&str>)?,
            &MenuItem::with_id(app, "check-updates", "Check for Updates\u{2026}", true, None::<&str>)?,
            &PredefinedMenuItem::about(app, Some("About"), Some(about))?,
        ],
    )?;
    Menu::with_items(app, &[&file, &edit, &view, &help])
}

// Menu modeled on the Electron teams-for-linux tray, trimmed to what this
// wrapper actually has: Open, Hide, Refresh, Settings, Quit.
fn tray_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    // The shortcut text sits in the label after a tab (Plasma draws it as a
    // shortcut column): muda only registers menu accelerators for window
    // menubars, not tray menus. The keys themselves come from the menubar,
    // which shares these ids, so a tray label and its menubar twin always
    // name the same key.
    Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "open", "Open", true, None::<&str>)?,
            &MenuItem::with_id(app, "hide", "Hide\tCtrl+H", true, None::<&str>)?,
            &MenuItem::with_id(app, "refresh", "Refresh\tCtrl+R", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "settings", "Settings\u{2026}\tCtrl+,", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "Quit\tCtrl+Q", true, None::<&str>)?,
        ],
    )
}

/// One handler for both menus: Tauri registers the tray's menu handler as a
/// global listener, so it receives menubar events too. Ids shared between
/// the two menus are the same action by design.
fn menu_action(app: &AppHandle, id: &str) {
    match id {
        "open" => show_main(app),
        "hide" => hide_main(app),
        "refresh" => refresh_main(app),
        "new-message" => open_compose_url(app, COMPOSE_URL),
        #[cfg(target_os = "linux")]
        "edit-cut" => editing_command(app, "Cut"),
        #[cfg(target_os = "linux")]
        "edit-copy" => editing_command(app, "Copy"),
        #[cfg(target_os = "linux")]
        "edit-paste" => editing_command(app, "Paste"),
        #[cfg(target_os = "linux")]
        "edit-select-all" => editing_command(app, "SelectAll"),
        #[cfg(target_os = "linux")]
        "zoom-in" => zoom(app, |z| z + 0.1),
        #[cfg(target_os = "linux")]
        "zoom-out" => zoom(app, |z| z - 0.1),
        #[cfg(target_os = "linux")]
        "zoom-reset" => zoom(app, |_| 1.0),
        "github" => {
            let _ = std::process::Command::new("xdg-open").arg(REPO_URL).spawn();
        }
        // GTK dialog: make sure it runs on the main thread.
        "settings" => on_main(app, settings::open),
        "check-updates" => on_main(app, |app| {
            settings::open_at(app, Some(settings::UPDATES));
            update::start_check(app, false);
        }),
        "quit" => app.exit(0),
        _ => {}
    }
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::tray::TrayIconBuilder;
    TrayIconBuilder::with_id("tray")
        .icon(app.default_window_icon().expect("bundled window icon").clone())
        .tooltip("Outlook")
        .menu(&tray_menu(app)?)
        .on_menu_event(|app, event| menu_action(app, event.id.as_ref()))
        .build(app)?;
    Ok(())
}

fn on_main(app: &AppHandle, f: fn(&AppHandle)) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || f(&handle));
}

/// target=_blank links and window.open from OWA: without a handler the
/// request dies silently in WebKitGTK. Hand http(s) URLs to the system
/// browser and never open a second webview.
fn deny_new_window(url: Url, _features: NewWindowFeatures) -> NewWindowResponse<Wry> {
    #[cfg(debug_assertions)]
    eprintln!("[new-window] {url}");
    if matches!(url.scheme(), "http" | "https") {
        let _ = std::process::Command::new("xdg-open").arg(url.as_str()).spawn();
    }
    NewWindowResponse::Deny
}

/// Builder for an OWA webview inside the main window. Compose windows get
/// the same settings through open_compose_window; the two builder types
/// share method names but not a trait, hence the repetition there.
#[cfg_attr(not(debug_assertions), allow(unused_mut))]
pub(crate) fn owa_webview(label: &str, url: &str) -> WebviewBuilder<Wry> {
    // Callers pass URLs we construct ourselves.
    let mut builder = WebviewBuilder::new(label, WebviewUrl::External(url.parse().expect("webview url parses")))
        .user_agent(USER_AGENT)
        .initialization_script(WEBAUTHN_POLYFILL)
        .initialization_script(UNREAD_TITLE)
        .on_new_window(deny_new_window);
    #[cfg(debug_assertions)]
    {
        builder = builder.initialization_script(PROBE);
    }
    builder
}

/// The main window: a plain Window with the menubar and one child webview
/// per profile, so profiles can be switched without recreating the window.
/// The first webview comes from activating the configured profile.
fn open_main(app: &AppHandle) -> tauri::Result<Window> {
    let window = tauri::window::WindowBuilder::new(app, "main")
        .title("Outlook")
        .inner_size(1280.0, 800.0)
        // Shown below, after the tao header bar is removed; set_titlebar only
        // takes effect before the window is realized.
        .visible(false)
        .build()?;

    // On Wayland tao installs its own GtkHeaderBar (client-side, tall, no
    // app icon). Dropping it lets GTK negotiate server-side decorations
    // through KWin's protocol, so the window manager draws the slim
    // titlebar with the icon, like any native app.
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::GtkWindowExt;
        window.gtk_window()?.set_titlebar(None::<&gtk::Widget>);
    }
    window.set_menu(menubar(app)?)?;
    #[cfg(target_os = "linux")]
    decorate_menubar(app, &window)?;
    profiles::activate(app, &profiles::active_id(app));
    window.show()?;
    Ok(window)
}

/// Separator line above the menubar and the profile switcher at its end.
/// Tauri keeps muda's GtkMenuBar private, so it is found by walking the
/// window's widget tree after set_menu.
#[cfg(target_os = "linux")]
fn decorate_menubar(app: &AppHandle, window: &Window) -> tauri::Result<()> {
    use gtk::prelude::*;
    let Some(menubar) = find_menubar(&window.gtk_window()?) else {
        eprintln!("menubar widget not found; profile switcher not shown");
        return Ok(());
    };
    // A thin theme-coloured line between the titlebar and the menubar; the
    // menubar is at index 0 of the window's GtkBox, the line goes above it.
    if let Some(vbox) = menubar.parent().and_then(|p| p.downcast::<gtk::Box>().ok()) {
        let line = gtk::Separator::new(gtk::Orientation::Horizontal);
        vbox.pack_start(&line, false, false, 0);
        vbox.reorder_child(&line, 0);
        line.show();
    }
    profiles::install_switcher(app, &menubar);
    Ok(())
}

#[cfg(target_os = "linux")]
fn find_menubar<C: gtk::glib::IsA<gtk::Container>>(container: &C) -> Option<gtk::MenuBar> {
    use gtk::prelude::*;
    for child in container.children() {
        match child.downcast::<gtk::MenuBar>() {
            Ok(bar) => return Some(bar),
            Err(other) => {
                if let Ok(inner) = other.downcast::<gtk::Container>() {
                    if let Some(bar) = find_menubar(&inner) {
                        return Some(bar);
                    }
                }
            }
        }
    }
    None
}

/// Compose windows are ordinary webview windows; one per mailto: or New
/// Message. They share the UA, scripts and link handling with the main
/// webview, and the data directory of the selected profile, so mail goes
/// out from that account.
fn open_compose_window(app: &AppHandle, label: &str, url: &str) -> tauri::Result<()> {
    let mut builder = WebviewWindowBuilder::new(app, label, WebviewUrl::External(url.parse().expect("window url parses")))
        .title("Outlook")
        .inner_size(1280.0, 800.0)
        .user_agent(USER_AGENT)
        .initialization_script(WEBAUTHN_POLYFILL)
        .initialization_script(UNREAD_TITLE)
        .visible(false)
        .on_new_window(deny_new_window);
    if let Some(dir) = profiles::data_dir(app, &profiles::active_id(app)) {
        builder = builder.data_directory(dir);
    }
    #[cfg(debug_assertions)]
    {
        builder = builder.initialization_script(PROBE);
    }
    let window = builder.build()?;
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::GtkWindowExt;
        window.gtk_window()?.set_titlebar(None::<&gtk::Widget>);
    }
    configure_webview(window.as_ref(), None)?;
    // After Send or Save, OWA calls window.close(). wry answers WebKit's
    // close signal by destroying the webview widget and nothing else, which
    // leaves the window standing empty; take the window down with it.
    #[cfg(target_os = "linux")]
    {
        let (handle, label) = (app.clone(), label.to_string());
        window.as_ref().with_webview(move |webview| {
            gtk::prelude::WidgetExt::connect_destroy(&webview.inner(), move |_| close_window(&handle, &label));
        })?;
    }
    window.show()?;
    Ok(())
}

/// Close a compose window from a GTK signal; deferred so nothing is torn
/// down while the signal is still being delivered.
#[cfg(target_os = "linux")]
fn close_window(app: &AppHandle, label: &str) {
    let (app, label) = (app.clone(), label.to_string());
    gtk::glib::idle_add_local_once(move || {
        if let Some(w) = app.get_webview_window(&label) {
            let _ = w.close();
        }
    });
}

/// WebKit-level setup every OWA webview needs: window.open policy, WebAuthn,
/// notification permission and forwarding, and (for profile webviews) the
/// title watcher that drives the unread counts.
#[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
pub(crate) fn configure_webview(webview: &Webview, profile: Option<&str>) -> tauri::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let app_handle = webview.app_handle().clone();
        let profile = profile.map(str::to_owned);
        webview.with_webview(move |webview| {
            use webkit2gtk::glib::prelude::*;
            use webkit2gtk::{NotificationExt, NotificationPermissionRequest, PermissionRequestExt, SettingsExt, WebContextExt, WebViewExt};

            let wv = webview.inner();
            if let Some(settings) = wv.settings() {
                // OWA opens email links from a click handler that first awaits
                // a Safe Links lookup, so window.open runs outside the user
                // gesture. WebKitGTK blocks that by default (no create signal,
                // nothing happens); the second click hits the cached lookup and
                // opens synchronously. Allow it: deny_new_window never
                // creates a webview anyway, it only hands URLs to the browser.
                settings.set_javascript_can_open_windows_automatically(true);
                // Prefer WebKitGTK's own WebAuthn when a build has it; the
                // bindings lack a typed setter and most builds lack the
                // property, so set it by name behind a guard (set_property
                // panics on a missing property). The polyfill covers the
                // usual case where it is absent.
                if settings.has_property("enable-webauthn", None) {
                    settings.set_property("enable-webauthn", true);
                }
                // Surface JS console messages on stdout in debug builds.
                #[cfg(debug_assertions)]
                settings.set_property("enable-write-console-messages-to-stdout", true);
            }

            // WebKit auto-denies a gestureless Notification.requestPermission
            // without emitting permission-request, and OWA checks
            // Notification.permission at load. Pre-grant the OWA origin at the
            // context so the page reads "granted" from the start. Applied both
            // directly and from the initialization signal, so it holds no matter
            // when the web process consults it.
            fn grant_notifications(ctx: &webkit2gtk::WebContext) {
                // OWA redirects from the launch URL to outlook.cloud.microsoft;
                // grant both so the permission holds wherever the mailbox lands.
                let origins = [
                    webkit2gtk::SecurityOrigin::for_uri(OWA_URL),
                    webkit2gtk::SecurityOrigin::for_uri("https://outlook.cloud.microsoft/"),
                ];
                let refs: Vec<&webkit2gtk::SecurityOrigin> = origins.iter().collect();
                ctx.initialize_notification_permissions(&refs, &[]);
            }
            if let Some(ctx) = wv.context() {
                grant_notifications(&ctx);
                ctx.connect_initialize_notification_permissions(grant_notifications);
            }

            // OWA asks for Web Notification permission; WebKitGTK denies it
            // unless the app answers. Everything else keeps the default (deny).
            wv.connect_permission_request(|_, request| {
                if request.is::<NotificationPermissionRequest>() {
                    request.allow();
                    true
                } else {
                    false
                }
            });

            // Hand web notifications to the desktop over DBus, filtered by
            // the notification settings. Returning true in every case keeps
            // WebKit from showing its own. The toast carries a default action,
            // so a click comes back through wait_for_action on the toast's
            // thread and is handed to the main thread.
            let (notify_app, notify_profile) = (app_handle.clone(), profile.clone());
            wv.connect_show_notification(move |_, notification| {
                let settings = config::load(&notify_app);
                let allowed = notify_profile.as_deref().map_or(settings.notifications.enabled, |id| settings.notifies(id));
                let focused = main_window(&notify_app).and_then(|w| w.is_focused().ok()).unwrap_or(false);
                if !allowed || (settings.notifications.only_when_hidden && focused) {
                    return true;
                }
                let mut summary = notification
                    .title()
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "Outlook".into());
                if let Some(id) = &notify_profile {
                    summary = profiles::notification_title(&notify_app, id, summary);
                }
                let mut body = notification.body().map(|b| b.to_string()).unwrap_or_default();
                if !settings.notifications.preview {
                    // unread_title.js puts the subject on the first line and
                    // the preview after it.
                    body = body.lines().next().unwrap_or_default().to_string();
                }
                let id = notification.id();
                TOASTS.with(|t| t.borrow_mut().insert(id, notification.clone()));
                notification.connect_closed(move |n| {
                    TOASTS.with(|t| t.borrow_mut().remove(&n.id()));
                });
                let (app, profile, sound) = (notify_app.clone(), notify_profile.clone(), settings.notifications.sound);
                std::thread::spawn(move || {
                    let mut toast = notify_rust::Notification::new();
                    toast
                        .summary(&summary)
                        .body(&body)
                        .appname("Outlook")
                        .icon("outlook-for-linux")
                        .hint(notify_rust::Hint::DesktopEntry("outlook-for-linux".into()))
                        .action("default", "Open");
                    if !sound {
                        toast.hint(notify_rust::Hint::SuppressSound(true));
                    }
                    let Ok(handle) = toast.show() else { return };
                    handle.wait_for_action(move |action| {
                        let clicked = action == "default";
                        let main = app.clone();
                        let _ = app.run_on_main_thread(move || toast_done(&main, id, clicked, profile.as_deref()));
                    });
                });
                true
            });

            if let Some(id) = profile {
                // New-mail notifications are raised page-side by unread_title.js
                // (it has the sender and subject; this side only has a number)
                // and arrive through the show-notification forwarder above. The
                // title watcher only drives the tray badge. Known counts only:
                // acting on a transient bare title (page navigations reset the
                // title before unread_title.js rewrites it) would flicker the
                // badge off.
                wv.connect_title_notify(move |wv| {
                    let title = wv.title().map(|t| t.to_string()).unwrap_or_default();
                    let unread = parse_unread(&title);
                    #[cfg(debug_assertions)]
                    eprintln!("[title-watch] title={title:?} parsed_unread={unread:?}");
                    if let Some(current) = unread {
                        profiles::report_unread(&app_handle, &id, current);
                    }
                });
            }
        })?;
    }
    Ok(())
}

fn main() {
    tauri::Builder::default()
        // Second launches (e.g. a mailto: click while running) land here in
        // the first instance instead of starting a new process.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // Debug aid: `outlook-for-linux --settings` opens Settings in the
            // running instance, for screenshots of the dialog.
            #[cfg(debug_assertions)]
            if let Some(arg) = argv.iter().find(|a| a.starts_with("--settings")) {
                let key = arg.strip_prefix("--settings=").map(str::to_owned);
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || settings::open_at(&handle, key.as_deref()));
                return;
            }
            if open_ics_from_args(app, &argv) {
                return;
            }
            match argv.iter().find(|a| a.starts_with("mailto:")) {
                Some(m) => open_compose(app, m),
                None => show_main(app),
            }
        }))
        .invoke_handler(tauri::generate_handler![webauthn::webauthn_get_assertion])
        // Closing the main window hides it to the tray; the app keeps running
        // so notifications and the unread badge stay live. Compose windows
        // close normally. Tray "Quit" exits.
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    // Settings is transient for this window; take it along.
                    settings::close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            profiles::init(app.handle());
            setup_tray(app.handle())?;
            open_main(app.handle())?;
            apply_icon(app.handle());
            update::check_at_start(app.handle());

            // Launched directly as a mailto: or .ics handler while not yet running.
            let args: Vec<String> = std::env::args().collect();
            if !open_ics_from_args(app.handle(), &args) {
                if let Some(m) = args.iter().find(|a| a.starts_with("mailto:")) {
                    open_compose(app.handle(), m);
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::{mailto_to_compose, parse_unread};

    #[test]
    fn unread_from_title() {
        assert_eq!(parse_unread("(2) Inbox - Outlook"), Some(2));
        assert_eq!(parse_unread("(0) Mail - Outlook"), Some(0));
        // Only the prefix written by unread_title.js counts.
        assert_eq!(parse_unread("Inbox (5) - nikolai@example.com - Outlook"), None);
        assert_eq!(parse_unread("Inbox - Outlook"), None);
        assert_eq!(parse_unread("Re: report (final) - Outlook"), None);
        assert_eq!(parse_unread(""), None);
    }

    #[test]
    fn mailto_translation() {
        assert_eq!(
            mailto_to_compose("mailto:a@b.com").unwrap(),
            "https://outlook.office.com/mail/deeplink/compose?to=a%40b.com"
        );
        let full = mailto_to_compose("mailto:a@b.com?subject=Hi%20there&cc=c@d.com&body=Line%20one").unwrap();
        assert!(full.contains("to=a%40b.com"));
        assert!(full.contains("subject=Hi+there") || full.contains("subject=Hi%20there"));
        assert!(full.contains("cc=c%40d.com"));
        // No recipient, subject only: valid mailto, compose still opens.
        assert!(mailto_to_compose("mailto:?subject=x").unwrap().contains("subject=x"));
        // Junk and non-mailto schemes are refused.
        assert!(mailto_to_compose("https://evil.example").is_none());
        assert!(mailto_to_compose("not a url").is_none());
    }
}
