// =============================================================================
// Eustress Downloads Worker — Auth-Gated Release Distribution
// =============================================================================
// Deploy: wrangler deploy
// Bindings required:
//   - RELEASES: R2 bucket (eustress-releases)
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
      // Public: latest.json manifest (no auth needed — engine updater reads this)
      if (path === '/api/releases/latest' || path === '/api/latest') {
        return handleLatest(env, cors);
      }

      // Auth-gated: download release artifact
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
  // URL format: https://releases.eustress.dev/v0.3.5/eustress-engine-v0.3.5-windows-x64.zip
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

function corsHeaders(request) {
  return {
    'Access-Control-Allow-Origin': request.headers.get('Origin') || '*',
    'Access-Control-Allow-Methods': 'GET, HEAD, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type, Authorization',
    'Access-Control-Max-Age': '86400',
  };
}
