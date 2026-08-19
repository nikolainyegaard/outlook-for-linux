// WebAuthn client over CTAP2 USB HID. WebKitGTK 4.1 ships without WebAuthn,
// so the injected polyfill forwards navigator.credentials.get() here and we
// drive the security key ourselves, like Firefox's authenticator-rs does.

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use ctap_hid_fido2::fidokey::get_assertion::get_assertion_params::GetAssertionArgsBuilder;
use ctap_hid_fido2::fidokey::get_info::InfoOption;
use ctap_hid_fido2::{FidoKeyHidFactory, LibCfg};
use serde::{Deserialize, Serialize};

// WebAuthn's security model hangs on origin binding, normally enforced by the
// browser. Here the Tauri capability (webauthn.json) restricts which origins
// can invoke this command at all; this list is the same boundary restated for
// the rpId check. The origin string also ends up in clientDataJSON, which the
// server verifies against its own expected origins, so a spoofed value cannot
// produce a usable assertion for a different site.
const ALLOWED_ORIGINS: &[&str] = &[
    "https://login.microsoftonline.com",
    "https://login.microsoft.com",
    "https://login.live.com",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetRequest {
    pub origin: String,
    pub rp_id: String,
    /// base64url, exactly as the RP sent it
    pub challenge: String,
    /// base64url credential ids; empty means discoverable (resident) credentials
    #[serde(default)]
    pub allow_credentials: Vec<String>,
    #[serde(default)]
    pub user_verification: Option<String>,
    #[serde(default)]
    pub cross_origin: bool,
    #[serde(default)]
    pub pin: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssertionOut {
    pub credential_id: String,
    pub authenticator_data: String,
    pub signature: String,
    pub user_handle: Option<String>,
    pub user_name: String,
    pub user_display_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetResponse {
    pub client_data_json: String,
    pub assertions: Vec<AssertionOut>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebAuthnError {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pin_retries: Option<i32>,
}

impl WebAuthnError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), pin_retries: None }
    }
}

/// Map ctap-hid-fido2's anyhow errors (which embed CTAP status names) to
/// stable codes the polyfill can act on.
fn map_ctap_error(err: anyhow::Error, pin_retries: Option<i32>) -> WebAuthnError {
    let msg = format!("{err:#}");
    let code = if msg.contains("PIN_INVALID") || msg.contains("PIN_AUTH_INVALID") {
        "pin-invalid"
    } else if msg.contains("PIN_REQUIRED") || msg.contains("PIN_NOT_SET") || msg.contains("PUAT_REQUIRED") {
        "pin-required"
    } else if msg.contains("PIN_BLOCKED") || msg.contains("PIN_AUTH_BLOCKED") || msg.contains("UV_BLOCKED") {
        "pin-blocked"
    } else if msg.contains("NO_CREDENTIALS") {
        "no-credentials"
    } else if msg.contains("USER_ACTION_TIMEOUT") || msg.contains("ACTION_TIMEOUT") || msg.contains("KEEPALIVE_CANCEL") {
        "timeout"
    } else if msg.contains("OPERATION_DENIED") {
        "not-allowed"
    } else {
        "internal"
    };
    WebAuthnError { code, message: msg, pin_retries }
}

fn b64_decode(s: &str, what: &str) -> Result<Vec<u8>, WebAuthnError> {
    B64.decode(s)
        .map_err(|e| WebAuthnError::new("bad-request", format!("invalid base64url in {what}: {e}")))
}

#[tauri::command]
pub async fn webauthn_get_assertion(request: GetRequest) -> Result<GetResponse, WebAuthnError> {
    // USB HID traffic blocks on the user touching the key; keep it off the
    // async runtime's core threads.
    tauri::async_runtime::spawn_blocking(move || get_assertion_blocking(request))
        .await
        .map_err(|e| WebAuthnError::new("internal", format!("task join error: {e}")))?
}

/// The security boundary: only allowlisted origins may sign, and the rpId must
/// be the origin's host or a parent domain of it (the same rule a browser
/// applies). The ACL in capabilities/webauthn.json is the first gate; this is
/// the second, so a config slip cannot turn into a signature for any site.
fn validate_origin_and_rpid(origin: &str, rp_id: &str) -> Result<(), WebAuthnError> {
    if !ALLOWED_ORIGINS.contains(&origin) {
        return Err(WebAuthnError::new("not-allowed", format!("origin {origin} is not allowed")));
    }
    let host = origin.strip_prefix("https://").unwrap_or(origin);
    let is_same = host == rp_id;
    // A bare suffix test would accept "com"; require a registrable-looking
    // domain of at least two labels.
    let is_parent = rp_id.contains('.') && host.ends_with(&format!(".{rp_id}"));
    if !is_same && !is_parent {
        return Err(WebAuthnError::new(
            "not-allowed",
            format!("rpId {rp_id} does not match origin {origin}"),
        ));
    }
    Ok(())
}

fn get_assertion_blocking(request: GetRequest) -> Result<GetResponse, WebAuthnError> {
    validate_origin_and_rpid(&request.origin, &request.rp_id)?;

    let allow_ids: Vec<Vec<u8>> = request
        .allow_credentials
        .iter()
        .map(|s| b64_decode(s, "allowCredentials"))
        .collect::<Result<_, _>>()?;
    // Round-trip the challenge to reject garbage early; clientDataJSON carries
    // the original base64url string.
    b64_decode(&request.challenge, "challenge")?;

    let client_data = serde_json::json!({
        "type": "webauthn.get",
        "challenge": request.challenge,
        "origin": request.origin,
        "crossOrigin": request.cross_origin,
    })
    .to_string();

    let cfg = LibCfg::init();
    let key = FidoKeyHidFactory::create(&cfg).map_err(|e| {
        WebAuthnError::new("no-device", format!("no FIDO2 security key found: {e:#}"))
    })?;

    // The crate hashes these bytes with SHA-256 to produce the CTAP
    // clientDataHash, which is exactly SHA-256(clientDataJSON).
    let mut builder = GetAssertionArgsBuilder::new(&request.rp_id, client_data.as_bytes());
    for id in &allow_ids {
        builder = builder.add_credential_id(id);
    }

    let uv_req = request.user_verification.as_deref().unwrap_or("preferred");
    if let Some(pin) = request.pin.as_deref() {
        builder = builder.pin(pin);
    } else if uv_req == "discouraged" {
        builder = builder.without_pin_and_uv();
    } else {
        // No PIN supplied and the RP wants user verification. Built-in UV
        // (fingerprint) can proceed as-is; a PIN-protected key needs the
        // polyfill to collect the PIN and retry.
        let built_in_uv = key.enable_info_option(&InfoOption::Uv).unwrap_or(None);
        if built_in_uv != Some(true) {
            let pin_set = key.enable_info_option(&InfoOption::ClientPin).unwrap_or(None);
            if pin_set == Some(true) {
                return Err(WebAuthnError::new("pin-required", "security key requires its PIN"));
            }
            if uv_req == "required" {
                return Err(WebAuthnError::new(
                    "uv-unavailable",
                    "user verification required but the key has no PIN set and no built-in verification",
                ));
            }
            builder = builder.without_pin_and_uv();
        }
    }

    let assertions = key
        .get_assertion_with_args(&builder.build())
        .map_err(|e| {
            let retries = key.get_pin_retries().ok();
            map_ctap_error(e, retries)
        })?;

    let assertions = assertions
        .into_iter()
        .map(|a| AssertionOut {
            credential_id: B64.encode(&a.credential_id),
            authenticator_data: B64.encode(&a.auth_data),
            signature: B64.encode(&a.signature),
            user_handle: if a.user.id.is_empty() { None } else { Some(B64.encode(&a.user.id)) },
            user_name: a.user.name,
            user_display_name: a.user.display_name,
        })
        .collect();

    Ok(GetResponse {
        client_data_json: B64.encode(client_data.as_bytes()),
        assertions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpid_and_origin_rules() {
        // Real Entra shapes: exact host, and a parent domain of it.
        assert!(validate_origin_and_rpid("https://login.microsoftonline.com", "login.microsoftonline.com").is_ok());
        assert!(validate_origin_and_rpid("https://login.microsoftonline.com", "microsoftonline.com").is_ok());
        // Anything not on the allowlist, whatever the rpId.
        assert!(validate_origin_and_rpid("https://evil.example", "evil.example").is_err());
        assert!(validate_origin_and_rpid("http://login.microsoftonline.com", "login.microsoftonline.com").is_err());
        // Signing for an unrelated or absurdly broad rpId.
        assert!(validate_origin_and_rpid("https://login.microsoftonline.com", "example.com").is_err());
        assert!(validate_origin_and_rpid("https://login.microsoftonline.com", "com").is_err());
        // Suffix lookalike: rpId must be a dot-delimited parent, not a substring.
        assert!(validate_origin_and_rpid("https://login.microsoftonline.com", "onmicrosoftonline.com").is_err());
    }

    /// The ACL is what actually gates remote pages, so test the shipped
    /// capability file rather than a copy of its URLs.
    #[test]
    fn capability_urls_match_real_login_urls() {
        use std::str::FromStr;
        use tauri::utils::acl::RemoteUrlPattern;

        let cap: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/webauthn.json")).unwrap();
        assert_eq!(cap["local"], serde_json::json!(false), "OWA itself must not reach the command");

        let urls = cap["remote"]["urls"].as_array().unwrap();
        assert_eq!(urls.len(), ALLOWED_ORIGINS.len(), "capability and ALLOWED_ORIGINS drifted");

        for u in urls {
            let origin = u.as_str().unwrap();
            assert!(ALLOWED_ORIGINS.contains(&origin), "{origin} missing from ALLOWED_ORIGINS");
            let pattern = RemoteUrlPattern::from_str(origin).unwrap();
            // A pattern with no path must still match a real sign-in URL.
            let real = format!("{origin}/common/oauth2/v2.0/authorize?client_id=abc&response_type=code");
            assert!(pattern.test(&tauri::Url::parse(&real).unwrap()), "{origin} pattern did not match {real}");
            // And must not match a lookalike host.
            let lookalike = origin.replace("https://", "https://") + ".evil.example/x";
            assert!(!pattern.test(&tauri::Url::parse(&lookalike).unwrap()), "{origin} pattern matched {lookalike}");
        }
    }
}
