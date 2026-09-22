// Avatar identity is a closed M/F/R enum, independent of body morph sliders.

// The one body a Robot has, mirroring `BodyMorphs::robot()` in
// eustress-avatar-schema (metrics.rs: MIN 1.45 m, MAX 2.05 m, bind 1.83 m).
// Height is the value that renders the rig at exactly its authored size, so
// the customizer's `height / bind` is 1.0; build 0.5 is neutral width. The
// neutral 0.5 height would shrink the model to 95.6%, which is why it is not
// simply "the defaults". Agents do not resize.
export const ROBOT_HEIGHT = (1.83 - 1.45) / (2.05 - 1.45);
export const ROBOT_BUILD = 0.5;
const near = (a, b) => Math.abs(a - b) < 1e-4;

export function validateAvatar(d) {
  if (!d || typeof d !== 'object' || Array.isArray(d)) throw new Error('Expected an avatar descriptor');
  if (d.schema_version !== 2) throw new Error('Unsupported avatar schema');
  const bodies = { M: 'masculine', F: 'feminine', R: 'robot' };
  if (!Object.hasOwn(bodies, d.identity)) throw new Error('Identity must be M, F or R');
  if (d.base_body !== bodies[d.identity]) throw new Error('Body does not match identity');
  const norm = value => typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= 1;
  const morphs = ['height', 'build', 'leg_ratio', 'face_round', 'face_square', 'face_oval', 'face_diamond'];
  if (!d.morphs || morphs.some(k => !norm(d.morphs[k]))) throw new Error('Invalid body proportions');
  if (d.identity === 'R' && !(near(d.morphs.height, ROBOT_HEIGHT) && near(d.morphs.build, ROBOT_BUILD)))
    throw new Error('Robot bodies have a fixed height and build');
  if (!d.palette || ['skin','hair','top','bottom'].some(k => !/^#[0-9a-f]{6}$/i.test(d.palette[k]))) throw new Error('Invalid palette');
  if (!Array.isArray(d.slots) || d.slots.length > 8) throw new Error('Invalid wearable slots');
  const kinds = new Set(['hair','face','top','bottom','shoes','hat','glasses','back']);
  const seen = new Set();
  for (const slot of d.slots) {
    if (!Array.isArray(slot) || slot.length !== 2 || !kinds.has(slot[0]) || seen.has(slot[0]) || (slot[1] !== null && (typeof slot[1] !== 'string' || slot[1].length > 128))) throw new Error('Invalid wearable slot');
    seen.add(slot[0]);
  }
  if (!d.motion || typeof d.motion !== 'object' || Array.isArray(d.motion)) throw new Error('Invalid motion');
  for (const [k,v] of Object.entries(d.motion)) {
    if (!['walk_speed_mps','run_speed_mps','sprint_multiplier','jump_apex_m'].includes(k) || (v !== null && (typeof v !== 'number' || !Number.isFinite(v) || v <= 0 || v > 100))) throw new Error('Invalid motion override');
  }
  if (d.rig != null) {
    const r = d.rig;
    if (typeof r.id !== 'string' || !/^[a-zA-Z0-9_-]{1,64}$/.test(r.id) || typeof r.label !== 'string' || !r.label.trim() || r.label.length > 80 || r.identity !== d.identity) throw new Error('Invalid rig identity');
    if (!Array.isArray(r.animations) || r.animations.length !== 4) throw new Error('Rig needs idle, walk, run and jump clips');
    for (const path of [r.body_asset, ...r.animations]) {
      const relative = typeof path === 'string' && path.startsWith('bundled://characters/') ? path.slice(21) : '';
      if (!relative || relative.length > 240 || !/^[a-zA-Z0-9_./-]+\.glb$/.test(relative) || relative.split('/').some(p => !p || p === '.' || p === '..')) throw new Error('Invalid rig asset path');
    }
    if (!Array.isArray(r.bone_aliases) || r.bone_aliases.length > 256) throw new Error('Invalid bone aliases');
    const sources = new Set(), targets = new Set();
    for (const pair of r.bone_aliases) {
      if (!Array.isArray(pair) || pair.length !== 2) throw new Error('Invalid bone alias');
      const [source,target] = pair;
      if (typeof source !== 'string' || !source || source.length > 128 || typeof target !== 'string' || !/^[a-z0-9]{1,64}$/.test(target) || sources.has(source) || targets.has(target)) throw new Error('Invalid bone alias');
      sources.add(source); targets.add(target);
    }
  }
  return d;
}

/// The sex marker the account's identity document carried: 'M', 'F', or null.
///
/// Null covers an X marker, a field the document read could not resolve, and
/// every record verified before `extracted_sex` existed. Reads the
/// front-of-document record under the real user id, which registration writes
/// when it links the verification session to the account. A record that never
/// reached 'verified' (and therefore was never 'linked') cannot supply a marker.
async function documentSex(env, userId) {
  const raw = await env.KYC_STATUS?.get(`kyc-${userId}-front`);
  if (!raw) return null;
  let record;
  try { record = JSON.parse(raw); } catch { return null; }
  if (record.status !== 'verified' && record.status !== 'linked') return null;
  return record.extracted_sex === 'M' || record.extracted_sex === 'F' ? record.extracted_sex : null;
}

/// The one avatar identity this account may have, and where it comes from.
///
/// Identity is never a choice. A human account is bound to the sex marker on
/// its verified document, M or F. An agent account (an AI model registered in
/// its own right, `account_type: 'agent'`) is Robot, and never sees a document.
/// Returns null when a human has no usable marker on file; policy is that such
/// an account cannot customise until a re-verification puts one there.
///
/// A user record without `account_type` predates the field and is human.
export async function lockedIdentity(env, userId) {
  let user = null;
  try { user = JSON.parse(await env.USERS.get(`user:${userId}`) || 'null'); } catch { user = null; }
  if (user?.account_type === 'agent') return { identity: 'R', source: 'agent' };
  const sex = await documentSex(env, userId);
  return sex ? { identity: sex, source: 'document' } : null;
}

export async function handleAvatar(request, env, cors, verifyAuth, json) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);
  // Separate KV key prevents avatar writes overwriting concurrent user changes.
  const key = `avatar:${userId}`;
  const lock = await lockedIdentity(env, userId);
  if (request.method === 'GET') {
    const stored = await env.USERS.get(key);
    // `locked_identity` is the one identity this account may have, or null when
    // it has none yet and the client must not offer the controls at all.
    return json({ descriptor: stored ? JSON.parse(stored) : null, locked_identity: lock?.identity ?? null, lock_source: lock?.source ?? null }, 200, { ...cors, 'Cache-Control': 'private, no-store' });
  }
  // Writes are gated on the account having an identity to be. The client hides
  // its controls in this state; this is the check that holds when the client
  // is not ours.
  if (!lock) return json({ error: 'Identity verification with a readable sex marker is required before customizing an avatar', code: 'kyc_sex_required' }, 403, cors);
  if (Number(request.headers.get('Content-Length')) > 32768) return json({ error: 'Avatar is too large' }, 413, cors);
  // Stream a bounded body; a missing Content-Length must not bypass the limit.
  const reader = request.body?.getReader();
  if (!reader) return json({ error: 'Missing avatar' }, 400, cors);
  const chunks = []; let size = 0;
  while (true) {
    const { done, value } = await reader.read(); if (done) break;
    size += value.length;
    if (size > 32768) { await reader.cancel(); return json({ error: 'Avatar is too large' }, 413, cors); }
    chunks.push(value);
  }
  const bytes = new Uint8Array(size); let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
  let descriptor;
  try { descriptor = validateAvatar(JSON.parse(new TextDecoder().decode(bytes))); }
  catch (error) { return json({ error: error.message }, 400, cors); }
  // The identity on the wire must be the account's one identity. Humans cannot
  // be Robot; agents cannot be a sex; nobody picks.
  if (descriptor.identity !== lock.identity)
    return json({ error: 'Avatar identity is fixed by the account', code: 'identity_locked' }, 403, cors);
  await env.USERS.put(key, JSON.stringify(descriptor));
  return json({ descriptor }, 200, { ...cors, 'Cache-Control': 'private, no-store' });
}
