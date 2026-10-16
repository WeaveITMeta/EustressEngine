// =============================================================================
// A computed answer, kept for a few seconds
// =============================================================================
//
// `Cache-Control: public` on a Worker's response caches nothing: the Worker runs on every request. A read that scans
// storage (the live strip, the makers list) would cost one scan per request to anyone who hammers its URL. This keeps the
// computed VALUE, as JSON, in the platform's cache for `ttlSeconds`, so a hammered URL costs one scan per window per data
// centre. The value is cached, not the response: the response is built again for each request, so its CORS headers always
// name the asking origin.
//
// The key is made of the parts that change the answer and nothing else (a rating slice from a fixed list), so a client cannot
// fill the cache or dodge it by varying a query string. It carries the request's own origin, so a deployment on another host
// (staging beside production) never reads another's answer. A value that could not be computed is not cached. Where there is
// no cache (a test, a local run), every request computes.

const KEY_ORIGIN_FALLBACK = 'https://api.eustress.dev';

/// The platform's default cache, or null where there is none.
export function defaultCache() {
  try {
    return typeof caches !== 'undefined' && caches?.default ? caches.default : null;
  } catch {
    return null;
  }
}

/// `compute()`'s value, from the cache when it holds one for `key` that is under `ttlSeconds` old. `key` is an https URL
/// made by the caller. Never throws because of the cache: a cache that fails costs a computation, not an answer.
export async function cachedValue(cache, key, ttlSeconds, compute) {
  const request = new Request(key || KEY_ORIGIN_FALLBACK);
  if (cache) {
    try {
      const hit = await cache.match(request);
      if (hit) return JSON.parse(await hit.text());
    } catch (e) {
      console.error('body cache read failed:', e?.message || e);
    }
  }
  const value = await compute();
  if (cache && value !== undefined) {
    try {
      await cache.put(request, new Response(JSON.stringify(value), {
        headers: { 'Content-Type': 'application/json', 'Cache-Control': `public, max-age=${ttlSeconds}` },
      }));
    } catch (e) {
      console.error('body cache write failed:', e?.message || e);
    }
  }
  return value;
}
