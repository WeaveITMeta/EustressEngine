import test from 'node:test';
import assert from 'node:assert/strict';
import { corsHeaders, allowedOrigins, siteOrigin, BUILT_IN_ORIGINS } from '../src/cors.mjs';

const PREVIEW = 'https://staging.eustress-cbb.pages.dev';
const asked = (origin) => new Request('https://api.eustress.dev/api/simulations', origin ? { headers: { Origin: origin } } : {});
const allow = (origin, env) => corsHeaders(asked(origin), env)['Access-Control-Allow-Origin'];

test('production admits exactly the four built-in origins, and nothing else is reflected', () => {
  for (const origin of BUILT_IN_ORIGINS) assert.equal(allow(origin, {}), origin);
  assert.equal(allow(PREVIEW, {}), 'https://eustress.dev', 'the staging preview is not admitted in production');
  assert.equal(allow('https://evil.example', {}), 'https://eustress.dev');
  assert.equal(allow('https://f1de41f5.eustress-cbb.pages.dev', {}), 'https://eustress.dev');
  assert.equal(allow(null, {}), 'https://eustress.dev', 'no Origin header (the engine, curl)');
  assert.equal(allowedOrigins({}).size, 4);
});

test('staging admits its exact preview origin and no other pages.dev host', () => {
  const env = { ALLOWED_ORIGINS: PREVIEW, PUBLIC_SITE_ORIGIN: PREVIEW };
  assert.equal(allow(PREVIEW, env), PREVIEW);
  assert.equal(allow('https://f1de41f5.eustress-cbb.pages.dev', env), PREVIEW, 'a per-deployment host is not the branch alias');
  assert.equal(allow('https://evil.pages.dev', env), PREVIEW);
  assert.equal(allow('https://staging.eustress-cbb.pages.dev.evil.example', env), PREVIEW);
  assert.equal(allow('https://eustress.dev', env), 'https://eustress.dev', 'the built-in origins still work');
});

test('an unlisted origin is answered with the deployment\'s own site, so the browser refuses it', () => {
  assert.equal(allow('https://evil.example', { PUBLIC_SITE_ORIGIN: PREVIEW }), PREVIEW);
  assert.equal(siteOrigin({}), 'https://eustress.dev');
  assert.equal(siteOrigin({ PUBLIC_SITE_ORIGIN: `${PREVIEW}/` }), PREVIEW);
  assert.equal(siteOrigin({ PUBLIC_SITE_ORIGIN: 'not a url' }), 'https://eustress.dev');
});

test('ALLOWED_ORIGINS takes only exact origins: a typo fails closed', () => {
  const env = {
    ALLOWED_ORIGINS: [
      '*', 'https://*.pages.dev', 'https://staging.eustress-cbb.pages.dev/path', 'http://evil.example',
      'javascript:alert(1)', 'ftp://x.example', ' ', 'https://user@x.example', 'https://x.example?q=1',
    ].join(','),
  };
  assert.equal(allowedOrigins(env).size, 4, 'every entry above was dropped');
  assert.equal(allow('http://evil.example', env), 'https://eustress.dev');

  const ok = { ALLOWED_ORIGINS: ` ${PREVIEW}/ , http://127.0.0.1:3107 ,https://other.example:8443` };
  const set = allowedOrigins(ok);
  for (const origin of [PREVIEW, 'http://127.0.0.1:3107', 'https://other.example:8443']) assert.ok(set.has(origin), origin);
  assert.equal(set.size, 7);
});

test('the headers never allow credentials and always vary on Origin', () => {
  const h = corsHeaders(asked(PREVIEW), { ALLOWED_ORIGINS: PREVIEW });
  assert.equal(h['Access-Control-Allow-Credentials'], undefined);
  assert.equal(h.Vary, 'Origin');
  assert.match(h['Access-Control-Allow-Methods'], /DELETE/);
  assert.match(h['Access-Control-Allow-Headers'], /Authorization/);
  assert.match(h['Access-Control-Allow-Headers'], /Eustress-Mode/);
});
