// =============================================================================
// Eustress API - Offering and subscriptions (how a person subscribes for units)
// =============================================================================
// The steps between "an offering is qualified" and "a holder is on the ledger":
// the offering's terms, a person's subscription, their signature, the record that
// their money arrived, and the acceptance that issues their units. See
// docs/finance/REGULATION_A_TIER_2.md, Stage 5 and Stage 8.
//
// NO MONEY MOVES THROUGH THIS CODE. A subscriber pays by bank transfer, outside
// the site, quoting a payment reference. An administrator records that the funds
// arrived and accepts the subscription. Nothing here charges a card.
//
// NOTHING IS OFFERED UNTIL AN ADMINISTRATOR SAYS SO. With no offering on file, or
// one still a draft, every public route answers that there is no offering. An
// offering opens only with the SEC file number and the date of its qualification;
// the code cannot check either, so the person who opens it must.
//
// The rules, in order:
//   - An offering is a draft, then (optionally) testing the waters, then open.
//     Testing takes indications of interest only: no money, no commitment.
//   - A subscriber is a signed-in account with a verified identity, a natural
//     person, in the United States, in a state the offering names.
//   - A subscriber who is not accredited may commit at most 10% of the greater of
//     their annual income or net worth across the offering. They state both
//     figures; the offering relies on the statement.
//   - A subscription is prepared (anything may prepare one: the site, the API, a
//     command line, an agent), and then signed by the person on a fresh session:
//     their typed legal name, the hash of the circular and of the agreement they
//     were shown, and five statements, each true.
//   - Signing reserves the units. No more are reserved or sold than the offering
//     has.
//   - Acceptance needs recorded funds of the exact amount, an open offering, and a
//     signature on the circular now in force. It records the holder and issues the
//     units in the same write, so a subscription is never accepted without its
//     units or issued twice.
//
// These operations run inside the HolderLedger object (src/holder_ledger.mjs),
// which is why this file imports nothing from it:
//
//   o:{offering_id}            an offering
//   s:{subscription_id}        a subscription
//   sh:{holder_id}:{sub_id}    1: the subscriptions of one account
//   i:{offering_id}:{holder_id} an indication of interest
//
// Routes:
//   GET    /api/offering                       the offering now testing or open, or none
//   GET    /api/offering/interest              the caller's indication of interest
//   PUT    /api/offering/interest              {amount_cents}   (testing only; binds nobody)
//   DELETE /api/offering/interest
//   GET    /api/subscriptions                  the caller's subscriptions
//   POST   /api/subscriptions                  prepare one
//   POST   /api/subscriptions/{id}/sign        the person signs (fresh session)
//   POST   /api/subscriptions/{id}/cancel      before acceptance
// Admin (/api/admin/*):
//   GET    /api/admin/offerings                every offering, with its counts checked
//   PUT    /api/admin/offerings                create or change one
//   POST   /api/admin/offerings/status         {offering_id, status, sec_file_number?, qualified_on?}
//   GET    /api/admin/subscriptions            ?status=&offering_id=
//   POST   /api/admin/subscriptions/expire     {older_than_days}
//   POST   /api/admin/subscriptions/{id}/funds     {amount_cents, bank_ref}
//   POST   /api/admin/subscriptions/{id}/accept
//   POST   /api/admin/subscriptions/{id}/reject    {reason}
//   POST   /api/admin/subscriptions/{id}/refunded  {bank_ref}
//
// Table of Contents:
// 1. Rules
// 2. Offering operations
// 3. Subscription operations
// 4. Routes
// =============================================================================

// -----------------------------------------------------------------------------
// 1. Rules
// -----------------------------------------------------------------------------

export const OFFERING_STATUSES = ['draft', 'testing', 'open', 'paused', 'closed'];
/// Which status may follow which. Closed is final.
const NEXT = {
  draft: ['testing', 'open', 'closed'],
  testing: ['draft', 'open', 'closed'],
  open: ['paused', 'closed'],
  paused: ['open', 'closed'],
  closed: [],
};
/// The statuses in which an offering is the live one. At most one offering is live.
const LIVE = ['testing', 'open', 'paused'];
/// The most a Tier 2 offering may raise in twelve months, in cents.
export const TIER2_CAP_CENTS = 7_500_000_000;
/// Subscriptions whose units are held back from other subscribers, and those that count toward a subscriber's limit.
const RESERVING = ['awaiting_funds', 'funds_received'];
const COMMITTED = ['awaiting_funds', 'funds_received', 'accepted'];
/// The five statements a signer makes. Every one must be true.
export const ATTESTATIONS = ['read_circular', 'agree_subscription', 'own_account', 'information_true', 'electronic_signature'];
const MAX_DRAFTS = 20;
const MAX_UNITS = 1_000_000_000_000;

/// The legend every testing-the-waters communication carries (Rule 255). Counsel confirms the wording before it is shown.
export const TESTING_LEGEND = 'No money or other consideration is being solicited, and if sent in response, it will not be accepted. '
  + 'No offer to buy the securities can be accepted and no part of the purchase price can be received until the offering statement is qualified, '
  + 'and any such offer may be withdrawn or revoked, without obligation or commitment of any kind, at any time before notice of its acceptance given after the qualification date. '
  + 'An indication of interest involves no obligation or commitment of any kind.';

const US_STATES = new Set(('AL AK AZ AR CA CO CT DE DC FL GA HI ID IL IN IA KS KY LA ME MD MA MI MN MS MO MT NE NV NH NJ NM NY NC ND OH OK OR PA RI SC SD TN TX UT VT VA WA WV WI WY').split(' '));
const OFFERING_ID = /^[a-z0-9-]{1,40}$/;
const SUB_ID = /^[a-f0-9-]{36}$/;
const HOLDER_ID = /^[A-Za-z0-9_-]{1,64}$/;
const SHA = /^[a-f0-9]{64}$/;
const SEC_FILE = /^024-\d{5}$/;
const DAY = /^\d{4}-\d{2}-\d{2}$/;
const PAGE = 1000;

const fail = (status, error, code) => ({ ok: false, status, error, code });
const text = (v, max) => (typeof v === 'string' ? v.trim().slice(0, max) : '');
const whole = (n, min = 0) => Number.isSafeInteger(n) && n >= min;
const sameName = (a, b) => a.trim().replace(/\s+/g, ' ').toLowerCase() === b.trim().replace(/\s+/g, ' ').toLowerCase();

/// The most a subscriber who is not accredited may commit to one offering: 10% of the greater of annual income or net worth.
export function investmentLimitCents(annualIncomeCents, netWorthCents) {
  return Math.floor(Math.max(annualIncomeCents, netWorthCents) / 10);
}

/// A linked document: where it is and the SHA-256 of its bytes, so a signature names the exact version shown.
function readDoc(v, name) {
  if (!v || typeof v !== 'object') return { problem: `${name} is { url, sha256 }` };
  const url = text(v.url, 500);
  if (!/^https:\/\/\S+$/.test(url)) return { problem: `${name}.url is an https address` };
  if (typeof v.sha256 !== 'string' || !SHA.test(v.sha256)) return { problem: `${name}.sha256 is 64 lowercase hex characters` };
  return { doc: { url, sha256: v.sha256 } };
}

/// What the public may see of an offering.
function publicView(o) {
  const open = o.status === 'open' || o.status === 'paused';
  return {
    offering_id: o.offering_id, status: o.status, title: o.title, price_cents: o.price_cents, min_units: o.min_units,
    states: o.states, circular: o.circular, agreement: o.agreement,
    sec_file_number: open ? o.sec_file_number : null, qualified_on: open ? o.qualified_on : null,
    units_available: open ? o.max_units - o.reserved_units - o.sold_units : null,
    legend: o.status === 'testing' ? TESTING_LEGEND : null,
  };
}

/// What a subscriber may see of their own subscription: everything but which administrator acted.
function ownView(s, offering) {
  const { funds, accepted, rejected, refunded, ...rest } = s;
  return {
    ...rest,
    funds: funds ? { received_at: funds.received_at } : null,
    accepted: accepted ? { at: accepted.at, entry_seq: accepted.entry_seq } : null,
    rejected: rejected ? { at: rejected.at, reason: rejected.reason } : null,
    refunded: refunded ? { at: refunded.at } : null,
    payment_instructions: s.status === 'awaiting_funds' && offering ? offering.payment_instructions : null,
  };
}

async function liveOffering(storage) {
  const all = await storage.list({ prefix: 'o:' });
  for (const o of all.values()) if (LIVE.includes(o.status)) return o;
  return null;
}

// -----------------------------------------------------------------------------
// 2. Offering operations
// -----------------------------------------------------------------------------

const OFFERING_OPS = {
  /// Create an offering as a draft, or change one. Price and size are fixed while it is open: pause it first.
  async offering_put({ offering_id, title, price_cents, max_units, min_units = 1, states, circular, agreement, payment_instructions, by }) {
    if (typeof offering_id !== 'string' || !OFFERING_ID.test(offering_id)) return fail(400, 'offering_id is 1 to 40 of lowercase letters, digits and -', 'invalid_request');
    const name = text(title, 200);
    if (name.length < 2) return fail(400, 'title is required', 'invalid_request');
    if (!whole(price_cents, 1)) return fail(400, 'price_cents is whole cents per unit, at least 1', 'invalid_request');
    if (!whole(max_units, 1) || max_units > MAX_UNITS) return fail(400, 'max_units is a whole number of units, at least 1', 'invalid_request');
    if (!whole(min_units, 1) || min_units > max_units) return fail(400, 'min_units is from 1 to max_units', 'invalid_request');
    if (price_cents * max_units > TIER2_CAP_CENTS) return fail(400, 'price times max_units is more than a Tier 2 offering may raise in twelve months ($75,000,000)', 'over_tier2_cap');
    if (!Array.isArray(states) || states.some((s) => !US_STATES.has(s))) return fail(400, 'states is a list of two-letter United States state codes', 'invalid_request');
    const docs = {};
    for (const [key, v] of [['circular', circular], ['agreement', agreement]]) {
      if (v === undefined || v === null) { docs[key] = null; continue; }
      const r = readDoc(v, key);
      if (r.problem) return fail(400, r.problem, 'invalid_request');
      docs[key] = r.doc;
    }
    return this.atomic(async () => {
      const prior = await this.storage.get(`o:${offering_id}`);
      const now = new Date().toISOString();
      if (prior?.status === 'closed') return fail(409, 'A closed offering does not change', 'offering_closed');
      if (prior?.status === 'open' && (prior.price_cents !== price_cents || prior.max_units !== max_units)) return fail(409, 'Pause the offering before changing its price or size', 'offering_open');
      if (prior && max_units < prior.reserved_units + prior.sold_units) return fail(409, 'max_units is fewer than the units already reserved and sold', 'below_committed');
      if (prior && LIVE.includes(prior.status) && prior.status !== 'testing' && (!docs.circular || !docs.agreement)) return fail(409, 'An open offering keeps its circular and its agreement', 'documents_required');
      const offering = {
        status: 'draft', sec_file_number: null, qualified_on: null, reserved_units: 0, sold_units: 0, created_at: now, ...prior,
        offering_id, title: name, price_cents, max_units, min_units, states: [...new Set(states)].sort(),
        circular: docs.circular, agreement: docs.agreement, payment_instructions: text(payment_instructions, 2000),
        updated_at: now, by: typeof by === 'string' ? by : null,
      };
      await this.storage.put(`o:${offering_id}`, offering);
      return { ok: true, offering };
    });
  },

  /// Move an offering to another status. Opening one states that the SEC qualified it: the file number and the date are required.
  async offering_status({ offering_id, status, sec_file_number, qualified_on, by }) {
    if (typeof offering_id !== 'string' || !OFFERING_ID.test(offering_id)) return fail(400, 'invalid offering_id', 'invalid_request');
    if (!OFFERING_STATUSES.includes(status)) return fail(400, `status is one of ${OFFERING_STATUSES.join(', ')}`, 'invalid_request');
    return this.atomic(async () => {
      const o = await this.storage.get(`o:${offering_id}`);
      if (!o) return fail(404, 'No such offering', 'not_found');
      if (o.status === status) return { ok: true, offering: o, replay: true };
      if (!NEXT[o.status].includes(status)) return fail(409, `An offering that is ${o.status} cannot become ${status}`, 'bad_transition');
      const next = { ...o, status, updated_at: new Date().toISOString(), by: typeof by === 'string' ? by : null };
      if (LIVE.includes(status)) {
        const live = await liveOffering(this.storage);
        if (live && live.offering_id !== offering_id) return fail(409, `Offering ${live.offering_id} is already live; close it first`, 'another_live');
      }
      if (status === 'open') {
        const file = o.sec_file_number || sec_file_number;
        const day = o.qualified_on || qualified_on;
        if (typeof file !== 'string' || !SEC_FILE.test(file)) return fail(400, 'sec_file_number is the offering statement file number, as 024-12345', 'qualification_required');
        if (typeof day !== 'string' || !DAY.test(day) || Number.isNaN(Date.parse(day)) || day > new Date().toISOString().slice(0, 10)) return fail(400, 'qualified_on is the date of the notice of qualification, YYYY-MM-DD, not in the future', 'qualification_required');
        if (!o.circular || !o.agreement) return fail(409, 'An offering opens with its circular and its subscription agreement on file', 'documents_required');
        if (!o.states.length) return fail(409, 'An offering opens in at least one state', 'states_required');
        if (!o.payment_instructions) return fail(409, 'An offering opens with payment instructions on file', 'payment_instructions_required');
        next.sec_file_number = file;
        next.qualified_on = day;
      }
      await this.storage.put(`o:${offering_id}`, next);
      return { ok: true, offering: next };
    });
  },

  async offering_public() {
    const o = await liveOffering(this.storage);
    return { ok: true, offering: o ? publicView(o) : null };
  },

  /// Every offering, with its reserved and sold units recomputed from its subscriptions and its indications of interest totalled.
  async offering_list() {
    const offerings = [...(await this.storage.list({ prefix: 'o:' })).values()];
    const subs = await this.listAll('s:');
    const out = [];
    for (const o of offerings) {
      const mine = subs.filter((s) => s.offering_id === o.offering_id);
      const sum = (statuses) => mine.filter((s) => statuses.includes(s.status)).reduce((a, s) => a + s.units, 0);
      const reserved = sum(RESERVING);
      const sold = sum(['accepted']);
      const interest = await this.listAll(`i:${o.offering_id}:`);
      out.push({
        ...o,
        counts_sound: reserved === o.reserved_units && sold === o.sold_units,
        raised_cents: sold * o.price_cents,
        interest: { people: interest.length, amount_cents: interest.reduce((a, i) => a + i.amount_cents, 0) },
      });
    }
    return { ok: true, offerings: out };
  },

  /// A non-binding indication of interest, while the offering is testing the waters. It reserves nothing and commits nobody.
  async interest_put({ holder_id, amount_cents }) {
    if (typeof holder_id !== 'string' || !HOLDER_ID.test(holder_id)) return fail(400, 'invalid holder', 'invalid_request');
    if (!whole(amount_cents, 1) || amount_cents > TIER2_CAP_CENTS) return fail(400, 'amount_cents is whole cents, at least 1', 'invalid_request');
    return this.atomic(async () => {
      const o = await liveOffering(this.storage);
      if (!o || o.status !== 'testing') return fail(409, 'No offering is taking indications of interest', 'not_testing');
      const interest = { offering_id: o.offering_id, holder_id, amount_cents, at: new Date().toISOString() };
      await this.storage.put(`i:${o.offering_id}:${holder_id}`, interest);
      return { ok: true, interest, legend: TESTING_LEGEND };
    });
  },

  async interest_get({ holder_id }) {
    const o = await liveOffering(this.storage);
    const interest = o && typeof holder_id === 'string' ? await this.storage.get(`i:${o.offering_id}:${holder_id}`) : null;
    return { ok: true, interest: interest || null };
  },

  async interest_delete({ holder_id }) {
    if (typeof holder_id !== 'string' || !HOLDER_ID.test(holder_id)) return fail(400, 'invalid holder', 'invalid_request');
    return this.atomic(async () => {
      const o = await liveOffering(this.storage);
      if (o) await this.storage.delete(`i:${o.offering_id}:${holder_id}`);
      return { ok: true, interest: null };
    });
  },
};

// -----------------------------------------------------------------------------
// 3. Subscription operations
// -----------------------------------------------------------------------------

/// The subscriptions of one account, oldest first.
async function subsOf(object, holderId) {
  const keys = await object.storage.list({ prefix: `sh:${holderId}:` });
  const out = [];
  for (const k of keys.keys()) {
    const s = await object.storage.get(`s:${k.slice(`sh:${holderId}:`.length)}`);
    if (s) out.push(s);
  }
  return out.sort((a, b) => (a.created_at < b.created_at ? -1 : 1));
}

/// Why this subscriber may not commit `s.amount_cents` more to the offering, or null. `others` are their other subscriptions.
function limitProblem(s, others) {
  if (s.accredited) return null;
  const committed = others.filter((x) => x.id !== s.id && x.offering_id === s.offering_id && COMMITTED.includes(x.status)).reduce((a, x) => a + x.amount_cents, 0);
  if (committed + s.amount_cents > s.limit_cents) {
    return fail(409, `This is more than the investment limit: 10% of the greater of annual income or net worth is ${s.limit_cents} cents, and ${committed} cents is already committed`, 'over_investment_limit');
  }
  return null;
}

/// Take a subscription that holds reserved units out of the reservation.
function release(offering, s) {
  return RESERVING.includes(s.status) ? { ...offering, reserved_units: offering.reserved_units - s.units } : offering;
}

const SUB_OPS = {
  /// Prepare a subscription. It reserves nothing and commits nobody until the person signs it.
  async sub_prepare({ id, holder_id, offering_id, units, legal_name, country, state, accredited, annual_income_cents = null, net_worth_cents = null }) {
    if (typeof id !== 'string' || !SUB_ID.test(id)) return fail(400, 'invalid subscription id', 'invalid_request');
    if (typeof holder_id !== 'string' || !HOLDER_ID.test(holder_id)) return fail(400, 'invalid holder', 'invalid_request');
    if (!whole(units, 1) || units > MAX_UNITS) return fail(400, 'units is a whole number, at least 1', 'invalid_request');
    const name = text(legal_name, 200);
    if (name.length < 2) return fail(400, 'legal_name is your full legal name', 'invalid_request');
    if (country !== 'US') return fail(409, 'This offering is open to residents of the United States only', 'country_not_offered');
    if (typeof accredited !== 'boolean') return fail(400, 'accredited is true or false', 'invalid_request');
    if (!accredited && !(whole(annual_income_cents) && whole(net_worth_cents))) return fail(400, 'annual_income_cents and net_worth_cents are required from a subscriber who is not accredited', 'invalid_request');
    return this.atomic(async () => {
      const o = typeof offering_id === 'string' ? await this.storage.get(`o:${offering_id}`) : null;
      if (!o || o.status !== 'open') return fail(409, 'No offering is open', 'not_open');
      if (!o.states.includes(state)) return fail(409, 'This offering is not available in that state', 'state_not_offered');
      if (units < o.min_units) return fail(409, `The smallest subscription is ${o.min_units} units`, 'below_minimum');
      if (units > o.max_units - o.reserved_units - o.sold_units) return fail(409, 'Fewer units than that are left', 'sold_out');
      const others = await subsOf(this, holder_id);
      if (others.filter((s) => s.status === 'draft').length >= MAX_DRAFTS) return fail(429, 'Sign or cancel a prepared subscription before preparing another', 'too_many_drafts');
      const now = new Date().toISOString();
      const sub = {
        id, offering_id, holder_id, status: 'draft', units, price_cents: o.price_cents, amount_cents: units * o.price_cents,
        legal_name: name, country, state, accredited,
        annual_income_cents: accredited ? null : annual_income_cents, net_worth_cents: accredited ? null : net_worth_cents,
        limit_cents: accredited ? null : investmentLimitCents(annual_income_cents, net_worth_cents),
        circular_sha256: null, agreement_sha256: null, signatures: [], payment_ref: null,
        funds: null, accepted: null, rejected: null, refunded: null, created_at: now, updated_at: now,
      };
      const over = limitProblem(sub, others);
      if (over) return over;
      await this.storage.put({ [`s:${id}`]: sub, [`sh:${holder_id}:${id}`]: 1 });
      return { ok: true, subscription: ownView(sub, o) };
    });
  },

  /// The person signs. A first signature reserves the units; a later one, after the circular or agreement changed, renews it.
  async sub_sign({ id, holder_id, typed_name, circular_sha256, agreement_sha256, attest, ip, user_agent }) {
    if (typeof id !== 'string' || !SUB_ID.test(id)) return fail(400, 'invalid subscription id', 'invalid_request');
    const missing = ATTESTATIONS.filter((k) => !(attest && attest[k] === true));
    if (missing.length) return fail(400, `Each statement must be true: ${missing.join(', ')}`, 'attestation_required');
    return this.atomic(async () => {
      const s = await this.storage.get(`s:${id}`);
      if (!s || s.holder_id !== holder_id) return fail(404, 'No such subscription', 'not_found');
      const o = await this.storage.get(`o:${s.offering_id}`);
      if (!o || o.status !== 'open') return fail(409, 'The offering is not open', 'not_open');
      const renewing = RESERVING.includes(s.status);
      if (s.status !== 'draft' && !renewing) return fail(409, `A subscription that is ${s.status} cannot be signed`, 'not_signable');
      if (circular_sha256 !== o.circular.sha256 || agreement_sha256 !== o.agreement.sha256) return fail(409, 'The offering circular or the subscription agreement changed. Read the current ones and sign again', 'documents_changed');
      if (renewing && s.circular_sha256 === o.circular.sha256 && s.agreement_sha256 === o.agreement.sha256) return { ok: true, replay: true, subscription: ownView(s, o) };
      if (typeof typed_name !== 'string' || !sameName(typed_name, s.legal_name)) return fail(400, 'Type your full legal name exactly as on the subscription', 'name_mismatch');
      const now = new Date().toISOString();
      const signature = { at: now, typed_name: typed_name.trim(), circular_sha256, agreement_sha256, attest: Object.fromEntries(ATTESTATIONS.map((k) => [k, true])), ip: text(ip, 64), user_agent: text(user_agent, 300) };
      const next = { ...s, circular_sha256, agreement_sha256, signatures: [...s.signatures, signature], updated_at: now };
      const writes = {};
      if (!renewing) {
        if (s.price_cents !== o.price_cents) return fail(409, 'The price changed. Prepare a new subscription', 'price_changed');
        if (s.units > o.max_units - o.reserved_units - o.sold_units) return fail(409, 'Fewer units than that are left', 'sold_out');
        const over = limitProblem(s, await subsOf(this, holder_id));
        if (over) return over;
        next.status = 'awaiting_funds';
        next.signed_at = now;
        next.payment_ref = `EUS-${id.replace(/-/g, '').slice(0, 12).toUpperCase()}`;
        writes[`o:${o.offering_id}`] = { ...o, reserved_units: o.reserved_units + s.units };
      }
      writes[`s:${id}`] = next;
      await this.storage.put(writes);
      return { ok: true, subscription: ownView(next, o) };
    });
  },

  /// The subscriber withdraws before acceptance. If their funds arrived, a refund is owed.
  async sub_cancel({ id, holder_id }) {
    return this.atomic(async () => {
      const s = typeof id === 'string' ? await this.storage.get(`s:${id}`) : null;
      if (!s || s.holder_id !== holder_id) return fail(404, 'No such subscription', 'not_found');
      if (s.status === 'cancelled' || s.status === 'refund_due') return { ok: true, replay: true, subscription: ownView(s) };
      if (s.status !== 'draft' && !RESERVING.includes(s.status)) return fail(409, `A subscription that is ${s.status} cannot be cancelled`, 'not_cancellable');
      const o = await this.storage.get(`o:${s.offering_id}`);
      const now = new Date().toISOString();
      const next = { ...s, status: s.status === 'funds_received' ? 'refund_due' : 'cancelled', cancelled_at: now, updated_at: now };
      await this.storage.put({ [`s:${id}`]: next, [`o:${o.offering_id}`]: release(o, s) });
      return { ok: true, subscription: ownView(next) };
    });
  },

  async sub_mine({ holder_id }) {
    if (typeof holder_id !== 'string' || !HOLDER_ID.test(holder_id)) return fail(400, 'invalid holder', 'invalid_request');
    const out = [];
    for (const s of await subsOf(this, holder_id)) out.push(ownView(s, await this.storage.get(`o:${s.offering_id}`)));
    return { ok: true, subscriptions: out };
  },

  /// Every subscription, newest first, for administrators.
  async sub_list({ status, offering_id } = {}) {
    let subs = await this.listAll('s:');
    if (typeof status === 'string') subs = subs.filter((s) => s.status === status);
    if (typeof offering_id === 'string') subs = subs.filter((s) => s.offering_id === offering_id);
    subs.sort((a, b) => (a.created_at < b.created_at ? 1 : -1));
    return { ok: true, total: subs.length, subscriptions: subs.slice(0, PAGE) };
  },

  /// An administrator records that the subscriber's bank transfer arrived. The amount must be the subscription's, to the cent.
  async sub_funds({ id, amount_cents, bank_ref, by }) {
    const ref = text(bank_ref, 100);
    if (!ref) return fail(400, 'bank_ref is the bank\'s reference for the transfer', 'invalid_request');
    return this.atomic(async () => {
      const s = typeof id === 'string' ? await this.storage.get(`s:${id}`) : null;
      if (!s) return fail(404, 'No such subscription', 'not_found');
      if (s.funds && s.funds.bank_ref === ref) return { ok: true, replay: true, subscription: s };
      if (s.status !== 'awaiting_funds') return fail(409, `A subscription that is ${s.status} is not waiting for funds`, 'not_awaiting_funds');
      if (amount_cents !== s.amount_cents) return fail(409, `The subscription is for ${s.amount_cents} cents. Record funds only when that exact amount arrived`, 'amount_mismatch');
      const now = new Date().toISOString();
      const next = { ...s, status: 'funds_received', funds: { received_at: now, amount_cents, bank_ref: ref, by: by || null }, updated_at: now };
      await this.storage.put(`s:${id}`, next);
      return { ok: true, subscription: next };
    });
  },

  /// Accept a subscription: record the holder, issue the units, and mark it accepted, in one write.
  async sub_accept({ id, by }) {
    return this.atomic(async () => {
      const s = typeof id === 'string' ? await this.storage.get(`s:${id}`) : null;
      if (!s) return fail(404, 'No such subscription', 'not_found');
      if (s.status === 'accepted') return { ok: true, replay: true, subscription: s };
      if (s.status !== 'funds_received') return fail(409, `A subscription that is ${s.status} cannot be accepted: its funds are not recorded`, 'funds_not_received');
      const o = await this.storage.get(`o:${s.offering_id}`);
      if (!o || o.status !== 'open') return fail(409, 'The offering is not open', 'not_open');
      if (s.circular_sha256 !== o.circular.sha256 || s.agreement_sha256 !== o.agreement.sha256) return fail(409, 'The subscriber signed an earlier circular or agreement and must sign the current ones first', 'documents_changed');
      const now = new Date().toISOString();
      const prior = await this.storage.get(`h:${s.holder_id}`);
      if (prior && prior.units > 0 && !sameName(prior.legal_name, s.legal_name)) return fail(409, 'This account already holds units under another legal name. Correct the holder record first', 'holder_name_differs');
      const holder = { holder_id: s.holder_id, units: 0, since: null, ...prior, legal_name: s.legal_name, country: s.country, accredited: s.accredited, updated_at: now };
      const r = await this.build(
        { kind: 'issue', ref: `sub:${id}`, to: s.holder_id, units: s.units, price_cents: s.price_cents, memo: `subscription ${s.offering_id}`, by },
        { [`h:${s.holder_id}`]: holder },
      );
      if (r.ok === false) return r;
      const next = { ...s, status: 'accepted', accepted: { at: now, by: by || null, entry_seq: r.entry.seq }, updated_at: now };
      await this.storage.put({
        ...(r.writes || {}),
        [`s:${id}`]: next,
        [`o:${o.offering_id}`]: { ...o, reserved_units: o.reserved_units - s.units, sold_units: o.sold_units + s.units },
      });
      return { ok: true, subscription: next, entry: r.entry };
    });
  },

  /// Refuse a subscription. If its funds arrived, a refund is owed.
  async sub_reject({ id, reason, by }) {
    const why = text(reason, 300);
    if (!why) return fail(400, 'reason is required', 'invalid_request');
    return this.atomic(async () => {
      const s = typeof id === 'string' ? await this.storage.get(`s:${id}`) : null;
      if (!s) return fail(404, 'No such subscription', 'not_found');
      if (s.status === 'rejected' || (s.status === 'refund_due' && s.rejected)) return { ok: true, replay: true, subscription: s };
      if (s.status !== 'draft' && !RESERVING.includes(s.status)) return fail(409, `A subscription that is ${s.status} cannot be rejected`, 'not_rejectable');
      const o = await this.storage.get(`o:${s.offering_id}`);
      const now = new Date().toISOString();
      const next = { ...s, status: s.status === 'funds_received' ? 'refund_due' : 'rejected', rejected: { at: now, reason: why, by: by || null }, updated_at: now };
      await this.storage.put({ [`s:${id}`]: next, [`o:${o.offering_id}`]: release(o, s) });
      return { ok: true, subscription: next };
    });
  },

  /// An administrator records that an owed refund was sent back by the bank.
  async sub_refunded({ id, bank_ref, by }) {
    const ref = text(bank_ref, 100);
    if (!ref) return fail(400, 'bank_ref is the bank\'s reference for the refund', 'invalid_request');
    return this.atomic(async () => {
      const s = typeof id === 'string' ? await this.storage.get(`s:${id}`) : null;
      if (!s) return fail(404, 'No such subscription', 'not_found');
      if (s.status === 'refunded') return { ok: true, replay: true, subscription: s };
      if (s.status !== 'refund_due') return fail(409, `A subscription that is ${s.status} is owed no refund`, 'no_refund_due');
      const now = new Date().toISOString();
      const next = { ...s, status: 'refunded', refunded: { at: now, bank_ref: ref, by: by || null }, updated_at: now };
      await this.storage.put(`s:${id}`, next);
      return { ok: true, subscription: next };
    });
  },

  /// Let go of signed subscriptions whose funds never arrived, so their units are free for others.
  async sub_expire({ older_than_days, by }) {
    if (!whole(older_than_days, 1)) return fail(400, 'older_than_days is a whole number of days, at least 1', 'invalid_request');
    return this.atomic(async () => {
      const cutoff = new Date(Date.now() - older_than_days * 86_400_000).toISOString();
      const now = new Date().toISOString();
      const writes = {};
      const offerings = {};
      const expired = [];
      for (const s of await this.listAll('s:')) {
        if (s.status !== 'awaiting_funds' || !(s.signed_at < cutoff)) continue;
        const key = `o:${s.offering_id}`;
        offerings[key] = offerings[key] || (await this.storage.get(key));
        offerings[key] = release(offerings[key], s);
        writes[`s:${s.id}`] = { ...s, status: 'expired', expired: { at: now, by: by || null }, updated_at: now };
        expired.push(s.id);
        if (expired.length >= 100) break;
      }
      if (expired.length) await this.storage.put({ ...writes, ...offerings });
      return { ok: true, expired };
    });
  },
};

/// The operations the HolderLedger object adds to its own.
export const SUBSCRIPTION_OPS = { ...OFFERING_OPS, ...SUB_OPS };

// -----------------------------------------------------------------------------
// 4. Routes
// -----------------------------------------------------------------------------

const readBody = async (request) => {
  try {
    const v = await request.json();
    return v && typeof v === 'object' && !Array.isArray(v) ? v : null;
  } catch {
    return null;
  }
};

/// The signed-in account, if it may subscribe: a verified identity and no ban. `{ userId }` or `{ refusal }`.
async function subscriber(request, env, cors, deps) {
  const userId = await deps.verifyAuth(request, env);
  if (!userId) return { refusal: deps.json({ error: 'Unauthorized' }, 401, cors) };
  let user = null;
  try {
    user = JSON.parse(await env.USERS.get(`user:${userId}`));
  } catch {
    user = null;
  }
  if (!user || user.banned) return { refusal: deps.json({ error: 'This account cannot subscribe', code: 'account_refused' }, 403, cors) };
  if (user.kyc_verified !== true) return { refusal: deps.json({ error: 'Verify your identity before subscribing', code: 'identity_required' }, 403, cors) };
  return { userId };
}

/// Handle /api/offering* and /api/subscriptions*; null for any other path. `deps`: { json, verifyAuth, ledgerCall }.
export async function handleSubscriptionRoute(request, url, env, cors, deps) {
  const p = url.pathname;
  const { json, ledgerCall } = deps;
  const m = request.method;
  const headers = { ...cors, 'Cache-Control': 'no-store' };
  const answer = (r) => (r.ok === false ? json({ error: r.error, code: r.code }, r.status || 500, headers) : json(r, 200, headers));

  if (p === '/api/offering' && m === 'GET') return answer(await ledgerCall(env, 'offering_public'));
  const signMatch = /^\/api\/subscriptions\/([a-f0-9-]{36})\/(sign|cancel)$/.exec(p);
  if (p !== '/api/offering/interest' && p !== '/api/subscriptions' && !signMatch) return null;

  const who = await subscriber(request, env, cors, deps);
  if (who.refusal) return who.refusal;
  const holder_id = who.userId;

  if (p === '/api/offering/interest') {
    if (m === 'GET') return answer(await ledgerCall(env, 'interest_get', { holder_id }));
    if (m === 'DELETE') return answer(await ledgerCall(env, 'interest_delete', { holder_id }));
    if (m === 'PUT') {
      const body = await readBody(request);
      if (!body) return json({ error: 'JSON body required' }, 400, cors);
      return answer(await ledgerCall(env, 'interest_put', { holder_id, amount_cents: body.amount_cents }));
    }
    return json({ error: 'Not found' }, 404, cors);
  }
  if (p === '/api/subscriptions' && m === 'GET') return answer(await ledgerCall(env, 'sub_mine', { holder_id }));
  if (m !== 'POST') return json({ error: 'Not found' }, 404, cors);
  if (signMatch && signMatch[2] === 'cancel') return answer(await ledgerCall(env, 'sub_cancel', { id: signMatch[1], holder_id }));
  const body = await readBody(request);
  if (!body) return json({ error: 'JSON body required' }, 400, cors);
  if (p === '/api/subscriptions') {
    const { offering_id, units, legal_name, country, state, accredited, annual_income_cents, net_worth_cents } = body;
    // The id is made here and the holder is the session: neither comes from the body.
    return answer(await ledgerCall(env, 'sub_prepare', { id: crypto.randomUUID(), holder_id, offering_id, units, legal_name, country, state, accredited, annual_income_cents, net_worth_cents }));
  }
  const id = signMatch[1];
  const { typed_name, circular_sha256, agreement_sha256, attest } = body;
  return answer(await ledgerCall(env, 'sub_sign', {
    id, holder_id, typed_name, circular_sha256, agreement_sha256, attest,
    ip: request.headers.get('cf-connecting-ip') || '', user_agent: request.headers.get('user-agent') || '',
  }));
}

const ADMIN_ACTS = { funds: 'sub_funds', accept: 'sub_accept', reject: 'sub_reject', refunded: 'sub_refunded' };

/// Handle /api/admin/offerings* and /api/admin/subscriptions*; null for any other path. `deps`: { json, requireAdmin, ledgerCall }.
export async function handleSubscriptionAdminRoute(request, url, env, cors, deps) {
  const p = url.pathname;
  const inOfferings = p === '/api/admin/offerings' || p.startsWith('/api/admin/offerings/');
  const inSubs = p === '/api/admin/subscriptions' || p.startsWith('/api/admin/subscriptions/');
  if (!inOfferings && !inSubs) return null;
  const { json, ledgerCall } = deps;
  const adminId = await deps.requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);
  const m = request.method;
  const headers = { ...cors, 'Cache-Control': 'no-store' };
  const answer = (r) => (r.ok === false ? json({ error: r.error, code: r.code }, r.status || 500, headers) : json(r, 200, headers));

  if (m === 'GET') {
    if (p === '/api/admin/offerings') return answer(await ledgerCall(env, 'offering_list'));
    if (p === '/api/admin/subscriptions') {
      return answer(await ledgerCall(env, 'sub_list', { status: url.searchParams.get('status') || undefined, offering_id: url.searchParams.get('offering_id') || undefined }));
    }
    return json({ error: 'Not found' }, 404, cors);
  }
  const body = await readBody(request);
  if (!body) return json({ error: 'JSON body required' }, 400, cors);
  // Who acted is the administrator's session, never the body.
  const by = adminId;
  if (p === '/api/admin/offerings' && m === 'PUT') {
    const { offering_id, title, price_cents, max_units, min_units, states, circular, agreement, payment_instructions } = body;
    return answer(await ledgerCall(env, 'offering_put', { offering_id, title, price_cents, max_units, min_units, states, circular, agreement, payment_instructions, by }));
  }
  if (m !== 'POST') return json({ error: 'Not found' }, 404, cors);
  if (p === '/api/admin/offerings/status') {
    const { offering_id, status, sec_file_number, qualified_on } = body;
    return answer(await ledgerCall(env, 'offering_status', { offering_id, status, sec_file_number, qualified_on, by }));
  }
  if (p === '/api/admin/subscriptions/expire') return answer(await ledgerCall(env, 'sub_expire', { older_than_days: body.older_than_days, by }));
  const act = /^\/api\/admin\/subscriptions\/([a-f0-9-]{36})\/(funds|accept|reject|refunded)$/.exec(p);
  if (!act) return json({ error: 'Not found' }, 404, cors);
  const { amount_cents, bank_ref, reason } = body;
  return answer(await ledgerCall(env, ADMIN_ACTS[act[2]], { id: act[1], amount_cents, bank_ref, reason, by }));
}
