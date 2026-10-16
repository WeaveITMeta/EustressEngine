// =============================================================================
// WebAuthn: the parts a passkey needs, and nothing else
// =============================================================================
//
// A strict CBOR decoder (only what WebAuthn carries), the authenticator data,
// the client data, COSE public keys for EdDSA (-8), ES256 (-7) and RS256 (-257)
// imported through WebCrypto, and the DER-to-raw conversion an ES256 signature
// needs. No attestation statement is verified: the site asks for attestation
// "none". The routes are in src/passkeys.mjs; the passkeys themselves are kept
// in the account's object (src/account_auth.mjs).
//
// Every function here either returns what it was asked for or throws a
// `WebauthnError` with a `code`; nothing is lenient about malformed input.
// =============================================================================

export const ALG_EDDSA = -8;
export const ALG_ES256 = -7;
export const ALG_RS256 = -257;
export const PASSKEY_ALGS = [ALG_EDDSA, ALG_ES256, ALG_RS256];

/// Credential ids, as WebAuthn bounds them.
export const CREDENTIAL_ID_MIN = 16;
export const CREDENTIAL_ID_MAX = 1023;

/// The most each encoded value may hold, in bytes.
export const LIMITS = {
  clientDataJSON: 8192,
  attestationObject: 32768,
  authenticatorData: 4096,
  signature: 1024,
  userHandle: 64,
};

const CBOR_MAX_DEPTH = 8;
const CBOR_MAX_ITEMS = 1024;

export class WebauthnError extends Error {
  constructor(code, message) {
    super(message || code);
    this.code = code;
  }
}

/// The same 12 small-order Ed25519 encodings as src/account_auth.mjs: a key that
/// is one of them verifies signatures for almost any message.
const SMALL_ORDER_ED25519 = new Set([
  '0100000000000000000000000000000000000000000000000000000000000000',
  'ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f',
  '0000000000000000000000000000000000000000000000000000000000000000',
  '0000000000000000000000000000000000000000000000000000000000000080',
  'c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a',
  'c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa',
  '26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05',
  '26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc85',
  'edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f',
  'edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff',
  'eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f',
  'eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff',
]);

// -----------------------------------------------------------------------------
// base64url (no padding), canonical only
// -----------------------------------------------------------------------------

const B64URL = /^[A-Za-z0-9_-]*$/;

export function toBase64url(bytes) {
  const view = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  let binary = '';
  for (let i = 0; i < view.length; i += 0x8000) binary += String.fromCharCode.apply(null, view.subarray(i, i + 0x8000));
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

/// The bytes `text` spells, or null when it is not canonical base64url without
/// padding, or decodes to more than `maxBytes`. Canonical: re-encoding the bytes
/// gives `text` back, so one value has one spelling and string comparison works.
export function fromBase64url(text, maxBytes = Infinity) {
  if (typeof text !== 'string' || !B64URL.test(text) || text.length % 4 === 1) return null;
  if (text.length > Math.ceil((maxBytes * 4) / 3)) return null;
  let binary;
  try {
    binary = atob(text.replace(/-/g, '+').replace(/_/g, '/') + '='.repeat((4 - (text.length % 4)) % 4));
  } catch {
    return null;
  }
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  if (bytes.length > maxBytes || toBase64url(bytes) !== text) return null;
  return bytes;
}

export const hexOfBytes = (bytes) => [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('');

export function bytesOfHex(hex) {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.substr(i * 2, 2), 16);
  return out;
}

function equalBytes(a, b) {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a[i] ^ b[i];
  return diff === 0;
}

function concatBytes(...parts) {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) { out.set(p, at); at += p.length; }
  return out;
}

export async function sha256Bytes(bytes) {
  return new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
}

// -----------------------------------------------------------------------------
// CBOR: definite lengths; unsigned and negative integers, byte and text strings,
// arrays, maps, and the simple values false, true and null (an authenticator's
// extension outputs carry booleans, for example hmac-secret). No tags, no floats,
// no indefinite lengths, no other simple values. Map keys are integers or text,
// and a key appears once.
// -----------------------------------------------------------------------------

const utf8 = new TextDecoder('utf-8', { fatal: true });

function cborFail(message) {
  throw new WebauthnError('cbor_invalid', message);
}

/// One CBOR item starting at `offset`: `{ value, end }`. Bytes become
/// Uint8Array, text a string, an array an Array, a map a Map.
export function decodeCborItem(bytes, offset = 0, depth = 0) {
  if (!(bytes instanceof Uint8Array)) cborFail('not bytes');
  if (depth > CBOR_MAX_DEPTH) cborFail('nested too deeply');
  let pos = offset;
  if (pos >= bytes.length) cborFail('truncated');
  const initial = bytes[pos++];
  const major = initial >> 5;
  const info = initial & 31;
  if (info === 31) cborFail('indefinite length');
  if (info >= 28) cborFail('reserved additional information');
  if (major === 6) cborFail('tags are not accepted');
  if (major === 7) {
    if (info === 20) return { value: false, end: pos };
    if (info === 21) return { value: true, end: pos };
    if (info === 22) return { value: null, end: pos };
    if (info >= 25 && info <= 27) cborFail('floating point is not accepted');
    cborFail('simple value is not accepted');
  }
  let arg;
  if (info < 24) arg = info;
  else {
    const size = 1 << (info - 24); // 1, 2, 4 or 8
    if (pos + size > bytes.length) cborFail('truncated');
    if (size === 8) {
      const hi = ((bytes[pos] << 24) >>> 0) + (bytes[pos + 1] << 16) + (bytes[pos + 2] << 8) + bytes[pos + 3];
      const lo = ((bytes[pos + 4] << 24) >>> 0) + (bytes[pos + 5] << 16) + (bytes[pos + 6] << 8) + bytes[pos + 7];
      if (hi > 0x1fffff) cborFail('integer too large');
      arg = hi * 0x100000000 + lo;
    } else {
      arg = 0;
      for (let i = 0; i < size; i++) arg = arg * 256 + bytes[pos + i];
    }
    pos += size;
  }
  const remaining = bytes.length - pos;
  switch (major) {
    case 0:
      return { value: arg, end: pos };
    case 1:
      if (arg >= Number.MAX_SAFE_INTEGER) cborFail('integer too large');
      return { value: -1 - arg, end: pos };
    case 2:
      if (arg > remaining) cborFail('truncated');
      return { value: bytes.slice(pos, pos + arg), end: pos + arg };
    case 3: {
      if (arg > remaining) cborFail('truncated');
      let text;
      try { text = utf8.decode(bytes.subarray(pos, pos + arg)); } catch { cborFail('text is not UTF-8'); }
      return { value: text, end: pos + arg };
    }
    case 4: {
      if (arg > CBOR_MAX_ITEMS || arg > remaining) cborFail('array too long');
      const out = [];
      for (let i = 0; i < arg; i++) {
        const item = decodeCborItem(bytes, pos, depth + 1);
        out.push(item.value);
        pos = item.end;
      }
      return { value: out, end: pos };
    }
    case 5: {
      if (arg > CBOR_MAX_ITEMS || arg * 2 > remaining) cborFail('map too long');
      const out = new Map();
      for (let i = 0; i < arg; i++) {
        const key = decodeCborItem(bytes, pos, depth + 1);
        if (typeof key.value !== 'number' && typeof key.value !== 'string') cborFail('map key is not an integer or text');
        if (out.has(key.value)) cborFail('duplicate map key');
        const value = decodeCborItem(bytes, key.end, depth + 1);
        out.set(key.value, value.value);
        pos = value.end;
      }
      return { value: out, end: pos };
    }
    default:
      return cborFail('unknown major type');
  }
}

/// Exactly one CBOR item filling all of `bytes`.
export function decodeCbor(bytes) {
  const { value, end } = decodeCborItem(bytes, 0, 0);
  if (end !== bytes.length) cborFail('trailing bytes');
  return value;
}

// -----------------------------------------------------------------------------
// Authenticator data
// -----------------------------------------------------------------------------

const FLAG_UP = 0x01;
const FLAG_UV = 0x04;
const FLAG_BE = 0x08;
const FLAG_BS = 0x10;
const FLAG_AT = 0x40;
const FLAG_ED = 0x80;

/// `attested`: true for a registration (attested credential data required),
/// false for an assertion (it must not be there). Every byte is accounted for.
export function parseAuthenticatorData(bytes, { attested }) {
  const bad = (m) => { throw new WebauthnError('authenticator_data_invalid', m); };
  if (!(bytes instanceof Uint8Array) || bytes.length < 37) bad('too short');
  const flagsByte = bytes[32];
  const flags = {
    up: !!(flagsByte & FLAG_UP), uv: !!(flagsByte & FLAG_UV), be: !!(flagsByte & FLAG_BE),
    bs: !!(flagsByte & FLAG_BS), at: !!(flagsByte & FLAG_AT), ed: !!(flagsByte & FLAG_ED),
  };
  if (flags.bs && !flags.be) bad('backed up but not backup eligible');
  const signCount = ((bytes[33] << 24) >>> 0) + (bytes[34] << 16) + (bytes[35] << 8) + bytes[36];
  let pos = 37;
  let credential = null;
  if (attested !== flags.at) bad(attested ? 'no attested credential data' : 'unexpected attested credential data');
  if (flags.at) {
    if (pos + 18 > bytes.length) bad('truncated attested credential data');
    const aaguid = bytes.slice(pos, pos + 16);
    const idLength = (bytes[pos + 16] << 8) + bytes[pos + 17];
    pos += 18;
    if (pos + idLength > bytes.length) bad('truncated credential id');
    const id = bytes.slice(pos, pos + idLength);
    pos += idLength;
    let key;
    try { key = decodeCborItem(bytes, pos, 0); } catch (e) { throw new WebauthnError('authenticator_data_invalid', `credential public key: ${e.message}`); }
    if (!(key.value instanceof Map)) bad('credential public key is not a map');
    credential = { aaguid, id, publicKey: key.value };
    pos = key.end;
  }
  let extensions = null;
  if (flags.ed) {
    let ext;
    try { ext = decodeCborItem(bytes, pos, 0); } catch (e) { throw new WebauthnError('authenticator_data_invalid', `extensions: ${e.message}`); }
    if (!(ext.value instanceof Map)) bad('extensions are not a map');
    extensions = ext.value;
    pos = ext.end;
  }
  if (pos !== bytes.length) bad('trailing bytes');
  return { rpIdHash: bytes.slice(0, 32), flags, signCount, credential, extensions };
}

// -----------------------------------------------------------------------------
// COSE public keys
// -----------------------------------------------------------------------------

const stripZeros = (b) => { let i = 0; while (i < b.length - 1 && b[i] === 0) i++; return b.slice(i); };
const bitLength = (b) => (b.length === 0 || (b.length === 1 && b[0] === 0) ? 0 : (b.length - 1) * 8 + (32 - Math.clz32(b[0])));

/// The COSE key from a registration as `{ alg, jwk }`, after WebCrypto has
/// imported it. Only the three algorithms the site offers are taken.
export async function coseToJwk(cose) {
  const bad = (m) => { throw new WebauthnError('key_unsupported', m); };
  if (!(cose instanceof Map)) bad('not a COSE key');
  const kty = cose.get(1);
  const alg = cose.get(3);
  if (!PASSKEY_ALGS.includes(alg)) bad('algorithm not offered');
  let jwk;
  if (alg === ALG_EDDSA) {
    const x = cose.get(-2);
    if (kty !== 1 || cose.get(-1) !== 6 || !(x instanceof Uint8Array) || x.length !== 32) bad('not an Ed25519 key');
    if (SMALL_ORDER_ED25519.has(hexOfBytes(x))) bad('Ed25519 key of small order');
    jwk = { kty: 'OKP', crv: 'Ed25519', x: toBase64url(x) };
  } else if (alg === ALG_ES256) {
    const x = cose.get(-2);
    const y = cose.get(-3);
    if (kty !== 2 || cose.get(-1) !== 1 || !(x instanceof Uint8Array) || !(y instanceof Uint8Array) || x.length !== 32 || y.length !== 32) bad('not a P-256 key');
    jwk = { kty: 'EC', crv: 'P-256', x: toBase64url(x), y: toBase64url(y) };
  } else {
    const n = cose.get(-1);
    const e = cose.get(-2);
    if (kty !== 3 || !(n instanceof Uint8Array) || !(e instanceof Uint8Array)) bad('not an RSA key');
    const modulus = stripZeros(n);
    const exponent = stripZeros(e);
    const bits = bitLength(modulus);
    // At least 2048 bits; at most 4096, which bounds what one verify costs.
    if (bits < 2048 || bits > 4096) bad('RSA modulus must be 2048 to 4096 bits');
    if (exponent.length > 4 || bitLength(exponent) < 2 || (exponent[exponent.length - 1] & 1) === 0) bad('RSA exponent not acceptable');
    jwk = { kty: 'RSA', n: toBase64url(modulus), e: toBase64url(exponent), alg: 'RS256' };
  }
  try {
    await importPasskeyKey(alg, jwk);
  } catch {
    bad('the key does not import');
  }
  return { alg, jwk };
}

/// A stored passkey key, ready to verify with.
export async function importPasskeyKey(alg, jwk) {
  if (alg === ALG_EDDSA) {
    const x = fromBase64url(jwk?.x, 32);
    if (!x || x.length !== 32) throw new WebauthnError('key_unsupported', 'bad Ed25519 key');
    return crypto.subtle.importKey('raw', x, { name: 'Ed25519' }, false, ['verify']);
  }
  if (alg === ALG_ES256) {
    return crypto.subtle.importKey('jwk', { kty: 'EC', crv: 'P-256', x: jwk.x, y: jwk.y, ext: true }, { name: 'ECDSA', namedCurve: 'P-256' }, false, ['verify']);
  }
  if (alg === ALG_RS256) {
    return crypto.subtle.importKey('jwk', { kty: 'RSA', n: jwk.n, e: jwk.e, alg: 'RS256', ext: true }, { name: 'RSASSA-PKCS1-v1_5', hash: 'SHA-256' }, false, ['verify']);
  }
  throw new WebauthnError('key_unsupported', 'algorithm not offered');
}

/// An ES256 signature as WebAuthn sends it (DER: SEQUENCE { INTEGER r, INTEGER s })
/// as the 64 raw bytes r || s WebCrypto verifies. Strict DER: short lengths,
/// minimal positive integers, nothing after.
export function derToRawEcdsa(der, size = 32) {
  const bad = (m) => { throw new WebauthnError('signature_invalid', `ES256 signature: ${m}`); };
  if (!(der instanceof Uint8Array) || der.length < 8 || der.length > 2 * size + 8) bad('length');
  if (der[0] !== 0x30) bad('not a sequence');
  if (der[1] & 0x80 || der[1] !== der.length - 2) bad('sequence length');
  let pos = 2;
  const integer = () => {
    if (pos + 2 > der.length || der[pos] !== 0x02) bad('not an integer');
    const length = der[pos + 1];
    pos += 2;
    if (length < 1 || length > size + 1 || pos + length > der.length) bad('integer length');
    let value = der.slice(pos, pos + length);
    pos += length;
    if (value[0] & 0x80) bad('negative integer');
    if (value.length > 1 && value[0] === 0 && !(value[1] & 0x80)) bad('integer not minimal');
    if (value[0] === 0 && value.length > 1) value = value.slice(1);
    if (value.length > size) bad('integer too large');
    const out = new Uint8Array(size);
    out.set(value, size - value.length);
    return out;
  };
  const r = integer();
  const s = integer();
  if (pos !== der.length) bad('trailing bytes');
  return concatBytes(r, s);
}

/// Whether `signature` by the passkey (`alg`, `jwk`) covers `data`. False for a
/// malformed signature as for a wrong one; never throws.
export async function verifyPasskeySignature(alg, jwk, data, signature) {
  try {
    if (!(signature instanceof Uint8Array) || signature.length === 0 || signature.length > LIMITS.signature) return false;
    const key = await importPasskeyKey(alg, jwk);
    if (alg === ALG_EDDSA) return signature.length === 64 && (await crypto.subtle.verify('Ed25519', key, signature, data));
    if (alg === ALG_ES256) return await crypto.subtle.verify({ name: 'ECDSA', hash: 'SHA-256' }, key, derToRawEcdsa(signature), data);
    if (alg === ALG_RS256) {
      const n = fromBase64url(jwk.n, 1024);
      if (!n || signature.length !== n.length) return false;
      return await crypto.subtle.verify('RSASSA-PKCS1-v1_5', key, signature, data);
    }
    return false;
  } catch {
    return false;
  }
}

// -----------------------------------------------------------------------------
// The checks a registration and an assertion share
// -----------------------------------------------------------------------------

/// What a refusal says: a status, a code, words for a person.
const REFUSALS = {
  bad_request: [400, 'That passkey request is malformed.'],
  client_data_invalid: [400, 'The browser\'s answer could not be read.'],
  client_data_type: [400, 'The browser\'s answer is for a different kind of request.'],
  challenge_mismatch: [400, 'The browser\'s answer is for a different challenge.'],
  passkey_origin_not_allowed: [403, 'This passkey was used on a site that does not sign in to Eustress.'],
  cross_origin: [403, 'A passkey cannot be used from inside another site.'],
  cbor_invalid: [400, 'The passkey\'s data could not be read.'],
  attestation_invalid: [400, 'The passkey\'s data could not be read.'],
  authenticator_data_invalid: [400, 'The passkey\'s data could not be read.'],
  rp_id_mismatch: [403, 'This passkey belongs to a different site.'],
  user_presence_required: [403, 'The passkey did not confirm that a person was there.'],
  user_verification_required: [403, 'The passkey did not verify you (a PIN, a fingerprint or your face).'],
  credential_id_mismatch: [400, 'The passkey\'s id does not match its data.'],
  key_unsupported: [400, 'This kind of passkey is not supported. Try another device or browser.'],
};

export function refusal(code, detail) {
  const [status, error] = REFUSALS[code] || [400, 'That passkey was refused.'];
  return { ok: false, status, code, error, ...(detail ? { detail: String(detail).slice(0, 120) } : {}) };
}

const asRefusal = (e) => refusal(e instanceof WebauthnError ? e.code : 'bad_request', e?.message);

/// The relying party this deployment is: `PASSKEY_RP_ID` and the site origins
/// `PASSKEY_ORIGINS` names (comma separated). Null when either is missing, or
/// no listed origin is the RP id or a subdomain of it on https (or on
/// http://localhost, for a local site).
export function passkeyConfig(env) {
  const rpId = String(env?.PASSKEY_RP_ID || '').trim().toLowerCase();
  if (!/^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)*$/.test(rpId) || rpId.length > 253) return null;
  const origins = new Set();
  for (const raw of String(env?.PASSKEY_ORIGINS || '').split(',')) {
    let url;
    try { url = new URL(raw.trim()); } catch { continue; }
    const host = url.hostname.toLowerCase();
    const secure = url.protocol === 'https:' || (url.protocol === 'http:' && host === 'localhost');
    if (secure && (host === rpId || host.endsWith(`.${rpId}`))) origins.add(url.origin);
  }
  if (!origins.size) return null;
  return { rpId, origins };
}

function checkClientData(bytes, { type, challenge, origins }) {
  let data;
  try { data = JSON.parse(utf8.decode(bytes)); } catch { throw new WebauthnError('client_data_invalid', 'not JSON'); }
  if (!data || typeof data !== 'object' || Array.isArray(data)) throw new WebauthnError('client_data_invalid', 'not an object');
  if (data.type !== type) throw new WebauthnError('client_data_type', String(data.type));
  if (typeof data.challenge !== 'string' || data.challenge !== challenge) throw new WebauthnError('challenge_mismatch');
  if (typeof data.origin !== 'string' || !origins.has(data.origin)) throw new WebauthnError('passkey_origin_not_allowed', String(data.origin));
  // The site does not run WebAuthn inside another site's frame.
  if (data.crossOrigin === true) throw new WebauthnError('cross_origin');
  return data;
}

async function checkRpAndFlags(auth, rpId) {
  if (!equalBytes(auth.rpIdHash, await sha256Bytes(new TextEncoder().encode(rpId)))) throw new WebauthnError('rp_id_mismatch');
  if (!auth.flags.up) throw new WebauthnError('user_presence_required');
  if (!auth.flags.uv) throw new WebauthnError('user_verification_required');
}

/// The credential's id, checked: `id` and `raw_id` are the same canonical
/// base64url, of 16 to 1023 bytes. Throws `bad_request`.
export function credentialIdOf(credential) {
  if (!credential || typeof credential !== 'object' || Array.isArray(credential)) throw new WebauthnError('bad_request', 'no credential');
  if (credential.type !== 'public-key') throw new WebauthnError('bad_request', 'type is not public-key');
  if (typeof credential.id !== 'string' || credential.id !== credential.raw_id) throw new WebauthnError('bad_request', 'id and raw_id differ');
  const raw = fromBase64url(credential.raw_id, CREDENTIAL_ID_MAX);
  if (!raw || raw.length < CREDENTIAL_ID_MIN) throw new WebauthnError('bad_request', 'credential id is not 16 to 1023 bytes of base64url');
  return { id: credential.raw_id, raw };
}

const TRANSPORTS = new Set(['usb', 'nfc', 'ble', 'internal', 'hybrid', 'smart-card']);

/// A new passkey from `navigator.credentials.create`, checked in the contract's
/// order. `challenge` is the base64url of the challenge bytes. Returns
/// `{ ok: true, record }` or a refusal; never throws.
export async function verifyRegistration({ credential, challenge, rpId, origins }) {
  try {
    const { id, raw } = credentialIdOf(credential);
    const response = credential.response;
    if (!response || typeof response !== 'object') throw new WebauthnError('bad_request', 'no response');
    const clientData = fromBase64url(response.client_data_json, LIMITS.clientDataJSON);
    const attestation = fromBase64url(response.attestation_object, LIMITS.attestationObject);
    if (!clientData || !attestation) throw new WebauthnError('bad_request', 'client_data_json or attestation_object is not base64url within its size');

    checkClientData(clientData, { type: 'webauthn.create', challenge, origins });

    const object = decodeCbor(attestation);
    if (!(object instanceof Map)) throw new WebauthnError('attestation_invalid', 'not a map');
    const fmt = object.get('fmt');
    const authData = object.get('authData');
    if (typeof fmt !== 'string' || !(object.get('attStmt') instanceof Map) || !(authData instanceof Uint8Array)) {
      throw new WebauthnError('attestation_invalid', 'fmt, attStmt and authData are required');
    }
    if (authData.length > LIMITS.authenticatorData) throw new WebauthnError('authenticator_data_invalid', 'too long');
    const auth = parseAuthenticatorData(authData, { attested: true });
    await checkRpAndFlags(auth, rpId);
    if (!equalBytes(auth.credential.id, raw)) throw new WebauthnError('credential_id_mismatch');

    const { alg, jwk } = await coseToJwk(auth.credential.publicKey);
    const transports = Array.isArray(response.transports)
      ? [...new Set(response.transports.filter((t) => typeof t === 'string' && TRANSPORTS.has(t)))].slice(0, 6)
      : [];
    return {
      ok: true,
      record: {
        id, alg, public_key: jwk, sign_count: auth.signCount, transports,
        backup_eligible: auth.flags.be, backed_up: auth.flags.bs,
      },
    };
  } catch (e) {
    return asRefusal(e);
  }
}

/// A sign-in from `navigator.credentials.get`, checked as far as it can be
/// without the stored key: the client data, the RP, the flags. The signature and
/// the sign count are the account object's to check. Returns `{ ok: true, ... }`
/// or a refusal; never throws.
export async function checkAssertion({ credential, challenge, rpId, origins }) {
  try {
    const { id } = credentialIdOf(credential);
    const response = credential.response;
    if (!response || typeof response !== 'object') throw new WebauthnError('bad_request', 'no response');
    const clientData = fromBase64url(response.client_data_json, LIMITS.clientDataJSON);
    const authData = fromBase64url(response.authenticator_data, LIMITS.authenticatorData);
    const signature = fromBase64url(response.signature, LIMITS.signature);
    const userHandle = fromBase64url(response.user_handle, LIMITS.userHandle);
    if (!clientData || !authData || !signature || !signature.length) throw new WebauthnError('bad_request', 'client_data_json, authenticator_data and signature are required base64url');
    if (!userHandle || !userHandle.length) throw new WebauthnError('bad_request', 'user_handle is required');

    checkClientData(clientData, { type: 'webauthn.get', challenge, origins });
    const auth = parseAuthenticatorData(authData, { attested: false });
    await checkRpAndFlags(auth, rpId);

    let user;
    try { user = utf8.decode(userHandle); } catch { throw new WebauthnError('bad_request', 'user_handle is not UTF-8'); }
    return {
      ok: true, id, user,
      authenticator_data: toBase64url(authData),
      client_data_hash: toBase64url(await sha256Bytes(clientData)),
      signature: toBase64url(signature),
    };
  } catch (e) {
    return asRefusal(e);
  }
}

/// What a passkey signs: the authenticator data, then the SHA-256 of the client data.
export function signedData(authenticatorData, clientDataHash) {
  return concatBytes(authenticatorData, clientDataHash);
}
