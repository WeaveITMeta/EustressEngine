// =============================================================================
// "Tell me when it is out": a release notice for the Player and for Studio
// =============================================================================
//
// There is no release yet, so the download page offers "Get notified". A signed-in account subscribes
// to ONE product (`player` or `studio`); when an administrator announces a release, every subscriber
// whose email is still verified is emailed once, and the subscription ends with that email.
//
//     GET    /api/release-notify/{product}                { subscribed, email_verified }          session
//     PUT    /api/release-notify/{product}                subscribe                               session
//     DELETE /api/release-notify/{product}                unsubscribe                             session
//     POST   /api/admin/release-notify/{product}/send     { version, url, dry_run?, cursor?, limit? }   admin
//
// The address is never taken from a request. The email is the account's own VERIFIED email (the same
// predicate the email routes use: `email_status` on the account's object, src/email_ops.mjs), read when the
// notice is sent, so an address changed or unverified since the subscription is honoured. A subscription
// is one key, `relnotify:{product}:{accountId}`, holding `{ at }` and nothing else: no copy of the address.
//
// The send is NOT run by anything automatic. It works through the subscribers a page at a time (one KV page
// of at most `limit`, SEND_PAGE_MAX at most, since one request can only make so many calls); the answer carries
// `next_cursor` until `done`. For each subscriber: the account must still exist, the email must still be
// verified (otherwise the subscription is kept and counted as skipped), then the address is CLAIMED before
// anything is sent, ONE email goes out in the shared shell through the account mail path and its daily
// allowances (src/account_notices.mjs, src/mail_limits.mjs), the claim is rewritten as sent, and the
// subscription is deleted.
//
// At most once, whatever overlaps or fails. The claim is an R2 object created with a precondition
// (`etagDoesNotMatch: '*'`), which R2 enforces atomically: the primitive moderation's lease uses
// (acquireLease in src/moderation.mjs), because KV is eventually consistent and cannot do this. Two sends that overlap, a retry
// after a timeout and a double click all race for the claim and one wins. A claim is made BEFORE the mail is sent, so a record exists the moment mail can leave,
// and no failure of the records that come after can make a second email possible. A send that fails (or is refused by the
// daily allowance) deletes its claim, so the next run retries. A claim that is still unconfirmed after
// CLAIM_STALE_MS (the Worker died between claim and mail, or could not write the confirmation) is NOT treated as sent and
// is not sent again: it is counted as `unconfirmed` for a person to look at, because at most once is the right failure for
// an email and a silent skip is not.
//
// The claim is named by a keyed hash of the address (HMAC-SHA-256, see claimHash), never by the address.
// The answer counts and never names: { sent, skipped_unverified, failed, already_sent, unconfirmed, orphans_removed }.
// Design: docs/networking/RELEASE_NOTIFY.md.

import { accountState, accountCall } from './account_auth.mjs';
import { cleanAddress } from './email_ops.mjs';
import { sendNotice, scrubAddresses } from './account_notices.mjs';
import { renderEmail, html, paragraph, note, button, urlLine } from './email_shell.mjs';

/// The products, by the name the routes use and the name an email uses. Anything else is 404.
export const PRODUCTS = Object.freeze({ player: 'Eustress Player', studio: 'Eustress Studio' });

/// What one invocation may spend. Every KV, R2, Durable Object and mail call counts against the platform's subrequest limit
/// for one request (1000 on the plan this runs on). One subscriber costs, at most:
///   the account's user record 1, its object's state 1 (4 the first time the object is seeded) and email_status 1,
///   the claim 1, the daily allowance 4 (two KV reads and two writes), the mail 1, the claim rewritten 1, the subscription deleted 1
///   = 11, and 14 when the object is seeded in this run.
/// A request itself spends about 12 (the admin check and session gate, the list, the audit entry).
/// 60 subscribers is 60 x 14 + 12 = 852 calls at worst, under the limit with room to spare; 70 would be 992.
export const SUBREQUESTS_PER_SUBSCRIBER = 14;
export const SUBREQUESTS_PER_REQUEST = 12;
export const SUBREQUEST_LIMIT = 1000;
/// Subscribers handled in one send request when no limit is asked for, and the most one request may ask for.
export const SEND_PAGE_DEFAULT = 50;
export const SEND_PAGE_MAX = 60;

/// The only hosts a release link may be on, and where on them. Any path on the downloads host; on the site only the
/// download pages. Never a query or a fragment: `?to=` on a page of ours is a redirect somebody else can aim.
export const RELEASE_LINK_HOSTS = Object.freeze(['downloads.eustress.dev', 'eustress.dev']);
/// A version is major.minor.patch with an optional short pre-release tag: 0.1.0, 1.0.0-alpha.1.
export const VERSION_PATTERN = /^\d{1,4}\.\d{1,4}\.\d{1,4}(?:-[0-9A-Za-z][0-9A-Za-z.-]{0,19})?$/;
/// How long a claim that was never confirmed is believed to be a send in progress. Past it, it is `unconfirmed`.
export const CLAIM_STALE_MS = 5 * 60 * 1000;
const MAX_URL_LENGTH = 300;
const MAX_CURSOR_LENGTH = 1024;
const JSON_META = { httpMetadata: { contentType: 'application/json' } };

const USER_ROUTE = /^\/api\/release-notify\/([a-z0-9-]{1,32})$/;
const SEND_ROUTE = /^\/api\/admin\/release-notify\/([a-z0-9-]{1,32})\/send$/;

export const subscriptionKey = (product, accountId) => `relnotify:${product}:${accountId}`;
const subscriptionPrefix = (product) => `relnotify:${product}:`;
/// The claim on one address for one version of one product: R2, where a create-if-absent is atomic.
export const claimKey = (product, version, hash) => `locks/release-notify/${product}/${version}/${hash}.json`;

const isProduct = (product) => Object.prototype.hasOwnProperty.call(PRODUCTS, product);

const encode = (text) => new TextEncoder().encode(text);
const hex = (bytes) => [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, '0')).join('');
async function hmac(keyBytes, message) {
  const key = await crypto.subtle.importKey('raw', keyBytes, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  return new Uint8Array(await crypto.subtle.sign('HMAC', key, encode(message)));
}

/// The secret an address is named under: the one the account object's audit uses for the same job (`addressPrint` in
/// src/email_ops.mjs): AUDIT_PRINT_KEY, else the Worker's JWT_SECRET. Null where neither is set.
export const claimSecret = (env) => env?.AUDIT_PRINT_KEY || env?.JWT_SECRET || null;

/// A keyed hash of an address: HMAC-SHA-256 under a key derived from the secret for this one purpose (the way src/visits.mjs
/// keys an address), so nobody who can read the claims can test whether an address received a notice, and the secret itself
/// signs nothing here. 128 bits, in hex.
export async function claimHash(env, address) {
  const subkey = await hmac(encode(String(claimSecret(env))), 'eustress-release-notify-v1');
  return hex(await hmac(subkey, String(address).toLowerCase())).slice(0, 32);
}

/// Whether text holds a space or a control character (code points 0 to 32 and 127 to 159).
const hasSpaceOrControl = (text) => [...text].some((ch) => { const c = ch.codePointAt(0); return c <= 32 || (c >= 127 && c <= 159); });

/// A release link, or the reason it is refused: https, one of the two hosts, no credentials, no port, no control character, no
/// query and no fragment, and on the site only a download page. The link in the email is the address as the URL parser reads it.
export function checkReleaseUrl(value) {
  if (typeof value !== 'string' || !value || value.length > MAX_URL_LENGTH || hasSpaceOrControl(value)) {
    return { ok: false, error: `url must be one https address of at most ${MAX_URL_LENGTH} characters, with no spaces` };
  }
  let parsed;
  try { parsed = new URL(value); } catch { return { ok: false, error: 'url is not an address' }; }
  if (parsed.protocol !== 'https:') return { ok: false, error: 'url must be https' };
  if (parsed.username || parsed.password) return { ok: false, error: 'url may not carry a name or password' };
  if (parsed.port) return { ok: false, error: 'url may not name a port' };
  if (!RELEASE_LINK_HOSTS.includes(parsed.hostname)) return { ok: false, error: `url must be on ${RELEASE_LINK_HOSTS.join(' or ')}` };
  if (value.includes('?') || value.includes('#') || parsed.search || parsed.hash) return { ok: false, error: 'url may not carry a query or a fragment' };
  if (parsed.hostname === 'eustress.dev' && !(parsed.pathname === '/download' || parsed.pathname.startsWith('/download/') || parsed.pathname === '/downloads' || parsed.pathname.startsWith('/downloads/'))) {
    return { ok: false, error: 'on eustress.dev the url must be a download page: a path under /download or /downloads/' };
  }
  return { ok: true, url: parsed.href };
}

/// The release email in the shell: its subject, plain text and HTML. It says what it is, who it is for and that it is sent
/// once, carries the one link and no other (the `plain` footer has none), and no tracking of any kind.
export function renderReleaseEmail({ product, version, url, assetOrigin }) {
  if (!isProduct(product)) throw new Error(`unknown product ${product}`);
  const name = PRODUCTS[product];
  const heading = `${name} ${version} is available`;
  const once = `This is a one-time notice. It was sent to the verified email address of the Eustress account that asked to be told when ${name} is released, and it is sent once: the request ends with this message. If you did not ask for it, ignore it.`;
  const text = [
    `${name} ${version} is available.`,
    '',
    `You asked to be told when ${name} is released. Get it here:`,
    url,
    '',
    once,
  ].join('\n');
  const body = html`${paragraph(`You asked to be told when ${name} is released. Version ${version} is ready.`)}
${button(url, `Get ${name}`)}
${urlLine('If the button does not work, copy this address into your browser:', url)}
${note(once)}`;
  const email = renderEmail({
    title: heading,
    heading,
    label: 'Release Notice',
    preheader: `${name} ${version} is available.`,
    body,
    text,
    footer: 'plain',
    assetOrigin,
  });
  return { subject: `${name} ${version} is available`, text: email.text, html: email.html };
}

/// Whether an account's email is verified, and the address, from the account's object: the one predicate the email routes use.
/// `{ ok: false, gone: true }` only when the account is DEFINITELY gone (its user record is not there); any other failure is
/// `{ ok: false }` and says nothing about whether the account exists.
export async function emailStatusOf(env, accountId) {
  if ((await env.USERS.get(`user:${accountId}`)) === null) return { ok: false, gone: true, code: 'no_account' };
  const held = await accountState(env, accountId);
  if (!held.ok) return { ok: false, gone: held.code === 'no_account', code: held.code || 'unavailable' };
  const r = await accountCall(env, accountId, 'email_status');
  if (!r.ok) return { ok: false, code: r.code || 'unavailable' };
  const address = cleanAddress(r.email?.on_file);
  return { ok: true, verified: r.email?.verified === true && !!address, address };
}

/// Returns a Response for a release-notify route, or null when the path is not one.
/// `deps`: verifyAuth, requireAdmin, json, auditLog (index.js). `deps.emailStatus` replaces the account lookup (a test
/// hands in its own to reach thousands of subscribers without thousands of account objects).
export async function handleReleaseNotify(request, url, env, cors, deps) {
  let match;
  if ((match = USER_ROUTE.exec(url.pathname))) return userRoute(request, match[1], env, cors, deps);
  if ((match = SEND_ROUTE.exec(url.pathname))) return sendRoute(request, url, match[1], env, cors, deps);
  return null;
}

// -----------------------------------------------------------------------------
// The account's side
// -----------------------------------------------------------------------------

async function userRoute(request, product, env, cors, deps) {
  const { json } = deps;
  const method = request.method;
  if (!['GET', 'PUT', 'DELETE'].includes(method)) return json({ error: 'Method not allowed' }, 405, cors);
  if (!isProduct(product)) return json({ error: 'Unknown product', code: 'unknown_product' }, 404, cors);
  const accountId = await deps.verifyAuth(request, env);
  if (!accountId) return json({ error: 'Unauthorized' }, 401, cors);
  if (!env.ACCOUNT_AUTH) return json({ error: 'This is not available on this deployment', code: 'not_configured' }, 501, cors);
  if (method !== 'GET' && env.AUTH_RATE_LIMITER) {
    const { success } = await env.AUTH_RATE_LIMITER.limit({ key: `release-notify:${accountId}` });
    if (!success) return json({ error: 'Too many requests. Try again in a minute.', code: 'rate_limited' }, 429, { ...cors, 'Retry-After': '60' });
  }

  const key = subscriptionKey(product, accountId);
  if (method === 'DELETE') {
    await env.USERS.delete(key);
    return json({ ok: true, subscribed: false }, 200, cors);
  }

  let status;
  try { status = await (deps.emailStatus || emailStatusOf)(env, accountId); } catch { status = { ok: false }; }
  if (status.gone) return json({ error: 'Account not found', code: 'no_account' }, 404, cors);
  if (!status.ok) return json({ error: 'This is temporarily unavailable. Try again in a minute.', code: 'account_unavailable' }, 503, cors);
  if (method === 'GET') {
    return json({ subscribed: (await env.USERS.get(key)) !== null, email_verified: status.verified }, 200, cors);
  }

  // PUT. No part of the request is read: the address is the account's own, read when the notice is sent.
  if (!status.verified) {
    return json({ error: 'Verify your email address first. The notice goes to the address verified on your account.', code: 'email_not_verified' }, 409, cors);
  }
  if ((await env.USERS.get(key)) === null) await env.USERS.put(key, JSON.stringify({ at: new Date(Date.now()).toISOString() }));
  return json({ ok: true, subscribed: true, email_verified: true }, 200, cors);
}

// -----------------------------------------------------------------------------
// The administrator's send
// -----------------------------------------------------------------------------

async function sendRoute(request, url, product, env, cors, deps) {
  const { json } = deps;
  if (request.method !== 'POST') return json({ error: 'Method not allowed' }, 405, cors);
  const adminId = await deps.requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);
  if (!isProduct(product)) return json({ error: 'Unknown product', code: 'unknown_product' }, 404, cors);

  let body;
  try { body = await request.json(); } catch { body = null; }
  if (!body || typeof body !== 'object' || Array.isArray(body)) return json({ error: 'A JSON body is required', code: 'bad_request' }, 400, cors);
  if (typeof body.version !== 'string' || !VERSION_PATTERN.test(body.version)) {
    return json({ error: 'version must look like 0.1.0 or 1.0.0-alpha.1', code: 'bad_version' }, 400, cors);
  }
  const link = checkReleaseUrl(body.url);
  if (!link.ok) return json({ error: link.error, code: 'bad_url' }, 400, cors);
  if (body.dry_run !== undefined && typeof body.dry_run !== 'boolean') return json({ error: 'dry_run must be true or false', code: 'bad_request' }, 400, cors);
  const dryRun = body.dry_run === true;
  let limit = SEND_PAGE_DEFAULT;
  if (body.limit !== undefined) {
    if (!Number.isInteger(body.limit) || body.limit < 1 || body.limit > SEND_PAGE_MAX) {
      return json({ error: `limit must be a whole number from 1 to ${SEND_PAGE_MAX}`, code: 'bad_limit' }, 400, cors);
    }
    limit = body.limit;
  }
  if (body.cursor !== undefined && body.cursor !== null && (typeof body.cursor !== 'string' || !body.cursor || body.cursor.length > MAX_CURSOR_LENGTH)) {
    return json({ error: 'cursor is the next_cursor of the earlier answer', code: 'bad_cursor' }, 400, cors);
  }
  if (!env.ACCOUNT_AUTH || !env.SCENES || !claimSecret(env)) return json({ error: 'This is not available on this deployment', code: 'not_configured' }, 501, cors);
  if (!dryRun && !env.EMAIL) return json({ error: 'Email is not configured on this deployment', code: 'mail_not_configured' }, 501, cors);

  const counts = { sent: 0, skipped_unverified: 0, failed: 0, already_sent: 0, unconfirmed: 0, orphans_removed: 0, would_send: 0, would_remove_orphans: 0 };
  const run = {
    env, product, version: body.version, dryRun, runId: crypto.randomUUID(),
    rendered: dryRun ? null : renderReleaseEmail({ product, version: body.version, url: link.url, assetOrigin: new URL(request.url).origin }),
    from: env.RELEASE_MAIL_FROM || env.NOTICE_MAIL_FROM || 'identity@eustress.dev',
    lookup: deps.emailStatus || emailStatusOf,
  };
  const prefix = subscriptionPrefix(product);
  let page = { keys: [], list_complete: true };
  let failure = null;
  try {
    page = await env.USERS.list({ prefix, limit, ...(body.cursor ? { cursor: body.cursor } : {}) });
    for (const { name } of page.keys || []) {
      const accountId = name.slice(prefix.length);
      if (!accountId) continue;
      counts[await notifyOne(run, accountId, name)] += 1;
    }
  } catch (e) {
    failure = e;
  }

  const done = !failure && (page.list_complete !== false || !page.cursor);
  const summary = { ...counts, dry_run: dryRun, done, next_cursor: done || failure ? null : page.cursor };
  // The audit entry names the product, the version, the link and the counts: never an address.
  try {
    await deps.auditLog(env, 'RELEASE_NOTIFY_SEND', adminId, product, {
      version: body.version, url: link.url, dry_run: dryRun, continued: !!body.cursor, limit, ...counts, done, error: failure ? scrubAddresses(failure?.message || failure) : undefined,
    });
  } catch (e) {
    console.error('release notify audit entry not written:', scrubAddresses(e?.message || e));
  }
  if (failure) {
    console.error('release notify send stopped:', scrubAddresses(failure?.message || failure));
    return json({ error: 'The send stopped part way. Run it again from the start: nobody is emailed twice.', code: 'send_failed', ...summary }, 500, cors);
  }
  return json({ ok: true, product, version: body.version, ...summary }, 200, cors);
}

/// Runs one storage step whose failure must not undo what came before it: logged, never thrown.
async function attempt(what, step) {
  try { await step(); } catch (e) { console.error(`${what}:`, scrubAddresses(e?.message || e)); }
}

/// What a claim that this run did not make says about the address: sent, being sent by another run right now, or unconfirmed.
function readHeld(held, now) {
  if (held?.state === 'sent') return { count: 'already_sent', endSubscription: true };
  if (held?.state === 'claiming' && now - held.at < CLAIM_STALE_MS) return { count: 'already_sent', endSubscription: false };
  return { count: 'unconfirmed', endSubscription: false };
}

async function readClaim(env, key) {
  const object = await env.SCENES.get(key);
  if (!object) return null;
  try {
    const body = JSON.parse(await object.text());
    return { state: body?.state, at: Number(body?.at) || 0 };
  } catch {
    return { state: 'unreadable', at: 0 };
  }
}

/// Create the claim if there is none: R2 enforces the precondition atomically. True when this run now holds it.
async function claimAddress(env, key, runId) {
  return Boolean(await env.SCENES.put(key, JSON.stringify({ state: 'claiming', at: Date.now(), run: runId }), { ...JSON_META, onlyIf: { etagDoesNotMatch: '*' } }));
}

/// One subscriber. Returns the count it belongs to.
async function notifyOne(run, accountId, subscription) {
  const { env, product, version, dryRun } = run;
  let status;
  try { status = await run.lookup(env, accountId); } catch { status = { ok: false }; }
  if (!status.ok) {
    // Only an account that is DEFINITELY gone loses its subscription; a lookup that failed for any other reason is a failure to try again.
    if (!status.gone) return 'failed';
    if (dryRun) return 'would_remove_orphans';
    try { await env.USERS.delete(subscription); } catch (e) { console.error('release notify orphan not removed:', scrubAddresses(e?.message || e)); return 'failed'; }
    return 'orphans_removed';
  }
  // The email is read now, not when the subscription was made: one that is no longer verified is skipped and kept.
  if (!status.verified) return 'skipped_unverified';

  const key = claimKey(product, version, await claimHash(env, status.address));
  if (dryRun) {
    const held = await readClaim(env, key);
    return held ? readHeld(held, Date.now()).count : 'would_send';
  }

  // Claim the address BEFORE any mail can leave. A claim someone else holds is never sent over.
  let claimed = await claimAddress(env, key, run.runId);
  if (!claimed) {
    const held = await readClaim(env, key);
    // Released between the two calls: one more try. Lost again, someone else is sending it.
    if (!held) claimed = await claimAddress(env, key, run.runId);
    if (!claimed) {
      const verdict = held ? readHeld(held, Date.now()) : { count: 'already_sent', endSubscription: false };
      if (verdict.endSubscription) await attempt('release notify subscription not removed', () => env.USERS.delete(subscription));
      return verdict.count;
    }
  }

  const result = (await sendNotice(env, [status.address], run.rendered, { from: run.from, fromName: 'Eustress', kind: 'notice', accountId }))[0];
  if (!result?.sent) {
    // Nothing left: give the claim back so the next run can try again.
    await attempt('release notify claim not released', () => env.SCENES.delete(key));
    return 'failed';
  }
  // Mailed. Confirm the claim, then end the subscription. Whichever fails, the claim stands, so no run can send this address again.
  await attempt('release notify claim not confirmed', () => env.SCENES.put(key, JSON.stringify({ state: 'sent', at: Date.now(), run: run.runId }), JSON_META));
  await attempt('release notify subscription not removed', () => env.USERS.delete(subscription));
  return 'sent';
}
