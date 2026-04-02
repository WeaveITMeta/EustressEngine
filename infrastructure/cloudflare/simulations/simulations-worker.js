// =============================================================================
// Eustress Simulations Worker - Publishing & Asset Delivery
// =============================================================================
// Deploy: wrangler deploy
// Bindings required:
//   - SIMULATIONS: R2 bucket (eustress-simulations)
//   - JWT_SECRET: Secret for token validation
// =============================================================================

// CORS headers for all responses
const corsHeaders = {
  'Access-Control-Allow-Origin': '*',
  'Access-Control-Allow-Methods': 'GET, POST, PUT, DELETE, OPTIONS',
  'Access-Control-Allow-Headers': 'Content-Type, Authorization',
};

// JWT validation (simplified - in production use a proper library)
async function validateJWT(token, secret) {
  if (!token || !secret) return null;
  
  try {
    const [headerB64, payloadB64, signature] = token.split('.');
    if (!headerB64 || !payloadB64 || !signature) return null;
    
    const payload = JSON.parse(atob(payloadB64.replace(/-/g, '+').replace(/_/g, '/')));
    
    // Check expiration
    if (payload.exp && payload.exp < Date.now() / 1000) {
      return null;
    }
    
    // In production, verify signature with crypto.subtle
    // For now, trust the payload if it has required fields
    if (!payload.sub || !payload.user_id) return null;
    
    return payload;
  } catch (e) {
    console.error('JWT validation error:', e);
    return null;
  }
}

// Generate presigned URL for R2 upload
async function generatePresignedUrl(bucket, key, expiresIn = 3600) {
  // R2 presigned URLs require the bucket to have public access or use signed URLs
  // For now, we'll return the key and let the worker handle the upload
  return {
    key,
    uploadUrl: `/api/simulation/upload/${encodeURIComponent(key)}`,
    expiresAt: new Date(Date.now() + expiresIn * 1000).toISOString(),
  };
}

// Main request handler
export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    const path = url.pathname;

    // Handle CORS preflight
    if (request.method === 'OPTIONS') {
      return new Response(null, { headers: corsHeaders });
    }

    // =========================================================================
    // Public Routes (no auth required)
    // =========================================================================

    // GET /api/simulation/:id - Get simulation manifest
    if (path.match(/^\/api\/simulation\/([a-f0-9-]+)$/) && request.method === 'GET') {
      const simulationId = path.split('/')[3];
      return await getSimulation(env, simulationId);
    }

    // GET /api/simulation/:id/download - Download simulation package
    if (path.match(/^\/api\/simulation\/([a-f0-9-]+)\/download$/) && request.method === 'GET') {
      const simulationId = path.split('/')[3];
      return await downloadSimulation(env, simulationId);
    }

    // GET /api/simulation/:id/thumbnail - Get thumbnail
    if (path.match(/^\/api\/simulation\/([a-f0-9-]+)\/thumbnail$/) && request.method === 'GET') {
      const simulationId = path.split('/')[3];
      return await getThumbnail(env, simulationId);
    }

    // GET /api/simulation/:id/versions - List versions
    if (path.match(/^\/api\/simulation\/([a-f0-9-]+)\/versions$/) && request.method === 'GET') {
      const simulationId = path.split('/')[3];
      return await listVersions(env, simulationId);
    }

    // =========================================================================
    // Protected Routes (auth required)
    // =========================================================================

    // Extract and validate JWT
    const authHeader = request.headers.get('Authorization');
    const token = authHeader?.replace('Bearer ', '');
    const user = await validateJWT(token, env.JWT_SECRET);

    if (!user && path.startsWith('/api/simulation/') && 
        (request.method === 'POST' || request.method === 'PUT' || request.method === 'DELETE')) {
      return new Response(JSON.stringify({ error: 'Unauthorized' }), {
        status: 401,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    // POST /api/simulation/publish - Initiate new simulation publish
    if (path === '/api/simulation/publish' && request.method === 'POST') {
      return await initiatePublish(env, request, user);
    }

    // PUT /api/simulation/:id - Update existing simulation
    if (path.match(/^\/api\/simulation\/([a-f0-9-]+)$/) && request.method === 'PUT') {
      const simulationId = path.split('/')[3];
      return await updateSimulation(env, request, user, simulationId);
    }

    // POST /api/simulation/upload/:key - Direct upload to R2 (with presigned validation)
    if (path.startsWith('/api/simulation/upload/') && request.method === 'POST') {
      const key = decodeURIComponent(path.replace('/api/simulation/upload/', ''));
      return await handleUpload(env, request, user, key);
    }

    // POST /api/simulation/:id/commit - Finalize publish after uploads complete
    if (path.match(/^\/api\/simulation\/([a-f0-9-]+)\/commit$/) && request.method === 'POST') {
      const simulationId = path.split('/')[3];
      return await commitPublish(env, request, user, simulationId);
    }

    // DELETE /api/simulation/:id - Delete simulation
    if (path.match(/^\/api\/simulation\/([a-f0-9-]+)$/) && request.method === 'DELETE') {
      const simulationId = path.split('/')[3];
      return await deleteSimulation(env, request, user, simulationId);
    }

    // 404 for unknown routes
    return new Response(JSON.stringify({ error: 'Not found' }), {
      status: 404,
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  },
};

// =============================================================================
// Route Handlers
// =============================================================================

// Get simulation manifest
async function getSimulation(env, simulationId) {
  try {
    const manifest = await env.SIMULATIONS.get(`${simulationId}/manifest.json`);
    if (!manifest) {
      return new Response(JSON.stringify({ error: 'Simulation not found' }), {
        status: 404,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    return new Response(manifest.body, {
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  } catch (e) {
    return new Response(JSON.stringify({ error: 'Failed to fetch simulation' }), {
      status: 500,
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  }
}

// Download simulation scene package
async function downloadSimulation(env, simulationId) {
  try {
    const scene = await env.SIMULATIONS.get(`${simulationId}/scene.eustress`);
    if (!scene) {
      return new Response(JSON.stringify({ error: 'Scene not found' }), {
        status: 404,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    return new Response(scene.body, {
      headers: {
        'Content-Type': 'application/octet-stream',
        'Content-Disposition': `attachment; filename="scene.eustress"`,
        'Content-Length': scene.size,
        ...corsHeaders,
      },
    });
  } catch (e) {
    return new Response(JSON.stringify({ error: 'Failed to download scene' }), {
      status: 500,
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  }
}

// Get simulation thumbnail
async function getThumbnail(env, simulationId) {
  try {
    const thumbnail = await env.SIMULATIONS.get(`${simulationId}/thumbnail.webp`);
    if (!thumbnail) {
      // Return default thumbnail
      return new Response(null, { status: 404, headers: corsHeaders });
    }

    return new Response(thumbnail.body, {
      headers: {
        'Content-Type': 'image/webp',
        'Cache-Control': 'public, max-age=3600',
        ...corsHeaders,
      },
    });
  } catch (e) {
    return new Response(null, { status: 500, headers: corsHeaders });
  }
}

// List simulation versions
async function listVersions(env, simulationId) {
  try {
    const list = await env.SIMULATIONS.list({ prefix: `${simulationId}/versions/` });
    const versions = [];

    for (const object of list.objects) {
      // Extract version number from path like "uuid/versions/v1/manifest.json"
      const match = object.key.match(/versions\/v(\d+)\/manifest\.json$/);
      if (match) {
        const versionManifest = await env.SIMULATIONS.get(object.key);
        if (versionManifest) {
          const data = await versionManifest.json();
          versions.push({
            version: parseInt(match[1]),
            published_at: data.published_at,
            changelog: data.changelog || null,
          });
        }
      }
    }

    versions.sort((a, b) => b.version - a.version);

    return new Response(JSON.stringify({ versions }), {
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  } catch (e) {
    return new Response(JSON.stringify({ error: 'Failed to list versions' }), {
      status: 500,
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  }
}

// Initiate new simulation publish - returns presigned URLs
async function initiatePublish(env, request, user) {
  try {
    const body = await request.json();
    const { name, description, genre, max_players, is_public, allow_copying } = body;

    if (!name || name.trim().length === 0) {
      return new Response(JSON.stringify({ error: 'Simulation name is required' }), {
        status: 400,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    // Generate new simulation ID
    const simulationId = crypto.randomUUID();
    const version = 1;

    // Create manifest
    const manifest = {
      id: simulationId,
      name: name.trim(),
      description: description || '',
      genre: genre || 'all_genres',
      max_players: max_players || 10,
      is_public: is_public !== false,
      allow_copying: allow_copying || false,
      author_id: user.user_id,
      author_name: user.username || 'Unknown',
      version,
      created_at: new Date().toISOString(),
      published_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
    };

    // Generate upload URLs for assets
    const uploadUrls = {
      scene: await generatePresignedUrl(env.SIMULATIONS, `${simulationId}/scene.eustress`),
      thumbnail: await generatePresignedUrl(env.SIMULATIONS, `${simulationId}/thumbnail.webp`),
      // Assets will be uploaded to simulationId/assets/...
    };

    // Store pending manifest (will be finalized on commit)
    await env.SIMULATIONS.put(
      `${simulationId}/pending-manifest.json`,
      JSON.stringify(manifest),
      { customMetadata: { status: 'pending', user_id: user.user_id } }
    );

    return new Response(JSON.stringify({
      simulation_id: simulationId,
      version,
      upload_urls: uploadUrls,
      manifest,
    }), {
      status: 201,
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  } catch (e) {
    console.error('Publish initiation error:', e);
    return new Response(JSON.stringify({ error: 'Failed to initiate publish' }), {
      status: 500,
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  }
}

// Update existing simulation
async function updateSimulation(env, request, user, simulationId) {
  try {
    // Check ownership
    const existingManifest = await env.SIMULATIONS.get(`${simulationId}/manifest.json`);
    if (!existingManifest) {
      return new Response(JSON.stringify({ error: 'Simulation not found' }), {
        status: 404,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    const existing = await existingManifest.json();
    if (existing.author_id !== user.user_id) {
      return new Response(JSON.stringify({ error: 'Not authorized to update this simulation' }), {
        status: 403,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    const body = await request.json();
    const newVersion = existing.version + 1;

    // Update manifest
    const manifest = {
      ...existing,
      name: body.name || existing.name,
      description: body.description !== undefined ? body.description : existing.description,
      genre: body.genre || existing.genre,
      max_players: body.max_players || existing.max_players,
      is_public: body.is_public !== undefined ? body.is_public : existing.is_public,
      allow_copying: body.allow_copying !== undefined ? body.allow_copying : existing.allow_copying,
      version: newVersion,
      updated_at: new Date().toISOString(),
      published_at: new Date().toISOString(),
    };

    // Generate upload URLs
    const uploadUrls = {
      scene: await generatePresignedUrl(env.SIMULATIONS, `${simulationId}/scene.eustress`),
      thumbnail: await generatePresignedUrl(env.SIMULATIONS, `${simulationId}/thumbnail.webp`),
    };

    // Store pending manifest
    await env.SIMULATIONS.put(
      `${simulationId}/pending-manifest.json`,
      JSON.stringify(manifest),
      { customMetadata: { status: 'pending', user_id: user.user_id, previous_version: existing.version.toString() } }
    );

    return new Response(JSON.stringify({
      simulation_id: simulationId,
      version: newVersion,
      upload_urls: uploadUrls,
      manifest,
    }), {
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  } catch (e) {
    console.error('Update error:', e);
    return new Response(JSON.stringify({ error: 'Failed to update simulation' }), {
      status: 500,
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  }
}

// Handle direct upload to R2
async function handleUpload(env, request, user, key) {
  try {
    // Validate key belongs to user's simulation
    const simulationId = key.split('/')[0];
    const pendingManifest = await env.SIMULATIONS.head(`${simulationId}/pending-manifest.json`);
    
    if (!pendingManifest || pendingManifest.customMetadata?.user_id !== user.user_id) {
      return new Response(JSON.stringify({ error: 'Not authorized to upload to this simulation' }), {
        status: 403,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    // Get content type from request
    const contentType = request.headers.get('Content-Type') || 'application/octet-stream';

    // Upload to R2
    await env.SIMULATIONS.put(key, request.body, {
      httpMetadata: { contentType },
    });

    return new Response(JSON.stringify({ success: true, key }), {
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  } catch (e) {
    console.error('Upload error:', e);
    return new Response(JSON.stringify({ error: 'Upload failed' }), {
      status: 500,
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  }
}

// Commit publish - finalize after all uploads complete
async function commitPublish(env, request, user, simulationId) {
  try {
    // Get pending manifest
    const pendingObj = await env.SIMULATIONS.get(`${simulationId}/pending-manifest.json`);
    if (!pendingObj) {
      return new Response(JSON.stringify({ error: 'No pending publish found' }), {
        status: 404,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    const pendingMeta = await env.SIMULATIONS.head(`${simulationId}/pending-manifest.json`);
    if (pendingMeta.customMetadata?.user_id !== user.user_id) {
      return new Response(JSON.stringify({ error: 'Not authorized' }), {
        status: 403,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    const manifest = await pendingObj.json();

    // Verify scene was uploaded
    const sceneExists = await env.SIMULATIONS.head(`${simulationId}/scene.eustress`);
    if (!sceneExists) {
      return new Response(JSON.stringify({ error: 'Scene file not uploaded' }), {
        status: 400,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    // Archive previous version if exists
    const previousVersion = pendingMeta.customMetadata?.previous_version;
    if (previousVersion) {
      const prevManifest = await env.SIMULATIONS.get(`${simulationId}/manifest.json`);
      const prevScene = await env.SIMULATIONS.get(`${simulationId}/scene.eustress`);
      
      if (prevManifest) {
        await env.SIMULATIONS.put(
          `${simulationId}/versions/v${previousVersion}/manifest.json`,
          prevManifest.body
        );
      }
      if (prevScene) {
        await env.SIMULATIONS.put(
          `${simulationId}/versions/v${previousVersion}/scene.eustress`,
          prevScene.body
        );
      }
    }

    // Finalize manifest
    await env.SIMULATIONS.put(`${simulationId}/manifest.json`, JSON.stringify(manifest));

    // Clean up pending
    await env.SIMULATIONS.delete(`${simulationId}/pending-manifest.json`);

    // TODO: Notify backend to update PostgreSQL and trigger notifications
    // await notifyBackend(env, simulationId, manifest);

    return new Response(JSON.stringify({
      success: true,
      simulation_id: simulationId,
      version: manifest.version,
      url: `https://simulations.eustress.dev/api/simulation/${simulationId}`,
    }), {
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  } catch (e) {
    console.error('Commit error:', e);
    return new Response(JSON.stringify({ error: 'Failed to commit publish' }), {
      status: 500,
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  }
}

// Delete simulation
async function deleteSimulation(env, request, user, simulationId) {
  try {
    // Check ownership
    const manifest = await env.SIMULATIONS.get(`${simulationId}/manifest.json`);
    if (!manifest) {
      return new Response(JSON.stringify({ error: 'Simulation not found' }), {
        status: 404,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    const data = await manifest.json();
    if (data.author_id !== user.user_id) {
      return new Response(JSON.stringify({ error: 'Not authorized to delete this simulation' }), {
        status: 403,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    // List and delete all objects for this simulation
    const list = await env.SIMULATIONS.list({ prefix: `${simulationId}/` });
    for (const object of list.objects) {
      await env.SIMULATIONS.delete(object.key);
    }

    return new Response(JSON.stringify({ success: true }), {
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  } catch (e) {
    console.error('Delete error:', e);
    return new Response(JSON.stringify({ error: 'Failed to delete simulation' }), {
      status: 500,
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  }
}
