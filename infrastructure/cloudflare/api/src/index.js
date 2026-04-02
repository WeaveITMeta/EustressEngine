/**
 * Eustress API Worker — api.eustress.dev
 *
 * Unified Cloudflare Worker handling:
 *   - Auth (Ed25519 challenge-response + JWT)
 *   - KYC (jurisdiction detection + R2 document upload)
 *   - Co-signing (witness co-signatures)
 *   - Health check
 *
 * KV Namespaces:
 *   USERS      — user_id → { username, public_key, ... }
 *   CHALLENGES — public_key → { challenge, expires_at }
 *   KYC_STATUS — verification_id → { status, r2_key, ... }
 *
 * R2 Bucket:
 *   KYC_BUCKET — identity documents (kyc-XX-timestamp-hash.bin)
 *
 * Secrets:
 *   JWT_SECRET — persistent JWT signing key
 */

import JURISDICTIONS from '../../kyc/jurisdictions.json';

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    const cors = corsHeaders(request);

    if (request.method === 'OPTIONS') {
      return new Response(null, { status: 204, headers: cors });
    }

    try {
      // Auth
      if (url.pathname === '/api/auth/register' && request.method === 'POST')
        return handleRegister(request, env, cors);
      if (url.pathname === '/api/auth/challenge' && request.method === 'POST')
        return handleChallenge(request, env, cors);
      if (url.pathname === '/api/auth/verify-challenge' && request.method === 'POST')
        return handleVerify(request, env, cors);
      if (url.pathname === '/api/auth/me' && request.method === 'GET')
        return handleMe(request, env, cors);

      // KYC
      if (url.pathname === '/api/kyc/jurisdiction')
        return handleJurisdiction(request, env, cors);
      if (url.pathname === '/api/kyc/upload' && request.method === 'POST')
        return handleKycUpload(request, env, cors);
      if (url.pathname.startsWith('/api/kyc/status/'))
        return handleKycStatus(url.pathname.split('/').pop(), env, cors);

      // Co-sign
      if (url.pathname === '/api/cosign' && request.method === 'POST')
        return handleCosign(request, env, cors);

      // Community
      if (url.pathname === '/api/community/stats')
        return handleCommunityStats(env, cors);
      if (url.pathname === '/api/community/search' && request.method === 'GET')
        return handleCommunitySearch(request, env, cors);
      if (url.pathname === '/api/community/leaderboard')
        return handleCommunityLeaderboard(env, cors);
      if (url.pathname.startsWith('/api/community/users/') && request.method === 'GET')
        return handleUserProfile(url.pathname.split('/').pop(), request, env, cors);

      // Social (authenticated)
      if (url.pathname === '/api/social/follow' && request.method === 'POST')
        return handleFollow(request, env, cors);
      if (url.pathname === '/api/social/unfollow' && request.method === 'POST')
        return handleUnfollow(request, env, cors);
      if (url.pathname === '/api/social/favorite' && request.method === 'POST')
        return handleFavorite(request, env, cors);
      if (url.pathname === '/api/social/unfavorite' && request.method === 'POST')
        return handleUnfavorite(request, env, cors);
      if (url.pathname === '/api/social/play' && request.method === 'POST')
        return handlePlay(request, env, cors);

      // Inventory
      if (url.pathname === '/api/inventory' && request.method === 'GET')
        return handleGetInventory(request, env, cors);

      // Screening (AI risk assessment)
      if (url.pathname === '/api/screening/check' && request.method === 'POST')
        return handleScreeningCheck(request, env, cors);
      if (url.pathname === '/api/screening/status' && request.method === 'GET')
        return handleScreeningStatus(request, env, cors);

      // Admin (requires admin JWT)
      if (url.pathname === '/api/admin/users' && request.method === 'GET')
        return handleAdminListUsers(request, env, cors);
      if (url.pathname === '/api/admin/ban' && request.method === 'POST')
        return handleAdminBan(request, env, cors);
      if (url.pathname === '/api/admin/warn' && request.method === 'POST')
        return handleAdminWarn(request, env, cors);
      if (url.pathname === '/api/admin/review' && request.method === 'POST')
        return handleAdminReview(request, env, cors);
      if (url.pathname === '/api/admin/risk-override' && request.method === 'POST')
        return handleAdminRiskOverride(request, env, cors);
      if (url.pathname === '/api/admin/rescreen' && request.method === 'POST')
        return handleAdminRescreen(request, env, cors);
      if (url.pathname === '/api/admin/screening-report' && request.method === 'GET')
        return handleAdminScreeningReport(request, env, cors);

      // Health
      if (url.pathname === '/health')
        return handleHealth(env, cors);

      return json({ error: 'Not found' }, 404, cors);
    } catch (err) {
      return json({ error: err.message }, 500, cors);
    }
  }
};

// ═══════════════════════════════════════════════════════════════════════════
// AUTH
// ═══════════════════════════════════════════════════════════════════════════

async function handleRegister(request, env, cors) {
  const body = await request.json();
  const { username, public_key, birthday, id_type, id_hash, kyc_session_id } = body;

  if (!username || username.length < 3 || username.length > 32)
    return json({ error: 'Username must be 3-32 characters' }, 400, cors);
  if (!/^[a-zA-Z0-9_]+$/.test(username))
    return json({ error: 'Username: letters, numbers, and _ only' }, 400, cors);
  if (!public_key || public_key.length < 32)
    return json({ error: 'Invalid public key' }, 400, cors);

  // Check username taken
  const existingByName = await env.USERS.get(`username:${username}`);
  if (existingByName) return json({ error: 'Username already taken' }, 409, cors);

  // Check public key already registered
  const existingByKey = await env.USERS.get(`pubkey:${public_key}`);
  if (existingByKey) return json({ error: 'Public key already registered' }, 409, cors);

  // Check ID hash (Sybil protection)
  if (id_hash) {
    const existingByHash = await env.USERS.get(`idhash:${id_hash}`);
    if (existingByHash) return json({ error: 'This ID has already been used to register' }, 409, cors);
  }

  // AI Screening — check criminal record verdicts via Grok before account creation
  if (env.GROK_API_KEY && username && birthday) {
    const screening = await performScreening(username, birthday, id_type, id_hash, env);

    // Store screening result regardless of outcome
    const screeningKey = `screen:${id_hash || public_key}`;
    await env.SCREENING.put(screeningKey, JSON.stringify(screening), { expirationTtl: 86400 * 365 * 7 });

    if (screening.decision === 'DENY') {
      return json({
        error: 'Registration denied based on background screening',
        risk_score: screening.risk_score,
        reason: screening.reason,
        appeal: 'Contact support@eustress.dev to appeal this decision',
      }, 403, cors);
    }

    if (screening.decision === 'REVIEW') {
      // Allow registration but flag for manual review
      // Admin will see this in the screening report
    }
  }

  const user_id = crypto.randomUUID();
  const now = new Date().toISOString();

  // Get screening result if it was performed
  const screeningResult = (env.GROK_API_KEY && (id_hash || public_key))
    ? JSON.parse(await env.SCREENING.get(`screen:${id_hash || public_key}`) || 'null')
    : null;

  const user = {
    id: user_id,
    username,
    public_key,
    birthday: birthday || null,
    id_type: id_type || null,
    id_hash: id_hash || null,
    bliss_balance: 0,
    created_at: now,
    email: null,
    avatar_url: null,
    discord_id: null,
    // Screening
    risk_score: screeningResult?.risk_score || 0,
    risk_decision: screeningResult?.decision || 'UNSCREENED',
    last_screened: screeningResult?.screened_at || null,
    screening_flags: screeningResult?.flags || [],
    banned: false,
    ban_reason: null,
    warnings: [],
  };

  // Store user (multiple indexes for lookup)
  const userData = JSON.stringify(user);
  await env.USERS.put(`user:${user_id}`, userData);
  await env.USERS.put(`username:${username}`, user_id);
  await env.USERS.put(`pubkey:${public_key}`, user_id);
  if (id_hash) await env.USERS.put(`idhash:${id_hash}`, user_id);

  // Link KYC uploads from registration session to this user
  if (kyc_session_id) {
    for (const side of ['front', 'back']) {
      const kycKey = `kyc-${kyc_session_id}-${side}`;
      const kycData = await env.KYC_STATUS.get(kycKey);
      if (kycData) {
        const kyc = JSON.parse(kycData);
        kyc.user_id = user_id;
        kyc.status = 'linked';
        // Re-store under the real user ID
        await env.KYC_STATUS.put(`kyc-${user_id}-${side}`, JSON.stringify(kyc), { expirationTtl: 86400 * 365 * 7 });
        // Clean up session key
        await env.KYC_STATUS.delete(kycKey);
      }
    }
  }

  const token = await createJwt(user_id, env.JWT_SECRET);

  return json({ token, user: publicUser(user) }, 200, cors);
}

async function handleChallenge(request, env, cors) {
  const { public_key } = await request.json();
  if (!public_key) return json({ error: 'Missing public_key' }, 400, cors);

  const challenge = hexEncode(crypto.getRandomValues(new Uint8Array(32)));
  const expires_at = new Date(Date.now() + 5 * 60 * 1000).toISOString();

  await env.CHALLENGES.put(public_key, JSON.stringify({ challenge, expires_at }), {
    expirationTtl: 300
  });

  return json({ challenge, expires_at }, 200, cors);
}

async function handleVerify(request, env, cors) {
  const { public_key, challenge, signature } = await request.json();
  if (!public_key || !challenge || !signature)
    return json({ error: 'Missing fields' }, 400, cors);

  // Get and consume challenge
  const stored = await env.CHALLENGES.get(public_key);
  if (!stored) return json({ error: 'No pending challenge' }, 400, cors);
  await env.CHALLENGES.delete(public_key);

  const parsed = JSON.parse(stored);
  if (parsed.challenge !== challenge)
    return json({ error: 'Challenge mismatch' }, 400, cors);
  if (new Date(parsed.expires_at) < new Date())
    return json({ error: 'Challenge expired' }, 400, cors);

  // Verify Ed25519 signature using Web Crypto
  try {
    const pubKeyBytes = hexDecode(public_key);
    const sigBytes = hexDecode(signature);
    const msgBytes = new TextEncoder().encode(challenge);

    const cryptoKey = await crypto.subtle.importKey(
      'raw', pubKeyBytes, { name: 'Ed25519' }, false, ['verify']
    );
    const valid = await crypto.subtle.verify('Ed25519', cryptoKey, sigBytes, msgBytes);
    if (!valid) return json({ error: 'Signature verification failed' }, 401, cors);
  } catch (e) {
    return json({ error: `Signature error: ${e.message}` }, 400, cors);
  }

  // Find user by public key
  const userId = await env.USERS.get(`pubkey:${public_key}`);
  if (!userId) return json({ error: 'Identity not registered' }, 404, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User data missing' }, 500, cors);

  const user = JSON.parse(userData);

  // Check if user is banned
  if (user.banned) {
    return json({ error: 'Account suspended', reason: user.ban_reason || 'Contact support@eustress.dev' }, 403, cors);
  }

  const token = await createJwt(userId, env.JWT_SECRET);

  return json({ token, user: publicUser(user) }, 200, cors);
}

async function handleMe(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User not found' }, 404, cors);

  return json(publicUser(JSON.parse(userData)), 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// KYC
// ═══════════════════════════════════════════════════════════════════════════

function handleJurisdiction(request, env, cors) {
  const country = request.headers.get('cf-ipcountry') || 'XX';
  const colo = request.cf?.colo || 'unknown';
  const jurisdiction = JURISDICTIONS.jurisdictions[country];

  if (jurisdiction) {
    return json({
      detected: true, iso2: country, name: jurisdiction.name, colo,
      accepted_ids: jurisdiction.natural_ids,
      r2_prefix: jurisdiction.r2_prefix, notes: jurisdiction.notes,
      minors: jurisdiction.minors || null,
    }, 200, cors);
  }

  return json({
    detected: false, iso2: country, name: `Unknown (${country})`, colo,
    accepted_ids: JURISDICTIONS.fallback.require,
    r2_prefix: `kyc-${country.toLowerCase()}-`,
    notes: 'Fallback: enhanced due diligence', flag: 'manual_review',
  }, 200, cors);
}

async function handleKycUpload(request, env, cors) {
  const country = request.headers.get('cf-ipcountry') || 'XX';
  const jurisdiction = JURISDICTIONS.jurisdictions[country];
  const prefix = jurisdiction?.r2_prefix || `kyc-${country.toLowerCase()}-`;

  const formData = await request.formData();
  const side = formData.get('side') || 'front';
  const idType = formData.get('id_type') || 'unknown';
  const sessionId = formData.get('session_id') || crypto.randomUUID();
  const file = formData.get('document');

  if (!file || !(file instanceof File))
    return json({ error: 'Missing document file' }, 400, cors);

  // Authenticated user → use real user ID; registration → use session_id
  let userId = await verifyAuth(request, env);
  const uploadId = userId || `session-${sessionId}`;

  const fileData = await file.arrayBuffer();
  const hashBuffer = await crypto.subtle.digest('SHA-256', fileData);
  const hashHex = hexEncode(new Uint8Array(hashBuffer));
  const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
  const objectKey = `${prefix}${uploadId}/${side}-${timestamp}-${hashHex.substring(0, 16)}.bin`;

  await env.KYC_BUCKET.put(objectKey, fileData, {
    httpMetadata: { contentType: file.type || 'application/octet-stream' },
    customMetadata: {
      'upload-id': uploadId, 'session-id': sessionId,
      'loc': country, 'id-type': idType,
      'side': side, 'file-name': file.name,
      'timestamp': new Date().toISOString(), 'hash': hashHex,
    },
  });

  // Track in KYC_STATUS keyed by session_id so registration can link it
  await env.KYC_STATUS.put(`kyc-${sessionId}-${side}`, JSON.stringify({
    status: 'uploaded', upload_id: uploadId, session_id: sessionId,
    iso2: country, id_type: idType, side,
    r2_key: objectKey, uploaded_at: new Date().toISOString(), hash: hashHex,
  }), { expirationTtl: 86400 * 30 }); // 30 day TTL for pending

  return json({
    session_id: sessionId, side, status: 'uploaded',
    r2_key: objectKey, hash: hashHex,
  }, 200, cors);
}

async function handleKycStatus(verificationId, env, cors) {
  const data = await env.KYC_STATUS.get(verificationId);
  if (!data) return json({ error: 'Not found' }, 404, cors);
  return json(JSON.parse(data), 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// CO-SIGN
// ═══════════════════════════════════════════════════════════════════════════

async function handleCosign(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  // TODO: rate limit, validate contribution, sign with server key
  return json({
    server_signature: 'cosign_placeholder',
    co_signed_at: new Date().toISOString(),
  }, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// HEALTH
// ═══════════════════════════════════════════════════════════════════════════

async function handleHealth(env, cors) {
  return json({
    status: 'ok',
    service: 'eustress-api',
    fork_id: env.FORK_ID || 'eustress.dev',
    timestamp: new Date().toISOString(),
  }, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// JWT (using Web Crypto HMAC-SHA256)
// ═══════════════════════════════════════════════════════════════════════════

async function createJwt(userId, secret) {
  const header = { alg: 'HS256', typ: 'JWT' };
  const payload = {
    sub: userId,
    iat: Math.floor(Date.now() / 1000),
    exp: Math.floor(Date.now() / 1000) + 72 * 3600, // 72 hours
  };

  const enc = new TextEncoder();
  const headerB64 = base64url(JSON.stringify(header));
  const payloadB64 = base64url(JSON.stringify(payload));
  const sigInput = enc.encode(`${headerB64}.${payloadB64}`);

  const key = await crypto.subtle.importKey(
    'raw', enc.encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']
  );
  const sig = await crypto.subtle.sign('HMAC', key, sigInput);
  const sigB64 = base64url(String.fromCharCode(...new Uint8Array(sig)));

  return `${headerB64}.${payloadB64}.${sigB64}`;
}

async function verifyJwt(token, secret) {
  const [headerB64, payloadB64, sigB64] = token.split('.');
  if (!headerB64 || !payloadB64 || !sigB64) return null;

  const enc = new TextEncoder();
  const key = await crypto.subtle.importKey(
    'raw', enc.encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['verify']
  );
  const sigInput = enc.encode(`${headerB64}.${payloadB64}`);
  const sig = base64urlDecode(sigB64);
  const valid = await crypto.subtle.verify('HMAC', key, sig, sigInput);
  if (!valid) return null;

  const payload = JSON.parse(atob(payloadB64.replace(/-/g, '+').replace(/_/g, '/')));
  if (payload.exp < Math.floor(Date.now() / 1000)) return null;
  return payload.sub;
}

async function verifyAuth(request, env) {
  const auth = request.headers.get('Authorization');
  if (!auth || !auth.startsWith('Bearer ')) return null;
  return verifyJwt(auth.slice(7), env.JWT_SECRET);
}

// ═══════════════════════════════════════════════════════════════════════════
// COMMUNITY
// ═══════════════════════════════════════════════════════════════════════════

async function handleCommunityStats(env, cors) {
  // Count registered users by listing KV keys with username: prefix
  // KV list is eventually consistent and returns up to 1000 keys per call
  let userCount = 0;
  let cursor = undefined;
  do {
    const list = await env.USERS.list({ prefix: 'username:', limit: 1000, cursor });
    userCount += list.keys.length;
    cursor = list.list_complete ? undefined : list.cursor;
  } while (cursor);

  return json({
    total_users: userCount,
    total_simulations: 0,  // TODO: query simulations Worker
    total_plays: 0,
    online_now: Math.max(1, userCount), // At least 1 if someone is viewing
    total_bliss_distributed: 0,
    timestamp: new Date().toISOString(),
  }, 200, cors);
}

async function handleCommunitySearch(request, env, cors) {
  const url = new URL(request.url);
  const query = url.searchParams.get('q') || '';
  const limit = Math.min(parseInt(url.searchParams.get('limit') || '10'), 20);

  if (!query || query.length < 2)
    return json({ users: [], query }, 200, cors);

  // Search Cloudflare KV for usernames matching the query
  const results = [];
  const list = await env.USERS.list({ prefix: `username:`, limit: 1000 });

  for (const key of list.keys) {
    const username = key.name.replace('username:', '');
    if (username.toLowerCase().includes(query.toLowerCase())) {
      const userId = await env.USERS.get(key.name);
      if (userId) {
        const userData = await env.USERS.get(`user:${userId}`);
        if (userData) {
          const user = JSON.parse(userData);
          results.push({
            username: user.username,
            display_name: user.username,
            avatar_url: user.avatar_url || null,
            is_verified: !!user.id_hash,
            follower_count: 0,
            created_at: user.created_at,
          });
        }
      }
      if (results.length >= limit) break;
    }
  }

  // If no KV results and Grok is configured, try AI-enhanced search
  if (results.length === 0 && env.GROK_API_KEY) {
    try {
      const grokResults = await searchWithGrok(query, env.GROK_API_KEY);
      if (grokResults) {
        return json({ users: results, query, ai_suggestion: grokResults }, 200, cors);
      }
    } catch (e) {
      // Grok unavailable — return empty results
    }
  }

  return json({ users: results, query }, 200, cors);
}

async function searchWithGrok(query, apiKey) {
  const resp = await fetch('https://api.x.ai/v1/responses', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Authorization': `Bearer ${apiKey}`,
    },
    body: JSON.stringify({
      model: 'grok-4.20-reasoning',
      input: `The user is searching for "${query}" on the Eustress Engine community platform. Eustress is a Rust-based game engine with a Bliss cryptocurrency. Suggest what they might be looking for: a username, a simulation, a feature, or a concept. Reply in 1-2 short sentences only.`,
    }),
  });

  if (!resp.ok) return null;
  const data = await resp.json();
  return data.output?.[0]?.content?.[0]?.text || data.output_text || null;
}

async function handleCommunityLeaderboard(env, cors) {
  // Build leaderboard from KV users sorted by bliss_balance
  const entries = [];
  const list = await env.USERS.list({ prefix: 'user:', limit: 1000 });

  for (const key of list.keys) {
    const userData = await env.USERS.get(key.name);
    if (userData) {
      const user = JSON.parse(userData);
      entries.push({
        username: user.username,
        avatar_url: user.avatar_url || null,
        bliss_balance: user.bliss_balance || 0,
        created_at: user.created_at,
      });
    }
  }

  // Sort by bliss balance descending
  entries.sort((a, b) => b.bliss_balance - a.bliss_balance);
  const top = entries.slice(0, 20).map((e, i) => ({
    rank: i + 1,
    user: { username: e.username, avatar_url: e.avatar_url },
    score: e.bliss_balance,
    score_label: `${e.bliss_balance} BLS`,
  }));

  return json({ entries: top, category: 'bliss', total: entries.length }, 200, cors);
}

async function handleUserProfile(username, request, env, cors) {
  if (!username || username.length < 2)
    return json({ error: 'Invalid username' }, 400, cors);

  const userId = await env.USERS.get(`username:${username}`);
  if (!userId) return json({ error: 'User not found' }, 404, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User data missing' }, 500, cors);

  const user = JSON.parse(userData);
  const created = new Date(user.created_at);
  const joinDate = created.toLocaleDateString('en-US', { month: 'long', year: 'numeric' });

  // Pull social stats from SOCIAL KV
  const followers = JSON.parse(await env.SOCIAL.get(`followers:${userId}`) || '[]');
  const following = JSON.parse(await env.SOCIAL.get(`following:${userId}`) || '[]');
  const favorites = JSON.parse(await env.SOCIAL.get(`favorites:${userId}`) || '[]');

  // Friends = mutual follows
  const followersSet = new Set(followers);
  const friends = following.filter(id => followersSet.has(id));

  // Play counter + simulation count
  const totalPlays = parseInt(await env.SOCIAL.get(`totalPlays:${userId}`) || '0');
  const simCount = parseInt(await env.SOCIAL.get(`simCount:${userId}`) || '0');

  // Inventory count
  const inventory = JSON.parse(await env.INVENTORY.get(`inventory:${userId}`) || '[]');

  // Check if requesting user follows this profile
  let isFollowing = false;
  const viewerUserId = await verifyAuth(request, env);
  if (viewerUserId) {
    const viewerFollowing = JSON.parse(await env.SOCIAL.get(`following:${viewerUserId}`) || '[]');
    isFollowing = viewerFollowing.includes(userId);
  }

  // Compute badges
  const badges = computeBadges(user, followers.length, simCount, totalPlays, inventory.length);

  return json({
    username: user.username,
    display_name: user.username,
    bio: user.bio || '',
    avatar_url: user.avatar_url || null,
    banner_url: null,
    join_date: joinDate,
    follower_count: followers.length,
    following_count: following.length,
    friend_count: friends.length,
    simulation_count: simCount,
    total_plays: totalPlays,
    favorite_count: favorites.length,
    inventory_count: inventory.length,
    badges,
    is_verified: !!user.id_hash,
    is_following: isFollowing,
    discord_linked: false,
    created_at: user.created_at,
  }, 200, cors);
}

function computeBadges(user, followerCount, simCount, totalPlays, inventoryCount) {
  const badges = [];
  const created = new Date(user.created_at);
  const ageMs = Date.now() - created.getTime();
  const ageYears = ageMs / (365.25 * 24 * 60 * 60 * 1000);

  if (user.role === 'admin')
    badges.push({ id: 'admin', name: 'Administrator', icon: '⚡', description: 'Platform administrator' });
  if (user.id_hash)
    badges.push({ id: 'verified', name: 'Verified', icon: '✓', description: 'Identity verified via KYC' });
  if (created < new Date('2030-01-01'))
    badges.push({ id: 'early', name: 'Early Adopter', icon: '🚀', description: 'Joined before 2030' });
  if (ageYears >= 1)
    badges.push({ id: 'veteran', name: 'Veteran', icon: '🏆', description: 'Member for 1+ year' });
  if (simCount >= 1)
    badges.push({ id: 'creator', name: 'Creator', icon: '🎨', description: 'Published 1+ simulation' });
  if (totalPlays >= 1000)
    badges.push({ id: 'popular', name: 'Popular', icon: '⭐', description: '1K+ total plays' });
  if (followerCount >= 100)
    badges.push({ id: 'social', name: 'Social', icon: '🦋', description: '100+ followers' });
  if (inventoryCount >= 10)
    badges.push({ id: 'collector', name: 'Collector', icon: '📦', description: '10+ marketplace items' });

  return badges;
}

// ═══════════════════════════════════════════════════════════════════════════
// SOCIAL — Follow, Unfollow, Favorite, Play Counter
// ═══════════════════════════════════════════════════════════════════════════

async function handleFollow(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const { username } = await request.json();
  const targetId = await env.USERS.get(`username:${username}`);
  if (!targetId) return json({ error: 'User not found' }, 404, cors);
  if (targetId === userId) return json({ error: 'Cannot follow yourself' }, 400, cors);

  // Add to my following
  const following = JSON.parse(await env.SOCIAL.get(`following:${userId}`) || '[]');
  if (!following.includes(targetId)) {
    following.push(targetId);
    await env.SOCIAL.put(`following:${userId}`, JSON.stringify(following));
  }

  // Add me to their followers
  const followers = JSON.parse(await env.SOCIAL.get(`followers:${targetId}`) || '[]');
  if (!followers.includes(userId)) {
    followers.push(userId);
    await env.SOCIAL.put(`followers:${targetId}`, JSON.stringify(followers));
  }

  return json({ success: true, following_count: following.length }, 200, cors);
}

async function handleUnfollow(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const { username } = await request.json();
  const targetId = await env.USERS.get(`username:${username}`);
  if (!targetId) return json({ error: 'User not found' }, 404, cors);

  // Remove from my following
  let following = JSON.parse(await env.SOCIAL.get(`following:${userId}`) || '[]');
  following = following.filter(id => id !== targetId);
  await env.SOCIAL.put(`following:${userId}`, JSON.stringify(following));

  // Remove me from their followers
  let followers = JSON.parse(await env.SOCIAL.get(`followers:${targetId}`) || '[]');
  followers = followers.filter(id => id !== userId);
  await env.SOCIAL.put(`followers:${targetId}`, JSON.stringify(followers));

  return json({ success: true, following_count: following.length }, 200, cors);
}

async function handleFavorite(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const { simulation_id } = await request.json();
  if (!simulation_id) return json({ error: 'Missing simulation_id' }, 400, cors);

  const favorites = JSON.parse(await env.SOCIAL.get(`favorites:${userId}`) || '[]');
  if (!favorites.includes(simulation_id)) {
    favorites.push(simulation_id);
    await env.SOCIAL.put(`favorites:${userId}`, JSON.stringify(favorites));
  }

  return json({ success: true, favorite_count: favorites.length }, 200, cors);
}

async function handleUnfavorite(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const { simulation_id } = await request.json();
  let favorites = JSON.parse(await env.SOCIAL.get(`favorites:${userId}`) || '[]');
  favorites = favorites.filter(id => id !== simulation_id);
  await env.SOCIAL.put(`favorites:${userId}`, JSON.stringify(favorites));

  return json({ success: true, favorite_count: favorites.length }, 200, cors);
}

async function handlePlay(request, env, cors) {
  const { simulation_id, author_id } = await request.json();
  if (!simulation_id) return json({ error: 'Missing simulation_id' }, 400, cors);

  // Increment per-simulation play counter
  const simPlays = parseInt(await env.SOCIAL.get(`plays:${simulation_id}`) || '0') + 1;
  await env.SOCIAL.put(`plays:${simulation_id}`, simPlays.toString());

  // Increment author's total plays
  if (author_id) {
    const authorPlays = parseInt(await env.SOCIAL.get(`totalPlays:${author_id}`) || '0') + 1;
    await env.SOCIAL.put(`totalPlays:${author_id}`, authorPlays.toString());
  }

  return json({ success: true, plays: simPlays }, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// INVENTORY
// ═══════════════════════════════════════════════════════════════════════════

async function handleGetInventory(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const inventory = JSON.parse(await env.INVENTORY.get(`inventory:${userId}`) || '[]');
  return json({ items: inventory, count: inventory.length }, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// AI SCREENING — Grok-powered criminal record verdict analysis
// ═══════════════════════════════════════════════════════════════════════════

/**
 * Perform AI screening via Grok.
 * Analyzes ONLY criminal record VERDICTS (not arrests, charges, or accusations).
 * Returns: { decision: APPROVE|REVIEW|DENY, risk_score: 0-100, reason, flags }
 */
async function performScreening(username, birthday, idType, idHash, env) {
  const screened_at = new Date().toISOString();

  if (!env.GROK_API_KEY) {
    return { decision: 'APPROVE', risk_score: 0, reason: 'Screening unavailable', flags: [], screened_at };
  }

  try {
    const prompt = `You are a background screening AI for Eustress, a game engine platform.
Your job is to assess risk based ONLY on publicly available criminal record VERDICTS (final court decisions).
Do NOT consider arrests, charges, accusations, or pending cases — ONLY convictions/verdicts.

Evaluate this registration:
- Username: ${username}
- Date of Birth: ${birthday}
- ID Type: ${idType || 'not provided'}

Based on publicly available information about this person (if identifiable), assess:
1. Are there any criminal VERDICT records associated with this identity?
2. If yes, what is the severity? (misdemeanor vs felony, violent vs non-violent)
3. What is the recency? (recent vs years ago)

Respond in EXACTLY this JSON format, nothing else:
{
  "risk_score": <0-100>,
  "decision": "<APPROVE|REVIEW|DENY>",
  "reason": "<one sentence explanation>",
  "flags": [<list of specific concerns, empty if none>],
  "verdict_found": <true|false>,
  "details": "<brief details if verdicts found, empty string if none>"
}

Rules:
- risk_score 0-20: APPROVE (clean or minor non-violent misdemeanor 5+ years ago)
- risk_score 21-60: REVIEW (non-violent felony, recent misdemeanor, pattern of offenses)
- risk_score 61-100: DENY (violent felony, sex offense, fraud conviction)
- If you cannot identify the person or find no records: risk_score 0, APPROVE
- A username alone is NOT enough to identify someone — require matching DOB AND full legal name
- Never deny based on name similarity alone
- Err on the side of approval — innocent until proven guilty by verdict
- If you are NOT CERTAIN this is the same person, set verdict_found to false and APPROVE
- Do NOT hallucinate or fabricate records. If unsure, say "no records found"
- A username is a chosen handle, NOT a legal name. Do not search for the username as a real name.
- Only consider PUBLICLY DOCUMENTED court verdicts from official sources`;

    const resp = await fetch('https://api.x.ai/v1/responses', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${env.GROK_API_KEY}`,
      },
      body: JSON.stringify({
        model: 'grok-4.20-reasoning',
        input: prompt,
      }),
    });

    if (!resp.ok) {
      return { decision: 'APPROVE', risk_score: 0, reason: 'Screening service unavailable', flags: [], screened_at };
    }

    const data = await resp.json();
    const responseText = data.output?.[0]?.content?.[0]?.text || data.output_text || '';

    // Parse JSON from response
    const jsonMatch = responseText.match(/\{[\s\S]*\}/);
    if (!jsonMatch) {
      return { decision: 'APPROVE', risk_score: 0, reason: 'Could not parse screening response', flags: [], screened_at };
    }

    const result = JSON.parse(jsonMatch[0]);

    return {
      decision: result.decision || 'APPROVE',
      risk_score: Math.min(100, Math.max(0, result.risk_score || 0)),
      reason: result.reason || 'No concerns found',
      flags: result.flags || [],
      verdict_found: result.verdict_found || false,
      details: result.details || '',
      screened_at,
      model: 'grok-4.20-reasoning',
    };
  } catch (e) {
    return { decision: 'APPROVE', risk_score: 0, reason: `Screening error: ${e.message}`, flags: [], screened_at };
  }
}

// Pre-registration screening check (can be called separately before register)
async function handleScreeningCheck(request, env, cors) {
  const { username, birthday, id_type, id_hash } = await request.json();
  if (!username || !birthday)
    return json({ error: 'Username and birthday required for screening' }, 400, cors);

  // Rate limit: max 5 screening checks per IP per hour
  const clientIp = request.headers.get('cf-connecting-ip') || 'unknown';
  const rateLimitKey = `screen-rate:${clientIp}`;
  const currentCount = parseInt(await env.SCREENING.get(rateLimitKey) || '0');
  if (currentCount >= 5) {
    return json({ error: 'Rate limit exceeded. Try again later.' }, 429, cors);
  }
  await env.SCREENING.put(rateLimitKey, (currentCount + 1).toString(), { expirationTtl: 3600 });

  const result = await performScreening(username, birthday, id_type, id_hash, env);

  // Add legal disclaimer
  result.disclaimer = 'AI-assisted screening. Not a legal determination. Based on publicly available information only. Results may contain errors. Contact support@eustress.dev to dispute.';

  // Store result
  const key = `screen:${id_hash || username}`;
  await env.SCREENING.put(key, JSON.stringify(result), { expirationTtl: 86400 * 365 });

  return json(result, 200, cors);
}

// Get screening status for a user
async function handleScreeningStatus(request, env, cors) {
  const url = new URL(request.url);
  const userId = url.searchParams.get('user_id');
  const idHash = url.searchParams.get('id_hash');

  if (!userId && !idHash)
    return json({ error: 'user_id or id_hash required' }, 400, cors);

  // Check by user data
  if (userId) {
    const userData = await env.USERS.get(`user:${userId}`);
    if (userData) {
      const user = JSON.parse(userData);
      return json({
        risk_score: user.risk_score || 0,
        decision: user.risk_decision || 'UNSCREENED',
        last_screened: user.last_screened || null,
        flags: user.screening_flags || [],
      }, 200, cors);
    }
  }

  // Check by screening record
  const key = `screen:${idHash || userId}`;
  const data = await env.SCREENING.get(key);
  if (data) return json(JSON.parse(data), 200, cors);

  return json({ error: 'No screening record found' }, 404, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// ADMIN — Moderation tools (requires admin role)
// ═══════════════════════════════════════════════════════════════════════════

// Admin user IDs (hardcoded for now — move to KV later)
const ADMIN_USERS = new Set([
  // Add your user ID here after registering
]);

async function requireAdmin(request, env) {
  const userId = await verifyAuth(request, env);
  if (!userId) return null;

  // Check admin role in KV
  const adminFlag = await env.USERS.get(`admin:${userId}`);
  if (adminFlag) return userId;

  // Check user record for admin role
  const userData = await env.USERS.get(`user:${userId}`);
  if (userData) {
    const user = JSON.parse(userData);
    if (user.role === 'admin') return userId;
  }

  return null; // Not an admin
}

async function auditLog(env, action, adminId, target, details) {
  const entry = {
    action,
    admin_id: adminId,
    target,
    details,
    timestamp: new Date().toISOString(),
  };
  const key = `audit:${Date.now()}-${crypto.randomUUID().slice(0, 8)}`;
  await env.AUDIT_LOG.put(key, JSON.stringify(entry), { expirationTtl: 86400 * 365 * 5 }); // 5 year retention
}

async function handleAdminListUsers(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const list = await env.USERS.list({ prefix: 'user:', limit: 100 });
  const users = [];
  for (const key of list.keys) {
    const data = await env.USERS.get(key.name);
    if (data) {
      const user = JSON.parse(data);
      users.push({
        id: user.id, username: user.username,
        risk_score: user.risk_score || 0, risk_decision: user.risk_decision || 'UNSCREENED',
        banned: user.banned || false, created_at: user.created_at,
        warnings: (user.warnings || []).length,
      });
    }
  }
  return json({ users, total: users.length }, 200, cors);
}

async function handleAdminBan(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const { username, reason } = await request.json();
  const userId = await env.USERS.get(`username:${username}`);
  if (!userId) return json({ error: 'User not found' }, 404, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User data missing' }, 500, cors);

  const user = JSON.parse(userData);
  user.banned = true;
  user.ban_reason = reason || 'Banned by admin';
  user.banned_at = new Date().toISOString();
  user.banned_by = adminId;

  await env.USERS.put(`user:${userId}`, JSON.stringify(user));
  await auditLog(env, 'BAN', adminId, username, { reason: user.ban_reason });

  return json({ success: true, username, banned: true, reason: user.ban_reason }, 200, cors);
}

async function handleAdminWarn(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const { username, message } = await request.json();
  const userId = await env.USERS.get(`username:${username}`);
  if (!userId) return json({ error: 'User not found' }, 404, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User data missing' }, 500, cors);

  const user = JSON.parse(userData);
  if (!user.warnings) user.warnings = [];
  user.warnings.push({
    message: message || 'Warning from admin',
    issued_at: new Date().toISOString(),
    issued_by: adminId,
  });

  await env.USERS.put(`user:${userId}`, JSON.stringify(user));
  await auditLog(env, 'WARN', adminId, username, { message: message || 'Warning from admin' });

  return json({ success: true, username, warning_count: user.warnings.length }, 200, cors);
}

async function handleAdminReview(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const { username, decision, notes } = await request.json();
  const userId = await env.USERS.get(`username:${username}`);
  if (!userId) return json({ error: 'User not found' }, 404, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User data missing' }, 500, cors);

  const user = JSON.parse(userData);
  user.risk_decision = decision || user.risk_decision;
  user.admin_review = {
    reviewed_by: adminId,
    reviewed_at: new Date().toISOString(),
    decision,
    notes: notes || '',
  };

  await env.USERS.put(`user:${userId}`, JSON.stringify(user));
  await auditLog(env, 'REVIEW', adminId, username, { decision, notes: notes || '' });

  return json({ success: true, username, decision }, 200, cors);
}

async function handleAdminRiskOverride(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const { username, risk_score, decision, reason } = await request.json();
  const userId = await env.USERS.get(`username:${username}`);
  if (!userId) return json({ error: 'User not found' }, 404, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User data missing' }, 500, cors);

  const user = JSON.parse(userData);
  user.risk_score = risk_score ?? user.risk_score;
  user.risk_decision = decision || user.risk_decision;
  user.risk_override = {
    overridden_by: adminId,
    overridden_at: new Date().toISOString(),
    previous_score: user.risk_score,
    new_score: risk_score,
    reason: reason || 'Admin override',
  };

  await env.USERS.put(`user:${userId}`, JSON.stringify(user));
  await auditLog(env, 'RISK_OVERRIDE', adminId, username, { risk_score, decision, reason });

  return json({ success: true, username, risk_score, decision }, 200, cors);
}

// Re-screen a specific user
async function handleAdminRescreen(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const { username } = await request.json();
  const userId = await env.USERS.get(`username:${username}`);
  if (!userId) return json({ error: 'User not found' }, 404, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User data missing' }, 500, cors);

  const user = JSON.parse(userData);

  // Cooldown: minimum 30 days between re-screens
  if (user.last_screened) {
    const lastScreened = new Date(user.last_screened);
    const daysSince = (Date.now() - lastScreened.getTime()) / (86400 * 1000);
    if (daysSince < 30) {
      return json({
        error: `Re-screening cooldown: ${Math.ceil(30 - daysSince)} days remaining`,
        last_screened: user.last_screened,
      }, 429, cors);
    }
  }

  // Re-screen with Grok
  const result = await performScreening(user.username, user.birthday, user.id_type, user.id_hash, env);

  // Track delta
  const previousScore = user.risk_score || 0;
  const delta = result.risk_score - previousScore;

  user.risk_score = result.risk_score;
  user.risk_decision = result.decision;
  user.last_screened = result.screened_at;
  user.screening_flags = result.flags;
  if (!user.screening_history) user.screening_history = [];
  user.screening_history.push({
    date: result.screened_at,
    score: result.risk_score,
    decision: result.decision,
    delta,
    triggered_by: 'admin_rescreen',
  });

  await env.USERS.put(`user:${userId}`, JSON.stringify(user));

  // Store screening record
  await env.SCREENING.put(`screen:${user.id_hash || user.public_key}`, JSON.stringify(result));

  return json({
    success: true, username, ...result,
    delta, previous_score: previousScore,
  }, 200, cors);
}

// Get screening report — all users with risk scores
async function handleAdminScreeningReport(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const list = await env.USERS.list({ prefix: 'user:', limit: 1000 });
  const report = { total: 0, approved: 0, review: 0, denied: 0, unscreened: 0, banned: 0, users: [] };

  for (const key of list.keys) {
    const data = await env.USERS.get(key.name);
    if (!data) continue;
    const user = JSON.parse(data);
    report.total++;

    const decision = user.risk_decision || 'UNSCREENED';
    if (decision === 'APPROVE') report.approved++;
    else if (decision === 'REVIEW') report.review++;
    else if (decision === 'DENY') report.denied++;
    else report.unscreened++;
    if (user.banned) report.banned++;

    report.users.push({
      id: user.id, username: user.username,
      risk_score: user.risk_score || 0,
      risk_decision: decision,
      last_screened: user.last_screened || null,
      flags: user.screening_flags || [],
      banned: user.banned || false,
      warnings: (user.warnings || []).length,
      screening_history: user.screening_history || [],
    });
  }

  // Sort by risk score descending
  report.users.sort((a, b) => b.risk_score - a.risk_score);

  return json(report, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════════════════

function publicUser(user) {
  return {
    id: user.id,
    username: user.username,
    email: user.email || null,
    avatar_url: user.avatar_url || null,
    discord_id: user.discord_id || null,
    bliss_balance: user.bliss_balance || 0,
    created_at: user.created_at,
  };
}

function json(data, status, headers) {
  return new Response(JSON.stringify(data), {
    status, headers: { ...headers, 'Content-Type': 'application/json' },
  });
}

function corsHeaders(request) {
  return {
    'Access-Control-Allow-Origin': request.headers.get('Origin') || '*',
    'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type, Authorization, X-ID-Type',
    'Access-Control-Max-Age': '86400',
  };
}

function hexEncode(bytes) {
  return [...bytes].map(b => b.toString(16).padStart(2, '0')).join('');
}

function hexDecode(hex) {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2)
    bytes[i / 2] = parseInt(hex.substr(i, 2), 16);
  return bytes;
}

function base64url(str) {
  return btoa(str).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function base64urlDecode(str) {
  const b64 = str.replace(/-/g, '+').replace(/_/g, '/');
  const binary = atob(b64);
  return new Uint8Array([...binary].map(c => c.charCodeAt(0)));
}
