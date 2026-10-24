# Configuration

Everything is compile-time, split between `src-tauri/tauri.conf.json` and constants in `src-tauri/src/main.rs`, except one runtime file.

## Runtime config

`$XDG_CONFIG_HOME/com.nikolainyegaard.outlookforlinux/config.json`, written by the Settings dialog and the profile switcher, read on demand (it is tiny) and always written whole through `config::update`:

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `app_icon` | string | `""` | Path of a custom PNG or SVG for the tray and window icon. Empty means the bundled icon. Also settable from Settings, which can copy it into the user icon theme (`~/.local/share/icons`) so the taskbar and launcher follow; those files outlive the package and are not written under Snap or Flatpak |
| `unload_after_minutes` | number or null | `15` | How long a deselected profile stays loaded. `0` unloads it the moment you switch away, `null` never unloads (every profile stays loaded, full notifications, most RAM). A missing key means 15. Settings (General, Background profiles) offers 0, 5, 15, 30, 60 and never; any other number typed here is kept and shown as its own entry |
| `active_profile` | string | `"default"` | Id of the profile shown at start, kept up to date on every switch |
| `notifications` | object | all true except `only_when_hidden` | `enabled` (master switch), `only_when_hidden` (drop toasts while the main window is focused), `preview` (off keeps sender and subject only), `sound` (off sends the suppress-sound hint; the desktop decides the sound otherwise), `tray_badge` (red dot on the tray icon). All on the Settings Notifications page |
| `profiles` | list | one profile `default` named Default | `{ "id", "name", "keep_awake", "notifications" }` per profile. `id` is the directory name and is never shown; `keep_awake` (default false) exempts that profile from unloading; `notifications` (default true) lets its new-mail toasts through. Profiles are added from the switcher or Settings; name, keep_awake and removal are on each profile's page under Settings, Profiles |

Profile data (cookies, local storage, IndexedDB, cache) lives under `$XDG_DATA_HOME/com.nikolainyegaard.outlookforlinux/`: the `default` profile uses that directory itself, which is where the app kept its single session before profiles existed, so the existing sign-in carries over; every other profile uses `profiles/<id>/` below it. Removing a profile from Settings deletes its directory (for `default`, the directory's own entries except `profiles/`). Profile ids carry a random four-hex suffix (`personal-3f2a`) so a removed id is never reused; see gotchas.md.

The icon theme copy keeps its own manifest at `$XDG_DATA_HOME/outlook-for-linux/theme-icon.json`; see architecture.md.

## tauri.conf.json

| Key | Value | Notes |
|-----|-------|-------|
| `build.frontendDist` | `https://outlook.office.com/mail/` | Tauri requires this; for remote-only apps it is the URL itself. Also makes outlook.office.com a "local" URL for the ACL. |
| `app.windows` | `[]` | The window is built in code (main.rs) because injecting a document-start script requires the builder. |
| `app.security.csp` | `null` | CSP is controlled by Microsoft's own headers, not by us. |
| `bundle.targets` | deb, rpm, appimage | Linux only. |
| `bundle.linux.deb/rpm.desktopTemplate` | `./main.desktop` | Custom template; the default one misses `%u` in Exec so mailto URLs never reach the app. |
| `plugins.deep-link.desktop.schemes` | `["mailto"]` | Bundle-time only: makes the bundler add the MimeType line to the .desktop. The runtime deep-link plugin is not used; argv is parsed directly. |
| `identifier` | `com.nikolainyegaard.outlookforlinux` | Do not change after first release; it keys desktop entries and data dirs. |

## main.rs constants

| Const | Value | Notes |
|-------|-------|-------|
| `OWA_URL` | `https://outlook.office.com/mail/` | The wrapped page. Keep in sync with `frontendDist`. |
| `COMPOSE_URL` | OWA compose deeplink | Target for translated mailto links. |
| `USER_AGENT` | Chrome on Linux | Required for Microsoft to offer passkey sign-in at all; see gotchas.md. Bump the Chrome version occasionally. |

## webauthn.rs / capabilities/webauthn.json

`ALLOWED_ORIGINS` in webauthn.rs and `remote.urls` in capabilities/webauthn.json must stay identical; a unit test (`capability_urls_match_real_login_urls`) fails the build's test run if they drift.

## Cargo

`tauri` needs the `tray-icon`, `image-png` and `unstable` features (tray, decoding the badge icon PNG, and `Window` plus child `Webview` for the main window; `unstable` is gated because its API may change between Tauri minor versions); the tray requires libayatana-appindicator at runtime. `ctap-hid-fido2` needs `libudev-dev` on the build machine. Linux-only dependencies: `webkit2gtk` (WebKitSettings properties, notification signals) and `notify-rust` (desktop notifications over DBus). `tauri-plugin-single-instance` forwards second launches, which carries mailto arguments.

`productName` is `outlook-for-linux` on purpose: the bundler names the .desktop file after it, and KWin on Wayland finds the window icon by matching the app id (the binary name) to that file name. The launcher label comes from `Name=` in main.desktop. Runtime environment: `WEBKIT_DISABLE_DMABUF_RENDERER=1` must never be set by default; it forces software rendering.
