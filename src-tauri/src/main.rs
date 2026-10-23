// Prevents an extra console window on Windows in release; harmless on Linux.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_icon;
mod webauthn;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const OWA_URL: &str = "https://outlook.office.com/mail/";
const COMPOSE_URL: &str = "https://outlook.office.com/mail/deeplink/compose";

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

static COMPOSE_SEQ: AtomicUsize = AtomicUsize::new(0);

/// Translate a mailto: URL into OWA's compose deeplink.
fn mailto_to_compose(mailto: &str) -> Option<String> {
    let url = tauri::Url::parse(mailto).ok()?;
    if url.scheme() != "mailto" {
        return None;
    }
    let mut compose = tauri::Url::parse(COMPOSE_URL).expect("COMPOSE_URL parses");
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
        let n = COMPOSE_SEQ.fetch_add(1, Ordering::Relaxed);
        if let Err(e) = open_window(app, &format!("compose-{n}"), &url) {
            eprintln!("failed to open compose window: {e}");
        }
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

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Tray image for the unread state: the custom icon when one is configured
/// (badged by us), the bundled ones otherwise.
fn tray_image(app: &AppHandle, unread: bool) -> Option<tauri::image::Image<'_>> {
    if let Some(custom) = app_icon::load_image(&app_icon::load_config(app).app_icon) {
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
    if let (Some(window), Some(icon)) = (app.get_webview_window("main"), tray_image(app, false)) {
        let _ = window.set_icon(icon);
    }
}

fn hide_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

fn refresh_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.eval("window.location.reload()");
    }
}

/// Ctrl shortcuts matching the tray menu labels, same keys as teams-for-linux.
/// Window-level GTK accelerators run before the webview sees the key, so
/// these shadow OWA's own Ctrl+O, Ctrl+R (reply) and Ctrl+Q (mark as read).
#[cfg(target_os = "linux")]
fn bind_shortcuts(window: &WebviewWindow) -> tauri::Result<()> {
    use gtk::prelude::{AccelGroupExtManual, GtkWindowExt};
    let group = gtk::AccelGroup::new();
    let actions: [(char, fn(&AppHandle)); 4] =
        [('o', show_main), ('h', hide_main), ('r', refresh_main), ('q', |app| app.exit(0))];
    for (key, action) in actions {
        let app = window.app_handle().clone();
        group.connect_accel_group(key as u32, gtk::gdk::ModifierType::CONTROL_MASK, gtk::AccelFlags::VISIBLE, move |_, _, _, _| {
            action(&app);
            true
        });
    }
    window.gtk_window()?.add_accel_group(&group);
    Ok(())
}

fn update_tray(app: &AppHandle, unread: u32) {
    let Some(tray) = app.tray_by_id("tray") else { return };
    let tooltip = if unread > 0 { format!("Outlook, {unread} unread") } else { "Outlook".into() };
    let _ = tray.set_tooltip(Some(tooltip));
    let show_unread = unread > 0;
    // Only swap the icon when the unread state flips.
    if TRAY_SHOWS_UNREAD.swap(show_unread, Ordering::Relaxed) != show_unread {
        let _ = tray.set_icon(tray_image(app, show_unread));
    }
}

// Menu modeled on the Electron teams-for-linux tray, trimmed to what this
// wrapper actually has: Open, Hide, Refresh, App Icon, Quit.
fn tray_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
    // The shortcut text sits in the label after a tab (Plasma draws it as a
    // shortcut column): muda only registers menu accelerators for window
    // menubars, not tray menus. The keys themselves are bound in open_window.
    let open = MenuItem::with_id(app, "open", "Open\tCtrl+O", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "Hide\tCtrl+H", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh\tCtrl+R", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit\tCtrl+Q", true, None::<&str>)?;
    let has_custom = !app_icon::load_config(app).app_icon.is_empty();
    let choose = MenuItem::with_id(app, "icon-choose", "Choose App Icon…", true, None::<&str>)?;
    let theme = CheckMenuItem::with_id(
        app,
        "icon-theme",
        "Also use it for the taskbar and launcher",
        has_custom,
        has_custom && app_icon::installed(),
        None::<&str>,
    )?;
    let reset = MenuItem::with_id(app, "icon-reset", "Reset to default", has_custom, None::<&str>)?;
    let icon_menu = if app_icon::supported() {
        Submenu::with_items(app, "App Icon", true, &[&choose, &theme, &reset])?
    } else {
        Submenu::with_items(app, "App Icon", true, &[&choose, &reset])?
    };
    Menu::with_items(
        app,
        &[
            &open,
            &hide,
            &refresh,
            &PredefinedMenuItem::separator(app)?,
            &icon_menu,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )
}

/// The App Icon items reflect config and theme state, so the menu is rebuilt
/// after every change there.
fn rebuild_tray_menu(app: &AppHandle) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id("tray"), tray_menu(app)) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::tray::TrayIconBuilder;
    TrayIconBuilder::with_id("tray")
        .icon(app.default_window_icon().expect("bundled window icon").clone())
        .tooltip("Outlook")
        .menu(&tray_menu(app)?)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "hide" => hide_main(app),
            "refresh" => refresh_main(app),
            // GTK dialogs: make sure these run on the main thread.
            "icon-choose" => on_main(app, app_icon::choose),
            "icon-theme" => on_main(app, app_icon::toggle_theme),
            "icon-reset" => on_main(app, app_icon::reset),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn on_main(app: &AppHandle, f: fn(&AppHandle)) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || f(&handle));
}

/// All windows (main and compose) share the UA, the WebAuthn polyfill, and
/// the Linux webview setup below.
fn open_window(app: &AppHandle, label: &str, url: &str) -> tauri::Result<WebviewWindow> {
    // Built in code rather than declared in tauri.conf.json because injecting
    // a document-start script requires the builder.
    #[cfg_attr(not(debug_assertions), allow(unused_mut))]
    let mut builder = WebviewWindowBuilder::new(
        app,
        label,
        // Both callers pass URLs we construct ourselves.
        WebviewUrl::External(url.parse().expect("window url parses")),
    )
    .title("Outlook")
    .inner_size(1280.0, 800.0)
    .user_agent(USER_AGENT)
    .initialization_script(WEBAUTHN_POLYFILL)
    .initialization_script(UNREAD_TITLE)
    // Shown below, after the tao header bar is removed; set_titlebar only
    // takes effect before the window is realized.
    .visible(false)
    // target=_blank links and window.open from OWA: without a handler the
    // request dies silently in WebKitGTK. Hand http(s) URLs to the system
    // browser and never open a second webview.
    .on_new_window(|url, _features| {
        #[cfg(debug_assertions)]
        eprintln!("[new-window] {url}");
        if matches!(url.scheme(), "http" | "https") {
            let _ = std::process::Command::new("xdg-open").arg(url.as_str()).spawn();
        }
        tauri::webview::NewWindowResponse::Deny
    });

    // Debug probe: log per page whether the polyfill installed.
    #[cfg(debug_assertions)]
    {
        builder = builder.initialization_script(
            "console.log('[probe] ' + location.origin + ' PublicKeyCredential=' + typeof PublicKeyCredential + ' notifPermission=' + (typeof Notification === 'undefined' ? 'n/a' : Notification.permission) + ' UA=' + navigator.userAgent)",
        );
    }

    let window = builder.build()?;

    // On Wayland tao installs its own GtkHeaderBar (client-side, tall, no
    // app icon). Dropping it lets GTK negotiate server-side decorations
    // through KWin's protocol, so the window manager draws the slim
    // titlebar with the icon, like any native app.
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::GtkWindowExt;
        window.gtk_window()?.set_titlebar(None::<&gtk::Widget>);
        if label == "main" {
            bind_shortcuts(&window)?;
        }
    }
    window.show()?;

    // The main window's page title carries the unread count; watch it to
    // drive the tray badge.
    #[cfg(target_os = "linux")]
    let title_watcher = if label == "main" {
        Some(window.app_handle().clone())
    } else {
        None
    };

    #[cfg(target_os = "linux")]
    window.with_webview(move |webview| {
        use webkit2gtk::glib::prelude::*;
        use webkit2gtk::{NotificationExt, NotificationPermissionRequest, PermissionRequestExt, SettingsExt, WebContextExt, WebViewExt};

        let wv = webview.inner();
        if let Some(settings) = wv.settings() {
            // OWA opens email links from a click handler that first awaits
            // a Safe Links lookup, so window.open runs outside the user
            // gesture. WebKitGTK blocks that by default (no create signal,
            // nothing happens); the second click hits the cached lookup and
            // opens synchronously. Allow it: on_new_window above never
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
            ctx.connect_initialize_notification_permissions(|ctx| grant_notifications(ctx));
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

        // Hand web notifications to the desktop over DBus.
        // ponytail: clicking the notification does not focus the window;
        // add notify-rust action handling if that starts to matter.
        wv.connect_show_notification(|_, notification| {
            let summary = notification
                .title()
                .map(|t| t.to_string())
                .unwrap_or_else(|| "Outlook".into());
            let body = notification.body().map(|b| b.to_string()).unwrap_or_default();
            std::thread::spawn(move || {
                let _ = notify_rust::Notification::new()
                    .summary(&summary)
                    .body(&body)
                    .appname("Outlook")
                    .icon("outlook-for-linux")
                    .hint(notify_rust::Hint::DesktopEntry("outlook-for-linux".into()))
                    .show();
            });
            true
        });

        if let Some(app_handle) = title_watcher {
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
                    update_tray(&app_handle, current);
                }
            });
        }
    })?;

    Ok(window)
}

fn main() {
    tauri::Builder::default()
        // Second launches (e.g. a mailto: click while running) land here in
        // the first instance instead of starting a new process.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
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
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            setup_tray(app.handle())?;
            open_window(app.handle(), "main", OWA_URL)?;
            apply_icon(app.handle());

            // Launched directly as a mailto: handler while not yet running.
            if let Some(m) = std::env::args().find(|a| a.starts_with("mailto:")) {
                open_compose(app.handle(), &m);
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
