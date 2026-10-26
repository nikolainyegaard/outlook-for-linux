# Architecture

Tauri v2 desktop wrapper around Outlook on the web. There is no local frontend: the main window loads https://outlook.office.com/mail/ directly, and `build.frontendDist` in tauri.conf.json points at the same URL. All app logic lives in the Rust side plus two injected scripts.

## File tree

```
outlook-for-linux/
├── .github/workflows/release.yml # tag push: build x86_64 and aarch64 bundles, publish the release
├── package.json                 # npm scripts wrapping the Tauri CLI
├── README.md
├── CHANGELOG.md
├── docs/
│   ├── architecture.md
│   ├── config.md
│   └── gotchas.md
└── src-tauri/
    ├── Cargo.toml               # tauri 2, ctap-hid-fido2, webkit2gtk/gtk/notify-rust (Linux only)
    ├── build.rs                 # declares the webauthn command in the app manifest
    ├── tauri.conf.json          # remote URL, bundle targets, mailto scheme; window is built in code
    ├── main.desktop             # .desktop template (default template misses %u in Exec)
    ├── capabilities/
    │   ├── default.json         # core:default for main and compose windows
    │   └── webauthn.json        # grants the command to the login origins only
    ├── icons/                   # envelope icon (icon.svg), generated RGBA sizes, badged tray variant
    └── src/
        ├── main.rs              # main window + menubar, compose windows, webview setup, notifications, mailto, tray
        ├── app_icon.rs          # app icon: picker, tray badge, icon theme copy, GTK dialogs
        ├── config.rs            # config.json: icon, profiles, unload timeout, notifications, updates
        ├── profiles.rs          # profile switcher: per-profile webviews, unload timer, GTK dropdown, rename/remove
        ├── settings.rs          # Settings window: categories, page list and page pane, Outlook layout
        ├── update.rs            # update check (curl, GitHub releases API) and Update Now (dnf/apt via pkexec, AppImage rename)
        ├── webauthn.rs          # Rust CTAP2 client (the actual authenticator driver)
        ├── webauthn_polyfill.js # PublicKeyCredential polyfill + overlay UI
        └── unread_title.js      # DOM-driven unread count in title + new-mail notifications
```

## Desktop integration

- **Notifications**: OWA never raises web notifications in this webview (its service worker bails out before calling showNotification, and WebKitGTK would not deliver service-worker notifications to the embedder anyway). The injected `unread_title.js` therefore drives everything from the page DOM: it polls the folder pane for the Inbox unread count and, on an increase, fires a page-context `Notification` per new message row with sender, subject and body preview parsed from the row's aria-label. The Rust side pre-grants notification permission for the OWA origins on the WebContext and forwards each notification to the desktop over DBus (notify-rust) with a desktop-entry hint so the app appears in the desktop's notification settings, filtered by the Notifications settings (master switch, only while hidden, preview, sound, per-profile switch). Each toast has a default action: a click shows the window, switches to the profile and is relayed to the page's Notification.onclick, which opens the message (see gotchas.md).
- **mailto**: the bundled .desktop registers x-scheme-handler/mailto. A mailto launch is translated into OWA's compose deeplink and opened in a compose window; if the app is already running, the single-instance plugin forwards the second launch's argv to it. The main webview and the compose windows share `configure_webview`, so compose windows get the same UA, polyfill, and webview setup as the main window.
- **App icon**: the icon picker under Settings is a port of teams-for-linux's App Icon menu (its `themeIcon.js`). The chosen PNG is stored in config.json and applied live to the tray; the window, taskbar and launcher icons are resolved by the desktop from the icon theme by name, so the user can opt in to a copy of the icon at every hicolor size under `$XDG_DATA_HOME/icons/hicolor`, named after the `Icon=` of the desktop entry that launches this binary. Written paths are recorded in `$XDG_DATA_HOME/outlook-for-linux/theme-icon.json` so removal only deletes what the app wrote. After writing, the theme directory mtime is touched (GTK desktops) and `org.kde.KIconLoader.iconChanged` is sent (KDE).
- **Main window and menubar**: the main window is a plain Tauri `Window` (not a `WebviewWindow`) holding the OWA page as a child webview, so that the profile switcher can hold one webview per profile and swap them without recreating the window. The menubar (File, Edit, View, Help) is a muda menu set on the window; its accelerators are the app's shortcuts. A right-justified "Profile" item is appended to the same GtkMenuBar in GTK directly. See gotchas.md for why.
- **Titlebar**: native Wayland. The window drops the header bar tao installs on Wayland before it is shown, so GTK negotiates server-side decorations and KWin draws the system titlebar and app icon. See gotchas.md.
- **Profiles**: each profile is a child webview of the main window with its own WebKit data directory, so each is an independent sign-in (OWA holds one account per cookie jar). Only the selected profile's webview is visible; switching hides one and shows the other, loading it first if it is not in memory. A deselected profile stays loaded for `unload_after_minutes` and is then closed, which drops its web process and network process; its data directory stays, so the next switch is a page load without a login. `keep_awake` exempts a profile. The switcher is a right-justified GTK item on the menubar: a radio entry per profile with a status dot (green selected, amber loaded in the background, grey unloaded) and the background unread count, plus Add Profile. The unread badge on the tray follows the selected profile only; notifications from other loaded profiles carry the profile name. Compose windows use the selected profile's data directory. Rename, keep awake, remove and add live on each profile's own page in the Settings window (`settings.rs`), a modal GTK window opened from Edit > Settings or the tray and laid out like Outlook's settings: categories with icons and a search box on the left, the category's pages in the middle, the page on the right. The Profiles category lists the profiles themselves (with status dots) and Add profile +; General is a static table (title, search keywords, builder function), so a new global setting is one entry plus one builder; every control applies at once and the page is rebuilt from config after each change. Removing a profile unloads it, deletes its data directory and switches away first if it was selected.
- **Tray**: closing the main window hides it to the tray and the app keeps running; the tray menu has Open, Hide, Refresh, Settings, Quit; all but Open share their ids and keys with the menubar. The unread badge (red-dot icon variant plus tooltip count) is driven by watching the selected profile webview's title for a "(N)" prefix, which `unread_title.js` writes itself: OWA stopped putting the count in the title (reproducible in Firefox). Compose windows close normally.

## WebAuthn: how it works

WebKitGTK 4.1 as shipped by distros has no WebAuthn at all, so the app plays the role a browser normally plays, the way Firefox does with its authenticator-rs backend:

1. `main.rs` builds the main window in code (required for a document-start script) and injects `webauthn_polyfill.js` via `initialization_script`. The script runs on every main-frame page but self-guards: it only installs `PublicKeyCredential` and `navigator.credentials` on the three Microsoft login origins.
2. When the login page calls `navigator.credentials.get()`, the polyfill serializes the request (challenge, rpId, allowCredentials, userVerification) and invokes the `webauthn_get_assertion` Tauri command. It shows a shadow-DOM overlay: touch prompt, PIN entry with retry counts, no-device retry, and an account picker for resident keys.
3. `webauthn.rs` validates origin and rpId, builds `clientDataJSON`, and drives the USB security key over HID with the ctap-hid-fido2 crate (the crate SHA-256 hashes the clientDataJSON bytes into the CTAP clientDataHash). The assertion goes back base64url-encoded and the polyfill reassembles a spec-shaped `PublicKeyCredential` for the page.

`create()` (registering a new key) intentionally throws NotSupportedError; register keys in a regular browser.

## Security model

Three independent gates keep the signing command scoped to Microsoft sign-in:

- **Tauri ACL**: `capabilities/webauthn.json` grants `allow-webauthn-get-assertion` to the three login origins with `local: false`. Remote pages always go through the ACL, and because `frontendDist` is the OWA URL, outlook.office.com counts as local, so the mail UI itself cannot invoke the command.
- **Rust-side validation**: `webauthn.rs` re-checks the origin against the same allowlist and enforces the browser rpId rule (rpId must equal the origin's host or be a registrable parent domain of it). Unit tests cover both, including a test that reads the shipped capability file so the two lists cannot drift.
- **Protocol**: the origin is baked into `clientDataJSON`, which Microsoft's server verifies, so even a spoofed request cannot yield an assertion usable for another site.

## User agent

Microsoft's login page only offers the passkey option to browsers on its FIDO support matrix, decided by user-agent sniffing, before any WebAuthn API is touched. WebKitGTK's default UA (Safari "Version/60.5" on Linux) fails that check silently. `main.rs` therefore sets a current Chrome-on-Linux UA on the webview; without it the whole WebAuthn stack is dead code.

## Debugging

Debug builds enable WebKit's console-to-stdout setting and inject a probe line logging each page's origin, `typeof PublicKeyCredential`, and UA. The polyfill logs its requests and errors with a `[webauthn]` prefix. Release builds carry none of this output. Debug builds also take `--settings` (optionally `--settings=page:0` or `--settings=profile:<id>`), which opens the Settings window in the already running instance through the single-instance plugin; with `spectacle -a -b -n -o shot.png` that gives a screenshot of any Settings page without clicking through.
