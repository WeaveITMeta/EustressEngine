// =============================================================================
// The one reading of the Authorization header
// =============================================================================
//
// Every check that asks "which account is this request?" reads the header here, so every check sees the same credential.
// Security review 2026-11-15 (F1): the gate read the token with one pattern and the routes with another. A token split
// by a space or a tab was "no token" to the gate, which let it through, and a valid session to the route, because atob
// drops whitespace while decoding. One reader, strict on both sides, closes that.
//
// A credential is one run of token characters after "Bearer " (RFC 6750 b64token): no space, tab or line break inside.
// A session token is three base64url segments joined by ".". Anything else is not a credential.
// =============================================================================

const CREDENTIAL = /^Bearer ([A-Za-z0-9._~+/-]+=*)$/i;
/// One base64url segment of a session token: the alphabet createJwt writes, nothing else.
export const JWT_SEGMENT = /^[A-Za-z0-9_-]+$/;

const headerOf = (request) => {
  const h = request?.headers?.get?.('Authorization');
  return typeof h === 'string' ? h : '';
};

/// The credential the request presents, or null when it presents none or one that does not read cleanly.
export function bearerToken(request) {
  const m = CREDENTIAL.exec(headerOf(request));
  return m ? m[1] : null;
}

/// Whether the request presents a Bearer header at all, well formed or not. With bearerToken, it tells "no
/// credential" (anonymous) from "a credential that does not read" (refused).
export function presentsBearer(request) {
  return /^Bearer(\s|$)/i.test(headerOf(request));
}
