// =============================================================================
// Commerce: microtransactions inside published simulations
// =============================================================================
//
// A creator sells products (consumables and passes, priced in whole Tickets)
// inside a simulation they published. Players buy them from inside the
// simulation; the creator's script grants what was bought and acknowledges it.
//
// The shape follows Stripe so its habits carry over: prefixed ids (prod_, pur_,
// evt_, we_, key_), test and live modes chosen by the API key (ek_test_... and
// ek_live_...) or, for a signed-in session, by `Eustress-Mode: live` (test
// without it), an event for everything that happens, signed webhooks, a
// `listen` stream for local development, and test helpers that fire events
// without moving money. `eustress commerce` in the CLI is its client.
//
// ## Where state lives
//
// Two Durable Object classes, because money and ordering need what KV cannot
// give: KV has no compare-and-set, and it is eventually consistent between
// locations.
//
//   Wallet       one per account. The authoritative Ticket balance, and the
//                buyer's side of every purchase: idempotency keys, passes
//                owned, receipts waiting for the game to process them, and an
//                outbox that finishes each purchase (credit the creator, tell
//                the creator's hub, write the books), retried on an alarm.
//                It opens with the balance USERS `user:{id}.ticket_balance`
//                held, and copies every change to USERS `ticketbal:{id}` for
//                readers outside it; the copy is for display only and nothing
//                decides with it.
//   CommerceHub  one per creator. Products, the creator's record of every
//                sale, events (kept 30 days), webhook endpoints and their
//                deliveries, the listen stream, API keys, and each sale's
//                deferred value score.
//
// KV keeps only what must be found without knowing the owner:
// `USERS:apikey:{sha256}` maps an API key to its account. Receipts stay where
// purchases.mjs keeps them.
//
// ## Money
//
// A price is read from the product at purchase time. The buyer also sends the
// price it showed the player (`expected_price`), and a mismatch is refused, so
// nobody is charged a price they did not see. A live purchase debits the buyer
// the price and credits the creator 70% (rounded down); the rest is the
// platform's. Test purchases move nothing. A sale's value score, which feeds
// the daily Bliss distribution, is credited only after its refund window has
// closed, so a refunded sale never earns one, and only for the part of the
// price paid with Tickets bought for money: Tickets earned from sales and
// spent again score nothing, so trading them in a circle earns nothing.
// =============================================================================

import { cleanText, cleanIcon, isAccountId, recordPurchase } from './purchases.mjs';
import { creditValueScore } from './bliss.mjs';

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

export const API_VERSION = '2026-09-23';

/// The creator's share of a live sale, rounded down to whole Tickets.
export const CREATOR_SHARE = 0.70;

/// A live purchase may be refunded by its creator for this long. Its value
/// score is filed a minute after the window closes (see settleAt).
export const REFUND_WINDOW_MS = 72 * 3600 * 1000;

/// Events are kept this long, like Stripe's 30 days.
export const EVENT_RETENTION_MS = 30 * 86400 * 1000;

export const LIMITS = {
  price_max: 1_000_000,
  products_per_simulation: 500,
  product_name: 60,
  product_description: 500,
  space_name: 96,
  metadata_keys: 20,
  metadata_key: 40,
  metadata_value: 500,
  keys_per_account: 25,
  key_name: 60,
  webhook_endpoints_per_account: 16,
  webhook_url: 2048,
  list_max: 100,
  stream_wait_max_s: 25,
};

export const PRODUCT_TYPES = ['consumable', 'pass'];

export const EVENT_TYPES = [
  'product.created',
  'product.updated',
  'product.archived',
  'purchase.succeeded',
  'purchase.failed',
  'purchase.fulfilled',
  'purchase.refunded',
];

/// Event types the test helper can fire.
export const TRIGGERS = ['purchase.succeeded', 'purchase.failed', 'purchase.fulfilled', 'purchase.refunded'];

/// Key types the account's key list accepts. Only `commerce` keys may use this
/// API; the others are the web dashboard's labels for keys other services read.
export const KEY_TYPES = ['commerce', 'datastore', 'http', 'ai'];

/// Wait after each failed webhook delivery before the next attempt: seven
/// retries over about 42 hours, then the delivery is given up.
const WEBHOOK_RETRY_DELAYS_MS = [60e3, 5 * 60e3, 30 * 60e3, 2 * 3600e3, 5 * 3600e3, 10 * 3600e3, 24 * 3600e3];

/// Wait after each failed outbox attempt before the next. The last delay
/// repeats: an outbox step is never abandoned, because it finishes a purchase
/// the buyer has already paid for.
const OUTBOX_RETRY_DELAYS_MS = [5e3, 30e3, 2 * 60e3, 10 * 60e3, 60 * 60e3];

const WEBHOOK_TIMEOUT_MS = 10_000;

// -----------------------------------------------------------------------------
// Ids, time and crypto
// -----------------------------------------------------------------------------

const B62 = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz';

/// `length` characters of base62 from the CSPRNG. Bytes of 248 and above are
/// dropped (248 = 62 * 4), so every character is equally likely.
export function randomToken(length) {
  let out = '';
  while (out.length < length) {
    const bytes = crypto.getRandomValues(new Uint8Array(length * 2));
    for (const b of bytes) {
      if (b >= 248) continue;
      out += B62[b % 62];
      if (out.length === length) break;
    }
  }
  return out;
}

export const newId = (prefix) => `${prefix}_${randomToken(24)}`;

const nowS = () => Math.floor(Date.now() / 1000);

const encoder = new TextEncoder();

function toHex(buffer) {
  return [...new Uint8Array(buffer)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

export async function sha256Hex(text) {
  return toHex(await crypto.subtle.digest('SHA-256', encoder.encode(text)));
}

export async function hmacSha256Hex(secret, message) {
  const key = await crypto.subtle.importKey('raw', encoder.encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  return toHex(await crypto.subtle.sign('HMAC', key, encoder.encode(message)));
}

/// `Eustress-Signature` for a payload: `t={unix seconds},v1={hex}` where the
/// hex is HMAC-SHA256 of `{t}.{payload}` under the endpoint's secret. The
/// same scheme as Stripe, so a receiver verifies it the same way.
export async function signatureHeader(secret, payload, t = nowS()) {
  return `t=${t},v1=${await hmacSha256Hex(secret, `${t}.${payload}`)}`;
}

function timingSafeEqualHex(a, b) {
  if (typeof a !== 'string' || typeof b !== 'string' || a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

/// True when `header` signs `payload` with `secret` and its timestamp is
/// within `toleranceS` of now.
export async function verifySignatureHeader(secret, payload, header, { toleranceS = 300, now = nowS() } = {}) {
  if (typeof header !== 'string') return false;
  const parts = Object.fromEntries(
    header.split(',').map((p) => p.trim().split('=')).filter((kv) => kv.length === 2),
  );
  const t = Number(parts.t);
  if (!Number.isSafeInteger(t) || Math.abs(now - t) > toleranceS) return false;
  return timingSafeEqualHex(parts.v1, await hmacSha256Hex(secret, `${t}.${payload}`));
}

// -----------------------------------------------------------------------------
// API keys
// -----------------------------------------------------------------------------

/// The mode an API key carries in its prefix, or null for anything else.
export function keyModeOf(token) {
  if (typeof token !== 'string') return null;
  if (/^ek_test_[A-Za-z0-9]{40}$/.test(token)) return 'test';
  if (/^ek_live_[A-Za-z0-9]{40}$/.test(token)) return 'live';
  return null;
}

export function generateApiKey(mode) {
  return `ek_${mode}_${randomToken(40)}`;
}

const apiKeyIndexKey = (hash) => `apikey:${hash}`;

// -----------------------------------------------------------------------------
// Input cleaning
// -----------------------------------------------------------------------------

export function isSimulationId(value) {
  return typeof value === 'string' && /^[a-f0-9-]{8,64}$/.test(value);
}

const isObjectId = (prefix, value) =>
  typeof value === 'string' && new RegExp(`^${prefix}_[A-Za-z0-9]{24}$`).test(value);

export const isProductId = (v) => isObjectId('prod', v);
export const isPurchaseId = (v) => isObjectId('pur', v);
export const isEventId = (v) => isObjectId('evt', v);
export const isEndpointId = (v) => isObjectId('we', v);
export const isKeyId = (v) => isObjectId('key', v);

const isWhole = (n) => Number.isSafeInteger(n);

export function cleanPrice(value) {
  const n = typeof value === 'string' && /^\d+$/.test(value.trim()) ? Number(value.trim()) : value;
  return isWhole(n) && n >= 1 && n <= LIMITS.price_max ? n : null;
}

/// A product reference as scripts and the CLI give it: the `prod_` id, or the
/// product's number within its simulation (what `PromptPurchase` takes).
export function productRef(value) {
  if (isProductId(value)) return { id: value };
  const n = typeof value === 'string' && /^\d{1,9}$/.test(value.trim()) ? Number(value.trim()) : value;
  if (isWhole(n) && n >= 1 && n <= 999_999_999) return { number: n };
  return null;
}

/// A Space name as it appears in the Universe's `Spaces/` folder.
export function cleanSpaceName(value) {
  if (typeof value !== 'string') return '';
  const s = value.trim();
  if (!s || /[\u0000-\u001f\u007f/\\]/.test(s) || s === '.' || s === '..') return '';
  return encoder.encode(s).length <= LIMITS.space_name ? s : '';
}

export function cleanIdempotencyKey(value) {
  return typeof value === 'string' && /^[A-Za-z0-9_.:-]{8,100}$/.test(value) ? value : '';
}

/// Stripe-style metadata: up to 20 string pairs. `null` clears a key on update.
export function cleanMetadata(value, previous = {}) {
  if (value === undefined) return { ok: true, metadata: { ...previous } };
  if (value === null) return { ok: true, metadata: {} };
  if (typeof value !== 'object' || Array.isArray(value)) return { ok: false, error: 'metadata must be an object' };
  const out = { ...previous };
  for (const [k, v] of Object.entries(value)) {
    if (!/^[A-Za-z0-9_.-]{1,40}$/.test(k)) return { ok: false, error: `metadata key "${k}" is invalid` };
    if (v === null || v === '') {
      delete out[k];
      continue;
    }
    if (typeof v !== 'string' && typeof v !== 'number' && typeof v !== 'boolean') {
      return { ok: false, error: `metadata.${k} must be a string` };
    }
    out[k] = cleanText(String(v), LIMITS.metadata_value);
  }
  if (Object.keys(out).length > LIMITS.metadata_keys) {
    return { ok: false, error: `metadata holds at most ${LIMITS.metadata_keys} keys` };
  }
  return { ok: true, metadata: out };
}

/// Host suffixes a webhook may not name: special-use and private names that
/// never reach a public server, Eustress's own hosts (so an endpoint cannot
/// aim signed requests back at the API), and wildcard DNS services whose
/// names resolve to any address written into them, loopback included.
const WEBHOOK_BLOCKED_SUFFIXES = [
  'localhost', 'local', 'internal', 'intranet', 'lan', 'home', 'corp', 'private',
  'home.arpa', 'arpa', 'test', 'example', 'invalid', 'onion',
  'eustress.dev',
  'nip.io', 'sslip.io', 'xip.io', 'traefik.me', 'localtest.me', 'lvh.me', 'vcap.me', 'lacolhost.com',
];

/// A webhook URL the Worker will POST to: https on the default port, a public
/// host name, no credentials, never an IP literal. Local development uses
/// `eustress commerce listen --forward-to` instead, which runs on the
/// developer's machine.
///
/// What keeps a webhook out of private networks is where its request leaves
/// from, not these checks. Deliveries use the global fetch(), which goes out
/// from Cloudflare's edge to the public internet with no private network
/// behind it. Any host name can resolve to a private address, so the name
/// rules only turn away the obvious cases: loopback services, special-use
/// names, this API itself. The invariant to keep: a delivery never goes
/// through a binding's fetch (a service binding, a VPC service, a Tunnel).
/// Those reach what no URL check sees, and would first need the resolved
/// address validated on every request.
export function cleanWebhookUrl(value) {
  if (typeof value !== 'string' || value.length > LIMITS.webhook_url) return '';
  let url;
  try {
    url = new URL(value);
  } catch {
    return '';
  }
  if (url.protocol !== 'https:' || url.username || url.password || url.port) return '';
  // The URL parser has already turned hex, octal and short IPv4 forms into a
  // dotted quad, so this catches every IPv4 literal; IPv6 is bracketed.
  const host = url.hostname.toLowerCase().replace(/\.$/, '');
  if (/^\d{1,3}(\.\d{1,3}){3}$/.test(host) || host.startsWith('[')) return '';
  const labels = host.split('.');
  if (labels.length < 2 || labels.some((l) => !/^[a-z0-9-]{1,63}$/.test(l))) return '';
  if (WEBHOOK_BLOCKED_SUFFIXES.some((s) => host === s || host.endsWith(`.${s}`))) return '';
  return url.href;
}

// -----------------------------------------------------------------------------
// Simulations
// -----------------------------------------------------------------------------

/// Every Space name this simulation is known to have published: those uploaded
/// one at a time (`sim.spaces`) and those the Universe publish listed
/// (`sim.space_names`).
export function publishedSpaces(sim) {
  const names = new Set(Object.keys(sim?.spaces || {}));
  if (Array.isArray(sim?.space_names)) for (const n of sim.space_names) if (typeof n === 'string') names.add(n);
  return names;
}

/// Why a simulation cannot sell anything right now, or null when it can.
export function commerceBlockedReason(sim) {
  if (!sim) return 'Simulation not found';
  const status = sim.moderation?.status;
  if (status === 'quarantined') return 'This simulation is under review and cannot sell anything';
  if (status === 'rejected') return 'This simulation was rejected in review and cannot sell anything';
  if (!sim.r2_key && publishedSpaces(sim).size === 0) {
    return 'Nothing has been published to this simulation yet; publish the Universe first';
  }
  return null;
}

export async function loadSimulation(env, simId) {
  if (!isSimulationId(simId)) return null;
  const raw = await env.SOCIAL.get(`sim:${simId}`);
  if (!raw) return null;
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

// -----------------------------------------------------------------------------
// Value score
// -----------------------------------------------------------------------------

/// When a live sale's value score is filed: a minute after its refund window
/// closes, so a refund at the last moment always lands first.
const settleAt = (sale) => sale.created * 1000 + REFUND_WINDOW_MS + 60e3;

/// How long a value score that could not be filed waits before it is tried again.
const SETTLE_RETRY_MS = 10 * 60e3;

// -----------------------------------------------------------------------------
// Durable Object plumbing
// -----------------------------------------------------------------------------

export const walletStub = (env, accountId) => env.WALLETS.get(env.WALLETS.idFromName(`wallet:${accountId}`));
export const hubStub = (env, accountId) => env.COMMERCE_HUBS.get(env.COMMERCE_HUBS.idFromName(`hub:${accountId}`));

/// Call one operation on a Durable Object. Every reply is a JSON object with
/// `ok`; a failure carries `status`, `error` and usually `code`.
export async function callObject(stub, op, args) {
  const res = await stub.fetch(`https://commerce.internal/${op}`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(args ?? {}),
  });
  let body;
  try {
    body = await res.json();
  } catch {
    body = { ok: false, error: `${op}: unreadable reply` };
  }
  if (!res.ok && body.ok !== false) body = { ...body, ok: false };
  if (body.ok === false && !body.status) body.status = res.status;
  return body;
}

function objectReply(result) {
  const status = result.ok === false ? result.status || 500 : 200;
  return new Response(JSON.stringify(result), { status, headers: { 'content-type': 'application/json' } });
}

async function readJson(request) {
  try {
    const v = await request.json();
    return v && typeof v === 'object' && !Array.isArray(v) ? v : {};
  } catch {
    return null;
  }
}

const fail = (status, error, code, extra = {}) => ({ ok: false, status, error, code, ...extra });

/// Run `op` on an object's handler table, answering in the shared reply shape.
async function dispatch(self, ops, request) {
  const op = new URL(request.url).pathname.slice(1);
  const args = (await readJson(request)) ?? {};
  try {
    await self.open(args.account_id);
    const handler = ops[op];
    if (!handler) return objectReply(fail(404, `unknown operation ${op}`, 'unknown_operation'));
    return objectReply(await handler.call(self, args));
  } catch (e) {
    console.error(`${self.constructor.name}.${op} failed:`, e);
    return objectReply(fail(500, String(e?.message || e), 'internal_error'));
  }
}

/// Move the object's alarm earlier to `at` if one is not already due sooner.
async function ensureAlarm(storage, at) {
  const current = await storage.getAlarm();
  if (current === null || current === undefined || at < current) await storage.setAlarm(at);
}

/// Run `fn` once every earlier critical section of this object has finished.
///
/// A Durable Object's input gates already stop a read-modify-write made only
/// of storage calls from interleaving with another request. This queue makes
/// that explicit instead of a consequence of which calls a section happens to
/// await, so an edit that adds an outside call inside a section cannot
/// quietly open a race. Sections must not nest, and must not wait on another
/// object: both would deadlock the queue.
function atomic(self, fn) {
  const run = (self.criticalTail || Promise.resolve()).then(() => fn());
  self.criticalTail = run.then(() => {}, () => {});
  return run;
}

/// Outbox entries for `steps`, one key per step, numbered after the purchase's
/// existing entries (`existingKeys`) so they run in order. One key per step
/// means appending a step never rewrites an entry a drain is working on.
function outboxWrites(purchaseId, steps, existingKeys = []) {
  let next = 0;
  for (const key of existingKeys) next = Math.max(next, Number(key.slice(key.lastIndexOf(':') + 1)) + 1);
  const writes = {};
  steps.forEach((step, i) => {
    writes[`out:${purchaseId}:${String(next + i).padStart(3, '0')}`] = { step, attempts: 0, next_at: 0 };
  });
  return writes;
}

const inverted = (ms) => String(9_999_999_999_999 - Math.trunc(ms)).padStart(13, '0');
const modeOf = (livemode) => (livemode ? 'live' : 'test');

/// Take `amount` Tickets from a wallet's `meta`, and return how many of them
/// were paid for. A wallet's balance is paid Tickets (bought with money) plus
/// earned ones (a creator's share of sales). Spending draws earned Tickets
/// first, so Tickets passed back and forth between accounts carry no paid
/// part, and only a sale paid for with money scores (see settleDue). A
/// chargeback takes back paid Tickets first (`paidFirst`), since those are
/// what it reverses.
function drawTickets(meta, amount, { paidFirst = false } = {}) {
  const paid = Math.max(0, meta.paid_balance || 0);
  const earned = Math.max(0, meta.balance - paid);
  const paidUsed = paidFirst ? Math.min(paid, amount) : Math.min(paid, Math.max(0, amount - earned));
  meta.balance -= amount;
  meta.paid_balance = paid - paidUsed;
  return paidUsed;
}

// =============================================================================
// Wallet
// =============================================================================
//
// Storage keys:
//   meta                          { account_id, balance, version, opened_with, ... }
//   ref:{ref}                     result of an idempotent credit or debit
//   idem:{mode}:{key}             { purchase_id } for a buyer's idempotency key
//   pur:{purchase_id}             the buyer's copy of a purchase
//   ent:{mode}:{sim}:{product}    a pass the account owns
//   pend:{mode}:{sim}:{purchase}  a consumable waiting for the game to grant it
//   out:{purchase_id}:{n}         a step still owed for a purchase or refund,
//                                 run in n order

export class Wallet {
  constructor(ctx, env) {
    this.ctx = ctx;
    this.env = env;
    this.storage = ctx.storage;
    this.accountId = null;
  }

  fetch(request) {
    return dispatch(this, WALLET_OPS, request);
  }

  /// Load the account the wallet belongs to. The first time, the opening
  /// balance is whatever USERS held: the wallet takes over from there.
  async open(accountId) {
    if (this.accountId) {
      if (accountId && accountId !== this.accountId) throw new Error('wallet belongs to another account');
      return;
    }
    await this.ctx.blockConcurrencyWhile(async () => {
      if (this.accountId) return;
      let meta = await this.storage.get('meta');
      if (!meta) {
        if (!isAccountId(accountId)) throw new Error('wallet opened without an account id');
        const raw = await this.env.USERS.get(`user:${accountId}`);
        const kvBalance = raw ? Math.trunc(Number(JSON.parse(raw).ticket_balance) || 0) : 0;
        const opening = Math.max(0, kvBalance);
        meta = {
          account_id: accountId,
          balance: opening,
          // Nothing records how a balance held before the wallet was made up,
          // so all of it counts as earned: it can be spent, never scored.
          paid_balance: 0,
          version: 0,
          opened_with: opening,
          opened_at: new Date().toISOString(),
        };
        await this.storage.put('meta', meta);
      } else if (accountId && meta.account_id !== accountId) {
        throw new Error('wallet belongs to another account');
      }
      this.accountId = meta.account_id;
    });
  }

  /// Copy the balance to USERS `ticketbal:{id}` for readers outside this
  /// object (the web profile). The key is the wallet's alone, so the copy
  /// never rewrites the account record other routes edit. Copies run one at a
  /// time and each writes the balance as it stands when it runs, so the last
  /// one leaves the latest. A failed copy is retried on the alarm.
  mirrorBalance() {
    const run = (this.mirrorTail || Promise.resolve()).then(() => this.writeMirror());
    this.mirrorTail = run.then(() => {}, () => {});
    return run;
  }

  async writeMirror() {
    const meta = await this.storage.get('meta');
    if (this.mirroredVersion === meta.version && !meta.mirror_pending) return;
    try {
      await this.env.USERS.put(`ticketbal:${meta.account_id}`, JSON.stringify({
        balance: meta.balance, version: meta.version, updated_at: new Date().toISOString(),
      }));
      this.mirroredVersion = meta.version;
      if (meta.mirror_pending) {
        // Any later change queued its own copy behind this one.
        await atomic(this, async () => {
          const fresh = await this.storage.get('meta');
          delete fresh.mirror_pending;
          await this.storage.put('meta', fresh);
        });
      }
    } catch (e) {
      console.error(`wallet ${meta.account_id}: balance mirror failed, retrying on the alarm:`, e);
      await atomic(this, async () => {
        const fresh = await this.storage.get('meta');
        fresh.mirror_pending = true;
        await this.storage.put('meta', fresh);
      });
      await ensureAlarm(this.storage, Date.now() + 30e3);
    }
  }

  async alarm() {
    await this.open();
    const next = await this.drainOutbox();
    const meta = await this.storage.get('meta');
    if (meta.mirror_pending) await this.mirrorBalance();
    if (next !== null) await ensureAlarm(this.storage, next);
  }

  /// Run the owed outbox steps of the given purchases now, or, from the alarm,
  /// every step that is due. A purchase's steps run in order, and a failure
  /// stops that purchase's later steps until the retry. Two drains may run the
  /// same step; every destination is idempotent, so that is harmless. Returns
  /// when the next retry is due, or null.
  async drainOutbox(purchaseIds = null) {
    const now = Date.now();
    const entries = new Map();
    if (purchaseIds) {
      for (const id of purchaseIds) for (const [k, v] of await this.storage.list({ prefix: `out:${id}:` })) entries.set(k, v);
    } else {
      for (const [k, v] of await this.storage.list({ prefix: 'out:' })) entries.set(k, v);
    }
    const byPurchase = new Map();
    for (const [key, item] of entries) {
      const purchaseId = key.split(':')[1];
      if (!byPurchase.has(purchaseId)) byPurchase.set(purchaseId, []);
      byPurchase.get(purchaseId).push([key, item]);
    }
    let nextDue = null;
    const later = (at) => {
      nextDue = nextDue === null ? at : Math.min(nextDue, at);
    };
    for (const [purchaseId, steps] of byPurchase) {
      const record = await this.storage.get(`pur:${purchaseId}`);
      for (const [key, item] of steps) {
        if (!purchaseIds && item.next_at > now) {
          later(item.next_at);
          break;
        }
        try {
          await this.runStep(item.step, record);
          await this.storage.delete(key);
        } catch (e) {
          const failure = `${item.step}: ${e?.message || e}`;
          const retry = await atomic(this, async () => {
            const fresh = await this.storage.get(key);
            if (!fresh) return null; // another drain finished it meanwhile
            fresh.attempts = (fresh.attempts || 0) + 1;
            fresh.last_error = failure;
            fresh.next_at = Date.now() + OUTBOX_RETRY_DELAYS_MS[Math.min(fresh.attempts - 1, OUTBOX_RETRY_DELAYS_MS.length - 1)];
            await this.storage.put(key, fresh);
            return fresh;
          });
          if (retry) {
            console.error(`wallet ${this.accountId}: purchase ${purchaseId} ${failure} (attempt ${retry.attempts})`);
            later(retry.next_at);
          }
          break;
        }
      }
    }
    if (purchaseIds && nextDue !== null) await ensureAlarm(this.storage, nextDue);
    return nextDue;
  }

  /// One step that finishes a purchase or a refund elsewhere. Each is
  /// idempotent at its destination, so a retry after a partial success is safe.
  async runStep(step, rec) {
    const env = this.env;
    const ok = (reply) => {
      if (!reply.ok) throw new Error(reply.error || 'failed');
      return reply;
    };
    switch (step) {
      case 'hub':
        ok(await callObject(hubStub(env, rec.seller_id), 'record_sale', { account_id: rec.seller_id, purchase: rec }));
        return;
      case 'seller_credit':
        if (rec.creator_amount > 0) {
          ok(await callObject(walletStub(env, rec.seller_id), 'credit', {
            account_id: rec.seller_id,
            ref: `sale:${rec.id}`,
            amount: rec.creator_amount,
            reason: `Sale: ${rec.product.name}`,
          }));
        }
        return;
      case 'books':
        await writeSaleBooks(env, rec);
        return;
      case 'hub_fulfilled':
        ok(await callObject(hubStub(env, rec.seller_id), 'sale_fulfilled', {
          account_id: rec.seller_id,
          purchase_id: rec.id,
          fulfilled_at: rec.fulfilled_at,
        }));
        return;
      case 'seller_debit':
        if (rec.creator_amount > 0) {
          ok(await callObject(walletStub(env, rec.seller_id), 'debit', {
            account_id: rec.seller_id,
            ref: `refund:${rec.id}`,
            amount: rec.creator_amount,
            reason: `Refund: ${rec.product.name}`,
            allow_negative: true,
          }));
        }
        return;
      case 'hub_refunded':
        ok(await callObject(hubStub(env, rec.seller_id), 'sale_refunded', {
          account_id: rec.seller_id,
          purchase_id: rec.id,
          refunded_at: rec.refunded_at,
        }));
        return;
      case 'refund_books':
        await writeRefundBooks(env, rec);
        return;
      default:
        throw new Error(`unknown outbox step ${step}`);
    }
  }
}

/// The ledger lines and receipt of a live sale, in the formats the Tickets
/// history and the purchases page already read. Keys are derived from the
/// purchase, so writing them twice writes the same keys.
async function writeSaleBooks(env, rec) {
  const ms = rec.created * 1000;
  const iso = new Date(ms).toISOString();
  const ttl = { expirationTtl: 86400 * 365 * 3 };
  const title = rec.product.name;
  await env.INVENTORY.put(`txn:${rec.buyer_id}:${ms}:${rec.id}`, JSON.stringify({
    id: rec.id, user_id: rec.buyer_id, type: 'spend', amount: -rec.amount, currency: 'TKT',
    product_id: rec.product.id, developer_id: rec.seller_id, simulation_id: rec.sim_id,
    description: `Purchased ${title}`, timestamp: iso,
  }), ttl);
  if (rec.creator_amount > 0) {
    await env.INVENTORY.put(`txn:${rec.seller_id}:${ms}:${rec.id}`, JSON.stringify({
      id: `${rec.id}_sale`, user_id: rec.seller_id, type: 'dev_payout', amount: rec.creator_amount, currency: 'TKT',
      product_id: rec.product.id, simulation_id: rec.sim_id, counterparty_id: rec.buyer_id,
      description: `Sale: ${title} (70% of ${rec.amount} TKT)`, timestamp: iso,
    }), ttl);
  }
  await recordPurchase(env, rec.buyer_id, {
    id: rec.id, ts: iso, currency: 'TKT', units: rec.amount, title, icon: rec.product.icon,
    product_id: rec.product.id, ref: rec.id,
    attribution: {
      simulation_id: rec.sim_id, simulation_name: rec.sim_name,
      creator_id: rec.seller_id, creator_name: rec.seller_name,
    },
  });
}

/// A refund's books: ledger lines that reverse the sale and a negative receipt,
/// so the buyer's totals on the purchases page net to what they kept.
async function writeRefundBooks(env, rec) {
  const ms = rec.refunded_at * 1000;
  const iso = new Date(ms).toISOString();
  const ttl = { expirationTtl: 86400 * 365 * 3 };
  const title = rec.product.name;
  await env.INVENTORY.put(`txn:${rec.buyer_id}:${ms}:${rec.id}_refund`, JSON.stringify({
    id: `${rec.id}_refund`, user_id: rec.buyer_id, type: 'refund', amount: rec.amount, currency: 'TKT',
    product_id: rec.product.id, developer_id: rec.seller_id, simulation_id: rec.sim_id,
    description: `Refund: ${title}`, timestamp: iso,
  }), ttl);
  if (rec.creator_amount > 0) {
    await env.INVENTORY.put(`txn:${rec.seller_id}:${ms}:${rec.id}_refund`, JSON.stringify({
      id: `${rec.id}_refund_sale`, user_id: rec.seller_id, type: 'dev_refund', amount: -rec.creator_amount,
      currency: 'TKT', product_id: rec.product.id, simulation_id: rec.sim_id, counterparty_id: rec.buyer_id,
      description: `Refund: ${title}`, timestamp: iso,
    }), ttl);
  }
  await recordPurchase(env, rec.buyer_id, {
    id: `${rec.id}_refund`, ts: iso, currency: 'TKT', units: -rec.amount, title: `Refund: ${title}`,
    icon: rec.product.icon, product_id: rec.product.id, ref: rec.id,
    attribution: {
      simulation_id: rec.sim_id, simulation_name: rec.sim_name,
      creator_id: rec.seller_id, creator_name: rec.seller_name,
    },
  });
}

/// What the game needs to grant a consumable: the Roblox `ProcessReceipt`
/// fields plus the product it names.
export function receiptView(rec) {
  return {
    object: 'receipt',
    purchase_id: rec.id,
    livemode: rec.livemode,
    sim_id: rec.sim_id,
    space: rec.space,
    buyer_id: rec.buyer_id,
    product: rec.product,
    amount: rec.amount,
    currency: 'TKT',
    created: rec.created,
  };
}

const WALLET_OPS = {
  async balance() {
    const meta = await this.storage.get('meta');
    return { ok: true, balance: meta.balance, account_id: meta.account_id };
  },

  /// Idempotent credit: a `ref` already seen returns the first result.
  /// `paid` marks Tickets bought with money (a Stripe checkout); anything
  /// else, such as a creator's share of a sale, is earned.
  async credit({ ref, amount, reason, paid }) {
    if (typeof ref !== 'string' || !ref || !isWhole(amount) || amount <= 0) return fail(400, 'invalid credit', 'invalid_request');
    const key = `ref:${ref}`;
    const result = await atomic(this, async () => {
      const prior = await this.storage.get(key);
      if (prior) return { ok: true, replay: true, ...prior };
      const meta = await this.storage.get('meta');
      meta.balance += amount;
      if (paid === true) meta.paid_balance = (meta.paid_balance || 0) + amount;
      meta.version += 1;
      const entry = {
        kind: 'credit', amount, paid: paid === true, balance: meta.balance,
        at: new Date().toISOString(), reason: cleanText(reason, 120),
      };
      await this.storage.put({ meta, [key]: entry });
      return { ok: true, ...entry };
    });
    if (!result.replay) await this.mirrorBalance();
    return result;
  },

  /// Idempotent debit, earned Tickets first (`paid_first` for a chargeback).
  /// Refused when the balance is short, unless `allow_negative` (a refund
  /// reversing a creator's earnings may leave a creator who has spent them
  /// owing Tickets).
  async debit({ ref, amount, reason, allow_negative, paid_first }) {
    if (typeof ref !== 'string' || !ref || !isWhole(amount) || amount <= 0) return fail(400, 'invalid debit', 'invalid_request');
    const key = `ref:${ref}`;
    const result = await atomic(this, async () => {
      const prior = await this.storage.get(key);
      if (prior) return { ok: true, replay: true, ...prior };
      const meta = await this.storage.get('meta');
      if (meta.balance < amount && !allow_negative) {
        return fail(402, 'Not enough Tickets', 'insufficient_tickets', { balance: meta.balance, price: amount });
      }
      const paidUsed = drawTickets(meta, amount, { paidFirst: paid_first === true });
      meta.version += 1;
      const entry = {
        kind: 'debit', amount, paid_used: paidUsed, balance: meta.balance,
        at: new Date().toISOString(), reason: cleanText(reason, 120),
      };
      await this.storage.put({ meta, [key]: entry });
      return { ok: true, ...entry };
    });
    if (result.ok && !result.replay) await this.mirrorBalance();
    return result;
  },

  /// The buyer's side of a purchase. The Worker has already checked the
  /// product, its price and the buyer's right to buy; this is where the money
  /// moves, once per idempotency key.
  async purchase({ purchase }) {
    const mode = modeOf(purchase.livemode);
    const idemKey = `idem:${mode}:${purchase.idempotency_key}`;
    const entKey = `ent:${mode}:${purchase.sim_id}:${purchase.product.id}`;
    const result = await atomic(this, async () => {
      const prior = await this.storage.get(idemKey);
      if (prior) {
        const record = await this.storage.get(`pur:${prior.purchase_id}`);
        const meta = await this.storage.get('meta');
        return { ok: true, replay: true, purchase: record, balance: meta.balance };
      }
      if (purchase.product.type === 'pass') {
        const owned = await this.storage.get(entKey);
        if (owned) return fail(409, 'This pass is already owned', 'already_owned', { purchase_id: owned.purchase_id });
      }
      const meta = await this.storage.get('meta');
      const writes = {};
      let paidAmount = 0;
      if (purchase.livemode) {
        if (meta.balance < purchase.amount) {
          return fail(402, 'Not enough Tickets', 'insufficient_tickets', { balance: meta.balance, price: purchase.amount });
        }
        paidAmount = drawTickets(meta, purchase.amount);
        meta.version += 1;
        writes.meta = meta;
      }
      const record = {
        ...purchase, paid_amount: paidAmount,
        status: 'succeeded', fulfilled: false, fulfilled_at: null, refunded_at: null,
      };
      writes[idemKey] = { purchase_id: record.id };
      writes[`pur:${record.id}`] = record;
      Object.assign(writes, outboxWrites(record.id, record.livemode ? ['hub', 'seller_credit', 'books'] : ['hub']));
      if (record.product.type === 'pass') {
        writes[entKey] = { purchase_id: record.id, product_id: record.product.id, number: record.product.number, created: record.created };
      } else {
        writes[`pend:${mode}:${record.sim_id}:${record.id}`] = receiptView(record);
      }
      // The debit, the record and the steps that finish it commit together.
      await this.storage.put(writes);
      return { ok: true, purchase: record, balance: meta.balance };
    });
    if (!result.ok) return result;
    if (result.purchase.livemode && !result.replay) await this.mirrorBalance();
    // Finish now if everything answers; the alarm retries whatever did not. A
    // replay drains too, in case the first attempt left a step behind.
    await this.drainOutbox([result.purchase.id]);
    return result;
  },

  async get_purchase({ purchase_id }) {
    const record = await this.storage.get(`pur:${purchase_id}`);
    return record ? { ok: true, purchase: record } : fail(404, 'No such purchase', 'resource_missing');
  },

  /// Consumables bought in `sim_id` that the game has not yet granted, oldest
  /// first: what a starting session hands to `ProcessReceipt` again.
  async pending({ sim_id, livemode }) {
    const entries = await this.storage.list({ prefix: `pend:${modeOf(livemode)}:${sim_id}:` });
    const receipts = [...entries.values()].sort((a, b) => a.created - b.created);
    return { ok: true, receipts };
  },

  async entitlements({ sim_id, livemode }) {
    const entries = await this.storage.list({ prefix: `ent:${modeOf(livemode)}:${sim_id}:` });
    return { ok: true, entitlements: [...entries.values()] };
  },

  /// The game granted the purchase. Idempotent.
  async fulfill({ purchase_id }) {
    const result = await atomic(this, async () => {
      const record = await this.storage.get(`pur:${purchase_id}`);
      if (!record) return fail(404, 'No such purchase', 'resource_missing');
      if (record.fulfilled) return { ok: true, replay: true, purchase: record };
      if (record.status !== 'succeeded') return fail(409, `A ${record.status} purchase cannot be fulfilled`, 'purchase_not_fulfillable');
      record.fulfilled = true;
      record.fulfilled_at = nowS();
      const existing = [...(await this.storage.list({ prefix: `out:${record.id}:` })).keys()];
      await this.storage.put({ [`pur:${record.id}`]: record, ...outboxWrites(record.id, ['hub_fulfilled'], existing) });
      await this.storage.delete(`pend:${modeOf(record.livemode)}:${record.sim_id}:${record.id}`);
      return { ok: true, purchase: record };
    });
    if (result.ok && !result.replay) await this.drainOutbox([purchase_id]);
    return result;
  },

  /// Refund a purchase to this (the buyer's) wallet. The creator's hub has
  /// already authorized it; the window is checked again here, where it cannot
  /// race a second refund.
  async refund({ purchase_id, now }) {
    const at = isWhole(now) ? now : Date.now();
    const result = await atomic(this, async () => {
      const record = await this.storage.get(`pur:${purchase_id}`);
      if (!record) return fail(404, 'No such purchase', 'resource_missing');
      if (record.status === 'refunded') return { ok: true, replay: true, purchase: record };
      if (at - record.created * 1000 > REFUND_WINDOW_MS) {
        return fail(409, 'The refund window for this purchase has closed', 'refund_window_closed');
      }
      const meta = await this.storage.get('meta');
      const writes = {};
      if (record.livemode) {
        // The Tickets go back as they came out: the paid part as paid.
        meta.balance += record.amount;
        meta.paid_balance = (meta.paid_balance || 0) + (record.paid_amount || 0);
        meta.version += 1;
        writes.meta = meta;
      }
      record.status = 'refunded';
      record.refunded_at = Math.floor(at / 1000);
      writes[`pur:${record.id}`] = record;
      const existing = [...(await this.storage.list({ prefix: `out:${record.id}:` })).keys()];
      Object.assign(writes, outboxWrites(
        record.id,
        record.livemode ? ['seller_debit', 'hub_refunded', 'refund_books'] : ['hub_refunded'],
        existing,
      ));
      await this.storage.put(writes);
      const mode = modeOf(record.livemode);
      await this.storage.delete([
        `pend:${mode}:${record.sim_id}:${record.id}`,
        ...(record.product.type === 'pass' ? [`ent:${mode}:${record.sim_id}:${record.product.id}`] : []),
      ]);
      return { ok: true, purchase: record, balance: meta.balance };
    });
    if (!result.ok || result.replay) return result;
    if (result.purchase.livemode) await this.mirrorBalance();
    await this.drainOutbox([purchase_id]);
    return result;
  },
};

// =============================================================================
// CommerceHub
// =============================================================================
//
// Storage keys:
//   meta                           { account_id }
//   prod:{id}                      a product
//   pnum:{sim}:{number}            product id by its number in the simulation
//   pseq:{sim}  pcount:{sim}       last number assigned, products in the sim
//   sale:{purchase_id}             the creator's record of a purchase
//   salet:{inverted ms}:{id}       sales, newest first
//   evt:{seq}  evtid:{event_id}  eseq
//                                  events in order, by id, and the last seq
//   we:{id}                        a webhook endpoint
//   wd:{due ms}:{event}:{endpoint} a webhook delivery due at that time
//   settle:{due ms}:{purchase}     a live sale's value score, due when its
//                                  refund window closes
//   lsec:{mode}                    the listen stream's signing secret
//   key:{id}                       an API key's record (its hash included, so
//                                  revoking can remove the KV index entry)
//   prune_at                       when events are next swept for retention

const pad12 = (n) => String(n).padStart(12, '0');
const pad13 = (n) => String(Math.trunc(n)).padStart(13, '0');

/// A product as the API returns it.
export function productView(p) {
  return {
    object: 'product', id: p.id, number: p.number, sim_id: p.sim_id, space: p.space,
    name: p.name, description: p.description, type: p.type, price: p.price, currency: 'TKT',
    icon: p.icon || null, active: p.active, metadata: p.metadata || {}, created: p.created, updated: p.updated,
  };
}

/// The compact product a purchase carries, frozen at the time of purchase.
export function productSummary(p) {
  return { id: p.id, number: p.number, name: p.name, type: p.type, price: p.price, icon: p.icon || null };
}

/// A purchase as the API returns it. The idempotency key stays private.
export function purchaseView(p) {
  const view = {
    object: 'purchase', id: p.id, livemode: p.livemode, status: p.status,
    sim_id: p.sim_id, sim_name: p.sim_name || '', space: p.space || null,
    buyer_id: p.buyer_id, seller_id: p.seller_id, product: p.product,
    amount: p.amount, currency: 'TKT', creator_amount: p.creator_amount, platform_amount: p.platform_amount,
    fulfilled: !!p.fulfilled, fulfilled_at: p.fulfilled_at || null, refunded_at: p.refunded_at || null,
    created: p.created,
  };
  if (p.synthetic) view.synthetic = true;
  return view;
}

function publicEvent(e) {
  const { seq, ...rest } = e;
  return rest;
}

/// Mode-less events (product changes) are visible in both modes.
const eventVisible = (e, livemode) => e.livemode === null || e.livemode === livemode;

function endpointWants(we, event) {
  if (we.status !== 'enabled') return false;
  if (event.livemode !== null && we.livemode !== event.livemode) return false;
  return we.enabled_events.includes('*') || we.enabled_events.includes(event.type);
}

function endpointView(we, { withSecret = false } = {}) {
  const view = {
    object: 'webhook_endpoint', id: we.id, url: we.url, enabled_events: we.enabled_events,
    livemode: we.livemode, description: we.description || '', status: we.status,
    created: we.created, last_delivery: we.last_delivery || null,
  };
  if (withSecret) view.secret = we.secret;
  return view;
}

function keyView(k) {
  return {
    object: 'api_key', id: k.id, name: k.name, mode: k.mode, key_type: k.key_type,
    key_prefix: k.key_prefix, last4: k.last4, created: k.created,
    last_used: k.last_used, usage_count: k.usage_count || 0,
  };
}

/// Page through `items` (already in list order) Stripe-style: `starting_after`
/// is the id of the last item of the previous page.
function paginate(items, { limit, starting_after }) {
  const n = Math.min(Math.max(Number(limit) || 10, 1), LIMITS.list_max);
  let start = 0;
  if (starting_after) {
    const i = items.findIndex((it) => it.id === starting_after);
    if (i >= 0) start = i + 1;
  }
  const data = items.slice(start, start + n);
  return { object: 'list', data, has_more: start + n < items.length };
}

export class CommerceHub {
  constructor(ctx, env) {
    this.ctx = ctx;
    this.env = env;
    this.storage = ctx.storage;
    this.accountId = null;
    this.waiters = new Set();
  }

  fetch(request) {
    return dispatch(this, HUB_OPS, request);
  }

  async open(accountId) {
    if (this.accountId) {
      if (accountId && accountId !== this.accountId) throw new Error('hub belongs to another account');
      return;
    }
    await this.ctx.blockConcurrencyWhile(async () => {
      if (this.accountId) return;
      let meta = await this.storage.get('meta');
      if (!meta) {
        if (!isAccountId(accountId)) throw new Error('hub opened without an account id');
        meta = { account_id: accountId, opened_at: new Date().toISOString() };
        await this.storage.put('meta', meta);
      } else if (accountId && meta.account_id !== accountId) {
        throw new Error('hub belongs to another account');
      }
      this.accountId = meta.account_id;
    });
  }

  /// Record an event, schedule its webhook deliveries, wake the listeners.
  /// Call it inside the caller's critical section: the sequence number is a
  /// read-modify-write.
  async publish(type, livemode, object, simId, extra = {}) {
    const seq = ((await this.storage.get('eseq')) || 0) + 1;
    const event = {
      object: 'event', id: newId('evt'), api_version: API_VERSION, type, livemode,
      created: nowS(), sim_id: simId || null, data: { object, ...extra },
    };
    const writes = { eseq: seq, [`evt:${pad12(seq)}`]: { ...event, seq }, [`evtid:${event.id}`]: seq };
    const now = Date.now();
    let deliveries = 0;
    for (const [, we] of await this.storage.list({ prefix: 'we:' })) {
      if (!endpointWants(we, event)) continue;
      writes[`wd:${pad13(now)}:${event.id}:${we.id}`] = { event_id: event.id, endpoint_id: we.id, attempt: 0 };
      deliveries += 1;
    }
    await this.storage.put(writes);
    if (deliveries > 0) await ensureAlarm(this.storage, now);
    if (!(await this.storage.get('prune_at'))) {
      const pruneAt = now + 86400e3;
      await this.storage.put('prune_at', pruneAt);
      await ensureAlarm(this.storage, pruneAt);
    }
    for (const wake of [...this.waiters]) wake();
    return event;
  }

  waitForEvent(ms) {
    return new Promise((resolve) => {
      let timer = null;
      const wake = () => {
        if (timer !== null) clearTimeout(timer);
        this.waiters.delete(wake);
        resolve();
      };
      timer = setTimeout(wake, ms);
      this.waiters.add(wake);
    });
  }

  async alarm() {
    await this.open();
    const now = Date.now();
    await this.deliverDue(now);
    await this.settleDue(now);
    await this.pruneEvents(now);
    const next = await this.nextDue();
    if (next !== null) await this.storage.setAlarm(Math.max(next, Date.now() + 1000));
  }

  async nextDue() {
    let next = null;
    for (const prefix of ['wd:', 'settle:']) {
      const first = await this.storage.list({ prefix, limit: 1 });
      for (const key of first.keys()) {
        const due = Number(key.split(':')[1]);
        next = next === null ? due : Math.min(next, due);
      }
    }
    const pruneAt = await this.storage.get('prune_at');
    if (pruneAt) next = next === null ? pruneAt : Math.min(next, pruneAt);
    return next;
  }

  /// Deliver webhooks that are due, 50 per alarm; a retry is rescheduled
  /// with the next delay until the schedule runs out.
  async deliverDue(now) {
    const due = await this.storage.list({ prefix: 'wd:', end: `wd:${pad13(now + 1)}`, limit: 50 });
    for (const [key, entry] of due) {
      await this.storage.delete(key);
      const we = await this.storage.get(`we:${entry.endpoint_id}`);
      const seq = await this.storage.get(`evtid:${entry.event_id}`);
      const event = seq ? await this.storage.get(`evt:${pad12(seq)}`) : null;
      if (!we || !event || we.status !== 'enabled') continue;
      const payload = JSON.stringify(publicEvent(event));
      let status = 0;
      let error = null;
      try {
        // The global fetch, never a binding's: see cleanWebhookUrl.
        const res = await fetch(we.url, {
          method: 'POST',
          headers: {
            'content-type': 'application/json',
            'user-agent': 'Eustress-Webhooks/1.0',
            'eustress-signature': await signatureHeader(we.secret, payload),
          },
          body: payload,
          redirect: 'manual',
          signal: AbortSignal.timeout(WEBHOOK_TIMEOUT_MS),
        });
        status = res.status;
      } catch (e) {
        error = String(e?.message || e);
      }
      const delivered = status >= 200 && status < 300;
      const attempt = entry.attempt + 1;
      await atomic(this, async () => {
        const fresh = await this.storage.get(`we:${we.id}`);
        if (!fresh) return; // deleted while the request was out
        fresh.last_delivery = { event_id: event.id, at: nowS(), status, ok: delivered, error, attempt };
        await this.storage.put(`we:${we.id}`, fresh);
        if (!delivered && entry.attempt < WEBHOOK_RETRY_DELAYS_MS.length) {
          const retryAt = Date.now() + WEBHOOK_RETRY_DELAYS_MS[entry.attempt];
          await this.storage.put(`wd:${pad13(retryAt)}:${event.id}:${we.id}`, { ...entry, attempt });
        }
      });
    }
  }

  /// File the value score of every live sale whose refund window has closed
  /// and which was not refunded in it: the creator's share of what the buyer
  /// paid for with money (see drawTickets). The score is filed under the
  /// purchase id, which makes filing it twice file one sale, so it is filed
  /// first and the sale marked after: a crash between the two files it again
  /// harmlessly. A score that could not be filed is tried again later.
  async settleDue(now) {
    const due = await this.storage.list({ prefix: 'settle:', end: `settle:${pad13(now + 1)}`, limit: 100 });
    for (const [key, entry] of due) {
      const sale = await this.storage.get(`sale:${entry.purchase_id}`);
      const scores = !!sale && sale.livemode && sale.status === 'succeeded' && !sale.value_score_credited;
      if (scores) {
        try {
          await creditValueScore(this.env, {
            creatorId: this.accountId, buyerId: sale.buyer_id, creatorTickets: entry.scoring_tickets, ref: sale.id,
          });
        } catch (e) {
          console.error(`hub ${this.accountId}: value score for ${entry.purchase_id} was not filed; retrying:`, e);
          await atomic(this, async () => {
            if (!(await this.storage.get(key))) return;
            await this.storage.delete(key);
            await this.storage.put(`settle:${pad13(Date.now() + SETTLE_RETRY_MS)}:${entry.purchase_id}`, entry);
          });
          continue;
        }
      }
      await atomic(this, async () => {
        await this.storage.delete(key);
        const fresh = await this.storage.get(`sale:${entry.purchase_id}`);
        if (scores && fresh && !fresh.value_score_credited) {
          fresh.value_score_credited = nowS();
          await this.storage.put(`sale:${fresh.id}`, fresh);
        }
      });
    }
  }

  /// Drop events older than the retention window, once a day.
  async pruneEvents(now) {
    const pruneAt = await this.storage.get('prune_at');
    if (!pruneAt || pruneAt > now) return;
    const cutoff = Math.floor((now - EVENT_RETENTION_MS) / 1000);
    const oldest = await this.storage.list({ prefix: 'evt:', limit: 500 });
    const doomed = [];
    for (const [key, event] of oldest) {
      if (event.created >= cutoff) break;
      doomed.push(key, `evtid:${event.id}`);
    }
    for (let i = 0; i < doomed.length; i += 128) await this.storage.delete(doomed.slice(i, i + 128));
    await this.storage.put('prune_at', now + 86400e3);
  }

  async listSales() {
    const index = await this.storage.list({ prefix: 'salet:' });
    const ids = [...index.keys()].map((k) => k.slice(k.lastIndexOf(':') + 1));
    const sales = [];
    for (let i = 0; i < ids.length; i += 128) {
      const batch = await this.storage.get(ids.slice(i, i + 128).map((id) => `sale:${id}`));
      for (const id of ids.slice(i, i + 128)) {
        const sale = batch.get(`sale:${id}`);
        if (sale) sales.push(sale);
      }
    }
    return sales;
  }
}

const HUB_OPS = {
  // ── Products ─────────────────────────────────────────────────────────────

  async create_product({ sim_id, fields }) {
    return atomic(this, async () => {
      const count = (await this.storage.get(`pcount:${sim_id}`)) || 0;
      if (count >= LIMITS.products_per_simulation) {
        return fail(400, `A simulation can have at most ${LIMITS.products_per_simulation} products`, 'too_many_products');
      }
      const number = ((await this.storage.get(`pseq:${sim_id}`)) || 0) + 1;
      const now = nowS();
      const product = { id: newId('prod'), number, sim_id, ...fields, created: now, updated: now };
      await this.storage.put({
        [`prod:${product.id}`]: product,
        [`pnum:${sim_id}:${number}`]: product.id,
        [`pseq:${sim_id}`]: number,
        [`pcount:${sim_id}`]: count + 1,
      });
      await this.publish('product.created', null, productView(product), sim_id);
      return { ok: true, product: productView(product) };
    });
  },

  async update_product({ id, patch }) {
    return atomic(this, async () => {
      const product = await this.storage.get(`prod:${id}`);
      if (!product) return fail(404, 'No such product', 'resource_missing');
      const previous = {};
      for (const [k, v] of Object.entries(patch)) {
        if (JSON.stringify(product[k]) !== JSON.stringify(v)) {
          previous[k] = product[k] === undefined ? null : product[k];
          product[k] = v;
        }
      }
      if (Object.keys(previous).length === 0) return { ok: true, product: productView(product), unchanged: true };
      product.updated = nowS();
      await this.storage.put(`prod:${id}`, product);
      const type = patch.active === false && previous.active === true ? 'product.archived' : 'product.updated';
      await this.publish(type, null, productView(product), product.sim_id, { previous_attributes: previous });
      return { ok: true, product: productView(product) };
    });
  },

  async get_product({ id, sim_id, number }) {
    let productId = id;
    if (!productId && sim_id && isWhole(number)) productId = await this.storage.get(`pnum:${sim_id}:${number}`);
    const product = productId ? await this.storage.get(`prod:${productId}`) : null;
    if (!product || (sim_id && product.sim_id !== sim_id)) return fail(404, 'No such product', 'resource_missing');
    return { ok: true, product: productView(product) };
  },

  async list_products({ sim_id, active, limit, starting_after, all }) {
    const products = [...(await this.storage.list({ prefix: 'prod:' })).values()]
      .filter((p) => (!sim_id || p.sim_id === sim_id) && (active === undefined || active === null || p.active === active))
      .sort((a, b) => b.created - a.created || b.number - a.number)
      .map(productView);
    if (all) return { ok: true, object: 'list', data: products, has_more: false };
    return { ok: true, ...paginate(products, { limit, starting_after }) };
  },

  async product_counts() {
    const counts = {};
    for (const [key, n] of await this.storage.list({ prefix: 'pcount:' })) counts[key.slice(7)] = n;
    return { ok: true, counts };
  },

  // ── Sales ────────────────────────────────────────────────────────────────

  /// The creator's copy of a purchase, sent by the buyer's wallet once the
  /// money has moved. Idempotent.
  async record_sale({ purchase }) {
    return atomic(this, async () => {
      const key = `sale:${purchase.id}`;
      if (await this.storage.get(key)) return { ok: true, replay: true };
      // The hub keeps its own lifecycle for the sale: it starts succeeded and
      // unfulfilled, and the wallet's later steps (fulfilled, refunded) move
      // it on, whatever the buyer's copy says by the time this step runs.
      const sale = { ...purchase, status: 'succeeded', fulfilled: false, fulfilled_at: null, refunded_at: null };
      delete sale.idempotency_key;
      const writes = { [key]: sale, [`salet:${inverted(sale.created * 1000)}:${sale.id}`]: 1 };
      // Only the part of the price the buyer paid for with money scores:
      // Tickets earned from sales and spent again carry none.
      const scoringTickets = Math.floor((sale.paid_amount || 0) * CREATOR_SHARE);
      const scores = sale.livemode && scoringTickets > 0;
      if (scores) {
        writes[`settle:${pad13(settleAt(sale))}:${sale.id}`] = { purchase_id: sale.id, scoring_tickets: scoringTickets };
      }
      await this.storage.put(writes);
      if (scores) await ensureAlarm(this.storage, settleAt(sale));
      await this.publish('purchase.succeeded', sale.livemode, purchaseView(sale), sale.sim_id);
      return { ok: true, sale: purchaseView(sale) };
    });
  },

  async sale_fulfilled({ purchase_id, fulfilled_at }) {
    return atomic(this, async () => {
      const sale = await this.storage.get(`sale:${purchase_id}`);
      if (!sale) return fail(404, 'No such purchase', 'resource_missing');
      if (sale.fulfilled) return { ok: true, replay: true, sale: purchaseView(sale) };
      sale.fulfilled = true;
      sale.fulfilled_at = isWhole(fulfilled_at) ? fulfilled_at : nowS();
      await this.storage.put(`sale:${sale.id}`, sale);
      await this.publish('purchase.fulfilled', sale.livemode, purchaseView(sale), sale.sim_id);
      return { ok: true, sale: purchaseView(sale) };
    });
  },

  async sale_refunded({ purchase_id, refunded_at }) {
    return atomic(this, async () => {
      const sale = await this.storage.get(`sale:${purchase_id}`);
      if (!sale) return fail(404, 'No such purchase', 'resource_missing');
      if (sale.status === 'refunded') return { ok: true, replay: true, sale: purchaseView(sale) };
      sale.status = 'refunded';
      sale.refunded_at = isWhole(refunded_at) ? refunded_at : nowS();
      await this.storage.put(`sale:${sale.id}`, sale);
      if (sale.livemode) await this.storage.delete(`settle:${pad13(settleAt(sale))}:${sale.id}`);
      await this.publish('purchase.refunded', sale.livemode, purchaseView(sale), sale.sim_id);
      return { ok: true, sale: purchaseView(sale) };
    });
  },

  async get_sale({ purchase_id }) {
    const sale = await this.storage.get(`sale:${purchase_id}`);
    return sale ? { ok: true, sale: purchaseView(sale), raw: sale } : fail(404, 'No such purchase', 'resource_missing');
  },

  async list_sales({ livemode, sim_id, limit, starting_after }) {
    const sales = (await this.listSales())
      .filter((s) => s.livemode === livemode && (!sim_id || s.sim_id === sim_id))
      .map(purchaseView);
    return { ok: true, ...paginate(sales, { limit, starting_after }) };
  },

  /// A purchase that did not go through, as an event for the creator's logs.
  async purchase_failed({ livemode, sim_id, product, buyer_id, code, message }) {
    const attempt = {
      object: 'purchase_attempt', livemode, sim_id, product, buyer_id,
      failure_code: code, failure_message: message, created: nowS(),
    };
    const event = await atomic(this, () => this.publish('purchase.failed', livemode, attempt, sim_id));
    return { ok: true, event: publicEvent(event) };
  },

  /// Test helper: fire `event` for `product` with a made-up buyer. No wallet
  /// is involved and nothing moves; the purchase is marked `synthetic`.
  async trigger({ event, sim_id, sim_name, product }) {
    if (event === 'purchase.failed') {
      return HUB_OPS.purchase_failed.call(this, {
        livemode: false, sim_id, product, buyer_id: `test_buyer_${randomToken(8)}`,
        code: 'insufficient_tickets', message: 'Not enough Tickets (test)',
      });
    }
    const creatorAmount = Math.floor(product.price * CREATOR_SHARE);
    const purchase = {
      object: 'purchase', id: newId('pur'), livemode: false, synthetic: true,
      sim_id, sim_name: sim_name || '', space: product.space || null,
      buyer_id: `test_buyer_${randomToken(8)}`, seller_id: this.accountId,
      product: productSummary(product), amount: product.price,
      creator_amount: creatorAmount, platform_amount: product.price - creatorAmount,
      created: nowS(), status: 'succeeded', fulfilled: false, fulfilled_at: null,
    };
    await HUB_OPS.record_sale.call(this, { purchase });
    if (event === 'purchase.fulfilled') await HUB_OPS.sale_fulfilled.call(this, { purchase_id: purchase.id });
    if (event === 'purchase.refunded') await HUB_OPS.sale_refunded.call(this, { purchase_id: purchase.id });
    const sale = await this.storage.get(`sale:${purchase.id}`);
    return { ok: true, purchase: purchaseView(sale) };
  },

  // ── Events ───────────────────────────────────────────────────────────────

  async list_events({ livemode, type, limit, starting_after }) {
    let end;
    if (starting_after) {
      const seq = await this.storage.get(`evtid:${starting_after}`);
      if (!seq) return fail(404, 'No such event', 'resource_missing');
      end = `evt:${pad12(seq)}`;
    }
    const n = Math.min(Math.max(Number(limit) || 10, 1), LIMITS.list_max);
    const data = [];
    let cursorEnd = end;
    let exhausted = false;
    // Filtered newest first; read in pages until `n` visible events are found.
    while (data.length < n + 1 && !exhausted) {
      const page = await this.storage.list({ prefix: 'evt:', reverse: true, limit: 200, ...(cursorEnd ? { end: cursorEnd } : {}) });
      if (page.size === 0) exhausted = true;
      for (const [key, e] of page) {
        cursorEnd = key;
        if (eventVisible(e, livemode) && (!type || e.type === type)) data.push(publicEvent(e));
        if (data.length === n + 1) break;
      }
      if (page.size < 200) exhausted = true;
    }
    return { ok: true, object: 'list', data: data.slice(0, n), has_more: data.length > n };
  },

  async get_event({ id }) {
    const seq = await this.storage.get(`evtid:${id}`);
    const event = seq ? await this.storage.get(`evt:${pad12(seq)}`) : null;
    return event ? { ok: true, event: publicEvent(event) } : fail(404, 'No such event', 'resource_missing');
  },

  /// Events after `after` (a cursor from an earlier call, or `now`), oldest
  /// first. With `wait`, holds the request up to that many seconds for the
  /// next event: `eustress commerce listen` long-polls this.
  async stream_events({ livemode, after, wait, types }) {
    const latest = (await this.storage.get('eseq')) || 0;
    let cursor = after === undefined || after === null || after === '' || after === 'now' ? latest : Number(after);
    if (!isWhole(cursor) || cursor < 0) return fail(400, 'Invalid cursor', 'invalid_request');
    const wanted = Array.isArray(types) && types.length > 0 ? new Set(types) : null;
    const deadline = Date.now() + Math.min(LIMITS.stream_wait_max_s, Math.max(0, Number(wait) || 0)) * 1000;
    while (true) {
      const page = await this.storage.list({ prefix: 'evt:', start: `evt:${pad12(cursor + 1)}`, limit: 100 });
      const data = [];
      for (const [, e] of page) {
        cursor = e.seq;
        if (eventVisible(e, livemode) && (!wanted || wanted.has(e.type))) data.push(publicEvent(e));
      }
      if (data.length > 0 || page.size === 100) return { ok: true, object: 'list', data, cursor: String(cursor), has_more: page.size === 100 };
      const remaining = deadline - Date.now();
      if (remaining <= 0) return { ok: true, object: 'list', data, cursor: String(cursor), has_more: false };
      await this.waitForEvent(remaining);
    }
  },

  /// Deliver an event to every matching webhook endpoint again.
  async resend_event({ id }) {
    const seq = await this.storage.get(`evtid:${id}`);
    const event = seq ? await this.storage.get(`evt:${pad12(seq)}`) : null;
    if (!event) return fail(404, 'No such event', 'resource_missing');
    const now = Date.now();
    const writes = {};
    for (const [, we] of await this.storage.list({ prefix: 'we:' })) {
      if (endpointWants(we, event)) writes[`wd:${pad13(now)}:${event.id}:${we.id}`] = { event_id: event.id, endpoint_id: we.id, attempt: 0 };
    }
    const deliveries = Object.keys(writes).length;
    if (deliveries > 0) {
      await this.storage.put(writes);
      await ensureAlarm(this.storage, now);
    }
    return { ok: true, event: publicEvent(event), deliveries };
  },

  // ── Webhook endpoints and the listen secret ─────────────────────────────

  async create_endpoint({ url, enabled_events, livemode, description }) {
    return atomic(this, async () => {
      const existing = await this.storage.list({ prefix: 'we:' });
      if (existing.size >= LIMITS.webhook_endpoints_per_account) {
        return fail(400, `At most ${LIMITS.webhook_endpoints_per_account} webhook endpoints`, 'too_many_endpoints');
      }
      const we = {
        id: newId('we'), url, enabled_events, livemode, description: description || '',
        status: 'enabled', secret: `whsec_${randomToken(32)}`, created: nowS(), last_delivery: null,
      };
      await this.storage.put(`we:${we.id}`, we);
      return { ok: true, endpoint: endpointView(we, { withSecret: true }) };
    });
  },

  async list_endpoints({ livemode }) {
    const data = [...(await this.storage.list({ prefix: 'we:' })).values()]
      .filter((we) => we.livemode === livemode)
      .sort((a, b) => b.created - a.created)
      .map((we) => endpointView(we));
    return { ok: true, object: 'list', data, has_more: false };
  },

  async delete_endpoint({ id }) {
    return atomic(this, async () => {
      const we = await this.storage.get(`we:${id}`);
      if (!we) return fail(404, 'No such webhook endpoint', 'resource_missing');
      const pending = [...(await this.storage.list({ prefix: 'wd:' })).keys()].filter((k) => k.endsWith(`:${id}`));
      for (let i = 0; i < pending.length; i += 127) await this.storage.delete(pending.slice(i, i + 127));
      await this.storage.delete(`we:${id}`);
      return { ok: true, deleted: true, id };
    });
  },

  /// The secret `listen` signs forwarded events with, one per mode. Stable,
  /// so a local server configured with it keeps verifying across restarts.
  async listen_secret({ livemode }) {
    return atomic(this, async () => {
      const key = `lsec:${modeOf(livemode)}`;
      let secret = await this.storage.get(key);
      if (!secret) {
        secret = `whsec_${randomToken(32)}`;
        await this.storage.put(key, secret);
      }
      return { ok: true, secret };
    });
  },

  // ── API keys ─────────────────────────────────────────────────────────────

  async create_key({ name, mode, key_type }) {
    const key = generateApiKey(mode);
    const hash = await sha256Hex(key);
    const created = await atomic(this, async () => {
      const existing = await this.storage.list({ prefix: 'key:' });
      if (existing.size >= LIMITS.keys_per_account) {
        return fail(400, `At most ${LIMITS.keys_per_account} API keys per account`, 'too_many_keys');
      }
      const record = {
        id: newId('key'), name, mode, key_type, hash,
        key_prefix: `${key.slice(0, 12)}...`, last4: key.slice(-4),
        created: nowS(), last_used: null, usage_count: 0,
      };
      await this.storage.put(`key:${record.id}`, record);
      return { ok: true, record };
    });
    if (!created.ok) return created;
    try {
      await this.env.USERS.put(apiKeyIndexKey(hash), JSON.stringify({
        account_id: this.accountId, key_id: created.record.id, mode, key_type,
      }));
    } catch (e) {
      await this.storage.delete(`key:${created.record.id}`);
      throw e;
    }
    return { ok: true, key, record: keyView(created.record) };
  },

  async list_keys() {
    const data = [...(await this.storage.list({ prefix: 'key:' })).values()]
      .sort((a, b) => b.created - a.created)
      .map(keyView);
    return { ok: true, object: 'list', data, has_more: false };
  },

  /// Revoke a key. The KV index entry goes first, so a failure leaves the key
  /// listed (and revocable again) rather than working but unlisted.
  async revoke_key({ id }) {
    const record = await this.storage.get(`key:${id}`);
    if (!record) return fail(404, 'No such API key', 'resource_missing');
    await this.env.USERS.delete(apiKeyIndexKey(record.hash));
    await this.storage.delete(`key:${id}`);
    return { ok: true, deleted: true, id };
  },

  /// A key was used: count it, and stamp the time at most once a minute.
  async touch_key({ id }) {
    return atomic(this, async () => {
      const record = await this.storage.get(`key:${id}`);
      if (!record) return fail(404, 'No such API key', 'resource_missing');
      record.usage_count = (record.usage_count || 0) + 1;
      const now = nowS();
      if (!record.last_used || now - record.last_used >= 60) record.last_used = now;
      await this.storage.put(`key:${id}`, record);
      return { ok: true };
    });
  },
};

// =============================================================================
// Wallet helpers for the Tickets routes
// =============================================================================
//
// Every write to a Ticket balance goes through the wallet, so the Stripe
// credit, chargebacks and commerce purchases all serialize on one object per
// account. `ref` makes each call idempotent: a ref already seen returns the
// first result and moves nothing.

export async function walletBalance(env, accountId) {
  const r = await callObject(walletStub(env, accountId), 'balance', { account_id: accountId });
  if (!r.ok) throw new Error(r.error || 'wallet balance failed');
  return r.balance;
}

/// `paid` marks Tickets bought with money: only those can score when spent
/// (see drawTickets). Everything else credited is earned.
export function walletCredit(env, accountId, { ref, amount, reason, paid = false }) {
  return callObject(walletStub(env, accountId), 'credit', { account_id: accountId, ref, amount, reason, paid });
}

/// `allow_negative` takes the debit even past zero, for a reversal the account
/// cannot refuse (a chargeback, a refunded sale's creator share). `paid_first`
/// takes paid Tickets before earned ones, for a chargeback.
export function walletDebit(env, accountId, { ref, amount, reason, allow_negative = false, paid_first = false }) {
  return callObject(walletStub(env, accountId), 'debit', {
    account_id: accountId, ref, amount, reason, allow_negative, paid_first,
  });
}

// =============================================================================
// HTTP
// =============================================================================

/// Who is calling, and in which mode. An API key fixes the mode by its prefix
/// and must be a commerce key. A signed-in session is in test mode unless it
/// sends `Eustress-Mode: live`: a caller that forgets the header reaches test
/// data and moves nothing. The Player sends it for a real purchase; Studio
/// never does, so testing a simulation never spends Tickets.
export async function authenticate(request, env, ctx, deps) {
  const m = /^Bearer\s+(\S+)\s*$/i.exec(request.headers.get('Authorization') || '');
  if (!m) return { error: 'Sign in, or send an API key as a Bearer token', status: 401, code: 'unauthenticated' };
  const token = m[1];
  const keyMode = keyModeOf(token);
  if (keyMode) {
    const raw = await env.USERS.get(apiKeyIndexKey(await sha256Hex(token)));
    if (!raw) return { error: 'Invalid API key', status: 401, code: 'invalid_api_key' };
    const rec = JSON.parse(raw);
    if (rec.mode !== keyMode || !isAccountId(rec.account_id)) return { error: 'Invalid API key', status: 401, code: 'invalid_api_key' };
    if (rec.key_type !== 'commerce') {
      return { error: `A ${rec.key_type} key cannot use the Commerce API; create a commerce key`, status: 403, code: 'wrong_key_type' };
    }
    const touch = callObject(hubStub(env, rec.account_id), 'touch_key', { account_id: rec.account_id, id: rec.key_id }).catch(() => {});
    if (ctx?.waitUntil) ctx.waitUntil(touch);
    return { account_id: rec.account_id, livemode: rec.mode === 'live', via: 'key', key_id: rec.key_id };
  }
  const userId = await deps.verifyAuth(request, env);
  if (!userId || !isAccountId(userId)) return { error: 'Invalid or expired session', status: 401, code: 'invalid_session' };
  const mode = (request.headers.get('Eustress-Mode') || '').trim().toLowerCase();
  if (mode && mode !== 'test' && mode !== 'live') return { error: 'Eustress-Mode must be test or live', status: 400, code: 'invalid_mode' };
  return { account_id: userId, livemode: mode === 'live', via: 'session' };
}

/// The product fields a create or update sets, cleaned. With `partial`, only
/// the fields present are returned; otherwise name and price are required.
export function productFields(body, { partial = false, previousMetadata = {} } = {}) {
  const out = {};
  if (!partial || body.name !== undefined) {
    const name = cleanText(body.name, LIMITS.product_name);
    if (!name) return { error: 'name is required (1 to 60 characters)', param: 'name' };
    out.name = name;
  }
  if (!partial || body.description !== undefined) {
    out.description = body.description === null ? '' : cleanText(body.description ?? '', LIMITS.product_description);
  }
  if (!partial) {
    const type = body.type === undefined ? 'consumable' : body.type;
    if (!PRODUCT_TYPES.includes(type)) return { error: `type must be one of ${PRODUCT_TYPES.join(', ')}`, param: 'type' };
    out.type = type;
  } else if (body.type !== undefined) {
    return { error: 'A product\'s type cannot change; create a new product instead', param: 'type' };
  }
  if (!partial || body.price !== undefined) {
    const price = cleanPrice(body.price);
    if (price === null) return { error: `price must be a whole number of Tickets from 1 to ${LIMITS.price_max}`, param: 'price' };
    out.price = price;
  }
  if (!partial || body.icon !== undefined) {
    const icon = body.icon ? cleanIcon(body.icon) : '';
    if (body.icon && !icon) return { error: 'icon must be served by Eustress (https://*.eustress.dev or /assets/...)', param: 'icon' };
    out.icon = icon || null;
  }
  if (!partial || body.active !== undefined) {
    if (body.active !== undefined && typeof body.active !== 'boolean') return { error: 'active must be true or false', param: 'active' };
    // New products start as drafts: testable in Studio, not yet for sale.
    out.active = body.active === true;
  }
  if (!partial || body.metadata !== undefined) {
    const md = cleanMetadata(body.metadata, partial ? previousMetadata : {});
    if (!md.ok) return { error: md.error, param: 'metadata' };
    out.metadata = md.metadata;
  }
  return { fields: out };
}

const listLimit = (url) => url.searchParams.get('limit');

/// Route `/api/commerce/*` and `/api/keys*`. Returns null for anything else.
///
/// `deps`: `verifyAuth`, `json`, `isListable` and `resolveAttribution` from
/// index.js and its modules, injected so this file has no copy of them.
export async function handleCommerceRoute(request, url, env, ctx, cors, deps) {
  const path = url.pathname;
  if (!path.startsWith('/api/commerce/') && path !== '/api/keys' && !path.startsWith('/api/keys/')) return null;

  const headers = { ...cors, 'Cache-Control': 'private, no-store' };
  const reply = (body, status = 200) => deps.json(body, status, headers);
  const error = (status, message, code, extra = {}) => reply({ error: message, code, ...extra }, status);
  const fromObject = (r, pick, status = 200) =>
    r.ok ? reply(pick(r), status) : error(r.status || 500, r.error || 'failed', r.code || 'internal_error', strip(r));
  // Products are shared by both modes. A draft is test data: test mode may
  // create and change it, and test-buy it in Studio. Putting a product on
  // sale, or changing one that is on sale, changes what players see and pay,
  // so it is a live change. The MCP server's test-mode product tools are
  // classified Write on the strength of this rule; relax it only after
  // reclassifying them (docs/monetization/COMMERCE.md, "MCP").
  const liveChangeRequired = () => error(403,
    'Putting a product on sale, or changing one that is on sale, is a live change: use a live key or Eustress-Mode: live',
    'livemode_required');

  if (!env.WALLETS || !env.COMMERCE_HUBS) return error(503, 'Commerce is not configured on this deployment', 'commerce_unavailable');

  const method = request.method;
  const caller = await authenticate(request, env, ctx, deps);

  // ── Public catalog: the one read that works signed out ──────────────────
  const catalogMatch = path.match(/^\/api\/commerce\/catalog\/([a-f0-9-]{8,64})$/);
  if (catalogMatch && method === 'GET') {
    const sim = await loadSimulation(env, catalogMatch[1]);
    if (!sim) return error(404, 'Simulation not found', 'resource_missing');
    const isAuthor = !caller.error && caller.account_id === sim.author_id;
    if (!isAuthor && !deps.isListable(sim)) return error(404, 'Simulation not found', 'resource_missing');
    const r = await callObject(hubStub(env, sim.author_id), 'list_products', { account_id: sim.author_id, sim_id: sim.id, all: true });
    if (!r.ok) return fromObject(r);
    const data = isAuthor ? r.data : r.data.filter((p) => p.active);
    const cacheHeaders = isAuthor ? headers : { ...cors, 'Cache-Control': 'public, max-age=30' };
    return deps.json({ object: 'list', sim_id: sim.id, data, has_more: false }, 200, cacheHeaders);
  }

  if (caller.error) return error(caller.status, caller.error, caller.code);
  const account = caller.account_id;
  const hub = hubStub(env, account);
  const wallet = walletStub(env, account);
  const call = (stub, op, args = {}) => callObject(stub, op, { account_id: account, ...args });

  // ── API keys (the web dashboard's list; the CLI mints its keys here) ────
  if (path === '/api/keys' || path.startsWith('/api/keys/')) {
    // A key must never mint more keys: only a signed-in session manages them.
    if (caller.via !== 'session') return error(403, 'API keys are managed from a signed-in session', 'session_required');
    if (path === '/api/keys' && method === 'GET') {
      const r = await call(hub, 'list_keys');
      return fromObject(r, (x) => ({
        keys: x.data.map((k) => ({
          id: k.id, name: k.name, key_prefix: k.key_prefix, key_type: k.key_type, mode: k.mode,
          created_at: new Date(k.created * 1000).toISOString(),
          last_used: k.last_used ? new Date(k.last_used * 1000).toISOString() : null,
          usage_count: k.usage_count,
        })),
      }));
    }
    if (path === '/api/keys' && method === 'POST') {
      const body = await readJson(request);
      if (!body) return error(400, 'Body must be JSON', 'invalid_json');
      const name = cleanText(body.name ?? '', LIMITS.key_name) || 'API key';
      const keyType = body.key_type ?? 'commerce';
      if (!KEY_TYPES.includes(keyType)) return error(400, `key_type must be one of ${KEY_TYPES.join(', ')}`, 'invalid_request', { param: 'key_type' });
      const mode = body.mode ?? 'test';
      if (mode !== 'test' && mode !== 'live') return error(400, 'mode must be test or live', 'invalid_request', { param: 'mode' });
      const r = await call(hub, 'create_key', { name, mode, key_type: keyType });
      return fromObject(r, (x) => ({
        id: x.record.id, name: x.record.name, key: x.key, key_type: x.record.key_type, mode: x.record.mode,
        key_prefix: x.record.key_prefix, created_at: new Date(x.record.created * 1000).toISOString(),
      }), 201);
    }
    const keyMatch = path.match(/^\/api\/keys\/(key_[A-Za-z0-9]{24})$/);
    if (keyMatch && method === 'DELETE') return fromObject(await call(hub, 'revoke_key', { id: keyMatch[1] }), (x) => ({ id: x.id, deleted: true }));
    return error(404, 'Not found', 'not_found');
  }

  const livemode = caller.livemode;

  // ── Account ──────────────────────────────────────────────────────────────
  if (path === '/api/commerce/account' && method === 'GET') {
    const ids = [];
    let cursor;
    do {
      const page = await env.SOCIAL.list({ prefix: `sim-author:${account}:`, cursor, limit: 1000 });
      for (const k of page.keys) ids.push(k.name.slice(`sim-author:${account}:`.length));
      cursor = page.list_complete ? undefined : page.cursor;
    } while (cursor);
    const counts = await call(hub, 'product_counts');
    const simulations = [];
    for (const id of ids.slice(0, 200)) {
      const sim = await loadSimulation(env, id);
      if (!sim || sim.author_id !== account) continue;
      const blocked = commerceBlockedReason(sim);
      simulations.push({
        id: sim.id, name: sim.name, spaces: [...publishedSpaces(sim)].sort(),
        listed: deps.isListable(sim), can_sell: !blocked, reason: blocked,
        products: counts.ok ? counts.counts[sim.id] || 0 : null,
        published_at: sim.published_at, updated_at: sim.updated_at,
      });
    }
    simulations.sort((a, b) => String(b.updated_at).localeCompare(String(a.updated_at)));
    return reply({ object: 'account', id: account, livemode, via: caller.via, key_id: caller.key_id || null, simulations });
  }

  // ── Products ─────────────────────────────────────────────────────────────
  if (path === '/api/commerce/products' && method === 'POST') {
    const body = await readJson(request);
    if (!body) return error(400, 'Body must be JSON', 'invalid_json');
    const sim = await loadSimulation(env, body.sim_id);
    if (!sim) return error(404, 'Simulation not found; publish the Universe first and use its id', 'simulation_not_found', { param: 'sim_id' });
    if (sim.author_id !== account) return error(403, 'Only the creator of this simulation can sell in it', 'not_simulation_owner');
    const blocked = commerceBlockedReason(sim);
    if (blocked) return error(409, blocked, 'simulation_not_sellable');
    const space = cleanSpaceName(body.space);
    if (!space) return error(400, 'space is required: the published Space the product is sold in', 'invalid_request', { param: 'space' });
    const spaceError = checkSpacePublished(sim, space);
    if (spaceError) return error(409, spaceError, 'space_not_published', { param: 'space', published_spaces: [...publishedSpaces(sim)].sort() });
    const cleaned = productFields(body);
    if (cleaned.error) return error(400, cleaned.error, 'invalid_request', { param: cleaned.param });
    if (cleaned.fields.active && !livemode) return liveChangeRequired();
    const r = await call(hub, 'create_product', { sim_id: sim.id, fields: { space, ...cleaned.fields } });
    return fromObject(r, (x) => x.product, 201);
  }

  if (path === '/api/commerce/products' && method === 'GET') {
    const active = url.searchParams.get('active');
    const r = await call(hub, 'list_products', {
      sim_id: url.searchParams.get('sim_id') || undefined,
      active: active === 'true' ? true : active === 'false' ? false : undefined,
      limit: listLimit(url), starting_after: url.searchParams.get('starting_after') || undefined,
    });
    return fromObject(r, strip);
  }

  const productMatch = path.match(/^\/api\/commerce\/products\/(prod_[A-Za-z0-9]{24})$/);
  if (productMatch) {
    const id = productMatch[1];
    if (method === 'GET') return fromObject(await call(hub, 'get_product', { id }), (x) => x.product);
    if (method === 'DELETE') {
      const current = await call(hub, 'get_product', { id });
      if (!current.ok) return fromObject(current);
      if (current.product.active && !livemode) return liveChangeRequired();
      return fromObject(await call(hub, 'update_product', { id, patch: { active: false } }), (x) => x.product);
    }
    if (method === 'POST') {
      const body = await readJson(request);
      if (!body) return error(400, 'Body must be JSON', 'invalid_json');
      const current = await call(hub, 'get_product', { id });
      if (!current.ok) return fromObject(current);
      const cleaned = productFields(body, { partial: true, previousMetadata: current.product.metadata });
      if (cleaned.error) return error(400, cleaned.error, 'invalid_request', { param: cleaned.param });
      if ((current.product.active || cleaned.fields.active) && !livemode) return liveChangeRequired();
      const patch = cleaned.fields;
      if (body.space !== undefined) {
        const space = cleanSpaceName(body.space);
        if (!space) return error(400, 'space must name a published Space', 'invalid_request', { param: 'space' });
        const sim = await loadSimulation(env, current.product.sim_id);
        const spaceError = sim ? checkSpacePublished(sim, space) : 'Simulation not found';
        if (spaceError) return error(409, spaceError, 'space_not_published', { param: 'space' });
        patch.space = space;
      }
      return fromObject(await call(hub, 'update_product', { id, patch }), (x) => x.product);
    }
  }

  // ── Purchases ────────────────────────────────────────────────────────────
  if (path === '/api/commerce/purchases' && method === 'POST') {
    return createPurchase(request, env, ctx, caller, deps, { reply, error, fromObject });
  }

  if (path === '/api/commerce/purchases' && method === 'GET') {
    const r = await call(hub, 'list_sales', {
      livemode, sim_id: url.searchParams.get('sim_id') || undefined,
      limit: listLimit(url), starting_after: url.searchParams.get('starting_after') || undefined,
    });
    return fromObject(r, strip);
  }

  const purchaseMatch = path.match(/^\/api\/commerce\/purchases\/(pur_[A-Za-z0-9]{24})(\/fulfill|\/refund)?$/);
  if (purchaseMatch) {
    const purchaseId = purchaseMatch[1];
    const action = purchaseMatch[2];
    if (!action && method === 'GET') {
      // Modes stay apart as in Stripe: a test key never reads a live purchase.
      const asSeller = await call(hub, 'get_sale', { purchase_id: purchaseId });
      if (asSeller.ok && asSeller.raw.livemode === livemode) return reply(asSeller.sale);
      const asBuyer = await call(wallet, 'get_purchase', { purchase_id: purchaseId });
      if (asBuyer.ok && asBuyer.purchase.livemode === livemode) return reply(purchaseView(asBuyer.purchase));
      return error(404, 'No such purchase', 'resource_missing');
    }
    if (action === '/fulfill' && method === 'POST') {
      const body = (await readJson(request)) || {};
      const sim = await loadSimulation(env, body.sim_id);
      if (!sim) return error(400, 'sim_id is required: the simulation the purchase was made in', 'invalid_request', { param: 'sim_id' });
      const seller = sim.author_id;
      const found = await callObject(hubStub(env, seller), 'get_sale', { account_id: seller, purchase_id: purchaseId });
      if (!found.ok) return error(404, 'No such purchase', 'resource_missing');
      const sale = found.raw;
      if (account !== sale.buyer_id && account !== seller) return error(404, 'No such purchase', 'resource_missing');
      if (sale.livemode !== livemode) return error(400, `This purchase was made in ${modeOf(sale.livemode)} mode`, 'mode_mismatch');
      if (sale.synthetic) {
        const r = await callObject(hubStub(env, seller), 'sale_fulfilled', { account_id: seller, purchase_id: purchaseId });
        return fromObject(r, (x) => x.sale);
      }
      const r = await callObject(walletStub(env, sale.buyer_id), 'fulfill', { account_id: sale.buyer_id, purchase_id: purchaseId });
      return fromObject(r, (x) => purchaseView(x.purchase));
    }
    if (action === '/refund' && method === 'POST') {
      const found = await call(hub, 'get_sale', { purchase_id: purchaseId });
      if (!found.ok) return error(404, 'No such purchase', 'resource_missing');
      const sale = found.raw;
      if (sale.livemode !== livemode) return error(400, `This purchase was made in ${modeOf(sale.livemode)} mode`, 'mode_mismatch');
      if (sale.status === 'refunded') return reply(found.sale);
      if (sale.synthetic) return fromObject(await call(hub, 'sale_refunded', { purchase_id: purchaseId }), (x) => x.sale);
      if (Date.now() - sale.created * 1000 > REFUND_WINDOW_MS) {
        return error(409, 'The refund window for this purchase has closed', 'refund_window_closed');
      }
      const r = await callObject(walletStub(env, sale.buyer_id), 'refund', { account_id: sale.buyer_id, purchase_id: purchaseId });
      return fromObject(r, (x) => purchaseView(x.purchase));
    }
  }

  // ── The buyer's own state ────────────────────────────────────────────────
  if (path === '/api/commerce/me/balance' && method === 'GET') {
    return fromObject(await call(wallet, 'balance'), (x) => ({ object: 'balance', tickets: x.balance, livemode }));
  }
  if ((path === '/api/commerce/me/pending' || path === '/api/commerce/me/entitlements') && method === 'GET') {
    const simId = url.searchParams.get('sim_id');
    if (!isSimulationId(simId)) return error(400, 'sim_id is required', 'invalid_request', { param: 'sim_id' });
    if (path.endsWith('/pending')) {
      return fromObject(await call(wallet, 'pending', { sim_id: simId, livemode }), (x) => ({ object: 'list', data: x.receipts, has_more: false }));
    }
    return fromObject(await call(wallet, 'entitlements', { sim_id: simId, livemode }), (x) => ({ object: 'list', data: x.entitlements, has_more: false }));
  }

  // ── Events ───────────────────────────────────────────────────────────────
  if (path === '/api/commerce/events' && method === 'GET') {
    const type = url.searchParams.get('type') || undefined;
    if (type && !EVENT_TYPES.includes(type)) return error(400, `Unknown event type ${type}`, 'invalid_request', { param: 'type' });
    const r = await call(hub, 'list_events', {
      livemode, type, limit: listLimit(url), starting_after: url.searchParams.get('starting_after') || undefined,
    });
    return fromObject(r, strip);
  }
  if (path === '/api/commerce/events/stream' && method === 'GET') {
    const types = (url.searchParams.get('types') || '').split(',').map((s) => s.trim()).filter(Boolean);
    const unknown = types.filter((t) => !EVENT_TYPES.includes(t));
    if (unknown.length) return error(400, `Unknown event type ${unknown[0]}`, 'invalid_request', { param: 'types' });
    const r = await call(hub, 'stream_events', {
      livemode, after: url.searchParams.get('after') || 'now',
      wait: Number(url.searchParams.get('wait') || 0), types,
    });
    return fromObject(r, strip);
  }
  const eventMatch = path.match(/^\/api\/commerce\/events\/(evt_[A-Za-z0-9]{24})(\/resend)?$/);
  if (eventMatch) {
    const id = eventMatch[1];
    const found = await call(hub, 'get_event', { id });
    if (!found.ok || !eventVisible(found.event, livemode)) return error(404, 'No such event', 'resource_missing');
    if (!eventMatch[2] && method === 'GET') return reply(found.event);
    if (eventMatch[2] && method === 'POST') return fromObject(await call(hub, 'resend_event', { id }), (x) => ({ event: x.event, deliveries: x.deliveries }));
  }

  // ── Webhook endpoints and listen ─────────────────────────────────────────
  if (path === '/api/commerce/webhook_endpoints' && method === 'GET') {
    return fromObject(await call(hub, 'list_endpoints', { livemode }), strip);
  }
  if (path === '/api/commerce/webhook_endpoints' && method === 'POST') {
    const body = await readJson(request);
    if (!body) return error(400, 'Body must be JSON', 'invalid_json');
    const target = cleanWebhookUrl(body.url);
    if (!target) {
      return error(400, 'url must be an https URL on a public host name, on the default port; for local development use `eustress commerce listen --forward-to`', 'invalid_request', { param: 'url' });
    }
    const events = Array.isArray(body.enabled_events) && body.enabled_events.length ? body.enabled_events : ['*'];
    const bad = events.find((t) => t !== '*' && !EVENT_TYPES.includes(t));
    if (bad) return error(400, `Unknown event type ${bad}`, 'invalid_request', { param: 'enabled_events' });
    const r = await call(hub, 'create_endpoint', {
      url: target, enabled_events: [...new Set(events)], livemode,
      description: cleanText(body.description ?? '', 200),
    });
    return fromObject(r, (x) => x.endpoint, 201);
  }
  const endpointMatch = path.match(/^\/api\/commerce\/webhook_endpoints\/(we_[A-Za-z0-9]{24})$/);
  if (endpointMatch && method === 'DELETE') {
    return fromObject(await call(hub, 'delete_endpoint', { id: endpointMatch[1] }), (x) => ({ id: x.id, deleted: true }));
  }
  if (path === '/api/commerce/listen/secret' && method === 'GET') {
    return fromObject(await call(hub, 'listen_secret', { livemode }), (x) => ({ secret: x.secret, livemode }));
  }

  // ── Test helpers ─────────────────────────────────────────────────────────
  if (path === '/api/commerce/test_helpers/trigger' && method === 'POST') {
    if (livemode) return error(403, 'Triggers only run in test mode', 'livemode_not_allowed');
    const body = await readJson(request);
    if (!body) return error(400, 'Body must be JSON', 'invalid_json');
    if (!TRIGGERS.includes(body.event)) {
      return error(400, `event must be one of ${TRIGGERS.join(', ')}`, 'invalid_request', { param: 'event' });
    }
    const sim = await loadSimulation(env, body.sim_id);
    if (!sim) return error(404, 'Simulation not found', 'simulation_not_found', { param: 'sim_id' });
    if (sim.author_id !== account) return error(403, 'Only the creator of this simulation can trigger its events', 'not_simulation_owner');
    const ref = productRef(body.product);
    if (!ref) return error(400, 'product must be a prod_ id or a product number', 'invalid_request', { param: 'product' });
    const found = await call(hub, 'get_product', { id: ref.id, sim_id: sim.id, number: ref.number });
    if (!found.ok) return fromObject(found);
    const r = await call(hub, 'trigger', { event: body.event, sim_id: sim.id, sim_name: sim.name, product: found.product });
    return fromObject(r, (x) => (x.purchase ? { triggered: body.event, purchase: x.purchase } : { triggered: body.event, event: x.event }));
  }

  return error(404, 'Not found', 'not_found');
}

/// Why `space` is not a Space this simulation published, or null when it is.
function checkSpacePublished(sim, space) {
  const spaces = publishedSpaces(sim);
  if (spaces.has(space)) return null;
  if (spaces.size === 0) {
    return 'This simulation has no recorded Spaces. Republish the Universe from Studio so its Spaces are recorded, then try again';
  }
  return `Space "${space}" is not published in this simulation. Published: ${[...spaces].sort().join(', ')}`;
}

/// A reply without the object-call envelope fields.
function strip(r) {
  const { ok, status, raw, ...rest } = r;
  return rest;
}

/// POST /api/commerce/purchases: a player buys a product inside a simulation.
async function createPurchase(request, env, ctx, caller, deps, { reply, error, fromObject }) {
  const body = await readJson(request);
  if (!body) return error(400, 'Body must be JSON', 'invalid_json');
  const idempotencyKey = cleanIdempotencyKey(body.idempotency_key);
  if (!idempotencyKey) {
    return error(400, 'idempotency_key is required (8 to 100 of A-Z a-z 0-9 _ . : -); reuse it to retry safely', 'invalid_request', { param: 'idempotency_key' });
  }
  const ref = productRef(body.product);
  if (!ref) return error(400, 'product must be a prod_ id or a product number', 'invalid_request', { param: 'product' });
  const sim = await loadSimulation(env, body.sim_id);
  if (!sim) return error(404, 'Simulation not found', 'simulation_not_found', { param: 'sim_id' });
  const seller = sim.author_id;
  const livemode = caller.livemode;

  if (livemode) {
    if (caller.via !== 'session') {
      return error(403, 'Live purchases are made by a signed-in player, not with an API key', 'live_purchase_requires_session');
    }
    if (caller.account_id === seller) {
      return error(403, 'Creators cannot buy their own products with real Tickets; test them in test mode', 'self_purchase');
    }
    if (!deps.isListable(sim)) return error(409, 'This simulation is not approved for sale yet', 'simulation_not_listed');
  } else if (caller.account_id !== seller) {
    return error(403, 'Only the simulation\'s creator can make test purchases; a player buying for real sends Eustress-Mode: live', 'test_purchase_not_owner');
  }
  const blocked = commerceBlockedReason(sim);
  if (blocked) return error(409, blocked, 'simulation_not_sellable');

  const found = await callObject(hubStub(env, seller), 'get_product', {
    account_id: seller, id: ref.id, sim_id: sim.id, number: ref.number,
  });
  if (!found.ok) return error(404, 'No such product in this simulation', 'product_not_found', { param: 'product' });
  const product = found.product;
  if (livemode && !product.active) return error(409, 'This product is not for sale', 'product_inactive');

  // The buyer agreed to the price they were shown. Live purchases must say
  // what that was; a test purchase may skip it.
  const expected = body.expected_price === undefined || body.expected_price === null ? null : cleanPrice(body.expected_price);
  if (livemode && expected === null) {
    return error(400, 'expected_price is required: the price shown to the player', 'invalid_request', { param: 'expected_price' });
  }
  if (expected !== null && expected !== product.price) {
    return error(409, 'The price changed; show the new price and ask again', 'price_changed', { price: product.price });
  }

  const attribution = await deps.resolveAttribution(env, { simulation_id: sim.id });
  const creatorAmount = Math.floor(product.price * CREATOR_SHARE);
  const purchase = {
    object: 'purchase',
    id: newId('pur'),
    livemode,
    idempotency_key: idempotencyKey,
    sim_id: sim.id,
    sim_name: attribution.simulation_name || cleanText(sim.name, 96),
    space: cleanSpaceName(body.space) || product.space || null,
    buyer_id: caller.account_id,
    seller_id: seller,
    seller_name: attribution.creator_name || '',
    product: productSummary(product),
    amount: product.price,
    currency: 'TKT',
    creator_amount: creatorAmount,
    platform_amount: product.price - creatorAmount,
    created: nowS(),
  };
  const result = await callObject(walletStub(env, caller.account_id), 'purchase', { account_id: caller.account_id, purchase });
  if (!result.ok) {
    if (result.code === 'insufficient_tickets' || result.code === 'already_owned') {
      const note = callObject(hubStub(env, seller), 'purchase_failed', {
        account_id: seller, livemode, sim_id: sim.id, product: productSummary(product),
        buyer_id: caller.account_id, code: result.code, message: result.error,
      }).catch(() => {});
      if (ctx?.waitUntil) ctx.waitUntil(note);
    }
    return fromObject(result);
  }
  return reply({
    purchase: purchaseView(result.purchase),
    balance: livemode ? result.balance : undefined,
    replayed: !!result.replay,
  }, result.replay ? 200 : 201);
}
