// =============================================================================
// Eustress Downloads Worker — Auth-Gated Release Distribution
// =============================================================================
// Deploy: wrangler deploy
// Bindings required:
//   - DOWNLOADS: R2 bucket (eustress-downloads)
//   - ANALYTICS: Analytics Engine dataset
//   - JWT_SECRET: Secret (same as auth worker uses to sign JWTs)
// =============================================================================

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    const path = url.pathname;
    const cors = corsHeaders(request);

    if (request.method === 'OPTIONS') {
      return new Response(null, { status: 204, headers: cors });
    }

    try {
      // Public: latest.json manifest.
      //
      // updater.rs compiles this URL in as a constant, so it is fixed in every
      // copy already in the field and cannot be changed by shipping a new
      // build: the only mechanism that would deliver that build is the updater
      // itself. The other two spellings are kept because downloads.eustress.dev
      // has served them since 2025-12.
      if (path === '/latest.json' || path === '/api/releases/latest' || path === '/api/latest') {
        return handleLatest(env, cors);
      }

      // Public: a permanent URL per platform, redirecting to the current
      // version. Docs and the download page can hardcode these and stay
      // correct across releases, which is what stops a version number being
      // retyped into a curl command that goes stale the next time we ship.
      //
      // Platform names are the keys of latest.json, so this cannot drift from
      // what the build actually produced.
      const latest = path.match(/^\/latest\/([a-z0-9-]+)$/);
      if (latest) {
        const manifestObj = await env.DOWNLOADS.get('latest.json');
        if (!manifestObj) {
          return jsonResponse({ error: 'No releases available' }, 404, cors);
        }
        const manifest = await manifestObj.json();
        const entry = manifest.platforms?.[latest[1]];
        if (!entry) {
          return jsonResponse({
            error: `Platform '${latest[1]}' not found`,
            available: Object.keys(manifest.platforms || {}),
          }, 404, cors);
        }
        // 302, never 301: the target moves on every release, and a permanent
        // redirect would be cached by clients that then never see a new one.
        return new Response(null, {
          status: 302,
          headers: {
            Location: `/v${manifest.version}/${entry.file}`,
            'Cache-Control': 'public, max-age=60',
            ...cors,
          },
        });
      }

      // Public: release artifacts, addressed by version and filename.
      //
      // The updater cannot hold a credential, so the bytes it fetches have to
      // be reachable without one. The gate below stays for the website's
      // sign-in-to-download flow, which buys attribution and per-user
      // analytics rather than secrecy: the build is free either way.
      //
      // The pattern is the whole boundary. It admits only vX.Y.Z/filename, so
      // no other key in the bucket is reachable through this route.
      const artifact = path.match(/^\/(v\d+\.\d+\.\d+)\/([A-Za-z0-9._-]+)$/);
      if (artifact && request.method === 'GET') {
        const object = await env.DOWNLOADS.get(`${artifact[1]}/${artifact[2]}`);
        if (!object) return jsonResponse({ error: 'Not found' }, 404, cors);
        return new Response(object.body, {
          headers: {
            'Content-Type': 'application/octet-stream',
            'Content-Disposition': `attachment; filename="${artifact[2]}"`,
            // A version path never changes content, so this is cacheable
            // forever and a repeat download costs the origin nothing.
            'Cache-Control': 'public, max-age=31536000, immutable',
            'ETag': object.httpEtag,
            ...cors,
          },
        });
      }

      // Auth-gated: the website's download flow.
      if (path === '/api/releases/download') {
        return handleDownload(request, env, ctx, cors);
      }

      // Public: download stats
      if (path === '/api/releases/stats') {
        return handleStats(env, cors);
      }

      return jsonResponse({ error: 'Not found' }, 404, cors);
    } catch (err) {
      return jsonResponse({ error: err.message }, 500, cors);
    }
  }
};

// ── Latest manifest (public) ────────────────────────────────────────────────

async function handleLatest(env, cors) {
  const object = await env.DOWNLOADS.get('latest.json');
  if (!object) {
    return jsonResponse({ error: 'No releases available' }, 404, cors);
  }
  return new Response(object.body, {
    headers: {
      'Content-Type': 'application/json',
      'Cache-Control': 'public, max-age=300',
      ...cors,
    },
  });
}

// ── Auth-gated download ─────────────────────────────────────────────────────

async function handleDownload(request, env, ctx, cors) {
  // Validate JWT from Authorization header or cookie
  const token = extractToken(request);
  if (!token) {
    return jsonResponse({
      error: 'Authentication required',
      message: 'Sign in at eustress.dev to download',
    }, 401, cors);
  }

  const user = await verifyJWT(token, env.JWT_SECRET);
  if (!user) {
    return jsonResponse({
      error: 'Invalid or expired token',
      message: 'Please sign in again at eustress.dev',
    }, 403, cors);
  }

  // Get platform from query string
  const platform = new URL(request.url).searchParams.get('platform');
  if (!platform) {
    return jsonResponse({ error: 'Missing platform parameter' }, 400, cors);
  }

  // Look up the latest version manifest
  const manifestObj = await env.DOWNLOADS.get('latest.json');
  if (!manifestObj) {
    return jsonResponse({ error: 'No releases available' }, 404, cors);
  }

  const manifest = await manifestObj.json();
  const platformData = manifest.platforms?.[platform];
  if (!platformData) {
    return jsonResponse({
      error: `Platform '${platform}' not found`,
      available: Object.keys(manifest.platforms || {}),
    }, 404, cors);
  }

  // Extract R2 key from the URL
  // URL format: https://downloads.eustress.dev/v0.3.5/eustress-engine-v0.3.5-windows-x64.zip
  const downloadUrl = new URL(platformData.url);
  const r2Key = downloadUrl.pathname.replace(/^\//, ''); // Remove leading slash

  // Fetch from R2
  const object = await env.DOWNLOADS.get(r2Key);
  if (!object) {
    return jsonResponse({ error: 'Release artifact not found in storage' }, 404, cors);
  }

  // Log download analytics
  if (env.ANALYTICS) {
    ctx.waitUntil(
      env.ANALYTICS.writeDataPoint({
        blobs: [
          platform,
          request.headers.get('cf-ipcountry') || 'XX',
          user.sub || 'unknown',
          manifest.version || 'unknown',
        ],
        doubles: [1, object.size],
        indexes: [platform],
      })
    );
  }

  const filename = r2Key.split('/').pop();
  return new Response(object.body, {
    headers: {
      'Content-Type': 'application/octet-stream',
      'Content-Disposition': `attachment; filename="${filename}"`,
      'Content-Length': object.size.toString(),
      'X-Version': manifest.version || '',
      'X-Platform': platform,
      ...cors,
    },
  });
}

// ── Stats (public) ──────────────────────────────────────────────────────────

async function handleStats(env, cors) {
  return jsonResponse({
    message: 'Download analytics available via Cloudflare Dashboard',
  }, 200, cors);
}

// ── JWT Verification ────────────────────────────────────────────────────────

function extractToken(request) {
  // Check Authorization header first
  const auth = request.headers.get('Authorization');
  if (auth && auth.startsWith('Bearer ')) {
    return auth.slice(7);
  }

  // Check cookie fallback
  const cookies = request.headers.get('Cookie') || '';
  const match = cookies.match(/auth_token=([^;]+)/);
  if (match) return match[1];

  // Check query param (for direct download links)
  const url = new URL(request.url);
  const tokenParam = url.searchParams.get('token');
  if (tokenParam) return tokenParam;

  return null;
}

async function verifyJWT(token, secret) {
  try {
    const parts = token.split('.');
    if (parts.length !== 3) return null;

    const header = JSON.parse(atob(parts[0]));
    const payload = JSON.parse(atob(parts[1]));

    // Check expiry
    if (payload.exp && payload.exp < Math.floor(Date.now() / 1000)) {
      return null; // Expired
    }

    // Verify signature using Web Crypto API
    if (secret) {
      const encoder = new TextEncoder();
      const key = await crypto.subtle.importKey(
        'raw',
        encoder.encode(secret),
        { name: 'HMAC', hash: 'SHA-256' },
        false,
        ['verify']
      );

      const signatureBytes = Uint8Array.from(
        atob(parts[2].replace(/-/g, '+').replace(/_/g, '/')),
        c => c.charCodeAt(0)
      );

      const valid = await crypto.subtle.verify(
        'HMAC',
        key,
        signatureBytes,
        encoder.encode(`${parts[0]}.${parts[1]}`)
      );

      if (!valid) return null;
    }

    return payload;
  } catch {
    return null;
  }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

function jsonResponse(data, status, headers) {
  return new Response(JSON.stringify(data), {
    status,
    headers: { ...headers, 'Content-Type': 'application/json' },
  });
}

const ALLOWED_ORIGINS = new Set([
  'https://eustress.dev',
  'https://www.eustress.dev',
  'http://localhost:3000',
  'http://127.0.0.1:3000',
]);

function corsHeaders(request) {
  const origin = request.headers.get('Origin');
  const allow = origin && ALLOWED_ORIGINS.has(origin) ? origin : 'https://eustress.dev';
  return {
    'Access-Control-Allow-Origin': allow,
    'Vary': 'Origin',
    'Access-Control-Allow-Methods': 'GET, HEAD, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type, Authorization',
    'Access-Control-Max-Age': '86400',
    'Strict-Transport-Security': 'max-age=31536000; includeSubDomains; preload',
    'X-Content-Type-Options': 'nosniff',
  };
}
