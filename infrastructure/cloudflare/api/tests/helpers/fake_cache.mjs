// The platform's cache (`caches.default`) in memory, for the routes that keep a computed answer for a few seconds
// (src/body_cache.mjs). It honours `Cache-Control: max-age` against Date.now, so a test that moves the clock moves the
// cache with it, and it counts what it was asked.

export function fakeCache() {
  const store = new Map();
  const cache = {
    store,
    matches: 0,
    hits: 0,
    puts: 0,
    async match(request) {
      cache.matches += 1;
      const entry = store.get(request.url);
      if (!entry || Date.now() >= entry.expires) return undefined;
      cache.hits += 1;
      return new Response(entry.text, { headers: { 'Content-Type': 'application/json' } });
    },
    async put(request, response) {
      const maxAge = Number(/max-age=(\d+)/.exec(response.headers.get('Cache-Control') || '')?.[1]);
      if (!maxAge) return;
      cache.puts += 1;
      store.set(request.url, { text: await response.text(), expires: Date.now() + maxAge * 1000 });
    },
  };
  return cache;
}

/// Makes `cache` the platform's default cache for the Worker under test until the returned function is called.
export function installCache(cache = fakeCache()) {
  const had = Object.getOwnPropertyDescriptor(globalThis, 'caches');
  Object.defineProperty(globalThis, 'caches', { value: { default: cache }, configurable: true, writable: true });
  return { cache, restore: () => { if (had) Object.defineProperty(globalThis, 'caches', had); else delete globalThis.caches; } };
}
