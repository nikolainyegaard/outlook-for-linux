// Prevents an extra console window on Windows in release; harmless on Linux.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod webauthn;

use std::sync::atomic::{AtomicUsize, Ordering};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const OWA_URL: &str = "https://outlook.office.com/mail/";
const COMPOSE_URL: &str = "https://outlook.office.com/mail/deeplink/compose";

// Microsoft's login page offers passkey sign-in only to browsers on its FIDO
// support matrix, decided by user-agent sniffing. WebKitGTK's default UA
// (Safari "Version/60.5" on Linux) fails that check, so the passkey option
// never appears. Present a current Chrome on Linux instead; OWA itself also
// renders fine with it.
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

// Tauri injects its IPC internals into the main frame only, so the polyfill
// goes there too; it self-guards to the Microsoft login origins.
const WEBAUTHN_POLYFILL: &str = include_str!("webauthn_polyfill.js");

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
    .initialization_script(WEBAUTHN_POLYFILL);

    // Debug probe: log per page whether the polyfill installed.
    #[cfg(debug_assertions)]
    {
        builder = builder.initialization_script(
            "console.log('[probe] ' + location.origin + ' PublicKeyCredential=' + typeof PublicKeyCredential + ' UA=' + navigator.userAgent)",
        );
    }

    let window = builder.build()?;

    #[cfg(target_os = "linux")]
    window.with_webview(|webview| {
        use webkit2gtk::glib::prelude::*;
        use webkit2gtk::{NotificationExt, NotificationPermissionRequest, PermissionRequestExt, WebViewExt};

        let wv = webview.inner();
        if let Some(settings) = wv.settings() {
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
                    .show();
            });
            true
        });
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
                None => {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.set_focus();
                    }
                }
            }
        }))
        .invoke_handler(tauri::generate_handler![webauthn::webauthn_get_assertion])
        .setup(|app| {
            // GTK draws its own titlebar on Wayland (client-side decoration)
            // and its default is much taller than KDE/GNOME system titlebars.
            // Slim it; the WM titlebar on X11 is unaffected.
            #[cfg(target_os = "linux")]
            {
                use gtk::prelude::CssProviderExt;
                let provider = gtk::CssProvider::new();
                provider
                    .load_from_data(
                        b"window.csd headerbar.default-decoration { min-height: 0; padding: 1px 4px; }
                          window.csd headerbar.default-decoration button.titlebutton { min-height: 22px; min-width: 22px; padding: 1px; margin: 0; }",
                    )
                    .expect("titlebar css parses");
                if let Some(screen) = gtk::gdk::Screen::default() {
                    gtk::StyleContext::add_provider_for_screen(
                        &screen,
                        &provider,
                        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                    );
                }
            }

            open_window(app.handle(), "main", OWA_URL)?;

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
    use super::mailto_to_compose;

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
