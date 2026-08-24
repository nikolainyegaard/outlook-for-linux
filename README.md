# Outlook for Linux

Unofficial desktop wrapper for Outlook on the web (OWA), built with Tauri v2. Loads https://outlook.office.com/mail/ in a native WebKitGTK window with working passkey and FIDO2 security key sign-in, which no distro WebKitGTK provides on its own.

## Features

- Native window for Outlook on the web, no browser chrome
- FIDO2 USB security key (passkey) sign-in, implemented in the app itself: WebKitGTK ships without WebAuthn, so the app injects a `PublicKeyCredential` polyfill on the Microsoft login pages and drives the key over USB HID (CTAP2) from Rust
- Desktop notifications for new mail with sender, subject and body preview, read from the page itself (OWA never raises web notifications in WebKitGTK)
- Registers as mailto handler: email links open a prefilled OWA compose window; set it as the default email app in your desktop's settings
- Single instance: relaunching focuses the existing window
- Tray icon with unread badge; closing the window keeps the app running in the tray, quit from the tray menu
- Light footprint: no bundled Chromium, uses the system WebKitGTK

## Passkey support

Sign-in with a USB FIDO2 security key works, including keys that require a PIN and resident keys (account picker). Scope and known limits:

- USB keys only; no phone QR (hybrid) flow, no platform authenticators
- Sign-in only: registering a new key must be done in a regular browser first
- Main frame only: login flows embedded in an iframe are not covered
- The polyfill only activates on Microsoft login origins (login.microsoftonline.com, login.microsoft.com, login.live.com)

See docs/architecture.md for how it works and the security model.

## Prerequisites

- Rust (rustup): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- Node.js 18+
- Tauri Linux system deps (Debian/Ubuntu): `sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libudev-dev`

`libudev-dev` is needed by the USB HID stack that talks to the security key.

## Run

```
npm install && npm run dev
```

## Build

```
npm run build
```

Bundles (deb, rpm, AppImage) land in `src-tauri/target/release/bundle/`.
