// =============================================================================
// CORS for the browser-facing API
// =============================================================================
//
// The API is bearer-token (Authorization header) auth, never cookies, so this
// list is the browser-facing surface only: non-browser clients (the engine, the
// updater) send no Origin and are unaffected. NEVER pair a reflected origin
// with Access-Control-Allow-Credentials: true.
//
// Production admits the four origins below. A deployment that serves another
// site, such as staging's Pages preview, names that site's exact origin in the
// ALLOWED_ORIGINS variable (comma separated, set in `[env.staging.vars]`).
// Production sets nothing, so its list cannot widen by accident.
// =============================================================================

export const BUILT_IN_ORIGINS = [
  'https://eustress.dev',
  'https://www.eustress.dev',
  'http://localhost:3000',
  'http://127.0.0.1:3000',
];

const PRODUCTION_SITE = 'https://eustress.dev';

/// An origin an operator may add: an exact `https://host[:port]`, or `http://`
/// for localhost only. No wildcard, no path, no query. Anything else is
/// dropped, so a typo fails closed.
function parseOrigin(entry) {
  const text = String(entry).trim().replace(/\/+$/, '');
  if (!text || text.includes('*')) return null;
  let url;
  try {
    url = new URL(text);
  } catch {
    return null;
  }
  if (url.origin !== text) return null;
  const local = url.hostname === 'localhost' || url.hostname === '127.0.0.1';
  if (url.protocol === 'https:' || (url.protocol === 'http:' && local)) return url.origin;
  return null;
}

/// The origins this deployment admits: the built-in set plus ALLOWED_ORIGINS.
export function allowedOrigins(env) {
  const extra = String(env?.ALLOWED_ORIGINS || '')
    .split(',')
    .map(parseOrigin)
    .filter(Boolean);
  return new Set([...BUILT_IN_ORIGINS, ...extra]);
}

/// The deployment's own site, which an unlisted origin is answered with so
/// the browser refuses the response. PUBLIC_SITE_ORIGIN names it (staging sets
/// it); production falls back to eustress.dev.
export function siteOrigin(env) {
  return parseOrigin(env?.PUBLIC_SITE_ORIGIN || '') || PRODUCTION_SITE;
}

export function corsHeaders(request, env) {
  const origin = request.headers.get('Origin');
  const allow = origin && allowedOrigins(env).has(origin) ? origin : siteOrigin(env);
  return {
    'Access-Control-Allow-Origin': allow,
    'Vary': 'Origin',
    // PUT is listed for the upload routes. DELETE is for the website's commerce
    // views and key list (archive a product, delete a webhook endpoint, revoke
    // a key). Eustress-Mode is how a signed-in page chooses live commerce data
    // (src/commerce.mjs); without it in the list every such request fails
    // preflight.
    'Access-Control-Allow-Methods': 'GET, POST, PUT, DELETE, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type, Authorization, X-ID-Type, If-None-Match, X-Eustress-Key, Eustress-Mode, X-Eustress-Lease',
    'Access-Control-Max-Age': '86400',
  };
}
