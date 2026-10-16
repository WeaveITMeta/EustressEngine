// A software WebAuthn authenticator for tests: real key pairs from node's crypto,
// a real CBOR attestation object, real authenticator data, real client data and
// real signatures for ES256 (-7), EdDSA (-8) and RS256 (-257). What a browser
// would hand the site, in the shape the site posts it to the Worker
// (src/passkeys.mjs). Every field can be bent, so a test can make exactly one
// thing wrong.

import crypto from 'node:crypto';

export const b64url = (bytes) => Buffer.from(bytes).toString('base64url');
export const fromB64url = (text) => new Uint8Array(Buffer.from(text, 'base64url'));
const sha256 = (data) => crypto.createHash('sha256').update(data).digest();

// ── CBOR, the encoding side, enough for WebAuthn ─────────────────────────────

function head(major, n) {
  if (n < 24) return Buffer.from([(major << 5) | n]);
  if (n < 0x100) return Buffer.from([(major << 5) | 24, n]);
  if (n < 0x10000) { const b = Buffer.alloc(3); b[0] = (major << 5) | 25; b.writeUInt16BE(n, 1); return b; }
  if (n < 0x100000000) { const b = Buffer.alloc(5); b[0] = (major << 5) | 26; b.writeUInt32BE(n, 1); return b; }
  const b = Buffer.alloc(9); b[0] = (major << 5) | 27; b.writeBigUInt64BE(BigInt(n), 1); return b;
}

/// Numbers are integers; Uint8Array/Buffer are byte strings; a Map keeps its key order.
export function cbor(value) {
  if (value === false) return Buffer.from([0xf4]);
  if (value === true) return Buffer.from([0xf5]);
  if (value === null) return Buffer.from([0xf6]);
  if (typeof value === 'number') return value >= 0 ? head(0, value) : head(1, -1 - value);
  if (typeof value === 'string') { const t = Buffer.from(value, 'utf8'); return Buffer.concat([head(3, t.length), t]); }
  if (value instanceof Uint8Array) return Buffer.concat([head(2, value.length), Buffer.from(value)]);
  if (Array.isArray(value)) return Buffer.concat([head(4, value.length), ...value.map(cbor)]);
  if (value instanceof Map) return Buffer.concat([head(5, value.size), ...[...value].flatMap(([k, v]) => [cbor(k), cbor(v)])]);
  throw new Error(`cbor: cannot encode ${typeof value}`);
}

// ── Keys ─────────────────────────────────────────────────────────────────────

/// A key pair for `alg`, with its COSE public key and a signer that returns what
/// an authenticator returns (ES256 as DER).
export function passkeyKey(alg) {
  if (alg === -7) {
    const { publicKey, privateKey } = crypto.generateKeyPairSync('ec', { namedCurve: 'P-256' });
    const jwk = publicKey.export({ format: 'jwk' });
    return {
      alg,
      cose: new Map([[1, 2], [3, -7], [-1, 1], [-2, fromB64url(jwk.x)], [-3, fromB64url(jwk.y)]]),
      sign: (data) => crypto.sign('sha256', data, { key: privateKey, dsaEncoding: 'der' }),
    };
  }
  if (alg === -8) {
    const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
    const jwk = publicKey.export({ format: 'jwk' });
    return {
      alg,
      cose: new Map([[1, 1], [3, -8], [-1, 6], [-2, fromB64url(jwk.x)]]),
      sign: (data) => crypto.sign(null, data, privateKey),
    };
  }
  if (alg === -257) {
    const { publicKey, privateKey } = crypto.generateKeyPairSync('rsa', { modulusLength: 2048 });
    const jwk = publicKey.export({ format: 'jwk' });
    return {
      alg,
      cose: new Map([[1, 3], [3, -257], [-1, fromB64url(jwk.n)], [-2, fromB64url(jwk.e)]]),
      sign: (data) => crypto.sign('sha256', data, privateKey),
    };
  }
  throw new Error(`no such alg ${alg}`);
}

const FLAGS = { up: 0x01, uv: 0x04, be: 0x08, bs: 0x10, at: 0x40, ed: 0x80 };
const flagByte = (f) => Object.entries(FLAGS).reduce((b, [name, bit]) => (f[name] ? b | bit : b), 0);

/// Authenticator data: the RP id hash, the flags, the sign count, then (for a
/// registration) the attested credential data, then any extensions.
export function authenticatorData({ rpId, flags, signCount = 0, credentialId = null, cose = null, extensions = null }) {
  const count = Buffer.alloc(4);
  count.writeUInt32BE(signCount >>> 0);
  const parts = [sha256(rpId), Buffer.from([flagByte({ ...flags, at: !!credentialId, ed: !!extensions })]), count];
  if (credentialId) {
    const len = Buffer.alloc(2);
    len.writeUInt16BE(credentialId.length);
    parts.push(Buffer.alloc(16), len, Buffer.from(credentialId), cose instanceof Uint8Array ? Buffer.from(cose) : cbor(cose));
  }
  if (extensions) parts.push(extensions instanceof Uint8Array ? Buffer.from(extensions) : cbor(extensions));
  return new Uint8Array(Buffer.concat(parts));
}

export const clientDataJSON = ({ type, challenge, origin, extra = {} }) =>
  new Uint8Array(Buffer.from(JSON.stringify({ type, challenge, origin, crossOrigin: false, ...extra })));

/// One authenticator holding one credential.
export class SoftAuthenticator {
  /// `countsUp`: the sign count rises with each use (a security key), or stays 0
  /// (a synced platform passkey).
  constructor(alg = -7, { idLength = 32, countsUp = true } = {}) {
    this.key = passkeyKey(alg);
    this.alg = alg;
    this.credentialId = new Uint8Array(crypto.randomBytes(idLength));
    this.id = b64url(this.credentialId);
    this.signCount = 0;
    this.countsUp = countsUp;
    this.userHandle = null;
  }

  /// What `navigator.credentials.create` gives, for the options /api/passkeys/add/begin
  /// returned. `o` bends one thing: origin, rpId, type, flags, signCount,
  /// credentialId (in the authenticator data), id / raw_id (in the JSON),
  /// cose, attestationObject (raw bytes), clientData (raw bytes), challenge.
  register(options, o = {}) {
    this.userHandle = options.user.id;
    const rpId = o.rpId ?? options.rp.id;
    const flags = { up: true, uv: true, be: true, bs: false, ...(o.flags || {}) };
    const authData = o.authData ?? authenticatorData({
      rpId, flags, signCount: o.signCount ?? this.signCount,
      credentialId: o.credentialId ?? this.credentialId, cose: o.cose ?? this.key.cose, extensions: o.extensions ?? null,
    });
    const attestation = o.attestationObject ?? new Uint8Array(cbor(new Map([['fmt', 'none'], ['attStmt', new Map()], ['authData', authData]])));
    const clientData = o.clientData ?? clientDataJSON({ type: o.type ?? 'webauthn.create', challenge: o.challenge ?? options.challenge, origin: o.origin ?? 'https://eustress.dev', extra: o.clientExtra });
    return {
      id: o.id ?? this.id,
      raw_id: o.raw_id ?? o.id ?? this.id,
      type: 'public-key',
      response: { client_data_json: b64url(clientData), attestation_object: b64url(attestation), transports: ['internal', 'hybrid'] },
    };
  }

  /// What `navigator.credentials.get` gives for /api/passkeys/login/begin's options.
  /// The sign count goes up by one unless `o.signCount` says what to sign.
  assert(options, o = {}) {
    const rpId = o.rpId ?? options.rp_id;
    const flags = { up: true, uv: true, be: true, bs: true, ...(o.flags || {}) };
    if (o.signCount === undefined && this.countsUp) this.signCount += 1;
    const authData = o.authData ?? authenticatorData({ rpId, flags, signCount: o.signCount ?? this.signCount, extensions: o.extensions ?? null });
    const clientData = o.clientData ?? clientDataJSON({ type: o.type ?? 'webauthn.get', challenge: o.challenge ?? options.challenge, origin: o.origin ?? 'https://eustress.dev', extra: o.clientExtra });
    const signature = o.signature ?? this.key.sign(Buffer.concat([Buffer.from(authData), sha256(clientData)]));
    return {
      id: o.id ?? this.id,
      raw_id: o.raw_id ?? o.id ?? this.id,
      type: 'public-key',
      response: {
        client_data_json: b64url(clientData),
        authenticator_data: b64url(authData),
        signature: b64url(signature),
        user_handle: o.userHandle ?? this.userHandle,
      },
    };
  }
}
