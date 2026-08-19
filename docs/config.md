# Configuration

No environment variables, no config files at runtime. Everything is compile-time, split between `src-tauri/tauri.conf.json` and constants in `src-tauri/src/main.rs`.

## tauri.conf.json

| Key | Value | Notes |
|-----|-------|-------|
| `build.frontendDist` | `https://outlook.office.com/mail/` | Tauri requires this; for remote-only apps it is the URL itself. Also makes outlook.office.com a "local" URL for the ACL. |
| `app.windows` | `[]` | The window is built in code (main.rs) because injecting a document-start script requires the builder. |
| `app.security.csp` | `null` | CSP is controlled by Microsoft's own headers, not by us. |
| `bundle.targets` | deb, rpm, appimage | Linux only. |
| `identifier` | `com.nikolainyegaard.outlookforlinux` | Do not change after first release; it keys desktop entries and data dirs. |

## main.rs constants

| Const | Value | Notes |
|-------|-------|-------|
| `OWA_URL` | `https://outlook.office.com/mail/` | The wrapped page. Keep in sync with `frontendDist`. |
| `USER_AGENT` | Chrome on Linux | Required for Microsoft to offer passkey sign-in at all; see gotchas.md. Bump the Chrome version occasionally. |

## webauthn.rs / capabilities/webauthn.json

`ALLOWED_ORIGINS` in webauthn.rs and `remote.urls` in capabilities/webauthn.json must stay identical; a unit test (`capability_urls_match_real_login_urls`) fails the build's test run if they drift.

## Cargo

`ctap-hid-fido2` needs `libudev-dev` on the build machine. `webkit2gtk` is a Linux-only dependency used to set WebKitSettings properties on the webview.
