// =============================================================================
// Eustress Downloads Worker - Analytics & Versioning
// =============================================================================
// Deploy: wrangler deploy
// Bindings required:
//   - DOWNLOADS: R2 bucket (eustress-downloads)
//   - ANALYTICS: Analytics Engine dataset
// =============================================================================

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    const path = url.pathname;

    // CORS headers
    const corsHeaders = {
      'Access-Control-Allow-Origin': '*',
      'Access-Control-Allow-Methods': 'GET, HEAD, OPTIONS',
      'Access-Control-Allow-Headers': 'Content-Type',
    };

    if (request.method === 'OPTIONS') {
      return new Response(null, { headers: corsHeaders });
    }

    // Route: /api/latest - Get latest version info
    if (path === '/api/latest') {
      const object = await env.DOWNLOADS.get('latest.json');
      if (!object) {
        return new Response('Not found', { status: 404 });
      }
      return new Response(object.body, {
        headers: {
          'Content-Type': 'application/json',
          ...corsHeaders,
        },
      });
    }

    // Route: /api/download/:platform - Download with analytics
    if (path.startsWith('/api/download/')) {
      const platform = path.replace('/api/download/', '');
      const fileMap = {
        'windows': 'windows/EustressEngine-Setup.exe',
        'mac': 'mac/EustressEngine.dmg',
        'mac-arm64': 'mac/EustressEngine-arm64.dmg',
        'linux': 'linux/EustressEngine.AppImage',
        'linux-deb': 'linux/eustress-engine.deb',
        'linux-rpm': 'linux/eustress-engine.rpm',
        'redox': 'redox/eustress-engine.tar.gz',
        // Player
        'player-windows': 'player/windows/EustressPlayer-Setup.exe',
        'player-mac': 'player/mac/EustressPlayer.dmg',
        'player-linux': 'player/linux/EustressPlayer.AppImage',
        'player-android': 'player/android/EustressPlayer.apk',
      };

      const filePath = fileMap[platform];
      if (!filePath) {
        return new Response('Invalid platform', { status: 400 });
      }

      // Log download analytics
      if (env.ANALYTICS) {
        ctx.waitUntil(
          env.ANALYTICS.writeDataPoint({
            blobs: [platform, request.headers.get('CF-IPCountry') || 'unknown'],
            doubles: [1],
            indexes: [platform],
          })
        );
      }

      // Get file from R2
      const object = await env.DOWNLOADS.get(filePath);
      if (!object) {
        return new Response('File not found', { status: 404 });
      }

      const filename = filePath.split('/').pop();
      return new Response(object.body, {
        headers: {
          'Content-Type': 'application/octet-stream',
          'Content-Disposition': `attachment; filename="${filename}"`,
          'Content-Length': object.size,
          ...corsHeaders,
        },
      });
    }

    // Route: /api/stats - Download statistics (protected)
    if (path === '/api/stats') {
      // Add auth check here if needed
      // For now, return basic stats from R2 metadata
      return new Response(JSON.stringify({
        message: 'Stats endpoint - implement with Analytics Engine query',
      }), {
        headers: { 'Content-Type': 'application/json', ...corsHeaders },
      });
    }

    return new Response('Not found', { status: 404 });
  },
};
