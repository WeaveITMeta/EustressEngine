/**
 * KYC Jurisdiction Worker — Cloudflare Worker for identity verification
 *
 * Endpoints:
 *   GET  /api/kyc/jurisdiction  — detect user's jurisdiction from CF headers
 *   POST /api/kyc/upload        — upload ID document to R2, verify via Grok 4.2
 *   GET  /api/kyc/status/:id    — check verification status
 *
 * Verification Flow:
 *   1. Document uploaded to R2
 *   2. Grok 4.2 Vision analyzes document in a single call:
 *      - OCR extraction (name, DOB, ID number, expiry)
 *      - Document authenticity check
 *      - Cross-validation against registration data
 *      - Criminal background check (public records)
 *   3. Result stored in KV (verified/rejected with reasons)
 *
 * Secrets:
 *   XAI_API_KEY — xAI API key for Grok 4.2
 */

import JURISDICTIONS from './jurisdictions.json';

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    const cors = corsHeaders(request);

    if (request.method === 'OPTIONS') {
      return new Response(null, { status: 204, headers: cors });
    }

    try {
      if (url.pathname === '/api/kyc/jurisdiction') {
        return handleJurisdiction(request, env, cors);
      }

      if (url.pathname === '/api/kyc/upload' && request.method === 'POST') {
        return handleUpload(request, env, cors, ctx);
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

  return jsonResponse({
    detected: false,
    iso2: country,
    name: `Unknown (${country})`,
    colo: colo,
    accepted_ids: JURISDICTIONS.fallback.require,
    r2_prefix: `kyc-${country.toLowerCase()}-`,
    notes: 'Fallback: enhanced due diligence required',
  }, 200, cors);
}

// ── Document Upload + Grok 4.2 Verification ────────────────────────────────

async function handleUpload(request, env, cors, ctx) {
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
  let fileData, fileName, idType, idNumber, birthday, username, side, sessionId;

  if (contentType.includes('multipart/form-data')) {
    const formData = await request.formData();
    const file = formData.get('document');
    idType = formData.get('id_type') || 'unknown';
    idNumber = formData.get('id_number') || '';
    birthday = formData.get('birthday') || '';
    username = formData.get('username') || '';
    side = formData.get('side') || 'front';
    sessionId = formData.get('session_id') || crypto.randomUUID();

    if (!file || !(file instanceof File)) {
      return jsonResponse({ error: 'Missing document file' }, 400, cors);
    }

    fileName = file.name;
    fileData = await file.arrayBuffer();
  } else {
    idType = request.headers.get('X-ID-Type') || 'unknown';
    idNumber = request.headers.get('X-ID-Number') || '';
    birthday = request.headers.get('X-Birthday') || '';
    username = request.headers.get('X-Username') || '';
    side = request.headers.get('X-Side') || 'front';
    sessionId = request.headers.get('X-Session-ID') || crypto.randomUUID();
    fileName = request.headers.get('X-File-Name') || 'document';
    fileData = await request.arrayBuffer();
  }

  // Hash the document
  const hashBuffer = await crypto.subtle.digest('SHA-256', fileData);
  const hashHex = [...new Uint8Array(hashBuffer)].map(b => b.toString(16).padStart(2, '0')).join('');

  // Upload to R2
  const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
  const objectKey = `${prefix}${timestamp}-${hashHex.substring(0, 16)}.bin`;

  await env.KYC_BUCKET.put(objectKey, fileData, {
    httpMetadata: { contentType: contentType.includes('pdf') ? 'application/pdf' : 'image/jpeg' },
    customMetadata: {
      'loc': country,
      'id-type': idType,
      'side': side,
      'session-id': sessionId,
      'file-name': fileName,
      'timestamp': new Date().toISOString(),
      'hash': hashHex,
    },
  });

  // Generate verification ID
  const verificationId = crypto.randomUUID();

  // Store initial status as "processing"
  await env.KYC_STATUS.put(verificationId, JSON.stringify({
    status: 'processing',
    iso2: country,
    id_type: idType,
    side: side,
    session_id: sessionId,
    r2_key: objectKey,
    uploaded_at: new Date().toISOString(),
    hash: hashHex,
  }), { expirationTtl: 86400 * 90 }); // 90 day TTL

  // Only verify on front-side documents (back is supplementary)
  if (side === 'front' && env.XAI_API_KEY) {
    // Run Grok verification asynchronously (don't block the upload response)
    ctx.waitUntil(verifyWithGrok(env, verificationId, fileData, {
      idType, idNumber, birthday, username, country,
      jurisdictionName: jurisdiction?.name || country,
    }));
  }

  return jsonResponse({
    verification_id: verificationId,
    status: 'processing',
    r2_key: objectKey,
    hash: hashHex,
    jurisdiction: country,
    session_id: sessionId,
  }, 200, cors);
}

// ── Grok 4.2 Vision — Document Verification + Background Check ─────────────

async function verifyWithGrok(env, verificationId, imageData, registrationData) {
  try {
    const base64Image = arrayBufferToBase64(imageData);
    const { idType, idNumber, birthday, username, country, jurisdictionName } = registrationData;

    const systemPrompt = `You are an identity verification and compliance officer for Eustress Engine, a global simulation platform. You must verify identity documents and perform due diligence checks.

Your task is to analyze the provided identity document image and return a structured JSON assessment. You must be thorough, accurate, and conservative — when in doubt, reject.

IMPORTANT: Return ONLY valid JSON. No markdown, no explanation outside the JSON.`;

    const userPrompt = `Analyze this identity document and perform verification.

REGISTRATION DATA (provided by the user during sign-up):
- Username: ${username}
- Claimed ID Type: ${idType}
- Claimed ID Number: ${idNumber || 'not provided'}
- Claimed Date of Birth: ${birthday || 'not provided'}
- Jurisdiction: ${jurisdictionName} (${country})

TASKS — Complete ALL in a single response:

1. DOCUMENT OCR: Extract the following from the document image:
   - Full name (as printed)
   - Date of birth (as printed)
   - Document number / ID number
   - Document type (passport, driver's license, national ID, etc.)
   - Issuing authority / country
   - Expiry date (if visible)
   - Photo present (yes/no)

2. DOCUMENT AUTHENTICITY:
   - Does the document appear genuine? (check for: consistent fonts, proper formatting, security features visible, no obvious digital manipulation, correct layout for claimed document type)
   - Does the document type match the claimed id_type "${idType}"?
   - Is the document expired? (if expiry date is visible)

3. CROSS-VALIDATION:
   - Does the extracted DOB match the claimed birthday "${birthday}"?
   - Does the extracted ID number match the claimed ID number "${idNumber || 'N/A'}"?
   - Is the document from the expected jurisdiction (${country})?

4. CRIMINAL BACKGROUND CHECK (public records assessment):
   - Based on the extracted full name and jurisdiction, assess whether this individual appears on any known public sanctions lists, watchlists, or registries:
     - OFAC SDN List (US Treasury)
     - UN Security Council Consolidated List
     - EU Consolidated Sanctions List
     - Interpol Red Notices (public)
     - PEP (Politically Exposed Person) indicators
   - Note: You are checking based on name match only. Flag if the name is an exact or very close match to any known sanctioned individual.

5. RISK ASSESSMENT:
   - Overall risk level: LOW / MEDIUM / HIGH / CRITICAL
   - Recommended action: VERIFY / REJECT / FLAG

Return this exact JSON structure:
{
  "ocr": {
    "full_name": "",
    "date_of_birth": "",
    "document_number": "",
    "document_type": "",
    "issuing_authority": "",
    "expiry_date": "",
    "photo_present": false
  },
  "authenticity": {
    "appears_genuine": false,
    "type_matches_claim": false,
    "is_expired": false,
    "confidence": 0.0,
    "issues": []
  },
  "cross_validation": {
    "dob_matches": false,
    "id_number_matches": false,
    "jurisdiction_matches": false,
    "discrepancies": []
  },
  "background": {
    "sanctions_match": false,
    "sanctions_lists": [],
    "pep_indicator": false,
    "risk_factors": []
  },
  "decision": {
    "risk_level": "LOW",
    "action": "VERIFY",
    "reasons": [],
    "verified": false
  }
}`;

    const response = await fetch('https://api.x.ai/v1/chat/completions', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${env.XAI_API_KEY}`,
      },
      body: JSON.stringify({
        model: 'grok-4.2',
        messages: [
          { role: 'system', content: systemPrompt },
          {
            role: 'user',
            content: [
              { type: 'text', text: userPrompt },
              {
                type: 'image_url',
                image_url: {
                  url: `data:image/jpeg;base64,${base64Image}`,
                },
              },
            ],
          },
        ],
        temperature: 0.1, // Low temperature for consistent structured output
        max_tokens: 2000,
      }),
    });

    if (!response.ok) {
      const errText = await response.text();
      console.error(`Grok API error (${response.status}): ${errText}`);
      await updateVerificationStatus(env, verificationId, {
        status: 'error',
        error: `Grok API returned ${response.status}`,
      });
      return;
    }

    const result = await response.json();
    const content = result.choices?.[0]?.message?.content || '';

    // Parse the JSON response from Grok
    let verification;
    try {
      // Strip markdown code fences if present
      const jsonStr = content.replace(/```json\n?/g, '').replace(/```\n?/g, '').trim();
      verification = JSON.parse(jsonStr);
    } catch (parseErr) {
      console.error('Failed to parse Grok response:', content);
      await updateVerificationStatus(env, verificationId, {
        status: 'error',
        error: 'Failed to parse verification response',
        raw_response: content.substring(0, 500),
      });
      return;
    }

    // Determine final status based on Grok's decision
    const decision = verification.decision || {};
    const isVerified = decision.action === 'VERIFY' &&
                       decision.risk_level !== 'CRITICAL' &&
                       decision.risk_level !== 'HIGH' &&
                       !verification.background?.sanctions_match;

    // Force reject if sanctions match regardless of other factors
    const finalStatus = verification.background?.sanctions_match
      ? 'rejected'
      : (isVerified ? 'verified' : 'rejected');

    await updateVerificationStatus(env, verificationId, {
      status: finalStatus,
      verified: finalStatus === 'verified',
      verification: verification,
      verified_at: new Date().toISOString(),
      model: 'grok-4.2',
      tokens_used: result.usage?.total_tokens || 0,
    });

    console.log(`KYC ${verificationId}: ${finalStatus} (risk: ${decision.risk_level}, sanctions: ${verification.background?.sanctions_match})`);

  } catch (err) {
    console.error(`Grok verification failed for ${verificationId}:`, err);
    await updateVerificationStatus(env, verificationId, {
      status: 'error',
      error: err.message,
    });
  }
}

// ── Verification Status ─────────────────────────────────────────────────────

async function handleStatus(verificationId, env, cors) {
  const data = await env.KYC_STATUS.get(verificationId);

  if (!data) {
    return jsonResponse({ error: 'Verification not found' }, 404, cors);
  }

  const parsed = JSON.parse(data);

  // Return safe subset (don't expose raw Grok response to client)
  return jsonResponse({
    status: parsed.status,
    verified: parsed.verified || false,
    iso2: parsed.iso2,
    id_type: parsed.id_type,
    uploaded_at: parsed.uploaded_at,
    verified_at: parsed.verified_at || null,
    decision: parsed.verification?.decision || null,
    ocr_name: parsed.verification?.ocr?.full_name || null,
  }, 200, cors);
}

// ── Helpers ─────────────────────────────────────────────────────────────────

async function updateVerificationStatus(env, verificationId, updates) {
  const existing = await env.KYC_STATUS.get(verificationId);
  const data = existing ? JSON.parse(existing) : {};
  const merged = { ...data, ...updates };
  await env.KYC_STATUS.put(verificationId, JSON.stringify(merged), {
    expirationTtl: 86400 * 365, // 1 year for verified/rejected records
  });
}

function arrayBufferToBase64(buffer) {
  const bytes = new Uint8Array(buffer);
  let binary = '';
  for (let i = 0; i < bytes.byteLength; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

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
    'Access-Control-Allow-Headers': 'Content-Type, Authorization, X-ID-Type, X-ID-Number, X-Birthday, X-Username, X-File-Name, X-Side, X-Session-ID',
    'Access-Control-Max-Age': '86400',
  };
}
