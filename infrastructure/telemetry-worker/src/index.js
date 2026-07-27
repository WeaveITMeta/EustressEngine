/**
 * Eustress usage-telemetry ingest Worker.
 *
 * Accepts ONE aggregate per session (not a raw event stream) and writes it to
 * Workers Analytics Engine, which is SQL-queryable and effectively free at
 * launch scale. There is no database to operate and no PII to leak: the
 * client sends counts keyed by tool id, plus a random install UUID that is
 * never joined to an Eustress account.
 *
 * Contract (POST /v1/usage):
 *   {
 *     "install_id": "uuid-v4",
 *     "app_version": "0.1.0",
 *     "mode": "student",              // mode active for most of the session
 *     "counts": { "math:equation_solver": 3, "data:import": 1 },
 *     "wired":  { "math:equation_solver": false, "data:import": true }
 *   }
 *
 * Deliberately rejected designs:
 *   • per-click POSTs — chatty, and would let timing reconstruct a session
 *   • free-text fields — nothing typed by a user is accepted here
 *   • IP logging — WAE blobs below carry no client address
 */

const MAX_BODY_BYTES = 64 * 1024;
const MAX_TOOLS = 2000; // generous vs. the ~1,400 real ids; blocks flooding

export default {
  async fetch(request, env) {
    if (request.method === 'OPTIONS') {
      return new Response(null, { status: 204, headers: cors() });
    }
    if (request.method !== 'POST') {
      return json({ error: 'POST only' }, 405);
    }
    const url = new URL(request.url);
    if (url.pathname !== '/v1/usage') {
      return json({ error: 'not found' }, 404);
    }

    const raw = await request.text();
    if (raw.length > MAX_BODY_BYTES) {
      return json({ error: 'payload too large' }, 413);
    }

    let body;
    try {
      body = JSON.parse(raw);
    } catch {
      return json({ error: 'invalid JSON' }, 400);
    }

    const installId = String(body.install_id || '').slice(0, 64);
    const appVersion = String(body.app_version || 'unknown').slice(0, 32);
    const mode = String(body.mode || 'unknown').slice(0, 64);
    const counts = body.counts && typeof body.counts === 'object' ? body.counts : null;
    const wired = body.wired && typeof body.wired === 'object' ? body.wired : {};
    if (!installId || !counts) {
      return json({ error: 'install_id and counts are required' }, 400);
    }

    const entries = Object.entries(counts).slice(0, MAX_TOOLS);
    for (const [tool, n] of entries) {
      const clicks = Number(n);
      if (!Number.isFinite(clicks) || clicks <= 0) continue;
      env.USAGE.writeDataPoint({
        // Dimensions we group by in SQL.
        blobs: [
          String(tool).slice(0, 128),
          mode,
          appVersion,
          wired[tool] ? 'wired' : 'dream',
        ],
        doubles: [Math.min(clicks, 100000)],
        // Cardinality key: makes "unique installs per tool" countable
        // without storing anything identifying.
        indexes: [installId],
      });
    }

    return json({ ok: true, recorded: entries.length });
  },
};

function cors() {
  return {
    'access-control-allow-origin': '*',
    'access-control-allow-methods': 'POST, OPTIONS',
    'access-control-allow-headers': 'content-type',
  };
}

function json(obj, status = 200) {
  return new Response(JSON.stringify(obj), {
    status,
    headers: { 'content-type': 'application/json', ...cors() },
  });
}
