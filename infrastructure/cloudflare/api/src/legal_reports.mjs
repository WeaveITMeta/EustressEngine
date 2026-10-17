// =============================================================================
// Legal reports: the write-up of illegal content for law enforcement, written and queued by the system
// =============================================================================
//
// McKale, 2026-10-15: moderators do not review illegal content. It is removed entirely, sealed (src/evidence_vault.mjs), and a
// report is written up from the case record and the account's verified identity, for legal or police action. The account is
// never frozen or banned for it: the author is told what rule was broken, why, and how to appeal, and keeps everything else.
//
// What is reported, and to whom:
//   csam     18 U.S.C. 2258A requires a report to NCMEC's CyberTipline. Every confident finding (confidence band `high`) is
//            written up; a borderline removal is removed and sealed and never reported.
//   threat   A threat to life. Disclosure in the US is voluntary (18 U.S.C. 2702); in the EU the DSA (Regulation (EU) 2022/2065)
//            Article 18 obliges a hosting provider to notify the Member State's authorities, or Europol when the Member State
//            is unclear. NOT DECIDED: the criteria are built (assessThreat) and the report is written, but it is held and sent
//            to no one while LEGAL_REPORT_THREATS is off, which is the default.
//   scam     A weekly batch of the confirmed scam removals, for submission to the FTC and the FBI's IC3 (weeklyScamBatch).
//   NCII of adults is removed under TAKE IT DOWN (src/takedown.mjs) and sealed; there is no duty to report the uploader, and
//   the person shown decides, so it is never written up here.
//
// Nothing is sent until its sender is switched on, and NCMEC cannot be: Eustress is not registered with NCMEC, so the NCMEC
// sender is a stub that shapes the record and sends nothing (cyberTiplineDraft; the field mapping is TODO at registration).
// Until a sender is on, a report sits in the queue as `ready_to_send` and staff are alerted on the TAKE IT DOWN staff path.
//
// A report is minimised. It carries what the authority needs and nothing more: the category, the detection (the classifier
// ids, their confidence, whether two independent checks agreed) and whether a person at Eustress viewed it, as recorded; the case
// reference, the listing or post, the times; and, from the account's verified identity (KYC_STATUS), the verified name, the
// date of birth, the country, the account id, the email address, and the upload addresses and times the Worker has. Never the
// identity document's image, the selfie, the document number or the identity check's own analysis: those stay sealed in
// KYC storage and are disclosed only under legal process.
//
// Settings (env vars; anything but the exact string "on" is off, so every one is off by default):
//   LEGAL_REPORT_NCMEC                 send CSAM reports to NCMEC. Also needs NCMEC_REGISTRATION (the credential NCMEC issues at
//                                      registration); without it nothing is sent whatever this says.
//   LEGAL_REPORT_EU_EMAIL              email threat reports to the authority addresses in LEGAL_REPORT_EU_AUTHORITIES
//                                      (JSON: { "DE": "address", ..., "EUROPOL": "address" }; none are filled in by default).
//   LEGAL_REPORT_THREATS               write threat reports as sendable. Off: they are held (`held_setting_off`) and go to no one.
//   LEGAL_REPORT_SCAMS                 send the weekly scam batch.
//   LEGAL_REPORT_CSAM_NOTICE_RELEASE   release the author's notice of a CSAM removal without waiting for the report to be sent.
//   FORUM_LEGAL_OWED_SWEEP             let the hourly sweep write the reports forum cases owe (writeOwedForumReports). Turned on
//                                      with the forums; off, the sweep makes no call into the forum code.
//   LISTING_LEGAL_OWED_SWEEP           let the hourly sweep write the reports listing cases owe (writeOwedListingReports, with its
//                                      heal over the quarantined queue). Off, the sweep touches no listing case and no listing
//                                      owed index.
//   Each owed sweep setting also gates the daily re-alert of its own kind's stuck debts: forum stuck debts under
//   FORUM_LEGAL_OWED_SWEEP, listing stuck debts under LISTING_LEGAL_OWED_SWEEP. With both off no stuck mail is re-sent. A
//   listing debt is told to staff once when it is created, whatever the setting (alertReportOwed).
//
// Storage (SOCIAL):
//   legalreport:{id}                            the report
//   legalq:{status}:{id}                        the queue by status
//   legalcase:{kind}:{caseKind}:{caseId}        the report a case already has (one per case and kind)
//   legalalert:{id}:{status}                    a staff alert sent once per report and status
//   scamremoval:{day}:{caseKind}:{caseId}       a confirmed scam removal, for the week's batch
//   scambatch:{weekStart}                       the week's batch, built once
//   legalowed:{caseKind}:{caseId}               a case that owes a report (src/moderation.mjs writeOwedForumReports and
//                                               writeOwedListingReports), retried hourly
//   legalstuck:{caseKind}:{caseId}:{kind}       a report debt of `kind` no retry can pay, waiting for a person (recordStuckReport):
//                                               one record per case and report kind, tied to its debt by `since`
//   legalstuck-cursor:{caseKind}                where the daily stuck re-alert left off (alertStuckReports)
//   legalalert:owed:{caseKind}:{caseId}:{since} the one staff alert of a debt when it was created (alertReportOwed)
//   legalabsent:{accountId}                     when a report first found no user record for the account (two reads at least
//                                               ACCOUNT_ABSENCE_CONFIRM_MS apart before a report says 'deleted')
//   R2 locks/legalcase/{kind}/{caseKind}/{caseId}.json   the report a case is getting, claimed create-if-absent before it is
//                                               written, so two writers (the sweep and a live decision) never write two; the
//                                               save is fenced on the claim's etag, and a later try finding the claim's
//                                               report repairs its case index and its queue key before counting it paid
//   legalunconfirmed:{reportId}                 a CSAM report written with account_status 'unconfirmed', for the confirmation
//
// Report debts. A case that owes a report keeps the debt until it is paid, and nothing clears or drops one silently. What a
// try at the report comes to (reportDebtOutcome): `paid` (written, or the case has one); `settled` (a decision that no report
// is owed: not a reported kind, a borderline finding, the threat criteria not met), closed with its reason recorded and
// audited first; `owed` (unavailable, busy, the sweep's budget, an account absence not yet confirmed), kept and tried again
// every hour; a debt whose tries fail REPORT_DEBT_MAX_TRIES times in a row FOR THE SAME REASON is stuck as `retries_exhausted`
// (a different reason starts the count again), with that reason as `last_reason`, and is still tried once every
// STUCK_RETRY_EVERY_MS: a retry that pays closes it and its stuck record; `stuck` (a defect in the data, such as no account
// for a threat, or any reason not named here), moved to the stuck index, staff alerted at once and then daily (no content,
// account or address in the alert), shown at GET /api/admin/legal/stuck, and cleared only by a person with a rationale,
// audited before anything changes (POST /api/admin/legal/stuck/{caseKind}/{caseId}/clear), or tried again at once by a
// person (POST .../retry, audited the same way). A case keeps one debt per report kind (a forum post can owe a threat report
// and a csam report at once), and one stuck record per kind, tied to its debt by `since`: a record whose debt is paid or
// replaced is closed, a clear names the record it was shown (`report_kind`, `since`) and is refused (409) when the debt it
// would clear is not that one, and a clear removes only a debt that is stuck, never a payable one. The sealed evidence is
// kept whatever the outcome.
//
// NOT DECIDED (counsel): a listing a person RELEASES from its quarantine keeps the csam debt its quarantine owed, and the
// sweep still writes the report (queued ready_to_send, sent by no one while every sender is off). Whether a person's release
// settles the debt is counsel's call; the code does not decide it.
//
// A CSAM report is written when the account is not known (no account id on the case) or was deleted: the duty in 2258A comes
// from knowing of the content, not from knowing who posted it, and a CSAM report is never held on the account's status.
// `who.account_status` says which: 'unknown' (no account id, or no account store to ask) with every field the account would
// give empty; 'unconfirmed' (the store answered that no user record exists, once: one negative read can be a stale one just
// after the account was made), written at once and confirmed by the hourly sweep (confirmUnconfirmedAccounts) through an
// amendment, as 'deleted' on a second read at least ACCOUNT_ABSENCE_CONFIRM_MS later or as 'known' when the record is found;
// 'deleted' with the account id kept, the email empty, and whatever the identity record (KYC_STATUS) still holds (the verified
// name, the date of birth, the country). A threat report waits for the second read instead (account_unconfirmed, owed). A
// read that fails is never reported as a fact: the report is not written, and the debt stays owed for the next try. NEEDS
// LEGAL REVIEW: counsel should confirm this reading.
// =============================================================================

import { assignCaseReference } from './case_reference.mjs';
import { readAccountEmail } from './email_ops.mjs';
import { sendNotice } from './account_notices.mjs';
import { renderEmail, html, paragraph, facts as factRows } from './email_shell.mjs';
import { isAdmin } from './admin_check.mjs';
import { writeAuditRecord } from './evidence_vault.mjs';
import { staffQueueState } from './staff_alerts.mjs';

export const REPORT_VERSION = 1;
export const REPORT_KINDS = Object.freeze(['csam', 'threat', 'scam_batch']);
export const REPORT_STATUSES = Object.freeze(['ready_to_send', 'held_setting_off', 'send_failed', 'sent']);
// Whether a person at Eustress viewed the content is stated as it was recorded, never assumed: courts have treated a
// government viewing of material the provider never viewed as a search, so a report that says staff viewed content they did
// not would mislead the authority it goes to. A report says "viewed" only when the detection carries a recorded view (who,
// when, how: src/moderation.mjs recordForumView, checked against the evidence vault's read receipt). A person's decision made
// from the case record alone is a person's finding and is still "Not viewed".
/// The statement every automated report carries.
export const NOT_VIEWED = 'Not viewed by Eustress staff. The content was removed and this report written by automated detection; the content is sealed and was not opened.';
/// The statement of a person's finding made without opening the content.
export const NOT_VIEWED_PERSON_DECIDED = 'Not viewed by Eustress staff. A Eustress staff member confirmed this finding from the case record (the reports, their categories and the detection) without opening the content; the content is sealed and was not opened.';
/// The statement of a finding by a person who read the content through the audited evidence read.
export const VIEWED_BY_STAFF = 'Viewed by Eustress staff. A Eustress staff member read the sealed content through the audited evidence read before confirming this finding.';

/// The recorded view a detection carries, cleaned, or null: `{ by, at, how }` with each present. Only a view on record makes a
/// report say the content was viewed; `viewed_by_staff: true` without one is reported as not viewed.
export function recordedViewOf(detection) {
  const v = detection?.viewed_by_staff === true ? detection.view : null;
  const by = clean(v?.by, 64);
  const at = clean(v?.at, 40);
  const how = clean(v?.how, 40);
  return by && at && how && Number.isFinite(Date.parse(at)) ? { by, at, how, read_at: clean(v.read_at, 40) } : null;
}
const ID_DOCUMENTS_LINE = 'The identity document images and the selfie are held sealed in Eustress identity storage and are disclosed only under legal process.';
// Whether an address is known is stated per sealed item, never for the report as a whole: `who.upload_ip_unknown_items` lists
// `{ item_id, upload_ip: null, reason: 'not_known' }` for each sealed content item no address is known for (for a forum post,
// each sealed text item, `post-text-...`, and a first post's sealed title, `post-title-...`, which no address is ever paired
// with; the items that carry addresses are not content). An address known for an item is in `who.upload_ips` with that
// `item_id`.
const FORUM_TEXT_ITEM = /^(?:post-text(?:-[0-9a-f]{32})?|post-title-[0-9a-f]{32})$/;
export function uploadIpUnknownItems(caseKind, items, ips) {
  const known = new Set((ips || []).map((x) => x?.item_id).filter(Boolean));
  const content = [...new Set((items || []).filter((i) => typeof i === 'string' && (caseKind !== 'forum' || FORUM_TEXT_ITEM.test(i))))];
  return content.filter((i) => !known.has(i)).map((item_id) => ({ item_id, upload_ip: null, reason: 'not_known' }));
}

export const SETTING_NAMES = Object.freeze(['LEGAL_REPORT_NCMEC', 'LEGAL_REPORT_EU_EMAIL', 'LEGAL_REPORT_THREATS', 'LEGAL_REPORT_SCAMS', 'LEGAL_REPORT_CSAM_NOTICE_RELEASE', 'FORUM_LEGAL_OWED_SWEEP', 'LISTING_LEGAL_OWED_SWEEP']);

/// The settings as the code reads them. Only the exact string "on" turns one on.
export function legalSettingsFrom(env) {
  const on = (name) => env?.[name] === 'on';
  let authorities = {};
  try {
    const parsed = typeof env?.LEGAL_REPORT_EU_AUTHORITIES === 'string' && env.LEGAL_REPORT_EU_AUTHORITIES.trim() ? JSON.parse(env.LEGAL_REPORT_EU_AUTHORITIES) : {};
    for (const [k, v] of Object.entries(parsed || {})) if (/^[A-Z]{2}$|^EUROPOL$/.test(k) && typeof v === 'string' && EMAIL.test(v.trim())) authorities[k] = v.trim();
  } catch {
    console.error('legal reports: LEGAL_REPORT_EU_AUTHORITIES is not valid JSON; no authority address is configured');
    authorities = {};
  }
  return {
    ncmec: on('LEGAL_REPORT_NCMEC'),
    ncmec_registered: typeof env?.NCMEC_REGISTRATION === 'string' && env.NCMEC_REGISTRATION.trim().length > 0,
    eu_email: on('LEGAL_REPORT_EU_EMAIL'),
    eu_authorities: authorities,
    threats: on('LEGAL_REPORT_THREATS'),
    scams: on('LEGAL_REPORT_SCAMS'),
    csam_notice_release: on('LEGAL_REPORT_CSAM_NOTICE_RELEASE'),
    forum_owed_sweep: on('FORUM_LEGAL_OWED_SWEEP'),
    listing_owed_sweep: on('LISTING_LEGAL_OWED_SWEEP'),
  };
}

// -----------------------------------------------------------------------------
// Routing
// -----------------------------------------------------------------------------
// NEEDS LEGAL REVIEW before any sender is switched on. Which authority a report goes to is decided by this table and nothing
// else (routeFor reads it), so counsel can change it as data. The country is the reported account's verified KYC country.
// Points counsel must confirm: CSAM goes to NCMEC whatever the account's country, because 2258A binds Eustress as a US
// provider; DSA Article 18's "Member State concerned" is read here as the suspect's country of residence (the Article also
// names where the offence took place and where the victim is); Eustress has no EU establishment or legal representative, so
// an unclear Member State goes to Europol; the EEA states (Iceland, Liechtenstein, Norway) are not treated as members; a US
// threat has no destination because US disclosure is voluntary and McKale has not decided it.

export const EU_MEMBER_STATES = Object.freeze(['AT', 'BE', 'BG', 'HR', 'CY', 'CZ', 'DK', 'EE', 'FI', 'FR', 'DE', 'GR', 'HU', 'IE', 'IT', 'LV', 'LT', 'LU', 'MT', 'NL', 'PL', 'PT', 'RO', 'SK', 'SI', 'ES', 'SE']);

export const ROUTING = Object.freeze({
  csam: Object.freeze({
    any: Object.freeze({ authority: 'NCMEC', channel: 'ncmec_cybertipline', basis: '18 U.S.C. 2258A, as amended by the REPORT Act (Public Law 118-59)' }),
  }),
  threat: Object.freeze({
    eu_member: Object.freeze({ authority: 'MEMBER_STATE', channel: 'authority_email', basis: 'Regulation (EU) 2022/2065 (Digital Services Act) Article 18(1)' }),
    unknown: Object.freeze({ authority: 'EUROPOL', channel: 'authority_email', basis: 'Regulation (EU) 2022/2065 (Digital Services Act) Article 18(2): the Member State concerned is unclear' }),
    US: Object.freeze({ authority: null, channel: null, basis: '18 U.S.C. 2702(b)(8): emergency disclosure is voluntary; not decided' }),
    other: Object.freeze({ authority: null, channel: null, basis: 'no reporting duty identified for this country; not decided' }),
  }),
  scam: Object.freeze({
    any: Object.freeze({ authority: 'FTC_IC3', channel: 'ftc_ic3_batch', basis: 'voluntary submission to the FTC (ReportFraud) and the FBI Internet Crime Complaint Center' }),
  }),
});

/// The destination of a report of `kind` ('csam' | 'threat' | 'scam') for an account in `country` (ISO 3166-1 alpha-2, or
/// null when it is unknown): `{ authority, channel, basis, country, member_state? }`, or null for a kind with no route.
export function routeFor(kind, country) {
  const table = ROUTING[kind];
  if (!table) return null;
  const c = typeof country === 'string' && /^[A-Za-z]{2}$/.test(country.trim()) ? country.trim().toUpperCase() : null;
  if (table.any) return { ...table.any, country: c };
  if (!c) return { ...table.unknown, country: null };
  if (EU_MEMBER_STATES.includes(c)) return { ...table.eu_member, authority: `MEMBER_STATE:${c}`, member_state: c, country: c };
  return { ...(table[c] || table.other), country: c };
}

// -----------------------------------------------------------------------------
// Threats: built, and switched off
// -----------------------------------------------------------------------------
// A threat is reportable only when it is specific (a named target, a place or a time) AND two independent checks agree that it
// is a threat at high confidence. Religious or political speech, criticism, hyperbole and jokes are never reported, and an
// exclusion no check has answered counts as present, so a missing answer can only stop a report. Only content signals count:
// a check whose kind is not `classifier` (member reports, their number, a person's flag) is ignored. assessThreat reads the
// `checks` and `content` it is given and nothing else: no attribute of the user (religion, politics, nationality, citizenship,
// country, or anything else about the account) is an input, so the standard is the same for every user.

export const THREAT_MIN_CONFIDENCE = 0.85;
export const THREAT_SPECIFICS = Object.freeze(['named_target', 'place', 'time']);
export const THREAT_EXCLUSIONS = Object.freeze(['religious_speech', 'political_speech', 'criticism', 'hyperbole', 'joke']);

/// The threat decision over content signals only. `checks`: `[{ kind: 'classifier', source, category, confidence }]`, one per
/// independent check (`source` names the model or system; two checks from one source are one check). `content`: the content
/// signals, `{ named_target, place, time, religious_speech, political_speech, criticism, hyperbole, joke }`, each true or false.
/// Returns `{ reportable, two_agree, sources, specific, excluded, unanswered, reasons }`.
export function assessThreat(input) {
  const checks = Array.isArray(input?.checks) ? input.checks : [];
  const content = input?.content && typeof input.content === 'object' ? input.content : {};
  const agreeing = checks.filter((c) => c && c.kind === 'classifier' && c.category === 'threat' && typeof c.source === 'string' && c.source
    && typeof c.confidence === 'number' && c.confidence >= THREAT_MIN_CONFIDENCE && c.confidence <= 1);
  const sources = [...new Set(agreeing.map((c) => c.source))].sort();
  const two_agree = sources.length >= 2;
  const specific = THREAT_SPECIFICS.filter((s) => content[s] === true);
  const excluded = THREAT_EXCLUSIONS.filter((s) => content[s] === true);
  const unanswered = THREAT_EXCLUSIONS.filter((s) => content[s] !== true && content[s] !== false);
  const reasons = [];
  if (!two_agree) reasons.push('two_independent_checks_must_agree');
  if (!specific.length) reasons.push('not_specific');
  if (excluded.length) reasons.push('excluded_speech');
  if (unanswered.length) reasons.push('exclusion_not_answered');
  return { reportable: reasons.length === 0, two_agree, sources, specific, excluded, unanswered, reasons };
}

// -----------------------------------------------------------------------------
// The reported account, minimised
// -----------------------------------------------------------------------------

const EMAIL = /^[^\s@<>,;"']{1,64}@[^\s@<>,;"']{1,190}\.[^\s@<>,;"']{2,}$/;
const clean = (v, n = 200) => (typeof v === 'string' ? v.replace(/[\x00-\x1f\x7f]+/g, ' ').trim().slice(0, n) || null : null);
const parse = (raw) => { try { return raw ? JSON.parse(raw) : null; } catch { return null; } };

/// The reported account as a report carries it, built field by field from the user record and the identity record. Only
/// these fields; never the document images, the selfie, their storage keys, the document number or the check's analysis.
/// A read of the account store that failed. writeLegalReport answers 'unavailable' (owed), so the report is written on a later
/// try from what the store says then, never from a guess.
export class AccountReadError extends Error {
  constructor(what) { super(`the ${what} could not be read`); this.name = 'AccountReadError'; }
}
const readOrThrow = async (binding, key, what) => {
  if (!binding) return null;
  try { return await binding.get(key); } catch { throw new AccountReadError(what); }
};

export async function reportedAccount(env, accountId) {
  const user = parse(await readOrThrow(env.USERS, `user:${accountId}`, 'user record'));
  const kyc = parse(await readOrThrow(env.KYC_STATUS, `kyc-${accountId}-front`, 'identity record'));
  const verified = !!kyc && (kyc.status === 'verified' || kyc.status === 'linked');
  let dob = null;
  if (verified) {
    const { resolveDob } = await import('./adult_content.mjs');
    dob = clean(resolveDob(kyc, user).dob, 10);
  }
  const country = clean(kyc?.jurisdiction || kyc?.iso2 || null, 2);
  let email = null;
  try { email = clean(await readAccountEmail(env, user ? { ...user, id: user.id || accountId } : null), 254); } catch { throw new AccountReadError('account email'); }
  return {
    account_id: accountId,
    identity_verified: verified,
    verified_name: verified ? clean(kyc.ocr_name) : null,
    date_of_birth: dob,
    country: country && /^[A-Za-z]{2}$/.test(country) ? country.toUpperCase() : null,
    email: email && EMAIL.test(email) ? email : null,
    identity_documents: ID_DOCUMENTS_LINE,
  };
}

// The reported account when none is known: every field the account would give is empty.
function unknownAccount() {
  return { account_id: null, identity_verified: false, verified_name: null, date_of_birth: null, country: null, email: null, identity_documents: ID_DOCUMENTS_LINE };
}

// -----------------------------------------------------------------------------
// Account takeover
// -----------------------------------------------------------------------------
// Whether the upload came from someone who had just taken the account: a credential change (the identity key replaced or
// rotated, a passkey added, the email address verified) in the 72 hours before it, or an account younger than that. The
// Worker keeps no sign-in history and no per-account address or device history, so a new device or a new address cannot be
// seen: the report says so instead of guessing.

export const TAKEOVER_WINDOW_HOURS = 72;
export const NO_LOGIN_HISTORY = 'The Worker keeps no sign-in history and no per-account address or device history, so whether the upload came from a new device or a new address cannot be checked. Credential changes come from the account\'s own record.';

async function defaultAccountState(env, accountId) {
  if (!env?.ACCOUNT_AUTH) return null;
  const { accountCall } = await import('./account_auth.mjs');
  const state = await accountCall(env, accountId, 'state');
  const passkeys = await accountCall(env, accountId, 'passkey_list');
  return state?.ok ? { ...state, passkeys: Array.isArray(passkeys?.passkeys) ? passkeys.passkeys : [] } : null;
}

// The takeover check of a report with no known account: nothing could be read, and it says so.
function noAccountTakeover() {
  return {
    checked_at: new Date().toISOString(), window_hours: TAKEOVER_WINDOW_HOURS, flag: false, credential_change_before_upload: false, credential_changes: [],
    account_age_hours: null, new_account: null, new_device: null, new_ip: null, login_history_kept: false, account_state_read: false,
    note: 'No account is known for this upload, so no credential change could be checked.',
  };
}

/// `{ flag, credential_change_before_upload, credential_changes, account_age_hours, new_account, new_device, new_ip,
/// login_history_kept, note, account_state_read, window_hours }`. `deps.accountState(env, accountId)` replaces the account
/// object's read (tests); it answers the object's public state with `passkeys`.
export async function takeoverCheck(env, accountId, uploadedAt, deps = {}) {
  const upload = Date.parse(uploadedAt || '') || Date.now();
  const windowMs = TAKEOVER_WINDOW_HOURS * 3600 * 1000;
  let state = null;
  try { state = await (deps.accountState || defaultAccountState)(env, accountId); } catch { state = null; }
  const changes = [];
  const add = (what, at) => { const t = Date.parse(at || ''); if (Number.isFinite(t)) changes.push({ what, at: new Date(t).toISOString() }); };
  if (state) {
    add('identity_key_changed', state.key_changed_at);
    for (const r of state.retired || []) add(`identity_key_retired_${clean(r.how, 40) || 'unknown'}`, r.at);
    add('email_verified', state.email_verified_at);
    for (const p of state.passkeys || []) add('passkey_added', p.created_at);
  }
  const recent = changes.filter((c) => { const t = Date.parse(c.at); return t <= upload && upload - t <= windowMs; });
  const user = parse(await env.USERS?.get(`user:${accountId}`).catch(() => null));
  const created = Date.parse(user?.created_at || '');
  const ageHours = Number.isFinite(created) ? Math.max(0, Math.floor((upload - created) / 3600000)) : null;
  return {
    checked_at: new Date().toISOString(),
    window_hours: TAKEOVER_WINDOW_HOURS,
    flag: recent.length > 0,
    credential_change_before_upload: recent.length > 0,
    credential_changes: recent.sort((a, b) => (a.at < b.at ? -1 : 1)),
    account_age_hours: ageHours,
    new_account: ageHours !== null && ageHours < TAKEOVER_WINDOW_HOURS,
    new_device: null,
    new_ip: null,
    login_history_kept: false,
    account_state_read: !!state,
    note: NO_LOGIN_HISTORY,
  };
}

// -----------------------------------------------------------------------------
// The record and its queue
// -----------------------------------------------------------------------------

const reportKey = (id) => `legalreport:${id}`;
const legalQueueKey = (status, id) => `legalq:${status}:${id}`;
const caseIndexKey = (kind, caseKind, caseId) => `legalcase:${kind}:${caseKind}:${caseId}`;
const alertKey = (id, status) => `legalalert:${id}:${status}`;

export async function loadLegalReport(env, id) {
  if (typeof id !== 'string' || !/^[0-9a-f-]{36}$/.test(id)) return null;
  return parse(await env.SOCIAL.get(reportKey(id)));
}

async function saveLegalReport(env, rep, previousStatus = null) {
  rep.updated_at = new Date().toISOString();
  await env.SOCIAL.put(reportKey(rep.id), JSON.stringify(rep));
  if (previousStatus && previousStatus !== rep.status) await env.SOCIAL.delete(legalQueueKey(previousStatus, rep.id)).catch(() => {});
  await env.SOCIAL.put(legalQueueKey(rep.status, rep.id), rep.updated_at);
}

const checkOf = (c) => ({
  kind: clean(c?.kind, 40), source: clean(c?.source, 120), category: clean(c?.category, 60),
  confidence: typeof c?.confidence === 'number' && c.confidence >= 0 && c.confidence <= 1 ? Math.round(c.confidence * 1000) / 1000 : null,
  band: c?.band === 'high' || c?.band === 'borderline' ? c.band : null,
});

// The upload addresses a report carries: `[{ ip, at, item_id }]`, each address with the time it was used and the text written
// from it, as the caller recorded them. `at: null` is "time not recorded": a report never gives an address a time it was not
// used at. At most 20, each (ip, at, item_id) once.
const uploadIpKey = (x) => `${x.ip}|${x.at || ''}|${x.item_id || ''}`;
function uploadIpsOf(list) {
  const seen = new Set();
  const out = [];
  for (const x of Array.isArray(list) ? list : []) {
    const e = { ip: clean(x?.ip, 64), at: clean(x?.at, 40), item_id: clean(x?.item_id, 64) };
    if (!e.ip || seen.has(uploadIpKey(e))) continue;
    seen.add(uploadIpKey(e));
    out.push(e);
    if (out.length >= 20) break;
  }
  return out;
}

/// Whether a finding is reported: only a confident one. Exported so every caller asks the same question.
export const reportableBand = (detection) => detection?.band === 'high';

/// A first absence of a user record is confirmed this long after it, on a later try, before a report says 'deleted'.
export const ACCOUNT_ABSENCE_CONFIRM_MS = 60_000;
const absentKey = (accountId) => `legalabsent:${accountId}`;
/// A claim on a case's report whose report never appeared (its writer died) is taken over after this long.
export const REPORT_CLAIM_MS = 5 * 60_000;
export const reportClaimKey = (kind, caseKind, caseId) => `locks/legalcase/${kind}/${caseKind}/${caseId}.json`;

// Claims the case's report for `id` (R2, create-if-absent): `{}` when this writer has it; `{ written: false, reason }` when
// another has: 'already_reported' (its report exists) or 'busy' (it is being written: owed, tried again). With no R2 binding
// there is nothing to claim with and the KV index alone stands.
async function claimReport(env, finding, id, nowMs) {
  if (!env.SCENES?.put) return {};
  const key = reportClaimKey(finding.kind, finding.caseKind, finding.caseId);
  const body = JSON.stringify({ id, at: new Date(nowMs).toISOString() });
  const meta = { httpMetadata: { contentType: 'application/json' } };
  const mine = await env.SCENES.put(key, body, { ...meta, onlyIf: { etagDoesNotMatch: '*' } });
  if (mine) return { etag: mine.etag || null };
  const held = await env.SCENES.get(key);
  const claim = held ? parse(await held.text()) : null;
  if (claim?.id) {
    const rep = await loadLegalReport(env, claim.id);
    if (rep) return { written: false, reason: 'already_reported', id: rep.id, status: rep.status };
  }
  if (held && nowMs - (Date.parse(claim?.at || '') || 0) >= REPORT_CLAIM_MS) {
    const taken = await env.SCENES.put(key, body, { ...meta, onlyIf: { etagMatches: held.etag } });
    if (taken) return { etag: taken.etag || null };
  }
  return { written: false, reason: 'busy' };
}

// The fence: the claim is put again on the etag this writer holds (compare-and-swap); false when another writer has it now.
async function refreshClaim(env, claimed) {
  const held = await env.SCENES.get(claimed.key);
  if (!held || held.etag !== claimed.etag || parse(await held.text())?.id !== claimed.id) return false;
  const again = await env.SCENES.put(claimed.key, JSON.stringify({ id: claimed.id, at: new Date().toISOString() }), { httpMetadata: { contentType: 'application/json' }, onlyIf: { etagMatches: claimed.etag } });
  if (!again) return false;
  claimed.etag = again.etag || claimed.etag;
  return true;
}

const unconfirmedKey = (id) => `legalunconfirmed:${id}`;

// A try that finds the case's report already written (its claim or its case index names it) makes sure it is recorded as a
// report is (B-1, R-1): the report is this case's (its kind, case kind and case id: a claim or an index that names another
// case's report pays nothing, and the debt is stuck for a person, N-4), the case index names it, and its queue key is there
// for its CURRENT status (read again right before, unless sent), so the hourly sweep dispatches it. The repair never
// dispatches or saves the report itself: a send between its reads cannot be repeated from a stale copy (D4-2). Only then is
// it answered already_reported (paid); a repair that fails is answered unavailable, so the debt stays owed.
async function repairClaimedReport(env, claim, finding, deps) {
  void deps;
  try {
    const rep = await loadLegalReport(env, claim.id);
    if (!rep) return { written: false, reason: 'busy' };
    if (rep.kind !== finding.kind || rep.when_where?.case_kind !== finding.caseKind || rep.when_where?.case_id !== finding.caseId) {
      console.error('legal reports: a claim or a case index names another case\'s report');
      return { written: false, reason: 'claim_mismatch' };
    }
    const idx = caseIndexKey(finding.kind, finding.caseKind, finding.caseId);
    if ((await env.SOCIAL.get(idx)) !== rep.id) await env.SOCIAL.put(idx, rep.id);
    // The queue key first, then the report as it is now: a send between the two reads is seen, and nothing is put back for it.
    let now = rep;
    let requeued = false;
    if (rep.status !== 'sent' && !(await env.SOCIAL.get(legalQueueKey(rep.status, rep.id)))) {
      now = (await loadLegalReport(env, rep.id)) || rep;
      if (now.status !== 'sent' && !(await env.SOCIAL.get(legalQueueKey(now.status, now.id)))) {
        await env.SOCIAL.put(legalQueueKey(now.status, now.id), now.updated_at || new Date().toISOString());
        requeued = true;
      }
    }
    return { written: false, reason: 'already_reported', id: rep.id, status: now.status, ...(requeued ? { repaired: true } : {}) };
  } catch (e) {
    console.error('legal reports: the written report was not repaired:', e?.name || 'Error');
    return { written: false, reason: 'unavailable' };
  }
}

/// Writes the report of one illegal finding and queues it. Never throws. `finding`:
///   kind        'csam' | 'threat'
///   caseKind, caseId, caseRef?   the case (src/case_reference.mjs)
///   category, article            the finding ('csam', 'threat', ...) and its Code article ('A1', ...)
///   subject     `{ kind: 'listing', sim_id }` or `{ kind: 'forum_post', post_id, thread_id }`
///   detection   `{ source, checks: [{ kind, source, category, confidence, band }], band, viewed_by_staff, view? }`; only band
///               'high' is reported. The report says the content was viewed only with a recorded `view` (recordedViewOf).
///   content     a threat's content signals (assessThreat)
///   uploader    `{ account_id, uploaded_at, upload_ips: [{ ip, at }] }`
///   detected_at, removed_at, evidence `{ case_ref, items, sealed }`
/// Returns `{ written, id?, status?, reason?, assessment? }`. A second finding on a case already reported returns the report
/// it has. `deps.accountState` replaces the account object's read (takeoverCheck).
export async function writeLegalReport(env, finding, deps = {}) {
  let claimed = null;
  try {
    if (!finding || !['csam', 'threat'].includes(finding.kind)) return { written: false, reason: 'not_a_reported_kind' };
    if (!reportableBand(finding.detection)) return { written: false, reason: 'borderline_never_reported' };
    let assessment = null;
    if (finding.kind === 'threat') {
      assessment = assessThreat({ checks: finding.detection.checks, content: finding.content });
      if (!assessment.reportable) return { written: false, reason: 'threat_criteria_not_met', assessment };
    }
    const accountId = finding.uploader?.account_id;
    const accountKnown = typeof accountId === 'string' && /^[A-Za-z0-9_.:-]{1,128}$/.test(accountId);
    // A CSAM report does not wait on the account: it is written with the account marked unknown (see the header).
    if (!accountKnown && finding.kind !== 'csam') return { written: false, reason: 'no_account' };
    const idx = caseIndexKey(finding.kind, finding.caseKind, finding.caseId);
    const existing = await env.SOCIAL.get(idx);
    if (existing) { const rep = await loadLegalReport(env, existing); if (rep) return await repairClaimedReport(env, { id: rep.id }, finding, deps); }
    const nowMs = typeof deps.now === 'function' ? deps.now() : Date.now();
    // An account id with no user record is 'deleted' only once two reads, ACCOUNT_ABSENCE_CONFIRM_MS apart, both found none. A
    // CSAM report is not held for it (R-3): it is written now, 'unconfirmed', and confirmed later by an amendment. A threat waits.
    let absence = 'none';
    if (accountKnown && env.USERS) {
      const present = !!(await readOrThrow(env.USERS, `user:${accountId}`, 'user record'));
      const firstRaw = await env.SOCIAL.get(absentKey(accountId));
      const first = Date.parse(firstRaw || '');
      if (present) {
        if (firstRaw) await env.SOCIAL.delete(absentKey(accountId)).catch(() => {});
      } else if (!Number.isFinite(first)) {
        await env.SOCIAL.put(absentKey(accountId), new Date(nowMs).toISOString(), { expirationTtl: 2 * 86400 });
        absence = 'unconfirmed';
      } else {
        absence = nowMs - first < ACCOUNT_ABSENCE_CONFIRM_MS ? 'unconfirmed' : 'confirmed';
      }
      if (absence === 'unconfirmed' && finding.kind !== 'csam') return { written: false, reason: 'account_unconfirmed' };
    }
    // The case's report is claimed before it is written (R2, create-if-absent): two writers of one case's report (the hourly
    // sweep and a live decision) never both write one. A claim whose report never appeared is taken over after REPORT_CLAIM_MS.
    const id = crypto.randomUUID();
    const claim = await claimReport(env, finding, id, nowMs);
    if (claim.reason === 'already_reported') return await repairClaimedReport(env, claim, finding, deps);
    if (claim.reason) return claim;
    claimed = { key: reportClaimKey(finding.kind, finding.caseKind, finding.caseId), id, etag: claim.etag || null };

    const settings = legalSettingsFrom(env);
    let reference = finding.caseRef || null;
    if (!reference) { try { reference = await assignCaseReference(env, finding.caseKind, finding.caseId); } catch { reference = null; } }
    const who = accountKnown ? await reportedAccount(env, accountId) : unknownAccount();
    // Whether the account is known: 'known', 'deleted' (the store answered that the id has no user record), or 'unknown' (no
    // id, or no store to ask). A read that fails throws (AccountReadError): the report is owed, never written on a guess.
    const accountStatus = !accountKnown || !env.USERS ? 'unknown' : absence === 'unconfirmed' ? 'unconfirmed' : (await readOrThrow(env.USERS, `user:${accountId}`, 'user record')) ? 'known' : 'deleted';
    const ips = uploadIpsOf(finding.uploader?.upload_ips);
    const view = recordedViewOf(finding.detection);
    const viewed = !!view;
    const now = new Date().toISOString();
    const rep = {
      v: REPORT_VERSION, id, kind: finding.kind, status: finding.kind === 'threat' && !settings.threats ? 'held_setting_off' : 'ready_to_send',
      created_at: now, updated_at: now,
      route: routeFor(finding.kind, who.country),
      what: {
        category: clean(finding.category, 60), article: clean(finding.article, 8),
        detection: { source: clean(finding.detection.source, 60), checks: (finding.detection.checks || []).slice(0, 8).map(checkOf), band: 'high', two_agree: new Set((finding.detection.checks || []).filter((c) => c?.kind === 'classifier' && c?.band === 'high').map((c) => c.source)).size >= 2 },
        viewed_by_staff: viewed,
        view,
        statement: viewed ? VIEWED_BY_STAFF : finding.detection.source === 'person' ? NOT_VIEWED_PERSON_DECIDED : NOT_VIEWED,
        ...(assessment ? { threat: { specific: assessment.specific, sources: assessment.sources } } : {}),
      },
      when_where: {
        case_reference: reference, case_kind: finding.caseKind, case_id: finding.caseId, subject: finding.subject || null,
        uploaded_at: clean(finding.uploader?.uploaded_at, 40), detected_at: clean(finding.detected_at, 40) || now, removed_at: clean(finding.removed_at, 40) || now,
      },
      who: { ...who, account_status: accountStatus, upload_ips: ips, upload_ip_unknown_items: uploadIpUnknownItems(finding.caseKind, (finding.evidence?.items || []).slice(0, 64), ips) },
      takeover: accountKnown ? await takeoverCheck(env, accountId, finding.uploader?.uploaded_at, deps) : noAccountTakeover(),
      evidence: { case_ref: finding.evidence?.case_ref || reference, items: (finding.evidence?.items || []).slice(0, 64), sealed: finding.evidence?.sealed === true },
      // CSAM only: the author's notice waits for this report to be sent (or for a person, or the setting, to release it).
      // A forum post's author is told at once with one generic entry (src/case_history.mjs), so nothing is withheld for it.
      notice: finding.kind === 'csam' && finding.subject?.kind !== 'forum_post' ? { withheld: true, released_at: null, released_by: null } : null,
      attempts: 0, last_error: null, sent_at: null, history: [{ at: now, event: 'written' }],
    };
    // Fenced: saved only while the claim is still this writer's (a writer that stalled past REPORT_CLAIM_MS and was taken over
    // saves nothing, and the one that took over writes the report).
    if (claimed.etag && !(await refreshClaim(env, claimed))) { claimed = null; return { written: false, reason: 'busy' }; }
    // The confirmation's key before the report (D4-3): one that cannot be written leaves the report unwritten and the debt owed.
    if (accountStatus === 'unconfirmed') await env.SOCIAL.put(unconfirmedKey(rep.id), new Date(nowMs).toISOString());
    await saveLegalReport(env, rep);
    await env.SOCIAL.put(idx, rep.id);
    claimed = null;
    await dispatch(env, rep, deps, settings);
    return { written: true, id: rep.id, status: rep.status };
  } catch (e) {
    console.error('legal reports: the report was not written:', e?.name || 'Error');
    // A claim whose report was not saved is let go, so the next try is not answered busy (its confirmation key with it).
    if (claimed) {
      try { if (!(await loadLegalReport(env, claimed.id))) await env.SOCIAL.delete(unconfirmedKey(claimed.id)); } catch { /* read again by the confirmation, which drops a key with no report */ }
      try { const held = await env.SCENES.get(claimed.key); if (held && parse(await held.text())?.id === claimed.id && !(await loadLegalReport(env, claimed.id))) await env.SCENES.delete(claimed.key); }
      catch { /* taken over after REPORT_CLAIM_MS */ }
    }
    return { written: false, reason: 'unavailable' };
  }
}

/// Adds what was learned after a case's report was written: upload addresses (`uploadIps`, `[{ ip, at, item_id }]`, the
/// forum's record, or a seal that came later) and sealed evidence items (`items`, the text a seal added after a push that
/// failed or came late; `sealed`, whether that seal held). New means not already on the report or an addendum: an address is
/// the same only when its ip, at and item_id all are. A report not yet sent is amended in place: `who.upload_ips` and
/// `evidence.items` gain the new ones, and `who.upload_ip_unknown_items` is said again per item. A report already sent is never edited: they are
/// recorded as an addendum (`addenda: [{ at, kind, upload_ips, items, sent: false }]`), and staff are told it waits to be
/// sent. Never throws. `{ amended: true, id, status, how: 'amended' | 'addendum', upload_ips, items }` (the counts added), or
/// `{ amended: false, reason }`: no_report, nothing_new, unavailable.
export async function amendLegalReport(env, { kind, caseKind, caseId, uploadIps = [], items = [], sealed = false, account = null, reportId = null } = {}) {
  try {
    const id = reportId || await env.SOCIAL.get(caseIndexKey(kind, caseKind, caseId));
    const rep = id ? await loadLegalReport(env, id) : null;
    if (!rep) return { amended: false, reason: 'no_report' };
    // The account's status, confirmed after the report was written ('unconfirmed' at the time): in place before it is sent,
    // as an addendum after.
    if (account && typeof account === 'object' && ['known', 'deleted'].includes(account.account_status) && rep.who?.account_status !== account.account_status) {
      const at = new Date().toISOString();
      const prev = rep.status;
      if (rep.status === 'sent') {
        const takeover = account.account_status === 'known' && rep.who?.account_id ? await takeoverCheck(env, rep.who.account_id, rep.when_where?.uploaded_at, {}) : null;
        rep.addenda = [...(rep.addenda || []), { at, kind: 'account_status', account: { ...account }, ...(takeover ? { takeover } : {}), sent: false }];
        rep.history.push({ at, event: 'addendum', detail: { account_status: account.account_status } });
        await saveLegalReport(env, rep, prev);
        await alertAddendum(env, rep);
        return { amended: true, id: rep.id, status: rep.status, how: 'addendum', account_status: account.account_status };
      }
      const fields = account.account_status === 'known' ? { ...account } : { account_status: 'deleted' };
      rep.who = { ...rep.who, ...fields, upload_ips: rep.who?.upload_ips || [], upload_ip_unknown_items: rep.who?.upload_ip_unknown_items || [] };
      // Found: the takeover check (the account's age, its credential changes) and the route (its country) are made again from
      // the account as it is, never left as they were made with no user record (D4-4).
      if (account.account_status === 'known' && rep.who.account_id) {
        rep.takeover = await takeoverCheck(env, rep.who.account_id, rep.when_where?.uploaded_at, {});
        if (rep.kind === 'csam' || rep.kind === 'threat') rep.route = routeFor(rep.kind, rep.who.country);
      }
      rep.history.push({ at, event: 'amended', detail: { account_status: account.account_status } });
      await saveLegalReport(env, rep, prev);
      return { amended: true, id: rep.id, status: rep.status, how: 'amended', account_status: account.account_status };
    }
    const knownIps = new Set([...(rep.who?.upload_ips || []), ...(rep.addenda || []).flatMap((a) => a.upload_ips || [])].map((x) => uploadIpKey({ ip: x.ip, at: x.at || null, item_id: x.item_id || null })));
    const freshIps = uploadIpsOf(uploadIps).filter((x) => !knownIps.has(uploadIpKey(x)));
    const knownItems = new Set([...(rep.evidence?.items || []), ...(rep.addenda || []).flatMap((a) => a.items || [])]);
    const freshItems = [...new Set((Array.isArray(items) ? items : []).map((i) => clean(i, 64)).filter((i) => i && !knownItems.has(i)))].slice(0, 64);
    if (!freshIps.length && !freshItems.length) return { amended: false, reason: 'nothing_new' };
    const at = new Date().toISOString();
    const prev = rep.status;
    const counts = { upload_ips: freshIps.length, items: freshItems.length };
    if (rep.status === 'sent') {
      const what = freshIps.length && freshItems.length ? 'upload_ips_and_evidence_items' : freshIps.length ? 'upload_ips' : 'evidence_items';
      // The new items the addendum names whose address is still not known, said per item as the report does.
      const allIps = [...(rep.who?.upload_ips || []), ...(rep.addenda || []).flatMap((a) => a.upload_ips || []), ...freshIps];
      const unknown = uploadIpUnknownItems(rep.when_where?.case_kind, freshItems, allIps);
      rep.addenda = [...(rep.addenda || []), { at, kind: what, upload_ips: freshIps, items: freshItems, ...(freshItems.length ? { case_ref: rep.evidence?.case_ref || null, sealed: sealed === true, upload_ip_unknown_items: unknown } : {}), sent: false }];
      rep.history.push({ at, event: 'addendum', detail: counts });
      await saveLegalReport(env, rep, prev);
      await alertAddendum(env, rep);
      return { amended: true, id: rep.id, status: rep.status, how: 'addendum', ...counts };
    }
    if (freshIps.length) rep.who = { ...rep.who, upload_ips: [...(rep.who?.upload_ips || []), ...freshIps].slice(0, 20) };
    if (freshItems.length) rep.evidence = { ...(rep.evidence || {}), items: [...(rep.evidence?.items || []), ...freshItems].slice(0, 64), sealed: rep.evidence?.sealed === true || sealed === true };
    // Said again per item: an item now with a known address leaves the list, a new item without one joins it.
    delete rep.who.upload_ip_note;
    rep.who.upload_ip_unknown_items = uploadIpUnknownItems(rep.when_where?.case_kind, rep.evidence?.items || [], rep.who.upload_ips || []);
    rep.history.push({ at, event: 'amended', detail: counts });
    await saveLegalReport(env, rep, prev);
    return { amended: true, id: rep.id, status: rep.status, how: 'amended', ...counts };
  } catch (e) {
    console.error('legal reports: the report was not amended:', e?.name || 'Error');
    return { amended: false, reason: 'unavailable' };
  }
}

export const UNCONFIRMED_CURSOR_KEY = 'legalunconfirmed-cursor';
/// The reports one run of the confirmation reads (each is a handful of reads and an amendment).
export const UNCONFIRMED_PER_RUN = 25;

/// From the hourly sweep: every CSAM report written with account_status 'unconfirmed' has its account read again: found, it is
/// amended to 'known' with the account's fields (and its takeover check and route made again, D4-4); still absent
/// ACCOUNT_ABSENCE_CONFIRM_MS after the first read, 'deleted'. Its key goes only on a later run that reads the report back
/// confirmed (an amendment whose save was lost is made again). A read that fails waits for the next run. At most
/// UNCONFIRMED_PER_RUN a run, from a cursor (UNCONFIRMED_CURSOR_KEY). Never throws. `{ confirmed }`.
export async function confirmUnconfirmedAccounts(env, { now = Date.now(), limit = UNCONFIRMED_PER_RUN } = {}) {
  const out = { confirmed: 0 };
  try {
    let cursor;
    try { cursor = (await env.SOCIAL.get(UNCONFIRMED_CURSOR_KEY)) || undefined; } catch { cursor = undefined; }
    let listed;
    try { listed = await env.SOCIAL.list({ prefix: 'legalunconfirmed:', limit, ...(cursor ? { cursor } : {}) }); }
    catch { listed = await env.SOCIAL.list({ prefix: 'legalunconfirmed:', limit }); }
    try {
      if (listed?.list_complete === false && listed.cursor) await env.SOCIAL.put(UNCONFIRMED_CURSOR_KEY, listed.cursor);
      else await env.SOCIAL.delete(UNCONFIRMED_CURSOR_KEY);
    } catch { /* from the start next run */ }
    for (const k of listed?.keys || []) {
      try {
        const id = k.name.slice('legalunconfirmed:'.length);
        const rep = await loadLegalReport(env, id);
        if (!rep || rep.who?.account_status !== 'unconfirmed') { await env.SOCIAL.delete(k.name); continue; }
        const accountId = rep.who.account_id;
        const present = !!(await readOrThrow(env.USERS, `user:${accountId}`, 'user record'));
        const first = Date.parse((await env.SOCIAL.get(absentKey(accountId))) || (await env.SOCIAL.get(k.name)) || '');
        let account = null;
        if (present) account = { ...(await reportedAccount(env, accountId)), account_status: 'known' };
        else if (Number.isFinite(first) && now - first >= ACCOUNT_ABSENCE_CONFIRM_MS) account = { account_status: 'deleted' };
        if (!account) continue;
        const r = await amendLegalReport(env, { reportId: id, account });
        // The key stays until a later run reads the report back confirmed (the check above), so a lost save is made again.
        if (r.amended) { out.confirmed += 1; if (present) await env.SOCIAL.delete(absentKey(accountId)).catch(() => {}); }
      } catch (e) {
        console.error('legal reports: an account was not confirmed:', e?.name || 'Error');
      }
    }
  } catch (e) {
    console.error('legal reports: the unconfirmed accounts were not read:', e?.name || 'Error');
  }
  return out;
}

/// The addresses-only form of amendLegalReport (kept for its callers): `{ kind, caseKind, caseId, uploadIps }`.
export const amendLegalReportUploadIps = (env, { kind, caseKind, caseId, uploadIps } = {}) => amendLegalReport(env, { kind, caseKind, caseId, uploadIps });

// -----------------------------------------------------------------------------
// Report debts: owed, settled, stuck (see the header)
// -----------------------------------------------------------------------------

/// The reasons a report is not written that are a decision that no report is owed.
export const SETTLED_REPORT_REASONS = Object.freeze(['not_a_reported_kind', 'borderline_never_reported', 'threat_criteria_not_met']);
/// The reasons that are a passing failure: the debt stays owed and is tried again.
export const OWED_REPORT_REASONS = Object.freeze(['unavailable', 'busy', 'budget', 'account_unconfirmed']);
/// The case kinds a debt is kept for.
export const DEBT_CASE_KINDS = Object.freeze(['forum', 'moderation']);
/// A stuck debt's alert is sent again this often while it stands.
export const STUCK_ALERT_EVERY_MS = 86400000;
/// An owed debt is tried at most this many times (hourly, about a day), then it is stuck as RETRIES_EXHAUSTED for a person.
export const REPORT_DEBT_MAX_TRIES = 24;
export const RETRIES_EXHAUSTED = 'retries_exhausted';
/// A debt stuck as RETRIES_EXHAUSTED is still tried this often (a long outage ends, and the report is then written).
export const STUCK_RETRY_EVERY_MS = 86400000;
/// The most stuck alerts one run of the re-alert sends, so it leaves room for the cron's later steps: split between the case
/// kinds (STUCK_ALERTS_PER_KIND_PER_RUN each), so one kind's backlog never starves the other's.
export const STUCK_ALERTS_PER_RUN = 50;
export const STUCK_ALERTS_PER_KIND_PER_RUN = 25;
/// The stuck debts GET /api/admin/legal/stuck answers per request.
export const STUCK_LIST_PAGE = 200;

/// What one try at a report comes to: 'paid', 'settled', 'owed' or 'stuck'. `result` is writeLegalReport's answer, or null
/// when the try did not get that far (owed). A reason named nowhere above is stuck, so a person sees it.
export function reportDebtOutcome(result) {
  if (!result) return 'owed';
  if (result.written || result.reason === 'already_reported') return 'paid';
  if (SETTLED_REPORT_REASONS.includes(result.reason)) return 'settled';
  if (OWED_REPORT_REASONS.includes(result.reason)) return 'owed';
  return 'stuck';
}

/// The stuck record of a case's debt of `kind`: legalstuck:{caseKind}:{caseId}:{kind}. With no kind, the key a record had before
/// records were kept per kind (still read, and closed like any other).
export const legalStuckKey = (caseKind, caseId, kind = null) => (kind ? `legalstuck:${caseKind}:${caseId}:${kind}` : `legalstuck:${caseKind}:${caseId}`);
const STUCK_CASE_ID = /^[A-Za-z0-9-]{1,64}$/;
const STUCK_KIND = /^[a-z_]{1,40}$/;
const sameSince = (a, b) => (a ?? null) === (b ?? null);

/// The id of the report of `kind` a case has, or null.
export async function caseReportId(env, kind, caseKind, caseId) {
  return (await env.SOCIAL.get(caseIndexKey(kind, caseKind, caseId))) || null;
}

/// Every stuck record of a case, one per report kind, each with the key it is at (`key`).
export async function loadStuckReports(env, caseKind, caseId) {
  if (!DEBT_CASE_KINDS.includes(caseKind) || typeof caseId !== 'string' || !STUCK_CASE_ID.test(caseId)) return [];
  const out = [];
  const legacy = parse(await env.SOCIAL.get(legalStuckKey(caseKind, caseId)));
  if (legacy) out.push({ ...legacy, key: legalStuckKey(caseKind, caseId) });
  const listed = await env.SOCIAL.list({ prefix: `${legalStuckKey(caseKind, caseId)}:`, limit: 20 });
  for (const k of listed?.keys || []) { const r = parse(await env.SOCIAL.get(k.name)); if (r) out.push({ ...r, key: k.name }); }
  return out;
}

/// The stuck record of a case's debt of `kind`, or with no kind the first the case has, or null.
export async function loadStuckReport(env, caseKind, caseId, kind = null) {
  if (!DEBT_CASE_KINDS.includes(caseKind) || typeof caseId !== 'string' || !STUCK_CASE_ID.test(caseId)) return null;
  if (kind) return parse(await env.SOCIAL.get(legalStuckKey(caseKind, caseId, kind))) || (await loadStuckReports(env, caseKind, caseId)).find((r) => r.report_kind === kind) || null;
  return (await loadStuckReports(env, caseKind, caseId))[0] || null;
}

/// Closes the stuck record of a case's debt of `kind` whose debt is paid, settled, replaced or made payable again (`why`), when
/// it is the record of that debt (`since`; null closes whatever record the kind has). Audited, best effort: the debt is
/// already settled on the case. Never throws. `true` when a record was closed.
export async function closeStuckReport(env, { caseKind, caseId, reportKind = null, since = null, why = 'paid', anySince = false } = {}) {
  try {
    const records = (await loadStuckReports(env, caseKind, caseId)).filter((r) => (r.report_kind ?? null) === (reportKind ?? null) || (!reportKind && !r.report_kind));
    let closed = false;
    for (const r of records) {
      if (!anySince && since !== null && !sameSince(r.since, since)) continue;
      try { await writeAuditRecord(env, 'LEGAL_REPORT_STUCK_CLOSED', 'system', r.reference || `${caseKind}:${caseId}`, { case_kind: caseKind, case_id: caseId, report_kind: r.report_kind || null, since: r.since || null, why: clean(why, 40) }); }
      catch (e) { console.error('legal reports: the stuck close was not audited:', e?.name || 'Error'); }
      await env.SOCIAL.delete(r.key);
      closed = true;
    }
    return closed;
  } catch (e) {
    console.error('legal reports: the stuck record was not closed:', e?.name || 'Error');
    return false;
  }
}

// Staff are told a debt is stuck: the case, the kind, the reason and the dates, never the content, the account or an address.
async function alertStuck(env, rec, nowMs, key = legalStuckKey(rec.case_kind, rec.case_id, rec.report_kind || null)) {
  try {
    const { alertStaffStuckReport } = await import('./takedown.mjs');
    const api = String(env.PUBLIC_API_ORIGIN || 'https://api.eustress.dev').replace(/\/$/, '');
    // The reason the tries kept failing is carried with retries_exhausted, so staff see the outage or the defect behind it.
    const reason = rec.last_reason ? `${rec.reason} (last failure: ${rec.last_reason})` : rec.reason;
    const told = await alertStaffStuckReport(env, { reference: rec.reference, case_kind: rec.case_kind, case_id: rec.case_id, report_kind: rec.report_kind, reason, since: rec.since, stuck_at: rec.stuck_at, alerts: (rec.alerts || 0) + 1, admin_url: `${api}/api/admin/legal/stuck` });
    // Recorded as alerted only when a staff address has it: an alert not sent is tried again on the next run.
    if (!(told?.sent > 0 || told?.digested > 0)) return false;
    rec.alerted_at = new Date(nowMs).toISOString();
    rec.alerts = (rec.alerts || 0) + 1;
    // Written back only onto the same stuck record, as it is now: one a person cleared meanwhile is not restored, and one
    // recorded again since (a new stuck_at) is not overwritten with this one.
    const current = parse(await env.SOCIAL.get(key));
    if (current && current.stuck_at === rec.stuck_at) await env.SOCIAL.put(key, JSON.stringify({ ...current, alerted_at: rec.alerted_at, alerts: Math.max(rec.alerts, (current.alerts || 0) + 1) }));
    return true;
  } catch (e) {
    console.error('legal reports: the stuck report alert failed:', e?.name || 'Error');
    return false;
  }
}

/// Moves a case's report debt of `reportKind` to the stuck index (legalstuck:{caseKind}:{caseId}:{reportKind}), audited first,
/// and alerts staff at once. The record is tied to its debt by `since`: a record of the same debt and reason is answered
/// `{ ok: true, already: true }` and not alerted again; one of another debt (another `since`) or another reason is replaced,
/// audited and alerted. `lastReason` is the reason the tries kept failing (RETRIES_EXHAUSTED). Never throws. `{ ok }`; ok
/// false when it could not be recorded (the debt stays owed).
export async function recordStuckReport(env, { caseKind, caseId, reportKind = null, category = null, reason, lastReason = null, reference = null, since = null, now = Date.now() } = {}) {
  try {
    if (!DEBT_CASE_KINDS.includes(caseKind) || typeof caseId !== 'string' || !STUCK_CASE_ID.test(caseId)) return { ok: false };
    const kind = typeof reportKind === 'string' && STUCK_KIND.test(reportKind) ? reportKind : null;
    const key = legalStuckKey(caseKind, caseId, kind);
    const rec = {
      case_kind: caseKind, case_id: caseId, report_kind: clean(kind, 20), category: clean(category, 60), reason: clean(reason, 60) || 'unknown',
      last_reason: clean(lastReason, 60), reference: clean(reference, 80), since: clean(since, 40), stuck_at: new Date(now).toISOString(), alerted_at: null, alerts: 0,
    };
    const current = parse(await env.SOCIAL.get(key));
    if (current && sameSince(current.since, rec.since) && current.reason === rec.reason) {
      // The same debt failing for a new reason: the record says so (no new alert).
      if (rec.last_reason && current.last_reason !== rec.last_reason) await env.SOCIAL.put(key, JSON.stringify({ ...current, last_reason: rec.last_reason }));
      return { ok: true, already: true };
    }
    await writeAuditRecord(env, 'LEGAL_REPORT_STUCK', 'system', rec.reference || `${caseKind}:${caseId}`, { case_kind: caseKind, case_id: caseId, report_kind: rec.report_kind, category: rec.category, reason: rec.reason, last_reason: rec.last_reason, since: rec.since, ...(current ? { replaces: { since: current.since || null, reason: current.reason || null } } : {}) });
    await env.SOCIAL.put(key, JSON.stringify(rec));
    // A record of the same kind still at the key records had before they were kept per kind is this one's: it goes.
    if (kind && (await env.SOCIAL.get(legalStuckKey(caseKind, caseId)))) {
      const legacy = parse(await env.SOCIAL.get(legalStuckKey(caseKind, caseId)));
      if (!legacy?.report_kind || legacy.report_kind === kind) await env.SOCIAL.delete(legalStuckKey(caseKind, caseId));
    }
    await alertStuck(env, rec, now, key);
    return { ok: true, ...(current ? { replaced: true } : {}) };
  } catch (e) {
    console.error('legal reports: the stuck report was not recorded:', e?.name || 'Error');
    return { ok: false };
  }
}

export const stuckCursorKey = (caseKind) => `legalstuck-cursor:${caseKind}`;

/// From the hourly sweep: every stuck debt of `caseKinds` not alerted in the last STUCK_ALERT_EVERY_MS (or never) is alerted
/// again. A page of `limit` at a time, one page a run per kind by default (`maxPages`), carrying on from where the last run stopped
/// (legalstuck-cursor:{caseKind}), so every stuck debt is reached however many there are; the run that reaches the end starts
/// the next from the beginning. Never throws. `{ stuck, alerted }` (the debts read this run).
export async function alertStuckReports(env, { now = Date.now(), limit = 200, maxPages = 1, caseKinds = DEBT_CASE_KINDS, maxAlerts = STUCK_ALERTS_PER_KIND_PER_RUN } = {}) {
  const out = { stuck: 0, alerted: 0, capped: false };
  for (const caseKind of caseKinds.filter((k) => DEBT_CASE_KINDS.includes(k))) {
    let alertedHere = 0;
    let cappedHere = false;
    try {
      let cursor;
      try { cursor = (await env.SOCIAL.get(stuckCursorKey(caseKind))) || undefined; } catch { cursor = undefined; }
      let pages = 0;
      while (pages < maxPages) {
        let listed;
        try { listed = await env.SOCIAL.list({ prefix: `legalstuck:${caseKind}:`, limit, ...(cursor ? { cursor } : {}) }); }
        catch { if (!cursor) break; cursor = undefined; continue; }
        pages += 1;
        for (const k of listed?.keys || []) {
          // Capped for this kind: the page is read again next run (the ones alerted now are not due for a day).
          if (alertedHere >= maxAlerts) { cappedHere = true; out.capped = true; break; }
          const rec = parse(await env.SOCIAL.get(k.name));
          if (!rec) continue;
          out.stuck += 1;
          const last = Date.parse(rec.alerted_at || '');
          if (Number.isFinite(last) && now - last < STUCK_ALERT_EVERY_MS) continue;
          if (await alertStuck(env, rec, now, k.name)) { out.alerted += 1; alertedHere += 1; }
        }
        if (cappedHere) break;
        if (listed?.list_complete !== false || !listed.cursor) { cursor = undefined; break; }
        cursor = listed.cursor;
      }
      try {
        if (cursor) await env.SOCIAL.put(stuckCursorKey(caseKind), cursor);
        else await env.SOCIAL.delete(stuckCursorKey(caseKind));
      } catch { /* the next run reads from where KV says, or from the start */ }
    } catch (e) {
      console.error('legal reports: the stuck reports were not alerted:', e?.name || 'Error');
    }
  }
  return out;
}

/// Staff are told once, at once, that a case now owes a legal report its try could not write (src/moderation.mjs, when a
/// listing debt is created), on the staff mail path (src/takedown.mjs, the legal lane): the case, the kind, the reason and the
/// date, never the content, the account or an address. Sent once per case and debt (`since`); one not sent is tried again the
/// next time the debt is tried. Never throws. `true` when staff have it.
export async function alertReportOwed(env, { caseKind, caseId, reference = null, category = null, reason = null, since = null } = {}) {
  try {
    if (!DEBT_CASE_KINDS.includes(caseKind) || typeof caseId !== 'string' || !STUCK_CASE_ID.test(caseId)) return false;
    const key = `legalalert:owed:${caseKind}:${caseId}:${clean(since, 40) || 'unknown'}`;
    if (await env.SOCIAL.get(key)) return true;
    const { alertStaffReportOwed } = await import('./takedown.mjs');
    const api = String(env.PUBLIC_API_ORIGIN || 'https://api.eustress.dev').replace(/\/$/, '');
    const told = await alertStaffReportOwed(env, { reference: clean(reference, 80), case_kind: caseKind, case_id: caseId, category: clean(category, 60), reason: clean(reason, 60), since: clean(since, 40), admin_url: `${api}/api/admin/legal/reports` });
    if (!(told?.sent > 0 || told?.digested > 0)) return false;
    await env.SOCIAL.put(key, new Date().toISOString(), { expirationTtl: 400 * 86400 });
    return true;
  } catch (e) {
    console.error('legal reports: the owed report alert failed:', e?.name || 'Error');
    return false;
  }
}

// The stuck record a person's clear or retry names: of `reportKind` (any, when the case has one record), and of the debt
// `since` when it is given. `{ record }` or `{ code }`: not_stuck, kind_required (the case has several and none was named),
// record_changed (the record is now of another debt than the one shown).
async function stuckRecordNamed(env, caseKind, caseId, reportKind, since) {
  // The row a person acts on names its debt in full (R-2): its since always, and its report kind whenever the record has one.
  if (since === undefined) return { code: 'since_required' };
  const records = await loadStuckReports(env, caseKind, caseId);
  const named = reportKind ? records.filter((r) => r.report_kind === reportKind) : records;
  if (!named.length) return { code: 'not_stuck' };
  if (named.length > 1) return { code: 'kind_required' };
  if (named[0].report_kind && !reportKind) return { code: 'kind_required' };
  if (!sameSince(named[0].since, since)) return { code: 'record_changed' };
  return { record: named[0] };
}

// The person and the rationale a clear or a retry needs. A code, or null.
function personAct(by, rationale) {
  if (typeof by !== 'string' || !by || by === 'system') return 'person_required';
  const why = typeof rationale === 'string' ? rationale.trim() : '';
  if (why.length < 10 || why.length > 2000) return 'rationale_required';
  return null;
}

/// A person clears a stuck debt. Only a person: `by` is the administrator (never 'system'), with a `rationale` of 10 to 2000
/// characters, and the route writes its audit before this is called (handleLegalReportRoute). It names the record it was
/// shown (`reportKind`, `since`), and clears only that record's debt: the case's debt of that kind, stuck, of that `since`
/// (src/moderation.mjs clearReportDebt, which never touches the evidence). A record whose debt is no longer that one is
/// refused (`debt_changed`) and refreshed, so the list shows the debt as it is. Never throws. `{ ok }` or `{ ok: false, code }`:
/// bad_case, person_required, rationale_required, not_stuck, kind_required, record_changed, debt_changed, busy, unavailable.
export async function clearStuckReport(env, { caseKind, caseId, by, rationale, reportKind = null, since = undefined } = {}) {
  try {
    if (!DEBT_CASE_KINDS.includes(caseKind) || typeof caseId !== 'string' || !STUCK_CASE_ID.test(caseId)) return { ok: false, code: 'bad_case' };
    const refused = personAct(by, rationale);
    if (refused) return { ok: false, code: refused };
    const named = await stuckRecordNamed(env, caseKind, caseId, reportKind, since);
    if (!named.record) return { ok: false, code: named.code };
    const stuck = named.record;
    const mod = await import('./moderation.mjs');
    const r = await mod.clearReportDebt(env, { caseKind, caseId, by, rationale: rationale.trim(), reportKind: stuck.report_kind || null, since: stuck.since ?? null });
    if (!r?.ok) {
      if (r?.code === 'debt_changed') await mod.refreshStuckRecord(env, { caseKind, caseId, reportKind: stuck.report_kind || null }).catch(() => {});
      return { ok: false, code: r?.code || 'unavailable' };
    }
    await env.SOCIAL.delete(stuck.key);
    return { ok: true };
  } catch (e) {
    console.error('legal reports: the stuck report was not cleared:', e?.name || 'Error');
    return { ok: false, code: 'unavailable' };
  }
}

/// A person tries a stuck debt again now (POST .../retry): the record shown (`reportKind`, `since`) names the debt; it is made
/// payable again (its stuck mark and try count go, its stuck record is closed) and tried once at once (src/moderation.mjs
/// retryReportDebt). Only a person, with a rationale, audited by the route first. Never throws. `{ ok, outcome }` or
/// `{ ok: false, code }`, as clearStuckReport.
export async function retryStuckReport(env, { caseKind, caseId, by, rationale, reportKind = null, since = undefined, deps = {} } = {}) {
  try {
    if (!DEBT_CASE_KINDS.includes(caseKind) || typeof caseId !== 'string' || !STUCK_CASE_ID.test(caseId)) return { ok: false, code: 'bad_case' };
    const refused = personAct(by, rationale);
    if (refused) return { ok: false, code: refused };
    if (since === undefined) return { ok: false, code: 'since_required' };
    // A stuck debt is named by its record; an owed one (not stuck) by its kind and since, as the case shows it.
    const named = await stuckRecordNamed(env, caseKind, caseId, reportKind, since);
    if (!named.record && named.code !== 'not_stuck') return { ok: false, code: named.code };
    const kind = named.record ? named.record.report_kind || null : reportKind;
    const r = await (await import('./moderation.mjs')).retryReportDebt(env, { caseKind, caseId, by, reportKind: kind, since: named.record ? named.record.since ?? null : since, owed: !named.record, deps });
    return r?.ok ? { ok: true, outcome: r.outcome } : { ok: false, code: r?.code || 'unavailable' };
  } catch (e) {
    console.error('legal reports: the stuck report was not retried:', e?.name || 'Error');
    return { ok: false, code: 'unavailable' };
  }
}

// -----------------------------------------------------------------------------
// Senders: every one off by default
// -----------------------------------------------------------------------------

/// The CSAM report shaped for the NCMEC CyberTipline. TODO(NCMEC registration): Eustress is not registered with NCMEC, and
/// NCMEC's CyberTipline Reporting API is documented to registered providers only. Every field name below is Eustress's own;
/// none has been checked against NCMEC's schema, and the mapping to it is to be written at registration. Nothing here sends.
export function cyberTiplineDraft(rep) {
  return {
    mapping: 'TODO(NCMEC registration): map to the CyberTipline Reporting API schema; these names are Eustress\'s own',
    reporting_provider: { name: 'Eustress', contact: null },
    incident: {
      category: rep.what.category, article: rep.what.article, detection: rep.what.detection, viewed_by_staff: rep.what.viewed_by_staff, view: rep.what.view || null, statement: rep.what.statement,
      case_reference: rep.when_where.case_reference, location: rep.when_where.subject,
      uploaded_at: rep.when_where.uploaded_at, detected_at: rep.when_where.detected_at, removed_at: rep.when_where.removed_at,
    },
    reported_person: {
      account_id: rep.who.account_id, name: rep.who.verified_name, date_of_birth: rep.who.date_of_birth, country: rep.who.country,
      email: rep.who.email, ip_addresses: rep.who.upload_ips, ip_address_not_known: rep.who.upload_ip_unknown_items || [],
    },
    // The files go from the vault at send time, under the registration's own transfer: TODO(NCMEC registration).
    files: rep.evidence.items.map((item) => ({ case_ref: rep.evidence.case_ref, item })),
    possible_account_takeover: { flag: rep.takeover.flag, credential_changes: rep.takeover.credential_changes, note: rep.takeover.note },
  };
}

/// The NCMEC client. It never sends: there is no registration and no documented API to send to. It answers why.
export async function sendToNcmec(env, draft) {
  void draft;
  if (!(typeof env?.NCMEC_REGISTRATION === 'string' && env.NCMEC_REGISTRATION.trim())) return { sent: false, reason: 'not_registered' };
  return { sent: false, reason: 'ncmec_client_not_implemented' };
}

function renderAuthorityMail(rep) {
  const rows = [
    ['Report', rep.id], ['Case reference', rep.when_where.case_reference || 'none'], ['Category', `${rep.what.category} (Code ${rep.what.article || 'n/a'})`],
    ['Detected', rep.when_where.detected_at], ['Removed', rep.when_where.removed_at], ['Uploaded', rep.when_where.uploaded_at || 'unknown'],
    ['Account', rep.who.account_id], ['Verified name', rep.who.verified_name || 'not verified'], ['Date of birth', rep.who.date_of_birth || 'unknown'],
    ['Country', rep.who.country || 'unknown'], ['Email', rep.who.email || 'none'],
    ['Upload addresses', rep.who.upload_ips.map((x) => `${x.ip} at ${x.at || 'a time not recorded'}${x.item_id ? ` (${x.item_id})` : ''}`).join('; ') || 'none known'],
    ['Items with no known address', (rep.who.upload_ip_unknown_items || []).map((x) => x.item_id).join('; ') || 'none'],
    ['Possible account takeover', rep.takeover.flag ? 'yes: a credential changed shortly before the upload' : 'no credential change before the upload'],
    ['Legal basis', rep.route?.basis || 'none'],
  ];
  const paragraphs = [
    `Eustress, an online platform, reports content it removed as a possible threat to the life or safety of persons. ${rep.what.statement}`,
    'The removed content is preserved and is available on request under legal process, citing the case reference.',
    rep.takeover.note,
  ];
  const text = [...paragraphs, '', ...rows.map(([k, v]) => `${k}: ${v}`)].join('\n');
  const body = html`${paragraphs.map((p) => paragraph(p))}${factRows(rows)}`;
  const email = renderEmail({ title: 'Report under DSA Article 18', heading: 'Report under DSA Article 18', label: 'Legal Report', preheader: paragraphs[0], body, text, footer: 'plain' });
  return { subject: `Eustress report under DSA Article 18: ${rep.when_where.case_reference || rep.id}`, text: email.text, html: email.html };
}

/// The sender for a report and whether it may run: `{ enabled, reason, send }`.
export function senderFor(rep, settings, env) {
  if (rep.status === 'held_setting_off') return { enabled: false, reason: 'LEGAL_REPORT_THREATS is off' };
  if (rep.kind === 'csam') {
    if (!settings.ncmec) return { enabled: false, reason: 'LEGAL_REPORT_NCMEC is off' };
    if (!settings.ncmec_registered) return { enabled: false, reason: 'Eustress is not registered with NCMEC (NCMEC_REGISTRATION is not set)' };
    return { enabled: true, send: () => sendToNcmec(env, cyberTiplineDraft(rep)) };
  }
  if (rep.kind === 'threat') {
    if (!rep.route?.authority) return { enabled: false, reason: 'no destination is decided for this country' };
    if (!settings.eu_email) return { enabled: false, reason: 'LEGAL_REPORT_EU_EMAIL is off' };
    const key = rep.route.member_state || rep.route.authority;
    const address = settings.eu_authorities[key];
    if (!address) return { enabled: false, reason: `no authority address is configured for ${key}` };
    return {
      enabled: true,
      send: async () => {
        const [r] = await sendNotice(env, [address], renderAuthorityMail(rep), { kind: 'notice', accountId: 'legal-reports', fromName: 'Eustress Trust and Safety' });
        return r?.sent ? { sent: true, to: key } : { sent: false, reason: r?.reason || 'send_failed' };
      },
    };
  }
  if (rep.kind === 'scam_batch') {
    if (!settings.scams) return { enabled: false, reason: 'LEGAL_REPORT_SCAMS is off' };
    // TODO(McKale): the FTC and IC3 take complaints through their own web forms; no submission API is used here.
    return { enabled: true, send: async () => ({ sent: false, reason: 'ftc_ic3_submission_not_implemented' }) };
  }
  return { enabled: false, reason: 'unknown report kind' };
}

async function alertOnce(env, rep, reason) {
  const key = alertKey(rep.id, rep.status);
  try {
    if (await env.SOCIAL.get(key)) return false;
    const { alertStaffLegalReport } = await import('./takedown.mjs');
    const api = String(env.PUBLIC_API_ORIGIN || 'https://api.eustress.dev').replace(/\/$/, '');
    await alertStaffLegalReport(env, { id: rep.id, kind: rep.kind, status: rep.status, reference: rep.when_where?.case_reference || null, destination: rep.route?.authority || null, reason, admin_url: `${api}/api/admin/legal/reports?status=${rep.status}` });
    await env.SOCIAL.put(key, new Date().toISOString());
    return true;
  } catch (e) {
    console.error('legal reports: the staff alert failed:', e?.name || 'Error');
    return false;
  }
}

// Staff are told once per addendum that it waits to be sent (no sender sends an addendum on its own).
async function alertAddendum(env, rep) {
  const n = (rep.addenda || []).length;
  const key = `legalalert:${rep.id}:addendum:${n}`;
  try {
    if (await env.SOCIAL.get(key)) return;
    const { alertStaffLegalReport } = await import('./takedown.mjs');
    const api = String(env.PUBLIC_API_ORIGIN || 'https://api.eustress.dev').replace(/\/$/, '');
    await alertStaffLegalReport(env, { id: rep.id, kind: rep.kind, status: rep.status, reference: rep.when_where?.case_reference || null, destination: rep.route?.authority || null, reason: `addendum ${n} (${String(rep.addenda?.at(-1)?.kind || 'details').replace(/_/g, ' ')}) to a sent report waits to be sent`, admin_url: `${api}/api/admin/legal/reports/${rep.id}` });
    await env.SOCIAL.put(key, new Date().toISOString());
  } catch (e) {
    console.error('legal reports: the addendum alert failed:', e?.name || 'Error');
  }
}

/// Tries to send one report, or leaves it queued and alerts staff once. Never throws. Returns the report.
async function dispatch(env, rep, deps, settings = legalSettingsFrom(env)) {
  const prev = rep.status;
  const sender = senderFor(rep, settings, env);
  if (!sender.enabled) {
    rep.waiting_reason = sender.reason;
    await saveLegalReport(env, rep, prev);
    await alertOnce(env, rep, sender.reason);
    return rep;
  }
  try {
    rep.attempts = (rep.attempts || 0) + 1;
    const r = await sender.send();
    if (r?.sent) {
      rep.status = 'sent';
      rep.sent_at = new Date().toISOString();
      rep.history.push({ at: rep.sent_at, event: 'sent', detail: { to: r.to || rep.route?.authority || null } });
      rep.waiting_reason = null;
      await saveLegalReport(env, rep, prev);
      if (rep.kind === 'csam' && rep.notice?.withheld) await releaseReportNotice(env, rep, { by: 'system', reason: 'the report was sent' });
      return rep;
    }
    // A sender that is on but cannot send (NCMEC's stub, a mail binding that is missing) leaves the report ready to send.
    rep.waiting_reason = r?.reason || 'not_sent';
    rep.status = r?.reason === 'send_failed' ? 'send_failed' : 'ready_to_send';
  } catch (e) {
    rep.status = 'send_failed';
    rep.last_error = String(e?.name || 'Error');
    rep.waiting_reason = 'the send failed';
  }
  await saveLegalReport(env, rep, prev);
  await alertOnce(env, rep, rep.waiting_reason);
  return rep;
}

// -----------------------------------------------------------------------------
// The author's notice of a CSAM removal
// -----------------------------------------------------------------------------

/// Releases the author's notice of the case a report is about: when the report is sent, or when a person or the setting
/// releases it. The notice itself (the rule, why, how to appeal) is the case's own (src/moderation.mjs).
export async function releaseReportNotice(env, rep, { by, reason }) {
  const mod = await import('./moderation.mjs');
  const s = rep.when_where?.subject;
  const r = s?.kind === 'listing' ? await mod.releaseRemovalNotice(env, s.sim_id, { by, reason })
    : s?.kind === 'forum_post' ? await mod.releaseForumNotice(env, s.post_id, { by, reason })
      : { ok: false, code: 'no_subject' };
  if (r?.ok) {
    const prev = rep.status;
    rep.notice = { withheld: false, released_at: new Date().toISOString(), released_by: by, reason };
    rep.history.push({ at: rep.notice.released_at, event: 'notice_released', detail: { by, reason } });
    await saveLegalReport(env, rep, prev);
  }
  return r;
}

// -----------------------------------------------------------------------------
// The weekly scam batch
// -----------------------------------------------------------------------------

const DAY_MS = 86400000;
const scamKey = (day, caseKind, caseId) => `scamremoval:${day}:${caseKind}:${caseId}`;
const batchKey = (weekStart) => `scambatch:${weekStart}`;

/// Records one confirmed scam removal for the week's batch. Never throws. `item`: `{ caseKind, caseId, caseRef?, subject,
/// account_id, removed_at, confirmed_by: 'person' | 'classifier', detection? }`.
export async function recordScamRemoval(env, item) {
  try {
    const at = Date.parse(item?.removed_at || '') || Date.now();
    const day = new Date(at).toISOString().slice(0, 10);
    if (!item?.caseKind || !item?.caseId) return false;
    await env.SOCIAL.put(scamKey(day, item.caseKind, item.caseId), JSON.stringify({
      case_kind: item.caseKind, case_id: item.caseId, case_reference: item.caseRef || null, subject: item.subject || null,
      account_id: clean(item.account_id, 128), removed_at: new Date(at).toISOString(), confirmed_by: item.confirmed_by === 'person' ? 'person' : 'classifier',
      // A person confirming a removal is not a person viewing it: only a recorded view says so.
      viewed_by_staff: !!recordedViewOf(item.detection),
      detection: item.detection ? { band: item.detection.band || null, checks: (item.detection.checks || []).slice(0, 4).map(checkOf) } : null,
    }), { expirationTtl: 400 * 86400 });
    return true;
  } catch (e) {
    console.error('legal reports: a scam removal was not recorded:', e?.name || 'Error');
    return false;
  }
}

/// The Monday (UTC, YYYY-MM-DD) that starts the week `nowMs` is in.
export function weekStartOf(nowMs) {
  const d = new Date(nowMs);
  const day = (d.getUTCDay() + 6) % 7;
  return new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate() - day)).toISOString().slice(0, 10);
}

/// Once a week, from the hourly cron: the confirmed scam removals of the last full week (Monday to Sunday, UTC) as one
/// batched report, routed for FTC and IC3 submission and queued `ready_to_send` (LEGAL_REPORT_SCAMS is off by default). Built
/// once per week; every later run that week answers what was built. Never throws.
/// `{ built, week_start, id, count }`.
export async function weeklyScamBatch(env, nowMs = Date.now(), deps = {}) {
  const thisWeek = weekStartOf(nowMs);
  const weekStart = new Date(Date.parse(`${thisWeek}T00:00:00Z`) - 7 * DAY_MS).toISOString().slice(0, 10);
  try {
    const done = parse(await env.SOCIAL.get(batchKey(weekStart)));
    if (done) return { built: false, week_start: weekStart, id: done.id || null, count: done.count || 0 };
    const items = [];
    for (let i = 0; i < 7; i++) {
      const day = new Date(Date.parse(`${weekStart}T00:00:00Z`) + i * DAY_MS).toISOString().slice(0, 10);
      let cursor;
      do {
        const page = await env.SOCIAL.list({ prefix: `scamremoval:${day}:`, limit: 1000, ...(cursor ? { cursor } : {}) });
        for (const k of page?.keys || []) { const it = parse(await env.SOCIAL.get(k.name)); if (it) items.push(it); }
        cursor = page?.list_complete === false ? page.cursor : undefined;
      } while (cursor);
    }
    if (!items.length) {
      await env.SOCIAL.put(batchKey(weekStart), JSON.stringify({ id: null, count: 0, at: new Date().toISOString() }), { expirationTtl: 400 * 86400 });
      return { built: true, week_start: weekStart, id: null, count: 0 };
    }
    const now = new Date().toISOString();
    const weekEnd = new Date(Date.parse(`${weekStart}T00:00:00Z`) + 7 * DAY_MS).toISOString();
    const rep = {
      v: REPORT_VERSION, id: crypto.randomUUID(), kind: 'scam_batch', status: 'ready_to_send', created_at: now, updated_at: now,
      route: routeFor('scam', null),
      what: { category: 'scam', article: 'A7', count: items.length, viewed_by_staff: items.some((i) => i.viewed_by_staff === true), statement: 'Confirmed scam removals on Eustress in the week named, batched for submission.' },
      when_where: { week_start: `${weekStart}T00:00:00.000Z`, week_end: weekEnd, case_reference: null },
      items: items.sort((a, b) => (a.removed_at < b.removed_at ? -1 : 1)),
      notice: null, attempts: 0, last_error: null, sent_at: null, history: [{ at: now, event: 'written' }],
    };
    await saveLegalReport(env, rep);
    await env.SOCIAL.put(batchKey(weekStart), JSON.stringify({ id: rep.id, count: items.length, at: now }), { expirationTtl: 400 * 86400 });
    await dispatch(env, rep, deps);
    return { built: true, week_start: weekStart, id: rep.id, count: items.length };
  } catch (e) {
    console.error('legal reports: the weekly scam batch failed:', e?.name || 'Error');
    return { built: false, week_start: weekStart, id: null, count: 0, error: 'unavailable' };
  }
}

// -----------------------------------------------------------------------------
// The hourly sweep
// -----------------------------------------------------------------------------

/// Hourly, from scheduled() in index.js: every report waiting to be sent is tried again (it stays queued while its sender is
/// off); with LEGAL_REPORT_CSAM_NOTICE_RELEASE on, every withheld CSAM notice is released; the weekly scam batch is built when
/// a week has ended; with FORUM_LEGAL_OWED_SWEEP on, every forum case that owes a report (src/moderation.mjs
/// writeOwedForumReports) has it written; every listing that owes a report has it written (writeOwedListingReports); and every
/// stuck debt is alerted again once a day (alertStuckReports). Never throws. `{ tried, sent, waiting, notices_released,
/// owed_reports, owed_listing_reports, stuck, scam_batch }`; `owed_reports` is 'off' while that setting is off.
export async function sweepLegalReports(env, deps = {}, { limit = 50, now = Date.now() } = {}) {
  const out = { tried: 0, sent: 0, waiting: 0, notices_released: 0, owed_reports: null, scam_batch: null };
  const settings = legalSettingsFrom(env);
  try {
    // A threat report held while LEGAL_REPORT_THREATS was off becomes sendable once it is on, and is tried below.
    if (settings.threats) {
      const held = await env.SOCIAL.list({ prefix: 'legalq:held_setting_off:', limit });
      for (const k of held?.keys || []) {
        const rep = await loadLegalReport(env, k.name.slice('legalq:held_setting_off:'.length));
        if (!rep || rep.status !== 'held_setting_off') continue;
        rep.status = 'ready_to_send';
        rep.history.push({ at: new Date().toISOString(), event: 'threat_setting_on' });
        await saveLegalReport(env, rep, 'held_setting_off');
      }
    }
    for (const status of ['ready_to_send', 'send_failed']) {
      const listed = await env.SOCIAL.list({ prefix: `legalq:${status}:`, limit });
      for (const k of listed?.keys || []) {
        const rep = await loadLegalReport(env, k.name.slice(`legalq:${status}:`.length));
        if (!rep || rep.status !== status) { await env.SOCIAL.delete(k.name).catch(() => {}); continue; }
        out.tried += 1;
        const after = await dispatch(env, rep, deps, settings);
        if (after.status === 'sent') out.sent += 1; else out.waiting += 1;
      }
    }
    // A report that was sent while its case was busy still owes the author's notice: it is released now.
    const sent = await env.SOCIAL.list({ prefix: 'legalq:sent:', limit });
    for (const k of sent?.keys || []) {
      const rep = await loadLegalReport(env, k.name.slice('legalq:sent:'.length));
      if (rep?.kind === 'csam' && rep.notice?.withheld && (await releaseReportNotice(env, rep, { by: 'system', reason: 'the report was sent' }))?.ok) out.notices_released += 1;
    }
    if (settings.csam_notice_release) {
      for (const status of ['ready_to_send', 'send_failed']) {
        const listed = await env.SOCIAL.list({ prefix: `legalq:${status}:`, limit });
        for (const k of listed?.keys || []) {
          const rep = await loadLegalReport(env, k.name.slice(`legalq:${status}:`.length));
          if (rep?.kind === 'csam' && rep.notice?.withheld && (await releaseReportNotice(env, rep, { by: 'setting', reason: 'LEGAL_REPORT_CSAM_NOTICE_RELEASE is on' }))?.ok) out.notices_released += 1;
        }
      }
    }
  } catch (e) {
    console.error('legal reports: the sweep failed:', e?.name || 'Error');
  }
  // Forum cases a person's decision left owing a report (a step after the decision failed): written now, once the forums are
  // on. Off, nothing below is reached: no import of the forum code and no read of its index.
  if (settings.forum_owed_sweep) {
    try { out.owed_reports = await (await import('./moderation.mjs')).writeOwedForumReports(env, deps); }
    catch (e) { console.error('legal reports: the owed forum reports were not written:', e?.name || 'Error'); out.owed_reports = null; }
  } else {
    out.owed_reports = 'off';
  }
  // Listings whose report is owed, once LISTING_LEGAL_OWED_SWEEP is on. Off, nothing below is reached for listings: no listing
  // case or owed index is read.
  if (settings.listing_owed_sweep) {
    try { out.owed_listing_reports = await (await import('./moderation.mjs')).writeOwedListingReports(env, deps); }
    catch (e) { console.error('legal reports: the owed listing reports were not written:', e?.name || 'Error'); out.owed_listing_reports = null; }
  } else {
    out.owed_listing_reports = 'off';
  }
  // Stuck debts: staff are alerted again once a day while each stands, each kind under its own sweep's setting. With both off
  // no stuck index is read and no stuck mail is sent.
  const stuckKinds = [...(settings.forum_owed_sweep ? ['forum'] : []), ...(settings.listing_owed_sweep ? ['moderation'] : [])];
  out.stuck = stuckKinds.length ? await alertStuckReports(env, { now, caseKinds: stuckKinds }) : 'off';
  out.accounts = await confirmUnconfirmedAccounts(env, { now });
  out.scam_batch = await weeklyScamBatch(env, now, deps);
  return out;
}

// -----------------------------------------------------------------------------
// The admin routes
// -----------------------------------------------------------------------------
//   GET  /api/admin/legal/reports?status=                     the queue (ready_to_send by default, or all)
//   GET  /api/admin/legal/reports/{id}                        one report
//   POST /api/admin/legal/reports/{id}/release-notice         a person releases the author's withheld notice: { note }
//   POST /api/admin/legal/notices/{caseKind}/{caseId}/release a person releases a withheld notice with no report (a
//                                                             borderline CSAM removal, which is never reported): { note }
//   GET  /api/admin/legal/stuck                               the stuck report debts (case, kind, reason, dates), audited
//   POST /api/admin/legal/stuck/{caseKind}/{caseId}/clear     a person clears a stuck debt: { note, report_kind?, since? } (the
//                                                             record shown; 409 when the debt is no longer that one), the
//                                                             rationale audited before anything changes (fail-closed)
//   POST /api/admin/legal/stuck/{caseKind}/{caseId}/retry     a person tries a stuck debt again now: the same body and audit
//   GET  /api/admin/legal/staff-alerts                        the staff alert over-cap queue's state (src/staff_alerts.mjs
//                                                             staffQueueState): counts, ages, kinds; audited (fail-closed)

export async function handleLegalReportRoute(request, url, env, cors, deps) {
  if (!url.pathname.startsWith('/api/admin/legal/')) return null;
  const headers = { ...cors, 'Cache-Control': 'private, no-store' };
  const adminId = await deps.requireAdmin(request, env);
  if (!adminId || !(await isAdmin(env, adminId))) return deps.json({ error: 'Admin access required' }, 403, cors);
  // An audit record that must be written: throws when it cannot be (src/evidence_vault.mjs readEvidence is the model).
  const auditOrThrow = async (action, target, details) => {
    if (typeof deps.auditLog !== 'function') throw new Error('the audit log is not configured');
    await deps.auditLog(env, action, adminId, target, details);
  };
  // An audit record whose loss does not stop the act: the list's counts, a release already done.
  const audit = async (action, target, details) => { try { await auditOrThrow(action, target, details); } catch (e) { console.error('legal reports: the audit log write failed:', e?.name || 'Error'); } };
  // The request's JSON body, read once (noteOf and the fields beside the note read the same one).
  let bodyRead = null;
  const bodyOf = async () => {
    if (bodyRead) return bodyRead;
    try { bodyRead = await request.json(); } catch { bodyRead = {}; }
    if (!bodyRead || typeof bodyRead !== 'object') bodyRead = {};
    return bodyRead;
  };
  const noteOf = async () => {
    const body = await bodyOf();
    const note = typeof body.note === 'string' ? body.note.trim() : '';
    return note.length >= 10 && note.length <= 2000 ? note : null;
  };
  if (url.pathname === '/api/admin/legal/reports') {
    if (request.method !== 'GET') return deps.json({ error: 'Method not allowed' }, 405, cors);
    const asked = url.searchParams.get('status') || 'ready_to_send';
    const statuses = asked === 'all' ? REPORT_STATUSES : REPORT_STATUSES.includes(asked) ? [asked] : null;
    if (!statuses) return deps.json({ error: `status is all or one of ${REPORT_STATUSES.join(', ')}` }, 400, cors);
    const items = [];
    for (const status of statuses) {
      const listed = await env.SOCIAL.list({ prefix: `legalq:${status}:`, limit: 200 });
      for (const k of listed?.keys || []) {
        const rep = await loadLegalReport(env, k.name.slice(`legalq:${status}:`.length));
        if (rep && rep.status === status) items.push({ id: rep.id, kind: rep.kind, status: rep.status, created_at: rep.created_at, reference: rep.when_where?.case_reference || null, destination: rep.route?.authority || null, waiting_reason: rep.waiting_reason || null, notice_withheld: !!rep.notice?.withheld });
      }
    }
    // The list is counts, statuses and dates (no address, no account id): its audit is written, and its loss is logged.
    await audit('LEGAL_REPORTS_LISTED', asked, { route: 'GET /api/admin/legal/reports', count: items.length });
    return deps.json({ items }, 200, headers);
  }
  let m = /^\/api\/admin\/legal\/reports\/([0-9a-f-]{36})(\/release-notice)?$/.exec(url.pathname);
  if (m) {
    const rep = await loadLegalReport(env, m[1]);
    if (!rep) return deps.json({ error: 'No such report' }, 404, cors);
    if (!m[2]) {
      if (request.method !== 'GET') return deps.json({ error: 'Method not allowed' }, 405, cors);
      // A report carries addresses and account ids: it is not served unless its read is on the audit log first.
      try { await auditOrThrow('LEGAL_REPORT_READ', rep.id, { route: 'GET /api/admin/legal/reports/{id}', kind: rep.kind }); }
      catch { return deps.json({ error: 'audit_unavailable', code: 'audit_unavailable' }, 503, headers); }
      return deps.json({ report: rep, ...(rep.kind === 'csam' ? { cybertipline_draft: cyberTiplineDraft(rep) } : {}) }, 200, headers);
    }
    if (request.method !== 'POST') return deps.json({ error: 'Method not allowed' }, 405, cors);
    const note = await noteOf();
    if (!note) return deps.json({ error: 'note is required: 10 to 2000 characters saying why', code: 'note_required' }, 400, cors);
    if (!rep.notice?.withheld) return deps.json({ error: 'This report has no withheld notice.', code: 'not_withheld' }, 409, cors);
    const r = await releaseReportNotice(env, rep, { by: adminId, reason: note });
    await audit('LEGAL_NOTICE_RELEASED', rep.id, { note, ok: !!r?.ok });
    return deps.json(r?.ok ? { ok: true } : { error: r?.code || 'unavailable', code: r?.code || 'unavailable' }, r?.ok ? 200 : 409, headers);
  }
  if (url.pathname === '/api/admin/legal/stuck') {
    if (request.method !== 'GET') return deps.json({ error: 'Method not allowed' }, 405, cors);
    // One page of STUCK_LIST_PAGE a request (each item is a read, so a request stays inside the per-invocation storage budget):
    // `?cursor=` (the `next_cursor` of the page before) goes on from there; `next_cursor` is null at the end.
    const items = [];
    let cursor = url.searchParams.get('cursor') || undefined;
    for (let page = 0; page < 1; page++) {
      let listed;
      try { listed = await env.SOCIAL.list({ prefix: 'legalstuck:', limit: STUCK_LIST_PAGE, ...(cursor ? { cursor } : {}) }); }
      catch { return deps.json({ error: 'The cursor is not valid, or the list is unavailable.', code: 'bad_cursor' }, 400, headers); }
      for (const k of listed?.keys || []) {
        const rec = parse(await env.SOCIAL.get(k.name));
        if (rec) items.push({ case_kind: rec.case_kind, case_id: rec.case_id, reference: rec.reference || null, report_kind: rec.report_kind || null, category: rec.category || null, reason: rec.reason, last_reason: rec.last_reason || null, since: rec.since || null, stuck_at: rec.stuck_at, alerted_at: rec.alerted_at || null, alerts: rec.alerts || 0 });
      }
      if (listed?.list_complete !== false || !listed.cursor) { cursor = undefined; break; }
      cursor = listed.cursor;
    }
    // The list names cases by id: it is not served unless its read is on the audit log first, like a report's read.
    try { await auditOrThrow('LEGAL_STUCK_LISTED', 'stuck', { route: 'GET /api/admin/legal/stuck', count: items.length, page_cursor: url.searchParams.get('cursor') ? 'given' : null }); }
    catch { return deps.json({ error: 'audit_unavailable', code: 'audit_unavailable' }, 503, headers); }
    return deps.json({ items, next_cursor: cursor || null }, 200, headers);
  }
  if (url.pathname === '/api/admin/legal/staff-alerts') {
    if (request.method !== 'GET') return deps.json({ error: 'Method not allowed' }, 405, cors);
    // Counts, ages and kinds only (no reference, account or address). Not served unless its read is on the audit log first.
    try { await auditOrThrow('STAFF_ALERT_QUEUE_READ', 'staffmail-queue', { route: 'GET /api/admin/legal/staff-alerts' }); }
    catch { return deps.json({ error: 'audit_unavailable', code: 'audit_unavailable' }, 503, headers); }
    let queue;
    try { queue = await staffQueueState(env); }
    catch { return deps.json({ error: 'The staff alert queue is unavailable.', code: 'unavailable' }, 503, headers); }
    return deps.json({ queue }, 200, headers);
  }
  m = /^\/api\/admin\/legal\/stuck\/(moderation|forum)\/([A-Za-z0-9-]{1,64})\/(clear|retry)$/.exec(url.pathname);
  if (m) {
    if (request.method !== 'POST') return deps.json({ error: 'Method not allowed' }, 405, cors);
    const note = await noteOf();
    if (!note) return deps.json({ error: 'note is required: 10 to 2000 characters saying why', code: 'note_required' }, 400, cors);
    const body = await bodyOf();
    const retry = m[3] === 'retry';
    // The row the person was shown names its debt (R-2): `since` is required (null for a record that has none), and
    // `report_kind` whenever the record has one.
    if (!Object.prototype.hasOwnProperty.call(body, 'since') || (body.since !== null && typeof body.since !== 'string')) return deps.json({ error: 'since is required: the debt\'s since, as the list shows it', code: 'since_required' }, 400, cors);
    const reportKind = typeof body.report_kind === 'string' && /^[a-z_]{1,40}$/.test(body.report_kind) ? body.report_kind : null;
    const since = body.since === null ? null : body.since.slice(0, 40);
    const named = await stuckRecordNamed(env, m[1], m[2], reportKind, since);
    if (!named.record && !(retry && named.code === 'not_stuck')) {
      if (named.code === 'not_stuck') return deps.json({ error: 'No stuck report debt for this case.', code: 'not_stuck' }, 404, cors);
      if (named.code === 'kind_required' && !reportKind) return deps.json({ error: 'report_kind is required: the record names one.', code: 'report_kind_required' }, 400, cors);
      return deps.json({ error: named.code === 'kind_required' ? 'This case has more than one stuck debt: name its report_kind.' : 'The stuck record is now of another debt: reload the list.', code: named.code }, 409, headers);
    }
    // A retry of an owed debt (not stuck) names it by its kind and since, as the case shows it.
    const stuck = named.record || { report_kind: reportKind, since, reason: 'owed', reference: null };
    // Only a person clears or retries a debt, and never unaudited: the attempt is audited first, or nothing changes; then its
    // outcome, as it came out (LEGAL_STUCK_CLEARED only for a clear that was made, LEGAL_STUCK_CLEAR_FAILED with the code
    // otherwise; LEGAL_STUCK_RETRIED with the try's outcome, or LEGAL_STUCK_RETRY_FAILED).
    const target = stuck.reference || `${m[1]}:${m[2]}`;
    const facts = { case_kind: m[1], case_id: m[2], report_kind: stuck.report_kind || null, since: stuck.since || null, reason: stuck.reason, note };
    try { await auditOrThrow(retry ? 'LEGAL_STUCK_RETRY_REQUESTED' : 'LEGAL_STUCK_CLEAR_REQUESTED', target, facts); }
    catch { return deps.json({ error: 'audit_unavailable', code: 'audit_unavailable' }, 503, headers); }
    const args = { caseKind: m[1], caseId: m[2], by: adminId, rationale: note, reportKind: stuck.report_kind || null, since: stuck.since ?? null };
    const r = retry ? await retryStuckReport(env, { ...args, deps: deps.reportDeps || {} }) : await clearStuckReport(env, args);
    await audit(retry ? (r.ok ? 'LEGAL_STUCK_RETRIED' : 'LEGAL_STUCK_RETRY_FAILED') : (r.ok ? 'LEGAL_STUCK_CLEARED' : 'LEGAL_STUCK_CLEAR_FAILED'), target, { ...facts, ...(r.ok ? (retry ? { outcome: r.outcome } : {}) : { code: r.code }) });
    return deps.json(r.ok ? { ok: true, ...(retry ? { outcome: r.outcome } : {}) } : { error: r.code, code: r.code }, r.ok ? 200 : r.code === 'busy' || r.code === 'unavailable' ? 503 : r.code === 'not_owed' ? 404 : 409, headers);
  }
  m = /^\/api\/admin\/legal\/notices\/(moderation|forum)\/([A-Za-z0-9-]{1,64})\/release$/.exec(url.pathname);
  if (m) {
    if (request.method !== 'POST') return deps.json({ error: 'Method not allowed' }, 405, cors);
    const note = await noteOf();
    if (!note) return deps.json({ error: 'note is required: 10 to 2000 characters saying why', code: 'note_required' }, 400, cors);
    const mod = await import('./moderation.mjs');
    const r = m[1] === 'moderation' ? await mod.releaseRemovalNotice(env, m[2], { by: adminId, reason: note }) : await mod.releaseForumNotice(env, m[2], { by: adminId, reason: note });
    await audit('LEGAL_NOTICE_RELEASED', `${m[1]}:${m[2]}`, { note, ok: !!r?.ok });
    return deps.json(r?.ok ? { ok: true } : { error: r?.code || 'unavailable', code: r?.code || 'unavailable' }, r?.ok ? 200 : 409, headers);
  }
  return deps.json({ error: 'Unknown legal route' }, 404, cors);
}
