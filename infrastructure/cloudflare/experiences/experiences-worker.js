// =============================================================================
// Eustress Experiences Worker - Publishing & Asset Delivery
// =============================================================================
// Deploy: wrangler deploy
// Bindings required:
//   - EXPERIENCES: R2 bucket (eustress-experiences)
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
    uploadUrl: `/api/experience/upload/${encodeURIComponent(key)}`,
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

    // GET /api/experience/:id - Get experience manifest
    if (path.match(/^\/api\/experience\/([a-f0-9-]+)$/) && request.method === 'GET') {
      const experienceId = path.split('/')[3];
      return await getExperience(env, experienceId);
    }

    // GET /api/experience/:id/download - Download experience package
    if (path.match(/^\/api\/experience\/([a-f0-9-]+)\/download$/) && request.method === 'GET') {
      const experienceId = path.split('/')[3];
      return await downloadExperience(env, experienceId);
    }

    // GET /api/experience/:id/thumbnail - Get thumbnail
    if (path.match(/^\/api\/experience\/([a-f0-9-]+)\/thumbnail$/) && request.method === 'GET') {
      const experienceId = path.split('/')[3];
      return await getThumbnail(env, experienceId);
    }

    // GET /api/experience/:id/versions - List versions
    if (path.match(/^\/api\/experience\/([a-f0-9-]+)\/versions$/) && request.method === 'GET') {
      const experienceId = path.split('/')[3];
      return await listVersions(env, experienceId);
    }

    // =========================================================================
    // Protected Routes (auth required)
    // =========================================================================

    // Extract and validate JWT
    const authHeader = request.headers.get('Authorization');
    const token = authHeader?.replace('Bearer ', '');
    const user = await validateJWT(token, env.JWT_SECRET);

    if (!user && path.startsWith('/api/experience/') && 
        (request.method === 'POST' || request.method === 'PUT' || request.method === 'DELETE')) {
      return new Response(JSON.stringify({ error: 'Unauthorized' }), {
        status: 401,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    // POST /api/experience/publish - Initiate new experience publish
    if (path === '/api/experience/publish' && request.method === 'POST') {
      return await initiatePublish(env, request, user);
    }

    // PUT /api/experience/:id - Update existing experience
    if (path.match(/^\/api\/experience\/([a-f0-9-]+)$/) && request.method === 'PUT') {
      const experienceId = path.split('/')[3];
      return await updateExperience(env, request, user, experienceId);
    }

    // POST /api/experience/upload/:key - Direct upload to R2 (with presigned validation)
    if (path.startsWith('/api/experience/upload/') && request.method === 'POST') {
      const key = decodeURIComponent(path.replace('/api/experience/upload/', ''));
      return await handleUpload(env, request, user, key);
    }

    // POST /api/experience/:id/commit - Finalize publish after uploads complete
    if (path.match(/^\/api\/experience\/([a-f0-9-]+)\/commit$/) && request.method === 'POST') {
      const experienceId = path.split('/')[3];
      return await commitPublish(env, request, user, experienceId);
    }

    // DELETE /api/experience/:id - Delete experience
    if (path.match(/^\/api\/experience\/([a-f0-9-]+)$/) && request.method === 'DELETE') {
      const experienceId = path.split('/')[3];
      return await deleteExperience(env, request, user, experienceId);
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

// Get experience manifest
async function getExperience(env, experienceId) {
  try {
    const manifest = await env.EXPERIENCES.get(`${experienceId}/manifest.json`);
    if (!manifest) {
      return new Response(JSON.stringify({ error: 'Experience not found' }), {
        status: 404,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    return new Response(manifest.body, {
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  } catch (e) {
    return new Response(JSON.stringify({ error: 'Failed to fetch experience' }), {
      status: 500,
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  }
}

// Download experience scene package
async function downloadExperience(env, experienceId) {
  try {
    const scene = await env.EXPERIENCES.get(`${experienceId}/scene.eustress`);
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

// Get experience thumbnail
async function getThumbnail(env, experienceId) {
  try {
    const thumbnail = await env.EXPERIENCES.get(`${experienceId}/thumbnail.webp`);
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

// List experience versions
async function listVersions(env, experienceId) {
  try {
    const list = await env.EXPERIENCES.list({ prefix: `${experienceId}/versions/` });
    const versions = [];

    for (const object of list.objects) {
      // Extract version number from path like "uuid/versions/v1/manifest.json"
      const match = object.key.match(/versions\/v(\d+)\/manifest\.json$/);
      if (match) {
        const versionManifest = await env.EXPERIENCES.get(object.key);
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

// Initiate new experience publish - returns presigned URLs
async function initiatePublish(env, request, user) {
  try {
    const body = await request.json();
    const { name, description, genre, max_players, is_public, allow_copying } = body;

    if (!name || name.trim().length === 0) {
      return new Response(JSON.stringify({ error: 'Experience name is required' }), {
        status: 400,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    // Generate new experience ID
    const experienceId = crypto.randomUUID();
    const version = 1;

    // Create manifest
    const manifest = {
      id: experienceId,
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
      scene: await generatePresignedUrl(env.EXPERIENCES, `${experienceId}/scene.eustress`),
      thumbnail: await generatePresignedUrl(env.EXPERIENCES, `${experienceId}/thumbnail.webp`),
      // Assets will be uploaded to experienceId/assets/...
    };

    // Store pending manifest (will be finalized on commit)
    await env.EXPERIENCES.put(
      `${experienceId}/pending-manifest.json`,
      JSON.stringify(manifest),
      { customMetadata: { status: 'pending', user_id: user.user_id } }
    );

    return new Response(JSON.stringify({
      experience_id: experienceId,
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

// Update existing experience
async function updateExperience(env, request, user, experienceId) {
  try {
    // Check ownership
    const existingManifest = await env.EXPERIENCES.get(`${experienceId}/manifest.json`);
    if (!existingManifest) {
      return new Response(JSON.stringify({ error: 'Experience not found' }), {
        status: 404,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    const existing = await existingManifest.json();
    if (existing.author_id !== user.user_id) {
      return new Response(JSON.stringify({ error: 'Not authorized to update this experience' }), {
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
      scene: await generatePresignedUrl(env.EXPERIENCES, `${experienceId}/scene.eustress`),
      thumbnail: await generatePresignedUrl(env.EXPERIENCES, `${experienceId}/thumbnail.webp`),
    };

    // Store pending manifest
    await env.EXPERIENCES.put(
      `${experienceId}/pending-manifest.json`,
      JSON.stringify(manifest),
      { customMetadata: { status: 'pending', user_id: user.user_id, previous_version: existing.version.toString() } }
    );

    return new Response(JSON.stringify({
      experience_id: experienceId,
      version: newVersion,
      upload_urls: uploadUrls,
      manifest,
    }), {
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  } catch (e) {
    console.error('Update error:', e);
    return new Response(JSON.stringify({ error: 'Failed to update experience' }), {
      status: 500,
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  }
}

// Handle direct upload to R2
async function handleUpload(env, request, user, key) {
  try {
    // Validate key belongs to user's experience
    const experienceId = key.split('/')[0];
    const pendingManifest = await env.EXPERIENCES.head(`${experienceId}/pending-manifest.json`);
    
    if (!pendingManifest || pendingManifest.customMetadata?.user_id !== user.user_id) {
      return new Response(JSON.stringify({ error: 'Not authorized to upload to this experience' }), {
        status: 403,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    // Get content type from request
    const contentType = request.headers.get('Content-Type') || 'application/octet-stream';

    // Upload to R2
    await env.EXPERIENCES.put(key, request.body, {
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
async function commitPublish(env, request, user, experienceId) {
  try {
    // Get pending manifest
    const pendingObj = await env.EXPERIENCES.get(`${experienceId}/pending-manifest.json`);
    if (!pendingObj) {
      return new Response(JSON.stringify({ error: 'No pending publish found' }), {
        status: 404,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    const pendingMeta = await env.EXPERIENCES.head(`${experienceId}/pending-manifest.json`);
    if (pendingMeta.customMetadata?.user_id !== user.user_id) {
      return new Response(JSON.stringify({ error: 'Not authorized' }), {
        status: 403,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    const manifest = await pendingObj.json();

    // Verify scene was uploaded
    const sceneExists = await env.EXPERIENCES.head(`${experienceId}/scene.eustress`);
    if (!sceneExists) {
      return new Response(JSON.stringify({ error: 'Scene file not uploaded' }), {
        status: 400,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    // Archive previous version if exists
    const previousVersion = pendingMeta.customMetadata?.previous_version;
    if (previousVersion) {
      const prevManifest = await env.EXPERIENCES.get(`${experienceId}/manifest.json`);
      const prevScene = await env.EXPERIENCES.get(`${experienceId}/scene.eustress`);
      
      if (prevManifest) {
        await env.EXPERIENCES.put(
          `${experienceId}/versions/v${previousVersion}/manifest.json`,
          prevManifest.body
        );
      }
      if (prevScene) {
        await env.EXPERIENCES.put(
          `${experienceId}/versions/v${previousVersion}/scene.eustress`,
          prevScene.body
        );
      }
    }

    // Finalize manifest
    await env.EXPERIENCES.put(`${experienceId}/manifest.json`, JSON.stringify(manifest));

    // Clean up pending
    await env.EXPERIENCES.delete(`${experienceId}/pending-manifest.json`);

    // TODO: Notify backend to update PostgreSQL and trigger notifications
    // await notifyBackend(env, experienceId, manifest);

    return new Response(JSON.stringify({
      success: true,
      experience_id: experienceId,
      version: manifest.version,
      url: `https://experiences.eustress.dev/api/experience/${experienceId}`,
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

// Delete experience
async function deleteExperience(env, request, user, experienceId) {
  try {
    // Check ownership
    const manifest = await env.EXPERIENCES.get(`${experienceId}/manifest.json`);
    if (!manifest) {
      return new Response(JSON.stringify({ error: 'Experience not found' }), {
        status: 404,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    const data = await manifest.json();
    if (data.author_id !== user.user_id) {
      return new Response(JSON.stringify({ error: 'Not authorized to delete this experience' }), {
        status: 403,
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    // List and delete all objects for this experience
    const list = await env.EXPERIENCES.list({ prefix: `${experienceId}/` });
    for (const object of list.objects) {
      await env.EXPERIENCES.delete(object.key);
    }

    return new Response(JSON.stringify({ success: true }), {
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  } catch (e) {
    console.error('Delete error:', e);
    return new Response(JSON.stringify({ error: 'Failed to delete experience' }), {
      status: 500,
      headers: { 'Content-Type': 'application/json', ...corsHeaders },
    });
  }
}
