# Outlook for Linux

Unofficial desktop wrapper for Outlook on the web (OWA), built with Tauri v2. Loads https://outlook.office.com/mail/ in a native WebKitGTK window with working passkey and FIDO2 security key sign-in, which no distro WebKitGTK provides on its own.

A spiritual companion to [teams-for-linux](https://github.com/IsmaelMartinez/teams-for-linux) by Ismael Martinez: the same idea, a native window around a Microsoft web app, for mail instead of Teams.

## Features

- Native window for Outlook on the web, no browser chrome
- FIDO2 USB security key (passkey) sign-in, implemented in the app itself: WebKitGTK ships without WebAuthn, so the app injects a `PublicKeyCredential` polyfill on the Microsoft login pages and drives the key over USB HID (CTAP2) from Rust
- Desktop notifications for new mail with sender, subject and body preview, read from the page itself (OWA never raises web notifications in WebKitGTK). Clicking one opens the message. Settings has switches for the preview, the sound, notifications only while the window is hidden, the tray badge, and per profile
- Registers as mailto handler: email links open a prefilled OWA compose window; set it as the default email app in your desktop's settings
- Single instance: relaunching focuses the existing window
- Tray icon with unread badge; closing the window keeps the app running in the tray, quit from the tray menu or from File. Ctrl+H, Ctrl+R and Ctrl+Q hide, refresh and quit from the main window
- Menubar: File (new message in its own window, hide, quit), Edit, View (refresh, zoom with Ctrl+=, Ctrl+- and Ctrl+0) and Help
- Profiles: several accounts in one window, switched from the Profile dropdown at the right end of the menubar; see [Profiles](#profiles)
- Light footprint: no bundled Chromium, uses the system WebKitGTK
- Custom app icon: pick any PNG or SVG under Settings, optionally copied into your icon theme so the taskbar and launcher follow; see [App icon](#app-icon) for the official logo

## App icon

Settings has an icon picker, so the app can wear any icon you like, for example Outlook's own, available on [Wikimedia Commons](https://commons.wikimedia.org/wiki/File:Microsoft_Outlook_Icon_(2025%E2%80%93present).svg):

1. Download the SVG from that page.
2. Edit, Settings (Ctrl+,), General, App icon, Choose, pick the file (PNG and SVG both work). Answer Yes when asked whether to also use it for the taskbar and launcher.

The icon is applied to the tray at once (with the unread badge drawn on top) and remembered in `~/.config/com.nikolainyegaard.outlookforlinux/config.json`. The window, taskbar and launcher icons come from the icon theme, not from the running app, which is what the Yes answer is for: it copies the icon at every hicolor size to `~/.local/share/icons/hicolor/<size>x<size>/apps/outlook-for-linux.png`, and it shows on the next start, on some desktops after you log in again. The checkbox under Settings toggles those files at any time, and Reset removes them along with the custom icon. The theme files are outside the package and survive an uninstall; untick before removing the app to clean them up.

## Profiles

Each profile is a separate sign-in with its own cookies and storage, for example Work and Personal. The Profile dropdown at the right end of the menubar switches between them; Add Profile at the bottom of it creates a new one and opens the Microsoft login. Your existing sign-in becomes the first profile, nothing to redo.

Switching is meant for "done with work, over to personal", not for juggling: OWA is heavy (several hundred MB per loaded profile), so only the selected profile is shown and a deselected one stays loaded for 15 minutes before it is unloaded. Switching back within that time is instant; after it, the profile reloads, still signed in. The dot next to each name shows this: green is selected, amber is loaded in the background (with its unread count), grey is unloaded. The tray badge and taskbar count follow the selected profile only; notifications from a background profile carry its name.

Edit, Settings (Ctrl+,) has the rest. Under General, Background profiles sets how long a profile you switched away from stays loaded (immediately, 5 minutes to an hour, or never, which keeps every profile loaded for the most RAM, instant switching and notifications from all of them). Under Profiles, each profile has its own page: rename it, see whether it is loaded and switch to it, tick Keep loaded to exempt it from unloading, or remove it (this deletes its sign-in and local data from the computer). Add profile + at the bottom of the list creates a new one. The same values live in `~/.config/com.nikolainyegaard.outlookforlinux/config.json` as `unload_after_minutes` (`0`, minutes, or `null` for never) and `keep_awake` per profile.

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
