// =============================================================================
// World play token: a short-lived pass from a manifest GET to the chunk GETs after it
// =============================================================================
//
// A Player downloading a world makes one manifest GET and then one GET per chunk. Every one of those ran the full serving
// gate (world.mjs servableListing): the listing record from KV, the legal lock, the deletion mark, the approval binding, the
// manifest's identity, and whatever credential the request carried (a server credential or a play pass costs KV reads of
// its own). The answer for a chunk is the same as the answer for the manifest a moment before, so the manifest GET now hands
// back a token, and a chunk GET that presents it skips the KV half of the gate.
//
// docs/architecture/WORLD_PLAY_TOKEN.md is the threat model. In short:
//
//   - A token is minted only where the manifest would have been served to ANYONE signed out: a listable (approved, public)
//     listing whose rating is within the signed-out cap and is not adult content, whose world is current, and whose approval
//     binding exists and is not revoked. Holding a token therefore grants nothing a signed-out stranger could not already
//     read, so a token that leaks, is shared or is replayed by another person exposes nothing more.
//   - It names one listing, one manifest (its SHA-256), the approval it was minted under (the binding's content identity) and
//     an expiry 15 minutes out. It is HMAC-SHA256 under a key derived from JWT_SECRET with its own label, never the secret.
//   - Redeeming it still reads, fresh from R2 (strongly consistent): the legal lock, the deletion mark and the approval
//     binding. A lock, a deletion, a revoked approval or a new approval (a republish that was reviewed) all make the token
//     worth nothing, and the request falls through to the full gate, which gives the right refusal.
//   - What a token does NOT see until it expires: changes that live only in the KV listing record (the author makes the
//     listing private or archives it, a rating change without a new approval). Those reach chunk GETs within 15 minutes and
//     reach every manifest GET at once, so no new download starts after them.
//
// A token that fails any check is never an error: the chunk GET simply runs the full gate.
// =============================================================================

/// The response header on a manifest GET that carries the token, and the request header a chunk GET presents it in.
export const PLAY_TOKEN_HEADER = 'X-Eustress-Play-Token';
/// The response header with the token's expiry, in Unix seconds.
export const PLAY_TOKEN_EXPIRES_HEADER = 'X-Eustress-Play-Token-Expires';
/// How long a token lives. A world of a few MB downloads in seconds; this covers a slow link and a retry.
export const PLAY_TOKEN_TTL_SECONDS = 900;
/// The token's own label, so its key is not the image URL key or the session key (all derive from JWT_SECRET).
const KEY_LABEL = 'world-play-token-v1';
const PREFIX = 'wpt1';
/// Anything longer is not a token this Worker minted.
const MAX_TOKEN_CHARS = 1024;

const encoder = new TextEncoder();
const decoder = new TextDecoder();
let derived = { secret: null, key: null };

/// The HMAC key, derived once per isolate: HMAC-SHA256(JWT_SECRET, label). Null when JWT_SECRET is not configured, and then
/// no token is minted or accepted.
async function tokenKey(env) {
  const secret = env?.JWT_SECRET;
  if (typeof secret !== 'string' || !secret) return null;
  if (derived.secret !== secret) {
    const outer = await crypto.subtle.importKey('raw', encoder.encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
    const raw = await crypto.subtle.sign('HMAC', outer, encoder.encode(KEY_LABEL));
    derived = { secret, key: await crypto.subtle.importKey('raw', raw, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign', 'verify']) };
  }
  return derived.key;
}

const toB64url = (bytes) => btoa(String.fromCharCode(...bytes)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
function fromB64url(text) {
  if (typeof text !== 'string' || !/^[A-Za-z0-9_-]*$/.test(text)) return null;
  const b64 = text.replace(/-/g, '+').replace(/_/g, '/');
  try {
    return Uint8Array.from(atob(b64 + '='.repeat((4 - (b64.length % 4)) % 4)), (c) => c.charCodeAt(0));
  } catch {
    return null;
  }
}

const nowSeconds = () => Math.floor(Date.now() / 1000);
const isSha256Hex = (s) => typeof s === 'string' && /^[0-9a-f]{64}$/.test(s);

/// Mint a token for one listing's committed manifest.
///
///     mintPlayToken(env, { simId, manifestSha, ident }, now?) -> Promise<{ token, expires } | null>
///       simId        the listing id the token is good for
///       manifestSha  the SHA-256 (hex) of the manifest that was just served
///       ident        the approval binding's content identity the manifest was served under (moderation.mjs readApproval)
///       now          Unix seconds, for tests
///       ->           null when JWT_SECRET is not configured or an argument is malformed
export async function mintPlayToken(env, { simId, manifestSha, ident }, now = nowSeconds()) {
  if (typeof simId !== 'string' || !simId || !isSha256Hex(manifestSha) || typeof ident !== 'string' || !ident) return null;
  const key = await tokenKey(env);
  if (!key) return null;
  const expires = now + PLAY_TOKEN_TTL_SECONDS;
  const body = toB64url(encoder.encode(JSON.stringify({ v: 1, sim: simId, sha: manifestSha, ident, exp: expires })));
  const mac = new Uint8Array(await crypto.subtle.sign('HMAC', key, encoder.encode(`${PREFIX}.${body}`)));
  return { token: `${PREFIX}.${body}.${toB64url(mac)}`, expires };
}

/// The claims of a token this Worker minted for `simId` that has not expired, or null for anything else (absent, malformed,
/// forged, for another listing, expired, or JWT_SECRET not configured). The MAC is checked with crypto.subtle.verify, which
/// compares in constant time.
///
///     readPlayToken(env, token, simId, now?) -> Promise<{ sim, sha, ident, exp } | null>
export async function readPlayToken(env, token, simId, now = nowSeconds()) {
  if (typeof token !== 'string' || token.length === 0 || token.length > MAX_TOKEN_CHARS) return null;
  const parts = token.split('.');
  if (parts.length !== 3 || parts[0] !== PREFIX) return null;
  const mac = fromB64url(parts[2]);
  const body = fromB64url(parts[1]);
  if (!mac || mac.length !== 32 || !body) return null;
  const key = await tokenKey(env);
  if (!key) return null;
  if (!(await crypto.subtle.verify('HMAC', key, mac, encoder.encode(`${PREFIX}.${parts[1]}`)))) return null;
  let claims;
  try {
    claims = JSON.parse(decoder.decode(body));
  } catch {
    return null;
  }
  if (!claims || claims.v !== 1 || claims.sim !== simId || !isSha256Hex(claims.sha) || typeof claims.ident !== 'string') return null;
  if (!Number.isSafeInteger(claims.exp) || claims.exp <= now) return null;
  // A token from the future (an expiry further out than one lifetime) was not minted by this code.
  if (claims.exp > now + PLAY_TOKEN_TTL_SECONDS) return null;
  return { sim: claims.sim, sha: claims.sha, ident: claims.ident, exp: claims.exp };
}
