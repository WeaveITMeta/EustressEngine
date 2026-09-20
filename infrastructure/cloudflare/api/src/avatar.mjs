// Avatar identity is a closed M/F/R enum, independent of body morph sliders.
export function validateAvatar(d) {
  if (!d || typeof d !== 'object' || Array.isArray(d)) throw new Error('Expected an avatar descriptor');
  if (d.schema_version !== 2) throw new Error('Unsupported avatar schema');
  const bodies = { M: 'masculine', F: 'feminine', R: 'robot' };
  if (!Object.hasOwn(bodies, d.identity)) throw new Error('Identity must be M, F or R');
  if (d.base_body !== bodies[d.identity]) throw new Error('Body does not match identity');
  const norm = value => typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= 1;
  const morphs = ['height', 'build', 'leg_ratio', 'face_round', 'face_square', 'face_oval', 'face_diamond'];
  if (!d.morphs || morphs.some(k => !norm(d.morphs[k]))) throw new Error('Invalid body proportions');
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

export async function handleAvatar(request, env, cors, verifyAuth, json) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);
  // Separate KV key prevents avatar writes overwriting concurrent user changes.
  const key = `avatar:${userId}`;
  if (request.method === 'GET') {
    const stored = await env.USERS.get(key);
    return json({ descriptor: stored ? JSON.parse(stored) : null }, 200, { ...cors, 'Cache-Control': 'private, no-store' });
  }
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
  await env.USERS.put(key, JSON.stringify(descriptor));
  return json({ descriptor }, 200, { ...cors, 'Cache-Control': 'private, no-store' });
}
