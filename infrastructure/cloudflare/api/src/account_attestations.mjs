// =============================================================================
// What review knows about an author's account: attested property, verified business
// =============================================================================
//
// Guardian Code v2.1. Two facts about an account change how a review reads a
// Space, and neither can be claimed by the author in the listing:
//
//   - Property-ownership attestation (Part I, after A14). An author who proved
//     they own, lease or protect a property carries an attestation. A10, A13 and
//     A14 never apply to the author's own attested property; a citation of one of
//     them against attested property is held for a person (Part IV rule 1).
//   - A verified business (B6(h), Part III "Product"). Payment off Eustress is
//     allowed only through a verified business connection, and never in an
//     Adult listing; the Product category is for a verified business.
//
// The flows that prove these (Disciplines & Modes: property records, deeds and
// leases; business verification) are not built yet. Until they are, an
// administrator records them through the routes below, so the review can be
// exercised end to end. Nothing here is self-service.
//
// Storage (USERS):
//   property-attestation:{accountId}  [{ address | parcel_id, kind, verified_at, source }]
//   verified-business:{accountId}     { name, verified_at, source, payment_connection }
//
// Routes (an administrator only; moderation.mjs gates every /api/admin/moderation/ path):
//   GET    /api/admin/moderation/property-attestation/{accountId}
//   PUT    /api/admin/moderation/property-attestation/{accountId}   { properties: [...] }
//   DELETE /api/admin/moderation/property-attestation/{accountId}
//   GET    /api/admin/moderation/verified-business/{accountId}
//   PUT    /api/admin/moderation/verified-business/{accountId}      { name, verified_at, source, payment_connection }
//   DELETE /api/admin/moderation/verified-business/{accountId}
// =============================================================================

export const PROPERTY_KINDS = ['own', 'lease', 'protect'];
export const MAX_ATTESTED_PROPERTIES = 25;
const ACCOUNT_ID = /^[A-Za-z0-9_-]{1,64}$/;
const TEXT_MAX = 200;

export const attestationKey = (accountId) => `property-attestation:${accountId}`;
export const businessKey = (accountId) => `verified-business:${accountId}`;

const isTime = (v) => typeof v === 'string' && v.length <= 40 && Number.isFinite(Date.parse(v));
const text = (v, max = TEXT_MAX) => (typeof v === 'string' && v.trim() && v.length <= max ? v.trim() : null);

/// `{ ok: true, properties }` or `{ ok: false, error }`. Each property names an address or a parcel id (or both), a kind
/// (own, lease or protect), when it was verified, and the source of the proof.
export function checkAttestations(value) {
  if (!Array.isArray(value)) return { ok: false, error: 'properties must be a list' };
  if (value.length > MAX_ATTESTED_PROPERTIES) return { ok: false, error: `at most ${MAX_ATTESTED_PROPERTIES} properties` };
  const properties = [];
  for (const [i, p] of value.entries()) {
    if (!p || typeof p !== 'object') return { ok: false, error: `property ${i + 1} must be an object` };
    const address = p.address === undefined ? null : text(p.address);
    const parcel = p.parcel_id === undefined ? null : text(p.parcel_id, 80);
    if (!address && !parcel) return { ok: false, error: `property ${i + 1} needs an address or a parcel_id` };
    if (!PROPERTY_KINDS.includes(p.kind)) return { ok: false, error: `property ${i + 1}: kind is one of ${PROPERTY_KINDS.join(', ')}` };
    if (!isTime(p.verified_at)) return { ok: false, error: `property ${i + 1}: verified_at is a date` };
    const source = text(p.source);
    if (!source) return { ok: false, error: `property ${i + 1}: source names the proof (a deed, a lease, property records)` };
    properties.push({ ...(address ? { address } : {}), ...(parcel ? { parcel_id: parcel } : {}), kind: p.kind, verified_at: new Date(p.verified_at).toISOString(), source });
  }
  return { ok: true, properties };
}

/// `{ ok: true, business }` or `{ ok: false, error }`. `payment_connection` names the verified connection payments off
/// Eustress go through (null when the business is verified but has none registered).
export function checkBusiness(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return { ok: false, error: 'a JSON object is required' };
  const name = text(value.name);
  if (!name) return { ok: false, error: 'name is the verified business name' };
  if (!isTime(value.verified_at)) return { ok: false, error: 'verified_at is a date' };
  const source = text(value.source);
  if (!source) return { ok: false, error: 'source names how the business was verified' };
  const connection = value.payment_connection === undefined || value.payment_connection === null ? null : text(value.payment_connection, 120);
  if (value.payment_connection !== undefined && value.payment_connection !== null && !connection) return { ok: false, error: 'payment_connection is the registered connection, or null' };
  return { ok: true, business: { name, verified_at: new Date(value.verified_at).toISOString(), source, payment_connection: connection } };
}

async function readJson(env, key) {
  try {
    const raw = await env.USERS.get(key);
    return raw ? JSON.parse(raw) : null;
  } catch {
    return null;
  }
}

/// The account's attested properties: always a list, empty when there are none or the record cannot be read.
export async function readPropertyAttestations(env, accountId) {
  if (typeof accountId !== 'string' || !ACCOUNT_ID.test(accountId) || !env?.USERS) return [];
  const stored = await readJson(env, attestationKey(accountId));
  const checked = checkAttestations(Array.isArray(stored) ? stored : []);
  return checked.ok ? checked.properties : [];
}

/// The account's verified business, or null.
export async function readVerifiedBusiness(env, accountId) {
  if (typeof accountId !== 'string' || !ACCOUNT_ID.test(accountId) || !env?.USERS) return null;
  const checked = checkBusiness(await readJson(env, businessKey(accountId)));
  return checked.ok ? checked.business : null;
}

/// Everything a review reads about the author's account, in one object the triage and the judge's case summary carry.
export async function accountFacts(env, accountId) {
  const [properties, business] = await Promise.all([readPropertyAttestations(env, accountId), readVerifiedBusiness(env, accountId)]);
  return {
    attested_properties: properties,
    verified_business: !!business,
    business_name: business?.name || null,
    payment_connection: !!business?.payment_connection,
  };
}

const fold = (s) => String(s || '').toLowerCase().normalize('NFKC').replace(/[^a-z0-9]+/g, ' ').trim();

/// The attested properties a piece of text names, by address or parcel id, compared with case and punctuation folded.
export function propertiesNamedIn(properties, ...texts) {
  const hay = ` ${texts.map(fold).join(' ')} `;
  return (properties || []).filter((p) => [p.address, p.parcel_id].some((v) => v && fold(v).length >= 4 && hay.includes(` ${fold(v)} `)));
}

// An audit entry that cannot be written does not undo the change; the failure is logged.
async function audit(deps, env, action, target, details) {
  try { await deps.auditLog?.(env, action, deps.adminId, target, details); } catch (e) { console.error('attestation audit failed', e?.message); }
}

/// The administrator's routes above, or null for any other path. `deps`: json, cors, auditLog, adminId.
export async function handleAttestationAdminRoute(request, url, env, deps) {
  const { json, cors } = deps;
  const m = /^\/api\/admin\/moderation\/(property-attestation|verified-business)\/([^/]+)$/.exec(url.pathname);
  if (!m) return null;
  const [, which, accountId] = m;
  if (!ACCOUNT_ID.test(accountId)) return json({ error: 'not an account id' }, 400, cors);
  const property = which === 'property-attestation';
  const key = property ? attestationKey(accountId) : businessKey(accountId);
  if (request.method === 'GET') {
    return json(property ? { account_id: accountId, properties: await readPropertyAttestations(env, accountId) } : { account_id: accountId, business: await readVerifiedBusiness(env, accountId) }, 200, cors);
  }
  if (request.method === 'DELETE') {
    await env.USERS.delete(key);
    await audit(deps, env, property ? 'property_attestation_removed' : 'verified_business_removed', accountId, {});
    return json({ ok: true, removed: true }, 200, cors);
  }
  if (request.method !== 'PUT' && request.method !== 'POST') return json({ error: 'Method not allowed' }, 405, cors);
  let body;
  try { body = await request.json(); } catch { return json({ error: 'JSON body required' }, 400, cors); }
  const checked = property ? checkAttestations(body?.properties) : checkBusiness(body);
  if (!checked.ok) return json({ error: checked.error }, 400, cors);
  const value = property ? checked.properties : checked.business;
  await env.USERS.put(key, JSON.stringify(value));
  await audit(deps, env, property ? 'property_attestation_set' : 'verified_business_set', accountId, property ? { count: value.length } : { name: value.name, payment_connection: !!value.payment_connection });
  return json({ ok: true, account_id: accountId, ...(property ? { properties: value } : { business: value }) }, 200, cors);
}
