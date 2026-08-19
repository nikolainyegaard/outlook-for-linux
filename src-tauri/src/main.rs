// Prevents an extra console window on Windows in release; harmless on Linux.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod webauthn;

use tauri::{WebviewUrl, WebviewWindowBuilder};

const OWA_URL: &str = "https://outlook.office.com/mail/";

// Microsoft's login page offers passkey sign-in only to browsers on its FIDO
// support matrix, decided by user-agent sniffing. WebKitGTK's default UA
// (Safari "Version/60.5" on Linux) fails that check, so the passkey option
// never appears. Present a current Chrome on Linux instead; OWA itself also
// renders fine with it.
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

// Tauri injects its IPC internals into the main frame only, so the polyfill
// goes there too; it self-guards to the Microsoft login origins.
const WEBAUTHN_POLYFILL: &str = include_str!("webauthn_polyfill.js");

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![webauthn::webauthn_get_assertion])
        .setup(|app| {
            // The window is built here rather than declared in tauri.conf.json
            // because injecting a document-start script requires the builder.
            let mut builder = WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::External(OWA_URL.parse().expect("OWA_URL parses")),
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
                use webkit2gtk::glib::prelude::ObjectExt;
                use webkit2gtk::WebViewExt;
                if let Some(settings) = webview.inner().settings() {
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
            })?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
