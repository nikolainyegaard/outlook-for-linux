# Outlook for Linux

Unofficial desktop wrapper for Outlook on the web (OWA), built with Tauri v2. Loads https://outlook.office.com/mail/ in a native WebKitGTK window with WebAuthn enabled, so passkeys and FIDO2 security keys work during Microsoft sign-in.

## Features

- Native window for Outlook on the web, no browser chrome
- Passkey and FIDO2 security key sign-in (WebAuthn enabled in the webview)
- Light footprint: no bundled Chromium, uses the system WebKitGTK

## Passkey support notes

WebKitGTK's WebAuthn implementation is experimental and off by default; this app turns it on. USB security keys (CTAP2) are the best supported path. Platform authenticators and hybrid transport (sign in with your phone via QR code) may not work depending on your WebKitGTK version. WebKitGTK 2.40 or newer is required.

## Prerequisites

- Rust (rustup): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- Node.js 18+
- Tauri Linux system deps (Debian/Ubuntu): `sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev`

## Run

```
npm install && npm run dev
```

## Build

```
npm run build
```

Bundles (deb, rpm, AppImage) land in `src-tauri/target/release/bundle/`.
