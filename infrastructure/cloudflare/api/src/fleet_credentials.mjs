// =============================================================================
// Fleet credentials: the host credential (ehost_) and the hosted-session credential (eshs_)
// =============================================================================
//
// Both follow the esrv_ server credential in src/live.mjs: 32 random bytes as hex behind a prefix, sent as
// `Authorization: Bearer`, read before verifyAuth so a credential is never taken for a user id, and only the SHA-256
// kept. docs/architecture/HOSTED_FLEET.md, "Authentication".
//
//   ehost_<64 hex>  one per rented machine, minted by an administrator. SOCIAL KV: `hostcred:<sha256>` holds
//                   { v: 1, machine_id, region, created_at }, and `hostcred-machine:<machine_id>` names the hash that is
//                   current, so minting again revokes the last. It authenticates the host heartbeat
//                   (POST /api/fleet/hosts/{machine_id}/heartbeat) and nothing else.
//   eshs_<64 hex>   one per started session, minted by the fleet object when it tells a host to start one. Held in the
//                   fleet Durable Object, not in KV: `hostsess:<sha256>` holds { v: 1, sim_id, machine_id, server_id,
//                   expires_at }. It is accepted on POST and DELETE /api/simulations/{sim_id}/live and on POST .../live/admit
//                   for that sim_id and that server_id only, and is deleted when the session's stop is acknowledged, when
//                   the host reports the session gone, or 90 s after its last accepted /live heartbeat.
//
// This file reads no other Worker module, so src/live.mjs can import it without a cycle.
// =============================================================================

export const HOST_CREDENTIAL = /^ehost_[0-9a-f]{64}$/;
export const SESSION_CREDENTIAL = /^eshs_[0-9a-f]{64}$/;
/// A machine id or server id: 1 to 64 of A-Z a-z 0-9 _ -, the /live server_id rule.
export const FLEET_ID = /^[A-Za-z0-9_-]{1,64}$/;
/// The one fleet object every decision goes through.
export const FLEET_OBJECT_NAME = 'fleet';

const hostKey = (hash) => `hostcred:${hash}`;
const hostMachineKey = (machineId) => `hostcred-machine:${machineId}`;

/// SHA-256 of `text`, lowercase hex.
export async function sha256Hex(text) {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

/// Equal strings, compared without stopping at the first difference.
export function sameText(a, b) {
  if (typeof a !== 'string' || typeof b !== 'string') return false;
  let diff = a.length ^ b.length;
  for (let i = 0; i < Math.max(a.length, b.length); i++) diff |= (a.charCodeAt(i) || 0) ^ (b.charCodeAt(i) || 0);
  return diff === 0;
}

/// `<prefix><64 hex>` from 32 random bytes.
export function mintSecret(prefix) {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return `${prefix}${[...bytes].map((b) => b.toString(16).padStart(2, '0')).join('')}`;
}

/// Random hex of `bytes` bytes, for ids that are not secrets (server ids, command ids).
export function randomHex(bytes) {
  const b = new Uint8Array(bytes);
  crypto.getRandomValues(b);
  return [...b].map((x) => x.toString(16).padStart(2, '0')).join('');
}

/// The machine an ehost_ credential stands for, `{ machine_id, region }`, or null: not the grammar, not stored, unreadable,
/// or replaced by a newer mint for its machine.
export async function hostFromCredential(env, token) {
  if (typeof token !== 'string' || !HOST_CREDENTIAL.test(token) || !env?.SOCIAL) return null;
  const hash = await sha256Hex(token);
  const raw = await env.SOCIAL.get(hostKey(hash));
  if (!raw) return null;
  let record;
  try {
    record = JSON.parse(raw);
  } catch {
    return null;
  }
  if (record?.v !== 1 || typeof record.machine_id !== 'string' || !FLEET_ID.test(record.machine_id)) return null;
  if (!sameText(await env.SOCIAL.get(hostMachineKey(record.machine_id)), hash)) return null;
  return { machine_id: record.machine_id, region: typeof record.region === 'string' ? record.region : '' };
}

/// Mint the machine's host credential, revoking the one before it. The token is returned once and never stored.
export async function mintHostCredential(env, machineId, region) {
  const token = mintSecret('ehost_');
  const hash = await sha256Hex(token);
  const previous = await env.SOCIAL.get(hostMachineKey(machineId));
  if (previous && previous !== hash) await env.SOCIAL.delete(hostKey(previous));
  await env.SOCIAL.put(hostKey(hash), JSON.stringify({ v: 1, machine_id: machineId, region, created_at: new Date().toISOString() }));
  await env.SOCIAL.put(hostMachineKey(machineId), hash);
  return token;
}

/// Revoke the machine's host credential. Whether there was one.
export async function revokeHostCredential(env, machineId) {
  const hash = await env.SOCIAL.get(hostMachineKey(machineId));
  if (hash) await env.SOCIAL.delete(hostKey(hash));
  await env.SOCIAL.delete(hostMachineKey(machineId));
  return !!hash;
}

/// A call into the fleet object. A missing binding or an unreadable answer is `{ ok: false, status: 503 }`.
export async function fleetCall(env, op, args) {
  if (!env?.FLEET) return { ok: false, status: 503, code: 'fleet_unavailable' };
  try {
    const stub = env.FLEET.get(env.FLEET.idFromName(FLEET_OBJECT_NAME));
    const res = await stub.fetch(`https://fleet.internal/${op}`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(args),
    });
    return await res.json();
  } catch (e) {
    console.error(`fleet ${op} failed:`, e?.name || 'Error');
    return { ok: false, status: 503, code: 'fleet_unavailable' };
  }
}

/// The session an eshs_ credential stands for, `{ machine_id, server_id }`, when it was minted for exactly `simId` and
/// `serverId` and has not expired or been revoked; else null. `beat`: this is an accepted /live heartbeat, which moves the
/// credential's expiry to 90 s from now.
export async function sessionFromCredential(env, token, simId, serverId, { now, beat = false } = {}) {
  if (typeof token !== 'string' || !SESSION_CREDENTIAL.test(token)) return null;
  if (typeof simId !== 'string' || typeof serverId !== 'string' || !FLEET_ID.test(serverId)) return null;
  const r = await fleetCall(env, 'session_credential', { hash: await sha256Hex(token), sim_id: simId, server_id: serverId, now, beat });
  return r?.ok === true ? { machine_id: r.machine_id, server_id: r.server_id } : null;
}

/// The server id an eshs_ credential was minted for, whatever listing is asked about, or null. For /live/admit, whose
/// body names no server id: the route then checks the ticket's audience against that server's own pin.
export async function sessionServerOf(env, token, simId, { now } = {}) {
  if (typeof token !== 'string' || !SESSION_CREDENTIAL.test(token)) return null;
  const r = await fleetCall(env, 'session_credential', { hash: await sha256Hex(token), sim_id: simId, server_id: null, now, beat: false });
  return r?.ok === true ? { machine_id: r.machine_id, server_id: r.server_id } : null;
}
