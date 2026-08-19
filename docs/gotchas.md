# Gotchas

- **WebAuthn is opt-in in WebKitGTK.** The `settings.set_enable_webauthn(true)` call in main.rs looks optional but is the whole point of this app: without it, Microsoft sign-in silently falls back to password or fails on passkey-only accounts. Do not remove it.
- **WebKitGTK WebAuthn is experimental.** USB CTAP2 security keys are the reliable path. Hybrid transport (phone via QR) and platform authenticators may not work; that is a WebKitGTK limitation, not ours. Test sign-in with a real key after any Tauri or webkit2gtk bump.
- **`frontendDist` is a URL on purpose.** There is no local dist folder. Tauri accepts a URL here for remote-only apps; do not "fix" it by adding a dist directory.
- **Placeholder icons.** `src-tauri/icons/` holds generated ImageMagick placeholders (blue square with an O). Replace with real artwork before any public release; regenerate sizes with `npx tauri icon path/to/source.png`.
- **New-window requests (target=_blank links in emails) are not handled yet.** They currently go nowhere in WebKitGTK. Planned: open them in the system browser via a new-window handler.
