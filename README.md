# Outlook for Linux

Unofficial desktop wrapper for Outlook on the web (OWA), built with Tauri v2. Loads https://outlook.office.com/mail/ in a native WebKitGTK window with working passkey and FIDO2 security key sign-in, which no distro WebKitGTK provides on its own.

A spiritual companion to [teams-for-linux](https://github.com/IsmaelMartinez/teams-for-linux) by Ismael Martinez: the same idea, a native window around a Microsoft web app, for mail instead of Teams.

## Features

- Native window for Outlook on the web, no browser chrome
- FIDO2 USB security key (passkey) sign-in, implemented in the app itself: WebKitGTK ships without WebAuthn, so the app injects a `PublicKeyCredential` polyfill on the Microsoft login pages and drives the key over USB HID (CTAP2) from Rust
- Desktop notifications for new mail with sender, subject and body preview, read from the page itself (OWA never raises web notifications in WebKitGTK)
- Registers as mailto handler: email links open a prefilled OWA compose window; set it as the default email app in your desktop's settings
- Single instance: relaunching focuses the existing window
- Tray icon with unread badge; closing the window keeps the app running in the tray, quit from the tray menu. Ctrl+O, Ctrl+H, Ctrl+R and Ctrl+Q open, hide, refresh and quit from the main window
- Light footprint: no bundled Chromium, uses the system WebKitGTK
- Custom app icon: pick any PNG from the tray menu, optionally copied into your icon theme so the taskbar and launcher follow; see [App icon](#app-icon) for the official logo

## App icon

The tray menu has an icon picker, so the app can wear any icon you like, for example Outlook's own, available on [Wikimedia Commons](https://commons.wikimedia.org/wiki/File:Microsoft_Outlook_Icon_(2025%E2%80%93present).svg):

1. Download the SVG from that page.
2. Convert it to PNG, for example `rsvg-convert -w 512 -h 512 Microsoft_Outlook_Icon.svg -o outlook.png` (rsvg-convert comes with librsvg, installed on most desktops).
3. Tray menu, App Icon, Choose App Icon, pick the PNG. Answer Yes when asked whether to also use it for the taskbar and launcher.

The icon is applied to the tray at once (with the unread badge drawn on top) and remembered in `~/.config/com.nikolainyegaard.outlookforlinux/config.json`. The window, taskbar and launcher icons come from the icon theme, not from the running app, which is what the Yes answer is for: it copies the icon at every hicolor size to `~/.local/share/icons/hicolor/<size>x<size>/apps/outlook-for-linux.png`, and it shows on the next start, on some desktops after you log in again. The checkbox under App Icon toggles those files at any time, and Reset to default removes them along with the custom icon. The theme files are outside the package and survive an uninstall; untick before removing the app to clean them up.

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
