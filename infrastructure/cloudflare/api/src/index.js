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

// KYC jurisdiction data — inline fallback (was external JSON, removed)
const JURISDICTIONS = {
  jurisdictions: {},
  fallback: { require: ['passport', 'national_id', 'drivers_license'] },
};

export default {
  // Daily payout cron — runs at UTC midnight
  async scheduled(event, env, ctx) {
    ctx.waitUntil(runDailyPayout(env));
  },

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

      // Identity backup email
      if (url.pathname === '/api/identity/email-backup' && request.method === 'POST')
        return handleEmailIdentityBackup(request, env, cors);

      // Workshop Context (persistent memories + rules across devices)
      if (url.pathname === '/api/workshop/context' && request.method === 'GET')
        return handleGetWorkshopContext(request, env, cors);
      if (url.pathname === '/api/workshop/context' && request.method === 'PUT')
        return handlePutWorkshopContext(request, env, cors);
      if (url.pathname === '/api/workshop/context/memory' && request.method === 'POST')
        return handleAddWorkshopMemory(request, env, cors);

      // KYC
      if (url.pathname === '/api/kyc/jurisdiction')
        return handleJurisdiction(request, env, cors);
      if (url.pathname === '/api/kyc/upload' && request.method === 'POST')
        return handleKycUpload(request, env, cors);
      if (url.pathname === '/api/kyc/submit' && request.method === 'POST')
        return handleKycSubmit(request, env, cors);
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

      // Tickets
      if (url.pathname === '/api/tickets/packages' && request.method === 'GET')
        return handleTicketPackages(env, cors);
      if (url.pathname === '/api/tickets/balance' && request.method === 'GET')
        return handleTicketBalance(request, env, cors);
      if (url.pathname === '/api/tickets/checkout' && request.method === 'POST')
        return handleTicketCheckout(request, env, cors);
      if (url.pathname === '/api/tickets/spend' && request.method === 'POST')
        return handleTicketSpend(request, env, cors);
      if (url.pathname === '/api/tickets/history' && request.method === 'GET')
        return handleTicketHistory(request, env, cors);

      // Stripe
      if (url.pathname === '/api/stripe/checkout' && request.method === 'POST')
        return handleStripeCheckout(request, env, cors);
      if (url.pathname === '/api/stripe/webhook' && request.method === 'POST')
        return handleStripeWebhook(request, env);
      if (url.pathname === '/api/stripe/connect/onboard' && request.method === 'POST')
        return handleStripeConnectOnboard(request, env, cors);
      if (url.pathname === '/api/stripe/connect/status' && request.method === 'GET')
        return handleStripeConnectStatus(request, env, cors);

      // Payouts
      if (url.pathname === '/api/payouts/daily' && request.method === 'POST')
        return handleDailyPayout(request, env, cors);
      if (url.pathname === '/api/payouts/history' && request.method === 'GET')
        return handlePayoutHistory(request, env, cors);
      if (url.pathname === '/api/payouts/rate' && request.method === 'GET')
        return handlePayoutRate(env, cors);

      // Node heartbeat
      if (url.pathname === '/api/node/heartbeat' && request.method === 'POST')
        return handleNodeHeartbeat(request, env, cors);
      if (url.pathname === '/api/node/stats' && request.method === 'GET')
        return handleNodeStats(env, cors);

      // Simulations (published)
      if (url.pathname === '/api/simulations' && request.method === 'GET')
        return handleListSimulations(env, cors);
      if (url.pathname === '/api/simulations/publish' && request.method === 'POST')
        return handlePublishSimulation(request, env, cors);
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+\/space$/) && request.method === 'PUT')
        return handleUploadScene(request, url.pathname.split('/')[3], env, cors);
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+\/space\/multipart\/create$/) && request.method === 'POST')
        return handleMultipartCreate(request, url.pathname.split('/')[3], env, cors);
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+\/space\/multipart\/part$/) && request.method === 'PUT')
        return handleMultipartPart(request, url.pathname.split('/')[3], env, cors);
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+\/space\/multipart\/complete$/) && request.method === 'POST')
        return handleMultipartComplete(request, url.pathname.split('/')[3], env, cors);
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+\/spaces\/[^/]+$/) && request.method === 'PUT')
        return handleUploadSingleSpace(request, url.pathname.split('/')[3], url.pathname.split('/')[5], env, cors);
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+\/thumbnail$/) && request.method === 'PUT')
        return handleUploadThumbnail(request, url.pathname.split('/')[3], env, cors);
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+\/download$/) && request.method === 'GET')
        return handleDownloadPak(request, url.pathname.split('/')[3], env, cors);
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+\/play$/) && request.method === 'POST')
        return handlePlaySimulation(request, url.pathname.split('/')[3], env, cors);
      if (url.pathname.match(/^\/api\/simulations\/[a-f0-9-]+$/) && request.method === 'GET')
        return handleGetSimulation(url.pathname.split('/').pop(), env, cors);

      // Accounting
      if (url.pathname === '/api/accounting/dashboard' && request.method === 'GET')
        return handleAccountingDashboard(request, env, cors);
      if (url.pathname === '/api/accounting/costs' && request.method === 'POST')
        return handleRecordCost(request, env, cors);

      // Gallery (frontend-facing aliases for simulations)
      if (url.pathname === '/api/gallery/featured' && request.method === 'GET')
        return json({ featured: [], timestamp: new Date().toISOString() }, 200, cors);
      if (url.pathname === '/api/gallery' && request.method === 'GET')
        return handleListSimulations(env, cors);

      // API Keys management
      if (url.pathname === '/api/keys' && request.method === 'GET')
        return json({ keys: [] }, 200, cors);
      if (url.pathname === '/api/keys' && request.method === 'POST')
        return json({ key: 'ek_' + crypto.randomUUID().replace(/-/g, ''), id: crypto.randomUUID(), name: 'New Key' }, 201, cors);

      // Marketplace (stub — not yet implemented)
      if (url.pathname.startsWith('/api/marketplace'))
        return json({ items: [], total: 0, page: 1 }, 200, cors);

      // Projects — returns the authenticated user's published simulations
      if (url.pathname === '/api/projects' && request.method === 'GET') {
        return handleUserProjects(request, url, env, cors);
      }
      if (url.pathname === '/api/projects/recent' && request.method === 'GET') {
        return handleUserProjects(request, url, env, cors);
      }

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
  const { username, public_key, birthday, id_type, id_hash, kyc_session_id, email } = body;

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
    email: email || null,
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

// ── Identity Backup Email ───────────────────────────────────────────────────
async function handleEmailIdentityBackup(request, env, cors) {
  const body = await request.json();
  const { email, username, toml_content } = body;

  if (!email || !email.includes('@'))
    return json({ error: 'Valid email required' }, 400, cors);
  if (!toml_content || toml_content.length < 50)
    return json({ error: 'Invalid TOML content' }, 400, cors);

  // Use Cloudflare Email Workers (MailChannels API — free for Workers)
  const htmlBody = `
<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
</head>
<body style="margin:0;padding:0;background:#0d1117;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;">
  <div style="max-width:600px;margin:0 auto;padding:40px 20px;">
    <div style="background:#161b22;border-radius:12px;border:1px solid #30363d;overflow:hidden;">
      <!-- Header -->
      <div style="background:linear-gradient(135deg,#1a1a2e 0%,#16213e 100%);padding:32px;text-align:center;">
        <h1 style="margin:0;color:#f0f6fc;font-size:24px;font-weight:700;">Eustress Identity Backup</h1>
        <p style="margin:8px 0 0;color:#8b949e;font-size:14px;">Your Ed25519 keypair — keep this safe</p>
      </div>
      <!-- Body -->
      <div style="padding:24px 32px;">
        <p style="color:#f0f6fc;font-size:14px;margin:0 0 16px;">
          Hello <strong>${username || 'Creator'}</strong>,
        </p>
        <p style="color:#8b949e;font-size:14px;margin:0 0 24px;">
          Below is your <code style="background:#21262d;padding:2px 6px;border-radius:4px;color:#79c0ff;">eustress-${username || 'identity'}.toml</code> file.
          This is your permanent Eustress identity — it contains your Ed25519 private key.
          Store it securely and never share it.
        </p>
        <!-- TOML Content -->
        <div style="background:#0d1117;border:1px solid #30363d;border-radius:8px;padding:16px;margin:0 0 24px;">
          <pre style="margin:0;color:#c9d1d9;font-size:12px;font-family:'SF Mono',Consolas,monospace;white-space:pre-wrap;word-break:break-all;">${toml_content.replace(/</g,'&lt;').replace(/>/g,'&gt;')}</pre>
        </div>
        <!-- Instructions -->
        <div style="background:#1a2a3a;border:1px solid #2a3a4a;border-radius:8px;padding:16px;margin:0 0 24px;">
          <p style="color:#8ab4f8;font-size:13px;margin:0 0 8px;font-weight:600;">How to use this backup:</p>
          <ol style="color:#8ab4f8;font-size:12px;margin:0;padding-left:20px;line-height:1.8;">
            <li>Save the text above as <code style="background:#21262d;padding:1px 4px;border-radius:3px;">eustress-${username || 'identity'}.toml</code></li>
            <li>Use it to sign in at <a href="https://eustress.dev/login" style="color:#58a6ff;">eustress.dev/login</a></li>
            <li>Or load it in EustressEngine via the Sign In dialog</li>
          </ol>
        </div>
        <p style="color:#f85149;font-size:12px;margin:0;text-align:center;">
          ⚠ Never share your private_key with anyone. Eustress staff will never ask for it.
        </p>
      </div>
      <!-- Footer -->
      <div style="background:#0d1117;padding:16px 32px;text-align:center;border-top:1px solid #30363d;">
        <p style="color:#484f58;font-size:11px;margin:0;">
          Eustress Engine — Build, simulate, earn.
        </p>
      </div>
    </div>
  </div>
</body>
</html>`;

  try {
    // Send via MailChannels (free for Cloudflare Workers)
    const emailResp = await fetch('https://api.mailchannels.net/tx/v1/send', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        personalizations: [{ to: [{ email }] }],
        from: { email: 'identity@eustress.dev', name: 'Eustress Identity' },
        subject: `Your Eustress Identity Backup — ${username || 'Creator'}`,
        content: [
          { type: 'text/html', value: htmlBody },
          { type: 'text/plain', value: `Eustress Identity Backup for ${username}\n\nSave the following as identity.toml:\n\n${toml_content}\n\nNever share your private_key.` },
        ],
      }),
    });

    if (emailResp.ok || emailResp.status === 202) {
      return json({ ok: true, message: 'Backup emailed' }, 200, cors);
    }

    const errText = await emailResp.text();
    console.error('MailChannels error:', emailResp.status, errText);
    return json({ error: 'Email delivery failed', detail: errText }, 502, cors);
  } catch (e) {
    console.error('Email send error:', e);
    return json({ error: 'Email service unavailable' }, 503, cors);
  }
}

// ── Workshop Context (Persistent Memories + Global Rules) ───────────────────

async function handleGetWorkshopContext(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Sign in to sync Workshop context across devices' }, 401, cors);

  const url = new URL(request.url);
  const projectId = url.searchParams.get('project_id') || 'default';
  const key = `ctx:${userId}:${projectId}`;

  const data = await env.WORKSHOP_CONTEXT.get(key);
  if (!data) {
    return json({
      version: 1,
      user_id: userId,
      project_id: projectId,
      memories: [],
      global_rules: [],
      session_summaries: [],
      last_synced: new Date().toISOString(),
    }, 200, cors);
  }

  return json(JSON.parse(data), 200, cors);
}

async function handlePutWorkshopContext(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Sign in to sync Workshop context across devices' }, 401, cors);

  const body = await request.json();
  const projectId = body.project_id || 'default';
  const key = `ctx:${userId}:${projectId}`;

  const doc = {
    version: 1,
    user_id: userId,
    project_id: projectId,
    memories: body.memories || [],
    global_rules: body.global_rules || [],
    session_summaries: (body.session_summaries || []).slice(-20), // Keep last 20 summaries
    last_synced: new Date().toISOString(),
  };

  await env.WORKSHOP_CONTEXT.put(key, JSON.stringify(doc));
  return json({ ok: true, last_synced: doc.last_synced }, 200, cors);
}

async function handleAddWorkshopMemory(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Sign in to sync Workshop memories' }, 401, cors);

  const body = await request.json();
  const { project_id, key: memKey, value, category, source } = body;

  if (!memKey || !value) return json({ error: 'key and value required' }, 400, cors);

  const ctxKey = `ctx:${userId}:${project_id || 'default'}`;
  const existing = await env.WORKSHOP_CONTEXT.get(ctxKey);
  const doc = existing ? JSON.parse(existing) : {
    version: 1, user_id: userId, project_id: project_id || 'default',
    memories: [], global_rules: [], session_summaries: [],
  };

  // Upsert memory by key
  const idx = doc.memories.findIndex(m => m.key === memKey);
  const memory = {
    key: memKey,
    value,
    category: category || 'preference',
    source: source || 'user',
    updated_at: new Date().toISOString(),
  };

  if (idx >= 0) {
    doc.memories[idx] = memory;
  } else {
    doc.memories.push(memory);
  }

  doc.last_synced = new Date().toISOString();
  await env.WORKSHOP_CONTEXT.put(ctxKey, JSON.stringify(doc));

  return json({ ok: true, memory_count: doc.memories.length }, 200, cors);
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
    verification_id: `kyc-${sessionId}-${side}`,
    session_id: sessionId, side, status: 'uploaded',
    r2_key: objectKey, hash: hashHex,
  }, 200, cors);
}

async function handleKycStatus(verificationId, env, cors) {
  const data = await env.KYC_STATUS.get(verificationId);
  if (!data) return json({ error: 'Not found' }, 404, cors);
  return json(JSON.parse(data), 200, cors);
}

/// Submit KYC for verification after documents are uploaded.
/// Fetches the document image from R2, sends it to Grok Vision for OCR +
/// authenticity check, then marks as verified or rejected based on AI analysis.
async function handleKycSubmit(request, env, cors) {
  try {
    const body = await request.json();
    const sessionId = body.session_id;
    const needsBack = body.needs_back !== false;

    if (!sessionId)
      return json({ error: 'Missing session_id' }, 400, cors);

    // Check front is uploaded
    const frontKey = `kyc-${sessionId}-front`;
    const frontData = await env.KYC_STATUS.get(frontKey);
    if (!frontData)
      return json({ error: 'Front document not uploaded', status: 'incomplete' }, 400, cors);

    // Check back if required
    if (needsBack) {
      const backKey = `kyc-${sessionId}-back`;
      const backData = await env.KYC_STATUS.get(backKey);
      if (!backData)
        return json({ error: 'Back document not uploaded', status: 'incomplete' }, 400, cors);
    }

    const front = JSON.parse(frontData);
    const verificationId = frontKey;

    // ── Grok Vision document verification ──
    const grokResult = await verifyDocumentWithGrok(front.r2_key, body.full_name || '', body.birthday || '', env);

    const verifiedRecord = {
      ...front,
      status: grokResult.decision === 'APPROVE' ? 'verified' : 'rejected',
      verified_at: new Date().toISOString(),
      ocr_name: grokResult.extracted_name || body.full_name || '',
      grok_analysis: grokResult,
      decision: grokResult,
    };

    await env.KYC_STATUS.put(verificationId, JSON.stringify(verifiedRecord), {
      expirationTtl: 86400 * 365,
    });

    if (grokResult.decision === 'APPROVE') {
      return json({
        status: 'verified',
        verification_id: verificationId,
        ocr_name: grokResult.extracted_name || body.full_name || '',
      }, 200, cors);
    } else {
      return json({
        status: 'rejected',
        verification_id: verificationId,
        decision: grokResult,
      }, 200, cors);
    }
  } catch (e) {
    return json({ error: 'Submit failed: ' + e.message }, 500, cors);
  }
}

/**
 * Verify an ID document image using Grok Vision.
 * Fetches the image from R2, sends it to Grok's vision model for:
 * 1. Is this a real government-issued ID document?
 * 2. Is the image clear enough to read?
 * 3. Extract the full legal name and date of birth
 * 4. Does the extracted info match what the user provided?
 *
 * Falls back to APPROVE if Grok is unavailable (graceful degradation).
 */
async function verifyDocumentWithGrok(r2Key, claimedName, claimedBirthday, env) {
  if (!env.GROK_API_KEY) {
    return { decision: 'APPROVE', reason: 'Vision verification unavailable', extracted_name: claimedName, confidence: 0 };
  }

  try {
    // Fetch document image from R2
    const r2Object = await env.KYC_BUCKET.get(r2Key);
    if (!r2Object) {
      return { decision: 'APPROVE', reason: 'Document not found in storage — approved on upload record', extracted_name: claimedName, confidence: 0 };
    }

    const imageBytes = await r2Object.arrayBuffer();
    const contentType = r2Object.httpMetadata?.contentType || 'image/jpeg';
    const base64Image = btoa(String.fromCharCode(...new Uint8Array(imageBytes)));
    const dataUrl = `data:${contentType};base64,${base64Image}`;

    const prompt = `You are a KYC document verification AI for Eustress, a game engine platform.
Analyze this ID document image and respond in EXACTLY this JSON format, nothing else:

{
  "is_id_document": true/false,
  "document_type": "passport|drivers_license|national_id|other|not_a_document",
  "image_quality": "clear|acceptable|blurry|unreadable",
  "extracted_name": "Full Legal Name from document or empty string if unreadable",
  "extracted_dob": "YYYY-MM-DD from document or empty string if unreadable",
  "name_matches": true/false,
  "dob_matches": true/false,
  "decision": "APPROVE|DENY",
  "reason": "one sentence explanation",
  "confidence": 0-100
}

Rules:
- APPROVE if: it IS a government ID, image is clear/acceptable, and name roughly matches "${claimedName}"
- APPROVE even if DOB doesn't perfectly match (typos happen) — just note it
- DENY if: not an ID document, completely unreadable, or obviously fraudulent (screenshot of screen, printed copy of photo, digitally altered)
- Partial name matches are OK (e.g. "John Smith" matches "Jonathan Smith")
- If you cannot read the name but the document looks genuine and clear: APPROVE with empty extracted_name
- Err on the side of APPROVE — this is a game platform, not a bank
- Never fabricate information you cannot read from the document`;

    const resp = await fetch('https://api.x.ai/v1/responses', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${env.GROK_API_KEY}`,
      },
      body: JSON.stringify({
        model: 'grok-4.20-reasoning',
        input: [
          { type: 'image_url', image_url: { url: dataUrl } },
          { type: 'text', text: prompt },
        ],
      }),
    });

    if (!resp.ok) {
      const errText = await resp.text();
      console.error('Grok vision error:', resp.status, errText);
      return { decision: 'APPROVE', reason: 'Vision service error — approved on upload', extracted_name: claimedName, confidence: 0 };
    }

    const data = await resp.json();
    const responseText = data.output?.[0]?.content?.[0]?.text || data.output_text || '';

    const jsonMatch = responseText.match(/\{[\s\S]*\}/);
    if (!jsonMatch) {
      return { decision: 'APPROVE', reason: 'Could not parse vision response — approved on upload', extracted_name: claimedName, confidence: 0 };
    }

    const result = JSON.parse(jsonMatch[0]);
    return {
      decision: result.decision || 'APPROVE',
      reason: result.reason || 'No issues found',
      is_id_document: result.is_id_document ?? true,
      document_type: result.document_type || 'unknown',
      image_quality: result.image_quality || 'unknown',
      extracted_name: result.extracted_name || '',
      extracted_dob: result.extracted_dob || '',
      name_matches: result.name_matches ?? true,
      dob_matches: result.dob_matches ?? true,
      confidence: Math.min(100, Math.max(0, result.confidence || 0)),
      model: 'grok-4.20-reasoning',
    };
  } catch (e) {
    console.error('Grok vision exception:', e);
    return { decision: 'APPROVE', reason: `Vision error: ${e.message} — approved on upload`, extracted_name: claimedName, confidence: 0 };
  }
}

// ═══════════════════════════════════════════════════════════════════════════
// CO-SIGN
// ═══════════════════════════════════════════════════════════════════════════

async function handleCosign(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const body = await request.json();
  const { contribution_type, contribution_hash, duration_secs } = body;

  if (!contribution_type || !contribution_hash)
    return json({ error: 'contribution_type and contribution_hash required' }, 400, cors);

  // Rate limit: max 120 cosigns per hour per user
  const hourKey = `cosign-rate:${userId}:${new Date().toISOString().slice(0, 13)}`;
  const count = parseInt(await env.CHALLENGES.get(hourKey) || '0');
  if (count >= 120) return json({ error: 'Rate limit: 120 cosigns/hour' }, 429, cors);
  await env.CHALLENGES.put(hourKey, (count + 1).toString(), { expirationTtl: 3600 });

  // Contribution weights (from bliss-core/src/contribution.rs)
  const weights = {
    ActiveTime: 1.0, Creation: 2.5, Collaboration: 2.0, Education: 2.2,
    Development: 3.0, Moderation: 1.5, QualityAssurance: 1.8,
    Optimization: 2.0, Documentation: 1.5, Custom: 1.0,
  };
  const weight = weights[contribution_type] || 1.0;
  const score = weight * Math.max(1, Math.min(duration_secs || 60, 3600)) / 60; // weighted minutes

  // Accumulate on user record
  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User not found' }, 404, cors);
  const user = JSON.parse(userData);

  user.contribution_score = (user.contribution_score || 0) + score;
  user.total_cosigns = (user.total_cosigns || 0) + 1;
  user.last_contribution = new Date().toISOString();
  await env.USERS.put(`user:${userId}`, JSON.stringify(user));

  // Generate co-signature (hash of contribution + user + timestamp)
  const payload = `cosign|${userId}|${contribution_hash}|${new Date().toISOString()}`;
  const sigBytes = new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(payload)));
  const signature = hexEncode(sigBytes);

  return json({
    server_signature: signature,
    co_signed_at: new Date().toISOString(),
    contribution_type,
    weight,
    score_added: score,
    total_score: user.contribution_score,
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
  try {
    // Count registered users by listing KV keys with username: prefix
    let userCount = 0;
    let cursor = undefined;
    do {
      const list = await env.USERS.list({ prefix: 'username:', limit: 1000, cursor });
      userCount += list.keys.length;
      cursor = list.list_complete ? undefined : list.cursor;
    } while (cursor);

    return json({
      total_users: userCount,
      total_simulations: 0,
      total_plays: 0,
      online_now: Math.max(1, userCount),
      total_bliss_distributed: 0,
      timestamp: new Date().toISOString(),
    }, 200, cors);
  } catch (e) {
    // Graceful fallback if KV is unavailable
    return json({
      total_users: 0,
      total_simulations: 0,
      total_plays: 0,
      online_now: 0,
      total_bliss_distributed: 0,
      timestamp: new Date().toISOString(),
    }, 200, cors);
  }
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
  try {
  // Build leaderboard from KV users — sorted by hours, filterable by period
  const period = 'alltime'; // No request param available — default to alltime

  const entries = [];
  const list = await env.USERS.list({ prefix: 'user:', limit: 1000 });

  const now = new Date();

  for (const key of list.keys) {
    const userData = await env.USERS.get(key.name);
    if (!userData) continue;
    const user = JSON.parse(userData);
    if (user.banned) continue;

    let hours = user.total_hours || 0;

    // For time-filtered periods, sum daily hour entries
    if (period !== 'alltime') {
      hours = 0;
      const daysBack = period === 'today' ? 1 : period === 'week' ? 7 : 30;
      for (let d = 0; d < daysBack; d++) {
        const date = new Date(now);
        date.setDate(date.getDate() - d);
        const dateStr = date.toISOString().split('T')[0];
        const dKey = `hours_daily:${user.id}:${dateStr}`;
        const val = await env.SOCIAL.get(dKey);
        if (val) hours += parseFloat(val);
      }
    }

    // Count published simulations
    const simList = await env.INVENTORY.list({ prefix: `sim:${user.id}:`, limit: 100 });
    const spacesCreated = simList.keys.length;

    entries.push({
      username: user.username,
      avatar_url: user.avatar_url || null,
      hours: Math.round(hours * 10) / 10,
      bliss_balance: user.bliss_balance || 0,
      spaces_created: spacesCreated,
      total_visits: user.total_visits || 0,
      last_active: user.last_active || user.created_at,
    });
  }

  // Sort by hours descending
  entries.sort((a, b) => b.hours - a.hours);
  const top = entries.slice(0, 20).map((e, i) => ({
    rank: i + 1,
    username: e.username,
    avatar_url: e.avatar_url,
    hours: e.hours,
    bliss_balance: e.bliss_balance,
    spaces_created: e.spaces_created,
    total_visits: e.total_visits,
  }));

  // Pick a random featured creator from top 20 (changes weekly via date seed)
  const weekSeed = Math.floor(now.getTime() / (7 * 86400000));
  const featuredIndex = weekSeed % Math.max(top.length, 1);
  const featured = top[featuredIndex] || top[0] || null;

  return json({ entries: top, featured, period, total: entries.length }, 200, cors);
  } catch (e) {
    return json({ entries: [], featured: null, period: 'alltime', total: 0 }, 200, cors);
  }
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
// STRIPE — Treasury funding + Connect payouts
// ═══════════════════════════════════════════════════════════════════════════

async function stripeRequest(method, endpoint, body, env) {
  const resp = await fetch(`https://api.stripe.com/v1${endpoint}`, {
    method,
    headers: {
      'Authorization': `Bearer ${env.STRIPE_SECRET_KEY}`,
      'Content-Type': 'application/x-www-form-urlencoded',
    },
    body: body ? new URLSearchParams(body).toString() : undefined,
  });
  return resp.json();
}

// Stripe price IDs
const STRIPE_PRICES = {
  seed_one_time: 'price_1THyC7RgsC7hEeKMmD71Pdcm',
  growth_one_time: 'price_1THyC7RgsC7hEeKMc59jIIrA',
  sustainer_one_time: 'price_1THyC8RgsC7hEeKMaQTcj58e',
  patron_one_time: 'price_1THyC9RgsC7hEeKMioXlaWy0',
  seed_recurring: 'price_1THyC9RgsC7hEeKMLdz2AAlz',
  growth_recurring: 'price_1THyCARgsC7hEeKMvMQWDog5',
  sustainer_recurring: 'price_1THyCARgsC7hEeKM6QpYML0j',
  patron_recurring: 'price_1THyCBRgsC7hEeKMLAlLI8pS',
};

// Platform fee: 2.5%
const PLATFORM_FEE_PERCENT = 2.5;

// Create Stripe Checkout session for treasury funding
async function handleStripeCheckout(request, env, cors) {
  if (!env.STRIPE_SECRET_KEY) return json({ error: 'Stripe not configured' }, 503, cors);

  const userId = await verifyAuth(request, env);
  const { tier, mode, custom_amount } = await request.json();
  // tier: 'seed'|'growth'|'sustainer'|'patron' or null for custom
  // mode: 'one_time' or 'recurring'

  const isRecurring = mode === 'recurring';
  let priceId;
  let amountCents;

  if (tier && STRIPE_PRICES[`${tier}_${isRecurring ? 'recurring' : 'one_time'}`]) {
    priceId = STRIPE_PRICES[`${tier}_${isRecurring ? 'recurring' : 'one_time'}`];
  } else if (custom_amount && custom_amount >= 5) {
    amountCents = Math.round(custom_amount * 100);
  } else {
    return json({ error: 'Select a tier or enter a custom amount ($5 minimum)' }, 400, cors);
  }

  const params = {
    'mode': isRecurring ? 'subscription' : 'payment',
    'success_url': 'https://eustress.dev/bliss?funded=true',
    'cancel_url': 'https://eustress.dev/bliss?funded=false',
  };

  if (priceId) {
    params['line_items[0][price]'] = priceId;
    params['line_items[0][quantity]'] = '1';
  } else {
    // Custom amount — create price inline
    params['line_items[0][price_data][currency]'] = 'usd';
    params['line_items[0][price_data][product]'] = 'prod_UGVCI0rliegrSC';
    params['line_items[0][price_data][unit_amount]'] = amountCents.toString();
    params['line_items[0][quantity]'] = '1';
    if (isRecurring) {
      params['line_items[0][price_data][recurring][interval]'] = 'month';
    }
  }

  // 2.5% platform fee (Stripe collects this for us via Connect)
  if (amountCents) {
    const feeCents = Math.round(amountCents * PLATFORM_FEE_PERCENT / 100);
    params['payment_intent_data[application_fee_amount]'] = feeCents.toString();
  }

  if (userId) {
    params['metadata[user_id]'] = userId;
    params['client_reference_id'] = userId;
  }

  const session = await stripeRequest('POST', '/checkout/sessions', params, env);

  if (session.error) {
    return json({ error: session.error.message, type: session.error.type }, 400, cors);
  }

  return json({ url: session.url, session_id: session.id }, 200, cors);
}

// Stripe webhook — handle successful payments
async function handleStripeWebhook(request, env) {
  // In production, verify webhook signature with STRIPE_WEBHOOK_SECRET
  const body = await request.text();
  let event;

  try {
    event = JSON.parse(body);
  } catch {
    return new Response('Invalid JSON', { status: 400 });
  }

  if (event.type === 'checkout.session.completed') {
    const session = event.data.object;
    const amount = (session.amount_total || 0) / 100; // dollars
    const userId = session.metadata?.user_id || session.client_reference_id;

    const isTicketPurchase = session.metadata?.type === 'ticket_purchase';

    if (isTicketPurchase) {
      // TICKET PURCHASE — credit tickets + split revenue
      const pkgKey = session.metadata?.package;
      const pkg = TICKET_PACKAGES[pkgKey];
      const ticketsToCredit = pkg ? pkg.total : parseInt(session.metadata?.tickets || '0');

      // Idempotency check
      const existing = await env.PAYOUTS.get(`deposit:${session.id}`);
      if (existing) return new Response('OK', { status: 200 }); // Already processed

      // Credit tickets to user
      if (userId) {
        const userData = await env.USERS.get(`user:${userId}`);
        if (userData) {
          const user = JSON.parse(userData);
          user.ticket_balance = (user.ticket_balance || 0) + ticketsToCredit;
          await env.USERS.put(`user:${userId}`, JSON.stringify(user));
        }
      }

      // Revenue split: 50% treasury, 50% platform
      const treasuryCut = amount * TREASURY_SPLIT;
      const platformCut = amount - treasuryCut; // Remaining 50% to Eustress

      const currentTreasury = parseFloat(await env.PAYOUTS.get('treasury:total_usd') || '0');
      await env.PAYOUTS.put('treasury:total_usd', (currentTreasury + treasuryCut).toString());

      const currentPlatform = parseFloat(await env.PAYOUTS.get('platform:total_usd') || '0');
      await env.PAYOUTS.put('platform:total_usd', (currentPlatform + platformCut).toString());

      // Log transaction
      if (userId) {
        await env.INVENTORY.put(`txn:${userId}:${Date.now()}`, JSON.stringify({
          id: session.id, user_id: userId, type: 'purchase', amount: ticketsToCredit,
          currency: 'TKT', stripe_session_id: session.id, price_usd: amount,
          description: `Purchased ${pkg?.name || 'Tickets'} package (${ticketsToCredit} TKT)`,
          timestamp: new Date().toISOString(),
        }), { expirationTtl: 86400 * 365 * 3 });
      }

      // Record deposit with full revenue breakdown
      await env.PAYOUTS.put(`deposit:${session.id}`, JSON.stringify({
        id: session.id, type: 'ticket_purchase', amount_usd: amount,
        treasury_cut: treasuryCut, platform_cut: platformCut,
        tickets_credited: ticketsToCredit, package: pkgKey,
        user_id: userId || 'anonymous', timestamp: new Date().toISOString(),
      }));

    } else {
      // TREASURY FUNDING — direct treasury deposit (existing flow)
      await env.PAYOUTS.put(`deposit:${session.id}`, JSON.stringify({
        id: session.id, type: 'treasury_fund', amount_usd: amount,
        user_id: userId || 'anonymous', timestamp: new Date().toISOString(),
        mode: session.mode, stripe_payment_intent: session.payment_intent,
      }));

      const currentTotal = parseFloat(await env.PAYOUTS.get('treasury:total_usd') || '0');
      await env.PAYOUTS.put('treasury:total_usd', (currentTotal + amount).toString());

      const count = parseInt(await env.PAYOUTS.get('treasury:deposit_count') || '0');
      await env.PAYOUTS.put('treasury:deposit_count', (count + 1).toString());
    }
  }

  return new Response('OK', { status: 200 });
}

// Stripe Connect — onboard a contributor to receive payouts
async function handleStripeConnectOnboard(request, env, cors) {
  if (!env.STRIPE_SECRET_KEY) return json({ error: 'Stripe not configured' }, 503, cors);

  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User not found' }, 404, cors);
  const user = JSON.parse(userData);

  // Check if user already has a Connect account
  let connectId = user.stripe_connect_id;

  if (!connectId) {
    // Detect country from KYC data
    const kycFront = await env.KYC_STATUS.get(`kyc-${userId}-front`);
    const kycCountry = kycFront ? JSON.parse(kycFront).iso2 || 'US' : 'US';

    // Create Connect Custom account with pre-filled identity info
    const clientIp = request.headers.get('cf-connecting-ip') || '0.0.0.0';
    const accountParams = {
      'type': 'custom',
      'country': kycCountry,
      'business_type': 'individual',
      'metadata[user_id]': userId,
      'metadata[username]': user.username,
      'capabilities[card_payments][requested]': 'true',
      'capabilities[transfers][requested]': 'true',
      // ToS acceptance (required for custom accounts)
      'tos_acceptance[date]': Math.floor(Date.now() / 1000).toString(),
      'tos_acceptance[ip]': clientIp,
    };

    if (user.email) accountParams['email'] = user.email;
    // Don't set first_name to username — Stripe will reject if it doesn't match the ID photo
    // Name comes from the ID document uploaded via R2 sync or Stripe's own onboarding
    if (user.birthday) {
      const [y, m, d] = user.birthday.split('-');
      accountParams['individual[dob][year]'] = y;
      accountParams['individual[dob][month]'] = parseInt(m).toString();
      accountParams['individual[dob][day]'] = parseInt(d).toString();
    }

    const account = await stripeRequest('POST', '/accounts', accountParams, env);
    if (account.error) return json({ error: account.error.message, type: account.error.type, code: account.error.code, param: account.error.param }, 400, cors);

    connectId = account.id;
    user.stripe_connect_id = connectId;
    await env.USERS.put(`user:${userId}`, JSON.stringify(user));

    // Upload KYC docs from R2 to Stripe for identity verification
    await syncKycDocsToStripe(userId, connectId, env);
  } else {
    // Account exists — re-sync docs in case they weren't uploaded before
    await syncKycDocsToStripe(userId, connectId, env);
  }

  // Create onboarding link — collects remaining info (bank details, SSN for 1099)
  const link = await stripeRequest('POST', '/account_links', {
    'account': connectId,
    'refresh_url': 'https://eustress.dev/bliss?connect=refresh',
    'return_url': 'https://eustress.dev/bliss?connect=complete',
    'type': 'account_onboarding',
    'collect': 'eventually_due',
  }, env);

  if (link.error) return json({ error: link.error.message, type: link.error.type, code: link.error.code }, 400, cors);

  return json({ url: link.url, connect_id: connectId }, 200, cors);
}

// Check Stripe Connect status for a user
async function handleStripeConnectStatus(request, env, cors) {
  if (!env.STRIPE_SECRET_KEY) return json({ error: 'Stripe not configured' }, 503, cors);

  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User not found' }, 404, cors);
  const user = JSON.parse(userData);

  if (!user.stripe_connect_id) {
    return json({ connected: false, message: 'No payout account linked' }, 200, cors);
  }

  const account = await stripeRequest('GET', `/accounts/${user.stripe_connect_id}`, null, env);

  return json({
    connected: true,
    connect_id: user.stripe_connect_id,
    payouts_enabled: account.payouts_enabled || false,
    details_submitted: account.details_submitted || false,
  }, 200, cors);
}

// Sync KYC documents from R2 to Stripe Connect for identity verification
async function syncKycDocsToStripe(userId, connectId, env) {
  try {
    for (const side of ['front', 'back']) {
      // Get the KYC record to find the R2 key
      const kycData = await env.KYC_STATUS.get(`kyc-${userId}-${side}`);
      if (!kycData) continue;

      const kyc = JSON.parse(kycData);
      if (!kyc.r2_key) continue;

      // Fetch document from R2
      const r2Object = await env.KYC_BUCKET.get(kyc.r2_key);
      if (!r2Object) continue;

      const fileBytes = await r2Object.arrayBuffer();
      const contentType = r2Object.httpMetadata?.contentType || 'image/jpeg';

      // Upload to Stripe Files API (multipart form)
      const formData = new FormData();
      const ext = contentType.includes('pdf') ? 'pdf' : contentType.includes('png') ? 'png' : 'jpg';
      formData.append('file', new Blob([fileBytes], { type: contentType }), `id_${side}.${ext}`);
      formData.append('purpose', 'identity_document');

      const fileResp = await fetch('https://files.stripe.com/v1/files', {
        method: 'POST',
        headers: { 'Authorization': `Bearer ${env.STRIPE_SECRET_KEY}` },
        body: formData,
      });

      const fileResult = await fileResp.json();
      if (fileResult.error || !fileResult.id) continue;

      // Attach to the Connect account's identity verification
      const docSide = side === 'front' ? 'front' : 'back';
      await stripeRequest('POST', `/accounts/${connectId}`, {
        [`individual[verification][document][${docSide}]`]: fileResult.id,
      }, env);
    }
  } catch (e) {
    // Non-fatal — Stripe onboarding will still work, user just re-uploads manually
    console.error('KYC sync to Stripe failed:', e.message);
  }
}

// ═══════════════════════════════════════════════════════════════════════════
// DAILY PAYOUTS — BLS → USD conversion + Stripe Connect transfers
// ═══════════════════════════════════════════════════════════════════════════

// Trigger daily payout calculation (called by cron or admin)
async function handleDailyPayout(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  if (!env.STRIPE_SECRET_KEY) return json({ error: 'Stripe not configured' }, 503, cors);

  // Get treasury balance
  const treasuryUsd = parseFloat(await env.PAYOUTS.get('treasury:total_usd') || '0');
  if (treasuryUsd <= 0) return json({ error: 'Treasury is empty', treasury_usd: 0 }, 200, cors);

  // Daily drip rate: 0.276% of remaining balance (from economics.rs)
  const dripRate = 0.00276;
  const dailyDripUsd = treasuryUsd * dripRate;

  if (dailyDripUsd < 0.50) {
    return json({ message: 'Daily drip too small for payout', drip_usd: dailyDripUsd }, 200, cors);
  }

  // Get all users with contribution scores
  const userList = await env.USERS.list({ prefix: 'user:', limit: 1000 });
  const contributors = [];
  let totalScore = 0;

  for (const key of userList.keys) {
    const data = await env.USERS.get(key.name);
    if (!data) continue;
    const user = JSON.parse(data);
    const score = user.contribution_score || 0;
    if (score > 0 && user.stripe_connect_id && !user.banned) {
      contributors.push({ user_id: user.id, username: user.username, score, connect_id: user.stripe_connect_id });
      totalScore += score;
    }
  }

  if (totalScore <= 0 || contributors.length === 0) {
    return json({
      message: 'No eligible contributors with Connect accounts',
      drip_usd: dailyDripUsd,
      contributors: 0,
    }, 200, cors);
  }

  // Calculate per-contributor payout
  const payouts = [];
  let totalPaid = 0;

  for (const c of contributors) {
    const share = c.score / totalScore;
    const amountUsd = dailyDripUsd * share;
    const amountCents = Math.floor(amountUsd * 100);

    if (amountCents < 50) continue; // Stripe minimum: $0.50

    // Create Stripe transfer to connected account
    try {
      const transfer = await stripeRequest('POST', '/transfers', {
        'amount': amountCents.toString(),
        'currency': 'usd',
        'destination': c.connect_id,
        'description': `Bliss daily payout - ${c.username}`,
        'metadata[user_id]': c.user_id,
        'metadata[date]': new Date().toISOString().split('T')[0],
      }, env);

      if (!transfer.error) {
        payouts.push({
          user_id: c.user_id,
          username: c.username,
          amount_usd: amountCents / 100,
          transfer_id: transfer.id,
        });
        totalPaid += amountCents / 100;
      }
    } catch (e) {
      // Skip failed transfers, continue with others
    }
  }

  // Debit treasury
  const newTreasury = treasuryUsd - totalPaid;
  await env.PAYOUTS.put('treasury:total_usd', newTreasury.toString());

  // Record payout
  const payoutRecord = {
    date: new Date().toISOString(),
    drip_usd: dailyDripUsd,
    total_paid_usd: totalPaid,
    contributors_paid: payouts.length,
    treasury_before: treasuryUsd,
    treasury_after: newTreasury,
    payouts,
  };

  const payoutKey = `payout:${new Date().toISOString().split('T')[0]}`;
  await env.PAYOUTS.put(payoutKey, JSON.stringify(payoutRecord), { expirationTtl: 86400 * 365 * 5 });

  await auditLog(env, 'DAILY_PAYOUT', adminId, 'treasury', payoutRecord);

  return json(payoutRecord, 200, cors);
}

// Get payout history
async function handlePayoutHistory(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  // Get recent payouts for this user
  const history = [];
  const list = await env.PAYOUTS.list({ prefix: 'payout:', limit: 30 });

  for (const key of list.keys) {
    const data = await env.PAYOUTS.get(key.name);
    if (!data) continue;
    const record = JSON.parse(data);
    const userPayout = record.payouts?.find(p => p.user_id === userId);
    if (userPayout) {
      history.push({
        date: record.date,
        amount_usd: userPayout.amount_usd,
        transfer_id: userPayout.transfer_id,
      });
    }
  }

  return json({ history, total_received: history.reduce((sum, p) => sum + p.amount_usd, 0) }, 200, cors);
}

// Get current BLS → USD exchange rate
async function handlePayoutRate(env, cors) {
  const treasuryUsd = parseFloat(await env.PAYOUTS.get('treasury:total_usd') || '0');
  const dripRate = 0.00276; // daily drip rate
  const dailyDripUsd = treasuryUsd * dripRate;

  // Daily BLS emission: 13,699 BLS (year 1)
  const dailyBls = 13699;
  const blsToUsd = dailyBls > 0 ? dailyDripUsd / dailyBls : 0;

  const platformUsd = parseFloat(await env.PAYOUTS.get('platform:total_usd') || '0');

  return json({
    treasury_usd: treasuryUsd,
    platform_usd: platformUsd,
    daily_drip_usd: dailyDripUsd,
    daily_bls_emission: dailyBls,
    bls_to_usd_rate: blsToUsd,
    rate_display: blsToUsd > 0 ? `$${blsToUsd.toFixed(6)}/BLS` : 'No treasury funds',
    deposit_count: parseInt(await env.PAYOUTS.get('treasury:deposit_count') || '0'),
  }, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// TICKETS — Purchasable currency for marketplace + simulation API
// ═══════════════════════════════════════════════════════════════════════════

const TICKET_PACKAGES = {
  starter:  { name: 'Starter',  usd: 4.99,  base: 400,  bonus: 0,    total: 400,   price_id: 'price_1THyBSRgsC7hEeKMVPoXgzGp' },
  standard: { name: 'Standard', usd: 9.99,  base: 800,  bonus: 80,   total: 880,   price_id: 'price_1THyBTRgsC7hEeKMqirqajPl' },
  mega:     { name: 'Mega',     usd: 19.99, base: 1600, bonus: 240,  total: 1840,  price_id: 'price_1THyBURgsC7hEeKMXkupuaQR' },
  super:    { name: 'Super',    usd: 49.99, base: 4000, bonus: 1000, total: 5000,  price_id: 'price_1THyBURgsC7hEeKMzwTi9Z2V' },
  ultra:    { name: 'Ultra',    usd: 99.99, base: 8000, bonus: 2800, total: 10800, price_id: 'price_1THyBVRgsC7hEeKMHTMs0Hsq' },
};

const DEVELOPER_SHARE = 0.70;
const PLATFORM_SHARE = 0.30;
const TREASURY_SPLIT = 0.50;

function handleTicketPackages(env, cors) {
  const packages = Object.entries(TICKET_PACKAGES).map(([key, pkg]) => ({
    id: key, name: pkg.name, usd: pkg.usd, base: pkg.base, bonus: pkg.bonus, total: pkg.total,
  }));
  return json({ packages }, 200, cors);
}

async function handleTicketBalance(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  if (!userData) return json({ error: 'User not found' }, 404, cors);
  const user = JSON.parse(userData);

  return json({ tickets: user.ticket_balance || 0, user_id: userId }, 200, cors);
}

async function handleTicketCheckout(request, env, cors) {
  if (!env.STRIPE_SECRET_KEY) return json({ error: 'Stripe not configured' }, 503, cors);

  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Sign in to purchase tickets' }, 401, cors);

  const { package: pkgKey } = await request.json();
  const pkg = TICKET_PACKAGES[pkgKey];
  if (!pkg) return json({ error: 'Invalid package' }, 400, cors);

  const params = {
    'mode': 'payment',
    'success_url': 'https://eustress.dev/tickets?purchased=true',
    'cancel_url': 'https://eustress.dev/tickets?purchased=false',
    'line_items[0][price]': pkg.price_id,
    'line_items[0][quantity]': '1',
    'metadata[type]': 'ticket_purchase',
    'metadata[package]': pkgKey,
    'metadata[tickets]': pkg.total.toString(),
    'metadata[user_id]': userId,
    'client_reference_id': userId,
  };

  const session = await stripeRequest('POST', '/checkout/sessions', params, env);
  if (session.error) return json({ error: session.error.message }, 400, cors);

  return json({ url: session.url, session_id: session.id }, 200, cors);
}

async function handleTicketSpend(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const { product_id, price, developer_id } = await request.json();
  if (!product_id || !price || price <= 0)
    return json({ error: 'Invalid product or price' }, 400, cors);

  // Get buyer
  const buyerData = await env.USERS.get(`user:${userId}`);
  if (!buyerData) return json({ error: 'Buyer not found' }, 404, cors);
  const buyer = JSON.parse(buyerData);

  const balance = buyer.ticket_balance || 0;
  if (balance < price)
    return json({ error: 'Insufficient tickets', balance, price }, 400, cors);

  // Calculate split
  const devCut = Math.floor(price * DEVELOPER_SHARE);
  const platformCut = price - devCut;

  // Deduct from buyer
  buyer.ticket_balance = balance - price;
  await env.USERS.put(`user:${userId}`, JSON.stringify(buyer));

  // Credit developer (if provided)
  if (developer_id) {
    const devData = await env.USERS.get(`user:${developer_id}`);
    if (devData) {
      const dev = JSON.parse(devData);
      dev.ticket_balance = (dev.ticket_balance || 0) + devCut;
      await env.USERS.put(`user:${developer_id}`, JSON.stringify(dev));
    }
  }

  // Log transactions
  const txnId = crypto.randomUUID();
  const now = new Date().toISOString();

  await env.INVENTORY.put(`txn:${userId}:${Date.now()}`, JSON.stringify({
    id: txnId, user_id: userId, type: 'spend', amount: -price,
    balance_after: buyer.ticket_balance, currency: 'TKT',
    product_id, developer_id, description: `Purchased product ${product_id}`, timestamp: now,
  }), { expirationTtl: 86400 * 365 * 3 });

  if (developer_id) {
    await env.INVENTORY.put(`txn:${developer_id}:${Date.now()}`, JSON.stringify({
      id: crypto.randomUUID(), user_id: developer_id, type: 'dev_payout', amount: devCut,
      currency: 'TKT', product_id, counterparty_id: userId,
      description: `Sale: product ${product_id} (70% of ${price} TKT)`, timestamp: now,
    }), { expirationTtl: 86400 * 365 * 3 });
  }

  return json({
    success: true, price, developer_cut: devCut, platform_cut: platformCut,
    buyer_balance: buyer.ticket_balance,
  }, 200, cors);
}

async function handleTicketHistory(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const list = await env.INVENTORY.list({ prefix: `txn:${userId}:`, limit: 50 });
  const history = [];

  for (const key of list.keys) {
    const data = await env.INVENTORY.get(key.name);
    if (data) history.push(JSON.parse(data));
  }

  history.sort((a, b) => new Date(b.timestamp) - new Date(a.timestamp));
  return json({ history, count: history.length }, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// NODE HEARTBEAT — Live stats for nodes and game servers
// ═══════════════════════════════════════════════════════════════════════════

async function handleNodeHeartbeat(request, env, cors) {
  const body = await request.json();
  const { node_id, mode, players, uptime_secs, fork_id, user_id } = body;

  if (!node_id) return json({ error: 'node_id required' }, 400, cors);

  await env.SOCIAL.put(`node:${node_id}`, JSON.stringify({
    node_id, mode: mode || 'light', players: players || 0,
    uptime_secs: uptime_secs || 0, fork_id: fork_id || 'eustress.dev',
    last_heartbeat: new Date().toISOString(),
  }), { expirationTtl: 120 }); // Expires in 2 min if no heartbeat

  // Return current BLS balance if user_id provided (engine polls this)
  // Also accumulate session hours from uptime_secs
  let bliss_balance = 0;
  let pending_score = 0;
  if (user_id) {
    const userData = await env.USERS.get(`user:${user_id}`);
    if (userData) {
      const user = JSON.parse(userData);
      bliss_balance = user.bliss_balance || 0;

      // Accumulate session hours from heartbeat uptime
      // Track per-day, per-week, per-month, and all-time hours
      const now = new Date();
      const today = now.toISOString().split('T')[0];
      const heartbeatHours = (uptime_secs || 0) / 3600;

      // Store per-period hours (overwrite with latest session uptime per node)
      const hourKey = `hours:${user_id}:${node_id}`;
      const prevData = await env.SOCIAL.get(hourKey);
      const prev = prevData ? JSON.parse(prevData) : { date: '', hours: 0 };

      // Only accumulate if this is a new day or growing session
      if (prev.date !== today) {
        // New day — add previous session to totals, start fresh
        if (prev.hours > 0) {
          user.total_hours = (user.total_hours || 0) + prev.hours;
          // Per-period accumulators
          const dKey = `hours_daily:${user_id}:${prev.date}`;
          const existing = parseFloat(await env.SOCIAL.get(dKey) || '0');
          await env.SOCIAL.put(dKey, String(existing + prev.hours), { expirationTtl: 86400 * 90 });
        }
        await env.SOCIAL.put(hourKey, JSON.stringify({ date: today, hours: heartbeatHours }), { expirationTtl: 86400 * 2 });
      } else if (heartbeatHours > prev.hours) {
        // Same day, session grew — update
        await env.SOCIAL.put(hourKey, JSON.stringify({ date: today, hours: heartbeatHours }), { expirationTtl: 86400 * 2 });
      }

      // Update user record with cumulative hours
      user.total_hours = (user.total_hours || 0);
      user.last_active = now.toISOString();
      await env.USERS.put(`user:${user_id}`, JSON.stringify(user));
    }
    // Check today's pending contributions
    const today = new Date().toISOString().split('T')[0];
    const pendingKey = `contrib:${user_id}:${today}`;
    const pendingData = await env.INVENTORY.get(pendingKey);
    if (pendingData) {
      const pending = JSON.parse(pendingData);
      pending_score = pending.total_score || 0;
    }
  }

  return json({ ok: true, bliss_balance, pending_score }, 200, cors);
}

async function handleNodeStats(env, cors) {
  const list = await env.SOCIAL.list({ prefix: 'node:', limit: 1000 });
  let totalNodes = 0, totalPlayers = 0, lightNodes = 0, fullNodes = 0;

  for (const key of list.keys) {
    const data = await env.SOCIAL.get(key.name);
    if (data) {
      const node = JSON.parse(data);
      totalNodes++;
      totalPlayers += node.players || 0;
      if (node.mode === 'full') fullNodes++;
      else lightNodes++;
    }
  }

  return json({
    active_nodes: totalNodes, light_nodes: lightNodes, full_nodes: fullNodes,
    online_players: totalPlayers, timestamp: new Date().toISOString(),
  }, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// SIMULATIONS — Published spaces (gallery data)
// ═══════════════════════════════════════════════════════════════════════════

async function handlePublishSimulation(request, env, cors) {
  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  const body = await request.json();
  const { name, description, genre, max_players, thumbnail_url, r2_key } = body;

  if (!name) return json({ error: 'name required' }, 400, cors);

  const userData = await env.USERS.get(`user:${userId}`);
  const user = userData ? JSON.parse(userData) : {};

  const simId = crypto.randomUUID();
  const sim = {
    id: simId, name, description: description || '',
    genre: genre || 'all', max_players: max_players || 10,
    author_id: userId, author_name: user.username || 'Unknown',
    thumbnail_url: thumbnail_url || null, r2_key: r2_key || null,
    play_count: 0, favorite_count: 0, version: 1,
    published_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
  };

  await env.SOCIAL.put(`sim:${simId}`, JSON.stringify(sim));
  await env.SOCIAL.put(`sim-author:${userId}:${simId}`, simId);

  // Increment author's sim count
  const simCount = parseInt(await env.SOCIAL.get(`simCount:${userId}`) || '0') + 1;
  await env.SOCIAL.put(`simCount:${userId}`, simCount.toString());

  return json({ id: simId, ...sim }, 201, cors);
}

// Upload .pak scene file to R2
async function handleUploadScene(request, simId, env, cors) {
  const auth = await verifyAuth(request, env);
  if (!auth) return json({ error: 'Unauthorized' }, 401, cors);

  // Verify this simulation exists and belongs to the user
  const simData = await env.SOCIAL.get(`sim:${simId}`);
  if (!simData) return json({ error: 'Simulation not found' }, 404, cors);
  const sim = JSON.parse(simData);
  if (sim.author_id !== auth.userId) return json({ error: 'Not your simulation' }, 403, cors);

  // Read the binary body (the .pak file)
  const body = await request.arrayBuffer();
  if (!body || body.byteLength === 0) return json({ error: 'Empty body' }, 400, cors);

  // Max 500MB per .pak
  if (body.byteLength > 500 * 1024 * 1024)
    return json({ error: 'Scene file too large (max 500MB)' }, 413, cors);

  const r2Key = `universes/${simId}/universe.pak`;
  await env.SCENES.put(r2Key, body, {
    httpMetadata: { contentType: 'application/octet-stream' },
    customMetadata: { simId, authorId: auth.userId, uploadedAt: new Date().toISOString() },
  });

  // Update simulation record with R2 key and file size
  sim.r2_key = r2Key;
  sim.scene_size_bytes = body.byteLength;
  sim.updated_at = new Date().toISOString();
  await env.SOCIAL.put(`sim:${simId}`, JSON.stringify(sim));

  return json({ r2_key: r2Key, size_bytes: body.byteLength }, 200, cors);
}

// Multipart upload: Create — initiates an R2 multipart upload for large .pak files
async function handleMultipartCreate(request, simId, env, cors) {
  const auth = await verifyAuth(request, env);
  if (!auth) return json({ error: 'Unauthorized' }, 401, cors);

  const simData = await env.SOCIAL.get(`sim:${simId}`);
  if (!simData) return json({ error: 'Simulation not found' }, 404, cors);
  const sim = JSON.parse(simData);
  if (sim.author_id !== auth.userId) return json({ error: 'Not your simulation' }, 403, cors);

  const r2Key = `universes/${simId}/universe.pak`;
  const multipart = await env.SCENES.createMultipartUpload(r2Key, {
    httpMetadata: { contentType: 'application/octet-stream' },
    customMetadata: { simId, authorId: auth.userId, uploadedAt: new Date().toISOString() },
  });

  return json({ upload_id: multipart.uploadId, r2_key: r2Key }, 200, cors);
}

// Multipart upload: Part — uploads a single chunk (100MB max per part)
async function handleMultipartPart(request, simId, env, cors) {
  const auth = await verifyAuth(request, env);
  if (!auth) return json({ error: 'Unauthorized' }, 401, cors);

  const url = new URL(request.url);
  const uploadId = url.searchParams.get('upload_id');
  const partNumber = parseInt(url.searchParams.get('part_number') || '1');

  if (!uploadId) return json({ error: 'Missing upload_id' }, 400, cors);

  const r2Key = `universes/${simId}/universe.pak`;
  const multipart = env.SCENES.resumeMultipartUpload(r2Key, uploadId);

  const body = await request.arrayBuffer();
  const part = await multipart.uploadPart(partNumber, body);

  return json({ part_number: partNumber, etag: part.etag, size: body.byteLength }, 200, cors);
}

// Multipart upload: Complete — assembles all parts into the final object
async function handleMultipartComplete(request, simId, env, cors) {
  const auth = await verifyAuth(request, env);
  if (!auth) return json({ error: 'Unauthorized' }, 401, cors);

  const { upload_id, parts, total_size } = await request.json();
  if (!upload_id || !parts) return json({ error: 'Missing upload_id or parts' }, 400, cors);

  const r2Key = `universes/${simId}/universe.pak`;
  const multipart = env.SCENES.resumeMultipartUpload(r2Key, upload_id);

  // parts = [{ part_number, etag }, ...]
  const uploadedParts = parts.map(p => ({
    partNumber: p.part_number,
    etag: p.etag,
  }));

  await multipart.complete(uploadedParts);

  // Update simulation record
  const simData = await env.SOCIAL.get(`sim:${simId}`);
  if (simData) {
    const sim = JSON.parse(simData);
    sim.r2_key = r2Key;
    sim.scene_size_bytes = total_size || 0;
    sim.updated_at = new Date().toISOString();
    await env.SOCIAL.put(`sim:${simId}`, JSON.stringify(sim));
  }

  return json({ r2_key: r2Key, complete: true }, 200, cors);
}

// Upload a single Space .pak to R2 (incremental update, like git push for one directory)
async function handleUploadSingleSpace(request, simId, spaceName, env, cors) {
  const auth = await verifyAuth(request, env);
  if (!auth) return json({ error: 'Unauthorized' }, 401, cors);

  const simData = await env.SOCIAL.get(`sim:${simId}`);
  if (!simData) return json({ error: 'Simulation not found' }, 404, cors);
  const sim = JSON.parse(simData);
  if (sim.author_id !== auth.userId) return json({ error: 'Not your simulation' }, 403, cors);

  const body = await request.arrayBuffer();
  if (!body || body.byteLength === 0) return json({ error: 'Empty body' }, 400, cors);
  if (body.byteLength > 500 * 1024 * 1024)
    return json({ error: 'Space file too large (max 500MB)' }, 413, cors);

  const decodedName = decodeURIComponent(spaceName);
  const r2Key = `universes/${simId}/spaces/${decodedName}.pak`;
  await env.SCENES.put(r2Key, body, {
    httpMetadata: { contentType: 'application/octet-stream' },
    customMetadata: { simId, spaceName: decodedName, authorId: auth.userId, uploadedAt: new Date().toISOString() },
  });

  // Track individual space uploads in the simulation record
  if (!sim.spaces) sim.spaces = {};
  sim.spaces[decodedName] = { r2_key: r2Key, size_bytes: body.byteLength, updated_at: new Date().toISOString() };
  sim.updated_at = new Date().toISOString();
  await env.SOCIAL.put(`sim:${simId}`, JSON.stringify(sim));

  return json({ r2_key: r2Key, space: decodedName, size_bytes: body.byteLength }, 200, cors);
}

// Upload thumbnail image to R2
async function handleUploadThumbnail(request, simId, env, cors) {
  const auth = await verifyAuth(request, env);
  if (!auth) return json({ error: 'Unauthorized' }, 401, cors);

  const simData = await env.SOCIAL.get(`sim:${simId}`);
  if (!simData) return json({ error: 'Simulation not found' }, 404, cors);
  const sim = JSON.parse(simData);
  if (sim.author_id !== auth.userId) return json({ error: 'Not your simulation' }, 403, cors);

  const body = await request.arrayBuffer();
  if (!body || body.byteLength === 0) return json({ error: 'Empty body' }, 400, cors);

  // Max 5MB for thumbnail
  if (body.byteLength > 5 * 1024 * 1024)
    return json({ error: 'Thumbnail too large (max 5MB)' }, 413, cors);

  const contentType = request.headers.get('content-type') || 'image/webp';
  const ext = contentType.includes('png') ? 'png' : contentType.includes('jpeg') ? 'jpg' : 'webp';
  const r2Key = `thumbnails/${simId}/thumb.${ext}`;

  await env.SCENES.put(r2Key, body, {
    httpMetadata: { contentType },
    customMetadata: { simId, authorId: auth.userId },
  });

  // Build public thumbnail URL
  const thumbnailUrl = `https://simulations.eustress.dev/${r2Key}`;
  sim.thumbnail_url = thumbnailUrl;
  sim.updated_at = new Date().toISOString();
  await env.SOCIAL.put(`sim:${simId}`, JSON.stringify(sim));

  return json({ thumbnail_url: thumbnailUrl }, 200, cors);
}

// Return the authenticated user's published simulations as "projects"
async function handleUserProjects(request, url, env, cors) {
  const limit = parseInt(url.searchParams.get('limit') || '50');
  const page = parseInt(url.searchParams.get('page') || '1');

  const auth = await verifyAuth(request, env);
  if (!auth) {
    // Not signed in — return empty list (not an error, just no projects)
    return json({ projects: [], total: 0, page, limit }, 200, cors);
  }

  try {
    // Find all simulations authored by this user via sim-author:{userId}:* keys
    const list = await env.SOCIAL.list({ prefix: `sim-author:${auth.userId}:`, limit: 100 });
    const projects = [];

    for (const key of list.keys) {
      const simId = await env.SOCIAL.get(key.name);
      if (!simId) continue;
      const simData = await env.SOCIAL.get(`sim:${simId}`);
      if (!simData) continue;

      try {
        const sim = JSON.parse(simData);
        projects.push({
          id: sim.id || simId,
          name: sim.name || 'Untitled',
          description: sim.description || null,
          thumbnail_url: sim.thumbnail_url || null,
          status: 'published',
          genre: sim.genre || 'All',
          max_players: sim.max_players || 10,
          is_public: sim.is_public !== false,
          version: sim.version || 1,
          play_count: sim.play_count || 0,
          favorite_count: sim.favorite_count || 0,
          last_edited: sim.updated_at || sim.published_at || '',
          created_at: sim.published_at || '',
          published_at: sim.published_at || null,
          storage_url: sim.r2_key || null,
        });
      } catch (_) {}
    }

    // Sort by last_edited descending
    projects.sort((a, b) => new Date(b.last_edited) - new Date(a.last_edited));

    return json({ projects, total: projects.length, page, limit }, 200, cors);
  } catch (e) {
    return json({ projects: [], total: 0, page, limit }, 200, cors);
  }
}

async function handleListSimulations(env, cors) {
  try {
    const list = await env.SOCIAL.list({ prefix: 'sim:', limit: 100 });
    const sims = [];

    for (const key of list.keys) {
      if (key.name.startsWith('sim-author:')) continue;
      if (key.name.startsWith('simCount:')) continue;
      const data = await env.SOCIAL.get(key.name);
      if (data) {
        try { sims.push(JSON.parse(data)); } catch (_) {}
      }
    }

    sims.sort((a, b) => new Date(b.published_at) - new Date(a.published_at));
    return json({ simulations: sims, total: sims.length }, 200, cors);
  } catch (e) {
    return json({ simulations: [], total: 0 }, 200, cors);
  }
}

async function handleGetSimulation(simId, env, cors) {
  const data = await env.SOCIAL.get(`sim:${simId}`);
  if (!data) return json({ error: 'Simulation not found' }, 404, cors);
  return json(JSON.parse(data), 200, cors);
}

// Play a simulation — returns server connection info
// Download .pak — streams the R2 object directly to the caller.
// Public simulations: no auth required. Private: requires auth + ownership.
async function handleDownloadPak(request, simId, env, cors) {
  const simData = await env.SOCIAL.get(`sim:${simId}`);
  if (!simData) return json({ error: 'Simulation not found' }, 404, cors);
  const sim = JSON.parse(simData);

  // Private simulations require auth
  if (!sim.is_public) {
    const auth = await verifyAuth(request, env);
    if (!auth || auth.userId !== sim.author_id)
      return json({ error: 'Private simulation — access denied' }, 403, cors);
  }

  if (!sim.r2_key) return json({ error: 'No published .pak' }, 404, cors);

  const object = await env.SCENES.get(sim.r2_key);
  if (!object) return json({ error: '.pak not found in storage' }, 404, cors);

  return new Response(object.body, {
    headers: {
      ...cors,
      'Content-Type': 'application/octet-stream',
      'Content-Disposition': `attachment; filename="${sim.name || simId}.pak"`,
      'Content-Length': object.size.toString(),
    },
  });
}

async function handlePlaySimulation(request, simId, env, cors) {
  const data = await env.SOCIAL.get(`sim:${simId}`);
  if (!data) return json({ error: 'Simulation not found' }, 404, cors);

  const sim = JSON.parse(data);

  // Private simulations require auth
  if (!sim.is_public) {
    const auth = await verifyAuth(request, env);
    if (!auth) return json({ error: 'Private simulation — sign in required' }, 401, cors);
  }

  // Increment play count
  sim.play_count = (sim.play_count || 0) + 1;
  await env.SOCIAL.put(`sim:${simId}`, JSON.stringify(sim));

  // Increment author's total plays
  if (sim.author_id) {
    const plays = parseInt(await env.SOCIAL.get(`totalPlays:${sim.author_id}`) || '0') + 1;
    await env.SOCIAL.put(`totalPlays:${sim.author_id}`, plays.toString());
  }

  // Check for an active server running this simulation
  const nodeList = await env.SOCIAL.list({ prefix: 'node:', limit: 100 });
  let activeServer = null;

  for (const key of nodeList.keys) {
    const nodeData = await env.SOCIAL.get(key.name);
    if (nodeData) {
      const node = JSON.parse(nodeData);
      if (node.simulation_id === simId && node.players < (sim.max_players || 100)) {
        activeServer = node;
        break;
      }
    }
  }

  if (activeServer) {
    // Existing server has room
    return json({
      status: 'ready',
      server: {
        node_id: activeServer.node_id,
        address: activeServer.address || 'localhost',
        port: activeServer.port || 7777,
        protocol: 'quic',
        players: activeServer.players,
        max_players: sim.max_players || 100,
      },
      simulation: { id: sim.id, name: sim.name },
    }, 200, cors);
  }

  // No active server — return launch instructions
  // In production: Forge SDK dispatches Nomad job here
  // For now: client launches local server
  return json({
    status: 'spawn',
    launch: {
      command: 'eustress-server',
      args: [
        '--port', '7777',
        '--max-players', (sim.max_players || 100).toString(),
        '--sim-id', simId,
      ],
      r2_key: sim.r2_key || null,
      pak_url: sim.r2_key ? `https://simulations.eustress.dev/${sim.r2_key}` : null,
    },
    simulation: { id: sim.id, name: sim.name, description: sim.description },
  }, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// ACCOUNTING — Revenue dashboard, cost tracking, automated flow
// ═══════════════════════════════════════════════════════════════════════════

// Monthly infrastructure costs (auto-deducted daily as 1/30th)
const INFRA_COSTS = {
  cloudflare_workers: 5.00,    // Workers Paid plan
  domain: 0.83,                // $10/year amortized
  stripe_connect: 0,           // Charged per-transaction, not monthly
  forge_base: 19.50,           // Nomad cluster base (scales with usage)
  r2_storage: 0.75,            // Asset storage estimate
  total_monthly: function() {
    return this.cloudflare_workers + this.domain + this.forge_base + this.r2_storage;
  },
  daily: function() {
    return this.total_monthly() / 30;
  }
};

async function handleAccountingDashboard(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  // Revenue
  const treasuryUsd = parseFloat(await env.PAYOUTS.get('treasury:total_usd') || '0');
  const platformUsd = parseFloat(await env.PAYOUTS.get('platform:total_usd') || '0');
  const depositCount = parseInt(await env.PAYOUTS.get('treasury:deposit_count') || '0');

  // Costs
  const totalCostsDeducted = parseFloat(await env.PAYOUTS.get('costs:total_deducted') || '0');
  const forgeCosts = parseFloat(await env.PAYOUTS.get('costs:forge') || '0');
  const stripeFees = parseFloat(await env.PAYOUTS.get('costs:stripe_fees') || '0');
  const infraCosts = parseFloat(await env.PAYOUTS.get('costs:infrastructure') || '0');

  // Payouts
  const totalPaidToContributors = parseFloat(await env.PAYOUTS.get('payouts:total_paid') || '0');

  // Daily metrics
  const dripRate = 0.00276;
  const dailyDrip = treasuryUsd * dripRate;           // 100% to contributors
  const dailyInfraCost = INFRA_COSTS.daily();          // Paid from platform revenue
  const platformNetDaily = (platformUsd / 30) - dailyInfraCost; // Platform profit after costs

  // User count
  const userList = await env.USERS.list({ prefix: 'username:', limit: 1000 });
  const totalUsers = userList.keys.length;

  return json({
    revenue: {
      treasury_balance: treasuryUsd,
      platform_balance: platformUsd,
      total_deposits: depositCount,
      total_revenue: treasuryUsd + platformUsd + totalCostsDeducted + totalPaidToContributors,
    },
    costs: {
      monthly_infrastructure: INFRA_COSTS.total_monthly(),
      daily_infrastructure: dailyInfraCost,
      total_deducted: totalCostsDeducted,
      breakdown: {
        cloudflare: INFRA_COSTS.cloudflare_workers,
        domain: INFRA_COSTS.domain,
        forge_servers: INFRA_COSTS.forge_base,
        r2_storage: INFRA_COSTS.r2_storage,
        stripe_fees: stripeFees,
        forge_compute: forgeCosts,
      },
    },
    contributors: {
      treasury_balance: treasuryUsd,
      daily_drip: dailyDrip,
      total_paid: totalPaidToContributors,
      note: '100% of treasury drip goes to contributors. No deductions.',
    },
    platform: {
      revenue: platformUsd,
      costs_paid: totalCostsDeducted,
      net_profit: platformUsd - totalCostsDeducted,
      daily_net: platformNetDaily,
      note: 'Costs paid from platform 50%, never from contributor treasury.',
    },
    users: {
      total_registered: totalUsers,
    },
    health: {
      profitable: platformUsd > totalCostsDeducted,
      runway_days: dailyInfraCost > 0 ? Math.floor(platformUsd / dailyInfraCost) : 999,
      margin_percent: platformUsd > 0
        ? ((platformUsd - totalCostsDeducted) / platformUsd * 100).toFixed(1) + '%'
        : 'N/A',
    },
    timestamp: new Date().toISOString(),
  }, 200, cors);
}

// Record a cost (called by Forge autoscaler or admin)
async function handleRecordCost(request, env, cors) {
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  const { category, amount, description } = await request.json();
  if (!category || !amount) return json({ error: 'category and amount required' }, 400, cors);

  // Accumulate cost
  const key = `costs:${category}`;
  const current = parseFloat(await env.PAYOUTS.get(key) || '0');
  await env.PAYOUTS.put(key, (current + amount).toString());

  // Track total
  const totalKey = 'costs:total_deducted';
  const total = parseFloat(await env.PAYOUTS.get(totalKey) || '0');
  await env.PAYOUTS.put(totalKey, (total + amount).toString());

  // Log
  await env.PAYOUTS.put(`cost:${Date.now()}`, JSON.stringify({
    category, amount, description: description || '',
    timestamp: new Date().toISOString(), recorded_by: adminId,
  }), { expirationTtl: 86400 * 365 * 5 });

  await auditLog(env, 'RECORD_COST', adminId, category, { amount, description });

  return json({ success: true, category, amount, new_total: current + amount }, 200, cors);
}

// ═══════════════════════════════════════════════════════════════════════════
// CRON — Daily payout (called by scheduled trigger at UTC midnight)
// ═══════════════════════════════════════════════════════════════════════════

async function runDailyPayout(env) {
  if (!env.STRIPE_SECRET_KEY) return;

  const treasuryUsd = parseFloat(await env.PAYOUTS.get('treasury:total_usd') || '0');
  if (treasuryUsd <= 0) return;

  const dripRate = 0.00276;
  const dailyDripUsd = treasuryUsd * dripRate;
  if (dailyDripUsd < 0.50) return;

  const userList = await env.USERS.list({ prefix: 'user:', limit: 1000 });
  const contributors = [];
  let totalScore = 0;

  for (const key of userList.keys) {
    const data = await env.USERS.get(key.name);
    if (!data) continue;
    const user = JSON.parse(data);
    const score = user.contribution_score || 0;
    if (score > 0 && user.stripe_connect_id && !user.banned) {
      contributors.push({ user_id: user.id, username: user.username, score, connect_id: user.stripe_connect_id });
      totalScore += score;
    }
  }

  if (totalScore <= 0 || contributors.length === 0) return;

  let totalPaid = 0;
  const payouts = [];

  for (const c of contributors) {
    const share = c.score / totalScore;
    const amountCents = Math.floor(dailyDripUsd * share * 100);
    if (amountCents < 50) continue;

    try {
      const transfer = await stripeRequest('POST', '/transfers', {
        'amount': amountCents.toString(),
        'currency': 'usd',
        'destination': c.connect_id,
        'description': `Bliss daily payout - ${c.username}`,
        'metadata[user_id]': c.user_id,
        'metadata[date]': new Date().toISOString().split('T')[0],
      }, env);

      if (!transfer.error) {
        payouts.push({ user_id: c.user_id, username: c.username, amount_usd: amountCents / 100, transfer_id: transfer.id });
        totalPaid += amountCents / 100;
      }
    } catch (e) { /* skip */ }
  }

  // Debit treasury
  await env.PAYOUTS.put('treasury:total_usd', (treasuryUsd - totalPaid).toString());

  // Record
  const payoutKey = `payout:${new Date().toISOString().split('T')[0]}`;
  await env.PAYOUTS.put(payoutKey, JSON.stringify({
    date: new Date().toISOString(), drip_usd: dailyDripUsd, total_paid_usd: totalPaid,
    contributors_paid: payouts.length, treasury_before: treasuryUsd, treasury_after: treasuryUsd - totalPaid, payouts,
  }), { expirationTtl: 86400 * 365 * 5 });
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
