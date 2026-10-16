// =============================================================================
// The time zone an account's emails are written in
// =============================================================================
//
// An email cannot know where its reader is, and every time in it should be in
// the reader's zone. The site tells the Worker the browser's zone once per
// session (POST /api/account/time-zone), the Worker keeps it under
// `timezone:{accountId}`, and every account email reads it back
// (src/email_shell.mjs formatWhen). With none saved, an email names UTC.
//
// A display preference, nothing more: any signed-in session may set it, it is
// validated as an IANA zone name that this runtime's Intl knows, and a write
// that changes nothing is not made.

const KEY = (accountId) => `timezone:${accountId}`;

/// Letters, digits, `_`, `+`, `-`, and at most two `/` separators: the shape of an IANA name ("America/Phoenix",
/// "America/Argentina/Buenos_Aires", "Etc/GMT+7", "UTC"). Nothing else reaches Intl or the KV.
const SHAPE = /^[A-Za-z][A-Za-z0-9_+-]*(?:\/[A-Za-z0-9_+-]+){0,2}$/;

/// The zone as given when it is a name Intl knows, else `null`. Never throws.
export function cleanTimeZone(value) {
  if (typeof value !== 'string' || value.length === 0 || value.length > 64 || !SHAPE.test(value)) return null;
  try {
    new Intl.DateTimeFormat('en-US', { timeZone: value }).format(0);
    return value;
  } catch {
    return null;
  }
}

/// The zone saved for the account, or `null` (none saved, a value that no longer validates, or storage unreadable).
export async function accountTimeZone(env, accountId) {
  if (!env?.USERS || typeof accountId !== 'string' || !accountId) return null;
  try {
    return cleanTimeZone(await env.USERS.get(KEY(accountId)));
  } catch {
    return null;
  }
}

const ROUTE = '/api/account/time-zone';

/// `POST /api/account/time-zone` `{ time_zone }`. A response for this route, or `null` for any other path.
/// `deps`: authenticate (the session check), json, cors.
export async function handleTimeZoneRoute(request, url, env, deps) {
  const { json, cors } = deps;
  if (url.pathname !== ROUTE || request.method !== 'POST') return null;
  const userId = await deps.authenticate(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);
  const body = await request.json().catch(() => ({}));
  const zone = cleanTimeZone(body?.time_zone);
  if (!zone) return json({ error: 'That is not a time zone this service knows', code: 'bad_time_zone' }, 400, cors);
  const saved = await accountTimeZone(env, userId);
  if (saved !== zone) await env.USERS.put(KEY(userId), zone);
  return json({ ok: true, time_zone: zone, changed: saved !== zone }, 200, cors);
}

export const timeZoneKey = KEY;
