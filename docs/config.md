# Configuration

Everything is compile-time, split between `src-tauri/tauri.conf.json` and constants in `src-tauri/src/main.rs`, except one runtime file.

## Runtime config

`$XDG_CONFIG_HOME/com.nikolainyegaard.outlookforlinux/config.json`, written by the tray menu, read at start and on every tray icon change:

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `app_icon` | string | `""` | Path of a custom PNG for the tray and window icon. Empty means the bundled icon. Also settable from the App Icon tray menu, which can copy it into the user icon theme (`~/.local/share/icons`) so the taskbar and launcher follow; those files outlive the package and are not written under Snap or Flatpak |

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

`tauri` needs the `tray-icon` and `image-png` features (tray plus decoding the badge icon PNG); the tray requires libayatana-appindicator at runtime. `ctap-hid-fido2` needs `libudev-dev` on the build machine. Linux-only dependencies: `webkit2gtk` (WebKitSettings properties, notification signals) and `notify-rust` (desktop notifications over DBus). `tauri-plugin-single-instance` forwards second launches, which carries mailto arguments.

`productName` is `outlook-for-linux` on purpose: the bundler names the .desktop file after it, and KWin on Wayland finds the window icon by matching the app id (the binary name) to that file name. The launcher label comes from `Name=` in main.desktop. Runtime environment: `WEBKIT_DISABLE_DMABUF_RENDERER=1` must never be set by default; it forces software rendering.
