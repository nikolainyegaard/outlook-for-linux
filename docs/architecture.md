# Architecture

Tauri v2 desktop wrapper around Outlook on the web. There is no local frontend: the main window loads https://outlook.office.com/mail/ directly, and `build.frontendDist` in tauri.conf.json points at the same URL. All app logic lives in the Rust side plus one injected script.

## File tree

```
outlook-for-linux/
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
    ├── icons/                   # Outlook icon, generated RGBA sizes
    └── src/
        ├── main.rs              # window factory, UA, polyfill injection, notifications, mailto
        ├── webauthn.rs          # Rust CTAP2 client (the actual authenticator driver)
        └── webauthn_polyfill.js # PublicKeyCredential polyfill + overlay UI
```

## Desktop integration

- **Notifications**: the Linux webview hook allows WebKit's notification permission request and forwards each web notification to the desktop over DBus (notify-rust). OWA additionally has its own notification setting.
- **mailto**: the bundled .desktop registers x-scheme-handler/mailto. A mailto launch is translated into OWA's compose deeplink and opened in a compose window; if the app is already running, the single-instance plugin forwards the second launch's argv to it. All windows come from one factory (`open_window`) so compose windows get the same UA, polyfill, and webview setup as the main window.
- **Titlebar**: on Wayland, GTK CSS shrinks the client-side titlebar toward system height.

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

Debug builds enable WebKit's console-to-stdout setting and inject a probe line logging each page's origin, `typeof PublicKeyCredential`, and UA. The polyfill logs its requests and errors with a `[webauthn]` prefix. Release builds carry none of this output.
