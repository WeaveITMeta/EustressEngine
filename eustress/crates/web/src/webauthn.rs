// =============================================================================
// Eustress Web - WebAuthn (passkeys) in the browser
// =============================================================================
// Passkeys sign in to the website with no identity file. The browser's
// `navigator.credentials.create` and `.get` do the work; this file calls them
// through `js_sys::Reflect` (no extra web_sys features), and keeps everything
// that can be decided without a browser as plain, tested functions:
//
//   1. base64url (no padding), the form every binary value takes on the wire
//   2. the options the browser is given, built from the Worker's "begin" answer
//   3. the JSON body the Worker is sent, built from what the browser returns
//   4. the sentences for what can go wrong
//   5. the calls into the browser
//
// Contract: docs/architecture/PASSKEYS.md (sections 3 and 4).
// =============================================================================

use serde_json::{json, Value};

// -----------------------------------------------------------------------------
// 1. base64url
// -----------------------------------------------------------------------------

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// base64url without padding (RFC 4648 section 5).
pub fn b64url_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() * 4 + 2) / 3);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[n as usize & 63] as char);
        }
    }
    out
}

/// The bytes of base64url text. Padding, `+`, `/`, spaces and a length no byte string can have
/// (one more than a multiple of four) are all refused: the Worker never sends them.
pub fn b64url_decode(text: &str) -> Result<Vec<u8>, String> {
    let bad = || "That value is not base64url text.".to_string();
    if text.len() % 4 == 1 {
        return Err(bad());
    }
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = ALPHABET.iter().position(|a| *a == c).ok_or_else(bad)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

/// The 32 bytes a Worker challenge stands for. The Worker sends it as base64url (43 characters);
/// those bytes are the WebAuthn challenge, and the request bodies send the text back unchanged.
pub fn challenge_bytes(challenge: &str) -> Result<Vec<u8>, String> {
    let bytes = b64url_decode(challenge).map_err(|_| "The challenge from Eustress was not valid.".to_string())?;
    if bytes.len() != 32 {
        return Err("The challenge from Eustress was not the expected length.".to_string());
    }
    Ok(bytes)
}

/// What the identity file signs in place of the challenge text: its 32 bytes as 64 lower-case hex
/// digits (the Worker builds its copy of the add and remove messages from this form).
pub fn challenge_hex(challenge: &str) -> Result<String, String> {
    challenge_bytes(challenge).map(hex::encode)
}

// -----------------------------------------------------------------------------
// 2. The options the browser is given
// -----------------------------------------------------------------------------

/// What `navigator.credentials.create` is asked for, with every binary value as bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct CreationOptions {
    pub challenge: Vec<u8>,
    pub rp_id: String,
    pub rp_name: String,
    pub user_id: Vec<u8>,
    pub user_name: String,
    pub user_display_name: String,
    /// COSE algorithm numbers, in the order the Worker prefers them.
    pub algorithms: Vec<i64>,
    /// Passkeys already on the account, so the device does not make a second one.
    pub exclude: Vec<Vec<u8>>,
    pub timeout_ms: u32,
    pub resident_key: String,
    pub user_verification: String,
    pub attestation: String,
}

/// What `navigator.credentials.get` is asked for. There is no allow list: the passkey is
/// discoverable, so the device finds it.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestOptions {
    pub challenge: Vec<u8>,
    pub rp_id: String,
    pub timeout_ms: u32,
    pub user_verification: String,
}

impl CreationOptions {
    /// From the answer to `POST /api/passkeys/add/begin`.
    pub fn from_begin(begin: &crate::api::PasskeyAddBegin) -> Result<Self, String> {
        if begin.pub_key_cred_params.is_empty() {
            return Err("Eustress named no passkey type this browser could use.".to_string());
        }
        Ok(Self {
            challenge: challenge_bytes(&begin.challenge)?,
            rp_id: begin.rp.id.clone(),
            rp_name: begin.rp.name.clone(),
            user_id: b64url_decode(&begin.user.id)?,
            user_name: begin.user.name.clone(),
            user_display_name: if begin.user.display_name.is_empty() { begin.user.name.clone() } else { begin.user.display_name.clone() },
            algorithms: begin.pub_key_cred_params.clone(),
            exclude: begin.exclude_credentials.iter().map(|id| b64url_decode(id)).collect::<Result<_, _>>()?,
            timeout_ms: begin.timeout_ms,
            resident_key: begin.authenticator_selection.resident_key.clone(),
            user_verification: begin.authenticator_selection.user_verification.clone(),
            attestation: begin.attestation.clone(),
        })
    }
}

impl RequestOptions {
    /// From the answer to `POST /api/passkeys/login/begin`.
    pub fn from_begin(begin: &crate::api::PasskeyLoginBegin) -> Result<Self, String> {
        Ok(Self {
            challenge: challenge_bytes(&begin.challenge)?,
            rp_id: begin.rp_id.clone(),
            timeout_ms: begin.timeout_ms,
            user_verification: begin.user_verification.clone(),
        })
    }
}

// -----------------------------------------------------------------------------
// 3. The body the Worker is sent
// -----------------------------------------------------------------------------

/// What `navigator.credentials.create` gave back, as bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct CreatedParts {
    pub raw_id: Vec<u8>,
    pub client_data_json: Vec<u8>,
    pub attestation_object: Vec<u8>,
    pub transports: Vec<String>,
}

/// What `navigator.credentials.get` gave back, as bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct AssertedParts {
    pub raw_id: Vec<u8>,
    pub client_data_json: Vec<u8>,
    pub authenticator_data: Vec<u8>,
    pub signature: Vec<u8>,
    pub user_handle: Option<Vec<u8>>,
}

impl CreatedParts {
    /// The credential id as the Worker and the add message write it: base64url of the raw id.
    pub fn id(&self) -> String {
        b64url_encode(&self.raw_id)
    }

    /// `credential` of `POST /api/passkeys/add`.
    pub fn to_json(&self) -> Value {
        let mut response = json!({
            "client_data_json": b64url_encode(&self.client_data_json),
            "attestation_object": b64url_encode(&self.attestation_object),
        });
        if !self.transports.is_empty() {
            response["transports"] = json!(self.transports);
        }
        json!({ "id": self.id(), "raw_id": self.id(), "type": "public-key", "response": response })
    }
}

impl AssertedParts {
    pub fn id(&self) -> String {
        b64url_encode(&self.raw_id)
    }

    /// `credential` of `POST /api/passkeys/login`.
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id(),
            "raw_id": self.id(),
            "type": "public-key",
            "response": {
                "client_data_json": b64url_encode(&self.client_data_json),
                "authenticator_data": b64url_encode(&self.authenticator_data),
                "signature": b64url_encode(&self.signature),
                "user_handle": self.user_handle.as_ref().map(|h| b64url_encode(h)),
            }
        })
    }
}

/// The body of `POST /api/passkeys/add`.
pub fn add_body(challenge: &str, name: &str, created: &CreatedParts, signature: &str) -> Value {
    json!({ "challenge": challenge, "name": name, "credential": created.to_json(), "signature": signature })
}

/// The body of `POST /api/passkeys/login`.
pub fn login_body(challenge: &str, asserted: &AssertedParts) -> Value {
    json!({ "challenge": challenge, "credential": asserted.to_json() })
}

/// The most bytes of a passkey's name the Worker keeps.
pub const MAX_NAME_BYTES: usize = 64;

/// The name to send: trimmed, control characters turned into spaces, at most 64 bytes (cut
/// between characters), and `Passkey` when nothing is left.
pub fn clean_name(raw: &str) -> String {
    let spaced: String = raw.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let mut name = String::new();
    for c in spaced.trim().chars() {
        if name.len() + c.len_utf8() > MAX_NAME_BYTES {
            break;
        }
        name.push(c);
    }
    let name = name.trim().to_string();
    if name.is_empty() { "Passkey".to_string() } else { name }
}

// -----------------------------------------------------------------------------
// 4. What can go wrong, in words
// -----------------------------------------------------------------------------

/// Shown where a browser has no passkeys.
pub const NO_WEBAUTHN: &str = "This browser cannot use passkeys.";

/// The sentence for a failure of `navigator.credentials.create` or `.get`, from the
/// `DOMException` name and message the browser gave.
pub fn browser_failure(name: &str, message: &str, making: bool) -> String {
    match name {
        "NotAllowedError" | "AbortError" => {
            if making {
                "The passkey was not made: the prompt was closed, refused or timed out. Nothing was changed.".to_string()
            } else {
                "The passkey prompt was closed, refused or timed out. You are not signed in.".to_string()
            }
        }
        "InvalidStateError" => "This device already has a passkey for your account. Nothing was changed.".to_string(),
        "NotSupportedError" | "SecurityError" => {
            "This browser or this address cannot use passkeys here. Try another browser, or sign in with your identity file.".to_string()
        }
        _ => {
            let why = if message.is_empty() { name } else { message };
            if making {
                format!("The browser could not make the passkey: {why}")
            } else {
                format!("The browser could not use the passkey: {why}")
            }
        }
    }
}

// -----------------------------------------------------------------------------
// 5. The calls into the browser
// -----------------------------------------------------------------------------

use js_sys::{Array, Function, Object, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};

/// Whether this browser has WebAuthn at all (`window.PublicKeyCredential`). The sign-in button
/// is hidden when it does not.
pub fn webauthn_available() -> bool {
    web_sys::window().map(|w| Reflect::has(&w, &"PublicKeyCredential".into()).unwrap_or(false)).unwrap_or(false)
}

fn bytes_js(bytes: &[u8]) -> JsValue {
    Uint8Array::from(bytes).into()
}

fn put(object: &Object, key: &str, value: impl Into<JsValue>) {
    let _ = Reflect::set(object, &key.into(), &value.into());
}

fn object_of(pairs: Vec<(&str, JsValue)>) -> Object {
    let object = Object::new();
    for (key, value) in pairs {
        put(&object, key, value);
    }
    object
}

fn descriptor(id: &[u8]) -> JsValue {
    object_of(vec![("type", "public-key".into()), ("id", bytes_js(id))]).into()
}

impl CreationOptions {
    /// The `publicKey` object `navigator.credentials.create` takes.
    pub fn to_js(&self) -> JsValue {
        let params = Array::new();
        for alg in &self.algorithms {
            params.push(&object_of(vec![("type", "public-key".into()), ("alg", JsValue::from_f64(*alg as f64))]));
        }
        let exclude = Array::new();
        for id in &self.exclude {
            exclude.push(&descriptor(id));
        }
        object_of(vec![
            ("challenge", bytes_js(&self.challenge)),
            ("rp", object_of(vec![("id", self.rp_id.as_str().into()), ("name", self.rp_name.as_str().into())]).into()),
            (
                "user",
                object_of(vec![
                    ("id", bytes_js(&self.user_id)),
                    ("name", self.user_name.as_str().into()),
                    ("displayName", self.user_display_name.as_str().into()),
                ])
                .into(),
            ),
            ("pubKeyCredParams", params.into()),
            ("timeout", JsValue::from_f64(f64::from(self.timeout_ms))),
            ("excludeCredentials", exclude.into()),
            (
                "authenticatorSelection",
                object_of(vec![
                    ("residentKey", self.resident_key.as_str().into()),
                    ("requireResidentKey", JsValue::from_bool(self.resident_key == "required")),
                    ("userVerification", self.user_verification.as_str().into()),
                ])
                .into(),
            ),
            ("attestation", self.attestation.as_str().into()),
        ])
        .into()
    }
}

impl RequestOptions {
    /// The `publicKey` object `navigator.credentials.get` takes.
    pub fn to_js(&self) -> JsValue {
        object_of(vec![
            ("challenge", bytes_js(&self.challenge)),
            ("rpId", self.rp_id.as_str().into()),
            ("timeout", JsValue::from_f64(f64::from(self.timeout_ms))),
            ("userVerification", self.user_verification.as_str().into()),
        ])
        .into()
    }
}

fn text_of(value: &JsValue, key: &str) -> String {
    Reflect::get(value, &key.into()).ok().and_then(|v| v.as_string()).unwrap_or_default()
}

/// `navigator.credentials.<method>({ publicKey })`, awaited.
async fn call_credentials(method: &str, public_key: JsValue, making: bool) -> Result<JsValue, String> {
    let window = web_sys::window().ok_or_else(|| NO_WEBAUTHN.to_string())?;
    let navigator = Reflect::get(&window, &"navigator".into()).map_err(|_| NO_WEBAUTHN.to_string())?;
    let container = Reflect::get(&navigator, &"credentials".into()).map_err(|_| NO_WEBAUTHN.to_string())?;
    if container.is_undefined() || container.is_null() {
        return Err(NO_WEBAUTHN.to_string());
    }
    let function: Function = Reflect::get(&container, &method.into())
        .ok()
        .and_then(|f| f.dyn_into().ok())
        .ok_or_else(|| NO_WEBAUTHN.to_string())?;
    let argument = object_of(vec![("publicKey", public_key)]);
    let promise = function
        .call1(&container, &argument)
        .map_err(|e| browser_failure(&text_of(&e, "name"), &text_of(&e, "message"), making))?;
    let promise: js_sys::Promise = promise.dyn_into().map_err(|_| NO_WEBAUTHN.to_string())?;
    wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(|e| browser_failure(&text_of(&e, "name"), &text_of(&e, "message"), making))
}

fn bytes_field(value: &JsValue, key: &str) -> Result<Vec<u8>, String> {
    let field = Reflect::get(value, &key.into()).map_err(|_| format!("The browser's answer had no {key}."))?;
    if field.is_undefined() || field.is_null() {
        return Err(format!("The browser's answer had no {key}."));
    }
    Ok(Uint8Array::new(&field).to_vec())
}

fn response_of(credential: &JsValue) -> Result<JsValue, String> {
    Reflect::get(credential, &"response".into())
        .ok()
        .filter(|r| r.is_object())
        .ok_or_else(|| "The browser's answer had no response.".to_string())
}

/// Ask the device for a new passkey.
pub async fn create_credential(options: &CreationOptions) -> Result<CreatedParts, String> {
    let credential = call_credentials("create", options.to_js(), true).await?;
    let response = response_of(&credential)?;
    let transports = Reflect::get(&response, &"getTransports".into())
        .ok()
        .and_then(|f| f.dyn_into::<Function>().ok())
        .and_then(|f| f.call0(&response).ok())
        .map(|list| Array::from(&list).iter().filter_map(|t| t.as_string()).collect())
        .unwrap_or_default();
    Ok(CreatedParts {
        raw_id: bytes_field(&credential, "rawId")?,
        client_data_json: bytes_field(&response, "clientDataJSON")?,
        attestation_object: bytes_field(&response, "attestationObject")?,
        transports,
    })
}

/// Ask the device to sign in with a passkey it holds for this site.
pub async fn get_credential(options: &RequestOptions) -> Result<AssertedParts, String> {
    let credential = call_credentials("get", options.to_js(), false).await?;
    let response = response_of(&credential)?;
    let handle = Reflect::get(&response, &"userHandle".into()).ok().filter(|h| !h.is_undefined() && !h.is_null());
    Ok(AssertedParts {
        raw_id: bytes_field(&credential, "rawId")?,
        client_data_json: bytes_field(&response, "clientDataJSON")?,
        authenticator_data: bytes_field(&response, "authenticatorData")?,
        signature: bytes_field(&response, "signature")?,
        user_handle: handle.map(|h| Uint8Array::new(&h).to_vec()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{PasskeyAddBegin, PasskeyLoginBegin};

    /// A challenge as the Worker sends it, and the hex form of the same 32 bytes.
    const CHALLENGE: &str = "cvysYD6JJR6k_cTpDEWWitdMW5IgH3esHrNnGKUHlZU";
    const CHALLENGE_HEX: &str = "72fcac603e89251ea4fdc4e90c45968ad74c5b92201f77ac1eb36718a5079595";

    fn add_begin() -> PasskeyAddBegin {
        serde_json::from_value(json!({
            "challenge": CHALLENGE,
            "rp": { "id": "eustress.dev", "name": "Eustress" },
            "user": { "id": "MTExMTExMTEtMjIyMi00MzMzLTg0NDQtNTU1NTU1NTU1NTU1", "name": "ada", "display_name": "Ada" },
            "pub_key_cred_params": [-8, -7, -257],
            "exclude_credentials": ["AQID", "BAUG"],
            "timeout_ms": 300000,
            "authenticator_selection": { "resident_key": "required", "user_verification": "required" },
            "attestation": "none",
            "api_origin": "https://api.eustress.dev"
        }))
        .expect("the begin answer parses")
    }

    #[test]
    fn base64url_matches_the_rfc_vectors_and_uses_the_url_alphabet() {
        for (plain, coded) in [("", ""), ("f", "Zg"), ("fo", "Zm8"), ("foo", "Zm9v"), ("foob", "Zm9vYg"), ("fooba", "Zm9vYmE"), ("foobar", "Zm9vYmFy")] {
            assert_eq!(b64url_encode(plain.as_bytes()), coded);
            assert_eq!(b64url_decode(coded).unwrap(), plain.as_bytes());
        }
        // Bytes whose sextets are 62 and 63 are `-` and `_`, never `+` and `/`, and never padded.
        assert_eq!(b64url_encode(&[0xfb, 0xff, 0xbe]), "-_--");
        assert_eq!(b64url_encode(&[0xff]), "_w");
        assert_eq!(b64url_decode("-_--").unwrap(), vec![0xfb, 0xff, 0xbe]);
    }

    #[test]
    fn base64url_round_trips_every_length_from_empty_to_a_credential_id() {
        for len in 0..=130usize {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            let coded = b64url_encode(&bytes);
            assert!(!coded.contains('=') && !coded.contains('+') && !coded.contains('/'), "len {len}: {coded}");
            assert_eq!(coded.len(), (len * 4 + 2) / 3, "len {len}");
            assert_eq!(b64url_decode(&coded).unwrap(), bytes, "len {len}");
        }
    }

    #[test]
    fn base64url_refuses_what_the_worker_never_sends() {
        for bad in ["Zg==", "Zm9v+", "Zm/v", "Zm 9v", "Z", "Zm9vY", "Zm9\n", "\u{e9}\u{e9}"] {
            assert!(b64url_decode(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn a_challenge_is_base64url_of_thirty_two_bytes_and_its_hex_form_is_what_gets_signed() {
        let bytes = challenge_bytes(CHALLENGE).unwrap();
        assert_eq!(bytes.len(), 32);
        assert_eq!((bytes[0], bytes[1], bytes[31]), (0x72, 0xfc, 0x95));
        assert_eq!(challenge_hex(CHALLENGE).unwrap(), CHALLENGE_HEX);
        assert_eq!(CHALLENGE.len(), 43);
        // The hex spelling is 64 characters and decodes to 48 bytes: not a challenge.
        assert!(challenge_bytes(CHALLENGE_HEX).is_err(), "hex is refused");
        assert!(challenge_bytes(&CHALLENGE[..42]).is_err(), "31 bytes");
        assert!(challenge_bytes(&format!("{CHALLENGE}A")).is_err(), "33 bytes");
        assert!(challenge_bytes(&format!("{CHALLENGE}=")).is_err(), "padding");
        assert!(challenge_bytes("+vysYD6JJR6k/cTpDEWWitdMW5IgH3esHrNnGKUHlZU").is_err(), "not the url alphabet");
        assert!(challenge_bytes("").is_err());
        assert!(challenge_hex("").is_err());
    }

    #[test]
    fn the_creation_options_are_the_begin_answer_with_bytes_decoded() {
        let o = CreationOptions::from_begin(&add_begin()).unwrap();
        assert_eq!(o.challenge, challenge_bytes(CHALLENGE).unwrap());
        assert_eq!((o.rp_id.as_str(), o.rp_name.as_str()), ("eustress.dev", "Eustress"));
        assert_eq!(o.user_id, b"11111111-2222-4333-8444-555555555555".to_vec(), "the account id's bytes");
        assert_eq!((o.user_name.as_str(), o.user_display_name.as_str()), ("ada", "Ada"));
        assert_eq!(o.algorithms, vec![-8, -7, -257], "in the Worker's order");
        assert_eq!(o.exclude, vec![vec![1, 2, 3], vec![4, 5, 6]]);
        assert_eq!(o.timeout_ms, 300_000);
        assert_eq!((o.resident_key.as_str(), o.user_verification.as_str(), o.attestation.as_str()), ("required", "required", "none"));
    }

    #[test]
    fn creation_options_refuse_a_begin_answer_the_browser_cannot_use() {
        let mut no_algs = add_begin();
        no_algs.pub_key_cred_params.clear();
        assert!(CreationOptions::from_begin(&no_algs).is_err());
        let mut bad_challenge = add_begin();
        bad_challenge.challenge = CHALLENGE_HEX.to_string();
        assert!(CreationOptions::from_begin(&bad_challenge).is_err());
        let mut bad_user = add_begin();
        bad_user.user.id = "not base64url!".to_string();
        assert!(CreationOptions::from_begin(&bad_user).is_err());
        let mut bad_exclude = add_begin();
        bad_exclude.exclude_credentials = vec!["a+b/".to_string()];
        assert!(CreationOptions::from_begin(&bad_exclude).is_err());
        let mut no_display = add_begin();
        no_display.user.display_name = String::new();
        assert_eq!(CreationOptions::from_begin(&no_display).unwrap().user_display_name, "ada", "the name stands in for an empty display name");
    }

    #[test]
    fn the_request_options_have_no_allow_list_and_carry_the_relying_party() {
        let begin: PasskeyLoginBegin = serde_json::from_value(json!({
            "challenge": CHALLENGE, "rp_id": "eustress.dev", "timeout_ms": 300000, "user_verification": "required"
        }))
        .unwrap();
        let o = RequestOptions::from_begin(&begin).unwrap();
        assert_eq!(o.challenge.len(), 32);
        assert_eq!((o.rp_id.as_str(), o.timeout_ms, o.user_verification.as_str()), ("eustress.dev", 300_000, "required"));
    }

    #[test]
    fn a_made_passkey_is_shaped_as_the_add_route_wants() {
        let made = CreatedParts { raw_id: vec![1, 2, 3, 4], client_data_json: b"{\"type\":\"webauthn.create\"}".to_vec(), attestation_object: vec![0xa3, 0x01], transports: vec!["internal".into(), "hybrid".into()] };
        assert_eq!(made.id(), "AQIDBA");
        let credential = made.to_json();
        assert_eq!(credential["id"], "AQIDBA");
        assert_eq!(credential["raw_id"], "AQIDBA");
        assert_eq!(credential["type"], "public-key");
        assert_eq!(credential["response"]["client_data_json"], b64url_encode(b"{\"type\":\"webauthn.create\"}"));
        assert_eq!(credential["response"]["attestation_object"], "owE");
        assert_eq!(credential["response"]["transports"], json!(["internal", "hybrid"]));

        let without = CreatedParts { transports: vec![], ..made.clone() };
        assert!(without.to_json()["response"].get("transports").is_none(), "no transports, no key");

        let body = add_body(CHALLENGE, "Laptop", &made, "ab12");
        assert_eq!(body["challenge"], CHALLENGE);
        assert_eq!(body["name"], "Laptop");
        assert_eq!(body["signature"], "ab12");
        assert_eq!(body["credential"], credential);
        let mut keys: Vec<_> = body.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["challenge", "credential", "name", "signature"]);
    }

    #[test]
    fn a_sign_in_is_shaped_as_the_login_route_wants() {
        let got = AssertedParts { raw_id: vec![9, 8, 7], client_data_json: vec![1], authenticator_data: vec![2, 2], signature: vec![3, 3, 3], user_handle: Some(b"abc".to_vec()) };
        let body = login_body(CHALLENGE, &got);
        assert_eq!(body["challenge"], CHALLENGE);
        let c = &body["credential"];
        assert_eq!((c["id"].as_str(), c["raw_id"].as_str(), c["type"].as_str()), (Some("CQgH"), Some("CQgH"), Some("public-key")));
        assert_eq!(c["response"]["client_data_json"], "AQ");
        assert_eq!(c["response"]["authenticator_data"], "AgI");
        assert_eq!(c["response"]["signature"], "AwMD");
        assert_eq!(c["response"]["user_handle"], "YWJj");
        let none = AssertedParts { user_handle: None, ..got };
        assert!(none.to_json()["response"]["user_handle"].is_null(), "no handle is null, not an empty string");
    }

    #[test]
    fn a_name_is_trimmed_cut_at_sixty_four_bytes_between_characters_and_never_empty() {
        assert_eq!(clean_name("  Work laptop  "), "Work laptop");
        assert_eq!(clean_name(""), "Passkey");
        assert_eq!(clean_name("   \n\t "), "Passkey");
        assert_eq!(clean_name("a\nb\tc"), "a b c");
        assert_eq!(clean_name(&"x".repeat(100)).len(), 64);
        // 33 two-byte characters are 66 bytes: 32 fit.
        let cut = clean_name(&"\u{e9}".repeat(33));
        assert_eq!(cut.chars().count(), 32);
        assert_eq!(cut.len(), 64);
    }

    #[test]
    fn each_browser_refusal_has_its_own_plain_sentence() {
        assert!(browser_failure("NotAllowedError", "x", true).starts_with("The passkey was not made"));
        assert!(browser_failure("NotAllowedError", "x", false).contains("You are not signed in"));
        assert!(browser_failure("InvalidStateError", "", true).contains("already has a passkey"));
        assert!(browser_failure("SecurityError", "", true).contains("identity file"));
        assert_eq!(browser_failure("Odd", "the disk is on fire", true), "The browser could not make the passkey: the disk is on fire");
        assert_eq!(browser_failure("Odd", "", false), "The browser could not use the passkey: Odd");
    }

    #[test]
    fn no_text_in_this_file_uses_a_long_dash() {
        let source = include_str!("webauthn.rs");
        for dash in ['\u{2014}', '\u{2013}'] {
            assert!(!source.contains(dash), "a long dash in webauthn.rs");
        }
    }
}
