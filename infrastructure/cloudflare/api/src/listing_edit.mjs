// =============================================================================
// An author edits a published listing from the website
// =============================================================================
//
// PATCH /api/simulations/{id}/listing with any of:
//
//   { "name": "...", "description": "...", "is_public": true | false,
//     "gpu": "none" | "optional" | "required", "gpu_min_vram_mb": 8192 }
//
// The listing's title and description are text a reviewer reads. A changed one sends the listing back to review
// (sendBackToReview), exactly as a commit from Studio with new text does, and the page then asks for the review with
// POST /api/simulations/{id}/submit, the same request Studio makes after a publish. Who may see it (`is_public`) is not
// text a reviewer reads: switching it changes nothing about the review, and a listing that passed review is listed or kept
// private at once. Tags have their own route (PATCH /api/simulations/{id}/tags).
//
// `gpu` is where Eustress may host the listing's sessions (docs/architecture/GPU_HOSTING_RULE.md): "none", the default,
// and "optional" place on any machine, CPU first; "required" places only on a machine with a GPU of at least
// `gpu_min_vram_mb` free, which "required" must name (src/fleet_policy.mjs checkGpuSetting). It is not text a reviewer
// reads either.
//
// Only the author, and never while a hold is on the listing. Never while a review is running either: a run judges the
// title and description it read when it began, and its verdict is applied to the listing as it is when it ends, so an edit
// written mid-run would go live on a verdict about the old text. The edit takes the run's own lease (acquireLease) for the
// moment it writes. A run that holds it refuses the edit, and a run that starts after reads the new text. The refusal reads
// the same as a hold's (inReview), so the two are never told apart.
//
// Studio keeps its own copy of the title and description (publish.toml) and sends it with every commit. The edit records
// the text it replaced (src/listing_guard.mjs), so a commit from a Studio that has not caught up keeps the edit.
//
// A value that breaks a rule refuses the whole request (400) and nothing is written: a title is 1 to 120 characters, a
// description at most 4000 (the limits a commit applies, world.mjs MAX_LISTING), `is_public` a boolean. The same value the
// listing already has is no change. docs/networking/LISTING_EDIT.md.
// =============================================================================

import { acquireLease, releaseLease, thresholdsFrom, contentLockedFor, inReview, sendBackToReview, authorStatusOf } from './moderation.mjs';
import { withTagsWrittenBack } from './tags.mjs';
import { guardReplacedText } from './listing_guard.mjs';
import { checkGpuSetting, gpuSettingOf } from './fleet_policy.mjs';

export const LISTING_EDIT_LIMITS = Object.freeze({ name: 120, description: 4000 });

/// The fields a request may change, checked. `{ ok: true, edit }` with only the fields sent, or `{ ok: false, error, code,
/// field }` for the first one that breaks a rule. Pure.
export function checkListingEdit(body) {
  if (!body || typeof body !== 'object' || Array.isArray(body)) {
    return { ok: false, error: 'Send a JSON object with name, description, is_public or gpu.', code: 'bad_request', field: null };
  }
  const edit = {};
  if (body.name !== undefined) {
    if (typeof body.name !== 'string') return { ok: false, error: 'The title must be text.', code: 'bad_name', field: 'name' };
    const name = body.name.trim();
    if (!name) return { ok: false, error: 'A listing needs a title.', code: 'bad_name', field: 'name' };
    if (name.length > LISTING_EDIT_LIMITS.name) return { ok: false, error: `The title is ${name.length} characters; the most is ${LISTING_EDIT_LIMITS.name}.`, code: 'too_long', field: 'name' };
    edit.name = name;
  }
  if (body.description !== undefined) {
    if (typeof body.description !== 'string') return { ok: false, error: 'The description must be text.', code: 'bad_description', field: 'description' };
    const description = body.description.trim();
    if (description.length > LISTING_EDIT_LIMITS.description) return { ok: false, error: `The description is ${description.length} characters; the most is ${LISTING_EDIT_LIMITS.description}.`, code: 'too_long', field: 'description' };
    edit.description = description;
  }
  if (body.is_public !== undefined) {
    if (typeof body.is_public !== 'boolean') return { ok: false, error: 'is_public must be true or false.', code: 'bad_visibility', field: 'is_public' };
    edit.is_public = body.is_public;
  }
  if (body.gpu !== undefined || body.gpu_min_vram_mb !== undefined) {
    if (body.gpu === undefined) {
      return { ok: false, error: 'gpu_min_vram_mb goes with gpu: "required".', code: 'bad_gpu', field: 'gpu' };
    }
    const gpu = checkGpuSetting(body.gpu, body.gpu_min_vram_mb);
    if (!gpu.ok) return gpu;
    edit.gpu = gpu.value.gpu;
    edit.gpu_min_vram_mb = gpu.value.gpu_min_vram_mb;
  }
  if (Object.keys(edit).length === 0) {
    return { ok: false, error: 'Nothing to change: send name, description, is_public or gpu.', code: 'bad_request', field: null };
  }
  return { ok: true, edit };
}

/// Applies a checked edit to a listing record. Returns the names of the fields that changed and whether the change is one a
/// review reads (the title or the description). Pure: the caller writes the record.
export function applyListingEdit(sim, edit, at = new Date().toISOString()) {
  const changed = [];
  const old = { name: sim.name, description: sim.description };
  if (edit.name !== undefined && edit.name !== String(sim.name ?? '').trim()) {
    sim.name = edit.name;
    changed.push('name');
  }
  if (edit.description !== undefined && edit.description !== String(sim.description ?? '').trim()) {
    sim.description = edit.description;
    changed.push('description');
  }
  if (edit.is_public !== undefined) {
    // An archived listing stays private whatever is asked; the choice is what Unarchive restores (as a commit's is).
    if (sim.archived === true) {
      if ((sim.archived_was_public !== false) !== edit.is_public) {
        sim.archived_was_public = edit.is_public;
        changed.push('is_public');
      }
    } else if ((sim.is_public !== false) !== edit.is_public) {
      sim.is_public = edit.is_public;
      changed.push('is_public');
    }
  }
  if (edit.gpu !== undefined) {
    const was = gpuSettingOf(sim);
    if (was.gpu !== edit.gpu || was.gpu_min_vram_mb !== edit.gpu_min_vram_mb) {
      // "none" is the default and is stored as no setting at all.
      if (edit.gpu === 'none') delete sim.gpu;
      else sim.gpu = edit.gpu;
      if (edit.gpu === 'required') sim.gpu_min_vram_mb = edit.gpu_min_vram_mb;
      else delete sim.gpu_min_vram_mb;
      changed.push('gpu');
    }
  }
  const review = changed.includes('name') || changed.includes('description');
  // Studio still holds the text this replaced (publish.toml). Its next commit sends it back; the guard keeps this edit
  // (src/listing_guard.mjs, read by world.mjs applyListing).
  if (review) {
    const replaced = {};
    if (changed.includes('name')) replaced.name = typeof old.name === 'string' ? old.name : '';
    if (changed.includes('description')) replaced.description = typeof old.description === 'string' ? old.description : '';
    guardReplacedText(sim, { old: replaced, by: 'author_web', at });
  }
  return { changed, review };
}

/// The listing as the author's edit answer shows it.
function answerOf(simId, sim, changed, review) {
  return {
    ok: true,
    id: simId,
    name: sim.name ?? '',
    description: sim.description ?? '',
    is_public: sim.is_public !== false,
    archived: sim.archived === true,
    ...gpuSettingOf(sim),
    changed,
    // True when the change sends the listing back to review: the page then asks for it (POST .../submit).
    review,
    status: authorStatusOf(sim.moderation?.status, sim.is_public !== false),
  };
}

/// PATCH /api/simulations/{id}/listing. `deps`: `{ json, verifyAuth }`.
export async function handleEditListing(request, simId, env, cors, deps) {
  const userId = await deps.verifyAuth(request, env);
  if (!userId) return deps.json({ error: 'Unauthorized' }, 401, cors);
  const read = async () => {
    const raw = await env.SOCIAL.get(`sim:${simId}`);
    return raw ? JSON.parse(raw) : null;
  };
  const seen = await read();
  // One answer for a listing that does not exist and one that is not yours.
  if (!seen || seen.author_id !== userId) return deps.json({ error: 'Simulation not found' }, 404, cors);
  if (await contentLockedFor(env, seen, userId)) return inReview(deps.json, cors);

  let body = null;
  try { body = await request.json(); } catch { body = null; }
  const checked = checkListingEdit(body);
  if (!checked.ok) return deps.json({ error: checked.error, code: checked.code, field: checked.field }, 400, cors);

  const lease = await acquireLease(env, simId, thresholdsFrom(env), 'listing_edit');
  if (!lease) return inReview(deps.json, cors);
  try {
    // Read again under the lease: a run that ended while this request waited may have written the listing, and a hold
    // applied meanwhile is seen through the R2 lock (contentLockedFor).
    const sim = await read();
    if (!sim || sim.author_id !== userId) return deps.json({ error: 'Simulation not found' }, 404, cors);
    if (await contentLockedFor(env, sim, userId)) return inReview(deps.json, cors);
    const { changed, review } = applyListingEdit(sim, checked.edit);
    if (changed.length === 0) return deps.json(answerOf(simId, sim, changed, false), 200, cors);
    if (review) sendBackToReview(sim);
    sim.updated_at = new Date().toISOString();
    await env.SOCIAL.put(`sim:${simId}`, JSON.stringify(withTagsWrittenBack(sim)));
    return deps.json(answerOf(simId, sim, changed, review), 200, cors);
  } finally {
    await releaseLease(env, lease);
  }
}
