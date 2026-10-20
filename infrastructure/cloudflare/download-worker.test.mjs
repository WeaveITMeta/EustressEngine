// Routes of infrastructure/cloudflare/download-worker.js that a browser or
// curl hits directly: short platform names, HEAD, and a single byte range.
// Run: node --test infrastructure/cloudflare/download-worker.test.mjs

import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { pathToFileURL } from 'node:url';

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'eustress-downloads-'));
const file = path.join(tmp, 'download-worker.mjs');
fs.copyFileSync(path.resolve(import.meta.dirname, 'download-worker.js'), file);
const worker = (await import(pathToFileURL(file).href)).default;

function bucket(entries) {
  const objects = new Map();
  for (const [key, value] of Object.entries(entries)) {
    objects.set(key, new TextEncoder().encode(value));
  }
  const view = (key, bytes, slice) => ({
    key,
    // size is the whole object, including when the body is a range. That is
    // what R2 returns.
    size: bytes.byteLength,
    httpEtag: `"${key}"`,
    body: new Response(slice ?? bytes).body,
    json: async () => JSON.parse(new TextDecoder().decode(bytes)),
  });
  return {
    async head(key) {
      const bytes = objects.get(key);
      return bytes ? view(key, bytes) : null;
    },
    async get(key, options) {
      const bytes = objects.get(key);
      if (!bytes) return null;
      const range = options?.range;
      if (range) {
        const slice = bytes.subarray(range.offset, range.offset + range.length);
        return view(key, bytes, slice);
      }
      return view(key, bytes);
    },
  };
}

const player = {
  version: '1.2.3',
  channel: 'stable',
  platforms: {
    'windows-x64': { file: 'player-windows.zip', url: 'https://downloads.eustress.dev/player/v1.2.3/player-windows.zip' },
    'windows-x64-installer': { file: 'EustressPlayer-Setup.exe', url: 'https://downloads.eustress.dev/player/v1.2.3/EustressPlayer-Setup.exe' },
    'macos-arm64': { file: 'player.dmg', url: 'https://downloads.eustress.dev/player/v1.2.3/player.dmg' },
    'linux-x64': { file: 'player.tar.gz', url: 'https://downloads.eustress.dev/player/v1.2.3/player.tar.gz' },
  },
};
const staging = { ...player, version: '9.9.9', channel: 'staging', platforms: {
  'windows-x64': { file: 'stage-windows.zip', url: 'https://downloads.eustress.dev/staging/player/v9.9.9/stage-windows.zip' },
  'windows-x64-installer': { file: 'StageSetup.exe', url: 'https://downloads.eustress.dev/staging/player/v9.9.9/StageSetup.exe' },
  'macos-arm64': { file: 'stage.dmg', url: 'https://downloads.eustress.dev/staging/player/v9.9.9/stage.dmg' },
  'linux-x64': { file: 'stage.tar.gz', url: 'https://downloads.eustress.dev/staging/player/v9.9.9/stage.tar.gz' },
}};
const studio = {
  version: '0.3.8',
  platforms: {
    'windows-x64': { file: 'studio-windows.zip', url: 'https://downloads.eustress.dev/v0.3.8/studio-windows.zip' },
    'macos-arm64': { file: 'studio.dmg', url: 'https://downloads.eustress.dev/v0.3.8/studio.dmg' },
    'linux-x64': { file: 'studio.tar.gz', url: 'https://downloads.eustress.dev/v0.3.8/studio.tar.gz' },
  },
};

const bytes = 'abcdefghijklmnopqrstuvwxyz'; // 26 bytes
const JWT_SECRET = 'test-secret';

function signToken(payload, secret = JWT_SECRET) {
  const b64 = (value) => Buffer.from(value).toString('base64url');
  const h = b64(JSON.stringify({ alg: 'HS256', typ: 'JWT' }));
  const p = b64(JSON.stringify(payload));
  const sig = crypto.createHmac('sha256', secret).update(`${h}.${p}`).digest();
  return `${h}.${p}.${b64(sig)}`;
}

const env = {
  JWT_SECRET,
  DOWNLOADS: bucket({
    'latest.json': JSON.stringify(studio),
    'player/latest.json': JSON.stringify(player),
    'staging/player/latest.json': JSON.stringify(staging),
    'player/v1.2.3/player-windows.zip': bytes,
    'staging/player/v9.9.9/stage-windows.zip': bytes,
    'v0.3.8/studio-windows.zip': bytes,
    'secret/private.txt': 'not a release',
  }),
};
const ctx = { waitUntil() {} };

async function hit(pathname, { method = 'GET', headers = {} } = {}) {
  const res = await worker.fetch(new Request(`https://downloads.eustress.dev${pathname}`, { method, headers }), env, ctx);
  const text = await res.text();
  return { status: res.status, headers: res.headers, text };
}

test('short platform names redirect for Studio, Player, and staging', async () => {
  const cases = [
    ['/latest/windows', '/v0.3.8/studio-windows.zip'],
    ['/latest/macos', '/v0.3.8/studio.dmg'],
    ['/latest/mac', '/v0.3.8/studio.dmg'],
    ['/latest/linux', '/v0.3.8/studio.tar.gz'],
    ['/latest/windows-x64', '/v0.3.8/studio-windows.zip'],
    ['/player/latest/windows', '/player/v1.2.3/player-windows.zip'],
    ['/player/latest/windows-x64', '/player/v1.2.3/player-windows.zip'],
    ['/player/latest/windows-x64-installer', '/player/v1.2.3/EustressPlayer-Setup.exe'],
    ['/player/latest/macos', '/player/v1.2.3/player.dmg'],
    ['/player/latest/mac', '/player/v1.2.3/player.dmg'],
    ['/player/latest/linux', '/player/v1.2.3/player.tar.gz'],
    ['/staging/player/latest/windows', '/staging/player/v9.9.9/stage-windows.zip'],
    ['/staging/player/latest/macos', '/staging/player/v9.9.9/stage.dmg'],
    ['/staging/player/latest/mac', '/staging/player/v9.9.9/stage.dmg'],
    ['/staging/player/latest/linux', '/staging/player/v9.9.9/stage.tar.gz'],
    ['/staging/player/latest/windows-x64', '/staging/player/v9.9.9/stage-windows.zip'],
  ];
  for (const [pathname, location] of cases) {
    const got = await hit(pathname);
    assert.equal(got.status, 302, pathname);
    assert.equal(got.headers.get('location'), location, pathname);
    assert.equal(got.text, '', pathname);
  }
});

test('an unknown platform is still not found, and windows is not the installer', async () => {
  const missing = await hit('/staging/player/latest/windows-arm');
  assert.equal(missing.status, 404);
  assert.match(missing.text, /Platform 'windows-arm' not found/);
  const zip = await hit('/staging/player/latest/windows');
  assert.equal(zip.headers.get('location'), '/staging/player/v9.9.9/stage-windows.zip');
  assert.notEqual(zip.headers.get('location'), '/staging/player/v9.9.9/StageSetup.exe');
});

test('HEAD on a redirect is the same 302, and HEAD on a file has headers and no body', async () => {
  const redirect = await hit('/staging/player/latest/windows', { method: 'HEAD' });
  assert.equal(redirect.status, 302);
  assert.equal(redirect.headers.get('location'), '/staging/player/v9.9.9/stage-windows.zip');
  assert.equal(redirect.text, '');

  const file = await hit('/player/v1.2.3/player-windows.zip', { method: 'HEAD' });
  assert.equal(file.status, 200);
  assert.equal(file.text, '');
  assert.equal(file.headers.get('content-length'), String(bytes.length));
  assert.equal(file.headers.get('content-type'), 'application/octet-stream');
  assert.equal(file.headers.get('etag'), '"player/v1.2.3/player-windows.zip"');
  assert.equal(file.headers.get('accept-ranges'), 'bytes');

  const manifest = await hit('/staging/player/latest.json', { method: 'HEAD' });
  assert.equal(manifest.status, 200);
  assert.equal(manifest.text, '');
  assert.equal(manifest.headers.get('content-type'), 'application/json');
  assert.ok(Number(manifest.headers.get('content-length')) > 0);
  assert.ok(manifest.headers.get('etag'));
});

test('a single byte range is 206, an unsatisfiable range is 416, and a full GET is unchanged', async () => {
  const full = await hit('/staging/player/v9.9.9/stage-windows.zip');
  assert.equal(full.status, 200);
  assert.equal(full.text, bytes);
  assert.equal(full.headers.get('accept-ranges'), 'bytes');
  assert.equal(full.headers.get('content-length'), String(bytes.length));

  const closed = await hit('/staging/player/v9.9.9/stage-windows.zip', { headers: { Range: 'bytes=0-3' } });
  assert.equal(closed.status, 206);
  assert.equal(closed.text, 'abcd');
  assert.equal(closed.headers.get('content-range'), `bytes 0-3/${bytes.length}`);
  assert.equal(closed.headers.get('content-length'), '4');
  assert.equal(closed.headers.get('accept-ranges'), 'bytes');
  assert.equal(closed.headers.get('etag'), '"staging/player/v9.9.9/stage-windows.zip"');

  const open = await hit('/player/v1.2.3/player-windows.zip', { headers: { Range: 'bytes=24-' } });
  assert.equal(open.status, 206);
  assert.equal(open.text, 'yz');
  assert.equal(open.headers.get('content-range'), `bytes 24-25/${bytes.length}`);

  const suffix = await hit('/v0.3.8/studio-windows.zip', { headers: { Range: 'bytes=-3' } });
  assert.equal(suffix.status, 206);
  assert.equal(suffix.text, 'xyz');
  assert.equal(suffix.headers.get('content-range'), `bytes 23-25/${bytes.length}`);

  const clamped = await hit('/player/v1.2.3/player-windows.zip', { headers: { Range: 'bytes=20-999' } });
  assert.equal(clamped.status, 206);
  assert.equal(clamped.text, 'uvwxyz');
  assert.equal(clamped.headers.get('content-range'), `bytes 20-25/${bytes.length}`);

  const past = await hit('/staging/player/v9.9.9/stage-windows.zip', { headers: { Range: 'bytes=26-30' } });
  assert.equal(past.status, 416);
  assert.equal(past.text, '');
  assert.equal(past.headers.get('content-range'), `bytes */${bytes.length}`);
  assert.equal(past.headers.get('accept-ranges'), 'bytes');

  const backwards = await hit('/player/v1.2.3/player-windows.zip', { headers: { Range: 'bytes=5-4' } });
  assert.equal(backwards.status, 416);

  // More than one range is not a single slice. The whole file comes back.
  const multi = await hit('/player/v1.2.3/player-windows.zip', { headers: { Range: 'bytes=0-1,2-3' } });
  assert.equal(multi.status, 200);
  assert.equal(multi.text, bytes);
});

test('a signed download still returns the file, including HEAD and a single range', async () => {
  const headers = { Authorization: `Bearer ${signToken({ sub: 'user', exp: Math.floor(Date.now() / 1000) + 3600 })}` };
  const full = await hit('/api/releases/download?platform=windows-x64', { headers });
  assert.equal(full.status, 200);
  assert.equal(full.text, bytes);
  assert.equal(full.headers.get('accept-ranges'), 'bytes');
  assert.equal(full.headers.get('content-length'), String(bytes.length));
  assert.equal(full.headers.get('content-type'), 'application/octet-stream');

  const head = await hit('/api/releases/download?platform=windows-x64', { method: 'HEAD', headers });
  assert.equal(head.status, 200);
  assert.equal(head.text, '');
  assert.equal(head.headers.get('content-length'), String(bytes.length));
  assert.equal(head.headers.get('etag'), '"v0.3.8/studio-windows.zip"');
  assert.equal(head.headers.get('accept-ranges'), 'bytes');

  const range = await hit('/api/releases/download?platform=windows-x64', { headers: { ...headers, Range: 'bytes=0-2' } });
  assert.equal(range.status, 206);
  assert.equal(range.text, 'abc');
  assert.equal(range.headers.get('content-range'), `bytes 0-2/${bytes.length}`);
});
