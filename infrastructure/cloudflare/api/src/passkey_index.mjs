// =============================================================================
// PasskeyIndex: which account holds a passkey's credential id
// =============================================================================
//
// One Durable Object per credential id (named by a hash of the id), holding the
// one account that has claimed it. Strongly consistent, so two accounts cannot
// both hold an id, and one account cannot release or overwrite another's claim:
//
//   claim    { account_id }  ok when the id is free or already this account's;
//                            409 passkey_registered when another account holds it
//   owner    {}              the account that holds it, or null
//   release  { account_id }  frees it, only when this account holds it
//
// The account's object (src/account_auth.mjs) claims an id when it adds the
// passkey, and releases it through its outbox (retried until it lands) when the
// passkey is removed: by its owner, by a key replacement, or with the account.
// A passkey sign-in asks `owner` to find the account. A lookup or a claim that
// finds the holder's account gone (deleted, and the release never landed) frees
// the id. The Worker is the only caller; nothing here is reachable from the internet.
// =============================================================================

const ACCOUNT_ID = /^[A-Za-z0-9_-]{8,64}$/;
const CALL_TIMEOUT_MS = 8000;
/// A claim is not judged an orphan until it is this old: KV, where the account's
/// record lives, can take up to about a minute to show a new account everywhere.
export const ORPHAN_GRACE_MS = 60 * 60 * 1000;

const fail = (status, code, error) => ({ ok: false, status, code, error });

export class PasskeyIndex {
  constructor(ctx, env) {
    this.storage = ctx.storage;
    this.env = env;
    // One operation at a time, whole.
    this.chain = Promise.resolve();
  }

  fetch(request) {
    const run = this.chain.then(() => this.handle(request));
    this.chain = run.catch(() => {});
    return run;
  }

  async handle(request) {
    const op = new URL(request.url).pathname.slice(1);
    let args = {};
    try {
      const v = await request.json();
      if (v && typeof v === 'object' && !Array.isArray(v)) args = v;
    } catch { /* no body */ }
    let result;
    try {
      result = await this.run(op, args);
    } catch (e) {
      console.error(`PasskeyIndex.${op} failed:`, e);
      result = fail(500, 'internal', 'internal error');
    }
    return new Response(JSON.stringify(result), { status: result.ok === false ? result.status || 500 : 200, headers: { 'content-type': 'application/json' } });
  }

  /// Whether the account holding a claim still exists. A claim whose account was
  /// deleted (and whose release never landed, say because clearing the passkeys
  /// failed) is an orphan: it would keep the id from ever being registered again.
  /// KV is eventually consistent, so a claim younger than ORPHAN_GRACE_MS is never
  /// judged, and a KV that cannot be read means the account exists.
  async ownerExists(held) {
    if (!this.env?.USERS) return true;
    const at = Date.parse(held.at);
    if (!Number.isFinite(at) || Date.now() - at < ORPHAN_GRACE_MS) return true;
    try {
      return (await this.env.USERS.get(`user:${held.account_id}`)) !== null;
    } catch {
      return true;
    }
  }

  async run(op, args) {
    let held = await this.storage.get('owner');
    // A lookup (sign-in) or a claim that finds an orphan releases it: the id is free.
    if (held && (op === 'owner' || op === 'claim') && !(await this.ownerExists(held))) {
      console.error('PasskeyIndex released the claim of an account that no longer exists');
      await this.storage.delete('owner');
      held = null;
    }
    if (op === 'owner') return { ok: true, account_id: held?.account_id || null };
    const accountId = typeof args.account_id === 'string' && ACCOUNT_ID.test(args.account_id) ? args.account_id : null;
    if (!accountId) return fail(400, 'bad_request', 'account_id required');
    if (op === 'claim') {
      if (held && held.account_id !== accountId) return fail(409, 'passkey_registered', 'another account holds this passkey');
      if (!held) await this.storage.put('owner', { account_id: accountId, at: new Date().toISOString() });
      return { ok: true };
    }
    if (op === 'release') {
      if (!held || held.account_id !== accountId) return { ok: true, released: false };
      await this.storage.delete('owner');
      return { ok: true, released: true };
    }
    return fail(404, 'unknown_operation', `unknown operation ${op}`);
  }
}

async function sha256Hex(text) {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

/// The object's name for a credential id: a hash, so any id length fits.
export async function passkeyIndexName(credentialId) {
  return `passkey:${await sha256Hex(`eustress-passkey:${credentialId}`)}`;
}

/// Call one operation of a credential id's index object. Never throws: an object
/// that cannot be reached answers `{ ok: false, status: 503, code: 'unavailable' }`.
export async function passkeyIndexCall(env, credentialId, op, args = {}) {
  if (!env.PASSKEY_INDEX) return fail(503, 'no_binding', 'the passkey index is not configured');
  let timer;
  try {
    const stub = env.PASSKEY_INDEX.get(env.PASSKEY_INDEX.idFromName(await passkeyIndexName(credentialId)));
    const call = (async () => {
      const res = await stub.fetch(`https://passkey-index.internal/${op}`, {
        method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(args),
      });
      return await res.json();
    })();
    const timeout = new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`passkey index ${op} timed out`)), Number(env.ACCOUNT_CALL_TIMEOUT_MS) || CALL_TIMEOUT_MS); });
    return await Promise.race([call, timeout]);
  } catch (e) {
    console.error(`passkeyIndexCall ${op} failed:`, e?.message || e);
    return fail(503, 'unavailable', 'passkey index unavailable');
  } finally {
    clearTimeout(timer);
  }
}
