fn main() {
    // Declaring the command here autogenerates its ACL permission
    // (allow-webauthn-get-assertion), which capabilities/webauthn.json grants
    // to the Microsoft login origins only. Remote pages always go through the
    // ACL, so without this the polyfill's IPC call is rejected.
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&["webauthn_get_assertion"]),
        ),
    )
    .expect("failed to run tauri-build")
}
