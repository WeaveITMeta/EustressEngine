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
//
// Storage (SOCIAL):
//   legalreport:{id}                            the report
//   legalq:{status}:{id}                        the queue by status
//   legalcase:{kind}:{caseKind}:{caseId}        the report a case already has (one per case and kind)
//   legalalert:{id}:{status}                    a staff alert sent once per report and status
//   scamremoval:{day}:{caseKind}:{caseId}       a confirmed scam removal, for the week's batch
//   scambatch:{weekStart}                       the week's batch, built once
// =============================================================================

import { assignCaseReference } from './case_reference.mjs';
import { readAccountEmail } from './email_ops.mjs';
import { sendNotice } from './account_notices.mjs';
import { renderEmail, html, paragraph, facts as factRows } from './email_shell.mjs';
import { isAdmin } from './admin_check.mjs';

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
// each sealed text item, `post-text-...`; the items that carry addresses are not content). An address known for an item is
// in `who.upload_ips` with that `item_id`.
const FORUM_TEXT_ITEM = /^post-text(?:-[0-9a-f]{32})?$/;
export function uploadIpUnknownItems(caseKind, items, ips) {
  const known = new Set((ips || []).map((x) => x?.item_id).filter(Boolean));
  const content = [...new Set((items || []).filter((i) => typeof i === 'string' && (caseKind !== 'forum' || FORUM_TEXT_ITEM.test(i))))];
  return content.filter((i) => !known.has(i)).map((item_id) => ({ item_id, upload_ip: null, reason: 'not_known' }));
}

export const SETTING_NAMES = Object.freeze(['LEGAL_REPORT_NCMEC', 'LEGAL_REPORT_EU_EMAIL', 'LEGAL_REPORT_THREATS', 'LEGAL_REPORT_SCAMS', 'LEGAL_REPORT_CSAM_NOTICE_RELEASE']);

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
export async function reportedAccount(env, accountId) {
  const user = parse(await env.USERS?.get(`user:${accountId}`).catch(() => null));
  const kyc = parse(await env.KYC_STATUS?.get(`kyc-${accountId}-front`).catch(() => null));
  const verified = !!kyc && (kyc.status === 'verified' || kyc.status === 'linked');
  let dob = null;
  if (verified) {
    const { resolveDob } = await import('./adult_content.mjs');
    dob = clean(resolveDob(kyc, user).dob, 10);
  }
  const country = clean(kyc?.jurisdiction || kyc?.iso2 || null, 2);
  let email = null;
  try { email = clean(await readAccountEmail(env, user ? { ...user, id: user.id || accountId } : null), 254); } catch { email = null; }
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
  try {
    if (!finding || !['csam', 'threat'].includes(finding.kind)) return { written: false, reason: 'not_a_reported_kind' };
    if (!reportableBand(finding.detection)) return { written: false, reason: 'borderline_never_reported' };
    let assessment = null;
    if (finding.kind === 'threat') {
      assessment = assessThreat({ checks: finding.detection.checks, content: finding.content });
      if (!assessment.reportable) return { written: false, reason: 'threat_criteria_not_met', assessment };
    }
    const accountId = finding.uploader?.account_id;
    if (typeof accountId !== 'string' || !/^[A-Za-z0-9_.:-]{1,128}$/.test(accountId)) return { written: false, reason: 'no_account' };
    const idx = caseIndexKey(finding.kind, finding.caseKind, finding.caseId);
    const existing = await env.SOCIAL.get(idx);
    if (existing) { const rep = await loadLegalReport(env, existing); if (rep) return { written: false, reason: 'already_reported', id: rep.id, status: rep.status }; }

    const settings = legalSettingsFrom(env);
    let reference = finding.caseRef || null;
    if (!reference) { try { reference = await assignCaseReference(env, finding.caseKind, finding.caseId); } catch { reference = null; } }
    const who = await reportedAccount(env, accountId);
    const ips = uploadIpsOf(finding.uploader?.upload_ips);
    const view = recordedViewOf(finding.detection);
    const viewed = !!view;
    const now = new Date().toISOString();
    const rep = {
      v: REPORT_VERSION, id: crypto.randomUUID(), kind: finding.kind, status: finding.kind === 'threat' && !settings.threats ? 'held_setting_off' : 'ready_to_send',
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
      who: { ...who, upload_ips: ips, upload_ip_unknown_items: uploadIpUnknownItems(finding.caseKind, (finding.evidence?.items || []).slice(0, 64), ips) },
      takeover: await takeoverCheck(env, accountId, finding.uploader?.uploaded_at, deps),
      evidence: { case_ref: finding.evidence?.case_ref || reference, items: (finding.evidence?.items || []).slice(0, 64), sealed: finding.evidence?.sealed === true },
      // CSAM only: the author's notice waits for this report to be sent (or for a person, or the setting, to release it).
      notice: finding.kind === 'csam' ? { withheld: true, released_at: null, released_by: null } : null,
      attempts: 0, last_error: null, sent_at: null, history: [{ at: now, event: 'written' }],
    };
    await saveLegalReport(env, rep);
    await env.SOCIAL.put(idx, rep.id);
    await dispatch(env, rep, deps, settings);
    return { written: true, id: rep.id, status: rep.status };
  } catch (e) {
    console.error('legal reports: the report was not written:', e?.name || 'Error');
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
export async function amendLegalReport(env, { kind, caseKind, caseId, uploadIps = [], items = [], sealed = false } = {}) {
  try {
    const id = await env.SOCIAL.get(caseIndexKey(kind, caseKind, caseId));
    const rep = id ? await loadLegalReport(env, id) : null;
    if (!rep) return { amended: false, reason: 'no_report' };
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

/// The addresses-only form of amendLegalReport (kept for its callers): `{ kind, caseKind, caseId, uploadIps }`.
export const amendLegalReportUploadIps = (env, { kind, caseKind, caseId, uploadIps } = {}) => amendLegalReport(env, { kind, caseKind, caseId, uploadIps });

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
/// a week has ended. Never throws. `{ tried, sent, waiting, notices_released, scam_batch }`.
export async function sweepLegalReports(env, deps = {}, { limit = 50, now = Date.now() } = {}) {
  const out = { tried: 0, sent: 0, waiting: 0, notices_released: 0, scam_batch: null };
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

export async function handleLegalReportRoute(request, url, env, cors, deps) {
  if (!url.pathname.startsWith('/api/admin/legal/')) return null;
  const headers = { ...cors, 'Cache-Control': 'private, no-store' };
  const adminId = await deps.requireAdmin(request, env);
  if (!adminId || !(await isAdmin(env, adminId))) return deps.json({ error: 'Admin access required' }, 403, cors);
  const audit = async (action, target, details) => { try { await deps.auditLog?.(env, action, adminId, target, details); } catch (e) { console.error('legal reports: the audit log write failed:', e?.name || 'Error'); } };
  const noteOf = async () => {
    let body = {};
    try { body = await request.json(); } catch { body = {}; }
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
    await audit('LEGAL_REPORTS_LISTED', asked, { count: items.length });
    return deps.json({ items }, 200, headers);
  }
  let m = /^\/api\/admin\/legal\/reports\/([0-9a-f-]{36})(\/release-notice)?$/.exec(url.pathname);
  if (m) {
    const rep = await loadLegalReport(env, m[1]);
    if (!rep) return deps.json({ error: 'No such report' }, 404, cors);
    if (!m[2]) {
      if (request.method !== 'GET') return deps.json({ error: 'Method not allowed' }, 405, cors);
      await audit('LEGAL_REPORT_READ', rep.id, { kind: rep.kind });
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
