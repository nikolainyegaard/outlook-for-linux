# Configuration

No environment variables, no config files at runtime. Everything is compile-time in `src-tauri/tauri.conf.json`.

| Key | Value | Notes |
|-----|-------|-------|
| `app.windows[0].url` | `https://outlook.office.com/mail/` | The wrapped page. Change here to point at another OWA endpoint. |
| `build.frontendDist` | same URL | Tauri requires this; for remote-only apps it is the URL itself. |
| `app.security.csp` | `null` | CSP is controlled by Microsoft's own headers, not by us. |
| `bundle.targets` | deb, rpm, appimage | Linux only. |
| `identifier` | `com.nikolainyegaard.outlookforlinux` | Do not change after first release; it keys desktop entries and data dirs. |

Cargo-side: the `webkit2gtk` dependency needs the `v2_40` feature for `set_enable_webauthn`; the build machine needs libwebkit2gtk-4.1-dev 2.40 or newer.
