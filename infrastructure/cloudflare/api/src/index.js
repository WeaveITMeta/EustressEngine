/**
 * Eustress API Worker — api.eustress.dev
 *
 * Unified Cloudflare Worker handling:
 *   - Auth (Ed25519 challenge-response + JWT)
 *   - KYC (jurisdiction detection + R2 document upload)
 *   - Co-signing (witness co-signatures)
 *   - Health check
 *
 * KV Namespaces:
 *   USERS      — user_id → { username, public_key, ... }
 *   CHALLENGES — public_key → { challenge, expires_at }
 *   KYC_STATUS — verification_id → { status, r2_key, ... }
 *
 * R2 Bucket:
 *   KYC_BUCKET — identity documents (kyc-XX-timestamp-hash.bin)
 *
 * Secrets:
 *   JWT_SECRET — persistent JWT signing key
 */

// ═══════════════════════════════════════════════════════════════════════════
// ═══════════════════════════════════════════════════════════════════════════
// xAI / Grok
// ═══════════════════════════════════════════════════════════════════════════

/// Model used for document verification, background screening, and search.
const GROK_MODEL = 'grok-4.6';

/// Single entry point for every xAI call.
///
/// The Responses API is STATEFUL: it stores prompts and outputs for 30 days by
/// default, retrievable later by response id. For these calls that payload is
/// photographs of government identity documents plus applicants' legal names
/// and dates of birth, so `store: false` is mandatory, not a preference.
///
/// The privacy fields are applied AFTER the caller's body is spread, so a call
/// site cannot override them, and routing every request through here means a
/// new call site cannot forget them.
///
/// Two limits this cannot reach, both account-level rather than per-request:
///   - xAI retains API traffic for 30 days for abuse auditing (encrypted at
///     rest, then deleted). Removing that needs a Zero Data Retention
///     agreement arranged with xAI on the account.
///   - xAI states it does not train on API data.
async function grokFetch(body, apiKey) {
  return fetch('https://api.x.ai/v1/responses', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Authorization': `Bearer ${apiKey}`,
    },
    body: JSON.stringify({
      ...body,
      model: GROK_MODEL,
      store: false,
    }),
  });
}

// KYC jurisdiction ontology
// ═══════════════════════════════════════════════════════════════════════════
// Per-country accepted identity documents + the local age of majority.
//
// `age_of_majority` is ONLY ever used to raise the bar. The effective minimum
// is max(MIN_AGE_FINANCE, age_of_majority), so a jurisdiction can require an
// older applicant than 18 but can never lower the global floor. See
// `minimumAgeFor()`.
//
// Countries not listed fall through to `fallback`, which accepts a passport
// and flags the application for manual review rather than silently approving.

/// Global floor for any money-touching flow (Bliss earning / cash out).
/// Nothing in this Worker may approve an applicant younger than this.
const MIN_AGE_FINANCE = 18;

const JURISDICTIONS = {
  jurisdictions: {
    // ── North America ──────────────────────────────────────────────────────
    US: { name: 'United States', natural_ids: ['Passport', "Driver's license", 'State ID card'], r2_prefix: 'kyc-us-', age_of_majority: 18, notes: 'IRS QI: government photo ID' },
    CA: { name: 'Canada', natural_ids: ['Passport', "Driver's licence", 'Provincial ID card'], r2_prefix: 'kyc-ca-', age_of_majority: 19, notes: 'Age of majority is 19 in BC, NB, NL, NS, NT, NU, YT' },
    MX: { name: 'Mexico', natural_ids: ['Passport', 'INE/IFE voter card', "Driver's licence"], r2_prefix: 'kyc-mx-', age_of_majority: 18 },

    // ── Europe ─────────────────────────────────────────────────────────────
    GB: { name: 'United Kingdom', natural_ids: ['Passport', "Driver's licence", 'National ID card'], r2_prefix: 'kyc-gb-', age_of_majority: 18 },
    IE: { name: 'Ireland', natural_ids: ['Passport', "Driver's licence", 'Public Services Card'], r2_prefix: 'kyc-ie-', age_of_majority: 18 },
    DE: { name: 'Germany', natural_ids: ['Passport', 'Personalausweis', "Driver's licence"], r2_prefix: 'kyc-de-', age_of_majority: 18 },
    FR: { name: 'France', natural_ids: ['Passport', "Carte nationale d'identité", "Driver's licence"], r2_prefix: 'kyc-fr-', age_of_majority: 18 },
    ES: { name: 'Spain', natural_ids: ['Passport', 'DNI', 'NIE', "Driver's licence"], r2_prefix: 'kyc-es-', age_of_majority: 18 },
    IT: { name: 'Italy', natural_ids: ['Passport', "Carta d'identità", "Driver's licence"], r2_prefix: 'kyc-it-', age_of_majority: 18 },
    NL: { name: 'Netherlands', natural_ids: ['Passport', 'Identiteitskaart', "Driver's licence"], r2_prefix: 'kyc-nl-', age_of_majority: 18 },
    BE: { name: 'Belgium', natural_ids: ['Passport', 'eID card', "Driver's licence"], r2_prefix: 'kyc-be-', age_of_majority: 18 },
    PL: { name: 'Poland', natural_ids: ['Passport', 'Dowód osobisty', "Driver's licence"], r2_prefix: 'kyc-pl-', age_of_majority: 18 },
    SE: { name: 'Sweden', natural_ids: ['Passport', 'National ID card', "Driver's licence"], r2_prefix: 'kyc-se-', age_of_majority: 18 },
    NO: { name: 'Norway', natural_ids: ['Passport', 'National ID card', "Driver's licence"], r2_prefix: 'kyc-no-', age_of_majority: 18 },
    DK: { name: 'Denmark', natural_ids: ['Passport', "Driver's licence"], r2_prefix: 'kyc-dk-', age_of_majority: 18 },
    FI: { name: 'Finland', natural_ids: ['Passport', 'National ID card', "Driver's licence"], r2_prefix: 'kyc-fi-', age_of_majority: 18 },
    CH: { name: 'Switzerland', natural_ids: ['Passport', 'Identity card', "Driver's licence"], r2_prefix: 'kyc-ch-', age_of_majority: 18 },
    AT: { name: 'Austria', natural_ids: ['Passport', 'Personalausweis', "Driver's licence"], r2_prefix: 'kyc-at-', age_of_majority: 18 },
    PT: { name: 'Portugal', natural_ids: ['Passport', 'Cartão de Cidadão', "Driver's licence"], r2_prefix: 'kyc-pt-', age_of_majority: 18 },
    CZ: { name: 'Czechia', natural_ids: ['Passport', 'Občanský průkaz', "Driver's licence"], r2_prefix: 'kyc-cz-', age_of_majority: 18 },
    RO: { name: 'Romania', natural_ids: ['Passport', 'Carte de identitate', "Driver's licence"], r2_prefix: 'kyc-ro-', age_of_majority: 18 },
    GR: { name: 'Greece', natural_ids: ['Passport', 'Identity card', "Driver's licence"], r2_prefix: 'kyc-gr-', age_of_majority: 18 },
    UA: { name: 'Ukraine', natural_ids: ['Passport', 'ID card'], r2_prefix: 'kyc-ua-', age_of_majority: 18, flag: 'enhanced_dd' },

    // ── Asia-Pacific ───────────────────────────────────────────────────────
    AU: { name: 'Australia', natural_ids: ['Passport', "Driver's licence", 'Proof of Age card'], r2_prefix: 'kyc-au-', age_of_majority: 18, minors: { note: 'Birth certificate accepted under 21' } },
    NZ: { name: 'New Zealand', natural_ids: ['Passport', "Driver's licence", 'Kiwi Access card'], r2_prefix: 'kyc-nz-', age_of_majority: 18 },
    JP: { name: 'Japan', natural_ids: ['Passport', 'My Number card', "Driver's licence"], r2_prefix: 'kyc-jp-', age_of_majority: 18, notes: 'Lowered from 20 to 18 in April 2022' },
    KR: { name: 'South Korea', natural_ids: ['Passport', 'Resident registration card', "Driver's licence"], r2_prefix: 'kyc-kr-', age_of_majority: 19 },
    SG: { name: 'Singapore', natural_ids: ['Passport', 'NRIC', "Driver's licence"], r2_prefix: 'kyc-sg-', age_of_majority: 21, notes: 'Contractual age of majority is 21' },
    IN: { name: 'India', natural_ids: ['Passport', 'Aadhaar', 'PAN card', "Driver's licence"], r2_prefix: 'kyc-in-', age_of_majority: 18 },
    PH: { name: 'Philippines', natural_ids: ['Passport', 'UMID', "Driver's licence"], r2_prefix: 'kyc-ph-', age_of_majority: 18 },
    ID: { name: 'Indonesia', natural_ids: ['Passport', 'KTP', "Driver's licence"], r2_prefix: 'kyc-id-', age_of_majority: 21 },
    TH: { name: 'Thailand', natural_ids: ['Passport', 'National ID card', "Driver's licence"], r2_prefix: 'kyc-th-', age_of_majority: 20 },
    MY: { name: 'Malaysia', natural_ids: ['Passport', 'MyKad', "Driver's licence"], r2_prefix: 'kyc-my-', age_of_majority: 18 },
    TW: { name: 'Taiwan', natural_ids: ['Passport', 'National ID card', "Driver's licence"], r2_prefix: 'kyc-tw-', age_of_majority: 18, notes: 'Lowered from 20 to 18 in January 2023' },
    HK: { name: 'Hong Kong', natural_ids: ['Passport', 'HKID card'], r2_prefix: 'kyc-hk-', age_of_majority: 18 },

    // ── Latin America ──────────────────────────────────────────────────────
    BR: { name: 'Brazil', natural_ids: ['Passport', 'RG', 'CPF', 'CNH'], r2_prefix: 'kyc-br-', age_of_majority: 18 },
    AR: { name: 'Argentina', natural_ids: ['Passport', 'DNI', "Driver's licence"], r2_prefix: 'kyc-ar-', age_of_majority: 18 },
    CL: { name: 'Chile', natural_ids: ['Passport', 'Cédula de identidad'], r2_prefix: 'kyc-cl-', age_of_majority: 18 },
    CO: { name: 'Colombia', natural_ids: ['Passport', 'Cédula de ciudadanía'], r2_prefix: 'kyc-co-', age_of_majority: 18 },

    // ── Middle East / Africa ───────────────────────────────────────────────
    AE: { name: 'United Arab Emirates', natural_ids: ['Passport', 'Emirates ID'], r2_prefix: 'kyc-ae-', age_of_majority: 21 },
    IL: { name: 'Israel', natural_ids: ['Passport', 'Teudat Zehut', "Driver's licence"], r2_prefix: 'kyc-il-', age_of_majority: 18 },
    SA: { name: 'Saudi Arabia', natural_ids: ['Passport', 'National ID card'], r2_prefix: 'kyc-sa-', age_of_majority: 18 },
    ZA: { name: 'South Africa', natural_ids: ['Passport', 'Smart ID card', "Driver's licence"], r2_prefix: 'kyc-za-', age_of_majority: 18 },
    NG: { name: 'Nigeria', natural_ids: ['Passport', 'NIN slip', "Driver's licence"], r2_prefix: 'kyc-ng-', age_of_majority: 18, flag: 'enhanced_dd' },
    EG: { name: 'Egypt', natural_ids: ['Passport', 'National ID card'], r2_prefix: 'kyc-eg-', age_of_majority: 21 },
    KE: { name: 'Kenya', natural_ids: ['Passport', 'National ID card'], r2_prefix: 'kyc-ke-', age_of_majority: 18 },
  },
  fallback: { require: ['passport', 'national_id', 'drivers_license'] },
};

/// Effective minimum age for an applicant in `iso2`.
/// Never returns below MIN_AGE_FINANCE — a jurisdiction may only raise it.
function minimumAgeFor(iso2) {
  const j = JURISDICTIONS.jurisdictions[iso2];
  return Math.max(MIN_AGE_FINANCE, j?.age_of_majority || 0);
}

/// The calendar date on which someone born on `dob` reaches `minAge`.
///
/// Returned to the applicant so a block reads as "not yet" with a date rather
/// than a flat refusal, and so support can answer "when?" without doing the
/// arithmetic by hand.
function eligibleAtFrom(dob, minAge) {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec((dob || '').trim());
  if (!m) return null;
  return `${Number(m[1]) + minAge}-${m[2]}-${m[3]}`;
}

/// Gate for any money-touching action (linking a payout account, cashing out).
///
/// An applicant who already passed KYC forwards straight through to Stripe.
/// Re-verification is never demanded from someone already verified: the
/// documents are in R2 and the outcome is on the record, so asking again would
/// be pure friction.
///
/// Being under age is a NOT-YET, not a verdict. The resolved date of birth is
/// cached, and the age is recomputed on every call, so an account blocked at 16
/// becomes eligible on its 18th birthday with nobody doing anything. Caching
/// "underage" as a decision would lock the account out permanently.
///
/// The date of birth is resolved by strength of evidence:
///   1. `extracted_dob` — read off the document. Always wins when present.
///   2. `age_dob` — previously resolved and pinned to the record, so a later
///      edit to a self-declared birthday cannot move the bar.
///   3. `user.birthday` — declared at registration. Used for accounts verified
///      under the older flow, which never captured a document DOB.
///
/// Returns `{ ok: true, kyc }` or `{ ok: false, status, error, reason, code }`,
/// with `eligible_at` present when the block is only a matter of time.
async function requireVerifiedAdult(userId, user, env) {
  const key = `kyc-${userId}-front`;
  const raw = await env.KYC_STATUS.get(key);
  if (!raw) {
    return {
      ok: false, status: 403,
      error: 'Identity verification required',
      reason: 'Complete identity verification before using payout features.',
      code: 'kyc_required',
    };
  }

  const kyc = JSON.parse(raw);

  // 'linked' is what registration writes when it attaches a session to a user;
  // it preserves the original verification outcome in the same record.
  const passedDocs = kyc.status === 'verified' || kyc.status === 'linked';
  if (!passedDocs) {
    return {
      ok: false, status: 403,
      error: 'Identity verification incomplete',
      reason: 'Your identity documents have not been verified yet.',
      code: 'kyc_incomplete',
    };
  }

  // Once an adult, always an adult: this one is safe to cache permanently.
  if (kyc.age_verified === true) return { ok: true, kyc };

  const minAge = minimumAgeFor(kyc.iso2 || 'XX');
  const dob = kyc.extracted_dob || kyc.age_dob || user?.birthday || '';
  const source = kyc.extracted_dob ? 'document'
    : (kyc.age_dob ? kyc.age_source || 'pinned' : (user?.birthday ? 'declared' : null));
  const age = ageFromDob(dob);

  if (age === null) {
    return {
      ok: false, status: 403,
      error: 'Age verification required',
      reason: 'We could not confirm your date of birth. Please contact support@eustress.dev.',
      code: 'age_unverified',
    };
  }

  if (age < minAge) {
    const eligibleAt = eligibleAtFrom(dob, minAge);

    // Pin the date of birth, NOT the verdict, so the account re-evaluates on
    // every attempt and clears itself once the applicant is old enough.
    if (kyc.age_dob !== dob || kyc.age_status !== 'pending_age') {
      await env.KYC_STATUS.put(key, JSON.stringify({
        ...kyc,
        age_verified: false,
        age_status: 'pending_age',
        age_dob: dob,
        age_source: source,
        minimum_age: minAge,
        eligible_at: eligibleAt,
        age_checked_at: new Date().toISOString(),
      }), { expirationTtl: 86400 * 365 * 7 });
    }

    return {
      ok: false, status: 403,
      error: 'Payouts open at ' + minAge,
      reason: eligibleAt
        ? `Your account is verified. Payout features unlock on ${eligibleAt}, when you turn ${minAge}.`
        : `You must be at least ${minAge} to use payout features.`,
      code: 'underage',
      eligible_at: eligibleAt,
      minimum_age: minAge,
    };
  }

  // Old enough. Cache the pass so later calls take the fast path, and keep the
  // evidence source on the record so an auditor can see what it rested on.
  const resolved = {
    ...kyc,
    age_verified: true,
    age_status: 'verified',
    age_dob: dob,
    age_source: source,
    document_age: age,
    minimum_age: minAge,
    age_resolved_at: new Date().toISOString(),
  };
  await env.KYC_STATUS.put(key, JSON.stringify(resolved), { expirationTtl: 86400 * 365 * 7 });

  return { ok: true, kyc: resolved };
}

/// Whole years elapsed from `dob` (YYYY-MM-DD) to now, in UTC.
/// Returns null when the date is absent, malformed, or not a real calendar
/// date. Callers MUST treat null as "age unknown" and refuse, never approve.
function ageFromDob(dob) {
  if (typeof dob !== 'string') return null;
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(dob.trim());
  if (!m) return null;
  const [y, mo, d] = [Number(m[1]), Number(m[2]), Number(m[3])];
  const t = Date.UTC(y, mo - 1, d);
  const dt = new Date(t);
  // Reject dates that rolled over (e.g. 2009-02-30) or are impossible.
  if (dt.getUTCFullYear() !== y || dt.getUTCMonth() !== mo - 1 || dt.getUTCDate() !== d)
    return null;
  const now = new Date();
  if (t > now.getTime()) return null; // born in the future
  let age = now.getUTCFullYear() - y;
  const beforeBirthday =
    now.getUTCMonth() < mo - 1 ||
    (now.getUTCMonth() === mo - 1 && now.getUTCDate() < d);
  if (beforeBirthday) age -= 1;
  return age;
}

// Desktop gear icon (icon.png from `crates/engine/assets/`) embedded as
// base64. Served by `/assets/eustress-gear.png` so identity-registration
// emails can `<img src="...">` against a stable URL — Gmail strips
// inline `<svg>` tags as a security measure, so PNG-by-URL is the only
// reliable way to brand the email header.
import { ICON_GEAR_PNG_B64 } from './icon-gear.js';

// ─── Identity email — branded template ─────────────────────────────────────
// One canonical place to assemble the registration / identity-backup
// email so /api/auth/register's auto-send and /api/identity/email-backup
// share pixel-identical output. Emits HTML + plain-text bodies + an
// attachment descriptor for the multipart/mixed wrapper.
//
// Brand cues match `crates/web/src/components/footer.rs` (industrial
// dark theme, "Creation at the speed of thought." tagline, four-column
// nav) and `crates/engine/assets/icon.png` (the desktop gear). The
// gear ships as a hosted PNG (served by this same Worker at
// `/assets/eustress-gear.png`) — Gmail strips inline `<svg>` for
// security, so PNG-via-URL is the only reliable header-image path.
//
// Per-feedback layout choices (2026-04-25):
//   * Header has the gear above the "EUSTRESS ENGINE" wordmark, both
//     centred — matches the website footer's brand block.
//   * No inline TOML preview block; the body points the user to the
//     attached `.toml` file instead.
//   * Footer wordmark, tagline, social row, nav columns, copyright —
//     all centred. Status pill removed.
//   * Subject + copy reframed as "Registration", not "Backup".
function buildIdentityEmail({ username, toml_content, host }) {
  const safeName = (username || 'Creator').replace(/[<>"']/g, '');
  const filename = `eustress-${safeName}.toml`;
  // Public URL of the gear PNG. Defaults to the production hostname
  // — the `/assets/eustress-gear.png` route is bound to both
  // `api.eustress.dev/*` and the workers.dev URL, so either resolves.
  // Caller-supplied `host` (when the Worker is reached via HTTP) still
  // wins, keeping image origin == request origin during dev.
  const iconUrl = (host || 'https://api.eustress.dev') + '/assets/eustress-gear.png';

  const html = `<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1.0">
  <meta name="color-scheme" content="dark">
  <meta name="supported-color-schemes" content="dark">
  <title>Eustress Identity Registration</title>
</head>
<body style="margin:0;padding:0;background:#0d1117;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;color:#c9d1d9;">
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="background:#0d1117;">
    <tr>
      <td align="center" style="padding:32px 16px;">
        <table role="presentation" width="600" cellpadding="0" cellspacing="0" border="0" style="max-width:600px;width:100%;background:#161b22;border:1px solid #30363d;border-radius:14px;overflow:hidden;box-shadow:0 20px 60px rgba(0,0,0,0.5);">

          <!-- Header: gear PNG centred above "EUSTRESS ENGINE" wordmark.
               Mirrors the website footer's brand block so the same
               identity reads top-of-email and bottom-of-email. -->
          <tr>
            <td align="center" style="background:linear-gradient(135deg,#1a1f2e 0%,#0f1729 50%,#16213e 100%);padding:40px 32px 32px;text-align:center;border-bottom:1px solid #30363d;">
              <img src="${iconUrl}" width="80" height="80" alt="" style="display:block;margin:0 auto 18px;border:0;outline:none;text-decoration:none;">
              <div style="color:#f0f6fc;font-size:18px;font-weight:700;letter-spacing:0.12em;text-transform:uppercase;line-height:1;">Eustress Engine</div>
              <div style="color:#8b949e;font-size:12px;margin-top:8px;letter-spacing:0.02em;">Identity Registration</div>
            </td>
          </tr>

          <!-- Body -->
          <tr>
            <td style="padding:32px 32px 8px;">
              <p style="color:#f0f6fc;font-size:15px;margin:0 0 16px;line-height:1.5;">Welcome, <strong>${safeName}</strong>.</p>
              <p style="color:#8b949e;font-size:14px;margin:0 0 24px;line-height:1.6;">
                Your Eustress account is registered. Your identity file
                <code style="background:#21262d;padding:2px 6px;border-radius:4px;color:#79c0ff;font-family:'SF Mono',Consolas,Monaco,monospace;font-size:12px;">${filename}</code>
                is attached to this email — it contains your Ed25519 private key, the cryptographic root of the account. Treat it like a password-manager vault key: store it securely, back it up off-device, and never share it.
              </p>

              <!-- Centred call-to-action pointing at the attachment dock.
                   Replaces the inline TOML preview block so the user
                   downloads the real .toml file from this email
                   instead of copy-pasting from a code block. -->
              <table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="margin:0 0 26px;">
                <tr>
                  <td align="center" style="background:linear-gradient(135deg,#1a2a3a 0%,#1e2a40 100%);border:1px solid #2a3a4a;border-radius:10px;padding:24px 20px;">
                    <div style="color:#8ab4f8;font-size:11px;font-weight:700;letter-spacing:0.12em;text-transform:uppercase;margin:0 0 8px;">Your Identity File</div>
                    <div style="color:#f0f6fc;font-size:15px;font-family:'SF Mono',Consolas,Monaco,monospace;margin:0 0 4px;">${filename}</div>
                    <div style="color:#8b949e;font-size:12px;line-height:1.5;">Download from the attachment dock.</div>
                  </td>
                </tr>
              </table>

              <div style="background:#0d1117;border:1px solid #30363d;border-radius:8px;padding:18px 20px;margin:0 0 22px;">
                <div style="color:#c9d1d9;font-size:13px;font-weight:600;letter-spacing:0.02em;margin:0 0 10px;">How to use your registration</div>
                <ol style="color:#8b949e;font-size:13px;margin:0;padding-left:20px;line-height:1.8;">
                  <li>Save the attached <code style="background:#21262d;padding:1px 5px;border-radius:3px;color:#79c0ff;font-family:'SF Mono',Consolas,Monaco,monospace;font-size:11px;">${filename}</code> on the device(s) you sign in from.</li>
                  <li>Sign in at <a href="https://eustress.dev/login" style="color:#58a6ff;text-decoration:none;border-bottom:1px solid #2a4a6a;">eustress.dev/login</a></li>
                  <li>Or load it in EustressEngine via the Sign In dialog.</li>
                </ol>
              </div>

              <p style="color:#f85149;font-size:12px;margin:0 0 6px;text-align:center;font-weight:500;">
                ⚠ Never share your <code style="background:rgba(248,81,73,0.1);padding:1px 5px;border-radius:3px;color:#ff7b72;font-family:'SF Mono',Consolas,Monaco,monospace;font-size:11px;">private_key</code> with anyone.
              </p>
              <p style="color:#6e7681;font-size:11px;margin:0;text-align:center;line-height:1.5;">
                Eustress staff will never ask for it. If you receive a request claiming otherwise, it is fraudulent.
              </p>
            </td>
          </tr>

          <!-- Footer brand block — mirrors the website footer. Everything
               here is centred per design feedback (top + footer share the
               same EUSTRESS ENGINE wordmark, both centred). -->
          <tr>
            <td align="center" style="padding:30px 32px 14px;border-top:1px solid #30363d;background:#0d1117;text-align:center;">
              <div style="color:#f0f6fc;font-size:13px;font-weight:700;letter-spacing:0.10em;text-transform:uppercase;margin:0 0 8px;">Eustress Engine</div>
              <div style="color:#6e7681;font-size:12px;margin:0 0 18px;">Creation at the speed of thought.</div>
              <div>
                <a href="https://twitter.com/eustressengine" style="color:#c9d1d9;font-size:12px;text-decoration:none;letter-spacing:0.02em;padding:0 10px;">X</a>
                <span style="color:#30363d;">·</span>
                <a href="https://discord.gg/DGP9my8DYN" style="color:#c9d1d9;font-size:12px;text-decoration:none;letter-spacing:0.02em;padding:0 10px;">Discord</a>
                <span style="color:#30363d;">·</span>
                <a href="https://github.com/WeaveITMeta/EustressEngine" style="color:#c9d1d9;font-size:12px;text-decoration:none;letter-spacing:0.02em;padding:0 10px;">GitHub</a>
              </div>
            </td>
          </tr>

          <!-- Footer nav columns — centred grid, each column header
               centred over centred links. Outlook desktop renders nested
               tables fine; this stays predictable across clients. -->
          <tr>
            <td style="padding:8px 16px 24px;background:#0d1117;">
              <table role="presentation" align="center" cellpadding="0" cellspacing="0" border="0" style="margin:0 auto;">
                <tr>
                  <td valign="top" align="center" style="padding:0 14px;">
                    <p style="color:#8b949e;font-size:10px;font-weight:700;text-transform:uppercase;letter-spacing:0.10em;margin:0 0 8px;text-align:center;">Product</p>
                    <p style="margin:0 0 6px;text-align:center;"><a href="https://eustress.dev/gallery" style="color:#c9d1d9;text-decoration:none;font-size:12px;">Gallery</a></p>
                    <p style="margin:0 0 6px;text-align:center;"><a href="https://eustress.dev/learn" style="color:#c9d1d9;text-decoration:none;font-size:12px;">Learn</a></p>
                    <p style="margin:0 0 6px;text-align:center;"><a href="https://eustress.dev/bliss" style="color:#c9d1d9;text-decoration:none;font-size:12px;">Bliss</a></p>
                    <p style="margin:0;text-align:center;"><a href="https://eustress.dev/premium" style="color:#c9d1d9;text-decoration:none;font-size:12px;">Premium</a></p>
                  </td>
                  <td valign="top" align="center" style="padding:0 14px;">
                    <p style="color:#8b949e;font-size:10px;font-weight:700;text-transform:uppercase;letter-spacing:0.10em;margin:0 0 8px;text-align:center;">Community</p>
                    <p style="margin:0 0 6px;text-align:center;"><a href="https://eustress.dev/groups" style="color:#c9d1d9;text-decoration:none;font-size:12px;">Groups</a></p>
                    <p style="margin:0 0 6px;text-align:center;"><a href="https://discord.gg/DGP9my8DYN" style="color:#c9d1d9;text-decoration:none;font-size:12px;">Discord</a></p>
                    <p style="margin:0 0 6px;text-align:center;"><a href="https://x.com/search?q=%23EustressEngine" style="color:#c9d1d9;text-decoration:none;font-size:12px;">X</a></p>
                    <p style="margin:0;text-align:center;"><a href="https://x.com/simbuilder" style="color:#c9d1d9;text-decoration:none;font-size:12px;">Forums</a></p>
                  </td>
                  <td valign="top" align="center" style="padding:0 14px;">
                    <p style="color:#8b949e;font-size:10px;font-weight:700;text-transform:uppercase;letter-spacing:0.10em;margin:0 0 8px;text-align:center;">Company</p>
                    <p style="margin:0 0 6px;text-align:center;"><a href="https://eustress.dev/about" style="color:#c9d1d9;text-decoration:none;font-size:12px;">About</a></p>
                    <p style="margin:0 0 6px;text-align:center;"><a href="https://eustress.dev/careers" style="color:#c9d1d9;text-decoration:none;font-size:12px;">Careers</a></p>
                    <p style="margin:0 0 6px;text-align:center;"><a href="https://eustress.dev/contact" style="color:#c9d1d9;text-decoration:none;font-size:12px;">Contact</a></p>
                    <p style="margin:0;text-align:center;"><a href="https://eustress.dev/press" style="color:#c9d1d9;text-decoration:none;font-size:12px;">Press Kit</a></p>
                  </td>
                  <td valign="top" align="center" style="padding:0 14px;">
                    <p style="color:#8b949e;font-size:10px;font-weight:700;text-transform:uppercase;letter-spacing:0.10em;margin:0 0 8px;text-align:center;">Legal</p>
                    <p style="margin:0 0 6px;text-align:center;"><a href="https://eustress.dev/terms" style="color:#c9d1d9;text-decoration:none;font-size:12px;">Terms</a></p>
                    <p style="margin:0 0 6px;text-align:center;"><a href="https://eustress.dev/privacy" style="color:#c9d1d9;text-decoration:none;font-size:12px;">Privacy</a></p>
                    <p style="margin:0 0 6px;text-align:center;"><a href="https://eustress.dev/cookies" style="color:#c9d1d9;text-decoration:none;font-size:12px;">Cookies</a></p>
                    <p style="margin:0;text-align:center;"><a href="https://eustress.dev/acts" style="color:#c9d1d9;text-decoration:none;font-size:12px;">Acts</a></p>
                  </td>
                </tr>
              </table>
            </td>
          </tr>

          <!-- Bottom bar — centred copyright only; status pill removed
               per design feedback. -->
          <tr>
            <td align="center" style="padding:14px 32px 22px;background:#0d1117;border-top:1px solid #30363d;text-align:center;">
              <span style="color:#484f58;font-size:11px;">© 2025 Eustress Engine. All rights reserved.</span>
            </td>
          </tr>

        </table>
      </td>
    </tr>
  </table>
</body>
</html>`;

  const text =
    `EUSTRESS ENGINE — Identity Registration\n\n` +
    `Welcome, ${safeName}.\n\n` +
    `Your Eustress account is registered. Your identity file (${filename}) is attached to this email.\n` +
    `It contains your Ed25519 private key — store it securely, back it up off-device, and never share it.\n\n` +
    `Download the attached ${filename} from this email to use it.\n\n` +
    `How to use your registration:\n` +
    `  1. Save the attached ${filename} on the device(s) you sign in from.\n` +
    `  2. Sign in at https://eustress.dev/login\n` +
    `  3. Or load it in EustressEngine via the Sign In dialog.\n\n` +
    `WARNING: Never share your private_key with anyone. Eustress staff will never ask for it.\n\n` +
    `— Eustress Engine\n` +
    `Creation at the speed of thought.\n` +
    `https://eustress.dev`;

  return {
    subject: `Your Eustress Identity Registration — ${safeName}`,
    html,
    text,
    attachment: {
      filename,
      mime: 'application/toml',
      content: toml_content,
    },
  };
}

// ─── Hand-built MIME helpers ────────────────────────────────────────────────
// Workers' `EmailMessage(from, to, raw)` constructor takes a full RFC 5322
// message as a single string. We build that string here instead of pulling
// in `mimetext` — it's an npm package that needs to be bundled into the
// Worker, and a missed `npm install` produces the silent-but-fatal
// "No such module 'mimetext'" runtime error we hit on first deploy.
// Hand-rolled keeps the path dependency-free.

/**
 * Build an RFC 5322 multipart/alternative message (HTML + plain-text
 * fallback). Both parts MUST end in CRLF; the boundary token MUST NOT
 * appear in either body.
 */
function buildMimeAlternative({ from, to, subject, html, text }) {
  const boundary = '----=_eustress_alt_' + Math.random().toString(36).slice(2);
  return (
    `From: Eustress Identity <${from}>\r\n` +
    `To: ${to}\r\n` +
    `Subject: ${subject}\r\n` +
    `MIME-Version: 1.0\r\n` +
    `Content-Type: multipart/alternative; boundary="${boundary}"\r\n` +
    `\r\n` +
    `--${boundary}\r\n` +
    `Content-Type: text/plain; charset=utf-8\r\n` +
    `Content-Transfer-Encoding: 7bit\r\n` +
    `\r\n` +
    `${text}\r\n` +
    `\r\n` +
    `--${boundary}\r\n` +
    `Content-Type: text/html; charset=utf-8\r\n` +
    `Content-Transfer-Encoding: 7bit\r\n` +
    `\r\n` +
    `${html}\r\n` +
    `\r\n` +
    `--${boundary}--\r\n`
  );
}

/**
 * Build a simple plain-text RFC 5322 message. Use when there's no
 * HTML body — keeps the structure flat (no multipart) which simplifies
 * what gets relayed and what spam filters see.
 */
function buildMimePlain({ from, to, subject, text }) {
  return (
    `From: Eustress Identity <${from}>\r\n` +
    `To: ${to}\r\n` +
    `Subject: ${subject}\r\n` +
    `MIME-Version: 1.0\r\n` +
    `Content-Type: text/plain; charset=utf-8\r\n` +
    `Content-Transfer-Encoding: 7bit\r\n` +
    `\r\n` +
    `${text}\r\n`
  );
}

/**
 * Wrap base64 to 76 cols + CRLF per RFC 2045 §6.8 — long unwrapped
 * runs trip strict MTAs that enforce the 998-octet line limit.
 */
function wrapBase64(b64, width = 76) {
  let out = '';
  for (let i = 0; i < b64.length; i += width) {
    out += b64.slice(i, i + width);
    if (i + width < b64.length) out += '\r\n';
  }
  return out;
}

/**
 * Build a multipart/mixed message with a single file attachment plus
 * a multipart/alternative body (HTML + plain-text fallback). The
 * attachment is base64-encoded with the supplied filename + MIME type.
 *
 * Structure:
 *   multipart/mixed
 *     ├─ multipart/alternative
 *     │    ├─ text/plain
 *     │    └─ text/html
 *     └─ <attachmentMime> (Content-Disposition: attachment)
 */
function buildMimeWithAttachment({
  from,
  to,
  subject,
  html,
  text,
  attachment, // { filename, mime, content } — content is utf-8 string
}) {
  const mixedBoundary = '----=_eustress_mixed_' + Math.random().toString(36).slice(2);
  const altBoundary = '----=_eustress_alt_' + Math.random().toString(36).slice(2);
  const b64 = btoa(unescape(encodeURIComponent(attachment.content)));
  const wrapped = wrapBase64(b64);

  return (
    `From: Eustress Identity <${from}>\r\n` +
    `To: ${to}\r\n` +
    `Subject: ${subject}\r\n` +
    `MIME-Version: 1.0\r\n` +
    `Content-Type: multipart/mixed; boundary="${mixedBoundary}"\r\n` +
    `\r\n` +
    `--${mixedBoundary}\r\n` +
    `Content-Type: multipart/alternative; boundary="${altBoundary}"\r\n` +
    `\r\n` +
    `--${altBoundary}\r\n` +
    `Content-Type: text/plain; charset=utf-8\r\n` +
    `Content-Transfer-Encoding: 7bit\r\n` +
    `\r\n` +
    `${text}\r\n` +
    `\r\n` +
    `--${altBoundary}\r\n` +
    `Content-Type: text/html; charset=utf-8\r\n` +
    `Content-Transfer-Encoding: 7bit\r\n` +
    `\r\n` +
    `${html}\r\n` +
    `\r\n` +
    `--${altBoundary}--\r\n` +
    `\r\n` +
    `--${mixedBoundary}\r\n` +
    `Content-Type: ${attachment.mime}; name="${attachment.filename}"\r\n` +
    `Content-Disposition: attachment; filename="${attachment.filename}"\r\n` +
    `Content-Transfer-Encoding: base64\r\n` +
    `\r\n` +
    `${wrapped}\r\n` +
    `\r\n` +
    `--${mixedBoundary}--\r\n`
  );
}

export default {
  // UTC-midnight cron: BLS emission distribution first (mints against
  // yesterday's score snapshot), then the USD treasury drip payout
  // (same snapshot). Sequential — both read the same day's scores.
  async scheduled(event, env, ctx) {
    ctx.waitUntil((async () => {
      let distribution = null;
      let payout = null;
      let backup = null;
      let models = null;
      let failed = [];
      try { distribution = await runDailyDistribution(env); }
      catch (e) { failed.push(`distribution: ${e.message}`); console.error('distribution failed:', e); }
      try { payout = await runDailyPayout(env); }
      catch (e) { failed.push(`payout: ${e.message}`); console.error('payout failed:', e); }
      // Backup LAST so it captures the state this run produced.
      try { backup = await backupLedger(env); }
      catch (e) { failed.push(`backup: ${e.message}`); console.error('backup failed:', e); }

      // Model catalog refresh. Independent of the ledger work above — it
      // shares only the schedule — so it runs on its own try/catch and a
      // failed payout never costs us a day of model currency. It keeps its
      // own detailed record in MODELS:run:{date}; this is just the summary
      // line so /api/admin/cron-health shows the whole night at a glance.
      try { models = await runModelDiscovery(env); }
      catch (e) { failed.push(`models: ${e.message}`); console.error('model discovery failed:', e); }

      // Durable run record. Cron failures used to vanish into console.error
      // with nothing queryable afterwards, so a silently skipped day was
      // invisible. `/api/admin/cron-health` reads these.
      const today = new Date().toISOString().split('T')[0];
      await env.PAYOUTS.put(`cronrun:${today}`, JSON.stringify({
        ran_at: new Date().toISOString(),
        ok: failed.length === 0,
        failed,
        minted_minor: distribution?.minted_minor ?? 0,
        contributors: distribution?.contributor_count ?? 0,
        truncated: distribution?.truncated ?? false,
        concentration_flag: distribution?.concentration_flag ?? false,
        paid_usd: payout?.total_paid_usd ?? 0,
        backup_key: backup?.key ?? null,
        models_applied: models?.applied ?? false,
        models_version: models?.version ?? models?.kept_version ?? null,
        models_changes: models?.changes ?? [],
      }), { expirationTtl: 86400 * 365 });
    })());
  },

  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    const cors = corsHeaders(request);

    if (request.method === 'OPTIONS') {
      // The manifest routes are read by sites we do not run, so their preflight
      // answers `*` instead of the ALLOWED_ORIGINS allowlist. This carve-out has
      // to live here rather than in the handler: the short-circuit runs before
      // route dispatch, and the custom X-Eustress-Key header guarantees a
      // preflight, so without it the browser rejects the request before the
      // handler is ever reached.
      if (isWebsiteManifestPath(url.pathname)) {
        return new Response(null, { status: 204, headers: manifestCorsHeaders() });
      }
      return new Response(null, { status: 204, headers: cors });
    }

    try {
      // Auth
      if (url.pathname === '/api/auth/register' && request.method === 'POST')
        return handleRegister(request, env, cors);
      if (url.pathname === '/api/auth/challenge' && request.method === 'POST')
        return handleChallenge(request, env, cors);
      if (url.pathname === '/api/auth/verify-challenge' && request.method === 'POST')
        return handleVerify(request, env, cors);
      if (url.pathname === '/api/auth/me' && request.method === 'GET')
        return handleMe(request, env, cors);

      // Identity backup email
      if (url.pathname === '/api/identity/email-backup' && request.method === 'POST')
        return handleEmailIdentityBackup(request, env, cors);

      // Workshop Context (persistent memories + rules across devices)
      if (url.pathname === '/api/workshop/context' && request.method === 'GET')
        return handleGetWorkshopContext(request, env, cors);
      if (url.pathname === '/api/workshop/context' && request.method === 'PUT')
        return handlePutWorkshopContext(request, env, cors);
      if (url.pathname === '/api/workshop/context/memory' && request.method === 'POST')
        return handleAddWorkshopMemory(request, env, cors);

      // KYC
      if (url.pathname === '/api/kyc/jurisdiction')
        return handleJurisdiction(request, env, cors);
      if (url.pathname === '/api/kyc/upload' && request.method === 'POST')
        return handleKycUpload(request, env, cors);
      if (url.pathname === '/api/kyc/submit' && request.method === 'POST')
        return handleKycSubmit(request, env, cors);
      if (url.pathname.startsWith('/api/kyc/status/'))
        return handleKycStatus(url.pathname.split('/').pop(), env, cors);
      // Desktop → mobile handoff (QR code flow)
      if (url.pathname === '/api/kyc/handoff' && request.method === 'POST')
        return handleKycHandoffCreate(request, env, cors);
      if (url.pathname.startsWith('/api/kyc/handoff/'))
        return handleKycHandoffGet(url.pathname.split('/').pop(), env, cors);
      if (url.pathname.startsWith('/api/kyc/session/'))
        return handleKycSessionStatus(url.pathname.split('/').pop(), env, cors);

      // Co-sign
      if (url.pathname === '/api/cosign' && request.method === 'POST')
        return handleCosign(request, env, cors);

      // Community
      if (url.pathname === '/api/community/stats')
        return handleCommunityStats(env, cors);
      if (url.pathname === '/api/community/search' && request.method === 'GET')
        return handleCommunitySearch(request, env, cors);
      if (url.pathname === '/api/community/leaderboard')
        return handleCommunityLeaderboard(env, cors);
      if (url.pathname.startsWith('/api/community/users/') && request.method === 'GET')
        return handleUserProfile(url.pathname.split('/').pop(), request, env, cors);

      // Social (authenticated)
      if (url.pathname === '/api/social/follow' && request.method === 'POST')
        return handleFollow(request, env, cors);
      if (url.pathname === '/api/social/unfollow' && request.method === 'POST')
        return handleUnfollow(request, env, cors);
      if (url.pathname === '/api/social/favorite' && request.method === 'POST')
        return handleFavorite(request, env, cors);
      if (url.pathname === '/api/social/unfavorite' && request.method === 'POST')
        return handleUnfavorite(request, env, cors);
      if (url.pathname === '/api/social/play' && request.method === 'POST')
        return handlePlay(request, env, cors);

      // Inventory
      if (url.pathname === '/api/inventory' && request.method === 'GET')
        return handleGetInventory(request, env, cors);

      // Screening (AI risk assessment)
      if (url.pathname === '/api/screening/check' && request.method === 'POST')
        return handleScreeningCheck(request, env, cors);
      if (url.pathname === '/api/screening/status' && request.method === 'GET')
        return handleScreeningStatus(request, env, cors);

      // Admin (requires admin JWT)
      if (url.pathname === '/api/admin/users' && request.method === 'GET')
        return handleAdminListUsers(request, env, cors);
      if (url.pathname === '/api/admin/ban' && request.method === 'POST')
        return handleAdminBan(request, env, cors);
      if (url.pathname === '/api/admin/warn' && request.method === 'POST')
        return handleAdminWarn(request, env, cors);
      if (url.pathname === '/api/admin/review' && request.method === 'POST')
        return handleAdminReview(request, env, cors);
      if (url.pathname === '/api/admin/risk-override' && request.method === 'POST')
        return handleAdminRiskOverride(request, env, cors);
      if (url.pathname === '/api/admin/rescreen' && request.method === 'POST')
        return handleAdminRescreen(request, env, cors);
      if (url.pathname === '/api/admin/delete-user' && request.method === 'POST')
        return handleAdminDeleteUser(request, env, cors);
      if (url.pathname === '/api/admin/screening-report' && request.method === 'GET')
        return handleAdminScreeningReport(request, env, cors);
      if (url.pathname === '/api/admin/stats' && request.method === 'GET')
        return handleAdminStats(request, env, cors);

      // Public, cookieless pageview beacon (feeds the admin funnel)
      // Engine usage telemetry (anonymous tool-click aggregates + feedback).
      if (url.pathname === '/api/telemetry/usage' && request.method === 'POST')
        return handleTelemetryUsage(request, env, cors);
      if (url.pathname === '/api/telemetry/comment' && request.method === 'POST')
        return handleTelemetryComment(request, env, cors);
      if (url.pathname === '/api/telemetry/summary' && request.method === 'GET')
        return handleTelemetrySummary(request, env, cors);

      if (url.pathname === '/api/analytics/hit' && request.method === 'POST')
        return handleAnalyticsHit(request, env, cors);

      // Tickets
      if (url.pathname === '/api/tickets/packages' && request.method === 'GET')
        return handleTicketPackages(env, cors);
      if (url.pathname === '/api/tickets/balance' && request.method === 'GET')
        return handleTicketBalance(request, env, cors);
      if (url.pathname === '/api/tickets/checkout' && request.method === 'POST')
        return handleTicketCheckout(request, env, cors);
      if (url.pathname === '/api/tickets/spend' && request.method === 'POST')
        return handleTicketSpend(request, env, cors);
      if (url.pathname === '/api/tickets/history' && request.method === 'GET')
        return handleTicketHistory(request, env, cors);

      // Stripe
      if (url.pathname === '/api/stripe/checkout' && request.method === 'POST')
        return handleStripeCheckout(request, env, cors);
      if (url.pathname === '/api/stripe/webhook' && request.method === 'POST')
        return handleStripeWebhook(request, env);
      if (url.pathname === '/api/stripe/connect/onboard' && request.method === 'POST')
        return handleStripeConnectOnboard(request, env, cors);
      if (url.pathname === '/api/stripe/connect/status' && request.method === 'GET')
        return handleStripeConnectStatus(request, env, cors);

      // Payouts
      if (url.pathname === '/api/payouts/daily' && request.method === 'POST')
        return handleDailyPayout(request, env, cors);
      if (url.pathname === '/api/payouts/history' && request.method === 'GET')
        return handlePayoutHistory(request, env, cors);
      if (url.pathname === '/api/payouts/rate' && request.method === 'GET')
        return handlePayoutRate(env, cors);

      // Ledger transparency (public, read-only, wildcard CORS) + health.
      // These three take `publicCors()` so browsers on any origin can audit
      // them; /api/ledger/me below stays origin-pinned AND bearer-gated.
      if (url.pathname === '/api/ledger/summary' && request.method === 'GET')
        return handleLedgerSummary(env, publicCors());
      if (url.pathname.startsWith('/api/ledger/distribution/') && request.method === 'GET')
        return handleLedgerDistribution(url.pathname.split('/').pop(), env, publicCors());
      if (url.pathname.startsWith('/api/ledger/history/') && request.method === 'GET')
        return handleLedgerHistory(url.pathname.split('/').pop(), env, publicCors());
      if (url.pathname === '/api/ledger/spend' && request.method === 'POST')
        return handleLedgerSpend(request, env, cors);
      if (url.pathname === '/api/ledger/me' && request.method === 'GET')
        return handleLedgerMe(request, env, cors);
      if (url.pathname === '/api/admin/cron-health' && request.method === 'GET')
        return handleCronHealth(request, env, cors);

      // Model catalog — the Workshop model picker's list, recompiled daily.
      // The read is public: it is public model names at public list prices,
      // and gating it would only push a signed-out engine onto its seed.
      if (url.pathname === '/api/models/catalog' && request.method === 'GET')
        return handleModelCatalog(request, env, cors);
      if (url.pathname === '/api/admin/models' && request.method === 'GET')
        return handleAdminModelRuns(request, env, cors);
      if (url.pathname === '/api/admin/models/refresh' && request.method === 'POST')
        return handleAdminModelRefresh(request, env, cors);
      if (url.pathname === '/api/admin/models/rollback' && request.method === 'POST')
        return handleAdminModelRollback(request, env, cors);

      // Node heartbeat
      if (url.pathname === '/api/node/heartbeat' && request.method === 'POST')
        return handleNodeHeartbeat(request, env, cors);
      if (url.pathname === '/api/node/stats' && request.method === 'GET')
        return handleNodeStats(env, cors);

      // Simulations (published)
      if (url.pathname === '/api/simulations' && request.method === 'GET')
        return handleListSimulations(env, cors);
      if (url.pathname === '/api/simulations/publish' && request.method === 'POST')
        return handlePublishSimulation(request, env, cors);
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+\/space$/) && request.method === 'PUT')
        return handleUploadScene(request, url.pathname.split('/')[3], env, cors);
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+\/space\/multipart\/create$/) && request.method === 'POST')
        return handleMultipartCreate(request, url.pathname.split('/')[3], env, cors);
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+\/space\/multipart\/part$/) && request.method === 'PUT')
        return handleMultipartPart(request, url.pathname.split('/')[3], env, cors);
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+\/space\/multipart\/complete$/) && request.method === 'POST')
        return handleMultipartComplete(request, url.pathname.split('/')[3], env, cors);
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+\/spaces\/[^/]+$/) && request.method === 'PUT')
        return handleUploadSingleSpace(request, url.pathname.split('/')[3], url.pathname.split('/')[5], env, cors);
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+\/thumbnail$/) && request.method === 'PUT')
        return handleUploadThumbnail(request, url.pathname.split('/')[3], env, cors);
      // Read side of the thumbnail, ported from the retired eustress-simulations
      // worker. Without it nothing served thumbnails/{id}/thumb.{ext} at all.
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+\/thumbnail$/) && request.method === 'GET')
        return handleGetThumbnail(url.pathname.split('/')[3], env, cors);
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+\/download$/) && request.method === 'GET')
        return handleDownloadPak(request, url.pathname.split('/')[3], env, cors);
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+\/play$/) && request.method === 'POST')
        return handlePlaySimulation(request, url.pathname.split('/')[3], env, cors);
      // Website manifest upload. Its own object and its own route, so a publish
      // can never overwrite the listing record the marketplace reads.
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+\/website-manifest$/) && request.method === 'PUT')
        return handlePutWebsiteManifest(request, url.pathname.split('/')[3], env, cors);
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+$/) && request.method === 'GET')
        return handleGetSimulation(url.pathname.split('/').pop(), env, cors);

      // Website manifest reads. Singular `/api/simulation/` on purpose: the
      // segment may be a namespace rather than a UUID, and a namespace spelled
      // in hex ("decade", "beef") would otherwise be swallowed by the
      // `/api/simulations/[a-f0-9-]+` route above. Both forms end in /manifest.
      if (url.pathname.match(/^\/api\/simulation\/[^/]+\/latest\/manifest$/) && request.method === 'GET')
        return handleGetWebsiteManifest(request, url, decodeURIComponent(url.pathname.split('/')[3]), env);
      if (url.pathname.match(/^\/api\/simulation\/[^/]+\/manifest$/) && request.method === 'GET')
        return handleGetWebsiteManifest(request, url, decodeURIComponent(url.pathname.split('/')[3]), env);

      // Accounting
      if (url.pathname === '/api/accounting/dashboard' && request.method === 'GET')
        return handleAccountingDashboard(request, env, cors);
      if (url.pathname === '/api/accounting/costs' && request.method === 'POST')
        return handleRecordCost(request, env, cors);

      // Gallery (frontend-facing aliases for simulations)
      if (url.pathname === '/api/gallery/featured' && request.method === 'GET')
        return json({ featured: [], timestamp: new Date().toISOString() }, 200, cors);
      if (url.pathname === '/api/gallery' && request.method === 'GET')
        return handleListSimulations(env, cors);

      // API Keys management
      if (url.pathname === '/api/keys' && request.method === 'GET')
        return json({ keys: [] }, 200, cors);
      if (url.pathname === '/api/keys' && request.method === 'POST')
        return json({ key: 'ek_' + crypto.randomUUID().replace(/-/g, ''), id: crypto.randomUUID(), name: 'New Key' }, 201, cors);

      // Marketplace (stub — not yet implemented)
      if (url.pathname.startsWith('/api/marketplace'))
        return json({ items: [], total: 0, page: 1 }, 200, cors);

      // Projects — returns the authenticated user's published simulations
      if (url.pathname === '/api/projects' && request.method === 'GET') {
        return handleUserProjects(request, url, env, cors);
      }
      if (url.pathname === '/api/projects/recent' && request.method === 'GET') {
        return handleUserProjects(request, url, env, cors);
      }

      // Health
      if (url.pathname === '/health')
        return handleHealth(env, cors);

      // Branded asset for outbound emails. Gmail blocks remote images
      // until the user clicks "show images"; once allowed, this URL
      // is cached server-side per-recipient. Year-long Cache-Control
      // is safe because the icon is content-addressed by route.
      if (url.pathname === '/assets/eustress-gear.png' && request.method === 'GET') {
        const bytes = Uint8Array.from(atob(ICON_GEAR_PNG_B64), c => c.charCodeAt(0));
        return new Response(bytes, {
          headers: {
            'Content-Type': 'image/png',
            'Cache-Control': 'public, max-age=31536000, immutable',
            'Access-Control-Allow-Origin': '*',
          },
        });
      }

      return json({ error: 'Not found' }, 404, cors);
    } catch (err) {
      return json({ error: err.message }, 500, cors);
    }
  }
};

// ═══════════════════════════════════════════════════════════════════════════
// AUTH
// ═══════════════════════════════════════════════════════════════════════════

async function handleRegister(request, env, cors) {
  const body = await request.json();
  const { username, public_key, birthday, id_type, id_hash, kyc_session_id, email, toml_content } = body;

  if (!username || username.length < 3 || username.length > 32)
    return json({ error: 'Username must be 3-32 characters' }, 400, cors);
  if (!/^[a-zA-Z0-9_]+$/.test(username))
    return json({ error: 'Username: letters, numbers, and _ only' }, 400, cors);
  if (!public_key || public_key.length < 32)
    return json({ error: 'Invalid public key' }, 400, cors);

  // Check username taken
  const existingByName = await env.USERS.get(`username:${username}`);
  if (existingByName) return json({ error: 'Username already taken' }, 409, cors);

  // Check public key already registered
  const existingByKey = await env.USERS.get(`pubkey:${public_key}`);
  if (existingByKey) return json({ error: 'Public key already registered' }, 409, cors);

  // Check ID hash (Sybil protection)
  if (id_hash) {
    const existingByHash = await env.USERS.get(`idhash:${id_hash}`);
    if (existingByHash) return json({ error: 'This ID has already been used to register' }, 409, cors);
  }

  // ── Age gate ─────────────────────────────────────────────────────────────
  // Two layers. First the self-declared birthday, which catches the obvious
  // case cheaply. Then, when a KYC session exists, the document-verified age,
  // which is the one that actually counts.
  const regCountry = request.headers.get('cf-ipcountry') || 'XX';
  let kycRecord = null;
  if (kyc_session_id) {
    const raw = await env.KYC_STATUS.get(`kyc-${kyc_session_id}-front`);
    if (raw) kycRecord = JSON.parse(raw);
  }
  const regMinAge = minimumAgeFor(kycRecord?.iso2 || regCountry);

  const declaredAge = ageFromDob(birthday);
  if (declaredAge === null)
    return json({ error: 'A valid date of birth (YYYY-MM-DD) is required' }, 400, cors);
  if (declaredAge < regMinAge)
    return json({
      error: `You must be at least ${regMinAge} years old to create an account`,
      code: 'underage', minimum_age: regMinAge,
    }, 403, cors);

  // A client that supplies a KYC session must supply one that actually passed.
  // Without this the browser could simply skip the upload step and register.
  if (kyc_session_id) {
    if (!kycRecord)
      return json({ error: 'Identity verification session not found', code: 'kyc_missing' }, 400, cors);
    if (kycRecord.status !== 'verified')
      return json({ error: 'Identity verification has not passed', code: 'kyc_incomplete' }, 403, cors);
    if (kycRecord.age_verified !== true)
      return json({
        error: kycRecord.age_status === 'underage'
          ? `You must be at least ${kycRecord.minimum_age || regMinAge} years old to create an account`
          : 'Your age could not be confirmed from your identity document',
        code: kycRecord.age_status === 'underage' ? 'underage' : 'age_unverified',
        minimum_age: kycRecord.minimum_age || regMinAge,
      }, 403, cors);
  }

  // AI Screening — reuse the result from KYC submit (single Grok call did
  // document verification + OCR + criminal screening together).
  // Only call performScreening as a fallback if KYC submit wasn't done.
  let screening = null;
  if (kyc_session_id) {
    const kycScreening = await env.SCREENING.get(`screen:session-${kyc_session_id}`);
    if (kycScreening) screening = JSON.parse(kycScreening);
  }
  if (!screening && env.GROK_API_KEY && username && birthday) {
    // Fallback: no KYC session or KYC submit wasn't called — run standalone screening
    screening = await performScreening(username, birthday, id_type, id_hash, env);
  }
  if (screening) {
    const screeningKey = `screen:${id_hash || public_key}`;
    await env.SCREENING.put(screeningKey, JSON.stringify(screening), { expirationTtl: 86400 * 365 * 7 });

    if (screening.decision === 'DENY') {
      return json({
        error: 'Registration denied based on background screening',
        risk_score: screening.risk_score,
        reason: screening.reason,
        appeal: 'Contact support@eustress.dev to appeal this decision',
      }, 403, cors);
    }
    // REVIEW: allow registration but flag for manual review
  }

  const user_id = crypto.randomUUID();
  const now = new Date().toISOString();

  // Get screening result if it was performed
  const screeningResult = (env.GROK_API_KEY && (id_hash || public_key))
    ? JSON.parse(await env.SCREENING.get(`screen:${id_hash || public_key}`) || 'null')
    : null;

  const user = {
    id: user_id,
    username,
    public_key,
    birthday: birthday || null,
    id_type: id_type || null,
    id_hash: id_hash || null,
    bliss_balance: 0,
    created_at: now,
    email: email || null,
    // Identity + age verification state. `age_verified` is true only when a
    // government document was read and its date of birth cleared the minimum,
    // so payout features can gate on it without re-deriving anything.
    kyc_verified: kycRecord?.status === 'verified',
    age_verified: kycRecord?.age_verified === true,
    verified_dob: kycRecord?.extracted_dob || null,
    minimum_age: regMinAge,
    avatar_url: null,
    discord_id: null,
    // Screening
    risk_score: screeningResult?.risk_score || 0,
    risk_decision: screeningResult?.decision || 'UNSCREENED',
    last_screened: screeningResult?.screened_at || null,
    screening_flags: screeningResult?.flags || [],
    banned: false,
    ban_reason: null,
    warnings: [],
  };

  // Store user (multiple indexes for lookup)
  const userData = JSON.stringify(user);
  await env.USERS.put(`user:${user_id}`, userData);
  await env.USERS.put(`username:${username}`, user_id);
  await env.USERS.put(`pubkey:${public_key}`, user_id);
  if (id_hash) await env.USERS.put(`idhash:${id_hash}`, user_id);

  // Link KYC uploads from registration session to this user
  if (kyc_session_id) {
    for (const side of ['front', 'back']) {
      const kycKey = `kyc-${kyc_session_id}-${side}`;
      const kycData = await env.KYC_STATUS.get(kycKey);
      if (kycData) {
        const kyc = JSON.parse(kycData);
        kyc.user_id = user_id;
        kyc.status = 'linked';
        // Re-store under the real user ID
        await env.KYC_STATUS.put(`kyc-${user_id}-${side}`, JSON.stringify(kyc), { expirationTtl: 86400 * 365 * 7 });
        // Clean up session key
        await env.KYC_STATUS.delete(kycKey);
      }
    }
  }

  const token = await createJwt(user_id, env.JWT_SECRET);

  // Auto-send the identity-backup email if the client supplied a
  // TOML body + a destination address. The TOML is built CLIENT-side
  // (the only place that has the user's private key), so this Worker
  // is just relaying — no server-side signing, no key persistence.
  // Failure is non-fatal: signup still succeeds, the client can
  // retry by hitting POST /api/identity/email-backup directly.
  let backup_emailed = false;
  if (email && toml_content && env.EMAIL) {
    try {
      const { EmailMessage } = await import('cloudflare:email');
      // Use whichever hostname this request came in on (api.eustress.dev
      // in production, *.workers.dev for direct deploy URLs) so the gear
      // image embedded in the email resolves to the same Worker that
      // sent it.
      const host = new URL(request.url).origin;
      const tpl = buildIdentityEmail({ username, toml_content, host });
      const raw = buildMimeWithAttachment({
        from: 'identity@eustress.dev',
        to: email,
        subject: tpl.subject,
        html: tpl.html,
        text: tpl.text,
        attachment: tpl.attachment,
      });
      const message = new EmailMessage('identity@eustress.dev', email, raw);
      await env.EMAIL.send(message);
      backup_emailed = true;
    } catch (e) {
      // Don't fail signup if email relay hiccups; log + continue.
      console.error('signup auto-email failed:', e?.message || e);
    }
  }

  return json({ token, user: await publicUser(user, env), backup_emailed }, 200, cors);
}

async function handleChallenge(request, env, cors) {
  const { public_key } = await request.json();
  if (!public_key) return json({ error: 'Missing public_key' }, 400, cors);

  const challenge = hexEncode(crypto.getRandomValues(new Uint8Array(32)));
  const expires_at = new Date(Date.now() + 5 * 60 * 1000).toISOString();

  await env.CHALLENGES.put(public_key, JSON.stringify({ challenge, expires_at }), {
    expirationTtl: 300
  });

  return json({ challenge, expires_at }, 200, cors);
}

async function handleVerify(request, env, cors) {
  const { public_key, challenge, signature } = await request.json();
  if (!public_key || !challenge || !signature)
    return json({ error: 'Missing fields' }, 400, cors);

  // Get and consume challenge
  const stored = await env.CHALLENGES.get(public_key);
  if (!stored) return json({ error: 'No pending challenge' }, 400, cors);
  await env.CHALLENGES.delete(public_key);

  const parsed = JSON.parse(stored);
  if (parsed.challenge !== challenge)
    return json({ error: 'Challenge mismatch' }, 400, cors);
  if (new Date(parsed.expires_at) < new Date())
    return json({ error: 'Challenge expired' }, 400, cors);

  // Verify Ed25519 signature using Web Crypto
  try {
    const pubKeyBytes = hexDecode(public_key);
    const sigBytes = hexDecode(signature);
    const msgBytes = new TextEncoder().encode(challenge);

    const cryptoKey = await crypto.subtle.importKey(
      'raw', pubKeyBytes, { name: 'Ed25519' }, false, ['verify']
    );
    const valid = await crypto.subtle.verify('Ed25519', cryptoKey, sigBytes, msgBytes);
    if (!valid) return json({ error: 'Signature verification failed' }, 401, cors);
  } catch (e) {
    return json({ error: `Signature error: ${e.message}` }, 400, cors);
  }

  // Find user by public key
  const userId = await env.USERS.get(`pubkey:${public_key}`);
  if (!userId) return json({ error: 'Identity not registered' }, 404, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User data missing' }, 500, cors);

  const user = JSON.parse(userData);

  // Check if user is banned
  if (user.banned) {
    return json({ error: 'Account suspended', reason: user.ban_reason || 'Contact support@eustress.dev' }, 403, cors);
  }

  const token = await createJwt(userId, env.JWT_SECRET);

  return json({ token, user: await publicUser(user, env) }, 200, cors);
}

// ── Identity Backup Email (Cloudflare Email Workers) ────────────────────────
async function handleEmailIdentityBackup(request, env, cors) {
  const body = await request.json();
  const { email, username, toml_content } = body;

  if (!email || !email.includes('@'))
    return json({ error: 'Valid email required' }, 400, cors);
  if (!toml_content || toml_content.length < 50)
    return json({ error: 'Invalid TOML content' }, 400, cors);

  // All branding + body assembly lives in `buildIdentityEmail` so this
  // route and the auto-send inside `/api/auth/register` produce
  // identical output. The TOML rides as a real file attachment via
  // `buildMimeWithAttachment`'s multipart/mixed wrapper.
  const host = new URL(request.url).origin;
  const tpl = buildIdentityEmail({ username: username || 'Creator', toml_content, host });

  try {
    if (env.EMAIL) {
      const { EmailMessage } = await import('cloudflare:email');
      const raw = buildMimeWithAttachment({
        from: 'identity@eustress.dev',
        to: email,
        subject: tpl.subject,
        html: tpl.html,
        text: tpl.text,
        attachment: tpl.attachment,
      });
      const message = new EmailMessage('identity@eustress.dev', email, raw);
      await env.EMAIL.send(message);
      return json({ ok: true, message: 'Backup emailed via Cloudflare Email' }, 200, cors);
    }

    // Fallback: MailChannels (legacy path, kept for environments where
    // the EMAIL binding is unavailable). MailChannels' JSON API doesn't
    // support file attachments out of the box, so the TOML stays inline
    // in the HTML/text body in this branch — accepted shortfall for the
    // fallback. Production traffic uses the EMAIL binding above.
    const emailResp = await fetch('https://api.mailchannels.net/tx/v1/send', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        personalizations: [{ to: [{ email }] }],
        from: { email: 'identity@eustress.dev', name: 'Eustress Identity' },
        subject: tpl.subject,
        content: [
          { type: 'text/plain', value: tpl.text },
          { type: 'text/html', value: tpl.html },
        ],
      }),
    });

    if (emailResp.ok || emailResp.status === 202) {
      return json({ ok: true, message: 'Backup emailed' }, 200, cors);
    }

    const errText = await emailResp.text();
    console.error('MailChannels error:', emailResp.status, errText);
    return json({ error: 'Email delivery failed', detail: errText }, 502, cors);
  } catch (e) {
    console.error('Email send error:', e);
    return json({ error: 'Email service error: ' + e.message }, 503, cors);
  }
}

// ── Workshop Context (Persistent Memories + Global Rules) ───────────────────

async function handleGetWorkshopContext(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Sign in to sync Workshop context across devices' }, 401, cors);

  const url = new URL(request.url);
  const projectId = url.searchParams.get('project_id') || 'default';
  const key = `ctx:${userId}:${projectId}`;

  const data = await env.WORKSHOP_CONTEXT.get(key);
  if (!data) {
    return json({
      version: 1,
      user_id: userId,
      project_id: projectId,
      memories: [],
      global_rules: [],
      session_summaries: [],
      last_synced: new Date().toISOString(),
    }, 200, cors);
  }

  return json(JSON.parse(data), 200, cors);
}

async function handlePutWorkshopContext(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Sign in to sync Workshop context across devices' }, 401, cors);

  const body = await request.json();
  const projectId = body.project_id || 'default';
  const key = `ctx:${userId}:${projectId}`;

  const doc = {
    version: 1,
    user_id: userId,
    project_id: projectId,
    memories: body.memories || [],
    global_rules: body.global_rules || [],
    session_summaries: (body.session_summaries || []).slice(-20), // Keep last 20 summaries
    last_synced: new Date().toISOString(),
  };

  await env.WORKSHOP_CONTEXT.put(key, JSON.stringify(doc));
  return json({ ok: true, last_synced: doc.last_synced }, 200, cors);
}

async function handleAddWorkshopMemory(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Sign in to sync Workshop memories' }, 401, cors);

  const body = await request.json();
  const { project_id, key: memKey, value, category, source } = body;

  if (!memKey || !value) return json({ error: 'key and value required' }, 400, cors);

  const ctxKey = `ctx:${userId}:${project_id || 'default'}`;
  const existing = await env.WORKSHOP_CONTEXT.get(ctxKey);
  const doc = existing ? JSON.parse(existing) : {
    version: 1, user_id: userId, project_id: project_id || 'default',
    memories: [], global_rules: [], session_summaries: [],
  };

  // Upsert memory by key
  const idx = doc.memories.findIndex(m => m.key === memKey);
  const memory = {
    key: memKey,
    value,
    category: category || 'preference',
    source: source || 'user',
    updated_at: new Date().toISOString(),
  };

  if (idx >= 0) {
    doc.memories[idx] = memory;
  } else {
    doc.memories.push(memory);
  }

  doc.last_synced = new Date().toISOString();
  await env.WORKSHOP_CONTEXT.put(ctxKey, JSON.stringify(doc));

  return json({ ok: true, memory_count: doc.memories.length }, 200, cors);
}

async function handleMe(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User not found' }, 404, cors);

  return json(await publicUser(JSON.parse(userData), env), 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// KYC
// ═══════════════════════════════════════════════════════════════════════════

function handleJurisdiction(request, env, cors) {
  const country = request.headers.get('cf-ipcountry') || 'XX';
  const colo = request.cf?.colo || 'unknown';
  const jurisdiction = JURISDICTIONS.jurisdictions[country];

  if (jurisdiction) {
    return json({
      detected: true, iso2: country, name: jurisdiction.name, colo,
      accepted_ids: jurisdiction.natural_ids,
      notes: jurisdiction.notes,
      minors: jurisdiction.minors || null,
    }, 200, cors);
  }

  return json({
    detected: false, iso2: country, name: `Unknown (${country})`, colo,
    accepted_ids: JURISDICTIONS.fallback.require,
    notes: 'Fallback: enhanced due diligence', flag: 'manual_review',
  }, 200, cors);
}

async function handleKycUpload(request, env, cors) {
  const country = request.headers.get('cf-ipcountry') || 'XX';
  const jurisdiction = JURISDICTIONS.jurisdictions[country];
  const prefix = jurisdiction?.r2_prefix || `kyc-${country.toLowerCase()}-`;

  const formData = await request.formData();
  const side = formData.get('side') || 'front';
  const idType = formData.get('id_type') || 'unknown';
  const sessionId = formData.get('session_id') || crypto.randomUUID();
  const file = formData.get('document');

  if (!file || !(file instanceof File))
    return json({ error: 'Missing document file' }, 400, cors);

  // Validate before touching R2. Without this the endpoint is an open,
  // unauthenticated write into the KYC bucket at any size or type.
  const ALLOWED_TYPES = ['image/jpeg', 'image/png', 'image/webp', 'application/pdf'];
  const MAX_BYTES = 12 * 1024 * 1024; // 12 MB — well above a cleaned phone capture

  const declaredType = (file.type || '').toLowerCase().split(';')[0].trim();
  if (!ALLOWED_TYPES.includes(declaredType))
    return json({
      error: 'Unsupported document format. Use JPEG, PNG, WebP, or PDF.',
      received: declaredType || 'unknown',
    }, 415, cors);

  if (file.size > MAX_BYTES)
    return json({
      error: `Document too large (${(file.size / 1048576).toFixed(1)} MB). Maximum is 12 MB.`,
    }, 413, cors);

  if (file.size === 0)
    return json({ error: 'Document is empty' }, 400, cors);

  if (side !== 'front' && side !== 'back')
    return json({ error: 'side must be "front" or "back"' }, 400, cors);

  // Authenticated user → use real user ID; registration → use session_id
  let userId = await verifyAuth(request, env);
  const uploadId = userId || `session-${sessionId}`;

  const fileData = await file.arrayBuffer();

  // The declared Content-Type is attacker-controlled, so confirm the bytes
  // actually are what they claim before persisting them.
  const sniffed = sniffFileType(fileData);
  if (sniffed !== declaredType)
    return json({
      error: 'Document contents do not match the declared format',
      declared: declaredType, detected: sniffed || 'unrecognized',
    }, 415, cors);

  const hashBuffer = await crypto.subtle.digest('SHA-256', fileData);
  const hashHex = hexEncode(new Uint8Array(hashBuffer));
  const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
  const objectKey = `${prefix}${uploadId}/${side}-${timestamp}-${hashHex.substring(0, 16)}.bin`;

  await env.KYC_BUCKET.put(objectKey, fileData, {
    httpMetadata: { contentType: file.type || 'application/octet-stream' },
    customMetadata: {
      'upload-id': uploadId, 'session-id': sessionId,
      'loc': country, 'id-type': idType,
      'side': side, 'file-name': file.name,
      'timestamp': new Date().toISOString(), 'hash': hashHex,
    },
  });

  // Track in KYC_STATUS keyed by session_id so registration can link it
  await env.KYC_STATUS.put(`kyc-${sessionId}-${side}`, JSON.stringify({
    status: 'uploaded', upload_id: uploadId, session_id: sessionId,
    iso2: country, id_type: idType, side,
    r2_key: objectKey, uploaded_at: new Date().toISOString(), hash: hashHex,
  }), { expirationTtl: 86400 * 30 }); // 30 day TTL for pending

  return json({
    verification_id: `kyc-${sessionId}-${side}`,
    session_id: sessionId, side, status: 'uploaded',
    r2_key: objectKey, hash: hashHex,
  }, 200, cors);
}

async function handleKycStatus(verificationId, env, cors) {
  const data = await env.KYC_STATUS.get(verificationId);
  if (!data) return json({ error: 'Not found' }, 404, cors);
  return json(JSON.parse(data), 200, cors);
}

// ── Desktop → mobile handoff ─────────────────────────────────────────────────
// A desktop browser usually has a poor camera (or none). The applicant scans a
// QR code, finishes document capture on their phone, and the desktop tab polls
// until the session completes.
//
// The session id in the QR acts as a bearer token, so the handoff record is
// short-lived and carries no personal data back to the phone. The applicant's
// name and birthday stay server-side and are read directly by /api/kyc/submit.

const HANDOFF_TTL_SECONDS = 30 * 60; // 30 minutes to photograph two sides

async function handleKycHandoffCreate(request, env, cors) {
  let body;
  try { body = await request.json(); }
  catch { return json({ error: 'Invalid JSON body' }, 400, cors); }

  const sessionId = body.session_id;
  if (!sessionId || typeof sessionId !== 'string' || sessionId.length < 8)
    return json({ error: 'A session_id of at least 8 characters is required' }, 400, cors);

  const country = request.headers.get('cf-ipcountry') || 'XX';
  const jurisdiction = JURISDICTIONS.jurisdictions[country];
  const expiresAt = new Date(Date.now() + HANDOFF_TTL_SECONDS * 1000).toISOString();

  await env.KYC_STATUS.put(`handoff:${sessionId}`, JSON.stringify({
    session_id: sessionId,
    id_type: body.id_type || 'unknown',
    needs_back: body.needs_back !== false,
    // Held server-side so the phone never receives personal data over the QR.
    full_name: body.full_name || '',
    birthday: body.birthday || '',
    username: body.username || '',
    iso2: country,
    minimum_age: minimumAgeFor(country),
    accepted_ids: jurisdiction?.natural_ids || JURISDICTIONS.fallback.require,
    created_at: new Date().toISOString(),
    expires_at: expiresAt,
  }), { expirationTtl: HANDOFF_TTL_SECONDS });

  return json({
    session_id: sessionId,
    verify_url: `https://eustress.dev/verify?s=${encodeURIComponent(sessionId)}`,
    expires_at: expiresAt,
    expires_in: HANDOFF_TTL_SECONDS,
  }, 200, cors);
}

/// Context the phone needs to run the capture flow. Deliberately excludes the
/// applicant's name and date of birth.
async function handleKycHandoffGet(sessionId, env, cors) {
  const raw = await env.KYC_STATUS.get(`handoff:${sessionId}`);
  // A missing key means the TTL lapsed — KV removes it for us.
  if (!raw)
    return json({ error: 'This verification link has expired', code: 'expired' }, 404, cors);

  const h = JSON.parse(raw);
  return json({
    session_id: h.session_id,
    id_type: h.id_type,
    needs_back: h.needs_back,
    iso2: h.iso2,
    minimum_age: h.minimum_age,
    accepted_ids: h.accepted_ids,
    expires_at: h.expires_at,
  }, 200, cors);
}

/// Whole-session progress for the polling desktop tab.
async function handleKycSessionStatus(sessionId, env, cors) {
  const [frontRaw, backRaw] = await Promise.all([
    env.KYC_STATUS.get(`kyc-${sessionId}-front`),
    env.KYC_STATUS.get(`kyc-${sessionId}-back`),
  ]);

  if (!frontRaw && !backRaw)
    return json({
      session_id: sessionId, status: 'pending',
      front_uploaded: false, back_uploaded: false,
    }, 200, cors);

  const front = frontRaw ? JSON.parse(frontRaw) : null;

  return json({
    session_id: sessionId,
    // 'uploaded' means documents arrived but verification has not run yet.
    status: front?.status || 'uploaded',
    front_uploaded: !!frontRaw,
    back_uploaded: !!backRaw,
    age_verified: front?.age_verified === true,
    age_status: front?.age_status || null,
    minimum_age: front?.minimum_age || null,
    ocr_name: front?.ocr_name || '',
    reason: front?.status === 'rejected'
      ? (front?.grok_analysis?.doc_reason || 'Verification did not pass')
      : null,
  }, 200, cors);
}

/// Submit KYC for verification after documents are uploaded.
/// Fetches ALL document images from R2, sends them to Grok in a SINGLE call
/// that performs document verification + OCR + criminal background screening.
async function handleKycSubmit(request, env, cors) {
  try {
    const body = await request.json();
    const sessionId = body.session_id;

    if (!sessionId)
      return json({ error: 'Missing session_id' }, 400, cors);

    // When the capture happened on a phone via the QR handoff, the identity
    // claims live server-side in the handoff record. Prefer those so the phone
    // never has to receive or re-send the applicant's name and birthday.
    const handoffRaw = await env.KYC_STATUS.get(`handoff:${sessionId}`);
    const handoff = handoffRaw ? JSON.parse(handoffRaw) : null;

    const needsBack = body.needs_back !== undefined
      ? body.needs_back !== false
      : (handoff ? handoff.needs_back !== false : true);
    const claimedName = body.full_name || handoff?.full_name || '';
    const claimedBirthday = body.birthday || handoff?.birthday || '';

    // Check front is uploaded
    const frontKey = `kyc-${sessionId}-front`;
    const frontData = await env.KYC_STATUS.get(frontKey);
    if (!frontData)
      return json({ error: 'Front document not uploaded', status: 'incomplete' }, 400, cors);

    const front = JSON.parse(frontData);
    let back = null;

    // Check back if required
    if (needsBack) {
      const backKey = `kyc-${sessionId}-back`;
      const backData = await env.KYC_STATUS.get(backKey);
      if (!backData)
        return json({ error: 'Back document not uploaded', status: 'incomplete' }, 400, cors);
      back = JSON.parse(backData);
    }

    const verificationId = frontKey;

    // ── Single Grok call: document verification + OCR + criminal screening ──
    const grokResult = await performFullKycVerification(
      front.r2_key,
      back?.r2_key || null,
      claimedName,
      claimedBirthday,
      front.id_type || '',
      env,
    );

    // Store screening result in SCREENING KV (same format as performScreening)
    // so handleRegister can skip the duplicate call
    const screeningKey = `screen:session-${sessionId}`;
    await env.SCREENING.put(screeningKey, JSON.stringify({
      decision: grokResult.screening_decision,
      risk_score: grokResult.risk_score,
      reason: grokResult.screening_reason,
      flags: grokResult.screening_flags,
      verdict_found: grokResult.verdict_found,
      details: grokResult.screening_details,
      screened_at: new Date().toISOString(),
      model: GROK_MODEL,
    }), { expirationTtl: 86400 * 365 * 7 });

    // ── Age verification ───────────────────────────────────────────────────
    // The DOCUMENT's date of birth is authoritative. A self-typed birthday is
    // an unverified claim and can never be the basis for passing the age gate,
    // so if OCR could not read a DOB we refuse rather than fall back to it.
    const iso2 = front.iso2 || 'XX';
    const minAge = minimumAgeFor(iso2);
    const documentDob = grokResult.extracted_dob || '';
    const documentAge = ageFromDob(documentDob);
    const claimedAge = ageFromDob(claimedBirthday);

    let ageStatus;      // 'verified' | 'underage' | 'unreadable' | 'mismatch'
    let ageReason = '';

    if (documentAge === null) {
      ageStatus = 'unreadable';
      ageReason = 'Date of birth could not be read from the document';
    } else if (documentAge < minAge) {
      ageStatus = 'underage';
      ageReason = `Applicant is ${documentAge}; minimum age is ${minAge}`;
    } else if (claimedAge !== null && Math.abs(claimedAge - documentAge) > 1) {
      // Tolerate a 1-year drift (timezone / birthday-today edge), reject beyond.
      ageStatus = 'mismatch';
      ageReason = `Stated age (${claimedAge}) does not match the document (${documentAge})`;
    } else {
      ageStatus = 'verified';
    }

    const ageApproved = ageStatus === 'verified';

    // Determine final decision: every gate must pass. Age is a hard gate —
    // a clean document and a clean background do not admit a minor.
    const docApproved = grokResult.doc_decision === 'APPROVE';
    const screenApproved = grokResult.screening_decision !== 'DENY';
    const finalStatus = (docApproved && screenApproved && ageApproved)
      ? 'verified'
      : 'rejected';

    const verifiedRecord = {
      ...front,
      status: finalStatus,
      verified_at: new Date().toISOString(),
      ocr_name: grokResult.extracted_name || claimedName,
      extracted_dob: documentDob,
      // Age facts are stored so registration can re-check them server-side
      // without trusting anything the client sends back.
      age_status: ageStatus,
      age_verified: ageApproved,
      document_age: documentAge,
      minimum_age: minAge,
      jurisdiction: iso2,
      grok_analysis: grokResult,
    };

    await env.KYC_STATUS.put(verificationId, JSON.stringify(verifiedRecord), {
      expirationTtl: 86400 * 365,
    });

    if (finalStatus === 'verified') {
      return json({
        status: 'verified',
        verification_id: verificationId,
        ocr_name: grokResult.extracted_name || claimedName,
        age_verified: true,
        minimum_age: minAge,
      }, 200, cors);
    } else {
      // Surface WHY in a form the UI can show the applicant. Age failures get
      // a plain-language reason; document and screening failures keep theirs.
      const reason = !ageApproved
        ? ageReason
        : (!docApproved
            ? (grokResult.doc_reason || 'Document could not be verified')
            : (grokResult.screening_reason || 'Application requires review'));

      return json({
        status: 'rejected',
        verification_id: verificationId,
        reason,
        age_status: ageStatus,
        age_verified: ageApproved,
        minimum_age: minAge,
        decision: grokResult,
      }, 200, cors);
    }
  } catch (e) {
    return json({ error: 'Submit failed: ' + e.message }, 500, cors);
  }
}

/// Identify a file from its leading bytes, independent of any declared type.
/// Returns a MIME string for the formats KYC accepts, or null if unrecognized.
function sniffFileType(buf) {
  const b = new Uint8Array(buf.slice(0, 12));
  if (b.length < 4) return null;
  // JPEG: FF D8 FF
  if (b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff) return 'image/jpeg';
  // PNG: 89 50 4E 47 0D 0A 1A 0A
  if (b[0] === 0x89 && b[1] === 0x50 && b[2] === 0x4e && b[3] === 0x47)
    return 'image/png';
  // PDF: %PDF
  if (b[0] === 0x25 && b[1] === 0x50 && b[2] === 0x44 && b[3] === 0x46)
    return 'application/pdf';
  // WebP: "RIFF" .... "WEBP"
  if (b[0] === 0x52 && b[1] === 0x49 && b[2] === 0x46 && b[3] === 0x46 &&
      b[8] === 0x57 && b[9] === 0x45 && b[10] === 0x42 && b[11] === 0x50)
    return 'image/webp';
  return null;
}

/// Pull the assistant's text out of an xAI response.
///
/// The shape is not guaranteed to be `output[0].content[0].text`: a reasoning
/// model puts a reasoning item first, so indexing position 0 yields nothing and
/// the caller sees an empty string that looks exactly like a refusal. Scan every
/// output item instead, and fall back to the chat-completions shape so a change
/// of endpoint does not silently return blank.
function extractGrokText(data) {
  if (typeof data?.output_text === 'string' && data.output_text.trim())
    return data.output_text;

  for (const item of (Array.isArray(data?.output) ? data.output : [])) {
    for (const c of (Array.isArray(item?.content) ? item.content : [])) {
      if (typeof c?.text === 'string' && c.text.trim()) return c.text;
    }
  }

  const msg = data?.choices?.[0]?.message?.content;
  if (typeof msg === 'string' && msg.trim()) return msg;

  // Nothing matched. Log the shape (keys only, never the content, which can
  // carry identity data) so a future format change is diagnosable.
  console.error('xAI: could not extract text; output item types:',
    JSON.stringify((data?.output || []).map(i => i?.type)));
  return '';
}

/// Base64-encode an ArrayBuffer without blowing the call stack.
///
/// `btoa(String.fromCharCode(...new Uint8Array(buf)))` spreads every byte as a
/// separate argument, which throws RangeError once the array passes the engine's
/// argument limit (~100k). A phone camera photo is 2-5 MB, so that form threw on
/// essentially every real mobile KYC upload. Chunked conversion has no such limit.
function bytesToBase64(buf) {
  const bytes = new Uint8Array(buf);
  const CHUNK = 0x8000; // 32 KB per fromCharCode call, well under any limit
  let binary = '';
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode.apply(null, bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}

/**
 * Single Grok call that does EVERYTHING:
 * 1. Document verification — are the images real government IDs?
 * 2. Image quality — clear enough to read?
 * 3. OCR — extract full legal name and date of birth
 * 4. Cross-reference — does extracted info match what user claimed?
 * 5. Criminal background screening — public court verdicts for extracted name + DOB
 *
 * Receives front image (required) + back image (optional) + claimed identity.
 * Falls back to APPROVE if Grok is unavailable.
 */
async function performFullKycVerification(frontR2Key, backR2Key, claimedName, claimedBirthday, idType, env) {
  // Fail CLOSED. This decision gates access to money (Bliss earning and cash
  // out), so "we could not check" must never read as "approved". Without a
  // verification backend the applicant stays unverified and goes to a human.
  if (!env.GROK_API_KEY) {
    return {
      doc_decision: 'DENY', screening_decision: 'REVIEW',
      doc_reason: 'Automated document verification is not configured',
      reason: 'Verification unavailable', extracted_name: '',
      extracted_dob: '', risk_score: 0, screening_flags: ['verification_unavailable'],
      confidence: 0, requires_manual_review: true,
    };
  }

  try {
    // Fetch front image from R2
    const frontObj = await env.KYC_BUCKET.get(frontR2Key);
    // No document in storage means there is nothing to verify. Approving here
    // would let a caller reach "verified" without ever presenting an ID.
    if (!frontObj) {
      return {
        doc_decision: 'DENY', screening_decision: 'REVIEW',
        doc_reason: 'Front document missing from storage',
        reason: 'Front document not found in storage', extracted_name: '',
        extracted_dob: '', risk_score: 0, screening_flags: ['document_missing'],
        confidence: 0, requires_manual_review: true,
      };
    }
    const frontBytes = await frontObj.arrayBuffer();
    const frontType = frontObj.httpMetadata?.contentType || 'image/jpeg';
    const frontB64 = bytesToBase64(frontBytes);

    // Fetch back image from R2 (if available)
    let backB64 = null;
    let backType = 'image/jpeg';
    if (backR2Key) {
      const backObj = await env.KYC_BUCKET.get(backR2Key);
      if (backObj) {
        const backBytes = await backObj.arrayBuffer();
        backType = backObj.httpMetadata?.contentType || 'image/jpeg';
        backB64 = bytesToBase64(backBytes);
      }
    }

    const imageCount = backB64 ? 'two images (front and back)' : 'one image (front only — passport or single-sided ID)';

    const prompt = `You are the KYC + Background Screening AI for Eustress, a game engine platform.
You are given ${imageCount} of an ID document. The user claims:
- Name: "${claimedName}"
- Date of Birth: "${claimedBirthday}"
- ID Type: "${idType}"

Perform ALL of the following in a single analysis:

## PART 1: Document Verification
- Is each image a real government-issued ID document (not a screenshot, printed copy, or digitally altered)?
- Is the image quality sufficient to read text?
- What type of document is it (passport, drivers license, national ID, other)?

## PART 2: OCR Extraction
- Extract the full legal name exactly as printed on the document
- Extract the date of birth exactly as printed (convert to YYYY-MM-DD)
- Does the extracted name roughly match the claimed name "${claimedName}"?
- Does the extracted DOB match the claimed DOB "${claimedBirthday}"?

## PART 3: Criminal Background Screening
Using the EXTRACTED legal name and date of birth (NOT the username or claimed name):
- Search publicly available criminal record VERDICTS (final court decisions only)
- Do NOT consider arrests, charges, accusations, or pending cases — ONLY convictions/verdicts
- If you cannot confidently identify the person, assume clean record

Respond in EXACTLY this JSON format, nothing else:
{
  "doc_decision": "APPROVE|DENY",
  "doc_reason": "one sentence about document authenticity and quality",
  "is_id_document": true/false,
  "document_type": "passport|drivers_license|national_id|other|not_a_document",
  "image_quality": "clear|acceptable|blurry|unreadable",
  "extracted_name": "Full Legal Name from document or empty string",
  "extracted_dob": "YYYY-MM-DD from document or empty string",
  "name_matches": true/false,
  "dob_matches": true/false,
  "screening_decision": "APPROVE|REVIEW|DENY",
  "screening_reason": "one sentence about background check result",
  "risk_score": 0-100,
  "screening_flags": [],
  "verdict_found": false,
  "screening_details": "",
  "confidence": 0-100
}

Document rules:
- APPROVE if genuine government ID with clear/acceptable quality
- DENY if not an ID, unreadable, screenshot of screen, printed copy, digitally altered
- Partial name matches OK (e.g. "John Smith" ≈ "Jonathan Smith")
- If name unreadable but document looks genuine: APPROVE with empty extracted_name

Screening rules:
- risk_score 0-20: APPROVE (clean or minor non-violent misdemeanor 5+ years ago)
- risk_score 21-60: REVIEW (non-violent felony, recent misdemeanor, pattern)
- risk_score 61-100: DENY (violent felony, sex offense, fraud conviction)
- Cannot identify person or no records: risk_score 0, APPROVE
- NEVER deny on name similarity alone — require matching DOB AND legal name
- Do NOT hallucinate or fabricate criminal records
- Err on the side of approval — innocent until proven guilty by verdict`;

    // Build input array with images + prompt
    const input = [];
    input.push({ type: 'image_url', image_url: { url: `data:${frontType};base64,${frontB64}` } });
    if (backB64) {
      input.push({ type: 'image_url', image_url: { url: `data:${backType};base64,${backB64}` } });
    }
    input.push({ type: 'text', text: prompt });

    const resp = await grokFetch({ input }, env.GROK_API_KEY);

    if (!resp.ok) {
      const errText = await resp.text();
      console.error('Grok KYC error:', resp.status, errText);
      // Fail CLOSED. An upstream error means the document was never actually
      // checked, and "not checked" must never be recorded as "approved".
      return {
        doc_decision: 'DENY', screening_decision: 'REVIEW',
        doc_reason: `Verification service returned ${resp.status}`,
        reason: 'Verification service error', extracted_name: '', extracted_dob: '',
        risk_score: 0, screening_flags: ['verification_error'], confidence: 0,
        requires_manual_review: true,
      };
    }

    const data = await resp.json();
    const responseText = extractGrokText(data);

    const jsonMatch = responseText.match(/\{[\s\S]*\}/);
    if (!jsonMatch) {
      // Fail CLOSED for the same reason: an unreadable verdict is not a pass.
      return {
        doc_decision: 'DENY', screening_decision: 'REVIEW',
        doc_reason: 'Verification response could not be parsed',
        reason: 'Could not parse verification response', extracted_name: '', extracted_dob: '',
        risk_score: 0, screening_flags: ['unparseable_response'], confidence: 0,
        requires_manual_review: true,
      };
    }

    const r = JSON.parse(jsonMatch[0]);
    return {
      doc_decision: r.doc_decision || 'APPROVE',
      doc_reason: r.doc_reason || 'No issues found',
      is_id_document: r.is_id_document ?? true,
      document_type: r.document_type || 'unknown',
      image_quality: r.image_quality || 'unknown',
      extracted_name: r.extracted_name || '',
      extracted_dob: r.extracted_dob || '',
      name_matches: r.name_matches ?? true,
      dob_matches: r.dob_matches ?? true,
      screening_decision: r.screening_decision || 'APPROVE',
      screening_reason: r.screening_reason || 'No concerns found',
      risk_score: Math.min(100, Math.max(0, r.risk_score || 0)),
      screening_flags: r.screening_flags || [],
      verdict_found: r.verdict_found || false,
      screening_details: r.screening_details || '',
      confidence: Math.min(100, Math.max(0, r.confidence || 0)),
      model: GROK_MODEL,
    };
  } catch (e) {
    console.error('Grok KYC exception:', e);
    // Fail CLOSED. A thrown exception means verification did not complete, so
    // the applicant goes to manual review rather than through the gate.
    return {
      doc_decision: 'DENY', screening_decision: 'REVIEW',
      doc_reason: 'Verification did not complete',
      reason: `Verification error: ${e.message}`,
      extracted_name: '', extracted_dob: '',
      risk_score: 0, screening_flags: ['verification_exception'], confidence: 0,
      requires_manual_review: true,
    };
  }
}

// ═══════════════════════════════════════════════════════════════════════════
// CO-SIGN
// ═══════════════════════════════════════════════════════════════════════════

// SECURITY MODEL — READ BEFORE EDITING
// ------------------------------------
// The client self-reports contribution_type + duration. A user owns their
// machine and their JWT, so ANY value here can be forged (the engine's
// bliss_tracker.toml is just one way to feed this endpoint; curl is
// another). Therefore the witness — not the client — is the sole trust
// boundary, and it must bound what any single authenticated account can
// earn regardless of what it claims. Four enforced invariants:
//   1. contribution_type must be a known type (no silent 1.0 fallback).
//   2. The Full-node +10% bonus is taken from server-OBSERVED heartbeat
//      mode (node-mode:{user}), never the request body.
//   3. ActiveTime credited for the day cannot exceed server-observed
//      presence (presence:{date}:{user}, wall-clock bounded in the
//      heartbeat handler). Fabricated idle time is dropped.
//   4. Per-user daily score is capped at MAX_DAILY_SCORE (~16h of
//      top-weighted work), bounding the blast radius of any forged claim.
// These make the endpoint abuse-BOUNDED. They do NOT make forged
// Development/Creation claims impossible — that needs server-verifiable
// artifacts (attestation), tracked separately as step 4.
async function handleCosign(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const body = await request.json();
  const { contribution_type, contribution_hash, duration_secs } = body;

  if (!contribution_type || !contribution_hash)
    return json({ error: 'contribution_type and contribution_hash required' }, 400, cors);

  // (1) Contribution weights (from bliss-core/src/contribution.rs).
  // Unknown types are REJECTED — no 1.0 fallback that would let an
  // attacker smuggle an arbitrary string through as a valid claim.
  const weights = {
    ActiveTime: 1.0, Creation: 2.5, Collaboration: 2.0, Education: 2.2,
    Development: 3.0, Moderation: 1.5, QualityAssurance: 1.8,
    Optimization: 2.0, Documentation: 1.5, Custom: 1.0,
  };
  if (!Object.prototype.hasOwnProperty.call(weights, contribution_type))
    return json({ error: `Unknown contribution_type: ${contribution_type}` }, 400, cors);
  const weight = weights[contribution_type];

  // Sanitize duration to a finite [1, 3600] second window.
  let durSecs = Number(duration_secs);
  if (!Number.isFinite(durSecs)) durSecs = 60;
  durSecs = Math.max(1, Math.min(durSecs, 3600));

  // Rate limit: max 120 cosigns per hour per user
  const hourKey = `cosign-rate:${userId}:${new Date().toISOString().slice(0, 13)}`;
  const count = parseInt(await env.CHALLENGES.get(hourKey) || '0');
  if (count >= 120) return json({ error: 'Rate limit: 120 cosigns/hour' }, 429, cors);
  await env.CHALLENGES.put(hourKey, (count + 1).toString(), { expirationTtl: 3600 });

  // Replay protection — the same contribution hash can only be
  // co-signed once. The engine binds user, day, type, duration, and a
  // monotonic chunk id into the hash, so re-submitting persisted work
  // after a crash is safe (same hash → rejected, no double credit).
  const dupeKey = `cosign-hash:${userId}:${contribution_hash}`;
  if (await env.CHALLENGES.get(dupeKey))
    return json({ error: 'Duplicate contribution hash' }, 409, cors);
  await env.CHALLENGES.put(dupeKey, '1', { expirationTtl: 86400 * 2 });

  const today = new Date().toISOString().split('T')[0];

  // (2) Node bonus from SERVER-OBSERVED heartbeat mode, not the body.
  const observedMode = await env.SOCIAL.get(`node-mode:${userId}`);
  const nodeBonus = observedMode === 'Full' ? 1.1 : 1.0;

  // Load the per-day record early — needed for the ActiveTime presence
  // check and the daily ceiling. `by_seconds` tracks credited seconds
  // per type (parallel to by_type's credited score) so the presence cap
  // can compare against cumulative ActiveTime seconds already credited.
  const dayKey = `contrib:${today}:${userId}`;
  const dayData = await env.INVENTORY.get(dayKey);
  const day = dayData
    ? JSON.parse(dayData)
    : { total_score: 0, by_type: {}, by_seconds: {}, count: 0 };
  if (!day.by_seconds) day.by_seconds = {};

  // (3) ActiveTime is bounded by server-observed presence. Credited
  // ActiveTime seconds for the day can never exceed the wall-clock
  // seconds the witness saw this user online.
  let creditSecs = durSecs;
  if (contribution_type === 'ActiveTime') {
    const presRaw = await env.SOCIAL.get(`presence:${today}:${userId}`);
    const presenceSecs = presRaw ? (JSON.parse(presRaw).seconds || 0) : 0;
    const alreadyActive = day.by_seconds.ActiveTime || 0;
    creditSecs = Math.max(0, Math.min(creditSecs, presenceSecs - alreadyActive));
  }

  let score = weight * nodeBonus * creditSecs / 60; // weighted minutes

  // (4) Per-user daily score ceiling — clamp to remaining headroom.
  const remaining = Math.max(0, MAX_DAILY_SCORE - (day.total_score || 0));
  if (score > remaining) score = remaining;

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User not found' }, 404, cors);
  const user = JSON.parse(userData);

  // Only mutate ledgers when there is real score to credit. A zero-credit
  // cosign (capped out, or ActiveTime beyond presence) still consumed its
  // dedupe + rate-limit slot above, so it can't be retried or spammed.
  if (score > 0) {
    user.contribution_score = (user.contribution_score || 0) + score;
    user.total_cosigns = (user.total_cosigns || 0) + 1;
    user.last_contribution = new Date().toISOString();
    await env.USERS.put(`user:${userId}`, JSON.stringify(user));

    day.total_score += score;
    day.by_type[contribution_type] = (day.by_type[contribution_type] || 0) + score;
    day.by_seconds[contribution_type] = (day.by_seconds[contribution_type] || 0) + creditSecs;
    day.count += 1;
    day.updated_at = new Date().toISOString();
    await env.INVENTORY.put(dayKey, JSON.stringify(day), { expirationTtl: 86400 * 90 });
    // Running network-wide score for the day — powers the live projection in
    // the heartbeat. Advisory only; the distribution recomputes from scratch.
    const dtKey = `daytotal:${today}`;
    const dtPrev = parseFloat(await env.INVENTORY.get(dtKey) || '0');
    await env.INVENTORY.put(dtKey, String(dtPrev + score), { expirationTtl: 86400 * 7 });
  }

  // Generate co-signature (hash of contribution + user + timestamp)
  const payload = `cosign|${userId}|${contribution_hash}|${new Date().toISOString()}`;
  const sigBytes = new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(payload)));
  const signature = hexEncode(sigBytes);

  // `capped` tells an honest client its claim was trimmed (daily ceiling
  // or presence) so it can stop flushing instead of burning rate-limit.
  const uncapped = weight * nodeBonus * durSecs / 60;
  return json({
    server_signature: signature,
    co_signed_at: new Date().toISOString(),
    contribution_type,
    weight,
    node_bonus: nodeBonus,
    score_added: score,
    total_score: user.contribution_score || 0,
    pending_today: day.total_score,
    capped: score < uncapped - 1e-9,
  }, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// HEALTH
// ═══════════════════════════════════════════════════════════════════════════

async function handleHealth(env, cors) {
  // Configuration is reported as booleans and lengths, never as values.
  //
  // This detail exists because a missing key produced no visible symptom: KYC
  // quietly rubber-stamped applicants when verification failed open, and
  // quietly denies them now that it fails closed. Neither state surfaced
  // anywhere, so make it observable.
  const present = v => typeof v === 'string' && v.trim().length > 0;
  return json({
    status: 'ok',
    service: 'eustress-api',
    fork_id: env.FORK_ID || 'eustress.dev',
    timestamp: new Date().toISOString(),
    model: GROK_MODEL,
    integrations: {
      grok: { configured: present(env.GROK_API_KEY), key_length: (env.GROK_API_KEY || '').length },
      stripe: { configured: present(env.STRIPE_SECRET_KEY) },
      email: { configured: !!env.EMAIL },
      kyc_bucket: { configured: !!env.KYC_BUCKET },
    },
  }, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// JWT (using Web Crypto HMAC-SHA256)
// ═══════════════════════════════════════════════════════════════════════════

async function createJwt(userId, secret) {
  const header = { alg: 'HS256', typ: 'JWT' };
  const payload = {
    sub: userId,
    iat: Math.floor(Date.now() / 1000),
    exp: Math.floor(Date.now() / 1000) + 72 * 3600, // 72 hours
  };

  const enc = new TextEncoder();
  const headerB64 = base64url(JSON.stringify(header));
  const payloadB64 = base64url(JSON.stringify(payload));
  const sigInput = enc.encode(`${headerB64}.${payloadB64}`);

  const key = await crypto.subtle.importKey(
    'raw', enc.encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']
  );
  const sig = await crypto.subtle.sign('HMAC', key, sigInput);
  const sigB64 = base64url(String.fromCharCode(...new Uint8Array(sig)));

  return `${headerB64}.${payloadB64}.${sigB64}`;
}

async function verifyJwt(token, secret) {
  const [headerB64, payloadB64, sigB64] = token.split('.');
  if (!headerB64 || !payloadB64 || !sigB64) return null;

  const enc = new TextEncoder();
  const key = await crypto.subtle.importKey(
    'raw', enc.encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['verify']
  );
  const sigInput = enc.encode(`${headerB64}.${payloadB64}`);
  const sig = base64urlDecode(sigB64);
  const valid = await crypto.subtle.verify('HMAC', key, sig, sigInput);
  if (!valid) return null;

  const payload = JSON.parse(atob(payloadB64.replace(/-/g, '+').replace(/_/g, '/')));
  if (payload.exp < Math.floor(Date.now() / 1000)) return null;
  return payload.sub;
}

async function verifyAuth(request, env) {
  const auth = request.headers.get('Authorization');
  if (!auth || !auth.startsWith('Bearer ')) return null;
  return verifyJwt(auth.slice(7), env.JWT_SECRET);
}

// ═══════════════════════════════════════════════════════════════════════════
// COMMUNITY
// ═══════════════════════════════════════════════════════════════════════════

async function handleCommunityStats(env, cors) {
  try {
    // Count registered users by listing KV keys with username: prefix
    let userCount = 0;
    let cursor = undefined;
    do {
      const list = await env.USERS.list({ prefix: 'username:', limit: 1000, cursor });
      userCount += list.keys.length;
      cursor = list.list_complete ? undefined : list.cursor;
    } while (cursor);

    return json({
      total_users: userCount,
      total_simulations: 0,
      total_plays: 0,
      online_now: Math.max(1, userCount),
      total_bliss_distributed: 0,
      timestamp: new Date().toISOString(),
    }, 200, cors);
  } catch (e) {
    // Graceful fallback if KV is unavailable
    return json({
      total_users: 0,
      total_simulations: 0,
      total_plays: 0,
      online_now: 0,
      total_bliss_distributed: 0,
      timestamp: new Date().toISOString(),
    }, 200, cors);
  }
}

async function handleCommunitySearch(request, env, cors) {
  const url = new URL(request.url);
  const query = url.searchParams.get('q') || '';
  const limit = Math.min(parseInt(url.searchParams.get('limit') || '10'), 20);

  if (!query || query.length < 2)
    return json({ users: [], query }, 200, cors);

  // Search Cloudflare KV for usernames matching the query
  const results = [];
  const list = await env.USERS.list({ prefix: `username:`, limit: 1000 });

  for (const key of list.keys) {
    const username = key.name.replace('username:', '');
    if (username.toLowerCase().includes(query.toLowerCase())) {
      const userId = await env.USERS.get(key.name);
      if (userId) {
        const userData = await env.USERS.get(`user:${userId}`);
        if (userData) {
          const user = JSON.parse(userData);
          results.push({
            username: user.username,
            display_name: user.username,
            avatar_url: user.avatar_url || null,
            is_verified: !!user.id_hash,
            follower_count: 0,
            created_at: user.created_at,
          });
        }
      }
      if (results.length >= limit) break;
    }
  }

  // If no KV results and Grok is configured, try AI-enhanced search
  if (results.length === 0 && env.GROK_API_KEY) {
    try {
      const grokResults = await searchWithGrok(query, env.GROK_API_KEY);
      if (grokResults) {
        return json({ users: results, query, ai_suggestion: grokResults }, 200, cors);
      }
    } catch (e) {
      // Log it. Swallowing this made a failing xAI integration indistinguishable
      // from a search that simply found nothing.
      console.error('xAI search threw:', e?.name, e?.message);
    }
  }

  return json({ users: results, query }, 200, cors);
}

async function searchWithGrok(query, apiKey) {
  const resp = await grokFetch({
    input: `The user is searching for "${query}" on the Eustress Engine community platform. Eustress is a Rust-based simulation and data platform with a Bliss currency. Suggest what they might be looking for: a username, a simulation, a feature, or a concept. Reply in 1-2 short sentences only.`,
  }, apiKey);

  // Log upstream failures. Swallowing them silently hid a broken model id and
  // an expired key behind an ordinary-looking empty search result.
  if (!resp.ok) {
    console.error('xAI search failed:', resp.status, (await resp.text()).slice(0, 300));
    return null;
  }
  const data = await resp.json();
  return extractGrokText(data) || null;
}

async function handleCommunityLeaderboard(env, cors) {
  try {
  // Build leaderboard from KV users — sorted by hours, filterable by period
  const period = 'alltime'; // No request param available — default to alltime

  const entries = [];
  const list = await env.USERS.list({ prefix: 'user:', limit: 1000 });

  const now = new Date();

  for (const key of list.keys) {
    const userData = await env.USERS.get(key.name);
    if (!userData) continue;
    const user = JSON.parse(userData);
    if (user.banned) continue;

    // All-time hours now live in SOCIAL `hours_total:{id}` — the heartbeat
    // stopped writing them back onto the user record (a stale whole-record
    // PUT there could erase distributed BLS). Fall back to the legacy field
    // for accounts that predate the split.
    const hoursTotalRaw = await env.SOCIAL.get(`hours_total:${user.id}`);
    let hours = hoursTotalRaw !== null && hoursTotalRaw !== undefined
      ? parseFloat(hoursTotalRaw) || 0
      : (user.total_hours || 0);

    // For time-filtered periods, sum daily hour entries
    if (period !== 'alltime') {
      hours = 0;
      const daysBack = period === 'today' ? 1 : period === 'week' ? 7 : 30;
      for (let d = 0; d < daysBack; d++) {
        const date = new Date(now);
        date.setDate(date.getDate() - d);
        const dateStr = date.toISOString().split('T')[0];
        const dKey = `hours_daily:${user.id}:${dateStr}`;
        const val = await env.SOCIAL.get(dKey);
        if (val) hours += parseFloat(val);
      }
    }

    // Count published simulations
    const simList = await env.INVENTORY.list({ prefix: `sim:${user.id}:`, limit: 100 });
    const spacesCreated = simList.keys.length;

    entries.push({
      username: user.username,
      avatar_url: user.avatar_url || null,
      hours: Math.round(hours * 10) / 10,
      spaces_created: spacesCreated,
      total_visits: user.total_visits || 0,
      last_active: user.last_active || user.created_at,
    });
  }

  // Sort by hours descending
  entries.sort((a, b) => b.hours - a.hours);
  const top = entries.slice(0, 20).map((e, i) => ({
    rank: i + 1,
    username: e.username,
    avatar_url: e.avatar_url,
    hours: e.hours,
    spaces_created: e.spaces_created,
    total_visits: e.total_visits,
  }));

  // Pick a random featured creator from top 20 (changes weekly via date seed)
  const weekSeed = Math.floor(now.getTime() / (7 * 86400000));
  const featuredIndex = weekSeed % Math.max(top.length, 1);
  const featured = top[featuredIndex] || top[0] || null;

  return json({ entries: top, featured, period, total: entries.length }, 200, cors);
  } catch (e) {
    return json({ entries: [], featured: null, period: 'alltime', total: 0 }, 200, cors);
  }
}

async function handleUserProfile(username, request, env, cors) {
  if (!username || username.length < 2)
    return json({ error: 'Invalid username' }, 400, cors);

  const userId = await env.USERS.get(`username:${username}`);
  if (!userId) return json({ error: 'User not found' }, 404, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User data missing' }, 500, cors);

  const user = JSON.parse(userData);
  const created = new Date(user.created_at);
  const joinDate = created.toLocaleDateString('en-US', { month: 'long', year: 'numeric' });

  // Pull social stats from SOCIAL KV
  const followers = JSON.parse(await env.SOCIAL.get(`followers:${userId}`) || '[]');
  const following = JSON.parse(await env.SOCIAL.get(`following:${userId}`) || '[]');
  const favorites = JSON.parse(await env.SOCIAL.get(`favorites:${userId}`) || '[]');

  // Friends = mutual follows
  const followersSet = new Set(followers);
  const friends = following.filter(id => followersSet.has(id));

  // Play counter + simulation count
  const totalPlays = parseInt(await env.SOCIAL.get(`totalPlays:${userId}`) || '0');
  const simCount = parseInt(await env.SOCIAL.get(`simCount:${userId}`) || '0');

  // Inventory count
  const inventory = JSON.parse(await env.INVENTORY.get(`inventory:${userId}`) || '[]');

  // Check if requesting user follows this profile
  let isFollowing = false;
  const viewerUserId = await verifyAuth(request, env);
  if (viewerUserId) {
    const viewerFollowing = JSON.parse(await env.SOCIAL.get(`following:${viewerUserId}`) || '[]');
    isFollowing = viewerFollowing.includes(userId);
  }

  // Compute badges
  const badges = computeBadges(user, followers.length, simCount, totalPlays, inventory.length);

  return json({
    username: user.username,
    display_name: user.username,
    bio: user.bio || '',
    avatar_url: user.avatar_url || null,
    banner_url: null,
    join_date: joinDate,
    follower_count: followers.length,
    following_count: following.length,
    friend_count: friends.length,
    simulation_count: simCount,
    total_plays: totalPlays,
    favorite_count: favorites.length,
    inventory_count: inventory.length,
    badges,
    is_verified: !!user.id_hash,
    is_following: isFollowing,
    discord_linked: false,
    created_at: user.created_at,
  }, 200, cors);
}

function computeBadges(user, followerCount, simCount, totalPlays, inventoryCount) {
  const badges = [];
  const created = new Date(user.created_at);
  const ageMs = Date.now() - created.getTime();
  const ageYears = ageMs / (365.25 * 24 * 60 * 60 * 1000);

  if (user.role === 'admin')
    badges.push({ id: 'admin', name: 'Administrator', icon: '⚡', description: 'Platform administrator' });
  if (user.id_hash)
    badges.push({ id: 'verified', name: 'Verified', icon: '✓', description: 'Identity verified via KYC' });
  if (created < new Date('2030-01-01'))
    badges.push({ id: 'early', name: 'Early Adopter', icon: '🚀', description: 'Joined before 2030' });
  if (ageYears >= 1)
    badges.push({ id: 'veteran', name: 'Veteran', icon: '🏆', description: 'Member for 1+ year' });
  if (simCount >= 1)
    badges.push({ id: 'creator', name: 'Creator', icon: '🎨', description: 'Published 1+ simulation' });
  if (totalPlays >= 1000)
    badges.push({ id: 'popular', name: 'Popular', icon: '⭐', description: '1K+ total plays' });
  if (followerCount >= 100)
    badges.push({ id: 'social', name: 'Social', icon: '🦋', description: '100+ followers' });
  if (inventoryCount >= 10)
    badges.push({ id: 'collector', name: 'Collector', icon: '📦', description: '10+ marketplace items' });

  return badges;
}

// ═══════════════════════════════════════════════════════════════════════════
// SOCIAL — Follow, Unfollow, Favorite, Play Counter
// ═══════════════════════════════════════════════════════════════════════════

async function handleFollow(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const { username } = await request.json();
  const targetId = await env.USERS.get(`username:${username}`);
  if (!targetId) return json({ error: 'User not found' }, 404, cors);
  if (targetId === userId) return json({ error: 'Cannot follow yourself' }, 400, cors);

  // Add to my following
  const following = JSON.parse(await env.SOCIAL.get(`following:${userId}`) || '[]');
  if (!following.includes(targetId)) {
    following.push(targetId);
    await env.SOCIAL.put(`following:${userId}`, JSON.stringify(following));
  }

  // Add me to their followers
  const followers = JSON.parse(await env.SOCIAL.get(`followers:${targetId}`) || '[]');
  if (!followers.includes(userId)) {
    followers.push(userId);
    await env.SOCIAL.put(`followers:${targetId}`, JSON.stringify(followers));
  }

  return json({ success: true, following_count: following.length }, 200, cors);
}

async function handleUnfollow(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const { username } = await request.json();
  const targetId = await env.USERS.get(`username:${username}`);
  if (!targetId) return json({ error: 'User not found' }, 404, cors);

  // Remove from my following
  let following = JSON.parse(await env.SOCIAL.get(`following:${userId}`) || '[]');
  following = following.filter(id => id !== targetId);
  await env.SOCIAL.put(`following:${userId}`, JSON.stringify(following));

  // Remove me from their followers
  let followers = JSON.parse(await env.SOCIAL.get(`followers:${targetId}`) || '[]');
  followers = followers.filter(id => id !== userId);
  await env.SOCIAL.put(`followers:${targetId}`, JSON.stringify(followers));

  return json({ success: true, following_count: following.length }, 200, cors);
}

async function handleFavorite(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const { simulation_id } = await request.json();
  if (!simulation_id) return json({ error: 'Missing simulation_id' }, 400, cors);

  const favorites = JSON.parse(await env.SOCIAL.get(`favorites:${userId}`) || '[]');
  if (!favorites.includes(simulation_id)) {
    favorites.push(simulation_id);
    await env.SOCIAL.put(`favorites:${userId}`, JSON.stringify(favorites));
  }

  return json({ success: true, favorite_count: favorites.length }, 200, cors);
}

async function handleUnfavorite(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const { simulation_id } = await request.json();
  let favorites = JSON.parse(await env.SOCIAL.get(`favorites:${userId}`) || '[]');
  favorites = favorites.filter(id => id !== simulation_id);
  await env.SOCIAL.put(`favorites:${userId}`, JSON.stringify(favorites));

  return json({ success: true, favorite_count: favorites.length }, 200, cors);
}

async function handlePlay(request, env, cors) {
  const { simulation_id, author_id } = await request.json();
  if (!simulation_id) return json({ error: 'Missing simulation_id' }, 400, cors);

  // Increment per-simulation play counter
  const simPlays = parseInt(await env.SOCIAL.get(`plays:${simulation_id}`) || '0') + 1;
  await env.SOCIAL.put(`plays:${simulation_id}`, simPlays.toString());

  // Increment author's total plays
  if (author_id) {
    const authorPlays = parseInt(await env.SOCIAL.get(`totalPlays:${author_id}`) || '0') + 1;
    await env.SOCIAL.put(`totalPlays:${author_id}`, authorPlays.toString());
  }

  return json({ success: true, plays: simPlays }, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// INVENTORY
// ═══════════════════════════════════════════════════════════════════════════

async function handleGetInventory(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const inventory = JSON.parse(await env.INVENTORY.get(`inventory:${userId}`) || '[]');
  return json({ items: inventory, count: inventory.length }, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// AI SCREENING — Grok-powered criminal record verdict analysis
// ═══════════════════════════════════════════════════════════════════════════

/**
 * Perform AI screening via Grok.
 * Analyzes ONLY criminal record VERDICTS (not arrests, charges, or accusations).
 * Returns: { decision: APPROVE|REVIEW|DENY, risk_score: 0-100, reason, flags }
 */
async function performScreening(username, birthday, idType, idHash, env) {
  const screened_at = new Date().toISOString();

  if (!env.GROK_API_KEY) {
    return { decision: 'APPROVE', risk_score: 0, reason: 'Screening unavailable', flags: [], screened_at };
  }

  try {
    const prompt = `You are a background screening AI for Eustress, a game engine platform.
Your job is to assess risk based ONLY on publicly available criminal record VERDICTS (final court decisions).
Do NOT consider arrests, charges, accusations, or pending cases — ONLY convictions/verdicts.

Evaluate this registration:
- Username: ${username}
- Date of Birth: ${birthday}
- ID Type: ${idType || 'not provided'}

Based on publicly available information about this person (if identifiable), assess:
1. Are there any criminal VERDICT records associated with this identity?
2. If yes, what is the severity? (misdemeanor vs felony, violent vs non-violent)
3. What is the recency? (recent vs years ago)

Respond in EXACTLY this JSON format, nothing else:
{
  "risk_score": <0-100>,
  "decision": "<APPROVE|REVIEW|DENY>",
  "reason": "<one sentence explanation>",
  "flags": [<list of specific concerns, empty if none>],
  "verdict_found": <true|false>,
  "details": "<brief details if verdicts found, empty string if none>"
}

Rules:
- risk_score 0-20: APPROVE (clean or minor non-violent misdemeanor 5+ years ago)
- risk_score 21-60: REVIEW (non-violent felony, recent misdemeanor, pattern of offenses)
- risk_score 61-100: DENY (violent felony, sex offense, fraud conviction)
- If you cannot identify the person or find no records: risk_score 0, APPROVE
- A username alone is NOT enough to identify someone — require matching DOB AND full legal name
- Never deny based on name similarity alone
- Err on the side of approval — innocent until proven guilty by verdict
- If you are NOT CERTAIN this is the same person, set verdict_found to false and APPROVE
- Do NOT hallucinate or fabricate records. If unsure, say "no records found"
- A username is a chosen handle, NOT a legal name. Do not search for the username as a real name.
- Only consider PUBLICLY DOCUMENTED court verdicts from official sources`;

    const resp = await grokFetch({ input: prompt }, env.GROK_API_KEY);

    if (!resp.ok) {
      return { decision: 'APPROVE', risk_score: 0, reason: 'Screening service unavailable', flags: [], screened_at };
    }

    const data = await resp.json();
    const responseText = extractGrokText(data);

    // Parse JSON from response
    const jsonMatch = responseText.match(/\{[\s\S]*\}/);
    if (!jsonMatch) {
      return { decision: 'APPROVE', risk_score: 0, reason: 'Could not parse screening response', flags: [], screened_at };
    }

    const result = JSON.parse(jsonMatch[0]);

    return {
      decision: result.decision || 'APPROVE',
      risk_score: Math.min(100, Math.max(0, result.risk_score || 0)),
      reason: result.reason || 'No concerns found',
      flags: result.flags || [],
      verdict_found: result.verdict_found || false,
      details: result.details || '',
      screened_at,
      model: GROK_MODEL,
    };
  } catch (e) {
    return { decision: 'APPROVE', risk_score: 0, reason: `Screening error: ${e.message}`, flags: [], screened_at };
  }
}

// Pre-registration screening check (can be called separately before register)
async function handleScreeningCheck(request, env, cors) {
  const { username, birthday, id_type, id_hash } = await request.json();
  if (!username || !birthday)
    return json({ error: 'Username and birthday required for screening' }, 400, cors);

  // Rate limit: max 5 screening checks per IP per hour
  const clientIp = request.headers.get('cf-connecting-ip') || 'unknown';
  const rateLimitKey = `screen-rate:${clientIp}`;
  const currentCount = parseInt(await env.SCREENING.get(rateLimitKey) || '0');
  if (currentCount >= 5) {
    return json({ error: 'Rate limit exceeded. Try again later.' }, 429, cors);
  }
  await env.SCREENING.put(rateLimitKey, (currentCount + 1).toString(), { expirationTtl: 3600 });

  const result = await performScreening(username, birthday, id_type, id_hash, env);

  // Add legal disclaimer
  result.disclaimer = 'AI-assisted screening. Not a legal determination. Based on publicly available information only. Results may contain errors. Contact support@eustress.dev to dispute.';

  // Store result
  const key = `screen:${id_hash || username}`;
  await env.SCREENING.put(key, JSON.stringify(result), { expirationTtl: 86400 * 365 });

  return json(result, 200, cors);
}

// Get screening status for a user
async function handleScreeningStatus(request, env, cors) {
  const url = new URL(request.url);
  const userId = url.searchParams.get('user_id');
  const idHash = url.searchParams.get('id_hash');

  if (!userId && !idHash)
    return json({ error: 'user_id or id_hash required' }, 400, cors);

  // Check by user data
  if (userId) {
    const userData = await env.USERS.get(`user:${userId}`);
    if (userData) {
      const user = JSON.parse(userData);
      return json({
        risk_score: user.risk_score || 0,
        decision: user.risk_decision || 'UNSCREENED',
        last_screened: user.last_screened || null,
        flags: user.screening_flags || [],
      }, 200, cors);
    }
  }

  // Check by screening record
  const key = `screen:${idHash || userId}`;
  const data = await env.SCREENING.get(key);
  if (data) return json(JSON.parse(data), 200, cors);

  return json({ error: 'No screening record found' }, 404, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// ADMIN — Moderation tools (requires admin role)
// ═══════════════════════════════════════════════════════════════════════════

// Admin user IDs (hardcoded for now — move to KV later)
const ADMIN_USERS = new Set([
  // Add your user ID here after registering
]);

async function requireAdmin(request, env) {
  const userId = await verifyAuth(request, env);
  if (!userId) return null;

  // Check admin role in KV
  const adminFlag = await env.USERS.get(`admin:${userId}`);
  if (adminFlag) return userId;

  // Check user record for admin role
  const userData = await env.USERS.get(`user:${userId}`);
  if (userData) {
    const user = JSON.parse(userData);
    if (user.role === 'admin') return userId;
  }

  return null; // Not an admin
}

async function auditLog(env, action, adminId, target, details) {
  const entry = {
    action,
    admin_id: adminId,
    target,
    details,
    timestamp: new Date().toISOString(),
  };
  const key = `audit:${Date.now()}-${crypto.randomUUID().slice(0, 8)}`;
  await env.AUDIT_LOG.put(key, JSON.stringify(entry), { expirationTtl: 86400 * 365 * 5 }); // 5 year retention
}

async function handleAdminListUsers(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const list = await env.USERS.list({ prefix: 'user:', limit: 100 });
  const users = [];
  for (const key of list.keys) {
    const data = await env.USERS.get(key.name);
    if (data) {
      const user = JSON.parse(data);
      users.push({
        id: user.id, username: user.username,
        risk_score: user.risk_score || 0, risk_decision: user.risk_decision || 'UNSCREENED',
        banned: user.banned || false, created_at: user.created_at,
        warnings: (user.warnings || []).length,
      });
    }
  }
  return json({ users, total: users.length }, 200, cors);
}

// ── Admin Analytics: "unique visits per sign-up" funnel ─────────────────────
// Sign-ups are derived from the USERS KV (created_at). Visit counters come from
// the optional ANALYTICS KV (populated by POST /api/analytics/hit). If the
// ANALYTICS binding isn't bound yet, visits report as zero and the endpoint
// still returns the full sign-up picture — so it's useful before collection
// is wired. NOTE: KV counters use read-modify-write and are eventually
// consistent; fine for an early-stage, low-traffic site with a single admin.
// At higher volume, swap the visit store for Workers Analytics Engine or D1.
async function handleAdminStats(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  // --- Sign-ups: paginated scan of `user:` records in USERS KV ---
  const byDay = {}, byDecision = {}, byIdType = {};
  let total = 0, admins = 0, banned = 0, withEmail = 0, stripeConnected = 0, blissTotal = 0;
  const MAX = 5000; // safety cap so one admin call can't scan unbounded
  let cursor;
  do {
    const list = await env.USERS.list({ prefix: 'user:', limit: 1000, cursor });
    for (const key of list.keys) {
      if (total >= MAX) break;
      const raw = await env.USERS.get(key.name);
      if (!raw) continue;
      let u; try { u = JSON.parse(raw); } catch { continue; }
      total++;
      if (u.role === 'admin') admins++;
      if (u.banned) banned++;
      if (u.email) withEmail++;
      if (u.stripe_connect_id) stripeConnected++;
      blissTotal += Number(u.bliss_balance || 0);
      const day = (u.created_at || '').slice(0, 10);
      if (day) byDay[day] = (byDay[day] || 0) + 1;
      byDecision[u.risk_decision || 'UNSCREENED'] = (byDecision[u.risk_decision || 'UNSCREENED'] || 0) + 1;
      byIdType[u.id_type || 'none'] = (byIdType[u.id_type || 'none'] || 0) + 1;
    }
    cursor = list.list_complete ? undefined : list.cursor;
  } while (cursor && total < MAX);

  // --- Visits: pageview (pv:) and unique (uv:) day counters in ANALYTICS KV ---
  const pvByDay = {}, uvByDay = {};
  let totalPv = 0, totalUv = 0;
  if (env.ANALYTICS) {
    for (const [prefix, bucket] of [['pv:', pvByDay], ['uv:', uvByDay]]) {
      let c;
      do {
        const l = await env.ANALYTICS.list({ prefix, limit: 1000, cursor: c });
        for (const k of l.keys) {
          const day = k.name.slice(prefix.length);
          const v = Number(await env.ANALYTICS.get(k.name)) || 0;
          bucket[day] = v;
          if (prefix === 'pv:') totalPv += v; else totalUv += v;
        }
        c = l.list_complete ? undefined : l.cursor;
      } while (c);
    }
  }

  const series = (o) => Object.keys(o).sort().map(d => ({ date: d, count: o[d] }));
  const pairs  = (o) => Object.keys(o).sort((a, b) => o[b] - o[a]).map(k => ({ key: k, count: o[k] }));

  const visitsPerSignup = total > 0 ? totalUv / total : 0;          // unique visits ÷ sign-ups
  const conversionPct   = totalUv > 0 ? (total / totalUv) * 100 : 0; // sign-ups ÷ unique visits

  return json({
    generated_at: new Date().toISOString(),
    analytics_enabled: !!env.ANALYTICS,
    signups: {
      total,
      by_day: series(byDay),
      by_decision: pairs(byDecision),
      by_id_type: pairs(byIdType),
    },
    visits: {
      total_pageviews: totalPv,
      total_unique: totalUv,
      unique_by_day: series(uvByDay),
      pageviews_by_day: series(pvByDay),
    },
    funnel: {
      unique_visits_per_signup: Number(visitsPerSignup.toFixed(2)),
      signup_conversion_rate: Number(conversionPct.toFixed(2)),
    },
    accounts: {
      admins, banned, with_email: withEmail,
      stripe_connected: stripeConnected,
      bliss_total: blissTotal,
      capped: total >= MAX,
    },
  }, 200, cors);
}

// ── Public, cookieless pageview beacon ──────────────────────────────────────
// Visitor identity is a daily-rotating salted hash of IP+UA: no cookie, no
// stable cross-day identifier, nothing personal stored. No-ops gracefully if
// the ANALYTICS KV binding isn't present, and never throws into page load.
// ── Engine usage telemetry ──────────────────────────────────────────────────
// One aggregate per engine session (counts keyed by tool id), never raw click
// streams — nothing here can be replayed into a behavioural timeline. The
// install_id is a random UUID the engine generates locally; it is NEVER an
// account id and this worker never joins it to one. Storage is the TELEMETRY
// KV: `tool:<id>:clicks` running totals, `tinst:<id>:<install>` first-seen
// markers feeding `tool:<id>:installs`, `comment:<ts>:<rand>` feedback rows.
// KV read-modify-write counters are lossy under heavy concurrency — accepted
// exactly like the pv:/uv: counters above; alpha volumes make it moot.
// Everything no-ops gracefully when the TELEMETRY binding is absent.

const TELEMETRY_MAX_TOOLS = 400;     // per session; ~1,400 ids exist total
const TELEMETRY_MAX_COMMENT = 500;   // chars
const TELEMETRY_COMMENTS_PER_DAY = 20;

function telemetryValidId(s, max) {
  return typeof s === 'string' && s.length > 0 && s.length <= max && /^[\w:.-]+$/.test(s);
}

async function kvIncr(kv, key, by) {
  const cur = Number(await kv.get(key)) || 0;
  await kv.put(key, String(cur + by));
  return cur + by;
}

async function handleTelemetryUsage(request, env, cors) {
  if (!env.TELEMETRY) return json({ ok: true, recorded: false }, 200, cors);
  try {
    const body = await request.json();
    const installId = body.install_id;
    if (!telemetryValidId(installId, 64)) return json({ error: 'bad install_id' }, 400, cors);
    const counts = body.counts && typeof body.counts === 'object' ? body.counts : {};
    const entries = Object.entries(counts).slice(0, TELEMETRY_MAX_TOOLS);

    const day = new Date().toISOString().slice(0, 10);
    let recorded = 0;
    const touched = [];
    for (const [tool, n] of entries) {
      if (!telemetryValidId(tool, 128)) continue;
      const clicks = Math.min(Math.max(Number(n) || 0, 0), 10000);
      if (clicks <= 0) continue;
      await kvIncr(env.TELEMETRY, `tool:${tool}:clicks`, clicks);
      const seenKey = `tinst:${tool}:${installId}`;
      if (!(await env.TELEMETRY.get(seenKey))) {
        await env.TELEMETRY.put(seenKey, '1');
        await kvIncr(env.TELEMETRY, `tool:${tool}:installs`, 1);
      }
      touched.push([tool, clicks]);
      recorded++;
    }

    // One recency record per SESSION (never per tool — KV daily write budget is
    // the binding constraint, and the admin feed only needs session grain).
    // Ordered by zero-padded ts so `list` returns them chronologically.
    if (touched.length) {
      const ts = Date.now();
      const rand = Math.random().toString(36).slice(2, 8);
      touched.sort((a, b) => b[1] - a[1]);
      await env.TELEMETRY.put(
        `recent:${String(ts).padStart(14, '0')}:${rand}`,
        JSON.stringify({
          ts,
          install: installId.slice(0, 8),
          mode: telemetryValidId(body.mode, 64) ? body.mode : '',
          version: telemetryValidId(body.app_version, 32) ? body.app_version : '',
          tools: touched.slice(0, 12),
          total: touched.reduce((sum, t) => sum + t[1], 0),
        }),
        { expirationTtl: 86400 * 45 }
      );
    }

    // Site-wide session counter per day (cheap health signal).
    await kvIncr(env.TELEMETRY, `sessions:${day}`, 1);
    return json({ ok: true, recorded }, 200, cors);
  } catch (e) {
    return json({ ok: false }, 200, cors); // telemetry must never error a client
  }
}

async function handleTelemetryComment(request, env, cors) {
  if (!env.TELEMETRY) return json({ ok: true, recorded: false }, 200, cors);
  try {
    const body = await request.json();
    const installId = body.install_id;
    if (!telemetryValidId(installId, 64)) return json({ error: 'bad install_id' }, 400, cors);
    let text = typeof body.text === 'string' ? body.text : '';
    text = text.replace(/[\u0000-\u001f\u007f]/g, ' ').trim().slice(0, TELEMETRY_MAX_COMMENT);
    if (!text) return json({ error: 'empty comment' }, 400, cors);
    const tool = telemetryValidId(body.tool, 128) ? body.tool : '';

    // Per-install daily cap — feedback, not a firehose.
    const day = new Date().toISOString().slice(0, 10);
    const capKey = `climit:${installId}:${day}`;
    const used = Number(await env.TELEMETRY.get(capKey)) || 0;
    if (used >= TELEMETRY_COMMENTS_PER_DAY) return json({ error: 'daily limit' }, 429, cors);
    await env.TELEMETRY.put(capKey, String(used + 1), { expirationTtl: 86400 * 2 });

    const ts = Date.now();
    const rand = Math.random().toString(36).slice(2, 8);
    // Key sorts chronologically via zero-padded ts; summary reverses for newest-first.
    await env.TELEMETRY.put(
      `comment:${String(ts).padStart(14, '0')}:${rand}`,
      JSON.stringify({ ts, tool, text, install: installId.slice(0, 8) })
    );
    return json({ ok: true }, 200, cors);
  } catch (e) {
    return json({ ok: false }, 200, cors);
  }
}

async function handleTelemetrySummary(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);
  if (!env.TELEMETRY) {
    return json({ generated_at: new Date().toISOString(), enabled: false, tools: [], comments: [], sessions_by_day: [] }, 200, cors);
  }

  // Tool counters — `tool:<id>:clicks` / `tool:<id>:installs`.
  const tools = new Map();
  let cursor;
  for (let page = 0; page < 20; page++) {
    const l = await env.TELEMETRY.list({ prefix: 'tool:', limit: 1000, cursor });
    for (const k of l.keys) {
      const m = k.name.match(/^tool:(.+):(clicks|installs)$/);
      if (!m) continue;
      const rec = tools.get(m[1]) || { id: m[1], clicks: 0, installs: 0 };
      rec[m[2]] = Number(await env.TELEMETRY.get(k.name)) || 0;
      tools.set(m[1], rec);
    }
    if (l.list_complete) break;
    cursor = l.cursor;
  }

  // Latest comments (list is lexicographic = chronological by padded ts).
  const comments = [];
  cursor = undefined;
  const commentKeys = [];
  for (let page = 0; page < 10; page++) {
    const l = await env.TELEMETRY.list({ prefix: 'comment:', limit: 1000, cursor });
    commentKeys.push(...l.keys.map(k => k.name));
    if (l.list_complete) break;
    cursor = l.cursor;
  }
  for (const name of commentKeys.slice(-200).reverse()) {
    const v = await env.TELEMETRY.get(name);
    if (v) { try { comments.push(JSON.parse(v)); } catch {} }
  }

  // Recent sessions (newest first). Doubles as the per-tool recency source —
  // the ingest path deliberately writes no `tool:<id>:last` key, so "when was
  // this last clicked" is derived here instead of costing a write per tool.
  const recentKeys = [];
  cursor = undefined;
  for (let page = 0; page < 5; page++) {
    const l = await env.TELEMETRY.list({ prefix: 'recent:', limit: 1000, cursor });
    recentKeys.push(...l.keys.map(k => k.name));
    if (l.list_complete) break;
    cursor = l.cursor;
  }
  const recent = [];
  for (const name of recentKeys.slice(-60).reverse()) {
    const v = await env.TELEMETRY.get(name);
    if (v) { try { recent.push(JSON.parse(v)); } catch {} }
  }
  const lastSeen = new Map();
  for (const r of recent) {
    for (const [id] of r.tools || []) if (!lastSeen.has(id)) lastSeen.set(id, r.ts);
  }

  // Session counts, last 14 days.
  const sessions_by_day = [];
  for (let i = 13; i >= 0; i--) {
    const d = new Date(Date.now() - i * 86400000).toISOString().slice(0, 10);
    sessions_by_day.push({ date: d, count: Number(await env.TELEMETRY.get(`sessions:${d}`)) || 0 });
  }

  return json({
    generated_at: new Date().toISOString(),
    enabled: true,
    tools: [...tools.values()]
      .map(t => ({ ...t, last: lastSeen.get(t.id) || 0 }))
      .sort((a, b) => b.installs - a.installs || b.clicks - a.clicks),
    comments,
    recent,
    sessions_by_day,
  }, 200, cors);
}

async function handleAnalyticsHit(request, env, cors) {
  if (!env.ANALYTICS) return json({ ok: true, recorded: false }, 200, cors);
  try {
    const day = new Date().toISOString().slice(0, 10);
    const ip = request.headers.get('cf-connecting-ip') || '';
    const ua = request.headers.get('user-agent') || '';
    const salt = env.ANALYTICS_SALT || 'eustress';
    const buf = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(`${day}|${ip}|${ua}|${salt}`));
    const hash = [...new Uint8Array(buf)].slice(0, 8).map(b => b.toString(16).padStart(2, '0')).join('');

    const pvKey = `pv:${day}`;
    await env.ANALYTICS.put(pvKey, String((Number(await env.ANALYTICS.get(pvKey)) || 0) + 1), { expirationTtl: 86400 * 400 });

    const seenKey = `seen:${day}:${hash}`;
    if (!(await env.ANALYTICS.get(seenKey))) {
      await env.ANALYTICS.put(seenKey, '1', { expirationTtl: 86400 * 2 });
      const uvKey = `uv:${day}`;
      await env.ANALYTICS.put(uvKey, String((Number(await env.ANALYTICS.get(uvKey)) || 0) + 1), { expirationTtl: 86400 * 400 });
    }
    return json({ ok: true, recorded: true }, 200, cors);
  } catch (e) {
    return json({ ok: false }, 200, cors); // analytics must never break a page load
  }
}

async function handleAdminBan(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const { username, reason } = await request.json();
  const userId = await env.USERS.get(`username:${username}`);
  if (!userId) return json({ error: 'User not found' }, 404, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User data missing' }, 500, cors);

  const user = JSON.parse(userData);
  user.banned = true;
  user.ban_reason = reason || 'Banned by admin';
  user.banned_at = new Date().toISOString();
  user.banned_by = adminId;

  await env.USERS.put(`user:${userId}`, JSON.stringify(user));
  await auditLog(env, 'BAN', adminId, username, { reason: user.ban_reason });

  return json({ success: true, username, banned: true, reason: user.ban_reason }, 200, cors);
}

async function handleAdminWarn(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const { username, message } = await request.json();
  const userId = await env.USERS.get(`username:${username}`);
  if (!userId) return json({ error: 'User not found' }, 404, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User data missing' }, 500, cors);

  const user = JSON.parse(userData);
  if (!user.warnings) user.warnings = [];
  user.warnings.push({
    message: message || 'Warning from admin',
    issued_at: new Date().toISOString(),
    issued_by: adminId,
  });

  await env.USERS.put(`user:${userId}`, JSON.stringify(user));
  await auditLog(env, 'WARN', adminId, username, { message: message || 'Warning from admin' });

  return json({ success: true, username, warning_count: user.warnings.length }, 200, cors);
}

async function handleAdminReview(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const { username, decision, notes } = await request.json();
  const userId = await env.USERS.get(`username:${username}`);
  if (!userId) return json({ error: 'User not found' }, 404, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User data missing' }, 500, cors);

  const user = JSON.parse(userData);
  user.risk_decision = decision || user.risk_decision;
  user.admin_review = {
    reviewed_by: adminId,
    reviewed_at: new Date().toISOString(),
    decision,
    notes: notes || '',
  };

  await env.USERS.put(`user:${userId}`, JSON.stringify(user));
  await auditLog(env, 'REVIEW', adminId, username, { decision, notes: notes || '' });

  return json({ success: true, username, decision }, 200, cors);
}

async function handleAdminRiskOverride(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const { username, risk_score, decision, reason } = await request.json();
  const userId = await env.USERS.get(`username:${username}`);
  if (!userId) return json({ error: 'User not found' }, 404, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User data missing' }, 500, cors);

  const user = JSON.parse(userData);
  user.risk_score = risk_score ?? user.risk_score;
  user.risk_decision = decision || user.risk_decision;
  user.risk_override = {
    overridden_by: adminId,
    overridden_at: new Date().toISOString(),
    previous_score: user.risk_score,
    new_score: risk_score,
    reason: reason || 'Admin override',
  };

  await env.USERS.put(`user:${userId}`, JSON.stringify(user));
  await auditLog(env, 'RISK_OVERRIDE', adminId, username, { risk_score, decision, reason });

  return json({ success: true, username, risk_score, decision }, 200, cors);
}

// Re-screen a specific user
async function handleAdminRescreen(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const { username } = await request.json();
  const userId = await env.USERS.get(`username:${username}`);
  if (!userId) return json({ error: 'User not found' }, 404, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User data missing' }, 500, cors);

  const user = JSON.parse(userData);

  // Cooldown: minimum 30 days between re-screens
  if (user.last_screened) {
    const lastScreened = new Date(user.last_screened);
    const daysSince = (Date.now() - lastScreened.getTime()) / (86400 * 1000);
    if (daysSince < 30) {
      return json({
        error: `Re-screening cooldown: ${Math.ceil(30 - daysSince)} days remaining`,
        last_screened: user.last_screened,
      }, 429, cors);
    }
  }

  // Re-screen with Grok
  const result = await performScreening(user.username, user.birthday, user.id_type, user.id_hash, env);

  // Track delta
  const previousScore = user.risk_score || 0;
  const delta = result.risk_score - previousScore;

  user.risk_score = result.risk_score;
  user.risk_decision = result.decision;
  user.last_screened = result.screened_at;
  user.screening_flags = result.flags;
  if (!user.screening_history) user.screening_history = [];
  user.screening_history.push({
    date: result.screened_at,
    score: result.risk_score,
    decision: result.decision,
    delta,
    triggered_by: 'admin_rescreen',
  });

  await env.USERS.put(`user:${userId}`, JSON.stringify(user));

  // Store screening record
  await env.SCREENING.put(`screen:${user.id_hash || user.public_key}`, JSON.stringify(result));

  return json({
    success: true, username, ...result,
    delta, previous_score: previousScore,
  }, 200, cors);
}

// Delete a user account — removes all KV entries so they can re-register
async function handleAdminDeleteUser(request, env, cors) {
  const { username } = await request.json();
  if (!username) return json({ error: 'Username required' }, 400, cors);

  // Look up user ID from username
  const userId = await env.USERS.get(`username:${username}`);
  if (!userId) return json({ error: 'User not found' }, 404, cors);

  // Load user data to get public_key and id_hash for cleanup
  const userData = await env.USERS.get(`user:${userId}`);
  const user = userData ? JSON.parse(userData) : {};

  // Delete all KV entries for this user
  await env.USERS.delete(`user:${userId}`);
  await env.USERS.delete(`username:${username}`);
  if (user.public_key) await env.USERS.delete(`pubkey:${user.public_key}`);
  if (user.id_hash) await env.USERS.delete(`idhash:${user.id_hash}`);

  // Clean up social data
  await env.SOCIAL.delete(`followers:${userId}`);
  await env.SOCIAL.delete(`following:${userId}`);
  await env.SOCIAL.delete(`favorites:${userId}`);
  await env.SOCIAL.delete(`plays:${userId}`);

  return json({ success: true, username, user_id: userId }, 200, cors);
}

// Get screening report — all users with risk scores
async function handleAdminScreeningReport(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const list = await env.USERS.list({ prefix: 'user:', limit: 1000 });
  const report = { total: 0, approved: 0, review: 0, denied: 0, unscreened: 0, banned: 0, users: [] };

  for (const key of list.keys) {
    const data = await env.USERS.get(key.name);
    if (!data) continue;
    const user = JSON.parse(data);
    report.total++;

    const decision = user.risk_decision || 'UNSCREENED';
    if (decision === 'APPROVE') report.approved++;
    else if (decision === 'REVIEW') report.review++;
    else if (decision === 'DENY') report.denied++;
    else report.unscreened++;
    if (user.banned) report.banned++;

    report.users.push({
      id: user.id, username: user.username,
      risk_score: user.risk_score || 0,
      risk_decision: decision,
      last_screened: user.last_screened || null,
      flags: user.screening_flags || [],
      banned: user.banned || false,
      warnings: (user.warnings || []).length,
      screening_history: user.screening_history || [],
    });
  }

  // Sort by risk score descending
  report.users.sort((a, b) => b.risk_score - a.risk_score);

  return json(report, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// STRIPE — Treasury funding + Connect payouts
// ═══════════════════════════════════════════════════════════════════════════

/// `idempotencyKey` (optional) makes a retried POST safe: Stripe returns the
/// ORIGINAL result instead of performing the action again. Required for
/// anything that moves money (see runDailyPayout).
async function stripeRequest(method, endpoint, body, env, idempotencyKey) {
  const headers = {
    'Authorization': `Bearer ${env.STRIPE_SECRET_KEY}`,
    'Content-Type': 'application/x-www-form-urlencoded',
  };
  if (idempotencyKey) headers['Idempotency-Key'] = idempotencyKey;
  const resp = await fetch(`https://api.stripe.com/v1${endpoint}`, {
    method,
    headers,
    body: body ? new URLSearchParams(body).toString() : undefined,
  });
  return resp.json();
}

// Stripe price IDs
const STRIPE_PRICES = {
  seed_one_time: 'price_1THyC7RgsC7hEeKMmD71Pdcm',
  growth_one_time: 'price_1THyC7RgsC7hEeKMc59jIIrA',
  sustainer_one_time: 'price_1THyC8RgsC7hEeKMaQTcj58e',
  patron_one_time: 'price_1THyC9RgsC7hEeKMioXlaWy0',
  seed_recurring: 'price_1THyC9RgsC7hEeKMLdz2AAlz',
  growth_recurring: 'price_1THyCARgsC7hEeKMvMQWDog5',
  sustainer_recurring: 'price_1THyCARgsC7hEeKM6QpYML0j',
  patron_recurring: 'price_1THyCBRgsC7hEeKMLAlLI8pS',
};

// Platform fee: 2.5%
const PLATFORM_FEE_PERCENT = 2.5;

// Create Stripe Checkout session for treasury funding
async function handleStripeCheckout(request, env, cors) {
  if (!env.STRIPE_SECRET_KEY) return json({ error: 'Stripe not configured' }, 503, cors);

  const userId = await verifyAuth(request, env);
  const { tier, mode, custom_amount } = await request.json();
  // tier: 'seed'|'growth'|'sustainer'|'patron' or null for custom
  // mode: 'one_time' or 'recurring'

  const isRecurring = mode === 'recurring';
  let priceId;
  let amountCents;

  if (tier && STRIPE_PRICES[`${tier}_${isRecurring ? 'recurring' : 'one_time'}`]) {
    priceId = STRIPE_PRICES[`${tier}_${isRecurring ? 'recurring' : 'one_time'}`];
  } else if (custom_amount && custom_amount >= 5) {
    amountCents = Math.round(custom_amount * 100);
  } else {
    return json({ error: 'Select a tier or enter a custom amount ($5 minimum)' }, 400, cors);
  }

  const params = {
    'mode': isRecurring ? 'subscription' : 'payment',
    'success_url': 'https://eustress.dev/bliss?funded=true',
    'cancel_url': 'https://eustress.dev/bliss?funded=false',
  };

  if (priceId) {
    params['line_items[0][price]'] = priceId;
    params['line_items[0][quantity]'] = '1';
  } else {
    // Custom amount — create price inline
    params['line_items[0][price_data][currency]'] = 'usd';
    params['line_items[0][price_data][product]'] = 'prod_UGVCI0rliegrSC';
    params['line_items[0][price_data][unit_amount]'] = amountCents.toString();
    params['line_items[0][quantity]'] = '1';
    if (isRecurring) {
      params['line_items[0][price_data][recurring][interval]'] = 'month';
    }
  }

  // 2.5% platform fee (Stripe collects this for us via Connect)
  if (amountCents) {
    const feeCents = Math.round(amountCents * PLATFORM_FEE_PERCENT / 100);
    params['payment_intent_data[application_fee_amount]'] = feeCents.toString();
  }

  if (userId) {
    params['metadata[user_id]'] = userId;
    params['client_reference_id'] = userId;
  }

  const session = await stripeRequest('POST', '/checkout/sessions', params, env);

  if (session.error) {
    return json({ error: session.error.message, type: session.error.type }, 400, cors);
  }

  return json({ url: session.url, session_id: session.id }, 200, cors);
}

// Stripe webhook — handle successful payments
/// Verify a Stripe webhook signature (scheme v1: HMAC-SHA256 over
/// `{timestamp}.{raw body}` keyed by the endpoint secret).
///
/// Constant-time compare, and a 5-minute timestamp tolerance so a captured
/// delivery can't be replayed indefinitely.
async function stripeSignatureValid(rawBody, sigHeader, secret) {
  if (!sigHeader || !secret) return false;
  const parts = Object.fromEntries(
    sigHeader.split(',').map((kv) => {
      const i = kv.indexOf('=');
      return [kv.slice(0, i).trim(), kv.slice(i + 1).trim()];
    })
  );
  const t = parts['t'];
  const v1 = parts['v1'];
  if (!t || !v1) return false;

  const age = Math.abs(Date.now() / 1000 - Number(t));
  if (!Number.isFinite(age) || age > 300) return false;

  const enc = new TextEncoder();
  const key = await crypto.subtle.importKey(
    'raw', enc.encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']
  );
  const mac = await crypto.subtle.sign('HMAC', key, enc.encode(`${t}.${rawBody}`));
  const expected = hexEncode(new Uint8Array(mac));

  if (expected.length !== v1.length) return false;
  let diff = 0;
  for (let i = 0; i < expected.length; i++) diff |= expected.charCodeAt(i) ^ v1.charCodeAt(i);
  return diff === 0;
}

async function handleStripeWebhook(request, env) {
  const body = await request.text();

  // SECURITY: this endpoint mints Tickets and credits the USD treasury, so an
  // unverified body is a direct "print money" primitive — previously anyone
  // could POST a fake checkout.session.completed and inflate the treasury.
  // Fail CLOSED: if the signing secret isn't configured we refuse rather than
  // silently trusting the caller.
  if (!env.STRIPE_WEBHOOK_SECRET) {
    console.error('stripe webhook rejected: STRIPE_WEBHOOK_SECRET not configured');
    return new Response('Webhook not configured', { status: 503 });
  }
  const sig = request.headers.get('Stripe-Signature');
  if (!(await stripeSignatureValid(body, sig, env.STRIPE_WEBHOOK_SECRET))) {
    return new Response('Invalid signature', { status: 400 });
  }

  let event;
  try {
    event = JSON.parse(body);
  } catch {
    return new Response('Invalid JSON', { status: 400 });
  }

  if (event.type === 'checkout.session.completed') {
    const session = event.data.object;
    const amount = (session.amount_total || 0) / 100; // dollars
    const userId = session.metadata?.user_id || session.client_reference_id;

    const isTicketPurchase = session.metadata?.type === 'ticket_purchase';

    if (isTicketPurchase) {
      // TICKET PURCHASE — credit tickets + split revenue
      const pkgKey = session.metadata?.package;
      const pkg = TICKET_PACKAGES[pkgKey];
      const ticketsToCredit = pkg ? pkg.total : parseInt(session.metadata?.tickets || '0');

      // Idempotency check
      const existing = await env.PAYOUTS.get(`deposit:${session.id}`);
      if (existing) return new Response('OK', { status: 200 }); // Already processed

      // Credit tickets to user
      if (userId) {
        const userData = await env.USERS.get(`user:${userId}`);
        if (userData) {
          const user = JSON.parse(userData);
          user.ticket_balance = (user.ticket_balance || 0) + ticketsToCredit;
          await env.USERS.put(`user:${userId}`, JSON.stringify(user));
        }
      }

      // Revenue split: 50% treasury, 50% platform
      // Storefront/processor fee comes off the top, THEN the 50/50. See
      // TREASURY_SPLIT: splitting gross would have the platform paying the
      // app store out of its own half once iOS/Android are live.
      const channel = session.metadata?.channel || 'web';
      const fee = channelFee(amount, channel);
      const net = Math.max(0, amount - fee);
      const treasuryCut = net * TREASURY_SPLIT;
      const platformCut = net - treasuryCut;
      // Track fees so the accounting dashboard can show true take-rate.
      const feesPrev = parseFloat(await env.PAYOUTS.get('costs:storefront_fees') || '0');
      await env.PAYOUTS.put('costs:storefront_fees', String(feesPrev + fee));

      const currentTreasury = parseFloat(await env.PAYOUTS.get('treasury:total_usd') || '0');
      const newTreasuryTotal = currentTreasury + treasuryCut;
      await env.PAYOUTS.put('treasury:total_usd', newTreasuryTotal.toString());
      // Deposits raise the high-water mark (scarcity self-heal —
      // bliss-core Treasury::deposit).
      const hwmT = parseFloat(await env.PAYOUTS.get('treasury:hwm') || '0');
      if (newTreasuryTotal > hwmT) await env.PAYOUTS.put('treasury:hwm', newTreasuryTotal.toString());

      const currentPlatform = parseFloat(await env.PAYOUTS.get('platform:total_usd') || '0');
      await env.PAYOUTS.put('platform:total_usd', (currentPlatform + platformCut).toString());

      // Log transaction
      if (userId) {
        await env.INVENTORY.put(`txn:${userId}:${Date.now()}`, JSON.stringify({
          id: session.id, user_id: userId, type: 'purchase', amount: ticketsToCredit,
          currency: 'TKT', stripe_session_id: session.id, price_usd: amount,
          description: `Purchased ${pkg?.name || 'Tickets'} package (${ticketsToCredit} TKT)`,
          timestamp: new Date().toISOString(),
        }), { expirationTtl: 86400 * 365 * 3 });
      }

      // Record deposit with full revenue breakdown
      await env.PAYOUTS.put(`deposit:${session.id}`, JSON.stringify({
        id: session.id, type: 'ticket_purchase', amount_usd: amount,
        channel, storefront_fee: fee, net_usd: net,
        treasury_cut: treasuryCut, platform_cut: platformCut,
        tickets_credited: ticketsToCredit, package: pkgKey,
        user_id: userId || 'anonymous', timestamp: new Date().toISOString(),
      }));

    } else {
      // TREASURY FUNDING — direct treasury deposit (existing flow).
      // Stripe delivers at-least-once and retries on any non-2xx or timeout,
      // so without this guard a redelivered $500 session credited $1000.
      const existingFund = await env.PAYOUTS.get(`deposit:${session.id}`);
      if (existingFund) return new Response('OK', { status: 200 });

      await env.PAYOUTS.put(`deposit:${session.id}`, JSON.stringify({
        id: session.id, type: 'treasury_fund', amount_usd: amount,
        user_id: userId || 'anonymous', timestamp: new Date().toISOString(),
        mode: session.mode, stripe_payment_intent: session.payment_intent,
      }));

      const currentTotal = parseFloat(await env.PAYOUTS.get('treasury:total_usd') || '0');
      const newTotal = currentTotal + amount;
      await env.PAYOUTS.put('treasury:total_usd', newTotal.toString());
      // Deposits raise the high-water mark (scarcity self-heal).
      const hwmF = parseFloat(await env.PAYOUTS.get('treasury:hwm') || '0');
      if (newTotal > hwmF) await env.PAYOUTS.put('treasury:hwm', newTotal.toString());

      const count = parseInt(await env.PAYOUTS.get('treasury:deposit_count') || '0');
      await env.PAYOUTS.put('treasury:deposit_count', (count + 1).toString());
    }
  }

  return new Response('OK', { status: 200 });
}

// Stripe Connect — onboard a contributor to receive payouts
async function handleStripeConnectOnboard(request, env, cors) {
  if (!env.STRIPE_SECRET_KEY) return json({ error: 'Stripe not configured' }, 503, cors);

  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User not found' }, 404, cors);
  const user = JSON.parse(userData);

  // Money gate: verified identity AND a document-confirmed age at or above the
  // jurisdiction minimum. This is the point where a person becomes able to
  // receive real USD, so it is the point that must not admit a minor.
  const gate = await requireVerifiedAdult(userId, user, env);
  if (!gate.ok)
    return json({ error: gate.error, reason: gate.reason, code: gate.code }, gate.status, cors);

  // Check if user already has a Connect account
  let connectId = user.stripe_connect_id;

  if (!connectId) {
    // Country comes from the verified KYC record loaded by the gate above.
    const kycCountry = gate.kyc.iso2 || 'US';

    // Create Connect Custom account with pre-filled identity info
    const clientIp = request.headers.get('cf-connecting-ip') || '0.0.0.0';
    const accountParams = {
      'type': 'custom',
      'country': kycCountry,
      'business_type': 'individual',
      'metadata[user_id]': userId,
      'metadata[username]': user.username,
      'capabilities[card_payments][requested]': 'true',
      'capabilities[transfers][requested]': 'true',
      // ToS acceptance (required for custom accounts)
      'tos_acceptance[date]': Math.floor(Date.now() / 1000).toString(),
      'tos_acceptance[ip]': clientIp,
    };

    if (user.email) accountParams['email'] = user.email;
    // Don't set first_name to username — Stripe will reject if it doesn't match the ID photo
    // Name comes from the ID document uploaded via R2 sync or Stripe's own onboarding
    // Prefer the date of birth READ FROM THE DOCUMENT over the one the user
    // typed. Stripe rejects a Connect account whose DOB contradicts the ID
    // photo, and the document value is the one that passed the age gate.
    const verifiedDob = gate.kyc.extracted_dob || user.birthday;
    if (verifiedDob && /^\d{4}-\d{2}-\d{2}$/.test(verifiedDob)) {
      const [y, m, d] = verifiedDob.split('-');
      accountParams['individual[dob][year]'] = y;
      accountParams['individual[dob][month]'] = parseInt(m, 10).toString();
      accountParams['individual[dob][day]'] = parseInt(d, 10).toString();
    }

    const account = await stripeRequest('POST', '/accounts', accountParams, env);
    if (account.error) return json({ error: account.error.message, type: account.error.type, code: account.error.code, param: account.error.param }, 400, cors);

    connectId = account.id;
    user.stripe_connect_id = connectId;
    await env.USERS.put(`user:${userId}`, JSON.stringify(user));

    // Upload KYC docs from R2 to Stripe for identity verification
    await syncKycDocsToStripe(userId, connectId, env);
  } else {
    // Account exists — re-sync docs in case they weren't uploaded before
    await syncKycDocsToStripe(userId, connectId, env);
  }

  // Create onboarding link — collects remaining info (bank details, SSN for 1099)
  const link = await stripeRequest('POST', '/account_links', {
    'account': connectId,
    'refresh_url': 'https://eustress.dev/bliss?connect=refresh',
    'return_url': 'https://eustress.dev/bliss?connect=complete',
    'type': 'account_onboarding',
    'collect': 'eventually_due',
  }, env);

  if (link.error) return json({ error: link.error.message, type: link.error.type, code: link.error.code }, 400, cors);

  return json({ url: link.url, connect_id: connectId }, 200, cors);
}

// Check Stripe Connect status for a user
async function handleStripeConnectStatus(request, env, cors) {
  if (!env.STRIPE_SECRET_KEY) return json({ error: 'Stripe not configured' }, 503, cors);

  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User not found' }, 404, cors);
  const user = JSON.parse(userData);

  if (!user.stripe_connect_id) {
    return json({ connected: false, message: 'No payout account linked' }, 200, cors);
  }

  const account = await stripeRequest('GET', `/accounts/${user.stripe_connect_id}`, null, env);

  return json({
    connected: true,
    connect_id: user.stripe_connect_id,
    payouts_enabled: account.payouts_enabled || false,
    details_submitted: account.details_submitted || false,
  }, 200, cors);
}

// Sync KYC documents from R2 to Stripe Connect for identity verification
async function syncKycDocsToStripe(userId, connectId, env) {
  try {
    for (const side of ['front', 'back']) {
      // Get the KYC record to find the R2 key
      const kycData = await env.KYC_STATUS.get(`kyc-${userId}-${side}`);
      if (!kycData) continue;

      const kyc = JSON.parse(kycData);
      if (!kyc.r2_key) continue;

      // Fetch document from R2
      const r2Object = await env.KYC_BUCKET.get(kyc.r2_key);
      if (!r2Object) continue;

      const fileBytes = await r2Object.arrayBuffer();
      const contentType = r2Object.httpMetadata?.contentType || 'image/jpeg';

      // Upload to Stripe Files API (multipart form)
      const formData = new FormData();
      const ext = contentType.includes('pdf') ? 'pdf' : contentType.includes('png') ? 'png' : 'jpg';
      formData.append('file', new Blob([fileBytes], { type: contentType }), `id_${side}.${ext}`);
      formData.append('purpose', 'identity_document');

      const fileResp = await fetch('https://files.stripe.com/v1/files', {
        method: 'POST',
        headers: { 'Authorization': `Bearer ${env.STRIPE_SECRET_KEY}` },
        body: formData,
      });

      const fileResult = await fileResp.json();
      if (fileResult.error || !fileResult.id) continue;

      // Attach to the Connect account's identity verification
      const docSide = side === 'front' ? 'front' : 'back';
      await stripeRequest('POST', `/accounts/${connectId}`, {
        [`individual[verification][document][${docSide}]`]: fileResult.id,
      }, env);
    }
  } catch (e) {
    // Non-fatal — Stripe onboarding will still work, user just re-uploads manually
    console.error('KYC sync to Stripe failed:', e.message);
  }
}

// ═══════════════════════════════════════════════════════════════════════════
// DAILY PAYOUTS — BLS → USD conversion + Stripe Connect transfers
// ═══════════════════════════════════════════════════════════════════════════

// Trigger the daily cycle manually (admin escape hatch). Runs the SAME
// code path as the UTC-midnight cron: BLS emission distribution, then
// USD treasury drip. Both are idempotent per score-date, so re-running
// after a partial failure is safe.
async function handleDailyPayout(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const distribution = await runDailyDistribution(env);
  const payout = env.STRIPE_SECRET_KEY ? await runDailyPayout(env) : null;

  await auditLog(env, 'DAILY_PAYOUT', adminId, 'treasury', {
    distribution_ran: !!distribution,
    payout_ran: !!payout,
    minted: distribution?.minted || 0,
    total_paid_usd: payout?.total_paid_usd || 0,
  });

  return json({
    distribution: distribution || { message: 'Already distributed for this date (or no scores)' },
    payout: payout || { message: 'Skipped (empty treasury, drip below $0.50, no eligible contributors, or already ran)' },
  }, 200, cors);
}

// Get payout history
async function handlePayoutHistory(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  // Get recent payouts for this user
  const history = [];
  const list = await env.PAYOUTS.list({ prefix: 'payout:', limit: 30 });

  for (const key of list.keys) {
    const data = await env.PAYOUTS.get(key.name);
    if (!data) continue;
    const record = JSON.parse(data);
    const userPayout = record.payouts?.find(p => p.user_id === userId);
    if (userPayout) {
      history.push({
        date: record.date,
        amount_usd: userPayout.amount_usd,
        transfer_id: userPayout.transfer_id,
      });
    }
  }

  return json({ history, total_received: history.reduce((sum, p) => sum + p.amount_usd, 0) }, 200, cors);
}

// Get current BLS → USD exchange rate + live emission/supply stats
async function handlePayoutRate(env, cors) {
  const treasuryUsd = parseFloat(await env.PAYOUTS.get('treasury:total_usd') || '0');
  const hwm = parseFloat(await env.PAYOUTS.get('treasury:hwm') || '0');
  const scarce = treasuryUsd > 0 && treasuryUsd <= hwm * TREASURY_SCARCITY_RATIO;
  const dripRate = scarce ? TREASURY_SCARCITY_RATE : TREASURY_DRIP_RATE;
  const dailyDripUsd = treasuryUsd * dripRate;

  // Live emission from the ledger (falls back to genesis defaults
  // before the first distribution has run). Minor units are authoritative;
  // the legacy float key is only a pre-migration fallback.
  const supplyMinorRaw = parseInt(await env.PAYOUTS.get('bliss:supply_minor') || '0', 10);
  const distributedMinorRaw = parseInt(await env.PAYOUTS.get('bliss:distributed_minor') || '0', 10);
  const supply = supplyMinorRaw
    ? fromMinor(supplyMinorRaw)
    : parseFloat(await env.PAYOUTS.get('bliss:current_supply') || String(BLISS_INITIAL_SUPPLY));
  const genesis = await env.PAYOUTS.get('bliss:genesis_date');
  const years = genesis
    ? Math.max(0, Math.floor((Date.now() - Date.parse(genesis)) / (365 * 86400 * 1000)))
    : 0;
  const annualRate = blissEmissionRate(years);
  const dailyBls = supply * annualRate / 365;
  const blsToUsd = dailyBls > 0 ? dailyDripUsd / dailyBls : 0;

  const platformUsd = parseFloat(await env.PAYOUTS.get('platform:total_usd') || '0');

  return json({
    treasury_usd: treasuryUsd,
    platform_usd: platformUsd,
    daily_drip_usd: dailyDripUsd,
    scarcity_active: scarce,
    daily_bls_emission: dailyBls,
    annual_emission_rate: annualRate,
    current_supply: supply,
    total_distributed: distributedMinorRaw
      ? fromMinor(distributedMinorRaw)
      : parseFloat(await env.PAYOUTS.get('bliss:total_distributed') || '0'),
    genesis_date: genesis || null,
    bls_to_usd_rate: blsToUsd,
    rate_display: blsToUsd > 0 ? `$${blsToUsd.toFixed(6)}/BLS` : 'No treasury funds',
    deposit_count: parseInt(await env.PAYOUTS.get('treasury:deposit_count') || '0'),
  }, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// TICKETS — Purchasable currency for marketplace + simulation API
// ═══════════════════════════════════════════════════════════════════════════

const TICKET_PACKAGES = {
  starter:  { name: 'Starter',  usd: 4.99,  base: 400,  bonus: 0,    total: 400,   price_id: 'price_1THyBSRgsC7hEeKMVPoXgzGp' },
  standard: { name: 'Standard', usd: 9.99,  base: 800,  bonus: 80,   total: 880,   price_id: 'price_1THyBTRgsC7hEeKMqirqajPl' },
  mega:     { name: 'Mega',     usd: 19.99, base: 1600, bonus: 240,  total: 1840,  price_id: 'price_1THyBURgsC7hEeKMXkupuaQR' },
  super:    { name: 'Super',    usd: 49.99, base: 4000, bonus: 1000, total: 5000,  price_id: 'price_1THyBURgsC7hEeKMzwTi9Z2V' },
  ultra:    { name: 'Ultra',    usd: 99.99, base: 8000, bonus: 2800, total: 10800, price_id: 'price_1THyBVRgsC7hEeKMHTMs0Hsq' },
};

const DEVELOPER_SHARE = 0.70;
const PLATFORM_SHARE = 0.30;
/// Contributor share of NET revenue (after the storefront's cut).
///
/// This is deliberately applied to NET, not gross. Taking 50% of gross works
/// only while Stripe-web is the sole rail; on iOS/Android the storefront takes
/// ~30% first, so a gross split would leave the platform 20% while
/// contributors took 50% — the platform would be funding the store out of its
/// own margin. On net, contributors get 50% of what actually arrives, which
/// is still ~35% of gross on mobile: comfortably above the ~24.5% a Roblox
/// creator nets, without making the platform side unsustainable.
const TREASURY_SPLIT = 0.50;

/// Storefront / processor fee by sales channel, deducted before the split.
/// `channel` rides in the Stripe session metadata; unknown channels fall back
/// to web pricing rather than silently assuming zero fees.
const CHANNEL_FEES = {
  web:     { rate: 0.029, flat: 0.30 },  // Stripe standard
  ios:     { rate: 0.30,  flat: 0.0  },  // App Store
  android: { rate: 0.30,  flat: 0.0  },  // Play Store
  steam:   { rate: 0.30,  flat: 0.0  },
};

/// Fee for a gross amount on a channel. Never returns more than the amount.
function channelFee(amountUsd, channel) {
  const f = CHANNEL_FEES[channel] || CHANNEL_FEES.web;
  return Math.min(amountUsd, amountUsd * f.rate + f.flat);
}

function handleTicketPackages(env, cors) {
  const packages = Object.entries(TICKET_PACKAGES).map(([key, pkg]) => ({
    id: key, name: pkg.name, usd: pkg.usd, base: pkg.base, bonus: pkg.bonus, total: pkg.total,
  }));
  return json({ packages }, 200, cors);
}

async function handleTicketBalance(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User not found' }, 404, cors);
  const user = JSON.parse(userData);

  return json({ tickets: user.ticket_balance || 0, user_id: userId }, 200, cors);
}

async function handleTicketCheckout(request, env, cors) {
  if (!env.STRIPE_SECRET_KEY) return json({ error: 'Stripe not configured' }, 503, cors);

  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Sign in to purchase tickets' }, 401, cors);

  const { package: pkgKey } = await request.json();
  const pkg = TICKET_PACKAGES[pkgKey];
  if (!pkg) return json({ error: 'Invalid package' }, 400, cors);

  const params = {
    'mode': 'payment',
    'success_url': 'https://eustress.dev/tickets?purchased=true',
    'cancel_url': 'https://eustress.dev/tickets?purchased=false',
    'line_items[0][price]': pkg.price_id,
    'line_items[0][quantity]': '1',
    'metadata[type]': 'ticket_purchase',
    'metadata[package]': pkgKey,
    'metadata[tickets]': pkg.total.toString(),
    'metadata[user_id]': userId,
    'client_reference_id': userId,
  };

  const session = await stripeRequest('POST', '/checkout/sessions', params, env);
  if (session.error) return json({ error: session.error.message }, 400, cors);

  return json({ url: session.url, session_id: session.id }, 200, cors);
}

async function handleTicketSpend(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const { product_id, price, developer_id } = await request.json();
  if (!product_id || !price || price <= 0)
    return json({ error: 'Invalid product or price' }, 400, cors);

  // Get buyer
  const buyerData = await env.USERS.get(`user:${userId}`);
  if (!buyerData) return json({ error: 'Buyer not found' }, 404, cors);
  const buyer = JSON.parse(buyerData);

  const balance = buyer.ticket_balance || 0;
  if (balance < price)
    return json({ error: 'Insufficient tickets', balance, price }, 400, cors);

  // Calculate split
  const devCut = Math.floor(price * DEVELOPER_SHARE);
  const platformCut = price - devCut;

  // Deduct from buyer
  buyer.ticket_balance = balance - price;
  await env.USERS.put(`user:${userId}`, JSON.stringify(buyer));

  // Credit developer (if provided)
  if (developer_id) {
    const devData = await env.USERS.get(`user:${developer_id}`);
    if (devData) {
      const dev = JSON.parse(devData);
      dev.ticket_balance = (dev.ticket_balance || 0) + devCut;
      await env.USERS.put(`user:${developer_id}`, JSON.stringify(dev));
    }
  }

  // Log transactions
  const txnId = crypto.randomUUID();
  const now = new Date().toISOString();

  await env.INVENTORY.put(`txn:${userId}:${Date.now()}`, JSON.stringify({
    id: txnId, user_id: userId, type: 'spend', amount: -price,
    balance_after: buyer.ticket_balance, currency: 'TKT',
    product_id, developer_id, description: `Purchased product ${product_id}`, timestamp: now,
  }), { expirationTtl: 86400 * 365 * 3 });

  // A sale is VERIFIED value: credit the creator BLS contribution score so
  // the daily distribution pays them for impact, not just for hours logged.
  if (developer_id && devCut > 0) {
    const vDay = new Date().toISOString().split('T')[0];
    const vKey = `contrib:${vDay}:${developer_id}`;
    const vRaw = await env.INVENTORY.get(vKey);
    const vRec = vRaw ? JSON.parse(vRaw)
      : { total_score: 0, by_type: {}, by_seconds: {}, count: 0 };
    const vScore = devCut * VALUE_SCORE_PER_TICKET;
    vRec.value_score = (vRec.value_score || 0) + vScore;
    vRec.by_type.Value = (vRec.by_type.Value || 0) + vScore;
    vRec.updated_at = new Date().toISOString();
    await env.INVENTORY.put(vKey, JSON.stringify(vRec), { expirationTtl: 86400 * 90 });
    const dtKey = `daytotal:${vDay}`;
    const dtPrev = parseFloat(await env.INVENTORY.get(dtKey) || '0');
    await env.INVENTORY.put(dtKey, String(dtPrev + vScore), { expirationTtl: 86400 * 7 });
  }

  if (developer_id) {
    await env.INVENTORY.put(`txn:${developer_id}:${Date.now()}`, JSON.stringify({
      id: crypto.randomUUID(), user_id: developer_id, type: 'dev_payout', amount: devCut,
      currency: 'TKT', product_id, counterparty_id: userId,
      description: `Sale: product ${product_id} (70% of ${price} TKT)`, timestamp: now,
    }), { expirationTtl: 86400 * 365 * 3 });
  }

  return json({
    success: true, price, developer_cut: devCut, platform_cut: platformCut,
    buyer_balance: buyer.ticket_balance,
  }, 200, cors);
}

async function handleTicketHistory(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const list = await env.INVENTORY.list({ prefix: `txn:${userId}:`, limit: 50 });
  const history = [];

  for (const key of list.keys) {
    const data = await env.INVENTORY.get(key.name);
    if (data) history.push(JSON.parse(data));
  }

  history.sort((a, b) => new Date(b.timestamp) - new Date(a.timestamp));
  return json({ history, count: history.length }, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// NODE HEARTBEAT — Live stats for nodes and game servers
// ═══════════════════════════════════════════════════════════════════════════

async function handleNodeHeartbeat(request, env, cors) {
  const body = await request.json();
  const { node_id, mode, players, uptime_secs, fork_id } = body;

  if (!node_id) return json({ error: 'node_id required' }, 400, cors);

  // IDENTITY — BEARER TOKEN ONLY. Do not reintroduce a body-supplied id.
  //
  // This endpoint writes `presence:{date}:{id}` and `node-mode:{id}`, which
  // handleCosign trusts as its integrity anchors (the ActiveTime ceiling and
  // the Full-node bonus). Accepting an id from the request body made those
  // anchors writable by anyone: a public key is a PUBLIC identifier, so an
  // unauthenticated curl loop could grant itself the Full-node bonus and
  // presence headroom, or pin a competitor's mode to Light to strip theirs.
  //
  // The id therefore comes from the JWT and nowhere else. Anonymous beats are
  // still accepted for node telemetry (the `node:` key below), but they carry
  // no user-scoped effects.
  const user_id = await verifyAuth(request, env);

  await env.SOCIAL.put(`node:${node_id}`, JSON.stringify({
    node_id, mode: mode || 'light', players: players || 0,
    uptime_secs: uptime_secs || 0, fork_id: fork_id || 'eustress.dev',
    last_heartbeat: new Date().toISOString(),
  }), { expirationTtl: 120 }); // Expires in 2 min if no heartbeat

  // Contribution-integrity signals (consumed by handleCosign):
  //   • presence — wall-clock-bounded online seconds today. Driven by
  //     real heartbeat arrival spacing, NOT the self-reported uptime_secs,
  //     so it can't be inflated: a flood credits tiny deltas, a gap is
  //     capped at PRESENCE_MAX_STEP. This is the ceiling on ActiveTime.
  //   • node-mode — the mode we actually observed, so the cosign
  //     Full-node bonus is server-derived, not client-claimed.
  if (user_id) {
    const nowMs = Date.now();
    const dayStr = new Date().toISOString().split('T')[0];
    const presKey = `presence:${dayStr}:${user_id}`;
    const presRaw = await env.SOCIAL.get(presKey);
    const pres = presRaw ? JSON.parse(presRaw) : { seconds: 0, last_ms: nowMs };
    const deltaSecs = Math.max(0, (nowMs - (pres.last_ms || nowMs)) / 1000);
    pres.seconds = (pres.seconds || 0) + Math.min(deltaSecs, PRESENCE_MAX_STEP);
    pres.last_ms = nowMs;
    await env.SOCIAL.put(presKey, JSON.stringify(pres), { expirationTtl: 86400 * 2 });

    const normMode = (mode === 'Full' || mode === 'full') ? 'Full' : 'Light';
    await env.SOCIAL.put(`node-mode:${user_id}`, normMode, { expirationTtl: 86400 * 2 });
  }

  // Return current BLS balance if authenticated (engine polls this)
  let bliss_balance = 0;
  let pending_score = 0;
  let projected_bls = 0;
  if (user_id) {
    bliss_balance = fromMinor(await ledgerBalanceMinor(env, user_id));

    // SESSION HOURS — written to dedicated keys, NEVER back into `user:`.
    //
    // This handler used to read the whole user record, mutate two fields, and
    // PUT the entire object back every ~90s. Workers KV has no compare-and-set
    // and serves cached reads, so that PUT carried a stale snapshot and
    // last-write-wins would silently erase BLS credited by the midnight
    // distribution cron — and could resurrect a just-banned account. A
    // high-frequency endpoint must never rewrite the account record.
    const now = new Date();
    const today = now.toISOString().split('T')[0];
    // Clamp: uptime is self-reported and was previously unbounded, so a single
    // request could bank ~10^8 hours and pin the public leaderboard forever.
    // One day is the most a single session can legitimately contribute.
    const uptimeSecsSafe = Math.max(0, Math.min(Number(uptime_secs) || 0, 86400));
    const heartbeatHours = uptimeSecsSafe / 3600;

    const hourKey = `hours:${user_id}:${node_id}`;
    const prevData = await env.SOCIAL.get(hourKey);
    const prev = prevData ? JSON.parse(prevData) : { date: '', hours: 0 };

    if (prev.date !== today) {
      // New day — bank the previous session into the durable totals.
      if (prev.hours > 0) {
        const dKey = `hours_daily:${user_id}:${prev.date}`;
        const existing = parseFloat(await env.SOCIAL.get(dKey) || '0');
        await env.SOCIAL.put(dKey, String(existing + prev.hours), { expirationTtl: 86400 * 90 });
        const tKey = `hours_total:${user_id}`;
        const tPrev = parseFloat(await env.SOCIAL.get(tKey) || '0');
        await env.SOCIAL.put(tKey, String(tPrev + prev.hours));
      }
      await env.SOCIAL.put(hourKey, JSON.stringify({ date: today, hours: heartbeatHours }), { expirationTtl: 86400 * 2 });
    } else if (heartbeatHours > prev.hours) {
      await env.SOCIAL.put(hourKey, JSON.stringify({ date: today, hours: heartbeatHours }), { expirationTtl: 86400 * 2 });
    }
    await env.SOCIAL.put(`last-active:${user_id}`, now.toISOString(), { expirationTtl: 86400 * 30 });
    // Check today's pending contributions (written by handleCosign;
    // date-first key so the distribution cron can prefix-scan a day)
    const pendingKey = `contrib:${today}:${user_id}`;
    const pendingData = await env.INVENTORY.get(pendingKey);
    if (pendingData) {
      const pending = JSON.parse(pendingData);
      pending_score = pending.total_score || 0;
    }

    // Projected BLS for today, so the engine can show earnings GROWING as
    // work happens instead of only a points number that means nothing to a
    // person. Same formula the midnight distribution uses:
    //   emission x min(1, dayTotal/FULL_DAY_SCORE) x (myScore / dayTotal)
    // `daytotal:` is a cheap running counter maintained by handleCosign; the
    // real distribution recomputes from the contrib records, so a small drift
    // here only affects the estimate, never the payout.
    const dayTotal = Math.max(
      pending_score,
      parseFloat(await env.INVENTORY.get(`daytotal:${today}`) || '0')
    );
    if (dayTotal > 0 && pending_score > 0) {
      const supplyMinorNow = parseInt(await env.PAYOUTS.get('bliss:supply_minor') || '0', 10)
        || toMinor(parseFloat(await env.PAYOUTS.get('bliss:current_supply') || String(BLISS_INITIAL_SUPPLY)));
      const genesisDate = await env.PAYOUTS.get('bliss:genesis_date');
      const yrs = genesisDate
        ? Math.max(0, Math.floor((Date.now() - Date.parse(genesisDate)) / (365 * 86400 * 1000)))
        : 0;
      const emissionToday = fromMinor(supplyMinorNow) * blissEmissionRate(yrs) / 365;
      const util = Math.min(1, dayTotal / FULL_DAY_SCORE);
      projected_bls = Math.floor(emissionToday * BLISS_UNIT * util * (pending_score / dayTotal)) / BLISS_UNIT;
    }
  }

  return json({ ok: true, bliss_balance, pending_score, projected_bls,
    full_day_score: FULL_DAY_SCORE }, 200, cors);
}

async function handleNodeStats(env, cors) {
  const list = await env.SOCIAL.list({ prefix: 'node:', limit: 1000 });
  let totalNodes = 0, totalPlayers = 0, lightNodes = 0, fullNodes = 0;

  for (const key of list.keys) {
    const data = await env.SOCIAL.get(key.name);
    if (data) {
      const node = JSON.parse(data);
      totalNodes++;
      totalPlayers += node.players || 0;
      if (node.mode === 'full') fullNodes++;
      else lightNodes++;
    }
  }

  return json({
    active_nodes: totalNodes, light_nodes: lightNodes, full_nodes: fullNodes,
    online_players: totalPlayers, timestamp: new Date().toISOString(),
  }, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// SIMULATIONS — Published spaces (gallery data)
// ═══════════════════════════════════════════════════════════════════════════

async function handlePublishSimulation(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const body = await request.json();
  const { name, description, genre, max_players, thumbnail_url, r2_key } = body;

  if (!name) return json({ error: 'name required' }, 400, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  const user = userData ? JSON.parse(userData) : {};

  const simId = crypto.randomUUID();
  const sim = {
    id: simId, name, description: description || '',
    genre: genre || 'all', max_players: max_players || 10,
    author_id: userId, author_name: user.username || 'Unknown',
    // Explicit: handleDownloadPak gates on `!sim.is_public`, and the listing
    // endpoint separately defaults undefined to public. Leaving it unset made
    // every published simulation publicly listed and privately denied.
    is_public: true,
    thumbnail_url: thumbnail_url || null, r2_key: r2_key || null,
    play_count: 0, favorite_count: 0, version: 1,
    published_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
  };

  await env.SOCIAL.put(`sim:${simId}`, JSON.stringify(sim));
  await env.SOCIAL.put(`sim-author:${userId}:${simId}`, simId);

  // Increment author's sim count
  const simCount = parseInt(await env.SOCIAL.get(`simCount:${userId}`) || '0') + 1;
  await env.SOCIAL.put(`simCount:${userId}`, simCount.toString());

  return json({ id: simId, ...sim }, 201, cors);
}

// Upload .pak scene file to R2
async function handleUploadScene(request, simId, env, cors) {
  const auth = await verifyAuth(request, env);
  if (!auth) return json({ error: 'Unauthorized' }, 401, cors);

  // Verify this simulation exists and belongs to the user
  const simData = await env.SOCIAL.get(`sim:${simId}`);
  if (!simData) return json({ error: 'Simulation not found' }, 404, cors);
  const sim = JSON.parse(simData);
  if (sim.author_id !== auth) return json({ error: 'Not your simulation' }, 403, cors);

  // Read the binary body (the .pak file)
  const body = await request.arrayBuffer();
  if (!body || body.byteLength === 0) return json({ error: 'Empty body' }, 400, cors);

  // Max 500MB per .pak
  if (body.byteLength > 500 * 1024 * 1024)
    return json({ error: 'Scene file too large (max 500MB)' }, 413, cors);

  const r2Key = `universes/${simId}/universe.pak`;
  await env.SCENES.put(r2Key, body, {
    httpMetadata: { contentType: 'application/octet-stream' },
    customMetadata: { simId, authorId: auth, uploadedAt: new Date().toISOString() },
  });

  // Update simulation record with R2 key and file size
  sim.r2_key = r2Key;
  sim.scene_size_bytes = body.byteLength;
  sim.updated_at = new Date().toISOString();
  await env.SOCIAL.put(`sim:${simId}`, JSON.stringify(sim));

  return json({ r2_key: r2Key, size_bytes: body.byteLength }, 200, cors);
}

// Multipart upload: Create — initiates an R2 multipart upload for large .pak files
async function handleMultipartCreate(request, simId, env, cors) {
  const auth = await verifyAuth(request, env);
  if (!auth) return json({ error: 'Unauthorized' }, 401, cors);

  const simData = await env.SOCIAL.get(`sim:${simId}`);
  if (!simData) return json({ error: 'Simulation not found' }, 404, cors);
  const sim = JSON.parse(simData);
  if (sim.author_id !== auth) return json({ error: 'Not your simulation' }, 403, cors);

  const r2Key = `universes/${simId}/universe.pak`;
  const multipart = await env.SCENES.createMultipartUpload(r2Key, {
    httpMetadata: { contentType: 'application/octet-stream' },
    customMetadata: { simId, authorId: auth, uploadedAt: new Date().toISOString() },
  });

  return json({ upload_id: multipart.uploadId, r2_key: r2Key }, 200, cors);
}

// Multipart upload: Part — uploads a single chunk (100MB max per part)
async function handleMultipartPart(request, simId, env, cors) {
  const auth = await verifyAuth(request, env);
  if (!auth) return json({ error: 'Unauthorized' }, 401, cors);

  const url = new URL(request.url);
  const uploadId = url.searchParams.get('upload_id');
  const partNumber = parseInt(url.searchParams.get('part_number') || '1');

  if (!uploadId) return json({ error: 'Missing upload_id' }, 400, cors);

  const r2Key = `universes/${simId}/universe.pak`;
  const multipart = env.SCENES.resumeMultipartUpload(r2Key, uploadId);

  const body = await request.arrayBuffer();
  const part = await multipart.uploadPart(partNumber, body);

  return json({ part_number: partNumber, etag: part.etag, size: body.byteLength }, 200, cors);
}

// Multipart upload: Complete — assembles all parts into the final object
async function handleMultipartComplete(request, simId, env, cors) {
  const auth = await verifyAuth(request, env);
  if (!auth) return json({ error: 'Unauthorized' }, 401, cors);

  const { upload_id, parts, total_size } = await request.json();
  if (!upload_id || !parts) return json({ error: 'Missing upload_id or parts' }, 400, cors);

  const r2Key = `universes/${simId}/universe.pak`;
  const multipart = env.SCENES.resumeMultipartUpload(r2Key, upload_id);

  // parts = [{ part_number, etag }, ...]
  const uploadedParts = parts.map(p => ({
    partNumber: p.part_number,
    etag: p.etag,
  }));

  await multipart.complete(uploadedParts);

  // Update simulation record
  const simData = await env.SOCIAL.get(`sim:${simId}`);
  if (simData) {
    const sim = JSON.parse(simData);
    sim.r2_key = r2Key;
    sim.scene_size_bytes = total_size || 0;
    sim.updated_at = new Date().toISOString();
    await env.SOCIAL.put(`sim:${simId}`, JSON.stringify(sim));
  }

  return json({ r2_key: r2Key, complete: true }, 200, cors);
}

// Upload a single Space .pak to R2 (incremental update, like git push for one directory)
async function handleUploadSingleSpace(request, simId, spaceName, env, cors) {
  const auth = await verifyAuth(request, env);
  if (!auth) return json({ error: 'Unauthorized' }, 401, cors);

  const simData = await env.SOCIAL.get(`sim:${simId}`);
  if (!simData) return json({ error: 'Simulation not found' }, 404, cors);
  const sim = JSON.parse(simData);
  if (sim.author_id !== auth) return json({ error: 'Not your simulation' }, 403, cors);

  const body = await request.arrayBuffer();
  if (!body || body.byteLength === 0) return json({ error: 'Empty body' }, 400, cors);
  if (body.byteLength > 500 * 1024 * 1024)
    return json({ error: 'Space file too large (max 500MB)' }, 413, cors);

  const decodedName = decodeURIComponent(spaceName);
  const r2Key = `universes/${simId}/spaces/${decodedName}.pak`;
  await env.SCENES.put(r2Key, body, {
    httpMetadata: { contentType: 'application/octet-stream' },
    customMetadata: { simId, spaceName: decodedName, authorId: auth, uploadedAt: new Date().toISOString() },
  });

  // Track individual space uploads in the simulation record
  if (!sim.spaces) sim.spaces = {};
  sim.spaces[decodedName] = { r2_key: r2Key, size_bytes: body.byteLength, updated_at: new Date().toISOString() };
  sim.updated_at = new Date().toISOString();
  await env.SOCIAL.put(`sim:${simId}`, JSON.stringify(sim));

  return json({ r2_key: r2Key, space: decodedName, size_bytes: body.byteLength }, 200, cors);
}

// Upload thumbnail image to R2
async function handleUploadThumbnail(request, simId, env, cors) {
  const auth = await verifyAuth(request, env);
  if (!auth) return json({ error: 'Unauthorized' }, 401, cors);

  const simData = await env.SOCIAL.get(`sim:${simId}`);
  if (!simData) return json({ error: 'Simulation not found' }, 404, cors);
  const sim = JSON.parse(simData);
  if (sim.author_id !== auth) return json({ error: 'Not your simulation' }, 403, cors);

  const body = await request.arrayBuffer();
  if (!body || body.byteLength === 0) return json({ error: 'Empty body' }, 400, cors);

  // Max 5MB for thumbnail
  if (body.byteLength > 5 * 1024 * 1024)
    return json({ error: 'Thumbnail too large (max 5MB)' }, 413, cors);

  const contentType = request.headers.get('content-type') || 'image/webp';
  const ext = contentType.includes('png') ? 'png' : contentType.includes('jpeg') ? 'jpg' : 'webp';
  const r2Key = `thumbnails/${simId}/thumb.${ext}`;

  await env.SCENES.put(r2Key, body, {
    httpMetadata: { contentType },
    customMetadata: { simId, authorId: auth },
  });

  // Public thumbnail URL. This was simulations.eustress.dev, a host that never
  // had a DNS record, so every record published before now carries a dead link.
  // liveThumbnailUrl rewrites those on read rather than migrating KV.
  const thumbnailUrl = `${new URL(request.url).origin}/api/simulations/${simId}/thumbnail`;
  sim.thumbnail_url = thumbnailUrl;
  sim.updated_at = new Date().toISOString();
  await env.SOCIAL.put(`sim:${simId}`, JSON.stringify(sim));

  return json({ thumbnail_url: thumbnailUrl }, 200, cors);
}

// Return the authenticated user's published simulations as "projects"
async function handleUserProjects(request, url, env, cors) {
  const limit = parseInt(url.searchParams.get('limit') || '50');
  const page = parseInt(url.searchParams.get('page') || '1');

  const auth = await verifyAuth(request, env);
  if (!auth) {
    // Not signed in — return empty list (not an error, just no projects)
    return json({ projects: [], total: 0, page, limit }, 200, cors);
  }

  try {
    // Find all simulations authored by this user via sim-author:{userId}:* keys
    const list = await env.SOCIAL.list({ prefix: `sim-author:${auth}:`, limit: 100 });
    const projects = [];

    for (const key of list.keys) {
      const simId = await env.SOCIAL.get(key.name);
      if (!simId) continue;
      const simData = await env.SOCIAL.get(`sim:${simId}`);
      if (!simData) continue;

      try {
        const sim = JSON.parse(simData);
        projects.push({
          id: sim.id || simId,
          name: sim.name || 'Untitled',
          description: sim.description || null,
          thumbnail_url: liveThumbnailUrl(sim),
          status: 'published',
          genre: sim.genre || 'All',
          max_players: sim.max_players || 10,
          is_public: sim.is_public !== false,
          version: sim.version || 1,
          play_count: sim.play_count || 0,
          favorite_count: sim.favorite_count || 0,
          last_edited: sim.updated_at || sim.published_at || '',
          created_at: sim.published_at || '',
          published_at: sim.published_at || null,
          storage_url: sim.r2_key || null,
        });
      } catch (_) {}
    }

    // Sort by last_edited descending
    projects.sort((a, b) => new Date(b.last_edited) - new Date(a.last_edited));

    return json({ projects, total: projects.length, page, limit }, 200, cors);
  } catch (e) {
    return json({ projects: [], total: 0, page, limit }, 200, cors);
  }
}

async function handleListSimulations(env, cors) {
  try {
    const list = await env.SOCIAL.list({ prefix: 'sim:', limit: 100 });
    const sims = [];

    for (const key of list.keys) {
      if (key.name.startsWith('sim-author:')) continue;
      if (key.name.startsWith('simCount:')) continue;
      const data = await env.SOCIAL.get(key.name);
      if (data) {
        try {
          const sim = JSON.parse(data);
          sim.thumbnail_url = liveThumbnailUrl(sim);
          sims.push(sim);
        } catch (_) {}
      }
    }

    sims.sort((a, b) => new Date(b.published_at) - new Date(a.published_at));
    return json({ simulations: sims, total: sims.length }, 200, cors);
  } catch (e) {
    return json({ simulations: [], total: 0 }, 200, cors);
  }
}

async function handleGetSimulation(simId, env, cors) {
  const data = await env.SOCIAL.get(`sim:${simId}`);
  if (!data) return json({ error: 'Simulation not found' }, 404, cors);
  const sim = JSON.parse(data);
  sim.thumbnail_url = liveThumbnailUrl(sim);
  return json(sim, 200, cors);
}

// Play a simulation — returns server connection info
// Download .pak — streams the R2 object directly to the caller.
// Public simulations: no auth required. Private: requires auth + ownership.
async function handleDownloadPak(request, simId, env, cors) {
  const simData = await env.SOCIAL.get(`sim:${simId}`);
  if (!simData) return json({ error: 'Simulation not found' }, 404, cors);
  const sim = JSON.parse(simData);

  // Private simulations require auth
  if (!sim.is_public) {
    const auth = await verifyAuth(request, env);
    if (!auth || auth !== sim.author_id)
      return json({ error: 'Private simulation — access denied' }, 403, cors);
  }

  if (!sim.r2_key) return json({ error: 'No published .pak' }, 404, cors);

  const object = await env.SCENES.get(sim.r2_key);
  if (!object) return json({ error: '.pak not found in storage' }, 404, cors);

  return new Response(object.body, {
    headers: {
      ...cors,
      'Content-Type': 'application/octet-stream',
      'Content-Disposition': `attachment; filename="${sim.name || simId}.pak"`,
      'Content-Length': object.size.toString(),
    },
  });
}

async function handlePlaySimulation(request, simId, env, cors) {
  const data = await env.SOCIAL.get(`sim:${simId}`);
  if (!data) return json({ error: 'Simulation not found' }, 404, cors);

  const sim = JSON.parse(data);

  // Private simulations require auth
  if (!sim.is_public) {
    const auth = await verifyAuth(request, env);
    if (!auth) return json({ error: 'Private simulation — sign in required' }, 401, cors);
  }

  // Increment play count
  sim.play_count = (sim.play_count || 0) + 1;
  await env.SOCIAL.put(`sim:${simId}`, JSON.stringify(sim));

  // Increment author's total plays
  if (sim.author_id) {
    const plays = parseInt(await env.SOCIAL.get(`totalPlays:${sim.author_id}`) || '0') + 1;
    await env.SOCIAL.put(`totalPlays:${sim.author_id}`, plays.toString());
  }

  // Check for an active server running this simulation
  const nodeList = await env.SOCIAL.list({ prefix: 'node:', limit: 100 });
  let activeServer = null;

  for (const key of nodeList.keys) {
    const nodeData = await env.SOCIAL.get(key.name);
    if (nodeData) {
      const node = JSON.parse(nodeData);
      if (node.simulation_id === simId && node.players < (sim.max_players || 100)) {
        activeServer = node;
        break;
      }
    }
  }

  if (activeServer) {
    // Existing server has room
    return json({
      status: 'ready',
      server: {
        node_id: activeServer.node_id,
        address: activeServer.address || 'localhost',
        port: activeServer.port || 7777,
        protocol: 'quic',
        players: activeServer.players,
        max_players: sim.max_players || 100,
      },
      simulation: { id: sim.id, name: sim.name },
    }, 200, cors);
  }

  // No active server — return launch instructions
  // In production: Forge SDK dispatches Nomad job here
  // For now: client launches local server
  return json({
    status: 'spawn',
    launch: {
      command: 'eustress-server',
      args: [
        '--port', '7777',
        '--max-players', (sim.max_players || 100).toString(),
        '--sim-id', simId,
      ],
      r2_key: sim.r2_key || null,
      // Was simulations.eustress.dev, whose DNS never resolved, so any server
      // that followed this URL failed to resolve it. handleDownloadPak is the
      // route that actually streams the object.
      pak_url: sim.r2_key ? `${new URL(request.url).origin}/api/simulations/${simId}/download` : null,
    },
    simulation: { id: sim.id, name: sim.name, description: sim.description },
  }, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// WEBSITE MANIFEST - the numbers a Space owns, baked at publish
// ═══════════════════════════════════════════════════════════════════════════
//
// A Space is the source of truth for its own figures. An author marks values as
// References in a Website service; publish bakes them into ONE small JSON
// document, and a website fetches that document once to update every number it
// shows. Twenty-five values cost one request, and a twenty-sixth costs nothing.
//
// This object is deliberately NOT the simulation listing. The listing lives in
// KV under `sim:{id}` and drives the marketplace; the manifest is a separate R2
// object at `universes/{id}/website-manifest.json`. Writing one must never
// touch the other, because overwriting the listing takes the Space out of the
// gallery.
//
// Specifications:
//   EustressEngine/docs/design/WEBSITE_SERVICE.md   engine and worker side
//   Voltec/docs/WEBSITE_MANIFEST_API.md             consumer side

// Manifests are scalars and labels, not payload. Twenty-five references bake to
// roughly 4 KB, so a megabyte is already three orders of magnitude of headroom
// and anything past it is a mistake worth refusing.
const WEBSITE_MANIFEST_MAX_BYTES = 1024 * 1024;

// A namespace is the stable handle a website hardcodes. It must not be
// UUID-shaped: the read route decides between "simulation id" and "namespace"
// by shape, so a UUID-shaped namespace would be unreachable. Enforced at write
// time rather than guessed at read time.
const WEBSITE_NAMESPACE_RE = /^[a-z0-9][a-z0-9_-]{0,63}$/;
const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

function websiteManifestKey(simId) {
  return `universes/${simId}/website-manifest.json`;
}

// CORS for the manifest routes only. These are the one part of this API meant
// to be read by sites we do not run, so the allowlist in corsHeaders() cannot
// apply: a browser on voltec.dev would be handed
// `Access-Control-Allow-Origin: https://eustress.dev` and refuse the response.
//
// Stating the tension plainly, because the auth key below is easy to misread as
// privacy: an `Access-Control-Allow-Origin` of `*` combined with a key that a
// public website sends from browser JavaScript, in a header or a query string,
// is NOT confidentiality. Any page may request this manifest, and any visitor
// who opens devtools reads the key straight out of the network tab. What the
// key genuinely buys is revocable attribution and abuse control: you can see
// which consumer is calling, a Cloudflare rate-limiting rule can key on the
// X-Eustress-Key header, and rotating the key cuts a consumer off on the next
// request. Real confidentiality would need a server-side proxy holding a secret
// the browser never sees, or short-lived signed tokens. This is neither.
// Nothing belongs in a Reference that the author would not put on a public page.
function manifestCorsHeaders() {
  return {
    'Access-Control-Allow-Origin': '*',
    'Access-Control-Allow-Methods': 'GET, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type, If-None-Match, X-Eustress-Key',
    // Cross-origin JavaScript cannot read ETag unless it is exposed. The browser
    // revalidates on its own, but a build-time baker wants the hash it just saw.
    'Access-Control-Expose-Headers': 'ETag',
    'Access-Control-Max-Age': '86400',
  };
}

// Matches both read forms, used by the OPTIONS short-circuit so a preflight is
// answered with `*` before route dispatch ever runs.
function isWebsiteManifestPath(pathname) {
  return /^\/api\/simulation\/[^/]+(\/latest)?\/manifest$/.test(pathname);
}

async function sha256Hex(text) {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return hexEncode(new Uint8Array(digest));
}

// Keys are stored as a SHA-256 digest and never in the clear, so a leaked
// bucket listing yields nothing a caller can present. The digest lives in the
// object's customMetadata rather than in the manifest body, because the body is
// the thing every consumer downloads.
async function hashWebsiteKey(raw) {
  return `sha256:${await sha256Hex(raw)}`;
}

// Constant time over the full width of both inputs. Comparing digests instead
// of raw keys already makes a timing leak useless, since SHA-256 does not
// invert, but an early-exit compare is the kind of detail that gets copied into
// a place where it does matter.
function timingSafeEqual(a, b) {
  const enc = new TextEncoder();
  const ab = enc.encode(typeof a === 'string' ? a : '');
  const bb = enc.encode(typeof b === 'string' ? b : '');
  const len = Math.max(ab.length, bb.length);
  let diff = ab.length ^ bb.length;
  for (let i = 0; i < len; i++)
    diff |= (i < ab.length ? ab[i] : 0) ^ (i < bb.length ? bb[i] : 0);
  return diff === 0;
}

// If-None-Match may carry a list, a weak validator, or `*`. `*` means "if any
// representation exists", and we only reach this once one does.
function etagMatches(header, publishHash) {
  if (!header || !publishHash) return false;
  const want = `"${publishHash}"`;
  return header.split(',').some(t => {
    const tag = t.trim().replace(/^W\//, '');
    return tag === '*' || tag === want;
  });
}

// Consumers pin with the full `sha256:...` value, but stripping the prefix is
// the obvious thing to try, and a 404 for a hash that is in fact current would
// be baffling. Normalize both sides.
function normalizeHash(h) {
  return typeof h === 'string' ? h.trim().replace(/^sha256:/i, '').toLowerCase() : '';
}

// A rejection must never stick in a shared cache: a rotated key has to take
// effect on the next request, not after a max-age expires.
function manifestError(status, code, message, extra, cors) {
  return new Response(JSON.stringify({ error: code, message, ...extra }), {
    status,
    headers: {
      ...cors,
      'Content-Type': 'application/json',
      'Cache-Control': 'no-store',
      'X-Content-Type-Options': 'nosniff',
      'Referrer-Policy': 'strict-origin-when-cross-origin',
    },
  });
}

// Returns a Response to send instead, or null when the caller may proceed.

// Per-key rate limit for manifest reads.
//
// Shape of the real traffic, which is what sets the number:
//   browsers      one fetch per visitor per 5 minutes (max-age=300), and the
//                 edge cache absorbs nearly all of it before the Worker runs
//   build bake    N namespaces fetched back to back at deploy, a genuine burst
//   revalidation  conditional If-None-Match, answered 304, almost free
//
// 120 per minute clears 1,200 manifests in ten minutes, so a site with many
// hundreds of products refreshes well inside that window, while remaining far
// below anything a real site reaches through cache.
//
// WHY the native binding and not a KV counter: Workers KV permits ONE WRITE PER
// SECOND TO THE SAME KEY. A counter incremented on every request writes to one
// key at exactly the request rate, so a 120/minute burst is 2 writes per second
// to that key and the counter is throttled by the very traffic it exists to
// measure. It would then undercount and under-enforce, which is the worst
// outcome: a limiter that looks present and is not. KV get-then-put is also a
// read-modify-write race across isolates. The native limiter is purpose built,
// and Cloudflare names API keys as the intended key.
//
// Honest about its granularity: the limit is PER CLOUDFLARE LOCATION and
// eventually consistent, so global throughput for one key can exceed the number
// when traffic is spread across colos. That is the right trade for cost control
// and abuse damping. It is not an accounting system and must never be presented
// as a quota anyone is billed against.
const MANIFEST_RATE_PER_MIN = 120;

async function checkManifestRate(env, identity) {
  // Absent binding means local dev or a stale deploy. Fail OPEN and say so:
  // silently dropping the limit is worse than a limit that is visibly absent,
  // and refusing every read because a limiter is missing would take the feature
  // down over a throttle.
  if (!env.MANIFEST_RATE_LIMITER) return { limited: false, enforced: false };

  const { success } = await env.MANIFEST_RATE_LIMITER.limit({ key: identity });
  return { limited: !success, enforced: true };
}

async function checkWebsiteKey(request, url, storedHash, namespace, cors, previousHash = null, overlapUntil = null) {
  // No stored key means the author published without one, so the manifest is
  // open. Refusing every caller over a key that was never issued would brick
  // the manifest rather than protect it. The PUT response reports key_required
  // so the publishing UI can tell the author which of the two they chose.
  if (!storedHash) return null;

  // Header first. The query parameter exists for consumers that cannot set
  // headers, such as a no-code embed or a CMS URL field, and it costs something
  // real: a key in a query string lands in access logs, Referer headers and
  // browser history. It is accepted because the key is not a secret, not
  // because query strings are a safe place for secrets.
  const presented = request.headers.get('X-Eustress-Key') || url.searchParams.get('key');

  if (!presented)
    return manifestError(401, 'auth_key_required',
      'This manifest requires an auth key. Send it as the request header X-Eustress-Key, or as the query parameter ?key=. The key is set in the Website service Properties of the Space that publishes this manifest, so ask that author for the current value. The key identifies a consumer, lets a rate limit apply per consumer, and can be revoked by rotation. It does not make the manifest private.',
      { namespace }, cors);

  const presentedHash = await hashWebsiteKey(presented);

  // The rotated-out key keeps working until the window closes. Both branches
  // run a full-width compare so the accepted and rejected paths cost the same.
  const overlapOpen = !!(previousHash && overlapUntil && Date.now() < Date.parse(overlapUntil));
  const matchesCurrent = timingSafeEqual(presentedHash, storedHash);
  const matchesPrevious = overlapOpen && timingSafeEqual(presentedHash, previousHash);

  if (!matchesCurrent && !matchesPrevious)
    return manifestError(401, 'auth_key_invalid',
      'The auth key presented does not match the key this manifest was published with. If the key was rotated, take the current value from the Website service Properties of the publishing Space and update any page that hardcodes it.',
      { namespace }, cors);

  return null;
}

// Namespace to simulation id.
//
// R2 has no secondary index, so resolving a namespace by listing `universes/`
// would be O(objects) on every page load. The smallest mechanism that works is
// a KV pointer written by the same PUT that stores the manifest, mirroring the
// existing `sim-author:` index idiom:
//
//   website-ns:{namespace}  -> { sim_id, owner_id, claimed_at, ... }
//   website-ns-of:{sim_id}  -> namespace
//
// The reverse key exists so a rename releases the old namespace instead of
// leaking it forever. Note the propagation caveat: a KV write reaches every
// edge within about a minute, so a namespace claimed seconds ago can still 404
// elsewhere in the world. Harmless against a 300 second max-age, but a publish
// UI must not present the namespace URL as instantly live.
//
// R2 customMetadata, not this pointer, is authoritative for the ETag and the
// key. The copies stored alongside are for answering "which Space does vcell
// point at", and are never read into a caching or auth decision.
async function resolveManifestTarget(segment, env) {
  if (UUID_RE.test(segment)) return { simId: segment, namespace: null };

  const ns = segment.toLowerCase();
  if (!WEBSITE_NAMESPACE_RE.test(ns))
    return { failure: { status: 400, code: 'bad_namespace', message: 'A namespace is 1 to 64 characters of lowercase letters, digits, hyphen or underscore, starting with a letter or digit.', namespace: segment } };

  const raw = await env.SOCIAL.get(`website-ns:${ns}`);
  if (!raw)
    return { failure: { status: 404, code: 'namespace_not_found', message: `No Space publishes a website manifest under the namespace "${ns}". Check the namespace in the publishing Space's Website service, or fetch by simulation id instead.`, namespace: ns } };

  let simId = null;
  try { simId = JSON.parse(raw).sim_id; } catch (_) {}

  // The pointer's sim_id becomes an R2 key, so its shape is checked here rather
  // than trusted. Only handlePutWebsiteManifest writes these records and its
  // route already constrains the id, but a value that reaches a storage path
  // should be validated where it is used, not where it was last written.
  if (typeof simId !== 'string' || !UUID_RE.test(simId))
    return { failure: { status: 500, code: 'namespace_index_corrupt', message: `The index entry for namespace "${ns}" does not name a valid simulation. Republish the Space to rewrite it.`, namespace: ns } };

  return { simId, namespace: ns };
}

// GET /api/simulation/{id}/manifest
// GET /api/simulation/{namespace}/latest/manifest
//
// `{id}` is a simulation UUID or a namespace. `{namespace}/latest` is the
// documented form; a bare namespace resolves identically, because a consumer
// who drops `/latest` should get their manifest rather than a 404 they cannot
// explain. The routes live on the singular `/api/simulation/` prefix and always
// end in `/manifest`, so they cannot collide with `/api/simulations/{id}` no
// matter what a namespace happens to spell.
async function handleGetWebsiteManifest(request, url, segment, env) {
  const cors = manifestCorsHeaders();

  const target = await resolveManifestTarget(segment, env);
  if (target.failure) {
    const f = target.failure;
    return manifestError(f.status, f.code, f.message, { namespace: f.namespace }, cors);
  }

  const objectKey = websiteManifestKey(target.simId);
  const inm = request.headers.get('If-None-Match');

  // A conditional request only needs metadata to answer, so HEAD first when the
  // caller offered a validator and fall through to a full GET only if it does
  // not match. Worst case is HEAD plus GET, which is exactly the case where a
  // whole body is being sent anyway.
  let object = null;
  let meta = null;
  if (inm) {
    meta = await env.SCENES.head(objectKey);
  } else {
    object = await env.SCENES.get(objectKey);
    meta = object;
  }

  if (!meta)
    return manifestError(404, 'manifest_not_found',
      `No website manifest is published for ${target.namespace ? `namespace "${target.namespace}"` : `simulation ${target.simId}`}. Add a Website service to the Space and publish it.`,
      { namespace: target.namespace, sim_id: target.simId }, cors);

  const storedHash = meta.customMetadata?.publishHash || null;
  const storedKeyHash = meta.customMetadata?.websiteKeyHash || null;
  const prevKeyHash = meta.customMetadata?.websiteKeyPreviousHash || null;
  const keyOverlapUntil = meta.customMetadata?.websiteKeyOverlapUntil || null;
  const namespace = target.namespace || meta.customMetadata?.namespace || null;

  // The key gate runs before anything is served, a 304 included, so a caller
  // without the key cannot poll the ETag to learn when a publish happened.
  const denied = await checkWebsiteKey(
    request, url, storedKeyHash, namespace, cors, prevKeyHash, keyOverlapUntil);
  if (denied) return denied;

  // Limit per KEY, not per IP. A build runs from one CI address while browsers
  // arrive from thousands, so an IP limit throttles the build and misses the
  // abuse. An unkeyed open manifest falls back to the namespace, which at least
  // bounds one manifest rather than the whole worker.
  const rateIdentity = storedKeyHash
    ? `k:${storedKeyHash.slice(-32)}`
    : `n:${namespace || target.simId}`;
  const rate = await checkManifestRate(env, rateIdentity);
  if (rate.limited) {
    return new Response(JSON.stringify({
      error: 'rate_limited',
      message: `Rate limit reached for this key: ${MANIFEST_RATE_PER_MIN} manifest requests per minute. Conditional requests answered 304 are cheaper than full downloads, so send If-None-Match rather than a cache-busting query string.`,
      retry_after_seconds: 60,
    }), {
      status: 429,
      headers: {
        'Content-Type': 'application/json',
        'Retry-After': '60',
        'Cache-Control': 'no-store',
        ...manifestCorsHeaders(),
      },
    });
  }

  const etag = storedHash ? `"${storedHash}"` : undefined;
  const pinned = url.searchParams.get('v');

  // A pinned response promises one exact state for a year, so serving a
  // different state under that promise is worse than refusing: the consumer
  // pinned precisely because it must not drift.
  if (pinned !== null && normalizeHash(pinned) !== normalizeHash(storedHash))
    return manifestError(404, 'pinned_hash_not_available',
      'The pinned publish_hash is not the state this manifest currently holds. A pinned URL is immutable by contract, so a different state is refused rather than served. Drop ?v= to follow the current manifest, or pin again to the current hash.',
      { namespace, requested: pinned, current: storedHash }, cors);

  const cacheControl = pinned !== null
    ? 'public, max-age=31536000, immutable'
    : 'public, max-age=300, stale-while-revalidate=86400';

  // `public` plus authentication by request header is a cache-poisoning shape
  // unless the cache keys on that header, so Vary names it. The manifest is not
  // confidential, but a shared cache handing a keyed 200 to a keyless caller
  // would quietly make the key decorative.
  const served = {
    ...cors,
    'Cache-Control': cacheControl,
    'Vary': 'X-Eustress-Key',
    'X-Content-Type-Options': 'nosniff',
    'Referrer-Policy': 'strict-origin-when-cross-origin',
    ...(etag ? { 'ETag': etag } : {}),
  };

  if (inm && etagMatches(inm, storedHash))
    return new Response(null, { status: 304, headers: served });

  if (!object) object = await env.SCENES.get(objectKey);
  if (!object)
    return manifestError(404, 'manifest_not_found',
      'The manifest was removed between the metadata read and the body read. Retry.',
      { namespace, sim_id: target.simId }, cors);

  return new Response(object.body, {
    headers: { ...served, 'Content-Type': 'application/json' },
  });
}

// PUT /api/simulations/{id}/website-manifest
//
// The engine uploads here at publish, after the bake and alongside the .pak.
// Authenticated and owner-checked like every other upload route. It writes ONE
// R2 object plus the namespace index, and touches no listing record.
async function handlePutWebsiteManifest(request, simId, env, cors) {
  const auth = await verifyAuth(request, env);
  if (!auth) return json({ error: 'Unauthorized' }, 401, cors);

  const simData = await env.SOCIAL.get(`sim:${simId}`);
  if (!simData) return json({ error: 'Simulation not found' }, 404, cors);
  const sim = JSON.parse(simData);
  if (sim.author_id !== auth) return json({ error: 'Not your simulation' }, 403, cors);

  const raw = await request.text();
  const rawBytes = new TextEncoder().encode(raw).length;
  if (rawBytes > WEBSITE_MANIFEST_MAX_BYTES)
    return json({ error: `Manifest too large (max ${WEBSITE_MANIFEST_MAX_BYTES} bytes, got ${rawBytes})` }, 413, cors);

  let manifest;
  try { manifest = JSON.parse(raw); }
  catch (e) { return json({ error: `Manifest is not valid JSON: ${e.message}` }, 400, cors); }

  if (!manifest || typeof manifest !== 'object' || Array.isArray(manifest))
    return json({ error: 'Manifest must be a JSON object' }, 400, cors);

  const namespace = typeof manifest.namespace === 'string' ? manifest.namespace.toLowerCase() : '';
  if (!WEBSITE_NAMESPACE_RE.test(namespace))
    return json({ error: 'namespace required: 1 to 64 characters of lowercase letters, digits, hyphen or underscore, starting with a letter or digit' }, 400, cors);
  if (UUID_RE.test(namespace))
    return json({ error: 'namespace must not be UUID-shaped: the read route tells a simulation id from a namespace by shape, so a UUID-shaped namespace would be unreachable' }, 400, cors);

  if (!manifest.values || typeof manifest.values !== 'object' || Array.isArray(manifest.values))
    return json({ error: 'values required (an object of baked references)' }, 400, cors);

  // The engine already computes a publish hash and already skips uploads when
  // it is unchanged, so that is the ETag rather than something invented here.
  const publishHash = typeof manifest.publish_hash === 'string' ? manifest.publish_hash.trim() : '';
  if (!publishHash)
    return json({ error: 'publish_hash required (it becomes the ETag consumers revalidate against)' }, 400, cors);

  // Key material must never reach the served body, where every consumer could
  // grind it offline. Strip the field names an author might plausibly have
  // used, and report what was removed rather than silently rewriting their
  // document. Top level only: a reference legitimately named "key" lives under
  // `values` and is untouched.
  const stripped = [];
  for (const field of ['auth_key', 'key', 'key_hash', 'api_key', 'secret', 'token']) {
    if (field in manifest) { delete manifest[field]; stripped.push(field); }
  }

  const objectKey = websiteManifestKey(simId);
  const existing = await env.SCENES.head(objectKey);
  const previousKeyHash = existing?.customMetadata?.websiteKeyHash || null;

  // Three ways the key can move, and the default is the safe one.
  //   X-Eustress-Key        raw key, hashed here and discarded
  //   X-Eustress-Key-Hash   "sha256:<64 hex>", for a publisher that prefers the
  //                         raw key never leave the author's machine
  //   X-Eustress-Key-Clear  "true", the only way to make a keyed manifest open
  // Absent all three the existing key is preserved, because a routine
  // republish must not silently un-protect a manifest. The engine sends
  // X-Eustress-Key-Clear when the Properties key field is empty; that is the
  // contract, and preserve-on-absent is the fallback for every other caller.
  let keyHash = previousKeyHash;
  // Default 30 days, matching what the Properties panel and both specs promise.
  // 0 means cut over immediately, which is the right choice for a key believed
  // to be compromised: a grace window is a convenience, not something to force
  // on an author who needs the old key dead now.
  const overlapDaysRaw = (request.headers.get('X-Eustress-Key-Overlap-Days') || '').trim();
  const overlapDays = /^\d+$/.test(overlapDaysRaw) ? Math.min(parseInt(overlapDaysRaw, 10), 365) : 30;
  let rotatedOutHash = existing?.customMetadata?.websiteKeyPreviousHash || null;
  let overlapUntil = existing?.customMetadata?.websiteKeyOverlapUntil || null;
  const clearKey = (request.headers.get('X-Eustress-Key-Clear') || '').trim().toLowerCase() === 'true';
  const rawKey = request.headers.get('X-Eustress-Key');
  const preHashedKey = request.headers.get('X-Eustress-Key-Hash');

  if (clearKey) {
    keyHash = null;
  } else if (rawKey) {
    // A guessable key defeats revocation as surely as no key at all, since
    // anyone can simply present the guess again after a rotation.
    if (rawKey.trim().length < 16)
      return json({ error: 'Auth key must be at least 16 characters' }, 400, cors);
    keyHash = await hashWebsiteKey(rawKey.trim());
  } else if (preHashedKey) {
    if (!/^sha256:[0-9a-f]{64}$/.test(preHashedKey.trim()))
      return json({ error: 'X-Eustress-Key-Hash must be "sha256:" followed by 64 lowercase hex characters' }, 400, cors);
    keyHash = preHashedKey.trim();
  }

  // The namespace claim is the one genuine security property in this design.
  // Without it any authenticated author could point their own Space at someone
  // else's namespace and rewrite the numbers on that person's website.
  let claimedAt = new Date().toISOString();
  const claimRaw = await env.SOCIAL.get(`website-ns:${namespace}`);
  if (claimRaw) {
    let claim = null;
    try { claim = JSON.parse(claimRaw); } catch (_) {}
    // Fail closed. A record without an owner is not evidence of consent.
    if (!claim || claim.owner_id !== auth)
      return json({
        error: 'Namespace already claimed',
        message: `The namespace "${namespace}" is published by another author. Choose a different namespace in the Website service.`,
      }, 409, cors);
    claimedAt = claim.claimed_at || claimedAt;
  }

  const body = JSON.stringify(manifest);
  const bodyDigest = await sha256Hex(body);

  // The failure this whole feature exists to prevent is a website confidently
  // showing a stale number. Content that changed under an unchanged
  // publish_hash produces exactly that: every consumer holding the old ETag
  // gets a 304 forever, and every pinned consumer caches the old state for a
  // year. Refuse, and say what to do about it.
  if (existing
      && existing.customMetadata?.publishHash === publishHash
      && existing.customMetadata?.bodyDigest
      && existing.customMetadata.bodyDigest !== bodyDigest)
    return json({
      error: 'publish_hash unchanged but manifest content changed',
      message: 'Consumers revalidate against publish_hash, so republishing different values under the same hash would serve them the previous state until their cache expires. Bump publish_hash and upload again.',
      publish_hash: publishHash,
    }, 409, cors);

  // Rotation bookkeeping, after keyHash is final. Only an actual CHANGE opens a
  // window: a republish with the same key must not keep extending it, or the
  // outgoing key never dies. Clearing the key closes the window immediately,
  // because "open to everyone" and "the old key still works" are different
  // states and conflating them would leave a revoked key alive on an open
  // manifest.
  if (clearKey) {
    rotatedOutHash = null;
    overlapUntil = null;
  } else if (previousKeyHash && keyHash && keyHash !== previousKeyHash) {
    rotatedOutHash = previousKeyHash;
    overlapUntil = overlapDays > 0
      ? new Date(Date.now() + overlapDays * 86400000).toISOString()
      : null;
  }

  await env.SCENES.put(objectKey, body, {
    httpMetadata: { contentType: 'application/json' },
    customMetadata: {
      simId,
      namespace,
      authorId: auth,
      publishHash,
      bodyDigest,
      updatedAt: new Date().toISOString(),
      ...(manifest.schema_version !== undefined ? { schemaVersion: String(manifest.schema_version) } : {}),
      // Authoritative for the read path. An R2 put replaces customMetadata
      // wholesale, which is why previousKeyHash is carried forward above.
      ...(keyHash ? { websiteKeyHash: keyHash } : {}),
      // Rotation grace. When the key CHANGES, the outgoing hash keeps working
      // until overlapUntil. Without this, rotation 401s every consumer the
      // instant the author clicks the button, which makes the one lever the
      // key provides too dangerous to pull on a live site. Carried forward
      // unchanged on a routine republish so the window does not creep.
      ...(rotatedOutHash ? { websiteKeyPreviousHash: rotatedOutHash } : {}),
      ...(overlapUntil ? { websiteKeyOverlapUntil: overlapUntil } : {}),
    },
  });

  const now = new Date().toISOString();
  await env.SOCIAL.put(`website-ns:${namespace}`, JSON.stringify({
    sim_id: simId,
    owner_id: auth,
    claimed_at: claimedAt,
    updated_at: now,
    // Informational only. See resolveManifestTarget: R2 customMetadata is
    // authoritative, so these can never be read into an auth or ETag decision.
    publish_hash: publishHash,
    schema_version: manifest.schema_version ?? null,
  }));

  // A rename must release the old namespace, or the author can never reuse it.
  const previousNs = await env.SOCIAL.get(`website-ns-of:${simId}`);
  if (previousNs && previousNs !== namespace) {
    const prevClaim = await env.SOCIAL.get(`website-ns:${previousNs}`);
    try {
      if (prevClaim && JSON.parse(prevClaim).sim_id === simId)
        await env.SOCIAL.delete(`website-ns:${previousNs}`);
    } catch (_) {}
  }
  await env.SOCIAL.put(`website-ns-of:${simId}`, namespace);

  const origin = new URL(request.url).origin;
  return json({
    ok: true,
    sim_id: simId,
    namespace,
    publish_hash: publishHash,
    schema_version: manifest.schema_version ?? null,
    // The publishing UI reports this back to the author, so "I set a key" and
    // "this manifest is open" can never be confused for each other.
    key_required: !!keyHash,
    key_rotated: !!keyHash && keyHash !== previousKeyHash,
    key_cleared: clearKey && !!previousKeyHash,
    stripped_fields: stripped,
    size_bytes: new TextEncoder().encode(body).length,
    manifest_url: `${origin}/api/simulation/${namespace}/latest/manifest`,
    manifest_url_by_id: `${origin}/api/simulation/${simId}/manifest`,
    pinned_url: `${origin}/api/simulation/${namespace}/latest/manifest?v=${encodeURIComponent(publishHash)}`,
  }, 200, cors);
}

// GET /api/simulations/{id}/thumbnail
//
// Ported from the retired eustress-simulations worker, which read
// `{id}/thumbnail.webp` - a key namespace nothing ever wrote. The live key is
// the one handleUploadThumbnail writes. webp is tried first because that is
// what the engine uploads, so the common case is a single R2 read.
async function handleGetThumbnail(simId, env, cors) {
  for (const ext of ['webp', 'png', 'jpg']) {
    const object = await env.SCENES.get(`thumbnails/${simId}/thumb.${ext}`);
    if (!object) continue;
    return new Response(object.body, {
      headers: {
        ...cors,
        'Content-Type': object.httpMetadata?.contentType || `image/${ext === 'jpg' ? 'jpeg' : ext}`,
        'Cache-Control': 'public, max-age=3600',
      },
    });
  }
  return json({ error: 'Thumbnail not found' }, 404, cors);
}

// Thumbnails published before the eustress-simulations retirement carry
// `https://simulations.eustress.dev/thumbnails/{id}/thumb.ext`, a host whose
// DNS never resolved. Rewriting on read repairs every historical gallery card
// without a KV migration pass. The production host is hardcoded because the
// callers are list handlers that do not carry the request.
function liveThumbnailUrl(sim) {
  const u = sim?.thumbnail_url;
  if (!u) return null;
  if (!u.startsWith('https://simulations.eustress.dev/')) return u;
  return sim.id ? `https://api.eustress.dev/api/simulations/${sim.id}/thumbnail` : null;
}


// ═══════════════════════════════════════════════════════════════════════════
// ACCOUNTING — Revenue dashboard, cost tracking, automated flow
// ═══════════════════════════════════════════════════════════════════════════

// Monthly infrastructure costs (auto-deducted daily as 1/30th)
const INFRA_COSTS = {
  cloudflare_workers: 5.00,    // Workers Paid plan
  domain: 0.83,                // $10/year amortized
  stripe_connect: 0,           // Charged per-transaction, not monthly
  forge_base: 19.50,           // Nomad cluster base (scales with usage)
  r2_storage: 0.75,            // Asset storage estimate
  total_monthly: function() {
    return this.cloudflare_workers + this.domain + this.forge_base + this.r2_storage;
  },
  daily: function() {
    return this.total_monthly() / 30;
  }
};

async function handleAccountingDashboard(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  // Revenue
  const treasuryUsd = parseFloat(await env.PAYOUTS.get('treasury:total_usd') || '0');
  const platformUsd = parseFloat(await env.PAYOUTS.get('platform:total_usd') || '0');
  const depositCount = parseInt(await env.PAYOUTS.get('treasury:deposit_count') || '0');

  // Costs
  const totalCostsDeducted = parseFloat(await env.PAYOUTS.get('costs:total_deducted') || '0');
  const forgeCosts = parseFloat(await env.PAYOUTS.get('costs:forge') || '0');
  const stripeFees = parseFloat(await env.PAYOUTS.get('costs:stripe_fees') || '0');
  const infraCosts = parseFloat(await env.PAYOUTS.get('costs:infrastructure') || '0');

  // Payouts
  const totalPaidToContributors = parseFloat(await env.PAYOUTS.get('payouts:total_paid') || '0');

  // Daily metrics
  const dripRate = 0.00276;
  const dailyDrip = treasuryUsd * dripRate;           // 100% to contributors
  const dailyInfraCost = INFRA_COSTS.daily();          // Paid from platform revenue
  const platformNetDaily = (platformUsd / 30) - dailyInfraCost; // Platform profit after costs

  // User count
  const userList = await env.USERS.list({ prefix: 'username:', limit: 1000 });
  const totalUsers = userList.keys.length;

  return json({
    revenue: {
      treasury_balance: treasuryUsd,
      platform_balance: platformUsd,
      total_deposits: depositCount,
      total_revenue: treasuryUsd + platformUsd + totalCostsDeducted + totalPaidToContributors,
    },
    costs: {
      monthly_infrastructure: INFRA_COSTS.total_monthly(),
      daily_infrastructure: dailyInfraCost,
      total_deducted: totalCostsDeducted,
      breakdown: {
        cloudflare: INFRA_COSTS.cloudflare_workers,
        domain: INFRA_COSTS.domain,
        forge_servers: INFRA_COSTS.forge_base,
        r2_storage: INFRA_COSTS.r2_storage,
        stripe_fees: stripeFees,
        forge_compute: forgeCosts,
      },
    },
    contributors: {
      treasury_balance: treasuryUsd,
      daily_drip: dailyDrip,
      total_paid: totalPaidToContributors,
      note: '100% of treasury drip goes to contributors. No deductions.',
    },
    platform: {
      revenue: platformUsd,
      costs_paid: totalCostsDeducted,
      net_profit: platformUsd - totalCostsDeducted,
      daily_net: platformNetDaily,
      note: 'Costs paid from platform 50%, never from contributor treasury.',
    },
    users: {
      total_registered: totalUsers,
    },
    health: {
      profitable: platformUsd > totalCostsDeducted,
      runway_days: dailyInfraCost > 0 ? Math.floor(platformUsd / dailyInfraCost) : 999,
      margin_percent: platformUsd > 0
        ? ((platformUsd - totalCostsDeducted) / platformUsd * 100).toFixed(1) + '%'
        : 'N/A',
    },
    timestamp: new Date().toISOString(),
  }, 200, cors);
}

// Record a cost (called by Forge autoscaler or admin)
async function handleRecordCost(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const { category, amount, description } = await request.json();
  if (!category || !amount) return json({ error: 'category and amount required' }, 400, cors);

  // Accumulate cost
  const key = `costs:${category}`;
  const current = parseFloat(await env.PAYOUTS.get(key) || '0');
  await env.PAYOUTS.put(key, (current + amount).toString());

  // Track total
  const totalKey = 'costs:total_deducted';
  const total = parseFloat(await env.PAYOUTS.get(totalKey) || '0');
  await env.PAYOUTS.put(totalKey, (total + amount).toString());

  // Log
  await env.PAYOUTS.put(`cost:${Date.now()}`, JSON.stringify({
    category, amount, description: description || '',
    timestamp: new Date().toISOString(), recorded_by: adminId,
  }), { expirationTtl: 86400 * 365 * 5 });

  await auditLog(env, 'RECORD_COST', adminId, category, { amount, description });

  return json({ success: true, category, amount, new_total: current + amount }, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// BLISS ECONOMICS — canonical model, ported from bliss-core 0.1.1
// (economics.rs). Two daily flows, both distributed by the SAME
// day-score snapshot (contrib:{date}:{user}):
//
//   1. BLS emission  — tail emission: 5% of supply/year, halving every
//      4 years, 0.5% floor forever. Minted at UTC midnight for the
//      previous day's contributors. 100% to contributors.
//   2. USD treasury drip — exponential decay: 0.276%/day of remaining
//      balance (0.136%/day in scarcity mode: remaining ≤ 15% of the
//      high-water mark; top-25% contributors get 2x weight while
//      scarce). HWM decays 0.171%/day toward remaining. 100% of the
//      drip to contributors via Stripe Connect.
//
// KV keys (PAYOUTS namespace):
//   bliss:genesis_date       — ISO date of the first distribution
//   bliss:current_supply     — total BLS supply (starts 100,000,000)
//   bliss:total_distributed  — lifetime BLS minted to contributors
//   distribution:{date}      — per-day emission record (idempotency)
//   treasury:total_usd       — remaining treasury balance
//   treasury:hwm             — treasury high-water mark
//   payout:{date}            — per-day USD payout record
//
// Known deviations from bliss-core, both deliberate:
//   • Stripe's $0.50 transfer minimum means sub-minimum shares are
//     skipped; only actually-transferred USD is debited, so skipped
//     shares stay in the treasury (favors future cycles).
//   • Balances are f64 (KV JSON), not 18-decimal fixed-point — ~1e-7
//     BLS precision at 100M scale, fine for the ledger's current stage.
// ═══════════════════════════════════════════════════════════════════════════

const BLISS_INITIAL_SUPPLY = 100_000_000;
const BLISS_INITIAL_RATE = 0.05;      // 5% year one
const BLISS_HALVING_YEARS = 4;
const BLISS_TAIL_RATE = 0.005;        // 0.5% floor, forever
const TREASURY_DRIP_RATE = 0.00276;   // 0.276%/day (normal)
const TREASURY_SCARCITY_RATE = 0.00136; // 0.136%/day (scarcity)
const TREASURY_SCARCITY_RATIO = 0.15; // scarce when remaining ≤ 15% of HWM
const TREASURY_TOP_BOOST = 2.0;       // top-contributor boost in scarcity
const TREASURY_TOP_FRACTION = 0.25;   // "top" = top 25% by score
const TREASURY_HWM_DECAY = 0.00171;   // 0.171%/day HWM decay

// ── Contribution integrity (anti-abuse) ─────────────────────────────
// The cosign endpoint accepts self-reported work, so the witness — not
// the client — must bound what any single account can earn. See the
// security notes above handleCosign.
//
// Per-user daily score ceiling. Score is weighted minutes
// (weight × minutes × node bonus); the max legitimate day is a marathon
// session at the top weight: 16h × 60 × 3.0 (Development) × 1.1 (Full)
// ≈ 3,168. A real mixed session lands far below this, so the cap never
// clips honest work — it only bounds a script hammering the endpoint,
// shrinking the worst case from ~570k/day to this number.
const MAX_DAILY_SCORE = 3200;
// Max presence seconds credited per heartbeat. The engine beats every
// ~90s; capping the per-beat credit means a heartbeat flood can't
// inflate observed presence, and an offline gap can't over-credit when
// the session resumes. Presence is thus wall-clock bounded.
const PRESENCE_MAX_STEP = 150;

// ═══════════════════════════════════════════════════════════════════════════
// BLS LEDGER — integer minor units, append-only, auditable
// ═══════════════════════════════════════════════════════════════════════════
//
// WHY THIS EXISTS. Balances used to be an f64 field mutated in place on the
// user record. That had three disqualifying properties for money:
//   1. Floats don't reconcile — two systems disagree on the last digits and
//      no amount of rounding makes a float ledger auditable.
//   2. Read-modify-write on KV (no compare-and-set) loses updates: a
//      concurrent write could silently erase credited BLS.
//   3. There was no record of HOW a balance got to its value. You could not
//      reconstruct, audit, or dispute it.
//
// The fix is the standard one: an append-only log of integer entries is the
// truth; a balance is a derived number.
//
// PRECISION: **2 decimals**. 1 BLS = 100 minor units. This deliberately
// diverges from `bliss-core`'s 18-decimal constant — that figure targets an
// on-chain token, and this is an off-chain ledger where exact integer math
// and human-readable amounts matter more. Every stored amount is an INTEGER
// number of minor units. Never store a fractional BLS amount.
//
// KEYS (in the PAYOUTS namespace):
//   entry:{userId}:{ts}:{id}  append-only entry {amount_minor, kind, ref, ts}
//   bal:{userId}              integer cache of the summed entries
//   cp:{userId}               {balance_minor, through} checkpoint for fast sums
//
// IDEMPOTENCY: entry keys are deterministic for automated credits (the daily
// distribution uses the score date), so replaying a cron cannot double-credit
// — the append is a no-op if the key already exists.
//
// The `bal:` cache is an optimization, NOT the source of truth. The daily
// cron re-derives every touched balance from entries and rewrites the cache,
// so any drift is self-healing within 24h.

/// Minor units per whole BLS. 2 decimal places.
const BLISS_UNIT = 100;

/// Contribution score representing ONE FULL DAY of network contribution —
/// the amount of work that earns the entire daily emission.
///
/// Score is weighted minutes (`weight × minutes × node bonus`), so this is
/// 8 hours at the top weight: 8 × 60 × 3.0 (Development) = 1440.
///
/// WHY THIS EXISTS. The pool used to be split purely by SHARE
/// (`your_score / total_score`), which is the Bitcoin block-reward model: the
/// only participant collects the whole reward no matter how little they did.
/// In practice a day with a score of 3.0 — one minute of work — minted the
/// same ~13,736 BLS as a day with 8x the effort. Effort was decoupled from
/// reward, which makes "proof of contribution" meaningless at small N.
///
/// So the daily emission is a CEILING, not a guarantee. The day mints
/// `emission × min(1, total_score / FULL_DAY_SCORE)`, and the remainder is
/// simply never created — supply tracks real contribution instead of the
/// calendar. Relative split between contributors is unchanged.
///
/// TUNING: raising this makes BLS harder to earn; lowering it makes a short
/// day worth proportionally more. It does not change the long-run supply
/// ceiling, only how much of each day's allowance is actually minted.
const FULL_DAY_SCORE = 1440;

/// Contribution score a creator earns per Ticket of verified sales.
///
/// This is how Bliss pays for IMPORTANCE rather than time. Effort score is
/// self-reported minutes; value score is a purchase that actually happened,
/// so it is server-verified and therefore NOT subject to MAX_DAILY_SCORE —
/// that cap exists precisely because effort cannot be verified. A creator
/// whose work people pay for can out-earn one who merely logged hours.
///
/// At 0.5, a 1,000-Ticket day (~$11 of sales) is worth 500 score, roughly a
/// 2.8-hour Development day. Raise it to tilt the economy further toward
/// outcomes and away from presence.
const VALUE_SCORE_PER_TICKET = 0.5;

/// Whole-BLS float -> integer minor units. Only for migration and for
/// converting emission math; never for storing user input.
function toMinor(bls) {
  return Math.round((Number(bls) || 0) * BLISS_UNIT);
}

/// Integer minor units -> whole-BLS number for JSON responses.
function fromMinor(minor) {
  return (Number(minor) || 0) / BLISS_UNIT;
}

/// Human display, always 2dp.
function formatBliss(minor) {
  return fromMinor(minor).toFixed(2);
}

/// Append a ledger entry. Returns true if written, false if the key already
/// existed (idempotent replay). `id` MUST be stable for automated credits.
async function ledgerAppend(env, userId, { amount_minor, kind, ref, ts, id }) {
  const amount = Math.trunc(Number(amount_minor) || 0);
  if (amount === 0) return false;
  const stamp = ts || new Date().toISOString();
  const key = `entry:${userId}:${stamp}:${id}`;
  if (await env.PAYOUTS.get(key)) return false;
  await env.PAYOUTS.put(
    key,
    JSON.stringify({ amount_minor: amount, kind, ref: ref || null, ts: stamp })
  );
  // Advance the cache. Truth is the entries; this is a fast-read convenience
  // that the daily reconcile rebuilds.
  const cur = parseInt(await env.PAYOUTS.get(`bal:${userId}`) || '0', 10);
  await env.PAYOUTS.put(`bal:${userId}`, String(cur + amount));
  return true;
}

/// Spend BLS. Appends a NEGATIVE ledger entry and burns the amount from
/// circulating supply.
///
/// A currency needs a sink. Until this existed the ledger could only ever
/// credit, so BLS accumulated forever with nothing to do — a scoreboard, not
/// money. Spending is a first-class ledger operation: it writes the same kind
/// of append-only entry a credit does (so the audit trail stays complete and
/// the balance stays derived), and it burns rather than transferring, which
/// keeps the emission schedule the only source of new BLS.
///
/// `purpose` is recorded verbatim so a sink can be added without touching the
/// ledger again. Idempotent per `ref` — a retried client call cannot
/// double-spend.
async function ledgerSpend(env, userId, { amount_minor, purpose, ref }) {
  const amount = Math.trunc(Number(amount_minor) || 0);
  if (amount <= 0) return { ok: false, error: 'Amount must be positive' };
  if (!purpose) return { ok: false, error: 'purpose required' };

  const balance = await ledgerBalanceMinor(env, userId);
  if (balance < amount) {
    return { ok: false, error: 'Insufficient balance', balance_minor: balance, required_minor: amount };
  }

  const id = ref ? `spend-${ref}` : `spend-${crypto.randomUUID()}`;
  const wrote = await ledgerAppend(env, userId, {
    amount_minor: -amount,
    kind: 'spend',
    ref: purpose,
    id,
  });
  if (!wrote) {
    return { ok: false, error: 'Duplicate spend reference', balance_minor: balance };
  }

  // Burned, not transferred — emission stays the only mint.
  const burned = parseInt(await env.PAYOUTS.get('bliss:burned_minor') || '0', 10);
  await env.PAYOUTS.put('bliss:burned_minor', String(burned + amount));

  return { ok: true, spent_minor: amount, balance_minor: balance - amount };
}

async function handleLedgerSpend(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const body = await request.json().catch(() => ({}));
  const { amount, purpose, ref } = body;
  const amountMinor = toMinor(amount);
  if (!Number.isFinite(amountMinor) || amountMinor <= 0)
    return json({ error: 'amount must be a positive number of BLS' }, 400, cors);

  const res = await ledgerSpend(env, userId, { amount_minor: amountMinor, purpose, ref });
  if (!res.ok) {
    const status = res.error === 'Insufficient balance' ? 402
      : res.error === 'Duplicate spend reference' ? 409 : 400;
    return json({
      error: res.error,
      balance: res.balance_minor !== undefined ? fromMinor(res.balance_minor) : undefined,
    }, status, cors);
  }
  return json({
    ok: true,
    spent: fromMinor(res.spent_minor),
    balance: fromMinor(res.balance_minor),
    balance_display: formatBliss(res.balance_minor),
  }, 200, cors);
}

/// Sum every entry for a user (authoritative). Uses the checkpoint to avoid
/// re-reading history that has already been folded in.
async function ledgerDeriveMinor(env, userId) {
  const cpRaw = await env.PAYOUTS.get(`cp:${userId}`);
  const cp = cpRaw ? JSON.parse(cpRaw) : { balance_minor: 0, through: '' };
  let total = Math.trunc(cp.balance_minor || 0);
  let newest = cp.through || '';
  const prefix = `entry:${userId}:`;
  let cursor;
  while (true) {
    const list = await env.PAYOUTS.list({ prefix, limit: 1000, cursor });
    for (const k of list.keys) {
      if (cp.through && k.name <= cp.through) continue;
      const v = await env.PAYOUTS.get(k.name);
      if (!v) continue;
      total += Math.trunc(JSON.parse(v).amount_minor || 0);
      if (k.name > newest) newest = k.name;
    }
    if (list.list_complete || !list.cursor) break;
    cursor = list.cursor;
  }
  return { balance_minor: total, through: newest };
}

/// Fast balance read (cache). Falls back to deriving when the cache is absent,
/// and LAZILY MIGRATES a legacy float balance if this account has no ledger
/// history yet.
///
/// The lazy path matters: the distribution cron only migrates accounts that
/// scored that day, so a holder who stopped contributing would otherwise have
/// no entries and no cache — and would read as a balance of ZERO. Migrating
/// on first read guarantees every legacy balance survives.
async function ledgerBalanceMinor(env, userId) {
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
  await env.PAYOUTS.put(`bal:${userId}`, String(balance_minor));
  return balance_minor;
}

/// Re-derive from entries, rewrite the cache and checkpoint. Self-heals any
/// cache drift. Called for each credited account by the daily cron.
async function ledgerReconcile(env, userId) {
  const { balance_minor, through } = await ledgerDeriveMinor(env, userId);
  await env.PAYOUTS.put(`bal:${userId}`, String(balance_minor));
  await env.PAYOUTS.put(`cp:${userId}`, JSON.stringify({ balance_minor, through }));
  return balance_minor;
}

// ═══════════════════════════════════════════════════════════════════════════
// LEDGER TRANSPARENCY — public, read-only
// ═══════════════════════════════════════════════════════════════════════════
// The docs claim "the ledger is public; anyone can verify the math." These
// endpoints are what make that true. They expose aggregate supply/emission
// and per-day distribution records (which contain per-recipient amounts that
// are already keyed by opaque account ids) — never emails or tokens.

async function handleLedgerSummary(env, cors) {
  // Fall back to the legacy float keys until the first post-migration cron
  // writes the minor-unit counters. Without this the endpoint reported
  // supply=100,000,000 and distributed=0 while the real ledger held ~178k
  // distributed — a transparency endpoint publishing a wrong number is worse
  // than publishing none.
  let supplyMinor = parseInt(await env.PAYOUTS.get('bliss:supply_minor') || '0', 10);
  if (!supplyMinor) {
    supplyMinor = toMinor(
      parseFloat(await env.PAYOUTS.get('bliss:current_supply') || String(BLISS_INITIAL_SUPPLY))
    );
  }
  let distributedMinor = parseInt(await env.PAYOUTS.get('bliss:distributed_minor') || '0', 10);
  if (!distributedMinor) {
    distributedMinor = toMinor(parseFloat(await env.PAYOUTS.get('bliss:total_distributed') || '0'));
  }
  const genesis = await env.PAYOUTS.get('bliss:genesis_date');
  const years = genesis
    ? Math.max(0, Math.floor((Date.now() - Date.parse(genesis)) / (365 * 86400 * 1000)))
    : 0;

  // Recent distributions so anyone can re-derive today's emission by hand.
  //
  // NOTE: KV lists lexicographically, so a bare `limit: 30` returned the
  // THIRTY OLDEST records while calling them "recent" — the newest days were
  // invisible. Page the whole prefix, then sort descending and trim.
  const keys = [];
  let cursor;
  while (true) {
    const page = await env.PAYOUTS.list({ prefix: 'distribution:', limit: 1000, cursor });
    keys.push(...page.keys);
    if (page.list_complete || !page.cursor || keys.length >= 3650) break;
    cursor = page.cursor;
  }
  keys.sort((a, b) => (a.name < b.name ? 1 : -1)); // newest first
  const recent = [];
  for (const k of keys.slice(0, 60)) {
    const v = await env.PAYOUTS.get(k.name);
    if (!v) continue;
    const r = JSON.parse(v);
    recent.push({
      date: r.date,
      emission_pool: r.emission_pool,
      minted: r.minted,
      total_score: r.total_score,
      contributors: r.contributor_count ?? (r.recipients || []).length,
      concentration_flag: r.concentration_flag ?? false,
      truncated: r.truncated ?? false,
    });
  }
  recent.sort((a, b) => (a.date < b.date ? 1 : -1));

  const burnedMinor = parseInt(await env.PAYOUTS.get('bliss:burned_minor') || '0', 10);
  return json({
    unit: { decimals: 2, minor_per_bls: BLISS_UNIT },
    supply: fromMinor(supplyMinor),
    supply_minor: supplyMinor,
    total_distributed: fromMinor(distributedMinor),
    total_distributed_minor: distributedMinor,
    // Spending burns rather than transfers, so circulating is what holders
    // actually still have.
    total_burned: fromMinor(burnedMinor),
    circulating: fromMinor(distributedMinor - burnedMinor),
    effort_full_day_score: FULL_DAY_SCORE,
    value_score_per_ticket: VALUE_SCORE_PER_TICKET,
    genesis_date: genesis,
    annual_emission_rate: blissEmissionRate(years),
    emission_model: {
      initial_rate: BLISS_INITIAL_RATE,
      halving_period_years: BLISS_HALVING_YEARS,
      tail_rate: BLISS_TAIL_RATE,
      initial_supply: BLISS_INITIAL_SUPPLY,
    },
    treasury_usd: parseFloat(await env.PAYOUTS.get('treasury:total_usd') || '0'),
    recent_distributions: recent,
  }, 200, cors);
}

/// Public daily balance series for one account, derived from its append-only
/// ledger entries (the authoritative source — includes the migration opening
/// entry, not just emission credits).
///
/// Returns one point per day that had activity, with a running cumulative
/// balance, so a dashboard can plot wallet growth without authenticating.
/// Sensitivity is equivalent to the per-day distribution records, which
/// already publish recipient ids and amounts.
async function handleLedgerHistory(userId, env, cors) {
  if (!userId || !/^[A-Za-z0-9_-]{1,128}$/.test(userId))
    return json({ error: 'Bad user id' }, 400, cors);

  const prefix = `entry:${userId}:`;
  const byDate = new Map();
  let cursor;
  while (true) {
    const page = await env.PAYOUTS.list({ prefix, limit: 1000, cursor });
    for (const k of page.keys) {
      const v = await env.PAYOUTS.get(k.name);
      if (!v) continue;
      const e = JSON.parse(v);
      // Fold the 1970 migration-opening entry into the genesis day so the
      // chart starts at the real opening balance instead of showing a
      // 56-year gap.
      const raw = (e.ts || '').slice(0, 10);
      const date = raw === '1970-01-01' ? 'opening' : raw;
      const cur = byDate.get(date) || { minor: 0, kinds: new Set() };
      cur.minor += Math.trunc(e.amount_minor || 0);
      cur.kinds.add(e.kind || 'unknown');
      byDate.set(date, cur);
    }
    if (page.list_complete || !page.cursor) break;
    cursor = page.cursor;
  }

  // 'opening' must come FIRST so the running total starts at the opening
  // balance. It does NOT sort there naturally — 'o' (0x6F) is greater than
  // '2' (0x32), so a plain sort put it last and made every intermediate
  // cumulative wrong.
  const dates = [...byDate.keys()].sort((a, b) => {
    if (a === 'opening') return -1;
    if (b === 'opening') return 1;
    return a < b ? -1 : a > b ? 1 : 0;
  });
  let cumulative = 0;
  const series = dates.map((date) => {
    const { minor, kinds } = byDate.get(date);
    cumulative += minor;
    return {
      date,
      change: fromMinor(minor),
      change_minor: minor,
      cumulative: fromMinor(cumulative),
      cumulative_minor: cumulative,
      kinds: [...kinds].sort(),
    };
  });

  return json({
    user_id: userId,
    unit: { decimals: 2, minor_per_bls: BLISS_UNIT },
    points: series.length,
    balance: fromMinor(cumulative),
    balance_minor: cumulative,
    series,
  }, 200, cors);
}

async function handleLedgerDistribution(date, env, cors) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date || '')) return json({ error: 'Bad date' }, 400, cors);
  const raw = await env.PAYOUTS.get(`distribution:${date}`);
  if (!raw) return json({ error: 'No distribution for that date' }, 404, cors);
  return json(JSON.parse(raw), 200, cors);
}

/// A contributor's own entry history — the audit trail for their balance.
async function handleLedgerMe(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  // Go through the cached read FIRST — it carries the lazy migration for
  // accounts whose legacy float balance has no ledger entries yet. Calling
  // ledgerDeriveMinor directly would report a balance of 0 for them.
  await ledgerBalanceMinor(env, userId);

  const prefix = `entry:${userId}:`;
  const items = [];
  let cursor;
  while (true) {
    const list = await env.PAYOUTS.list({ prefix, limit: 1000, cursor });
    for (const k of list.keys) {
      const v = await env.PAYOUTS.get(k.name);
      if (!v) continue;
      const e = JSON.parse(v);
      items.push({
        ts: e.ts, kind: e.kind, ref: e.ref,
        amount: fromMinor(e.amount_minor), amount_minor: e.amount_minor,
      });
    }
    if (list.list_complete || !list.cursor) break;
    cursor = list.cursor;
  }
  items.sort((a, b) => (a.ts < b.ts ? 1 : -1));

  const derived = await ledgerDeriveMinor(env, userId);
  const cached = parseInt(await env.PAYOUTS.get(`bal:${userId}`) || '0', 10);
  return json({
    balance: fromMinor(derived.balance_minor),
    balance_minor: derived.balance_minor,
    balance_display: formatBliss(derived.balance_minor),
    // Surfaced so drift between the fast cache and the authoritative entries
    // is visible rather than silent. The daily reconcile self-heals it.
    cache_in_sync: cached === derived.balance_minor,
    entry_count: items.length,
    entries: items,
  }, 200, cors);
}

async function handleCronHealth(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);
  const list = await env.PAYOUTS.list({ prefix: 'cronrun:', limit: 30 });
  const runs = [];
  for (const k of list.keys) {
    const v = await env.PAYOUTS.get(k.name);
    if (v) runs.push({ date: k.name.slice('cronrun:'.length), ...JSON.parse(v) });
  }
  runs.sort((a, b) => (a.date < b.date ? 1 : -1));
  const lastOk = runs.find((r) => r.ok);
  return json({
    runs,
    last_success: lastOk ? lastOk.date : null,
    // A gap here means a day was silently skipped — distribution is
    // idempotent per date, so a missed day needs a manual re-run.
    missing_days: (() => {
      const have = new Set(runs.map((r) => r.date));
      const out = [];
      for (let i = 1; i <= 14; i++) {
        const d = new Date(Date.now() - i * 86400 * 1000).toISOString().split('T')[0];
        if (!have.has(d)) out.push(d);
      }
      return out;
    })(),
  }, 200, cors);
}

/// Snapshot the entire BLS ledger to R2. Without this, losing or corrupting
/// the KV namespace would destroy every balance with no recovery path — the
/// entries ARE the money, so they need to exist somewhere else too.
///
/// Writes a single JSON object per day: every entry, every cached balance,
/// and the supply counters. Restoring is replaying the entries.
async function backupLedger(env) {
  if (!env.SCENES) return null; // R2 not bound — nothing to write to
  const date = new Date().toISOString().split('T')[0];

  const entries = [];
  for (const prefix of ['entry:', 'bal:', 'cp:']) {
    let cursor;
    while (true) {
      const list = await env.PAYOUTS.list({ prefix, limit: 1000, cursor });
      for (const k of list.keys) {
        const v = await env.PAYOUTS.get(k.name);
        if (v !== null && v !== undefined) entries.push([k.name, v]);
      }
      if (list.list_complete || !list.cursor) break;
      cursor = list.cursor;
    }
  }

  const snapshot = {
    version: 1,
    taken_at: new Date().toISOString(),
    unit_minor_per_bls: BLISS_UNIT,
    supply_minor: parseInt(await env.PAYOUTS.get('bliss:supply_minor') || '0', 10),
    distributed_minor: parseInt(await env.PAYOUTS.get('bliss:distributed_minor') || '0', 10),
    genesis_date: await env.PAYOUTS.get('bliss:genesis_date'),
    treasury_usd: parseFloat(await env.PAYOUTS.get('treasury:total_usd') || '0'),
    record_count: entries.length,
    records: entries,
  };

  const key = `ledger-backups/${date}.json`;
  await env.SCENES.put(key, JSON.stringify(snapshot), {
    httpMetadata: { contentType: 'application/json' },
  });
  return { key, record_count: entries.length };
}

/// One-time migration: fold a legacy float `user.bliss_balance` into an
/// opening ledger entry so historical balances survive the format change.
/// Idempotent — the opening entry key is fixed per user.
async function ledgerMigrateUser(env, userId, user) {
  if (user.ledger_migrated) return;
  const legacy = Number(user.bliss_balance) || 0;
  if (legacy > 0) {
    await ledgerAppend(env, userId, {
      amount_minor: toMinor(legacy),
      kind: 'migration_opening',
      ref: 'legacy float balance',
      ts: '1970-01-01T00:00:00.000Z', // sorts first — it is the opening entry
      id: 'opening',
    });
  }
  user.ledger_migrated = true;
  await env.USERS.put(`user:${userId}`, JSON.stringify(user));
}

/// Annual emission rate for a given year since genesis (tail emission).
function blissEmissionRate(yearsSinceGenesis) {
  const halvings = Math.floor(yearsSinceGenesis / BLISS_HALVING_YEARS);
  return Math.max(BLISS_INITIAL_RATE / Math.pow(2, halvings), BLISS_TAIL_RATE);
}

/// Collect one day's contributors from the contrib:{date}:{user} records.
/// Returns { entries: [{userId, score}], totalScore, truncated }.
///
/// Pages through the full keyspace with a cursor — a bare
/// `list({limit:1000})` silently dropped every contributor past the first
/// 1000, which would under-pay them with no error anywhere. `truncated`
/// is only ever true if we hit the hard safety bound below.
async function collectDayScores(env, date) {
  const prefix = `contrib:${date}:`;
  const entries = [];
  let totalScore = 0;
  let cursor = undefined;
  let truncated = false;
  // Safety bound so a pathological keyspace can't run the cron past its
  // CPU limit. 100k contributors/day is far beyond current scale; if this
  // ever trips, the distribution must move to a queue/Durable Object.
  const MAX_KEYS = 100_000;

  while (true) {
    const list = await env.INVENTORY.list({ prefix, limit: 1000, cursor });
    for (const key of list.keys) {
      const data = await env.INVENTORY.get(key.name);
      if (!data) continue;
      const rec = JSON.parse(data);
      // Effort (capped at cosign time) + verified sales value (uncapped,
      // because a real purchase needs no fraud ceiling).
      const score = (rec.total_score || 0) + (rec.value_score || 0);
      if (score <= 0) continue;
      entries.push({ userId: key.name.slice(prefix.length), score });
      totalScore += score;
    }
    if (list.list_complete || !list.cursor) break;
    if (entries.length >= MAX_KEYS) { truncated = true; break; }
    cursor = list.cursor;
  }
  return { entries, totalScore, truncated };
}

/// Daily BLS emission distribution — mints the day's emission and
/// credits each contributor's bliss_balance by score share.
/// Idempotent per date (distribution:{date} record).
async function runDailyDistribution(env) {
  const yesterday = new Date(Date.now() - 86400 * 1000).toISOString().split('T')[0];
  if (await env.PAYOUTS.get(`distribution:${yesterday}`)) return null; // already ran

  // Genesis is stamped by the first distribution ever run.
  let genesis = await env.PAYOUTS.get('bliss:genesis_date');
  if (!genesis) {
    genesis = yesterday;
    await env.PAYOUTS.put('bliss:genesis_date', genesis);
  }
  const years = Math.max(0, Math.floor((Date.parse(yesterday) - Date.parse(genesis)) / (365 * 86400 * 1000)));
  // Supply in integer minor units, migrating off the legacy float key the
  // first time this runs after the ledger change.
  let supplyMinor = parseInt(await env.PAYOUTS.get('bliss:supply_minor') || '0', 10);
  if (!supplyMinor) {
    supplyMinor = toMinor(
      parseFloat(await env.PAYOUTS.get('bliss:current_supply') || String(BLISS_INITIAL_SUPPLY))
    );
    await env.PAYOUTS.put('bliss:supply_minor', String(supplyMinor));
    const legacyDist = parseFloat(await env.PAYOUTS.get('bliss:total_distributed') || '0');
    if (legacyDist > 0 && !(await env.PAYOUTS.get('bliss:distributed_minor'))) {
      await env.PAYOUTS.put('bliss:distributed_minor', String(toMinor(legacyDist)));
    }
  }
  const supply = fromMinor(supplyMinor);
  const rate = blissEmissionRate(years);
  const dailyEmission = supply * rate / 365;

  const { entries, totalScore, truncated } = await collectDayScores(env, yesterday);

  const record = {
    date: yesterday,
    emission_pool: dailyEmission,
    annual_rate: rate,
    supply_before: supply,
    total_score: totalScore,
    contributor_count: entries.length,
    // True only if the 100k safety bound was hit — means some contributors
    // were NOT paid and the distribution needs re-architecting, not a retry.
    truncated,
    // Observability, deliberately NOT a penalty. A per-account share CAP was
    // considered and rejected: it would strip earnings from the single most
    // productive contributor (10 people, one does half the work → capped to
    // a tenth), violating "you earn what you contribute" — and it cannot stop
    // sybil anyway, since N accounts each get their own cap. Per-account abuse
    // is bounded in ABSOLUTE terms by MAX_DAILY_SCORE at cosign time, and the
    // cash-out rail is KYC'd via Stripe Connect. This flag just surfaces
    // unusual concentration for human review.
    concentration_flag: false,
    top_share: 0,
    recipients: [],
    minted: 0,
  };

  if (totalScore > 0 && entries.length > 0) {
    const topScore = entries.reduce((m, e) => Math.max(m, e.score), 0);
    record.top_share = topScore / totalScore;
    // Flag when one account takes >50% of a day that had real breadth.
    record.concentration_flag = entries.length >= 5 && record.top_share > 0.5;
  }

  // Effort gate: mint only the fraction of the day's allowance that the
  // day's ACTUAL work justifies. Without this, a single contributor collected
  // the full emission for one minute of activity.
  const utilization = Math.min(1, totalScore / FULL_DAY_SCORE);
  record.utilization = utilization;
  record.full_day_score = FULL_DAY_SCORE;
  record.emission_ceiling = dailyEmission;

  if (totalScore > 0) {
    // Integer minor units throughout. `floor` on each share guarantees the
    // sum of credits never exceeds the pool (leftover dust stays unminted
    // rather than inflating supply).
    const poolMinor = Math.floor(dailyEmission * BLISS_UNIT * utilization);
    let mintedMinor = 0;
    for (const e of entries) {
      const userData = await env.USERS.get(`user:${e.userId}`);
      if (!userData) continue;
      const user = JSON.parse(userData);
      if (user.banned) continue;
      await ledgerMigrateUser(env, e.userId, user);

      const shareMinor = Math.floor(poolMinor * (e.score / totalScore));
      if (shareMinor <= 0) continue;

      // Deterministic entry id => replaying this cron is a no-op. This
      // REPLACES the old separate `distcredit:` marker: idempotency is now a
      // property of the ledger itself, not a side table.
      const wrote = await ledgerAppend(env, e.userId, {
        amount_minor: shareMinor,
        kind: 'emission',
        ref: yesterday,
        ts: `${yesterday}T00:00:00.000Z`,
        id: 'dist',
      });
      if (!wrote) continue; // already credited on a previous run

      // Rebuild this account's cache/checkpoint from entries — self-heals any
      // drift introduced by concurrent writes.
      await ledgerReconcile(env, e.userId);

      mintedMinor += shareMinor;
      record.recipients.push({
        user_id: e.userId,
        score: e.score,
        bls: fromMinor(shareMinor),
        bls_minor: shareMinor,
      });
    }
    // Supply counters in minor units, advanced by exactly what was credited.
    if (mintedMinor > 0) {
      const supplyMinorNow = parseInt(
        await env.PAYOUTS.get('bliss:supply_minor') || String(toMinor(BLISS_INITIAL_SUPPLY)), 10
      );
      await env.PAYOUTS.put('bliss:supply_minor', String(supplyMinorNow + mintedMinor));
      const distMinorNow = parseInt(await env.PAYOUTS.get('bliss:distributed_minor') || '0', 10);
      await env.PAYOUTS.put('bliss:distributed_minor', String(distMinorNow + mintedMinor));
    }
    record.minted = fromMinor(mintedMinor);
    record.minted_minor = mintedMinor;
  }

  await env.PAYOUTS.put(`distribution:${yesterday}`, JSON.stringify(record), { expirationTtl: 86400 * 365 * 5 });
  return record;
}
// ═══════════════════════════════════════════════════════════════════════════
// MODEL CATALOG — the list of models Workshop offers, recompiled daily
// ═══════════════════════════════════════════════════════════════════════════
//
// The engine used to hardcode its model list in a Rust enum, so every frontier
// release needed a code change, a recompile and a shipped build before anyone
// could pick it. The catalog moves that list into KV: Grok 4.6 recompiles it
// once a day from live search, the engine fetches it, and a new model reaches
// users without a release.
//
// Grok is the only researcher here. We hold no Anthropic or OpenAI key, so a
// discovered id is never confirmed against the provider's own /v1/models — and
// the daily result applies with no human in the loop. The guardrail is
// therefore structural rather than an existence check:
//
//   1. PROVIDER WHITELIST. An entry whose provider is not one of the three in
//      `MODEL_PROVIDERS` is dropped. Grok cannot introduce a fourth vendor
//      into a paid code path by writing one into its JSON.
//   2. SHAPE AND RANGE. Ids, names, prices, token caps and timeouts each have
//      to parse and sit inside a sane range. A $4,000/MTok "bargain" or a
//      600-character display name is a malformed run, not a price cut.
//   3. FLOOR. Every whitelisted provider keeps at least one model, the catalog
//      still names a default and an advisor that exist in it, and the list
//      never shrinks below `MIN_CATALOG_SIZE`. A run that would empty a
//      provider is rejected whole rather than partially applied.
//   4. LAST GOOD WINS. Rejection leaves `catalog:current` exactly as it was and
//      records why in `run:{date}`. A bad night is a no-op, never an outage.
//
// The engine carries its own compiled-in copy of this same seed, so a machine
// that has never reached the network still gets a working picker. The catalog
// widens the list; it is never the only thing standing between a user and a
// model.
//
// KYC is deliberately NOT a consumer of this catalog. `GROK_MODEL` stays a
// pinned const: identity adjudication should not change model underneath
// itself on a cron. The pin is surfaced in the catalog as `pinned_kyc_model`
// so it reads as a decision rather than a forgotten constant.
//
// KV (MODELS namespace):
//   catalog:current          the live catalog — what the engine reads
//   catalog:snapshot:{date}  one snapshot per applied run, for rollback
//   run:{date}               run record: applied/rejected, changes, errors

/// The only vendors a catalog entry may name. This is the whitelist the whole
/// design rests on — everything downstream (which key is required, which
/// client speaks the wire format) is keyed off it, so an unknown provider is
/// not merely unsupported, it is unroutable.
const MODEL_PROVIDERS = Object.freeze({
  anthropic: 'Anthropic',
  xai: 'xAI',
  openai: 'OpenAI',
});

/// Bumped when the catalog's SHAPE changes, so an older engine can tell "I do
/// not understand this document" apart from "this document has new models in
/// it". Engines refuse a schema they were not built for and fall back to their
/// compiled-in seed.
const CATALOG_SCHEMA = 1;

/// Sanity bounds. Deliberately generous — these exist to catch a garbled run,
/// not to encode a pricing opinion that would reject a genuinely expensive
/// new flagship.
const MIN_CATALOG_SIZE = 3;
const MAX_CATALOG_SIZE = 24;
const MAX_PRICE_PER_MTOK = 500;
const MODEL_ID_RE = /^[a-zA-Z0-9][a-zA-Z0-9._:-]{2,63}$/;

/// The starting catalog, and the floor the system falls back to.
///
/// Kept byte-for-byte in sync with `WorkshopModel::SEED` in the engine
/// (crates/engine/src/soul/workshop_model.rs) — the engine ships this exact
/// list compiled in, so the two must not drift.
///
/// Ordered cheapest-first within each provider: the picker renders the array
/// order, and the cheapest option reading first is the contract.
const SEED_CATALOG = {
  schema: CATALOG_SCHEMA,
  version: 1,
  updated_at: '2026-09-07T00:00:00Z',
  source: 'seed',
  default_model: 'claude-sonnet-5',
  advisor_model: 'claude-fable-5-1',
  pinned_kyc_model: GROK_MODEL,
  // Retired id → the model that replaced it. A user whose settings still hold
  // a retired id must be UPGRADED, never silently reassigned to the default:
  // that would move them to another provider, at another price, with no
  // notice. The engine resolves through this map before it gives up.
  aliases: {
    'grok-4.5': 'grok-4.6',
    'claude-fable-5': 'claude-fable-5-1',
  },
  models: [
    {
      id: 'claude-sonnet-5',
      display_name: 'Sonnet 5',
      provider: 'anthropic',
      tagline: 'Balanced speed and depth. The everyday driver.',
      input_price_per_mtok: 3.0,
      output_price_per_mtok: 15.0,
      max_tokens: 16384,
      timeout_secs: 180,
      vision: true,
    },
    {
      id: 'claude-opus-5',
      display_name: 'Opus 5',
      provider: 'anthropic',
      tagline: 'Deeper reasoning for work that has to be right.',
      input_price_per_mtok: 5.0,
      output_price_per_mtok: 25.0,
      max_tokens: 32000,
      timeout_secs: 300,
      vision: true,
    },
    {
      id: 'claude-fable-5-1',
      display_name: 'Fable 5.1',
      provider: 'anthropic',
      tagline: 'Always-on thinking. The advisor on hard calls.',
      input_price_per_mtok: 10.0,
      output_price_per_mtok: 50.0,
      // Fable's thinking is always on and counts toward the same budget, and
      // a turn can run for minutes — hence the headroom on both numbers.
      max_tokens: 32000,
      timeout_secs: 360,
      vision: true,
    },
    {
      id: 'grok-4.6',
      display_name: 'Grok 4.6',
      provider: 'xai',
      tagline: 'Fast and cheap, with live search built in.',
      input_price_per_mtok: 2.0,
      output_price_per_mtok: 6.0,
      max_tokens: 16384,
      timeout_secs: 180,
      vision: true,
    },
    {
      id: 'gpt-6-astra',
      display_name: 'GPT-6 Astra',
      provider: 'openai',
      tagline: 'OpenAI flagship. Long context, agentic reasoning.',
      input_price_per_mtok: 10.0,
      output_price_per_mtok: 50.0,
      max_tokens: 32000,
      timeout_secs: 300,
      vision: true,
    },
  ],
};

/// Ask Grok 4.6, with live search on, for the current best model per vendor.
///
/// The prompt asks for the FLAGSHIP AND THE WORKHORSE rather than "every model
/// you can find": a picker with thirty entries is worse than one with six, and
/// the value of this job is currency, not breadth.
function buildCatalogPrompt(current) {
  const vendors = Object.entries(MODEL_PROVIDERS)
    .map(([id, label]) => `  - ${label} (use provider id "${id}")`)
    .join('\n');

  return `You are compiling the model picker for a professional 3D engine's built-in AI assistant.
Today is ${new Date().toISOString().split('T')[0]}. Use live search — your training data is stale by definition here.

Return the CURRENT, GENERALLY AVAILABLE text models from EXACTLY these vendors:
${vendors}

Per vendor return between 1 and 3 models: the current flagship, the balanced
workhorse, and (only if it genuinely exists) a fast/cheap tier. Do NOT list
deprecated models, previews, research previews, dated snapshot aliases, embedding
models, image models, or audio models. Prefer the stable id a customer would put
in an API "model" field.

This is the catalog in production right now:
${JSON.stringify({ models: current.models.map(m => ({ id: m.id, provider: m.provider, display_name: m.display_name, input_price_per_mtok: m.input_price_per_mtok, output_price_per_mtok: m.output_price_per_mtok })) }, null, 2)}

Rules:
- If a model above is still current, KEEP its id and display_name byte-identical.
- If a model above has been superseded, list the replacement AND record the old
  id in "aliases" mapping old id -> new id, so existing users get upgraded.
- Prices are USD per MILLION tokens, standard tier, no batch or cached discount.
  If you cannot verify a price, keep the price already in the catalog.
- "display_name" is what a user sees in a dropdown: short and human, like
  "Sonnet 5" or "GPT-6 Astra". Never the raw api id. Max 32 characters.
- "tagline" is one short sentence, max 60 characters, saying what the model is
  FOR — the tradeoff a user picks it on. No marketing adjectives.
- "default_model" should be the best all-round value for everyday agentic work.
- "advisor_model" should be the strongest reasoning model available — it is
  consulted on hard architecture calls, not used for every turn.
- "max_tokens" is a per-request output cap: 16384 for standard models, 32000
  for reasoning models whose thinking shares the budget.
- "timeout_secs" between 180 and 360, higher for slower reasoning models.

Reply with ONLY a JSON object, no prose and no code fence:
{
  "default_model": "<id>",
  "advisor_model": "<id>",
  "aliases": { "<retired id>": "<replacement id>" },
  "models": [
    {
      "id": "<api id>",
      "display_name": "<short label>",
      "provider": "anthropic|xai|openai",
      "tagline": "<one short sentence>",
      "input_price_per_mtok": <number>,
      "output_price_per_mtok": <number>,
      "max_tokens": <integer>,
      "timeout_secs": <integer>,
      "vision": <boolean>
    }
  ]
}`;
}

/// Structural validation. Returns `{ ok, catalog, errors, dropped }`.
///
/// Every rejection reason is collected rather than thrown on first sight, so a
/// run record says everything that was wrong with a bad night instead of only
/// the first thing.
function validateCatalog(raw, previous) {
  const errors = [];
  const dropped = [];

  if (!raw || typeof raw !== 'object') {
    return { ok: false, errors: ['response was not a JSON object'], dropped };
  }
  if (!Array.isArray(raw.models)) {
    return { ok: false, errors: ['response had no models array'], dropped };
  }
  if (raw.models.length > MAX_CATALOG_SIZE) {
    return { ok: false, errors: [`${raw.models.length} models exceeds the ${MAX_CATALOG_SIZE} cap`], dropped };
  }

  const seen = new Set();
  const models = [];

  for (const m of raw.models) {
    const id = typeof m?.id === 'string' ? m.id.trim() : '';
    const label = id || '(unnamed entry)';

    if (!MODEL_ID_RE.test(id)) { dropped.push(`${label}: malformed id`); continue; }
    if (seen.has(id)) { dropped.push(`${label}: duplicate id`); continue; }
    // THE whitelist check. Everything downstream keys off provider, so an
    // unrecognised vendor is unroutable, not merely unsupported.
    if (!Object.hasOwn(MODEL_PROVIDERS, m?.provider)) {
      dropped.push(`${label}: provider "${m?.provider}" is not whitelisted`);
      continue;
    }

    const name = typeof m?.display_name === 'string' ? m.display_name.trim() : '';
    if (!name || name.length > 32) { dropped.push(`${label}: display_name missing or too long`); continue; }

    const inPrice = Number(m?.input_price_per_mtok);
    const outPrice = Number(m?.output_price_per_mtok);
    if (!Number.isFinite(inPrice) || inPrice <= 0 || inPrice > MAX_PRICE_PER_MTOK) {
      dropped.push(`${label}: input price ${m?.input_price_per_mtok} out of range`);
      continue;
    }
    if (!Number.isFinite(outPrice) || outPrice <= 0 || outPrice > MAX_PRICE_PER_MTOK) {
      dropped.push(`${label}: output price ${m?.output_price_per_mtok} out of range`);
      continue;
    }

    const maxTokens = Math.trunc(Number(m?.max_tokens));
    const timeout = Math.trunc(Number(m?.timeout_secs));
    if (!Number.isFinite(maxTokens) || maxTokens < 1024 || maxTokens > 200000) {
      dropped.push(`${label}: max_tokens ${m?.max_tokens} out of range`);
      continue;
    }
    if (!Number.isFinite(timeout) || timeout < 30 || timeout > 900) {
      dropped.push(`${label}: timeout_secs ${m?.timeout_secs} out of range`);
      continue;
    }

    const tagline = typeof m?.tagline === 'string' ? m.tagline.trim().slice(0, 60) : '';

    seen.add(id);
    models.push({
      id,
      display_name: name,
      provider: m.provider,
      tagline,
      input_price_per_mtok: inPrice,
      output_price_per_mtok: outPrice,
      max_tokens: maxTokens,
      timeout_secs: timeout,
      vision: m?.vision !== false,
    });
  }

  if (models.length < MIN_CATALOG_SIZE) {
    errors.push(`only ${models.length} valid models survived, need ${MIN_CATALOG_SIZE}`);
  }

  // A run that loses a whole vendor is far more likely to be a bad search than
  // a vendor exiting the market, and the cost of being wrong is asymmetric:
  // every user of that vendor silently loses the model they paid to use.
  for (const [providerId, label] of Object.entries(MODEL_PROVIDERS)) {
    if (!models.some((m) => m.provider === providerId)) {
      errors.push(`no ${label} model survived validation`);
    }
  }

  // Keep only aliases that point at a model we actually kept, so the map can
  // never strand a user on an id that resolves to nothing.
  const aliases = {};
  for (const [from, to] of Object.entries({ ...previous.aliases, ...(raw.aliases || {}) })) {
    if (typeof from === 'string' && typeof to === 'string' && seen.has(to) && !seen.has(from)) {
      aliases[from] = to;
    }
  }

  const defaultModel = seen.has(raw.default_model) ? raw.default_model : previous.default_model;
  const advisorModel = seen.has(raw.advisor_model) ? raw.advisor_model : previous.advisor_model;
  if (!seen.has(defaultModel)) errors.push(`default_model "${defaultModel}" is not in the catalog`);
  if (!seen.has(advisorModel)) errors.push(`advisor_model "${advisorModel}" is not in the catalog`);

  if (errors.length) return { ok: false, errors, dropped };

  // Group by the whitelist's own order, cheapest-first inside each vendor, so
  // the picker's sections are stable run to run rather than reshuffling
  // whenever Grok returns the same models in a different order.
  const providerOrder = Object.keys(MODEL_PROVIDERS);
  models.sort((a, b) =>
    providerOrder.indexOf(a.provider) - providerOrder.indexOf(b.provider) ||
    a.input_price_per_mtok - b.input_price_per_mtok ||
    a.id.localeCompare(b.id));

  return {
    ok: true,
    dropped,
    errors,
    catalog: {
      schema: CATALOG_SCHEMA,
      version: (previous.version || 0) + 1,
      updated_at: new Date().toISOString(),
      source: GROK_MODEL,
      default_model: defaultModel,
      advisor_model: advisorModel,
      pinned_kyc_model: GROK_MODEL,
      aliases,
      models,
    },
  };
}

/// Human-readable diff between two catalogs, for the run record.
function diffCatalogs(before, after) {
  const beforeById = new Map(before.models.map((m) => [m.id, m]));
  const afterById = new Map(after.models.map((m) => [m.id, m]));
  const changes = [];

  for (const [id, m] of afterById) {
    const prev = beforeById.get(id);
    if (!prev) { changes.push(`added ${id} (${m.display_name}, ${MODEL_PROVIDERS[m.provider]})`); continue; }
    if (prev.input_price_per_mtok !== m.input_price_per_mtok || prev.output_price_per_mtok !== m.output_price_per_mtok) {
      changes.push(`repriced ${id}: $${prev.input_price_per_mtok}/$${prev.output_price_per_mtok} -> $${m.input_price_per_mtok}/$${m.output_price_per_mtok}`);
    }
    if (prev.display_name !== m.display_name) changes.push(`renamed ${id}: "${prev.display_name}" -> "${m.display_name}"`);
  }
  for (const id of beforeById.keys()) {
    if (!afterById.has(id)) changes.push(`removed ${id}${after.aliases[id] ? ` (users upgraded to ${after.aliases[id]})` : ''}`);
  }
  if (before.default_model !== after.default_model) changes.push(`default: ${before.default_model} -> ${after.default_model}`);
  if (before.advisor_model !== after.advisor_model) changes.push(`advisor: ${before.advisor_model} -> ${after.advisor_model}`);

  return changes;
}

/// Read the live catalog, falling back to the seed. Never throws: a Workshop
/// that cannot read KV must still get a usable list.
async function readCatalog(env) {
  try {
    const stored = await env.MODELS?.get('catalog:current');
    if (stored) {
      const parsed = JSON.parse(stored);
      if (parsed?.schema === CATALOG_SCHEMA && Array.isArray(parsed.models) && parsed.models.length) {
        return parsed;
      }
      console.error('model catalog: stored copy unusable, serving seed');
    }
  } catch (e) {
    console.error('model catalog: read failed, serving seed:', e.message);
  }
  return SEED_CATALOG;
}

/// The daily job. Returns the run record; never throws into the cron.
async function runModelDiscovery(env) {
  const date = new Date().toISOString().split('T')[0];
  const previous = await readCatalog(env);

  const record = { ran_at: new Date().toISOString(), applied: false, changes: [], dropped: [], errors: [] };

  if (!env.GROK_API_KEY) {
    record.errors.push('GROK_API_KEY not configured');
  } else if (!env.MODELS) {
    record.errors.push('MODELS KV namespace not bound');
  } else {
    try {
      const resp = await grokFetch({
        input: [{ role: 'user', content: buildCatalogPrompt(previous) }],
        // Web search is the entire point: a model released this week is not in
        // any model's weights, including the weights of the model doing the
        // searching. This is the server-side tool form — the older
        // `search_parameters` field was retired on 2026-01-12 and now answers
        // 410 Gone, which would have made this job fail every night while
        // looking like a model that simply never found anything new.
        tools: [{ type: 'web_search' }],
      }, env.GROK_API_KEY);

      if (!resp.ok) {
        record.errors.push(`xAI returned ${resp.status}`);
        console.error('model discovery: xAI error', resp.status, await resp.text());
      } else {
        // Strip a ``` fence before looking for the object. The prompt asks
        // for bare JSON, but a fence is the single most common way a model
        // ignores that, and a fenced reply is otherwise a perfectly good run
        // thrown away.
        const text = extractGrokText(await resp.json()).replace(/```(?:json)?/gi, '');
        const match = text.match(/\{[\s\S]*\}/);
        if (!match) {
          record.errors.push('no JSON object in the response');
        } else {
          let parsed = null;
          try { parsed = JSON.parse(match[0]); }
          catch (e) { record.errors.push(`response was not valid JSON: ${e.message}`); }

          if (parsed) {
            const result = validateCatalog(parsed, previous);
            record.dropped = result.dropped;
            if (!result.ok) {
              record.errors.push(...result.errors);
            } else {
              record.changes = diffCatalogs(previous, result.catalog);
              // Write the snapshot BEFORE it goes live, so a catalog that is
              // serving is always one we can also roll back to.
              await env.MODELS.put(`catalog:snapshot:${date}`, JSON.stringify(result.catalog), { expirationTtl: 86400 * 365 });
              await env.MODELS.put('catalog:current', JSON.stringify(result.catalog));
              record.applied = true;
              record.version = result.catalog.version;
              record.model_count = result.catalog.models.length;
            }
          }
        }
      }
    } catch (e) {
      record.errors.push(`discovery threw: ${e.message}`);
      console.error('model discovery failed:', e);
    }
  }

  // A rejected run is the normal safe path, not an incident — but an
  // unattended job that silently does nothing for a month is, so every run
  // leaves a record whether it applied or not.
  if (!record.applied) {
    record.kept_version = previous.version ?? 0;
    console.error('model discovery: keeping existing catalog —', record.errors.join('; '));
  }
  try { await env.MODELS?.put(`run:${date}`, JSON.stringify(record), { expirationTtl: 86400 * 365 }); }
  catch (e) { console.error('model discovery: could not record run:', e.message); }

  return record;
}

/// GET /api/models/catalog — public. The engine reads this on startup.
///
/// Unauthenticated on purpose: it is a list of public model names and public
/// list prices, it carries nothing about the caller, and gating it would mean
/// a signed-out engine falls back to its compiled-in seed for no benefit.
async function handleModelCatalog(request, env, cors) {
  const catalog = await readCatalog(env);
  return json(catalog, 200, {
    ...cors,
    // Refreshed once a day, so an hour of staleness costs nothing and spares
    // the worker a request per engine launch.
    'Cache-Control': 'public, max-age=3600',
  });
}

/// GET /api/admin/models — run history, so a job that quietly stopped applying
/// is visible instead of being inferred from the catalog standing still.
async function handleAdminModelRuns(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const catalog = await readCatalog(env);
  const list = await env.MODELS?.list({ prefix: 'run:', limit: 30 });
  const runs = [];
  for (const k of (list?.keys || [])) {
    const v = await env.MODELS.get(k.name);
    if (v) runs.push({ date: k.name.slice('run:'.length), ...JSON.parse(v) });
  }
  runs.sort((a, b) => (a.date < b.date ? 1 : -1));

  return json({
    catalog,
    runs,
    last_applied: runs.find((r) => r.applied)?.date || null,
    providers: MODEL_PROVIDERS,
  }, 200, cors);
}

/// POST /api/admin/models/refresh — run discovery now instead of waiting for
/// midnight. Same code path as the cron, so testing it tests the real job.
async function handleAdminModelRefresh(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const record = await runModelDiscovery(env);
  await auditLog(env, 'model_catalog_refresh', adminId, 'catalog:current', {
    applied: record.applied,
    changes: record.changes,
    errors: record.errors,
  });
  return json(record, 200, cors);
}

/// POST /api/admin/models/rollback — restore a dated snapshot.
///
/// The daily job applies with no human gate, so the recovery path has to be
/// one call rather than a hand-written KV write under pressure.
async function handleAdminModelRollback(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const { date } = await request.json().catch(() => ({}));
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date || '')) {
    return json({ error: 'date must be YYYY-MM-DD' }, 400, cors);
  }

  const snapshot = await env.MODELS?.get(`catalog:snapshot:${date}`);
  if (!snapshot) return json({ error: `no snapshot for ${date}` }, 404, cors);

  const parsed = JSON.parse(snapshot);
  const current = await readCatalog(env);
  // Roll forward the version rather than back, so "which catalog is newer" is
  // still answerable by comparing version numbers after a rollback.
  parsed.version = (current.version || 0) + 1;
  parsed.updated_at = new Date().toISOString();
  parsed.source = `rollback:${date}`;
  await env.MODELS.put('catalog:current', JSON.stringify(parsed));

  await auditLog(env, 'model_catalog_rollback', adminId, `catalog:snapshot:${date}`, {
    restored_models: parsed.models.map((m) => m.id),
  });
  return json({ ok: true, restored_from: date, catalog: parsed }, 200, cors);
}


// ═══════════════════════════════════════════════════════════════════════════
// CRON — Daily payout (called by scheduled trigger at UTC midnight)
// ═══════════════════════════════════════════════════════════════════════════

async function runDailyPayout(env) {
  if (!env.STRIPE_SECRET_KEY) return null;

  // Idempotent per score-date — a cron retry or manual re-run can't
  // double-pay a day.
  const scoreDate = new Date(Date.now() - 86400 * 1000).toISOString().split('T')[0];
  if (await env.PAYOUTS.get(`payout:${scoreDate}`)) return null;

  const treasuryUsd = parseFloat(await env.PAYOUTS.get('treasury:total_usd') || '0');
  if (treasuryUsd <= 0) return null;

  // ── Scarcity mode (canonical treasury, bliss-core economics.rs) ──
  let hwm = parseFloat(await env.PAYOUTS.get('treasury:hwm') || '0');
  if (treasuryUsd > hwm) hwm = treasuryUsd;
  const scarce = treasuryUsd <= hwm * TREASURY_SCARCITY_RATIO;
  const dripRate = scarce ? TREASURY_SCARCITY_RATE : TREASURY_DRIP_RATE;
  const dailyDripUsd = treasuryUsd * dripRate;

  if (dailyDripUsd < 0.50) return null; // below Stripe's practical floor

  // ── Same day-score snapshot the BLS emission uses ──
  const yesterday = new Date(Date.now() - 86400 * 1000).toISOString().split('T')[0];
  const { entries, totalScore } = await collectDayScores(env, yesterday);
  if (totalScore <= 0 || entries.length === 0) return null;

  // Resolve users; only Stripe-connected, non-banned users receive USD.
  const contributors = [];
  for (const e of entries) {
    const data = await env.USERS.get(`user:${e.userId}`);
    if (!data) continue;
    const user = JSON.parse(data);
    if (user.banned || !user.stripe_connect_id) continue;
    contributors.push({
      user_id: e.userId, username: user.username,
      score: e.score, connect_id: user.stripe_connect_id,
    });
  }
  if (contributors.length === 0) return null;

  // Top-contributor boost while scarce: the people keeping the system
  // alive are paid aggressively (bliss-core scarcity_top_boost).
  contributors.sort((a, b) => b.score - a.score);
  const topCount = scarce ? Math.max(1, Math.ceil(contributors.length * TREASURY_TOP_FRACTION)) : 0;
  let weightTotal = 0;
  contributors.forEach((c, i) => {
    c.weight = c.score * (i < topCount ? TREASURY_TOP_BOOST : 1.0);
    weightTotal += c.weight;
  });

  let totalPaid = 0;
  const payouts = [];

  for (const c of contributors) {
    const share = c.weight / weightTotal;
    const amountCents = Math.floor(dailyDripUsd * share * 100);
    if (amountCents < 50) continue; // Stripe minimum: $0.50

    try {
      // Deterministic idempotency key: if this cron is retried (or a manual
      // admin run overlaps), Stripe replays the original transfer instead of
      // sending a SECOND real payment. Without it, an error late in the loop
      // re-paid everyone already paid on the next attempt.
      const idemKey = `bliss-payout-${yesterday}-${c.user_id}`;
      const transfer = await stripeRequest('POST', '/transfers', {
        'amount': amountCents.toString(),
        'currency': 'usd',
        'destination': c.connect_id,
        'description': `Bliss daily payout - ${c.username}`,
        'metadata[user_id]': c.user_id,
        'metadata[date]': yesterday,
      }, env, idemKey);

      if (!transfer.error) {
        payouts.push({ user_id: c.user_id, username: c.username, amount_usd: amountCents / 100, transfer_id: transfer.id });
        totalPaid += amountCents / 100;
        // Debit incrementally so an interrupted run leaves the treasury
        // consistent with money that actually left. Re-read each time: a
        // deposit webhook landing mid-loop would otherwise be erased by a
        // stale end-of-loop write.
        const tNow = parseFloat(await env.PAYOUTS.get('treasury:total_usd') || '0');
        await env.PAYOUTS.put('treasury:total_usd', String(Math.max(0, tNow - amountCents / 100)));
        const pNow = parseFloat(await env.PAYOUTS.get('payouts:total_paid') || '0');
        await env.PAYOUTS.put('payouts:total_paid', String(pNow + amountCents / 100));
      }
    } catch (e) { /* skip failed transfer, continue with others */ }
  }

  // Decay the high-water mark toward remaining so a one-time large deposit
  // can't pin the system in scarcity mode forever. Applied HERE — after a
  // payout actually ran — because doing it before the early returns meant
  // every no-op day (empty treasury, sub-$0.50 drip, no eligible
  // contributors) still compounded the decay.
  if (hwm > treasuryUsd) {
    hwm = Math.max(treasuryUsd, hwm * (1 - TREASURY_HWM_DECAY));
  }
  await env.PAYOUTS.put('treasury:hwm', hwm.toString());

  const record = {
    date: new Date().toISOString(), score_date: yesterday,
    drip_usd: dailyDripUsd, scarcity_active: scarce,
    total_paid_usd: totalPaid, contributors_paid: payouts.length,
    treasury_before: treasuryUsd, treasury_after: treasuryUsd - totalPaid,
    payouts,
  };
  await env.PAYOUTS.put(`payout:${yesterday}`, JSON.stringify(record), { expirationTtl: 86400 * 365 * 5 });
  return record;
}

// ═══════════════════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════════════════

async function publicUser(user, env) {
  // Balance comes from the append-only ledger, not the (legacy) float field
  // on the user record. `env` is optional so old call sites degrade to 0
  // rather than throwing.
  const minor = env && user && user.id ? await ledgerBalanceMinor(env, user.id) : 0;
  return {
    id: user.id,
    username: user.username,
    email: user.email || null,
    avatar_url: user.avatar_url || null,
    discord_id: user.discord_id || null,
    bliss_balance: fromMinor(minor),
    bliss_balance_minor: minor,
    // The web app deserializes this into User.ticket_balance. Omitting it
    // meant every /api/auth/me refresh reset the displayed Ticket balance to
    // zero and re-persisted that zero to localStorage.
    ticket_balance: user.ticket_balance || 0,
    created_at: user.created_at,
  };
}

function json(data, status, headers) {
  return new Response(JSON.stringify(data), {
    status, headers: {
      ...headers,
      'Content-Type': 'application/json',
      'Strict-Transport-Security': 'max-age=31536000; includeSubDomains; preload',
      'X-Content-Type-Options': 'nosniff',
      'Referrer-Policy': 'strict-origin-when-cross-origin',
    },
  });
}

// Origins allowed to read cross-origin API responses. The API is bearer-token
// (Authorization header) auth, never cookies, so this list is the browser-facing
// surface only; non-browser clients (engine, updater) send no Origin and are
// unaffected. NEVER pair a reflected/allowlisted origin with
// Access-Control-Allow-Credentials: true.
const ALLOWED_ORIGINS = new Set([
  'https://eustress.dev',
  'https://www.eustress.dev',
  'http://localhost:3000',
  'http://127.0.0.1:3000',
]);

function corsHeaders(request) {
  const origin = request.headers.get('Origin');
  const allow = origin && ALLOWED_ORIGINS.has(origin) ? origin : 'https://eustress.dev';
  return {
    'Access-Control-Allow-Origin': allow,
    'Vary': 'Origin',
    // PUT is listed because four upload routes use it. Their callers today are
    // ureq/reqwest and send no Origin, so this changed nothing in practice, but
    // the first browser upload would have failed preflight for no visible
    // reason. DELETE stays off the list until a route needs it.
    'Access-Control-Allow-Methods': 'GET, POST, PUT, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type, Authorization, X-ID-Type, If-None-Match, X-Eustress-Key',
    'Access-Control-Max-Age': '86400',
  };
}

/// CORS for the PUBLIC, UNAUTHENTICATED ledger transparency reads only.
///
/// The normal `corsHeaders` pins browsers to https://eustress.dev. These
/// ledger endpoints are deliberately world-readable ("anyone can verify the
/// math"), carry no credentials, and expose only aggregate figures plus
/// opaque account UUIDs and amounts that the per-day distribution records
/// already publish. Allowing `*` lets dashboards and third-party auditors
/// read them from a browser.
///
/// NEVER use this for an authenticated route — `/api/ledger/me` and every
/// admin endpoint stay on `corsHeaders` + a bearer check.
function publicCors() {
  return {
    'Access-Control-Allow-Origin': '*',
    'Access-Control-Allow-Methods': 'GET, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type',
    'Access-Control-Max-Age': '86400',
  };
}

function hexEncode(bytes) {
  return [...bytes].map(b => b.toString(16).padStart(2, '0')).join('');
}

function hexDecode(hex) {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2)
    bytes[i / 2] = parseInt(hex.substr(i, 2), 16);
  return bytes;
}

function base64url(str) {
  return btoa(str).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function base64urlDecode(str) {
  const b64 = str.replace(/-/g, '+').replace(/_/g, '/');
  const binary = atob(b64);
  return new Uint8Array([...binary].map(c => c.charCodeAt(0)));
}
