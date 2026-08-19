# Architecture

Tauri v2 desktop wrapper around Outlook on the web. There is no local frontend: the main window loads https://outlook.office.com/mail/ directly, and `build.frontendDist` in tauri.conf.json points at the same URL. All app logic lives in the Rust side.

## File tree

```
outlook-for-linux/
├── package.json             # npm scripts wrapping the Tauri CLI
├── README.md
├── CHANGELOG.md
├── docs/
│   ├── architecture.md
│   ├── config.md
│   └── gotchas.md
└── src-tauri/
    ├── Cargo.toml           # tauri 2, webkit2gtk (Linux only, feature v2_40)
    ├── build.rs
    ├── tauri.conf.json      # window, remote URL, bundle targets
    ├── capabilities/
    │   └── default.json     # core:default for the main window, no custom IPC
    ├── icons/               # placeholder icons, replace before release
    └── src/
        └── main.rs          # setup hook enables WebAuthn on the webview
```

## How it works

`main.rs` builds a default Tauri app. In the setup hook, on Linux only, it grabs the main webview via `with_webview` and flips WebKitGTK's `enable-webauthn` setting to true. That single setting is what makes passkeys and FIDO2 security keys work during Microsoft sign-in; everything else is stock Tauri.

There is no IPC surface: the remote page cannot invoke Rust commands, and none are defined.
