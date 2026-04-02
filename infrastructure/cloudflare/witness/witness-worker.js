// =============================================================================
// Eustress Witness Worker — Co-signing API for Local-First Earning
// =============================================================================
// This Worker is the independent witness that makes local-first earning secure.
// It holds the server's Ed25519 signing key and co-signs contribution hashes
// submitted by local EustressEngine instances.
//
// The user never touches the server key. The Worker never touches user content.
//
// Deploy: cd infrastructure/cloudflare/witness && wrangler deploy
// Secrets required:
//   - SIGNING_KEY: Ed25519 private key (base64)
//   - SERVER_PUBLIC_KEY: Ed25519 public key (base64)
// KV namespaces required:
//   - USERS: user registry (user_id → {public_key, registered_at, revoked})
//   - RATE_LIMITS: per-user rate counters (user_id:hour → count)
// =============================================================================

// CORS headers for cross-origin requests from local engines
const CORS_HEADERS = {
  'Access-Control-Allow-Origin': '*',
  'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
  'Access-Control-Allow-Headers': 'Content-Type, Authorization',
};

export default {
  async fetch(request, env, ctx) {
    // Handle CORS preflight
    if (request.method === 'OPTIONS') {
      return new Response(null, { status: 204, headers: CORS_HEADERS });
    }

    const url = new URL(request.url);

    try {
      // -----------------------------------------------------------------------
      // Well-known endpoints (public, read-only)
      // -----------------------------------------------------------------------

      if (url.pathname === '/.well-known/eustress-fork') {
        return json({
          fork_id: env.FORK_ID,
          public_key: env.SERVER_PUBLIC_KEY,
          chain_id: parseInt(env.CHAIN_ID),
          bliss_version: env.BLISS_VERSION,
          identity_schema_version: env.IDENTITY_SCHEMA_VERSION,
          contact: 'admin@eustress.dev',
        });
      }

      if (url.pathname === '/.well-known/eustress-identity') {
        return json({
          public_key: env.SERVER_PUBLIC_KEY,
          fork_id: env.FORK_ID,
        });
      }

      if (url.pathname === '/.well-known/eustress-revoked') {
        // Revocation list — read from KV or return empty
        const revoked = await env.USERS.get('__revocation_list', 'json');
        return json(revoked || {
          issued_by: env.FORK_ID,
          list_version: 0,
          entries: [],
          signature: '',
        });
      }

      if (url.pathname === '/.well-known/eustress-rates') {
        // Rate reporting — read from KV
        const rates = await env.USERS.get('__rate_report', 'json');
        return json(rates || {
          fork_id: env.FORK_ID,
          total_issued: '0',
          total_contribution_score: 0,
          rate: 0,
          active_users: 0,
          total_bridged_out: '0',
          total_bridged_in: '0',
          last_updated: new Date().toISOString(),
        });
      }

      // -----------------------------------------------------------------------
      // POST /api/cosign — Co-sign a contribution hash
      // -----------------------------------------------------------------------

      if (url.pathname === '/api/cosign' && request.method === 'POST') {
        const body = await request.json();
        const { user_id, contribution_hash, timestamp } = body;

        // Validate required fields
        if (!user_id || !contribution_hash || !timestamp) {
          return json({ error: 'missing_fields' }, 400);
        }

        // Look up user in KV
        const user = await env.USERS.get(`user:${user_id}`, 'json');
        if (!user) {
          return json({ error: 'unknown_user' }, 403);
        }
        if (user.revoked) {
          return json({ error: 'user_revoked' }, 403);
        }

        // Rate limiting: check requests this hour
        const hourKey = `rate:${user_id}:${currentHourKey()}`;
        const currentCount = parseInt(await env.RATE_LIMITS.get(hourKey) || '0');
        const limit = parseInt(env.RATE_LIMIT_PER_HOUR);

        if (currentCount >= limit) {
          return json({ error: 'rate_limited', limit, reset_at: nextHourISO() }, 429);
        }

        // Increment rate counter (TTL: 2 hours to auto-cleanup)
        ctx.waitUntil(
          env.RATE_LIMITS.put(hourKey, String(currentCount + 1), { expirationTtl: 7200 })
        );

        // Build canonical payload and sign
        const payload = `cosign|${user_id}|${contribution_hash}|${timestamp}`;
        const signature = await signPayload(env.SIGNING_KEY, payload);

        return json({
          server_signature: signature,
          co_signed_at: new Date().toISOString(),
        });
      }

      // -----------------------------------------------------------------------
      // POST /api/register — Register a new user identity
      // -----------------------------------------------------------------------

      if (url.pathname === '/api/register' && request.method === 'POST') {
        const body = await request.json();
        const { user_id, public_key } = body;

        if (!user_id || !public_key) {
          return json({ error: 'missing_fields' }, 400);
        }

        // Check if user already exists
        const existing = await env.USERS.get(`user:${user_id}`, 'json');
        if (existing) {
          return json({ error: 'user_exists' }, 409);
        }

        // Store user in KV
        await env.USERS.put(`user:${user_id}`, JSON.stringify({
          public_key,
          registered_at: new Date().toISOString(),
          revoked: false,
        }));

        return json({ registered: true, user_id });
      }

      // -----------------------------------------------------------------------
      // POST /api/fork/register — Register an external fork
      // -----------------------------------------------------------------------

      if (url.pathname === '/api/fork/register' && request.method === 'POST') {
        const body = await request.json();
        const { fork_id, public_key, chain_id, endpoint } = body;

        if (!fork_id || !public_key || !chain_id || !endpoint) {
          return json({ error: 'missing_fields' }, 400);
        }

        // Verify the fork's well-known endpoint is live
        try {
          const forkResp = await fetch(`${endpoint}/.well-known/eustress-fork`);
          if (!forkResp.ok) {
            return json({ error: 'endpoint_unreachable' }, 502);
          }
          const forkInfo = await forkResp.json();
          if (forkInfo.fork_id !== fork_id || forkInfo.public_key !== public_key) {
            return json({ error: 'endpoint_mismatch' }, 400);
          }
        } catch (e) {
          return json({ error: 'endpoint_unreachable', detail: e.message }, 502);
        }

        // Store fork in KV
        await env.USERS.put(`fork:${fork_id}`, JSON.stringify({
          fork_id,
          public_key,
          chain_id,
          endpoint,
          registered_at: new Date().toISOString(),
          trusted: false, // Trust requires manual approval or mutual trust flow
        }));

        return json({ registered: true, fork_id });
      }

      // -----------------------------------------------------------------------
      // GET /api/forks — List registered forks (for trust registry)
      // -----------------------------------------------------------------------

      if (url.pathname === '/api/forks' && request.method === 'GET') {
        // List all forks from KV (prefix scan)
        const list = await env.USERS.list({ prefix: 'fork:' });
        const forks = [];

        for (const key of list.keys) {
          const fork = await env.USERS.get(key.name, 'json');
          if (fork) {
            forks.push(fork);
          }
        }

        return json({ forks });
      }

      // -----------------------------------------------------------------------
      // GET /api/trust-registry — Public Online Trust Registry data
      // -----------------------------------------------------------------------

      if (url.pathname === '/api/trust-registry' && request.method === 'GET') {
        // Aggregate rate data from all registered forks
        const list = await env.USERS.list({ prefix: 'fork:' });
        const registry = [];

        for (const key of list.keys) {
          const fork = await env.USERS.get(key.name, 'json');
          if (!fork) continue;

          // Try to fetch the fork's rate data
          let rates = null;
          try {
            const ratesResp = await fetch(`${fork.endpoint}/.well-known/eustress-rates`);
            if (ratesResp.ok) {
              rates = await ratesResp.json();
            }
          } catch (_) {
            // Fork unreachable — use cached data or skip
          }

          registry.push({
            fork_id: fork.fork_id,
            public_key: fork.public_key,
            chain_id: fork.chain_id,
            endpoint: fork.endpoint,
            trusted: fork.trusted,
            registered_at: fork.registered_at,
            rates: rates || null,
          });
        }

        // Compute median rate for deviation calculation
        const ratesWithData = registry.filter(f => f.rates && f.rates.rate > 0);
        let median_rate = 0;
        if (ratesWithData.length > 0) {
          const sorted = ratesWithData.map(f => f.rates.rate).sort((a, b) => a - b);
          median_rate = sorted[Math.floor(sorted.length / 2)];
        }

        // Add deviation percentage to each fork
        const entries = registry.map(f => ({
          ...f,
          deviation_pct: (f.rates && median_rate > 0)
            ? ((f.rates.rate - median_rate) / median_rate) * 100
            : 0,
        }));

        return json({
          fork_count: registry.length,
          median_rate,
          last_updated: new Date().toISOString(),
          entries,
        });
      }

      // -----------------------------------------------------------------------
      // Health check
      // -----------------------------------------------------------------------

      if (url.pathname === '/health') {
        return json({ status: 'healthy', fork_id: env.FORK_ID });
      }

      return json({ error: 'not_found' }, 404);

    } catch (e) {
      return json({ error: 'internal_error', detail: e.message }, 500);
    }
  },
};

// =============================================================================
// Helpers
// =============================================================================

function json(data, status = 200) {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json', ...CORS_HEADERS },
  });
}

function currentHourKey() {
  const now = new Date();
  return `${now.getUTCFullYear()}-${now.getUTCMonth()}-${now.getUTCDate()}-${now.getUTCHours()}`;
}

function nextHourISO() {
  const now = new Date();
  now.setUTCHours(now.getUTCHours() + 1, 0, 0, 0);
  return now.toISOString();
}

/**
 * Sign a payload using Ed25519.
 *
 * Uses the Web Crypto API with the SIGNING_KEY secret (base64-encoded
 * Ed25519 private key seed — 32 bytes).
 */
async function signPayload(signingKeyB64, payload) {
  const keyBytes = base64ToBytes(signingKeyB64);

  // Import as Ed25519 signing key (CryptoKey)
  const cryptoKey = await crypto.subtle.importKey(
    'raw',
    keyBytes,
    { name: 'Ed25519' },
    false,
    ['sign']
  );

  const payloadBytes = new TextEncoder().encode(payload);
  const signatureBuffer = await crypto.subtle.sign('Ed25519', cryptoKey, payloadBytes);

  return bytesToBase64(new Uint8Array(signatureBuffer));
}

function base64ToBytes(b64) {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

function bytesToBase64(bytes) {
  let binary = '';
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary);
}
