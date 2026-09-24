// =============================================================================
// Bliss: the BLS ledger, the emission schedule, and contribution scores
// =============================================================================
//
// Shared by the HTTP handlers in index.js, the nightly settlement that runs in
// the Treasury (treasury.mjs), and commerce's value score (commerce.mjs).
//
// ## The ledger
//
// BLS is held as integer minor units with two decimals: 1 BLS = 100 minor
// units, and every stored amount is a whole number of them. An append-only
// log of entries is the truth and a balance is derived from it, so any
// balance can be rebuilt, audited and disputed entry by entry. PAYOUTS keys:
//
//   entry:{userId}:{ts}:{id}  one entry, {amount_minor, kind, ref, ts}. The
//                             key's metadata holds the same fields, so a
//                             listing sums a balance without reading entries.
//   bal:{userId}              cache of the summed entries
//   cp:{userId}               checkpoint {balance_minor, through, v}: every
//                             entry at or before `through` is summed into it
//   spendref:{userId}:{ref}   the entry key a spend reference wrote
//
// Automated credits use deterministic ids (an emission credit is
// `{scoreDate}T00:00:00.000Z:dist`), so writing one twice writes it once.
//
// A checkpoint folds in only entries older than FOLD_HORIZON_MS. Some entries
// are dated in the past when written (an emission credit carries the day it
// pays for), and a checkpoint that reached past such an entry's key would
// skip that entry for good.
//
// ## Scores
//
// A day's BLS emission and its USD drip are both shared by the day's score.
// INVENTORY keys:
//
//   contrib:{day}:{userId}          effort score, written by cosign;
//                                   metadata {s: effort, v: value recorded
//                                   inside the record}
//   vscore:{day}:{creatorId}:{ref}  one sale's value score; metadata {t, b}
//   vref:{ref}                      the day a sale's value score is filed under
// =============================================================================

// -----------------------------------------------------------------------------
// Units and emission
// -----------------------------------------------------------------------------

/// Minor units per whole BLS: two decimal places.
export const BLISS_UNIT = 100;

export const BLISS_INITIAL_SUPPLY = 100_000_000;
export const BLISS_INITIAL_RATE = 0.05; // 5% of supply in year one
export const BLISS_HALVING_YEARS = 4;
export const BLISS_TAIL_RATE = 0.005; // 0.5% floor, forever

/// Score that earns a day's whole emission ceiling: 8 hours at the top weight
/// (8 x 60 minutes x 3.0 for Development).
///
/// A day mints `emission x min(1, total_score / FULL_DAY_SCORE)` and the rest
/// of the day's allowance is never created. Supply follows contribution
/// rather than the calendar, and one contributor alone on a quiet day earns
/// for the minutes they worked, not the whole day's emission. Contributors
/// split what is minted by share of score. The USD drip follows the same rule.
///
/// Raising it makes BLS harder to earn; lowering it makes a short day worth
/// more. The long-run supply ceiling does not depend on it.
export const FULL_DAY_SCORE = 1440;

/// Checkpoints fold in only entries older than this. The nightly settlement
/// refuses to credit a score date close to it (see treasury.mjs).
export const FOLD_HORIZON_MS = 7 * 86400 * 1000;

/// Annual emission rate a given number of whole years after genesis.
export function blissEmissionRate(yearsSinceGenesis) {
  const halvings = Math.floor(yearsSinceGenesis / BLISS_HALVING_YEARS);
  return Math.max(BLISS_INITIAL_RATE / Math.pow(2, halvings), BLISS_TAIL_RATE);
}

/// Whole years from `genesis` (an ISO date) to `atMs`; 0 before genesis.
export function yearsSince(genesis, atMs) {
  if (!genesis) return 0;
  return Math.max(0, Math.floor((atMs - Date.parse(genesis)) / (365 * 86400 * 1000)));
}

/// Total supply in minor units. Before the first minor-unit write it is read
/// from the legacy float key.
export async function readSupplyMinor(env) {
  const minor = parseInt((await env.PAYOUTS.get('bliss:supply_minor')) || '0', 10);
  if (minor) return minor;
  return toMinor(parseFloat((await env.PAYOUTS.get('bliss:current_supply')) || String(BLISS_INITIAL_SUPPLY)));
}

/// Lifetime BLS minted to contributors, in minor units, with the same legacy
/// fallback.
export async function readDistributedMinor(env) {
  const minor = parseInt((await env.PAYOUTS.get('bliss:distributed_minor')) || '0', 10);
  if (minor) return minor;
  return toMinor(parseFloat((await env.PAYOUTS.get('bliss:total_distributed')) || '0'));
}

/// Whole-BLS number to integer minor units. For emission math and legacy
/// float balances; user input is validated before it reaches here.
export function toMinor(bls) {
  return Math.round((Number(bls) || 0) * BLISS_UNIT);
}

/// Integer minor units to a whole-BLS number for JSON responses.
export function fromMinor(minor) {
  return (Number(minor) || 0) / BLISS_UNIT;
}

/// Display form, always two decimals.
export function formatBliss(minor) {
  return fromMinor(minor).toFixed(2);
}

// -----------------------------------------------------------------------------
// Ledger
// -----------------------------------------------------------------------------

const CHECKPOINT_VERSION = 2;

/// Length of the ISO timestamp inside an entry key (`YYYY-MM-DDTHH:MM:SS.sssZ`).
const TS_LEN = 24;

function entryMetadata(amount, kind, ref, ts) {
  return { a: amount, k: kind ?? null, r: ref === null || ref === undefined ? null : String(ref).slice(0, 200), t: ts };
}

/// Write the cache, and never fail the caller over it: the entries are the
/// truth, and the next reconcile or cache miss rebuilds the cache from them.
async function putCache(env, userId, minor) {
  try {
    await env.PAYOUTS.put(`bal:${userId}`, String(minor));
  } catch (e) {
    console.error(`bal:${userId} cache write failed; it is rebuilt from entries:`, e?.message || e);
    try {
      await env.PAYOUTS.delete(`bal:${userId}`);
    } catch {
      // A stale cache is corrected by the next reconcile.
    }
  }
}

/// Append a ledger entry. Returns true if it was written, false if the key
/// already existed. `id` must be stable for automated credits.
export async function ledgerAppend(env, userId, { amount_minor, kind, ref, ts, id }) {
  const amount = Math.trunc(Number(amount_minor) || 0);
  if (amount === 0) return false;
  const stamp = ts || new Date().toISOString();
  const key = `entry:${userId}:${stamp}:${id}`;
  if (await env.PAYOUTS.get(key)) return false;
  const entry = { amount_minor: amount, kind, ref: ref || null, ts: stamp };
  await env.PAYOUTS.put(key, JSON.stringify(entry), { metadata: entryMetadata(amount, kind, entry.ref, stamp) });
  const cached = await env.PAYOUTS.get(`bal:${userId}`);
  if (cached !== null && cached !== undefined) await putCache(env, userId, (parseInt(cached, 10) || 0) + amount);
  return true;
}

/// An entry as a listing returns it: from the key's metadata, or from its
/// value for an entry written without metadata. Null if it is gone.
export async function readEntry(env, key) {
  const m = key.metadata;
  if (m && Number.isSafeInteger(m.a) && typeof m.t === 'string') {
    return { amount_minor: m.a, kind: m.k ?? null, ref: m.r ?? null, ts: m.t, has_metadata: true };
  }
  const raw = await env.PAYOUTS.get(key.name);
  if (!raw) return null;
  const e = JSON.parse(raw);
  return {
    amount_minor: Math.trunc(Number(e.amount_minor) || 0),
    kind: e.kind ?? null,
    ref: e.ref ?? null,
    ts: e.ts,
    has_metadata: false,
  };
}

/// Give an entry written without metadata its metadata, so later listings
/// need not read it. The value is rewritten unchanged.
export async function backfillEntryMetadata(env, keyName, entry) {
  await env.PAYOUTS.put(
    keyName,
    JSON.stringify({ amount_minor: entry.amount_minor, kind: entry.kind, ref: entry.ref, ts: entry.ts }),
    { metadata: entryMetadata(entry.amount_minor, entry.kind, entry.ref, entry.ts) },
  );
}

/// Sum a user's entries: the checkpoint plus every entry after it.
///
/// Returns the balance and the checkpoint to store next, which folds in only
/// entries older than FOLD_HORIZON_MS. A stored checkpoint of an older
/// version, or one that reaches inside the horizon, is not trusted, and the
/// sum starts again from the first entry.
export async function ledgerDeriveMinor(env, userId, nowMs = Date.now()) {
  const prefix = `entry:${userId}:`;
  const horizon = prefix + new Date(nowMs - FOLD_HORIZON_MS).toISOString();
  const cpRaw = await env.PAYOUTS.get(`cp:${userId}`);
  let cp = null;
  try {
    cp = cpRaw ? JSON.parse(cpRaw) : null;
  } catch {
    cp = null;
  }
  if (!cp || cp.v !== CHECKPOINT_VERSION || typeof cp.through !== 'string' || cp.through >= horizon) {
    cp = { balance_minor: 0, through: '' };
  }
  const start = cp.through;
  let folded = Math.trunc(Number(cp.balance_minor) || 0);
  let through = cp.through;
  let recent = 0;
  let cursor;
  while (true) {
    const list = await env.PAYOUTS.list({ prefix, limit: 1000, cursor });
    for (const k of list.keys) {
      if (start && k.name <= start) continue;
      const e = await readEntry(env, k);
      if (!e) continue;
      if (k.name < horizon) {
        folded += e.amount_minor;
        through = k.name;
      } else {
        recent += e.amount_minor;
      }
    }
    if (list.list_complete || !list.cursor) break;
    cursor = list.cursor;
  }
  return { balance_minor: folded + recent, checkpoint: { balance_minor: folded, through, v: CHECKPOINT_VERSION } };
}

/// A user's balance from the cache, derived when the cache is absent. A legacy
/// float balance with no ledger history is migrated on first read, so a holder
/// who stopped contributing keeps it.
export async function ledgerBalanceMinor(env, userId) {
  const cached = await env.PAYOUTS.get(`bal:${userId}`);
  if (cached !== null && cached !== undefined) return parseInt(cached, 10) || 0;

  let { balance_minor } = await ledgerDeriveMinor(env, userId);
  if (balance_minor === 0) {
    const raw = await env.USERS.get(`user:${userId}`);
    if (raw) {
      const user = JSON.parse(raw);
      if (!user.ledger_migrated && (Number(user.bliss_balance) || 0) > 0) {
        await ledgerMigrateUser(env, userId, user);
        ({ balance_minor } = await ledgerDeriveMinor(env, userId));
      }
    }
  }
  await putCache(env, userId, balance_minor);
  return balance_minor;
}

/// Derive the balance again, then rewrite the cache and the checkpoint.
/// Corrects any drift in the cache. Run for every account the nightly
/// settlement credits.
export async function ledgerReconcile(env, userId) {
  const { balance_minor, checkpoint } = await ledgerDeriveMinor(env, userId);
  await env.PAYOUTS.put(`bal:${userId}`, String(balance_minor));
  await env.PAYOUTS.put(`cp:${userId}`, JSON.stringify(checkpoint));
  return balance_minor;
}

/// Fold a legacy float `user.bliss_balance` into an opening entry, once. The
/// opening entry's key is fixed per user, so a repeat writes nothing.
export async function ledgerMigrateUser(env, userId, user) {
  if (user.ledger_migrated) return;
  const legacy = Number(user.bliss_balance) || 0;
  if (legacy > 0) {
    await ledgerAppend(env, userId, {
      amount_minor: toMinor(legacy),
      kind: 'migration_opening',
      ref: 'legacy float balance',
      ts: '1970-01-01T00:00:00.000Z', // sorts first: it is the opening entry
      id: 'opening',
    });
  }
  user.ledger_migrated = true;
  await env.USERS.put(`user:${userId}`, JSON.stringify(user));
}

/// Spend BLS: append a negative entry and burn the amount, so emission stays
/// the only source of new BLS.
///
/// `purpose` is recorded as the entry's ref. A `ref` spends once: its marker
/// records the entry key before the entry is written, so a retry after a
/// failure in between completes that same entry instead of spending again.
///
/// Two spends racing on one account can both pass the balance check, because
/// KV has no compare-and-set. Spending grants nothing today, so the race can
/// only overdraw the spender's own balance; a sink that grants something must
/// serialize an account's spends first.
export async function ledgerSpend(env, userId, { amount_minor, purpose, ref }) {
  const amount = Math.trunc(Number(amount_minor) || 0);
  if (amount <= 0) return { ok: false, error: 'Amount must be positive' };
  if (!purpose) return { ok: false, error: 'purpose required' };

  const marker = ref ? `spendref:${userId}:${ref}` : null;
  let ts = new Date().toISOString();
  const id = ref ? `spend-${ref}` : `spend-${crypto.randomUUID()}`;
  let resume = false;
  if (marker) {
    const prior = await env.PAYOUTS.get(marker);
    if (prior) {
      if (await env.PAYOUTS.get(prior)) {
        return { ok: false, error: 'Duplicate spend reference', balance_minor: await ledgerBalanceMinor(env, userId) };
      }
      // The marker landed and its entry did not: write that entry now.
      const stamp = prior.slice(`entry:${userId}:`.length, `entry:${userId}:`.length + TS_LEN);
      if (!Number.isNaN(Date.parse(stamp))) {
        ts = stamp;
        resume = true;
      }
    }
  }

  const balance = await ledgerBalanceMinor(env, userId);
  if (balance < amount) {
    return { ok: false, error: 'Insufficient balance', balance_minor: balance, required_minor: amount };
  }

  if (marker && !resume) await env.PAYOUTS.put(marker, `entry:${userId}:${ts}:${id}`);
  const wrote = await ledgerAppend(env, userId, { amount_minor: -amount, kind: 'spend', ref: purpose, id, ts });
  if (!wrote) return { ok: false, error: 'Duplicate spend reference', balance_minor: balance };

  // Burned, not transferred.
  const burned = parseInt((await env.PAYOUTS.get('bliss:burned_minor')) || '0', 10);
  await env.PAYOUTS.put('bliss:burned_minor', String(burned + amount));

  return { ok: true, spent_minor: amount, balance_minor: balance - amount, entry_id: id, ts };
}

// -----------------------------------------------------------------------------
// Scores
// -----------------------------------------------------------------------------

/// Contribution score a creator earns per Ticket of their share of a sale.
///
/// Effort score is self-reported minutes and is capped per day because it
/// cannot be verified. Value score comes from purchases that happened, so a
/// creator whose work people pay for can out-earn one who only logs hours.
/// At 0.5, 1,000 Tickets of a creator's share (about 1,430 Tickets of sales,
/// roughly $15) are worth 500 score, about 2.8 hours of Development.
export const VALUE_SCORE_PER_TICKET = 0.5;

/// Value score is paid from the shared treasury drip, so buying it can pay:
/// an account that buys from a friendly creator claims a share of a pool that
/// everyone else's sales filled. Three rules bound that. Only Tickets bought
/// with money score (commerce passes the paid part of a sale), a sale scores
/// only after its refund window closes, and these caps apply when a day's
/// sales are summed:
///
/// Of one buyer's purchases from one creator in a day, at most this many of
/// the creator's Tickets score.
export const VALUE_CAP_TICKETS_PER_BUYER = 1000;

/// A creator's value score for one day is at most this, three times the
/// daily ceiling on effort score.
export const VALUE_CAP_SCORE_PER_CREATOR = 9600;

const SALE_REF = /^[A-Za-z0-9_.-]{1,100}$/;

/// File a sale's value score: `creatorTickets` of the creator's share,
/// counted under today's date, capped later when the day is summed.
///
/// Idempotent per `ref` (the purchase id): a retry files nothing new, even on
/// a later day, because the first attempt records which day it used. A buyer
/// never scores for their own account. Returns the score filed before caps,
/// or 0 when nothing was filed.
export async function creditValueScore(env, { creatorId, buyerId, creatorTickets, ref } = {}, now = new Date()) {
  const tickets = Math.trunc(Number(creatorTickets) || 0);
  if (!creatorId || !buyerId || tickets <= 0 || typeof ref !== 'string' || !SALE_REF.test(ref)) return 0;
  if (creatorId === buyerId) return 0;
  const refKey = `vref:${ref}`;
  let day = await env.INVENTORY.get(refKey);
  if (!/^\d{4}-\d{2}-\d{2}$/.test(day || '')) {
    day = now.toISOString().slice(0, 10);
    await env.INVENTORY.put(refKey, day, { expirationTtl: 86400 * 90 });
  }
  await env.INVENTORY.put(
    `vscore:${day}:${creatorId}:${ref}`,
    JSON.stringify({ tickets, buyer: buyerId, at: now.toISOString() }),
    { expirationTtl: 86400 * 90, metadata: { t: tickets, b: buyerId } },
  );
  return tickets * VALUE_SCORE_PER_TICKET;
}

/// A creator's value score from their sales for one day, `sales` being
/// [{buyer, tickets}], with both caps applied.
export function cappedValueScore(sales) {
  const byBuyer = new Map();
  for (const s of sales) byBuyer.set(s.buyer, (byBuyer.get(s.buyer) || 0) + s.tickets);
  let tickets = 0;
  for (const t of byBuyer.values()) tickets += Math.min(t, VALUE_CAP_TICKETS_PER_BUYER);
  return Math.min(tickets * VALUE_SCORE_PER_TICKET, VALUE_CAP_SCORE_PER_CREATOR);
}

/// Creator and ref from a `vscore:{day}:{creator}:{ref}` key, given the
/// `vscore:{day}:` prefix. Null for a malformed key.
export function parseSaleKey(name, prefix) {
  const rest = name.slice(prefix.length);
  const i = rest.lastIndexOf(':');
  if (i <= 0) return null;
  return { creator: rest.slice(0, i), ref: rest.slice(i + 1) };
}

/// One filed sale from a listing key: {buyer, tickets}, or null.
export async function readSale(env, key) {
  const m = key.metadata;
  if (m && Number.isSafeInteger(m.t) && typeof m.b === 'string') return { buyer: m.b, tickets: m.t };
  const raw = await env.INVENTORY.get(key.name);
  if (!raw) return null;
  const v = JSON.parse(raw);
  const tickets = Math.trunc(Number(v.tickets) || 0);
  return typeof v.buyer === 'string' && tickets > 0 ? { buyer: v.buyer, tickets } : null;
}

/// A creator's capped value score for `day`. For the live projection; the
/// settlement sums every creator's in one pass.
export async function valueScoreForDay(env, day, creatorId, { maxPages = 5 } = {}) {
  const prefix = `vscore:${day}:${creatorId}:`;
  const sales = [];
  let cursor;
  for (let page = 0; page < maxPages; page++) {
    const list = await env.INVENTORY.list({ prefix, limit: 1000, cursor });
    for (const k of list.keys) {
      const sale = await readSale(env, k);
      if (sale) sales.push(sale);
    }
    if (list.list_complete || !list.cursor) break;
    cursor = list.cursor;
  }
  return cappedValueScore(sales);
}

/// Effort score from a `contrib:{day}:{user}` listing key, and any value
/// score recorded inside the record itself. Null if the record is gone.
export async function readContrib(env, key) {
  const m = key.metadata;
  if (m && Number.isFinite(m.s)) return { effort: Math.max(0, m.s), recorded_value: Math.max(0, Number(m.v) || 0) };
  const raw = await env.INVENTORY.get(key.name);
  if (!raw) return null;
  const rec = JSON.parse(raw);
  return {
    effort: Math.max(0, Number(rec.total_score) || 0),
    recorded_value: Math.max(0, Number(rec.value_score) || 0),
  };
}

/// What `score` earns if the day's network total ends at `dayTotal`, by the
/// rule the settlement applies: the day's emission ceiling times
/// min(1, total / FULL_DAY_SCORE), shared by score. In minor units.
export function projectEmissionMinor({ emissionBls, score, dayTotal }) {
  const total = Math.max(Number(dayTotal) || 0, score);
  if (!(score > 0) || !(total > 0) || !(emissionBls > 0)) return 0;
  const utilization = Math.min(1, total / FULL_DAY_SCORE);
  return Math.floor(emissionBls * BLISS_UNIT * utilization * (score / total));
}
