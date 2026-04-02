/**
 * KYC Jurisdiction Worker — Cloudflare Worker for identity verification
 *
 * Endpoints:
 *   GET  /api/kyc/jurisdiction  — detect user's jurisdiction from CF headers
 *   POST /api/kyc/upload        — upload ID document to R2
 *   GET  /api/kyc/status/:id    — check verification status
 *
 * Uses:
 *   - Cloudflare cf-ipcountry header for jurisdiction detection
 *   - R2 bucket for encrypted document storage
 *   - KV for verification status tracking
 */

// Inline jurisdiction data (loaded from jurisdictions.json at build time)
import JURISDICTIONS from './jurisdictions.json';

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    const cors = corsHeaders(request);

    // CORS preflight
    if (request.method === 'OPTIONS') {
      return new Response(null, { status: 204, headers: cors });
    }

    try {
      if (url.pathname === '/api/kyc/jurisdiction') {
        return handleJurisdiction(request, env, cors);
      }

      if (url.pathname === '/api/kyc/upload' && request.method === 'POST') {
        return handleUpload(request, env, cors);
      }

      if (url.pathname.startsWith('/api/kyc/status/')) {
        const id = url.pathname.split('/').pop();
        return handleStatus(id, env, cors);
      }

      return jsonResponse({ error: 'Not found' }, 404, cors);
    } catch (err) {
      return jsonResponse({ error: err.message }, 500, cors);
    }
  }
};

// ── Jurisdiction Detection ──────────────────────────────────────────────────

function handleJurisdiction(request, env, cors) {
  // Cloudflare sets cf-ipcountry header automatically
  const country = request.headers.get('cf-ipcountry') || 'XX';
  const colo = request.cf?.colo || 'unknown';

  const jurisdiction = JURISDICTIONS.jurisdictions[country];

  if (jurisdiction) {
    return jsonResponse({
      detected: true,
      iso2: country,
      name: jurisdiction.name,
      colo: colo,
      accepted_ids: jurisdiction.natural_ids,
      r2_prefix: jurisdiction.r2_prefix,
      notes: jurisdiction.notes,
      minors: jurisdiction.minors || null,
    }, 200, cors);
  }

  // Fallback — jurisdiction not in ontology
  return jsonResponse({
    detected: false,
    iso2: country,
    name: `Unknown (${country})`,
    colo: colo,
    accepted_ids: JURISDICTIONS.fallback.require,
    r2_prefix: `kyc-${country.toLowerCase()}-`,
    notes: 'Fallback: enhanced due diligence required',
    flag: 'manual_review',
  }, 200, cors);
}

// ── Document Upload to R2 ───────────────────────────────────────────────────

async function handleUpload(request, env, cors) {
  // Require auth token
  const authHeader = request.headers.get('Authorization');
  if (!authHeader || !authHeader.startsWith('Bearer ')) {
    return jsonResponse({ error: 'Unauthorized' }, 401, cors);
  }

  const contentType = request.headers.get('Content-Type') || '';
  if (!contentType.includes('multipart/form-data') && !contentType.includes('application/octet-stream')) {
    return jsonResponse({ error: 'Expected multipart/form-data or application/octet-stream' }, 400, cors);
  }

  const country = request.headers.get('cf-ipcountry') || 'XX';
  const jurisdiction = JURISDICTIONS.jurisdictions[country];
  const prefix = jurisdiction?.r2_prefix || `kyc-${country.toLowerCase()}-`;

  // Parse form data
  let fileData, fileName, idType;

  if (contentType.includes('multipart/form-data')) {
    const formData = await request.formData();
    const file = formData.get('document');
    idType = formData.get('id_type') || 'unknown';

    if (!file || !(file instanceof File)) {
      return jsonResponse({ error: 'Missing document file' }, 400, cors);
    }

    fileName = file.name;
    fileData = await file.arrayBuffer();
  } else {
    idType = request.headers.get('X-ID-Type') || 'unknown';
    fileName = request.headers.get('X-File-Name') || 'document';
    fileData = await request.arrayBuffer();
  }

  // Generate unique object key
  const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
  const hashBuffer = await crypto.subtle.digest('SHA-256', fileData);
  const hashHex = [...new Uint8Array(hashBuffer)].map(b => b.toString(16).padStart(2, '0')).join('');
  const objectKey = `${prefix}${timestamp}-${hashHex.substring(0, 16)}.bin`;

  // Upload to R2
  await env.KYC_BUCKET.put(objectKey, fileData, {
    httpMetadata: { contentType: contentType.includes('pdf') ? 'application/pdf' : 'image/jpeg' },
    customMetadata: {
      'loc': country,
      'id-type': idType,
      'file-name': fileName,
      'timestamp': new Date().toISOString(),
      'hash': hashHex,
    },
  });

  // Store verification status in KV
  const verificationId = crypto.randomUUID();
  await env.KYC_STATUS.put(verificationId, JSON.stringify({
    status: 'pending',
    iso2: country,
    id_type: idType,
    r2_key: objectKey,
    uploaded_at: new Date().toISOString(),
    hash: hashHex,
  }), { expirationTtl: 86400 * 30 }); // 30 day TTL for pending

  return jsonResponse({
    verification_id: verificationId,
    status: 'pending',
    r2_key: objectKey,
    hash: hashHex,
    jurisdiction: country,
  }, 200, cors);
}

// ── Verification Status ─────────────────────────────────────────────────────

async function handleStatus(verificationId, env, cors) {
  const data = await env.KYC_STATUS.get(verificationId);

  if (!data) {
    return jsonResponse({ error: 'Verification not found' }, 404, cors);
  }

  return jsonResponse(JSON.parse(data), 200, cors);
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
    'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type, Authorization, X-ID-Type, X-File-Name',
    'Access-Control-Max-Age': '86400',
  };
}
