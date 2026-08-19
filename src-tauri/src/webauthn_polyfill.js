// WebAuthn polyfill for WebKitGTK builds without WebAuthn. Forwards
// navigator.credentials.get() to the Rust CTAP2 client via Tauri IPC.
// Injected at document start into every page; IPC access is limited to the
// Microsoft login origins by the Tauri capability, and the Rust side
// re-checks origin and rpId.
(() => {
  'use strict';
  if (window.PublicKeyCredential) return;

  // Injected into every frame, so install only where IPC is actually granted
  // (see capabilities/webauthn.json and ALLOWED_ORIGINS in webauthn.rs).
  // Elsewhere, leaving WebAuthn undefined is more honest than advertising an
  // API whose calls the ACL would reject.
  const ALLOWED_ORIGINS = [
    'https://login.microsoftonline.com',
    'https://login.microsoft.com',
    'https://login.live.com',
  ];
  if (!ALLOWED_ORIGINS.includes(location.origin)) return;
  if (!window.__TAURI_INTERNALS__) return;

  const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args);

  const b64uEncode = (buf) => {
    const bytes = new Uint8Array(buf);
    let bin = '';
    for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
    return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  };
  const b64uDecode = (s) => {
    s = s.replace(/-/g, '+').replace(/_/g, '/');
    if (s.length % 4) s += '='.repeat(4 - (s.length % 4));
    const bin = atob(s);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    return bytes.buffer;
  };
  const toBuffer = (v) => (v instanceof ArrayBuffer ? v : ArrayBuffer.isView(v) ? v.buffer.slice(v.byteOffset, v.byteOffset + v.byteLength) : v);

  // ---- overlay UI (shadow DOM so page CSS cannot interfere) ----
  let host = null;
  function overlay(html) {
    if (!host) {
      host = document.createElement('div');
      host.attachShadow({ mode: 'open' });
      (document.body || document.documentElement).appendChild(host);
    }
    host.shadowRoot.innerHTML = `
      <style>
        .backdrop { position: fixed; inset: 0; background: rgba(0,0,0,.45); z-index: 2147483647;
                    display: flex; align-items: center; justify-content: center;
                    font-family: system-ui, sans-serif; }
        .card { background: #fff; color: #222; border-radius: 10px; padding: 24px 28px;
                max-width: 340px; box-shadow: 0 8px 30px rgba(0,0,0,.35); text-align: center; }
        .card h3 { margin: 0 0 8px; font-size: 16px; }
        .card p { margin: 0 0 14px; font-size: 13px; color: #555; }
        .key { font-size: 34px; margin-bottom: 10px; }
        input { width: 100%; box-sizing: border-box; padding: 8px; font-size: 15px;
                border: 1px solid #bbb; border-radius: 6px; margin-bottom: 12px; text-align: center; }
        button { font-size: 13px; padding: 7px 16px; border-radius: 6px; border: 1px solid #bbb;
                 background: #f5f5f5; cursor: pointer; margin: 0 4px; }
        button.primary { background: #0f6cbd; border-color: #0f6cbd; color: #fff; }
        .list button { display: block; width: 100%; margin: 6px 0; text-align: left; padding: 10px 12px; }
        .err { color: #b00020; font-size: 12px; margin: 0 0 10px; }
      </style>
      <div class="backdrop"><div class="card">${html}</div></div>`;
    return host.shadowRoot;
  }
  function closeOverlay() {
    if (host) { host.remove(); host = null; }
  }

  function showTouch() {
    overlay(`<div class="key">🔑</div><h3>Touch your security key</h3>
             <p>Confirm sign-in on your security key.</p>
             <button id="cancel">Cancel</button>`)
      .getElementById('cancel').onclick = () => { closeOverlay(); rejectActive('NotAllowedError', 'The user cancelled the operation.'); };
  }

  function askPin(errText, retries) {
    return new Promise((resolve) => {
      const note = errText
        ? `<p class="err">${errText}${retries != null ? ` (${retries} tries left)` : ''}</p>`
        : '';
      const root = overlay(`<div class="key">🔑</div><h3>Security key PIN</h3>${note}
        <input id="pin" type="password" autocomplete="off" inputmode="numeric">
        <div><button id="cancel">Cancel</button><button id="ok" class="primary">Continue</button></div>`);
      const pin = root.getElementById('pin');
      pin.focus();
      root.getElementById('ok').onclick = () => resolve(pin.value || null);
      pin.onkeydown = (e) => { if (e.key === 'Enter') resolve(pin.value || null); };
      root.getElementById('cancel').onclick = () => resolve(null);
    });
  }

  function pickAccount(assertions) {
    return new Promise((resolve) => {
      const items = assertions
        .map((a, i) => `<button data-i="${i}">${(a.userDisplayName || a.userName || 'Account ' + (i + 1))
          .replace(/[<>&]/g, '')}</button>`)
        .join('');
      const root = overlay(`<h3>Choose an account</h3><div class="list">${items}</div>
                            <button id="cancel">Cancel</button>`);
      root.querySelectorAll('.list button').forEach((b) => (b.onclick = () => resolve(assertions[+b.dataset.i])));
      root.getElementById('cancel').onclick = () => resolve(null);
    });
  }

  // Active-request bookkeeping so Cancel buttons and AbortSignals can reject.
  let rejectActive = () => {};

  // ---- WebAuthn API surface ----
  class AuthenticatorResponse {}
  class AuthenticatorAssertionResponse extends AuthenticatorResponse {}
  class AuthenticatorAttestationResponse extends AuthenticatorResponse {}

  class PublicKeyCredential {
    static isUserVerifyingPlatformAuthenticatorAvailable() { return Promise.resolve(false); }
    static isConditionalMediationAvailable() { return Promise.resolve(false); }
    static getClientCapabilities() {
      return Promise.resolve({ conditionalGet: false, hybridTransport: false, userVerifyingPlatformAuthenticator: false });
    }
    getClientExtensionResults() { return {}; }
    toJSON() {
      return {
        id: this.id, rawId: this.id, type: this.type,
        authenticatorAttachment: this.authenticatorAttachment,
        clientExtensionResults: {},
        response: {
          clientDataJSON: b64uEncode(this.response.clientDataJSON),
          authenticatorData: b64uEncode(this.response.authenticatorData),
          signature: b64uEncode(this.response.signature),
          userHandle: this.response.userHandle ? b64uEncode(this.response.userHandle) : null,
        },
      };
    }
  }

  async function get(options) {
    const pk = options && options.publicKey;
    if (!pk) throw new TypeError('options.publicKey is required');
    if (options.signal && options.signal.aborted) throw new DOMException('Aborted.', 'AbortError');

    const request = {
      origin: location.origin,
      rpId: pk.rpId || location.hostname,
      challenge: b64uEncode(toBuffer(pk.challenge)),
      allowCredentials: (pk.allowCredentials || []).map((c) => b64uEncode(toBuffer(c.id))),
      userVerification: pk.userVerification || 'preferred',
      crossOrigin: window.self !== window.top,
      pin: null,
    };
    // Visible only in debug builds (console-to-stdout is enabled there).
    console.log('[webauthn] get() rpId=' + request.rpId + ' allowCredentials=' + request.allowCredentials.length + ' uv=' + request.userVerification);

    const done = new Promise((resolve, reject) => {
      rejectActive = (name, msg) => reject(new DOMException(msg, name));
      if (options.signal) {
        options.signal.addEventListener('abort', () => { closeOverlay(); reject(new DOMException('Aborted.', 'AbortError')); }, { once: true });
      }
      (async () => {
        let pinErr = null, pinRetries = null;
        for (;;) {
          try {
            showTouch();
            resolve(await invoke('webauthn_get_assertion', { request }));
            return;
          } catch (e) {
            const code = e && e.code;
            console.error('[webauthn] error code=' + code + ' message=' + ((e && e.message) || JSON.stringify(e)));
            if (code === 'pin-required' || code === 'pin-invalid') {
              if (code === 'pin-invalid') { pinErr = 'Wrong PIN.'; pinRetries = e.pinRetries; }
              const pin = await askPin(pinErr, pinRetries);
              if (pin === null) { reject(new DOMException('The user cancelled the operation.', 'NotAllowedError')); return; }
              request.pin = pin;
              continue;
            }
            if (code === 'no-device') {
              const root = overlay(`<div class="key">🔑</div><h3>No security key found</h3>
                <p>Insert your security key, then try again.</p>
                <div><button id="cancel">Cancel</button><button id="retry" class="primary">Try again</button></div>`);
              const again = await new Promise((r) => {
                root.getElementById('retry').onclick = () => r(true);
                root.getElementById('cancel').onclick = () => r(false);
              });
              if (again) continue;
              reject(new DOMException('No authenticator available.', 'NotAllowedError'));
              return;
            }
            const messages = {
              'pin-blocked': 'The security key PIN is blocked. Unplug and reinsert the key.',
              'no-credentials': 'This security key holds no passkey for this account.',
              'timeout': 'The security key timed out.',
              'uv-unavailable': 'This key has no PIN set; set a PIN on it first.',
            };
            reject(new DOMException(messages[code] || (e && e.message) || 'WebAuthn request failed.', 'NotAllowedError'));
            return;
          }
        }
      })();
    });

    let res;
    try {
      res = await done;
    } finally {
      closeOverlay();
      rejectActive = () => {};
    }

    let assertion = res.assertions[0];
    if (res.assertions.length > 1) {
      assertion = await pickAccount(res.assertions);
      closeOverlay();
      if (!assertion) throw new DOMException('The user cancelled the operation.', 'NotAllowedError');
    }
    if (!assertion) throw new DOMException('No assertion returned.', 'NotAllowedError');

    const cred = Object.create(PublicKeyCredential.prototype);
    const response = Object.create(AuthenticatorAssertionResponse.prototype);
    Object.defineProperties(response, {
      clientDataJSON: { value: b64uDecode(res.clientDataJson), enumerable: true },
      authenticatorData: { value: b64uDecode(assertion.authenticatorData), enumerable: true },
      signature: { value: b64uDecode(assertion.signature), enumerable: true },
      userHandle: { value: assertion.userHandle ? b64uDecode(assertion.userHandle) : null, enumerable: true },
    });
    Object.defineProperties(cred, {
      id: { value: assertion.credentialId, enumerable: true },
      rawId: { value: b64uDecode(assertion.credentialId), enumerable: true },
      type: { value: 'public-key', enumerable: true },
      authenticatorAttachment: { value: 'cross-platform', enumerable: true },
      response: { value: response, enumerable: true },
    });
    return cred;
  }

  async function create() {
    // ponytail: sign-in only; registration needs makeCredential + attestation
    // passthrough. Add when someone needs to enroll a key from this app.
    throw new DOMException('Registering a security key is not supported here yet. Register it in a regular browser, then sign in.', 'NotSupportedError');
  }

  window.PublicKeyCredential = PublicKeyCredential;
  window.AuthenticatorResponse = AuthenticatorResponse;
  window.AuthenticatorAssertionResponse = AuthenticatorAssertionResponse;
  window.AuthenticatorAttestationResponse = AuthenticatorAttestationResponse;
  Object.defineProperty(navigator, 'credentials', {
    value: {
      get,
      create,
      store: async () => { throw new DOMException('Not supported.', 'NotSupportedError'); },
      preventSilentAccess: async () => {},
    },
    configurable: true,
  });
})();
