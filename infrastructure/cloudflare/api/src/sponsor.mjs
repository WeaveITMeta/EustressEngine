// =============================================================================
// Eustress API - Engine sponsorships (/sponsor on the site)
// =============================================================================
// A sponsorship is money given to build the engine: once, or every month. It is
// the mirror of a gift to the Treasury. A gift to the Treasury goes whole to
// contributors; a sponsorship goes whole to Eustress's side, where the engine's
// costs are paid, after the payment fee. Nothing is split.
//
// A sponsor receives nothing that can be spent, traded or cashed out: no
// Tickets, no Delta, no Bliss, no share of the company and no claim on its
// revenue. What a sponsor can have is their name on the sponsor list, and only
// when they are signed in and ask for it: the list shows account usernames,
// never text typed at checkout.
//
//   POST /api/sponsor/checkout   {tier?, custom_amount?, mode, listed?}
//        Opens Stripe Checkout. Works signed out; a signed-out sponsor cannot
//        be listed. Prices are sent inline, so no Stripe price or product has
//        to exist in either mode.
//   GET  /api/sponsors           public
//        The tiers on offer, and the sponsors who asked to be listed.
//   PUT  /api/sponsor/listing    {listed}
//        The signed-in sponsor puts their name on the list or takes it off.
//
// The Stripe webhook (index.js) hands a paid session or a paid renewal to
// settleSponsorCheckout / settleSponsorInvoice. Each deposit is keyed by the
// session or invoice, so a redelivery adds nothing. Refunds and disputes are
// reversed by the Treasury's ordinary refund path, by payment intent.
//
// Table of Contents:
// 1. Tiers and the split
// 2. Checkout
// 3. Settling a payment
// 4. The sponsor list
// =============================================================================

import { CHANNEL_FEES } from './treasury.mjs';

// -----------------------------------------------------------------------------
// 1. Tiers and the split
// -----------------------------------------------------------------------------

/// The deposit kind a sponsorship is recorded under (treasury.mjs DEPOSIT_KINDS).
export const SPONSOR_KIND = 'engine_sponsor';

/// The tiers, in the order they are offered. A custom amount is also taken.
export const SPONSOR_TIERS = [
  { id: 'supporter', name: 'Supporter', cents: 1000 },
  { id: 'builder', name: 'Builder', cents: 5000 },
  { id: 'champion', name: 'Champion', cents: 25000 },
  { id: 'founding', name: 'Founding Sponsor', cents: 100000 },
];

export const SPONSOR_MIN_CENTS = 500;
export const SPONSOR_MAX_CENTS = 10_000_000;
/// The sponsor list shows at most this many names.
export const SPONSOR_LIST_LIMIT = 200;

const tierOf = (id) => (typeof id === 'string' ? SPONSOR_TIERS.find((t) => t.id === id) || null : null);

/// A sponsorship of `grossCents` split into the payment fee and Eustress's
/// share, in whole cents that add up to the gross. The Treasury's share is
/// zero: a sponsorship pays for the engine, and contributors are funded by
/// sales and by gifts to the Treasury.
export function splitSponsorship(grossCents) {
  const gross = Math.max(0, Math.trunc(Number(grossCents) || 0));
  const f = CHANNEL_FEES.web;
  const fee = Math.min(gross, Math.round(gross * f.rate) + f.flat_cents);
  return { gross_cents: gross, fee_cents: fee, treasury_cents: 0, platform_cents: gross - fee };
}

// -----------------------------------------------------------------------------
// 2. Checkout
// -----------------------------------------------------------------------------

/// What a checkout request asks for: `{ cents, tier, monthly, listed }`, or
/// `{ error }`. A tier wins over a custom amount.
export function readSponsorRequest(body) {
  if (!body || typeof body !== 'object' || Array.isArray(body)) return { error: 'JSON body required' };
  const monthly = body.mode === 'recurring';
  const listed = body.listed === true;
  const tier = tierOf(body.tier);
  if (tier) return { cents: tier.cents, tier: tier.id, monthly, listed };
  const custom = typeof body.custom_amount === 'number' ? body.custom_amount : Number.NaN;
  const cents = Math.round(custom * 100);
  if (Number.isFinite(custom) && cents >= SPONSOR_MIN_CENTS && cents <= SPONSOR_MAX_CENTS) {
    return { cents, tier: null, monthly, listed };
  }
  return { error: 'Select a tier or enter a custom amount from $5 to $100,000' };
}

/// `deps`: { verifyAuth, json, stripeRequest, siteOrigin }.
export async function handleSponsorCheckout(request, env, cors, deps) {
  const { verifyAuth, json, stripeRequest, siteOrigin } = deps;
  if (!env.STRIPE_SECRET_KEY) return json({ error: 'Stripe not configured' }, 503, cors);
  const userId = await verifyAuth(request, env);
  const ask = readSponsorRequest(await request.json().catch(() => null));
  if (ask.error) return json({ error: ask.error }, 400, cors);
  // Only an account can be listed: the list shows its username.
  const listed = ask.listed && !!userId;
  const name = ask.tier ? `Eustress Engine sponsorship: ${tierOf(ask.tier).name}` : 'Eustress Engine sponsorship';

  const params = {
    'mode': ask.monthly ? 'subscription' : 'payment',
    'success_url': `${siteOrigin(env)}/sponsor?sponsored=true`,
    'cancel_url': `${siteOrigin(env)}/sponsor?sponsored=false`,
    'metadata[type]': SPONSOR_KIND,
    'metadata[tier]': ask.tier || 'custom',
    'metadata[listed]': listed ? '1' : '0',
    'line_items[0][price_data][currency]': 'usd',
    'line_items[0][price_data][product_data][name]': name,
    'line_items[0][price_data][unit_amount]': String(ask.cents),
    'line_items[0][quantity]': '1',
  };
  if (ask.monthly) {
    params['line_items[0][price_data][recurring][interval]'] = 'month';
    params['subscription_data[metadata][type]'] = SPONSOR_KIND;
    params['subscription_data[metadata][tier]'] = ask.tier || 'custom';
  }
  if (userId) {
    params['metadata[user_id]'] = userId;
    params['client_reference_id'] = userId;
    if (ask.monthly) params['subscription_data[metadata][user_id]'] = userId;
  }

  const session = await stripeRequest('POST', '/checkout/sessions', params, env);
  if (session.error) return json({ error: session.error.message, type: session.error.type }, 400, cors);
  return json({ url: session.url, session_id: session.id }, 200, cors);
}

// -----------------------------------------------------------------------------
// 3. Settling a payment
// -----------------------------------------------------------------------------

const sponsorKey = (userId) => `sponsor:${userId}`;

async function readSponsor(env, userId) {
  try {
    const raw = await env.PAYOUTS.get(sponsorKey(userId));
    return raw ? JSON.parse(raw) : null;
  } catch {
    return null;
  }
}

/// Add one payment to an account's sponsor record. `listed` is set only when
/// the payment says so (true or false); a renewal leaves the choice as it is.
async function recordSponsor(env, userId, { cents, monthly, tier, listed }) {
  const now = new Date().toISOString();
  const prior = (await readSponsor(env, userId)) || { user_id: userId, since: now, total_cents: 0, payments: 0, listed: false, monthly: false, tier: null };
  const next = {
    ...prior,
    total_cents: (Number(prior.total_cents) || 0) + cents,
    payments: (Number(prior.payments) || 0) + 1,
    monthly: prior.monthly || monthly,
    tier: tier || prior.tier || null,
    listed: typeof listed === 'boolean' ? listed : prior.listed,
    last_at: now,
  };
  await env.PAYOUTS.put(sponsorKey(userId), JSON.stringify(next));
  return next;
}

/// Deposit one sponsorship payment. Resolves to the Treasury's answer.
function depositSponsorship(env, treasuryCall, { ref, cents, userId, paymentIntent }) {
  const split = splitSponsorship(cents);
  return treasuryCall(env, 'deposit', {
    ref, kind: SPONSOR_KIND,
    gross_cents: split.gross_cents, fee_cents: split.fee_cents,
    treasury_cents: split.treasury_cents, platform_cents: split.platform_cents,
    user_id: userId || null, payment_intent: paymentIntent || null, channel: 'web',
  });
}

/// A paid sponsorship Checkout Session (the first payment of a monthly one
/// included). `deps`: { treasuryCall }.
export async function settleSponsorCheckout(session, grossCents, userId, env, deps) {
  if (grossCents > 0) {
    const deposit = await depositSponsorship(env, deps.treasuryCall, {
      ref: `stripe:${session.id}`, cents: grossCents, userId, paymentIntent: session.payment_intent,
    });
    if (!deposit.ok) {
      console.error(`sponsorship deposit for session ${session.id} failed:`, deposit.error);
      return new Response('Sponsorship deposit failed', { status: 500 });
    }
    if (!deposit.replay) {
      await env.PAYOUTS.put(`deposit:${session.id}`, JSON.stringify({
        id: session.id, type: SPONSOR_KIND, amount_usd: grossCents / 100,
        user_id: userId || 'anonymous', timestamp: new Date().toISOString(),
        mode: session.mode, stripe_payment_intent: session.payment_intent || null,
      }));
      if (userId) {
        const tier = tierOf(session.metadata?.tier)?.id || null;
        await recordSponsor(env, userId, { cents: grossCents, monthly: session.mode === 'subscription', tier, listed: session.metadata?.listed === '1' });
      }
    }
  }
  return new Response('OK', { status: 200 });
}

/// A paid renewal of a monthly sponsorship. The first invoice is paid through
/// its Checkout Session, so only the cycles after it are deposited here.
export async function settleSponsorInvoice(invoice, meta, env, deps) {
  if (invoice.billing_reason !== 'subscription_cycle') return new Response('OK', { status: 200 });
  const cents = Math.max(0, Math.trunc((invoice.amount_paid || 0) - (invoice.tax || 0)));
  if (cents > 0) {
    const userId = typeof meta.user_id === 'string' && meta.user_id ? meta.user_id : null;
    const deposit = await depositSponsorship(env, deps.treasuryCall, {
      ref: `stripe-invoice:${invoice.id}`, cents, userId, paymentIntent: invoice.payment_intent,
    });
    if (!deposit.ok) {
      console.error(`sponsorship deposit for invoice ${invoice.id} failed:`, deposit.error);
      return new Response('Sponsorship deposit failed', { status: 500 });
    }
    if (!deposit.replay && userId) {
      await recordSponsor(env, userId, { cents, monthly: true, tier: tierOf(meta.tier)?.id || null });
    }
  }
  return new Response('OK', { status: 200 });
}

// -----------------------------------------------------------------------------
// 4. The sponsor list
// -----------------------------------------------------------------------------

/// The sponsors who asked to be listed, earliest first: username, the month
/// they first sponsored, and whether they sponsor monthly. Amounts are not
/// shown. A banned or deleted account is left out.
export async function listSponsors(env) {
  const out = [];
  if (!env.PAYOUTS || !env.USERS) return out;
  const page = await env.PAYOUTS.list({ prefix: 'sponsor:', limit: 1000 });
  const records = [];
  for (const { name } of page.keys || []) {
    const rec = await readSponsor(env, name.slice('sponsor:'.length));
    if (rec && rec.listed === true && typeof rec.user_id === 'string') records.push(rec);
  }
  records.sort((a, b) => (String(a.since) < String(b.since) ? -1 : 1));
  for (const rec of records) {
    if (out.length >= SPONSOR_LIST_LIMIT) break;
    let user = null;
    try {
      const raw = await env.USERS.get(`user:${rec.user_id}`);
      user = raw ? JSON.parse(raw) : null;
    } catch {
      user = null;
    }
    if (!user || user.banned || typeof user.username !== 'string') continue;
    out.push({ username: user.username, since: String(rec.since).slice(0, 7), monthly: rec.monthly === true });
  }
  return out;
}

/// GET /api/sponsors. `deps`: { json, publicCors }.
export async function handleSponsorList(env, deps) {
  const headers = { ...deps.publicCors(), 'Cache-Control': 'public, max-age=300' };
  const tiers = SPONSOR_TIERS.map((t) => ({ id: t.id, name: t.name, usd: t.cents / 100 }));
  return deps.json({ tiers, sponsors: await listSponsors(env) }, 200, headers);
}

/// PUT /api/sponsor/listing {listed}. `deps`: { verifyAuth, json }.
export async function handleSponsorListing(request, env, cors, deps) {
  const userId = await deps.verifyAuth(request, env);
  if (!userId) return deps.json({ error: 'Unauthorized' }, 401, cors);
  const body = await request.json().catch(() => null);
  if (!body || typeof body.listed !== 'boolean') return deps.json({ error: 'listed is true or false' }, 400, cors);
  const rec = await readSponsor(env, userId);
  if (!rec) return deps.json({ error: 'This account has not sponsored the engine' }, 404, cors);
  rec.listed = body.listed;
  await env.PAYOUTS.put(sponsorKey(userId), JSON.stringify(rec));
  return deps.json({ ok: true, listed: rec.listed }, 200, cors);
}
