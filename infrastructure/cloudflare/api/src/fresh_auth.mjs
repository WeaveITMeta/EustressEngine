// =============================================================================
// The gate: fresh sessions, and the value freeze
// =============================================================================
//
// `freshGate` runs before a mutating request is dispatched. For a route the
// table (src/route_policy.mjs) classes `fresh` or `value`, a request that
// carries a session is checked against the account's object, not against the
// KV mirror of the floor: a sign-out, a ban or a key replacement ends the
// session at once, everywhere. A `value` route also refuses while the account's
// value is frozen after a key replacement.
//
// What the gate leaves alone, on purpose:
//   - a request with no session token: the route decides (an API key, or 401);
//   - a deployment without account objects: the cheap check stays the only one;
//   - an account the object does not know (deleted): the route answers 401.
// What it never does: let an unreachable object through. A fresh route that
// cannot ask is refused with a retryable 503.
//
// `valueFrozenUntil` is for code that moves value without a session (the
// scheduled payout run): it reads the KV mirror the object keeps.
// =============================================================================

import { accountState } from './account_auth.mjs';
import { routePolicy, isMutating, FRESH, VALUE } from './route_policy.mjs';

const when = (ms) => new Date(ms).toISOString().slice(0, 16).replace('T', ' ') + ' UTC';

/// A response that stops the request, or null to let it through.
/// `deps`: json, verifyJwt.
export async function freshGate(request, env, cors, deps) {
  if (!isMutating(request.method)) return null;
  const policy = routePolicy(request.method, new URL(request.url).pathname);
  if (policy.class !== FRESH && policy.class !== VALUE) return null;

  const auth = request.headers.get('Authorization');
  if (!auth || !auth.startsWith('Bearer ')) return null;
  if (!env.ACCOUNT_AUTH) return null;
  const claims = await deps.verifyJwt(auth.slice(7), env.JWT_SECRET);
  if (!claims || typeof claims.sub !== 'string' || !claims.sub) return null;

  const held = await accountState(env, claims.sub);
  if (!held.ok) {
    if (held.code === 'no_account') return null;
    return deps.json({ error: 'This is temporarily unavailable. Try again in a minute.', code: 'account_unavailable' }, 503, cors);
  }
  if (held.floor && !(claims.iat >= held.floor)) {
    return deps.json({ error: 'Your session has ended. Sign in again.', code: 'session_revoked' }, 401, cors);
  }
  if (policy.class === VALUE && held.value_frozen_until > Date.now()) {
    return deps.json({
      error: `Spending, transfers and payouts are paused until ${when(held.value_frozen_until)}, after the identity key was replaced.`,
      code: 'account_frozen',
      until: new Date(held.value_frozen_until).toISOString(),
    }, 423, cors);
  }
  return null;
}

/// The session's account, checked against the object: for a route that takes
/// the account from the session itself and wants the strict check whatever
/// class the table gives it. Null when there is no valid, current session.
export async function verifyFreshAuth(request, env, deps) {
  const auth = request.headers.get('Authorization');
  if (!auth || !auth.startsWith('Bearer ')) return null;
  const claims = await deps.verifyJwt(auth.slice(7), env.JWT_SECRET);
  if (!claims || typeof claims.sub !== 'string' || !claims.sub) return null;
  if (!env.ACCOUNT_AUTH) return deps.verifyAuth(request, env);
  const held = await accountState(env, claims.sub);
  if (!held.ok) return null;
  if (held.floor && !(claims.iat >= held.floor)) return null;
  // The cheap check as well: the KV floor can only be higher than the object's
  // when something other than the object raised it (a ban in KV).
  return deps.verifyAuth(request, env);
}

/// When the account's value freeze ends, in milliseconds, or 0 when it is not
/// frozen. Reads the KV mirror (up to about a minute behind the object), for the
/// scheduled payout run and other code that moves value with no session.
/// Throws when KV cannot be read: a caller that moves value must not carry on
/// as if the account were free.
export async function valueFrozenUntil(env, accountId) {
  const raw = await env.USERS.get(`valuefreeze:${accountId}`);
  const until = Number(raw) || 0;
  return until > Date.now() ? until : 0;
}

/// When the payout destination may be set or changed again, in milliseconds, or 0
/// when it may be now. The pin follows a key replacement and lasts 30 days. `ok`
/// is false when the object cannot be asked: a caller that changes where money
/// goes must refuse then, not carry on.
export async function payoutPinnedUntil(env, accountId) {
  if (!env.ACCOUNT_AUTH) return { ok: true, until: 0 };
  const held = await accountState(env, accountId);
  if (!held.ok) return held.code === 'no_account' ? { ok: true, until: 0 } : { ok: false, until: 0 };
  return { ok: true, until: held.payout_pinned_until > Date.now() ? held.payout_pinned_until : 0 };
}
