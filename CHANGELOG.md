# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Update check: on every launch by default, Outlook asks the GitHub releases API (one `curl` request, nothing downloaded) and shows a desktop notification when a newer version exists; the click opens Settings, General, Updates. The page has the automatic check switch and interval (every launch, daily, weekly, monthly), Check Now, a Release page link and Update Now, which replaces this copy the way it was installed: `dnf` or `apt` through polkit for packages, in place for the AppImage, then restarts. Help > Check for Updates… opens the page and checks. `updates` in config.json
- Notifications setting to hide the unread counts next to background profiles in the Profile dropdown (`switcher_unread`)

### Fixed

- Scrolling over a Settings page that fits no longer shows the move cursor
- Clicking a notification, or Open in the tray, no longer stacks a loaded background profile under the selected one in the same window

## [0.7.0] - 2026-10-24

### Added

- Menubar on the main window: File (New Message, Hide, Quit), Edit (Cut, Copy, Paste, Select All), View (Refresh, Zoom In, Zoom Out, Actual Size) and Help (GitHub Repository, About)
- Ctrl+=, Ctrl+- and Ctrl+0 zoom the page
- Profiles: several accounts in one window, each with its own sign-in and storage, switched from a Profile dropdown at the right end of the menubar; the existing sign-in becomes the first profile. Deselected profiles unload after 15 minutes (`unload_after_minutes` in config.json: 0 at once, null never, `keep_awake` per profile), status dots and background unread counts in the dropdown, notifications from background profiles carry the profile name
- Settings window (Edit > Settings, Ctrl+,) laid out like Outlook's: categories and search on the left, pages in the middle. Profiles lists every profile plus Add profile +, each with its own page (name, status with Switch to, keep loaded, remove); General holds Notifications, Background profiles (the unload timeout) and the app icon picker with the theme copy toggle. Replaces the App Icon submenu in the tray menu
- Clicking a new-mail notification opens that message: the window comes forward, the profile is selected and the message is shown in the reading pane
- Notification settings: master switch, only while the window is hidden or not focused, include the message preview, desktop sound, red dot on the tray icon, and a per-profile switch on each profile's page

### Changed

- Ctrl+H, Ctrl+R and Ctrl+Q are the menubar's shortcuts now; Ctrl+O is gone, Open stays in the tray menu

## [0.6.1] - 2026-10-23

### Changed

- The icon picker takes SVG files directly, no conversion to PNG needed

## [0.6.0] - 2026-10-23

### Added

- Ctrl+O, Ctrl+H, Ctrl+R and Ctrl+Q open, hide, refresh and quit from the main window, shown next to the tray menu items
- App Icon in the tray menu: choose a PNG for the tray (unread badge included), optionally copied into the user icon theme so the window, taskbar and launcher follow; a checkbox toggles the theme copy and Reset to default removes everything again

### Changed

- New envelope app icon; the tray menu can swap it for any PNG, Outlook's official icon included

## [0.5.0] - 2026-10-23

### Added

- Releases are built by GitHub Actions on a version tag, for x86_64 and aarch64: deb, rpm, AppImage, tarball and checksums

### Fixed

- Pasting from other apps into a message works with Ctrl+V (previously only Ctrl+Shift+V worked); the XWayland clipboard bridge hid the text from the editor

### Changed

- The app runs native Wayland; the window manager still draws the slim system titlebar with the app icon
- Package and bundle files are now named outlook-for-linux instead of "Outlook for Linux"; the launcher entry keeps its name

## [0.4.3] - 2026-10-13

### Fixed

- Links in emails open on the first click; previously the first click did nothing and only a second click opened the browser

## [0.4.2] - 2026-09-24

### Fixed

- Links in emails (and anything else OWA opens in a new tab) open in the system browser instead of doing nothing

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

[Unreleased]: https://github.com/nikolainyegaard/outlook-for-linux/compare/v0.7.0...HEAD
[0.7.0]: https://github.com/nikolainyegaard/outlook-for-linux/compare/v0.6.1...v0.7.0
[0.6.1]: https://github.com/nikolainyegaard/outlook-for-linux/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/nikolainyegaard/outlook-for-linux/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/nikolainyegaard/outlook-for-linux/compare/v0.4.3...v0.5.0
[0.4.3]: https://github.com/nikolainyegaard/outlook-for-linux/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/nikolainyegaard/outlook-for-linux/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/nikolainyegaard/outlook-for-linux/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/nikolainyegaard/outlook-for-linux/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/nikolainyegaard/outlook-for-linux/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/nikolainyegaard/outlook-for-linux/releases/tag/v0.2.0
