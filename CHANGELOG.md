# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/nikolainyegaard/outlook-for-linux/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/nikolainyegaard/outlook-for-linux/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/nikolainyegaard/outlook-for-linux/releases/tag/v0.2.0
