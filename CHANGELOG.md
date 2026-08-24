# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.1] - 2026-08-25

### Fixed

- New-mail notifications actually work now: OWA never raises web notifications in WebKitGTK (its service worker bails out before showing anything), so the app reads the message list itself and shows a notification per new mail with sender, subject and body preview
- The tray unread badge works again: OWA no longer puts the unread count in the tab title, so the app reads the count from the folder pane and writes the title prefix itself
- Notifications carry a desktop-entry hint, so the app shows up in the desktop's per-app notification settings

## [0.4.0] - 2026-08-21

### Added

- Tray icon with Open, Hide, Refresh, and Quit; a red dot and tooltip count show unread mail
- Closing the window hides the app to the tray and it keeps running (notifications and the unread badge stay live); quit from the tray menu

### Changed

- The app now runs via XWayland so the window manager draws the system titlebar with the app icon, replacing the GTK CSS titlebar tweak, which did not take effect

## [0.3.0] - 2026-08-19

### Added

- Desktop notifications: OWA's notification permission is granted and web notifications are shown as system notifications. Also enable notifications inside OWA settings
- mailto handler: the app registers for mailto links and opens OWA's compose window with recipient, cc, bcc, subject and body filled in
- Single instance: launching the app again focuses the existing window instead of starting a second copy

### Changed

- Slimmer titlebar on Wayland, close to system titlebar height (GTK's own titlebar is much taller by default)

## [0.2.0] - 2026-08-19

### Added

- Initial Tauri v2 wrapper for Outlook on the web
- Sign-in with a FIDO2 USB security key, implemented in the app itself since WebKitGTK ships without WebAuthn. Supports PIN-protected keys with retry counts and resident keys with an account picker. Sign-in only, USB keys only; register new keys in a regular browser first
- Outlook app icon

### Changed

- The webview identifies as Chrome on Linux; Microsoft's login page hides the passkey option from unrecognized browsers

[Unreleased]: https://github.com/nikolainyegaard/outlook-for-linux/compare/v0.4.1...HEAD
[0.4.1]: https://github.com/nikolainyegaard/outlook-for-linux/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/nikolainyegaard/outlook-for-linux/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/nikolainyegaard/outlook-for-linux/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/nikolainyegaard/outlook-for-linux/releases/tag/v0.2.0
