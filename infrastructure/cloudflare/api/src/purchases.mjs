// =============================================================================
// Purchases: an account's own receipts for Tickets and Bliss spend
// =============================================================================
//
// Every spend writes one receipt here, whichever currency it used, so a single
// read answers what an account bought, from whom, and in which simulation.
// The two spend paths keep their own books (Tickets in the USERS balance and
// the txn: log, Bliss in the append-only ledger). A receipt is the buyer's view
// of either one and carries what neither book records: a title, an icon, the
// simulation, and the creator.
//
// Receipts live in INVENTORY under
//
//     purchase:{buyerId}:{inverted ms}:{receipt id}
//
// with the display fields copied into KV metadata. KV returns metadata from
// list() without a get per key, so one list call reads a thousand receipts,
// and the inverted timestamp makes that list newest first. A Worker invocation
// may make 1,000 KV operations; a get per receipt would have run out of them
// at the thousandth purchase.
//
// Receipts are private. The only read is the signed-in owner's own, and it
// takes no account id, so there is nothing to point at someone else's.
// =============================================================================

export const PURCHASE_KEY_PREFIX = 'purchase:';

/// Inverting against this ceiling turns KV's ascending key order into newest
/// first. It is 13 digits of milliseconds, which lasts until the year 2286.
const TS_CEILING_MS = 9_999_999_999_999;

const LIST_PAGE = 1000;

/// 50 pages is 50,000 receipts per read. Past that the totals are marked
/// truncated rather than silently short.
const MAX_PAGES = 50;

/// KV rejects metadata over 1,024 bytes of serialized JSON.
const MAX_METADATA_BYTES = 1024;

/// Byte budgets for each stored field. They sum to well under the metadata
/// limit; `fitMetadata` covers the case that JSON escaping pushes it over.
export const LIMITS = {
  title: 120,
  product_id: 64,
  simulation_name: 96,
  creator_name: 64,
  icon: 300,
};

const encoder = new TextEncoder();
const byteLength = (s) => encoder.encode(s).length;

/// Trim to at most `maxBytes` of UTF-8 without splitting a code point.
function truncateBytes(s, maxBytes) {
  if (byteLength(s) <= maxBytes) return s;
  let out = '';
  let used = 0;
  for (const ch of s) {
    const n = byteLength(ch);
    if (used + n > maxBytes) break;
    out += ch;
    used += n;
  }
  return out;
}

/// Display text from a creator or client: control characters and bidi
/// overrides removed, whitespace folded, length capped in bytes.
///
/// Bidi overrides go because a title is shown in the buyer's own history, and
/// a U+202E can make "Refund" read as something it is not.
export function cleanText(value, maxBytes) {
  if (typeof value !== 'string') return '';
  const flat = value
    .replace(/[\u0000-\u001f\u007f-\u009f\u2028\u2029\u200e\u200f\u202a-\u202e\u2066-\u2069]/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
  return truncateBytes(flat, maxBytes).trim();
}

/// An icon is kept only when it is served by Eustress: an https URL on
/// eustress.dev or one of its subdomains, or a path under the site's own
/// /assets. The purchases page loads every icon in the buyer's browser, so a
/// URL on any other host would tell that host who opened their history, and
/// when.
export function cleanIcon(value) {
  if (typeof value !== 'string' || value.length === 0 || byteLength(value) > LIMITS.icon) return '';
  if (value.startsWith('/assets/')) {
    return /^\/assets\/[A-Za-z0-9/_.-]+$/.test(value) && !value.includes('..') && !value.includes('//')
      ? value
      : '';
  }
  let url;
  try {
    url = new URL(value);
  } catch {
    return '';
  }
  if (url.protocol !== 'https:' || url.username || url.password || url.port) return '';
  if (!/^(?:[a-z0-9-]+\.)*eustress\.dev$/.test(url.hostname)) return '';
  return byteLength(url.href) <= LIMITS.icon ? url.href : '';
}

/// A product id as the engine sends it: a number or a short slug.
export function cleanProductId(value) {
  const s = typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 ? String(value) : value;
  return typeof s === 'string' && /^[A-Za-z0-9][A-Za-z0-9_.:-]{0,63}$/.test(s) ? s : '';
}

/// Account ids are server-minted, but a receipt prefix is built from one, so
/// anything that could reach into another account's keys is refused.
export function isAccountId(value) {
  return typeof value === 'string' && /^[A-Za-z0-9_-]{1,128}$/.test(value);
}

function isSimulationId(value) {
  return typeof value === 'string' && /^[a-f0-9-]{8,64}$/.test(value);
}

/// Key for one receipt. Newer purchases sort first.
export function purchaseKey(buyerId, tsMs, id) {
  const inverted = String(TS_CEILING_MS - Math.trunc(tsMs)).padStart(13, '0');
  return `${PURCHASE_KEY_PREFIX}${buyerId}:${inverted}:${id}`;
}

// -----------------------------------------------------------------------------
// Attribution
// -----------------------------------------------------------------------------

/// Who a purchase is credited to, and which simulation it happened in.
///
/// The simulation record is the authority: when a purchase names a simulation,
/// its author is the creator. A caller-supplied creator is then only accepted
/// when it matches, so the receipt, the per-creator totals and the account
/// that is paid always agree. Without a simulation, the creator must be an
/// account that exists.
///
/// Resolved before any money moves, so a bad reference refuses the spend
/// instead of recording a purchase that points nowhere.
export async function resolveAttribution(env, { simulation_id, creator_id } = {}) {
  const hasSim = simulation_id !== undefined && simulation_id !== null && simulation_id !== '';
  const hasCreator = creator_id !== undefined && creator_id !== null && creator_id !== '';

  if (hasCreator && !isAccountId(creator_id)) return { error: 'Invalid creator id', status: 400 };

  if (hasSim) {
    if (!isSimulationId(simulation_id)) return { error: 'Invalid simulation_id', status: 400 };
    const raw = await env.SOCIAL.get(`sim:${simulation_id}`);
    if (!raw) return { error: 'Simulation not found', status: 404 };
    const sim = JSON.parse(raw);
    const author = typeof sim.author_id === 'string' && sim.author_id ? sim.author_id : null;
    if (hasCreator && creator_id !== author) {
      return { error: 'That creator did not publish this simulation', status: 400 };
    }
    // The current username rather than the one stamped at publish time, so a
    // renamed creator reads as who they are now.
    const authorRaw = author ? await env.USERS.get(`user:${author}`) : null;
    const authorName = authorRaw ? JSON.parse(authorRaw).username : sim.author_name;
    return {
      simulation_id,
      simulation_name: cleanText(sim.name, LIMITS.simulation_name),
      creator_id: author,
      creator_name: author ? cleanText(authorName || '', LIMITS.creator_name) : '',
      // Read from the simulation record, not taken from the caller.
      verified: true,
    };
  }

  if (hasCreator) {
    const raw = await env.USERS.get(`user:${creator_id}`);
    if (!raw) return { error: 'Creator not found', status: 404 };
    return {
      simulation_id: null,
      simulation_name: '',
      creator_id,
      creator_name: cleanText(JSON.parse(raw).username || '', LIMITS.creator_name),
      // Only that the account exists. Nothing server-side says this creator is
      // owed anything, so this is fit to LABEL a receipt and never to pay.
      verified: false,
    };
  }

  return { simulation_id: null, simulation_name: '', creator_id: null, creator_name: '', verified: false };
}

/// Who a Ticket sale may actually PAY, given `resolveAttribution`'s result.
///
/// There is no server-side product record, so a simulation's `author_id` is the
/// only thing that proves who a sale owes. A creator named without a simulation
/// is just the caller's claim, and paying it let any buyer route a sale's 70% to
/// an account of their choosing: Tickets that cannot be cashed out, turned into
/// developer earnings that the daily payout converts to real money.
///
/// Refused rather than quietly crediting no one, so a buyer is never charged for
/// a sale whose recipient they believed they were paying.
export function payableCreator(attribution) {
  if (!attribution || !attribution.creator_id) return { creator_id: null };
  if (!attribution.verified) {
    return { error: 'A creator can only be paid for a sale made inside a simulation', status: 400 };
  }
  return { creator_id: attribution.creator_id };
}

// -----------------------------------------------------------------------------
// Writing a receipt
// -----------------------------------------------------------------------------

/// Metadata keys are one or two letters because every receipt field competes
/// for the same 1,024 bytes.
///
///   ts  ISO time          c  'TKT' | 'BLS'     a  whole Tickets, or Bliss minor units
///   t   title             i  icon              p  product id
///   s   simulation id     sn simulation name
///   k   creator id        kn creator name
function compact(r) {
  const m = { v: 1, ts: r.ts, c: r.currency, a: r.units, t: r.title };
  if (r.icon) m.i = r.icon;
  if (r.product_id) m.p = r.product_id;
  if (r.simulation_id) {
    m.s = r.simulation_id;
    m.sn = r.simulation_name;
  }
  if (r.creator_id) {
    m.k = r.creator_id;
    m.kn = r.creator_name;
  }
  return m;
}

/// Metadata that fits KV's limit. Only a pathological title (every character
/// escaped by JSON) gets here; the icon is dropped first because the page
/// falls back to the simulation's thumbnail, then text fields are trimmed.
/// The full receipt is the KV value either way.
export function fitMetadata(meta) {
  const size = (m) => byteLength(JSON.stringify(m));
  if (size(meta) <= MAX_METADATA_BYTES) return meta;
  const m = { ...meta };
  delete m.i;
  for (const field of ['t', 'sn', 'kn', 'p']) {
    while (size(m) > MAX_METADATA_BYTES && typeof m[field] === 'string' && m[field].length > 0) {
      m[field] = truncateBytes(m[field], Math.max(0, byteLength(m[field]) - 16));
    }
  }
  return m;
}

/// Write the receipt for a spend that has already succeeded.
///
/// `units` is whole Tickets for 'TKT' and integer minor units for 'BLS', the
/// same integers each book stores, so nothing is rounded on the way in.
export async function recordPurchase(env, buyerId, { id, ts, currency, units, title, icon, product_id, attribution, ref }) {
  const receiptId = id || crypto.randomUUID();
  const stamp = ts || new Date().toISOString();
  const record = {
    id: receiptId,
    ts: stamp,
    currency,
    units,
    title: cleanText(title, LIMITS.title),
    icon: cleanIcon(icon),
    product_id: product_id || '',
    simulation_id: attribution?.simulation_id || null,
    simulation_name: attribution?.simulation_name || '',
    creator_id: attribution?.creator_id || null,
    creator_name: attribution?.creator_name || '',
    // The spend this receipt describes: the txn: id or the ledger entry id.
    ref: ref || null,
  };
  await env.INVENTORY.put(purchaseKey(buyerId, Date.parse(stamp), receiptId), JSON.stringify(record), {
    metadata: fitMetadata(compact(record)),
  });
  return record;
}

// -----------------------------------------------------------------------------
// Reading
// -----------------------------------------------------------------------------

function expand(name, m) {
  return {
    id: name.slice(name.lastIndexOf(':') + 1),
    ts: m.ts,
    currency: m.c,
    units: Math.trunc(Number(m.a) || 0),
    title: m.t || '',
    icon: m.i || '',
    product_id: m.p || '',
    simulation_id: m.s || null,
    simulation_name: m.sn || '',
    creator_id: m.k || null,
    creator_name: m.kn || '',
  };
}

/// Every receipt for one account, newest first.
export async function listReceipts(env, buyerId, { maxPages = MAX_PAGES } = {}) {
  const prefix = `${PURCHASE_KEY_PREFIX}${buyerId}:`;
  const items = [];
  let cursor;
  let pages = 0;
  let truncated = false;
  while (true) {
    const page = await env.INVENTORY.list({ prefix, limit: LIST_PAGE, cursor });
    pages += 1;
    for (const k of page.keys) {
      let m = k.metadata;
      if (!m) {
        // Every receipt is written with metadata. One without it is read in
        // full rather than dropped from the totals.
        const raw = await env.INVENTORY.get(k.name);
        if (!raw) continue;
        m = compact(JSON.parse(raw));
      }
      items.push(expand(k.name, m));
    }
    if (page.list_complete || !page.cursor) break;
    if (pages >= maxPages) {
      truncated = true;
      break;
    }
    cursor = page.cursor;
  }
  return { items, truncated };
}

function byTotals(a, b) {
  return (
    b.tickets - a.tickets ||
    b.bliss_minor - a.bliss_minor ||
    b.purchases - a.purchases ||
    (a.name || '').localeCompare(b.name || '')
  );
}

/// Totals, and spend grouped by creator and by simulation.
///
/// Tickets and Bliss are never added together: they are different currencies
/// with no fixed rate between them, so every total carries both.
export function aggregate(items) {
  const totals = { tickets: 0, bliss_minor: 0, purchases: items.length, creators: 0, simulations: 0 };
  const creators = new Map();
  const simulations = new Map();

  for (const it of items) {
    const tickets = it.currency === 'TKT' ? it.units : 0;
    const bliss = it.currency === 'BLS' ? it.units : 0;
    totals.tickets += tickets;
    totals.bliss_minor += bliss;

    // Items arrive newest first, so the first sighting of a group carries its
    // most recent name and time.
    const ck = it.creator_id || '';
    if (!creators.has(ck)) {
      creators.set(ck, {
        creator_id: it.creator_id,
        name: it.creator_name,
        tickets: 0,
        bliss_minor: 0,
        purchases: 0,
        last_at: it.ts,
      });
    }
    const c = creators.get(ck);
    c.tickets += tickets;
    c.bliss_minor += bliss;
    c.purchases += 1;

    // Purchases made outside any simulation share one bucket, which can hold
    // several creators, so that bucket names none.
    const sk = it.simulation_id || '';
    if (!simulations.has(sk)) {
      simulations.set(sk, {
        simulation_id: it.simulation_id,
        name: it.simulation_name,
        creator_id: sk ? it.creator_id : null,
        creator_name: sk ? it.creator_name : '',
        tickets: 0,
        bliss_minor: 0,
        purchases: 0,
        last_at: it.ts,
      });
    }
    const s = simulations.get(sk);
    s.tickets += tickets;
    s.bliss_minor += bliss;
    s.purchases += 1;
  }

  totals.creators = [...creators.keys()].filter(Boolean).length;
  totals.simulations = [...simulations.keys()].filter(Boolean).length;
  return {
    totals,
    by_creator: [...creators.values()].sort(byTotals),
    by_simulation: [...simulations.values()].sort(byTotals),
  };
}

// -----------------------------------------------------------------------------
// HTTP
// -----------------------------------------------------------------------------

/// GET /api/purchases          every receipt plus both groupings
/// GET /api/purchases/summary  the totals only, for the profile
///
/// `deps` carries the index.js helpers (verifyAuth, json) and `blissUnit`,
/// the minor units per BLS, so this module has no copy of either.
export async function handlePurchases(request, env, cors, deps, { summaryOnly = false } = {}) {
  const { verifyAuth, json, blissUnit } = deps;
  const userId = await verifyAuth(request, env);
  if (!userId || !isAccountId(userId)) return json({ error: 'Unauthorized' }, 401, cors);

  const { items, truncated } = await listReceipts(env, userId);
  const { totals, by_creator, by_simulation } = aggregate(items);
  const bliss = (minor) => minor / blissUnit;
  // A purchase history is one person's spending. It must never sit in a
  // shared cache.
  const headers = { ...cors, 'Cache-Control': 'private, no-store' };
  const unit = { bliss_minor_per_bls: blissUnit, bliss_decimals: 2 };
  const presentTotals = { ...totals, bliss: bliss(totals.bliss_minor) };

  if (summaryOnly) return json({ unit, totals: presentTotals, truncated }, 200, headers);

  return json({
    unit,
    totals: presentTotals,
    purchases: items.map((it) => ({
      id: it.id,
      ts: it.ts,
      currency: it.currency,
      amount: it.currency === 'BLS' ? bliss(it.units) : it.units,
      amount_minor: it.currency === 'BLS' ? it.units : undefined,
      title: it.title,
      icon: it.icon,
      product_id: it.product_id,
      simulation_id: it.simulation_id,
      simulation_name: it.simulation_name,
      creator_id: it.creator_id,
      creator_name: it.creator_name,
    })),
    by_creator: by_creator.map((c) => ({
      creator_id: c.creator_id,
      creator_name: c.name,
      tickets: c.tickets,
      bliss: bliss(c.bliss_minor),
      bliss_minor: c.bliss_minor,
      purchases: c.purchases,
      last_at: c.last_at,
    })),
    by_simulation: by_simulation.map((s) => ({
      simulation_id: s.simulation_id,
      simulation_name: s.name,
      creator_id: s.creator_id,
      creator_name: s.creator_name,
      tickets: s.tickets,
      bliss: bliss(s.bliss_minor),
      bliss_minor: s.bliss_minor,
      purchases: s.purchases,
      last_at: s.last_at,
    })),
    truncated,
  }, 200, headers);
}
