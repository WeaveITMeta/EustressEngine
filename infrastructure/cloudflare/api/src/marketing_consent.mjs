// =============================================================================
// Marketing email consent: the "Marketing Emails" box at sign-up and the switch
// in Settings. Design: docs/marketing/TARGETED_MARKETING_OPT_IN.md.
// =============================================================================
//
// Whether the box may start ticked is decided here, from the country Cloudflare
// reports for the request, and never from the page: the page only shows what
// this module says. The rule is GetCSV's jurisdiction table
// (getcsv-desktop crates/marketing/src/jurisdiction.rs): the United States is
// opt-out under CAN-SPAM, so the box may start ticked there. Every other
// country in that table needs consent the person gives themselves (GDPR, UK
// PECR, CASL for a consumer sign-up, and the rest), so the box starts empty and
// only their own tick turns it on. A country Cloudflare cannot name (XX, Tor
// exits as T1) is treated as one that needs consent.
//
// What is kept, per account (USERS):
//   notify-prefs:{id}         .marketing_email (bool) and .marketing (the latest
//                             choice: on, basis, country, version, source, at),
//                             beside .moderation_email
//   mkt-optin:{id}            present while marketing_email is on: the index the
//                             daily list is built from (no walk of every account)
//   mkt-consent:{id}:{at}     one record per change, never overwritten: the
//                             evidence of who agreed to what, where and when
//
// The basis of an "on":
//   prechecked  the box started ticked (an opt-out country) and was left ticked
//   express     the person ticked it themselves (the box started empty, or the
//               Settings switch was turned on)
// GetCSV reads `express` as consent `granted` and `prechecked` as `not-required`.

/// Countries where the box may start ticked. Opt-out countries in GetCSV's
/// table; widening this is a legal decision, not a code one.
export const OPT_OUT_COUNTRIES = new Set(['US']);

/// The version of the sentence shown beside the box and the switch. Bump it
/// whenever the sentence changes, so each record says which words were agreed to.
export const MARKETING_CONSENT_VERSION = 1;

/// The sentence itself, so the record and the page cannot drift apart.
export const MARKETING_CONSENT_TEXT =
  'Marketing Emails (optional): Eustress news, release notes, your stats and holiday promotions. Unsubscribe anytime.';

/// The two-letter country Cloudflare reports for this request, or null when it
/// names none (XX) or a Tor exit (T1).
export function requestCountry(request) {
  const raw = request?.cf?.country || request?.headers?.get?.('cf-ipcountry') || '';
  const code = String(raw).trim().toUpperCase();
  if (!/^[A-Z]{2}$/.test(code) || code === 'XX' || code === 'T1') return null;
  return code;
}

/// Whether the box may start ticked for a request from `country`.
export function marketingPrechecked(country) {
  return typeof country === 'string' && OPT_OUT_COUNTRIES.has(country);
}

/// What the sign-up page shows: the box's starting state and its sentence.
export function marketingDefault(request) {
  const country = requestCountry(request);
  return {
    prechecked: marketingPrechecked(country),
    text: MARKETING_CONSENT_TEXT,
    version: MARKETING_CONSENT_VERSION,
  };
}

/// The basis of a choice: `on` with the box shown `prechecked`, from `source`.
export function consentBasis(on, prechecked, source) {
  if (!on) return source === 'unsubscribe_link' ? 'unsubscribed' : 'declined';
  return source === 'signup' && prechecked ? 'prechecked' : 'express';
}

/// Record a choice for `userId`: the prefs, the index and one log record.
/// `source` is `signup`, `settings` or `unsubscribe_link`. `prechecked` is what
/// this Worker showed, recomputed from the request, never what the page claims.
export async function recordMarketingChoice(env, userId, { on, country, prechecked, source, now = new Date() }) {
  const at = now.toISOString();
  const choice = {
    on: !!on,
    basis: consentBasis(!!on, !!prechecked, source),
    country: country || null,
    version: MARKETING_CONSENT_VERSION,
    source,
    at,
  };
  const prev = JSON.parse((await env.USERS.get(`notify-prefs:${userId}`)) || '{}');
  await env.USERS.put(`notify-prefs:${userId}`, JSON.stringify({ ...prev, marketing_email: choice.on, marketing: choice, updated_at: at }));
  if (choice.on) {
    await env.USERS.put(`mkt-optin:${userId}`, JSON.stringify({ basis: choice.basis, country: choice.country, version: choice.version, at }));
  } else {
    await env.USERS.delete(`mkt-optin:${userId}`);
  }
  await env.USERS.put(`mkt-consent:${userId}:${at}`, JSON.stringify(choice));
  return choice;
}

/// The account's current choice: off unless it chose on.
export async function marketingEmailOn(env, userId) {
  const prefs = JSON.parse((await env.USERS.get(`notify-prefs:${userId}`)) || '{}');
  return prefs.marketing_email === true;
}
