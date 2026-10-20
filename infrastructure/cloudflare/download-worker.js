// =============================================================================
// Eustress Downloads Worker — Auth-Gated Release Distribution
// =============================================================================
// Deploy: wrangler deploy
// Bindings required:
//   - DOWNLOADS: R2 bucket (eustress-downloads)
//   - ANALYTICS: Analytics Engine dataset
//   - JWT_SECRET: Secret (same as auth worker uses to sign JWTs)
//
// Two products publish into the one bucket, each under its own key prefix:
// Studio at the root, where every updater already in the field looks, and the
// Player under player/ (.github/workflows/player-release.yml). A staging
// Player publish uses staging/player/ and does not replace player/latest.json.
// Every public route below takes the same optional player/ segment, and the
// same three routes again under staging/player/, so the Player's manifest,
// permanent links and artifacts mirror Studio's one for one. Staging is that
// prefix only: no other key under staging/ is reachable.
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
        return handleLatest(request, env, cors, '');
      }
      if (path === '/player/latest.json') {
        return handleLatest(request, env, cors, 'player/');
      }
      // Staging Player. Same manifest, a different key, so a staging publish
      // cannot replace the production one.
      if (path === '/staging/player/latest.json') {
        return handleLatest(request, env, cors, 'staging/player/');
      }

      // Public: a permanent URL per platform, redirecting to the current
      // version. Docs and the download page can hardcode these and stay
      // correct across releases, which is what stops a version number being
      // retyped into a curl command that goes stale the next time we ship.
      //
      // Platform names are the keys of latest.json, plus the short aliases
      // (windows, macos, mac, linux). The full names keep working.
      // player/ and staging/player/. The optional group is the R2 prefix,
      // empty for Studio. HEAD answers with the same 302 as GET.
      const latest = path.match(/^\/((?:staging\/)?player\/)?latest\/([a-z0-9-]+)$/);
      if (latest && (request.method === 'GET' || request.method === 'HEAD')) {
        const prefix = latest[1] || '';
        const manifestObj = await env.DOWNLOADS.get(`${prefix}latest.json`);
        if (!manifestObj) {
          return jsonResponse({ error: 'No releases available' }, 404, cors);
        }
        const manifest = await manifestObj.json();
        const entry = lookupPlatform(manifest.platforms, latest[2]);
        if (!entry) {
          return jsonResponse({
            error: `Platform '${latest[2]}' not found`,
            available: Object.keys(manifest.platforms || {}),
          }, 404, cors);
        }
        // 302, never 301: the target moves on every release, and a permanent
        // redirect would be cached by clients that then never see a new one.
        return new Response(null, {
          status: 302,
          headers: {
            Location: `/${prefix}v${manifest.version}/${entry.file}`,
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
      // The pattern is the whole boundary. It admits only vX.Y.Z/filename,
      // player/vX.Y.Z/filename and staging/player/vX.Y.Z/filename, so no other
      // key in the bucket is reachable through this route.
      const artifact = path.match(/^\/((?:staging\/)?player\/)?(v\d+\.\d+\.\d+)\/([A-Za-z0-9._-]+)$/);
      if (artifact && (request.method === 'GET' || request.method === 'HEAD')) {
        return serveR2Object(request, env.DOWNLOADS, `${artifact[1] || ''}${artifact[2]}/${artifact[3]}`, {
          'Content-Type': 'application/octet-stream',
          'Content-Disposition': `attachment; filename="${artifact[3]}"`,
          // A version path never changes content, so this is cacheable
          // forever and a repeat download costs the origin nothing.
          'Cache-Control': 'public, max-age=31536000, immutable',
        }, cors);
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

async function handleLatest(request, env, cors, prefix) {
  if (request.method !== 'GET' && request.method !== 'HEAD') {
    return jsonResponse({ error: 'Method not allowed' }, 405, cors);
  }
  return serveR2Object(request, env.DOWNLOADS, `${prefix}latest.json`, {
    'Content-Type': 'application/json',
    'Cache-Control': 'public, max-age=300',
  }, cors, {
    missing: () => jsonResponse({ error: 'No releases available' }, 404, cors),
  });
}

// Short names on /latest/<platform>. An exact manifest key always wins, so
// `windows` is the zip (windows-x64) and never the installer.
const PLATFORM_ALIASES = {
  windows: 'windows-x64',
  macos: 'macos-arm64',
  mac: 'macos-arm64',
  linux: 'linux-x64',
};

function lookupPlatform(platforms, name) {
  if (!platforms || typeof platforms !== 'object') return null;
  if (Object.prototype.hasOwnProperty.call(platforms, name)) return platforms[name];
  const alias = PLATFORM_ALIASES[name];
  if (alias && Object.prototype.hasOwnProperty.call(platforms, alias)) return platforms[alias];
  return null;
}

// One R2 object as an HTTP response. GET returns the bytes. HEAD returns
// Content-Length, Content-Type and ETag and no body. A single `Range:
// bytes=` returns 206, or 416 when the range cannot be satisfied. Every
// file response advertises `Accept-Ranges: bytes`.
async function serveR2Object(request, bucket, key, baseHeaders, cors, options = {}) {
  const missing = options.missing || (() => jsonResponse({ error: 'Not found' }, 404, cors));
  const rangeHeader = request.headers.get('Range');
  const singleRange = request.method === 'GET' ? classifyRange(rangeHeader) : null;
  const needsSize = request.method === 'HEAD' || singleRange;

  let stat = null;
  if (needsSize) {
    stat = await objectStat(bucket, key);
    if (!stat) return missing();
  }

  if (request.method === 'HEAD') {
    return new Response(null, { status: 200, headers: fileHeaders(stat, stat.size, baseHeaders, cors) });
  }

  if (singleRange) {
    const decided = decideRange(singleRange, stat.size);
    if (decided === 'unsatisfiable') {
      return new Response(null, {
        status: 416,
        headers: fileHeaders(stat, 0, {
          ...baseHeaders,
          'Content-Range': `bytes */${stat.size}`,
        }, cors),
      });
    }
    const object = await bucket.get(key, { range: { offset: decided.offset, length: decided.length } });
    if (!object) return missing();
    const end = decided.offset + decided.length - 1;
    return new Response(object.body, {
      status: 206,
      headers: fileHeaders(object, decided.length, {
        ...baseHeaders,
        'Content-Range': `bytes ${decided.offset}-${end}/${stat.size}`,
      }, cors),
    });
  }

  const object = await bucket.get(key);
  if (!object) return missing();
  return new Response(object.body, { status: 200, headers: fileHeaders(object, object.size, baseHeaders, cors) });
}

async function objectStat(bucket, key) {
  if (typeof bucket.head === 'function') return bucket.head(key);
  return bucket.get(key);
}

function fileHeaders(object, length, baseHeaders, cors) {
  const headers = {
    ...baseHeaders,
    'Accept-Ranges': 'bytes',
    'Content-Length': String(length),
    ...cors,
  };
  const etag = object.httpEtag || (object.etag ? `"${object.etag}"` : '');
  if (etag) headers.ETag = etag;
  return headers;
}

// `null` means "not a single byte range" (absent, a different unit, or more
// than one range): the caller serves the whole object. A parsed spec is
// decided against the object size by decideRange.
function classifyRange(header) {
  if (header == null || header === '') return null;
  const trimmed = header.trim();
  if (!/^bytes=/i.test(trimmed) || trimmed.includes(',')) return null;
  const spec = trimmed.slice(trimmed.indexOf('=') + 1).trim();
  if (/^-\d+$/.test(spec) || /^\d+-$/.test(spec) || /^\d+-\d+$/.test(spec)) return spec;
  return 'unsatisfiable';
}

function decideRange(spec, size) {
  const total = Number(size);
  if (!Number.isFinite(total) || total < 0) return 'unsatisfiable';
  if (spec === 'unsatisfiable') return 'unsatisfiable';

  let offset;
  let length;
  if (/^-\d+$/.test(spec)) {
    const n = Number(spec.slice(1));
    if (!Number.isSafeInteger(n) || n <= 0 || total === 0) return 'unsatisfiable';
    length = Math.min(n, total);
    offset = total - length;
  } else if (/^\d+-$/.test(spec)) {
    offset = Number(spec.slice(0, -1));
    if (!Number.isSafeInteger(offset) || offset >= total) return 'unsatisfiable';
    length = total - offset;
  } else if (/^\d+-\d+$/.test(spec)) {
    const [start, endRaw] = spec.split('-').map(Number);
    if (!Number.isSafeInteger(start) || !Number.isSafeInteger(endRaw) || start > endRaw || start >= total) {
      return 'unsatisfiable';
    }
    const end = Math.min(endRaw, total - 1);
    offset = start;
    length = end - start + 1;
  } else {
    return 'unsatisfiable';
  }
  if (length <= 0) return 'unsatisfiable';
  return { offset, length };
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
  const params = new URL(request.url).searchParams;
  const platform = params.get('platform');
  if (!platform) {
    return jsonResponse({ error: 'Missing platform parameter' }, 400, cors);
  }

  // Studio unless ?product=player.
  const product = params.get('product') || 'studio';
  if (product !== 'studio' && product !== 'player') {
    return jsonResponse({ error: `Unknown product '${product}'` }, 400, cors);
  }
  const prefix = product === 'player' ? 'player/' : '';

  // Look up the latest version manifest
  const manifestObj = await env.DOWNLOADS.get(`${prefix}latest.json`);
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
  // or, for the Player, https://downloads.eustress.dev/player/v0.1.0/EustressPlayer-Setup.exe
  const downloadUrl = new URL(platformData.url);
  const r2Key = downloadUrl.pathname.replace(/^\//, ''); // Remove leading slash

  // Only a release artifact, the same shape the public route admits. The
  // manifest is ours, but a bad entry in it must not turn this route into a
  // read of any other key in the bucket.
  if (!/^(player\/)?v\d+\.\d+\.\d+\/[A-Za-z0-9._-]+$/.test(r2Key)) {
    return jsonResponse({ error: 'Release artifact not found in storage' }, 404, cors);
  }

  if (request.method !== 'GET' && request.method !== 'HEAD') {
    return jsonResponse({ error: 'Method not allowed' }, 405, cors);
  }

  const filename = r2Key.split('/').pop();
  const response = await serveR2Object(request, env.DOWNLOADS, r2Key, {
    'Content-Type': 'application/octet-stream',
    'Content-Disposition': `attachment; filename="${filename}"`,
    'X-Version': manifest.version || '',
    'X-Platform': platform,
  }, cors, {
    missing: () => jsonResponse({ error: 'Release artifact not found in storage' }, 404, cors),
  });

  // A ranged or full GET is a download. HEAD is not: it returns no bytes.
  if (env.ANALYTICS && request.method === 'GET' && (response.status === 200 || response.status === 206)) {
    const bytes = Number(response.headers.get('Content-Range')?.split('/')[1] || response.headers.get('Content-Length') || 0);
    ctx.waitUntil(
      env.ANALYTICS.writeDataPoint({
        blobs: [
          platform,
          request.headers.get('cf-ipcountry') || 'XX',
          user.sub || 'unknown',
          manifest.version || 'unknown',
          product,
        ],
        doubles: [1, bytes],
        indexes: [platform],
      })
    );
  }

  return response;
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

  // No query-string token. A credential in a URL is written to access logs,
  // browser history and Referer headers, and the website no longer links here
  // with one: its download buttons go to the public routes above.
  return null;
}

function base64urlToBytes(text) {
  const padded = text.replace(/-/g, '+').replace(/_/g, '/');
  return Uint8Array.from(atob(padded), c => c.charCodeAt(0));
}

async function verifyJWT(token, secret) {
  // No secret means nothing can be verified, so nothing is accepted. A missing
  // binding must read as a closed gate, never as an open one.
  if (typeof secret !== 'string' || secret.length === 0) return null;
  try {
    const parts = token.split('.');
    if (parts.length !== 3) return null;

    const header = JSON.parse(new TextDecoder().decode(base64urlToBytes(parts[0])));
    if (header.alg !== 'HS256') return null;

    // Verify the signature before reading anything the token claims.
    const encoder = new TextEncoder();
    const key = await crypto.subtle.importKey(
      'raw',
      encoder.encode(secret),
      { name: 'HMAC', hash: 'SHA-256' },
      false,
      ['verify']
    );
    const valid = await crypto.subtle.verify(
      'HMAC',
      key,
      base64urlToBytes(parts[2]),
      encoder.encode(`${parts[0]}.${parts[1]}`)
    );
    if (!valid) return null;

    const payload = JSON.parse(new TextDecoder().decode(base64urlToBytes(parts[1])));

    // A token must expire. One without `exp` would be good forever.
    if (typeof payload.exp !== 'number' || payload.exp < Math.floor(Date.now() / 1000)) {
      return null;
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
  // Pages branch `staging`. Without this, the Worker answers
  // Access-Control-Allow-Origin: https://eustress.dev and the staging site
  // cannot read either manifest.
  'https://staging.eustress-cbb.pages.dev',
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
