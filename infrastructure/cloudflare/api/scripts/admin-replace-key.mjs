// Replaces the identity key of an account whose owner lost it, after a person has checked who they are
// (docs/architecture/IDENTITY_KEY_ROTATION_AND_RECOVERY.md, "Support procedure for a lost key").
//
// The administrator signs the request with their OWN identity file; the script signs in with it, asks for a
// challenge, signs it, and sends it. It sends the owner's new PUBLIC key and the signature the owner's new
// key made on /replacement-key, never a private key. Nothing secret is printed.
//
//   node scripts/admin-replace-key.mjs --api https://api-staging.eustress.dev \
//     --identity C:\path\to\eustress-admin.toml --username owner \
//     --new-public-key <64 hex> --new-key-signature <128 hex> \
//     --case SUP-1042 --reason "ID checked against the sign-up ID on a video call"
//
// The route is off unless the deployment sets ADMIN_REPLACE_KEY_ENABLED.

import crypto from 'node:crypto';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

const NL = '\n';
const PKCS8_PREFIX = Buffer.from('302e020100300506032b657004220420', 'hex');

/// The public and private key of an identity file: its `[identity]` table, or the top level of an older file.
export function readIdentity(text) {
  let section = '';
  const out = {};
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith('#')) continue;
    const table = /^\[(.+)\]$/.exec(line);
    if (table) { section = table[1].trim(); continue; }
    if (section && section !== 'identity') continue;
    const [k, ...rest] = line.split('=');
    out[k.trim()] = rest.join('=').trim().replace(/^["']|["']$/g, '');
  }
  if (!/^[0-9a-fA-F]{64}$/.test(out.public_key || '') || !/^[0-9a-fA-F]{64}$/.test(out.private_key || '')) throw new Error('that is not an identity file');
  return { publicKey: out.public_key.toLowerCase(), privateKey: out.private_key.toLowerCase() };
}

function signer(privateKeyHex) {
  const key = crypto.createPrivateKey({ key: Buffer.concat([PKCS8_PREFIX, Buffer.from(privateKeyHex, 'hex')]), format: 'der', type: 'pkcs8' });
  return (text) => crypto.sign(null, Buffer.from(text), key).toString('hex');
}

/// Sign in with the administrator's identity file (login v1), then replace the key. `fetchImpl` is the
/// network, replaceable in a test. Returns the route's answer; throws on any refusal, with the Worker's words.
export async function replaceKey({ api, identityText, username, accountId, newPublicKey, newKeySignature, caseRef, reason, fetchImpl = fetch }) {
  const origin = new URL(api).origin;
  const { publicKey, privateKey } = readIdentity(identityText);
  const sign = signer(privateKey);
  const call = async (method, route, body, token) => {
    const res = await fetchImpl(`${origin}${route}`, {
      method,
      headers: { 'Content-Type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
      body: method === 'GET' ? undefined : JSON.stringify(body || {}),
    });
    const json = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(`${route}: ${res.status} ${json.code || ''} ${json.error || ''}`.trim());
    return json;
  };

  const challenge = (await call('POST', '/api/auth/challenge', { public_key: publicKey })).challenge;
  const login = await call('POST', '/api/auth/verify-challenge', {
    format: 'eustress-login-v1', public_key: publicKey, challenge, signature: sign(['eustress-login-v1', origin, challenge].join(NL)),
  });
  const token = login.token;

  const target = accountId ? { account_id: accountId } : { username };
  const begun = await call('POST', '/api/admin/accounts/replace-key/begin', { ...target, new_public_key: newPublicKey, case_ref: caseRef }, token);
  const message = ['eustress-admin-replace-v1', begun.origin, begun.admin_id, begun.account_id, begun.new_public_key, caseRef, begun.challenge].join(NL);
  return call('POST', '/api/admin/accounts/replace-key', {
    ...target, new_public_key: newPublicKey, case_ref: caseRef, reason, challenge: begun.challenge,
    admin_signature: sign(message), new_key_signature: newKeySignature,
  }, token);
}

function args(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    if (argv[i].startsWith('--')) out[argv[i].slice(2)] = argv[++i];
  }
  return out;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const a = args(process.argv.slice(2));
  const missing = ['api', 'identity', 'new-public-key', 'new-key-signature', 'case', 'reason'].filter((k) => !a[k]);
  if (missing.length || (!a.username && !a['account-id'])) {
    console.error(`Missing: ${[...missing, ...(!a.username && !a['account-id'] ? ['username or account-id'] : [])].join(', ')}`);
    process.exit(2);
  }
  try {
    const r = await replaceKey({
      api: a.api, identityText: fs.readFileSync(a.identity, 'utf8'), username: a.username, accountId: a['account-id'],
      newPublicKey: a['new-public-key'], newKeySignature: a['new-key-signature'], caseRef: a.case, reason: a.reason,
    });
    console.log(JSON.stringify({
      ok: r.ok, account: r.username, key_epoch: r.key_epoch, replaced_at: r.replaced_at, value_frozen_until: r.value_frozen_until,
      payout_pinned_until: r.payout_pinned_until, undo_until: r.undo_until, revoked: r.revoked, revoked_complete: r.revoked_complete, notices_sent: r.notices_sent,
    }, null, 2));
    if (r.revoked_complete === false) console.error('Some credentials could not be revoked yet; the account object keeps retrying them.');
  } catch (e) {
    console.error(e.message);
    process.exit(1);
  }
}
