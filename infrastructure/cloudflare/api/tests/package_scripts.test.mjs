// The listing review reads every script a package ships from the package itself (src/package_scripts.mjs). These tests
// build real .echk chunks (version 1 raw, version 2 through Node's own zstd) in an in-memory R2, and check that every script
// is found, that progress resumes across calls without reading a chunk twice, and that a chunk that cannot be read is
// reported and never skipped.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import zlib from 'node:zlib';
import {
  extractPackageScripts, readPackageScripts, inlineScriptOf, chunkKeyOf, SCRIPT_EXTENSIONS, MAX_SCRIPT_BYTES, MAX_CHUNK_SCRIPT_BYTES,
} from '../src/package_scripts.mjs';
import { chunkKey, checkWorldManifest } from '../src/world.mjs';

// ─────────────────────────────────────────────────────────────────────────────
// Fixtures
// ─────────────────────────────────────────────────────────────────────────────

const enc = new TextEncoder();
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
const u32 = (n) => {
  const b = Buffer.alloc(4);
  b.writeUInt32LE(n >>> 0);
  return b;
};
const u64 = (n) => {
  const b = Buffer.alloc(8);
  b.writeBigUInt64LE(BigInt(n));
  return b;
};
const bytesOf = (data) => (typeof data === 'string' ? Buffer.from(enc.encode(data)) : Buffer.from(data));

/// The record stream both container versions share.
function recordStream(records) {
  const parts = [];
  for (const [path, data] of records) {
    const p = bytesOf(path);
    const d = bytesOf(data);
    parts.push(u32(p.length), p, u32(d.length), d);
  }
  return Buffer.concat(parts);
}

function v1(records, { count = records.length } = {}) {
  return new Uint8Array(Buffer.concat([Buffer.from('ECHK'), u32(1), u32(count), recordStream(records)]));
}

function v2(records, { rawLen, contentSize = true, count = records.length, codec = 1 } = {}) {
  const stream = recordStream(records);
  const frame = zlib.zstdCompressSync(stream, {
    params: { [zlib.constants.ZSTD_c_compressionLevel]: 9, [zlib.constants.ZSTD_c_contentSizeFlag]: contentSize ? 1 : 0 },
  });
  const header = Buffer.concat([Buffer.from('ECHK'), u32(2), u32(count), Buffer.from([codec, 0, 0, 0]), u64(rawLen ?? stream.length)]);
  return new Uint8Array(Buffer.concat([header, frame]));
}

/// An R2 bucket in memory whose bodies stream in `piece`-byte pieces, whose etag changes with every put, and which counts
/// every get.
function bucket({ piece = 65536 } = {}) {
  const objects = new Map();
  const gets = new Map();
  const puts = [];
  let version = 0;
  const view = (key, o) => ({
    key,
    size: o.body.length,
    etag: o.etag,
    get body() {
      let off = 0;
      return new ReadableStream({
        pull(c) {
          if (off >= o.body.length) return c.close();
          const end = Math.min(o.body.length, off + piece);
          c.enqueue(o.body.slice(off, end));
          off = end;
        },
      });
    },
    text: async () => new TextDecoder().decode(o.body),
    arrayBuffer: async () => o.body.slice().buffer,
  });
  return {
    objects,
    gets,
    puts,
    async get(key) {
      gets.set(key, (gets.get(key) || 0) + 1);
      return objects.has(key) ? view(key, objects.get(key)) : null;
    },
    async head(key) {
      return objects.has(key) ? view(key, objects.get(key)) : null;
    },
    async put(key, value, meta = {}) {
      const body = typeof value === 'string' ? enc.encode(value) : new Uint8Array(value);
      const cur = objects.get(key);
      if (meta.onlyIf?.etagDoesNotMatch === '*' && cur) return null;
      if (meta.onlyIf?.etagMatches !== undefined && (!cur || cur.etag !== meta.onlyIf.etagMatches)) return null;
      const etag = `etag-${++version}`;
      objects.set(key, { body, etag });
      puts.push(key);
      return { key, etag, size: body.length };
    },
    async delete(key) {
      objects.delete(key);
    },
    chunkReads() {
      let n = 0;
      for (const [k, v] of gets) if (k.includes('/chunks/')) n += v;
      return n;
    },
  };
}

/// Store a manifest and its chunks the way a world commit leaves them. `spaces`: [{ name, chunks: [bytes] }].
async function storeManifest(b, simId, spaces, assets = []) {
  const entry = async (bytes, i) => {
    const hash = sha256(bytes);
    if (!b.objects.has(chunkKey(simId, hash))) await b.put(chunkKey(simId, hash), bytes);
    return { cx: i, cz: 0, file: `${i}_0.echk`, size: bytes.length, count: 1, blake3: hash };
  };
  const manifest = {
    format: 'echk', encoder_version: 2, chunk_size: 256, engine_version: 'test', universe: 'U', start_space: '',
    spaces: await Promise.all(spaces.map(async (s) => ({ name: s.name, chunks: await Promise.all(s.chunks.map(entry)) }))),
    assets: await Promise.all(assets.map(entry)),
  };
  assert.equal(checkWorldManifest(manifest).error, undefined, 'the fixture manifest passes the commit check');
  const text = JSON.stringify(manifest);
  const sha = sha256(text);
  const key = `universes/${simId}/manifests/${sha}.json`;
  await b.put(key, text);
  return { key, sha, manifest };
}

async function listing(b, simId, players, source = null) {
  const p = await storeManifest(b, simId, players.spaces, players.assets);
  const world = { manifest_key: p.key, manifest_sha256: p.sha };
  if (source) {
    const s = await storeManifest(b, simId, source.spaces, source.assets);
    Object.assign(world, { source_manifest_key: s.key, source_manifest_sha256: s.sha });
  }
  return { id: simId, format: 'echk', r2_key: p.key, world };
}

async function extractAll(env, sim, opts = {}, limit = 1000) {
  let r;
  for (let calls = 1; calls <= limit; calls++) {
    r = await extractPackageScripts(env, sim, opts);
    if (r.complete || r.errors.length) return { ...r, calls };
  }
  throw new Error('extraction never completed');
}

const SIM = 'aaaaaaaa-bbbb-cccc-dddd-000000000001';

// ─────────────────────────────────────────────────────────────────────────────
// Reading chunks
// ─────────────────────────────────────────────────────────────────────────────

test('chunkKeyOf is world.mjs chunkKey', () => {
  assert.equal(chunkKeyOf(SIM, 'ab'.repeat(32)), chunkKey(SIM, 'ab'.repeat(32)));
  assert.deepEqual(SCRIPT_EXTENSIONS, ['.luau', '.lua', '.rune', '.rn', '.soul']);
});

test('reads every script from version 1 and version 2 chunks, and nothing else', async () => {
  const b = bucket();
  const env = { SCENES: b };
  const big = `-- big\n${'local x = 1\n'.repeat(6000)}`; // ~72 KB, more than one zstd block
  const sim = await listing(b, SIM, {
    spaces: [{
      name: 'Main',
      chunks: [
        v1([
          ['ServerScriptService/Boot.luau', 'print("boot")'],
          ['Workspace/Part/_instance.toml', '[metadata]\nclass_name = "Part"\n'],
          ['Workspace/Mesh.glb', new Uint8Array([0, 1, 2, 3])],
          ['Lib/Old.LUA', 'return 1'],
        ]),
        v2([
          ['StarterPlayer/Client.luau', big],
          ['Souls/Mind.rune', 'pub fn main() {}'],
          ['Souls/Other.rn', 'pub fn other() {}'],
          ['Souls/Station.soul', 'soul text'],
          ['Workspace/notes.txt', 'print("not a script")'],
        ]),
      ],
    }],
  });
  const r = await extractPackageScripts(env, sim);
  assert.equal(r.supported, true);
  assert.equal(r.complete, true, JSON.stringify(r.errors));
  assert.deepEqual(r.errors, []);
  assert.equal(r.chunks_total, 2);
  assert.equal(r.chunks_read, 2);
  assert.equal(r.scripts_total, 6);
  const scripts = await readPackageScripts(env, sim);
  assert.deepEqual(scripts.map((s) => [s.path, s.language, s.audience, s.space]), [
    ['Main/Lib/Old.LUA', 'luau', 'players', 'Main'],
    ['Main/ServerScriptService/Boot.luau', 'luau', 'players', 'Main'],
    ['Main/Souls/Mind.rune', 'rune', 'players', 'Main'],
    ['Main/Souls/Other.rn', 'rune', 'players', 'Main'],
    ['Main/Souls/Station.soul', 'rune', 'players', 'Main'],
    ['Main/StarterPlayer/Client.luau', 'luau', 'players', 'Main'],
  ]);
  const client = scripts.find((s) => s.path.endsWith('Client.luau'));
  assert.equal(client.source, big, 'the whole script, not a head');
  assert.equal(client.bytes, enc.encode(big).length);
  assert.equal(client.sha256, sha256(enc.encode(big)));
  assert.deepEqual(client.paths, ['Main/StarterPlayer/Client.luau']);
  assert.equal(r.script_bytes, scripts.reduce((n, s) => n + s.bytes, 0));
});

test('a version 2 chunk decodes the same whatever pieces its body streams in', async () => {
  const records = [];
  for (let i = 0; i < 40; i++) records.push([`S/Script${i}.luau`, `-- ${i}\n${'print(math.random())\n'.repeat(400 + i)}`]);
  records.push(['S/blob.bin', new Uint8Array(300_000).map((_, i) => (i * 7919) & 255)]);
  let reference = null;
  for (const piece of [1, 7, 4096, 1 << 20]) {
    const b = bucket({ piece });
    const env = { SCENES: b };
    const sim = await listing(b, SIM, { spaces: [{ name: 'S', chunks: [v2(records, { contentSize: piece % 2 === 1 })] }] });
    const r = await extractPackageScripts(env, sim);
    assert.equal(r.complete, true, `piece ${piece}: ${JSON.stringify(r.errors)}`);
    const scripts = (await readPackageScripts(env, sim)).map(({ path, sha256: h, bytes }) => ({ path, h, bytes }));
    assert.equal(scripts.length, 40);
    if (reference) assert.deepEqual(scripts, reference, `piece ${piece}`);
    reference = scripts;
  }
});

// ─────────────────────────────────────────────────────────────────────────────
// Spaces, assets and the source package
// ─────────────────────────────────────────────────────────────────────────────

test('reads every Space, the shared assets and the source package, each chunk once', async () => {
  const b = bucket();
  const env = { SCENES: b };
  const shared = v2([['Workspace/Door.luau', 'door()']]);
  const sim = await listing(b, SIM, {
    spaces: [
      { name: 'Lobby', chunks: [shared, v1([['StarterGui/Menu.luau', 'menu()']])] },
      { name: 'Arena', chunks: [v1([['Workspace/Ring.rune', 'ring()']])] },
    ],
    assets: [v1([['assets/lib/Util.luau', 'util()']])],
  }, {
    spaces: [
      { name: 'Lobby', chunks: [shared, v2([['ServerScriptService/Admin.luau', 'admin()']])] },
      { name: 'Arena', chunks: [v1([['ServerStorage/Secret.luau', 'secret()']])] },
    ],
  });
  const r = await extractPackageScripts(env, sim);
  assert.equal(r.complete, true, JSON.stringify(r.errors));
  assert.equal(r.chunks_total, 6, 'the chunk both packages name is one unit');
  assert.equal(b.chunkReads(), 6, 'each chunk read once');
  assert.equal(r.key, `${sim.world.manifest_sha256}_${sim.world.source_manifest_sha256}`);
  const scripts = await readPackageScripts(env, sim);
  assert.deepEqual(scripts.map((s) => [s.path, s.audience, s.space]), [
    ['Arena/ServerStorage/Secret.luau', 'source', 'Arena'],
    ['Arena/Workspace/Ring.rune', 'players', 'Arena'],
    ['Lobby/ServerScriptService/Admin.luau', 'source', 'Lobby'],
    ['Lobby/StarterGui/Menu.luau', 'players', 'Lobby'],
    ['Lobby/Workspace/Door.luau', 'players', 'Lobby'],
    ['assets/lib/Util.luau', 'players', ''],
  ]);
  const door = scripts.find((s) => s.path === 'Lobby/Workspace/Door.luau');
  assert.deepEqual(door.audiences, ['players', 'source'], 'the shared chunk ships in both packages');
  assert.equal(r.scripts_total, 7, 'Door counted once per package');
});

test('nothing is written outside the package_scripts store, and nothing under chunks/ or manifests/ is touched', async () => {
  const b = bucket();
  const env = { SCENES: b };
  const sim = await listing(b, SIM, { spaces: [{ name: 'A', chunks: [v1([['X.luau', 'x']])] }] });
  const before = b.puts.length;
  await extractAll(env, sim, { maxChunks: 1 });
  await readPackageScripts(env, sim);
  const written = b.puts.slice(before);
  assert.ok(written.length > 0);
  for (const key of written) assert.ok(key.startsWith(`universes/${SIM}/moderation/package_scripts/${sim.world.manifest_sha256}/`), key);
});

// ─────────────────────────────────────────────────────────────────────────────
// Resuming under a budget
// ─────────────────────────────────────────────────────────────────────────────

function manyChunks() {
  const spaces = [];
  for (let s = 0; s < 3; s++) {
    const chunks = [];
    for (let c = 0; c < 5; c++) {
      const recs = [[`Workspace/C${c}/Part.glb`, new Uint8Array(500).fill(c)]];
      for (let k = 0; k < 3; k++) recs.push([`Workspace/C${c}/S${k}.luau`, `-- space ${s} chunk ${c} script ${k}\nprint(${s * 100 + c * 10 + k})`]);
      recs.push(['Workspace/Common.luau', 'print("every chunk ships this")']);
      chunks.push(c % 2 ? v1(recs) : v2(recs));
    }
    spaces.push({ name: `Space${s}`, chunks });
  }
  return { spaces };
}

test('a budget-limited extraction resumes across calls, reads no chunk twice, and matches one unbounded run', async () => {
  const single = bucket();
  const simA = await listing(single, SIM, manyChunks());
  const one = await extractPackageScripts({ SCENES: single }, simA, { maxChunks: 10_000 });
  assert.equal(one.complete, true);
  const expected = await readPackageScripts({ SCENES: single }, simA);

  const b = bucket();
  const env = { SCENES: b };
  const sim = await listing(b, SIM, manyChunks());
  const progress = [];
  let r;
  for (let calls = 0; calls < 20; calls++) {
    r = await extractPackageScripts(env, sim, { maxChunks: 4, checkpointChunks: 3 });
    progress.push(r.chunks_read);
    if (r.complete) break;
  }
  assert.deepEqual(progress, [4, 8, 12, 15], 'four chunks a call');
  assert.equal(r.complete, true);
  assert.equal(r.chunks_total, 15);
  for (const [key, n] of b.gets) if (key.includes('/chunks/')) assert.equal(n, 1, `${key} read ${n} times`);
  assert.equal(b.chunkReads(), 15);
  assert.deepEqual({ ...r }, { ...one }, 'the same totals');
  assert.deepEqual(await readPackageScripts(env, sim), expected, 'the same scripts');

  // A complete package reads no chunk.
  const again = await extractPackageScripts(env, sim);
  assert.equal(again.complete, true);
  assert.equal(b.chunkReads(), 15);
});

test('the byte budget stops a call, yet always reads at least one chunk', async () => {
  const b = bucket();
  const env = { SCENES: b };
  const sim = await listing(b, SIM, manyChunks());
  const r = await extractPackageScripts(env, sim, { maxBytes: 1 });
  assert.equal(r.chunks_read, 1);
  assert.equal(r.complete, false);
  assert.deepEqual(r.errors, []);
  const done = await extractAll(env, sim, { maxBytes: 1 });
  assert.equal(done.complete, true);
  assert.equal(done.calls, 14);
});

test('a time budget stops a call from starting another chunk', async () => {
  const b = bucket();
  const env = { SCENES: b };
  const sim = await listing(b, SIM, manyChunks());
  const realNow = Date.now;
  let clock = 0;
  Date.now = () => (clock += 100);
  try {
    const r = await extractPackageScripts(env, sim, { budgetMs: 250 });
    assert.equal(r.chunks_read, 3, 'started at 100; chunks begun at 200 and 300; the one at 400 is over budget');
    assert.equal(r.complete, false);
  } finally {
    Date.now = realNow;
  }
  assert.equal((await extractPackageScripts(env, sim)).complete, true, 'no budget: the rest in one call');
});

test('readPackageScripts refuses a package that is not completely extracted', async () => {
  const b = bucket();
  const env = { SCENES: b };
  const sim = await listing(b, SIM, manyChunks());
  await assert.rejects(readPackageScripts(env, sim), (e) => e.code === 'not_complete');
  await extractPackageScripts(env, sim, { maxChunks: 2 });
  await assert.rejects(readPackageScripts(env, sim), (e) => e.code === 'not_complete');
});

test('a republished package gets a new key and is read afresh', async () => {
  const b = bucket();
  const env = { SCENES: b };
  const first = await listing(b, SIM, { spaces: [{ name: 'A', chunks: [v1([['X.luau', 'old()']])] }] });
  assert.equal((await extractPackageScripts(env, first)).complete, true);
  const second = await listing(b, SIM, { spaces: [{ name: 'A', chunks: [v1([['X.luau', 'new()']])] }] });
  const r = await extractPackageScripts(env, second);
  assert.notEqual(r.key, first.world.manifest_sha256);
  assert.deepEqual((await readPackageScripts(env, second)).map((s) => s.source), ['new()']);
  assert.deepEqual((await readPackageScripts(env, first)).map((s) => s.source), ['old()']);
});

// ─────────────────────────────────────────────────────────────────────────────
// Deduplication
// ─────────────────────────────────────────────────────────────────────────────

test('identical sources are kept once, with every path that ships them', async () => {
  const b = bucket();
  const env = { SCENES: b };
  const sim = await listing(b, SIM, manyChunks());
  const r = await extractAll(env, sim, { maxChunks: 2, checkpointChunks: 1 });
  assert.equal(r.complete, true);
  assert.equal(r.scripts_total, 15 * 4);
  assert.equal(r.distinct_scripts, 15 * 3 + 1);
  const scripts = await readPackageScripts(env, sim);
  assert.equal(scripts.length, 15 * 3 + 1);
  const common = scripts.filter((s) => s.source === 'print("every chunk ships this")');
  assert.equal(common.length, 1);
  assert.equal(common[0].path, 'Space0/Workspace/Common.luau', 'the first path in order is the kept one');
  assert.deepEqual(common[0].paths, ['Space0/Workspace/Common.luau', 'Space1/Workspace/Common.luau', 'Space2/Workspace/Common.luau']);
  // The source is stored once in R2, not once per path.
  let stored = 0;
  for (const [key, o] of b.objects) {
    if (key.includes('/parts/')) stored += new TextDecoder().decode(o.body).split('every chunk ships this').length - 1;
  }
  assert.equal(stored, 1);
});

// ─────────────────────────────────────────────────────────────────────────────
// Inline TOML
// ─────────────────────────────────────────────────────────────────────────────

test('inlineScriptOf reads basic, multi-line, literal and escaped sources', () => {
  const read = (toml) => inlineScriptOf(toml);
  assert.deepEqual(read('[script]\nsource = "print(\\"hi\\")\\n\\tx = \\u00e9 \\U0001F600 \\\\"\n'),
    { kind: 'code', source: 'print("hi")\n\tx = \u00e9 \u{1F600} \\', language: 'luau' });
  assert.deepEqual(read('[script]\nsource = """\nlocal a = 1\nlocal b = "two"\n"""\n[metadata]\nclass_name = "LuauScript"\n'),
    { kind: 'code', source: 'local a = 1\nlocal b = "two"\n', language: 'luau' });
  assert.deepEqual(read('[script]\nsource = """one \\\n     two"""\n'), { kind: 'code', source: 'one two', language: 'luau' });
  assert.deepEqual(read('[script]\nsource = """ends with quotes"""""\n'), { kind: 'code', source: 'ends with quotes""', language: 'luau' });
  assert.deepEqual(read("[script]\nsource = '''\nraw \\n stays\n'''\n"), { kind: 'code', source: 'raw \\n stays\n', language: 'luau' });
  assert.deepEqual(read("[script]\nsource = 'C:\\path \\n'\n"), { kind: 'code', source: 'C:\\path \\n', language: 'luau' });
  assert.deepEqual(read('[script]\r\nsource = """\r\nwindows\r\n"""\r\n'), { kind: 'code', source: 'windows\r\n', language: 'luau' });
  assert.deepEqual(read('script.source = "dotted()"\n'), { kind: 'code', source: 'dotted()', language: 'luau' });
  assert.deepEqual(read('[ "script" ]  # quoted header\n"source" = "quoted()" # trailing\n'), { kind: 'code', source: 'quoted()', language: 'luau' });
  // A [script] table's other keys, arrays and inline tables with tricky strings are skipped, not misread as source.
  assert.deepEqual(read('[script]\ntags = ["a]", \'b"\', """c\n]"""]\nopts = { k = "}" }\nenabled = true\nsource = "after()"\n'),
    { kind: 'code', source: 'after()', language: 'luau' });
  // Multi-line content that looks like a table header stays content.
  assert.deepEqual(read('[script]\nsource = """\n[metadata]\nclass_name = "x"\n"""\n'),
    { kind: 'code', source: '[metadata]\nclass_name = "x"\n', language: 'luau' });
  // A SoulScript's code is Rune, by class or by run context.
  assert.equal(read('[script]\nsource = "fn main() {}"\n[metadata]\nclass_name = "SoulScript"\n').language, 'rune');
  assert.equal(read('[script]\nsource = "fn main() {}"\nrun_context = "Rune"\n').language, 'rune');
});

test('inlineScriptOf: references, empty sources, no script table, and unreadable TOML', () => {
  assert.deepEqual(inlineScriptOf('[script]\nsource = "StationMind.rune"\n'), { kind: 'reference', value: 'StationMind.rune' });
  assert.deepEqual(inlineScriptOf('[script]\nsource = "Lib/Helper.LUAU"\n'), { kind: 'reference', value: 'Lib/Helper.LUAU' });
  assert.equal(inlineScriptOf('[script]\nsource = """\nrequire("x.luau")\n"""\n').kind, 'code', 'a multi-line source is code');
  // One line of code that ends in a script extension is code, not a reference.
  for (const code of ['print(1) -- see a.luau', 'return a.luau', 'x=a.rune', 'Station Mind.rune', ' a.luau\t;'])
    assert.equal(inlineScriptOf(`[script]\nsource = ${JSON.stringify(code)}\n`).kind, 'code', code);
  assert.deepEqual(inlineScriptOf('[script]\nsource = ""\nenabled = true\n'), { kind: 'none' });
  assert.deepEqual(inlineScriptOf('[script]\nenabled = true\n'), { kind: 'none' });
  assert.deepEqual(inlineScriptOf('[metadata]\nclass_name = "Part"\n'), { kind: 'none' });
  assert.deepEqual(inlineScriptOf('[metadata]\nsource = "not a script table"\ndescription = "script"\n'), { kind: 'none' });
  for (const bad of [
    '[script]\nsource = "unterminated\n',
    '[script]\nsource = """never closed\n',
    '[script]\nsource = "bad escape \\q"\n',
    '[script]\nsource = 42\n',
    'script = { source = "inline table()" }\n',
    '[[script]]\nsource = "array of tables()"\n',
    '[script]\nsource = "a"\nsource = "b"\n',
  ]) assert.equal(inlineScriptOf(bad).kind, 'unparsed', bad);
});

test('inline TOML scripts are read from _instance.toml records; a SoulScript file reference is not', async () => {
  const b = bucket();
  const env = { SCENES: b };
  const unreadable = '[script]\nsource = "never closed\n[metadata]\nclass_name = "LuauScript"\n';
  const sim = await listing(b, SIM, {
    spaces: [{
      name: 'Station',
      chunks: [v2([
        ['Workspace/Door/_instance.toml', '[script]\nsource = """\nlocal open = false\nprint("door")\n"""\nenabled = true\n[metadata]\nclass_name = "LuauScript"\n'],
        ['SoulService/StationMind/_instance.toml', '[script]\nsource = "StationMind.rune"\nrun_context = "Rune"\n[metadata]\nclass_name = "SoulScript"\n'],
        ['SoulService/StationMind/StationMind.rune', 'pub fn think() {}'],
        ['Workspace/Empty/_instance.toml', '[script]\nsource = ""\n[metadata]\nclass_name = "LuauScript"\n'],
        ['Workspace/Inline/_instance.toml', '[script]\nsource = \'fn tick() {}\'\n[metadata]\nclass_name = "SoulScript"\n'],
        ['Workspace/Broken/_instance.toml', unreadable],
        ['Workspace/Part/_instance.toml', '[metadata]\nclass_name = "Part"\n'],
      ])],
    }],
  });
  assert.equal((await extractPackageScripts(env, sim)).complete, true);
  const scripts = await readPackageScripts(env, sim);
  assert.deepEqual(scripts.map((s) => [s.path, s.language]), [
    ['Station/SoulService/StationMind/StationMind.rune', 'rune'],
    ['Station/Workspace/Broken/_instance.toml#script', 'luau'],
    ['Station/Workspace/Door/_instance.toml#script', 'luau'],
    ['Station/Workspace/Inline/_instance.toml#script', 'rune'],
  ]);
  assert.equal(scripts[2].source, 'local open = false\nprint("door")\n');
  assert.equal(scripts[1].source, unreadable, 'TOML that cannot be read is read whole');
  assert.equal(scripts[2].sha256, sha256(enc.encode(scripts[2].source)));
});

// ─────────────────────────────────────────────────────────────────────────────
// Hostile and missing chunks
// ─────────────────────────────────────────────────────────────────────────────

function forged() {
  const good = v2([['A.luau', 'a()']]);
  const withRaw = (raw) => {
    const c = good.slice();
    Buffer.from(c.buffer, c.byteOffset, c.byteLength).writeBigUInt64LE(BigInt(raw), 16);
    return c;
  };
  const noContentSize = v2([['A.luau', 'a()']], { contentSize: false });
  const rawNoCs = (delta) => {
    const c = noContentSize.slice();
    const view = Buffer.from(c.buffer, c.byteOffset, c.byteLength);
    view.writeBigUInt64LE(view.readBigUInt64LE(16) + BigInt(delta), 16);
    return c;
  };
  const bigWindow = (() => {
    // A frame header claiming a 1 GiB window: single segment off, window descriptor exponent 20.
    const c = noContentSize.slice();
    c[24 + 5] = 20 << 3;
    return c;
  })();
  const pathPastEnd = new Uint8Array(Buffer.concat([Buffer.from('ECHK'), u32(1), u32(1), u32(500), Buffer.from('short')]));
  const longPath = new Uint8Array(Buffer.concat([Buffer.from('ECHK'), u32(1), u32(1), u32(2000), Buffer.alloc(2008)]));
  const dataPastEnd = new Uint8Array(Buffer.concat([Buffer.from('ECHK'), u32(1), u32(1), u32(6), Buffer.from('a.luau'), u32(10_000), Buffer.from('x')]));
  const whole = v1([['A.luau', 'print("whole")'], ['B.luau', 'print("b")']]);
  return {
    'bad magic': [new Uint8Array(Buffer.concat([Buffer.from('ECHQ'), whole.slice(4)])), /bad magic/],
    'too short for a header': [new Uint8Array([0x45, 0x43, 0x48]), /bad magic/],
    'unsupported version': [new Uint8Array(Buffer.concat([Buffer.from('ECHK'), u32(3), u32(0)])), /unsupported .echk version 3/],
    'truncated v1': [whole.slice(0, whole.length - 5), /truncated .echk \(data\)/],
    'truncated v2 header': [good.slice(0, 20), /truncated .echk \(version 2 header\)/],
    'truncated zstd frame': [good.slice(0, good.length - 3), /truncated|bad compressed/],
    'record count beyond the bytes': [v1([['A.luau', 'a']], { count: 1000 }), /record count/],
    'raw_len over the cap': [withRaw(256 * 1024 * 1024 + 1), /over the 268435456 limit/],
    'raw_len past u32': [withRaw(2 ** 40), /over the 268435456 limit/],
    'raw_len disagrees with the frame content size': [withRaw(33), /declares/],
    'inflates short of raw_len': [rawNoCs(+1), /inflates to \d+ bytes, declares/],
    'inflates past raw_len': [rawNoCs(-1), /inflates past/],
    'a window over the cap': [bigWindow, /window of \d+ bytes, over/],
    'unknown codec': [v2([['A.luau', 'a()']], { codec: 2 }), /unknown codec 2/],
    'path_len past the end': [pathPastEnd, /truncated .echk \(path\)/],
    'path_len over 1024': [longPath, /truncated .echk \(path\)/],
    'data_len past the end': [dataPastEnd, /truncated .echk \(data\)/],
    'garbage after the header': [new Uint8Array(Buffer.concat([good.slice(0, 24), Buffer.from('not zstd at all, not at all')])), /zstd frame/],
  };
}

test('every hostile chunk is an error, never a throw and never a skip; the good chunks are still read', async () => {
  for (const [name, [bytes, reason]] of Object.entries(forged())) {
    const b = bucket({ piece: 5 });
    const env = { SCENES: b };
    const sim = await listing(b, SIM, { spaces: [{ name: 'S', chunks: [v1([['Good.luau', 'good()']]), bytes] }] });
    const bad = sha256(bytes);
    const r = await extractPackageScripts(env, sim);
    assert.equal(r.complete, false, name);
    assert.equal(r.chunks_total, 2, name);
    assert.equal(r.chunks_read, 1, name);
    assert.equal(r.errors.length, 1, name);
    assert.equal(r.errors[0].chunk, bad, name);
    assert.match(r.errors[0].reason, reason, `${name}: ${r.errors[0].reason}`);
    await assert.rejects(readPackageScripts(env, sim), (e) => e.code === 'not_complete', name);
  }
});

test('a chunk missing from storage is an error, retried on the next call, and completes once it is there', async () => {
  const b = bucket();
  const env = { SCENES: b };
  const late = v2([['Late.luau', 'late()']]);
  const sim = await listing(b, SIM, { spaces: [{ name: 'S', chunks: [v1([['Early.luau', 'early()']]), late, v1([['After.luau', 'after()']])] }] });
  const lateKey = chunkKey(SIM, sha256(late));
  const stored = b.objects.get(lateKey);
  b.objects.delete(lateKey);

  const first = await extractPackageScripts(env, sim);
  assert.equal(first.complete, false);
  assert.equal(first.chunks_read, 2);
  assert.deepEqual(first.errors, [{ chunk: sha256(late), reason: 'the chunk is missing from storage' }]);

  const second = await extractPackageScripts(env, sim);
  assert.equal(second.complete, false, 'still missing: still not complete');
  assert.equal(b.gets.get(lateKey), 2, 'the failed chunk is read again');
  assert.equal(b.gets.get(chunkKey(SIM, sha256(v1([['Early.luau', 'early()']])))), 1, 'a chunk already read is not');

  b.objects.set(lateKey, stored);
  const third = await extractPackageScripts(env, sim);
  assert.equal(third.complete, true);
  assert.deepEqual(third.errors, []);
  assert.deepEqual((await readPackageScripts(env, sim)).map((s) => s.path), ['S/After.luau', 'S/Early.luau', 'S/Late.luau']);
});

test('a stored chunk whose size disagrees with the manifest is an error', async () => {
  const b = bucket();
  const env = { SCENES: b };
  const chunk = v1([['A.luau', 'a()']]);
  const sim = await listing(b, SIM, { spaces: [{ name: 'S', chunks: [chunk] }] });
  b.objects.get(chunkKey(SIM, sha256(chunk))).body = v1([['A.luau', 'a() -- and more']]);
  const r = await extractPackageScripts(env, sim);
  assert.equal(r.complete, false);
  assert.match(r.errors[0].reason, /the manifest says/);
});

test('a script too large to read in full is an error, never a truncated read', async () => {
  const b = bucket();
  const env = { SCENES: b };
  const huge = new Uint8Array(MAX_SCRIPT_BYTES + 1).fill(0x2d);
  const sim = await listing(b, SIM, { spaces: [{ name: 'S', chunks: [v1([['Huge.luau', huge]])] }] });
  const r = await extractPackageScripts(env, sim);
  assert.equal(r.complete, false);
  assert.match(r.errors[0].reason, /Huge\.luau is \d+ bytes, over/);
});

test('a chunk holding more script than MAX_CHUNK_SCRIPT_BYTES (16 MiB) is an error, never read past it', async () => {
  assert.equal(MAX_CHUNK_SCRIPT_BYTES, 16 * 1024 * 1024);
  const b = bucket();
  const env = { SCENES: b };
  // Three scripts, each under the per-script cap, together over the per-chunk one.
  const piece = n => new Uint8Array(6 * 1024 * 1024).fill(0x61 + n);
  const sim = await listing(b, SIM, { spaces: [{ name: 'S', chunks: [v1([['A.luau', piece(0)], ['B.luau', piece(1)], ['C.luau', piece(2)]])] }] });
  const r = await extractPackageScripts(env, sim);
  assert.equal(r.complete, false);
  assert.match(r.errors[0].reason, /holds over 16777216 bytes of scripts/);
});

test('a manifest missing from storage, or whose bytes are not its hash, is an error', async () => {
  const b = bucket();
  const env = { SCENES: b };
  const sim = await listing(b, SIM, { spaces: [{ name: 'S', chunks: [v1([['A.luau', 'a()']])] }] });
  const text = b.objects.get(sim.world.manifest_key).body;
  b.objects.get(sim.world.manifest_key).body = enc.encode(new TextDecoder().decode(text).replace('"universe":"U"', '"universe":"V"'));
  const r = await extractPackageScripts(env, sim);
  assert.equal(r.complete, false);
  assert.equal(r.errors[0].chunk, 'manifest:players');
  assert.match(r.errors[0].reason, /hash to/);
  b.objects.delete(sim.world.manifest_key);
  const gone = await extractPackageScripts(env, sim);
  assert.deepEqual(gone.errors, [{ chunk: 'manifest:players', reason: 'the manifest is missing from storage' }]);
});

test('a listing that is not .echk is unsupported', async () => {
  const env = { SCENES: bucket() };
  const pak = { id: SIM, format: 'pak', r2_key: 'scenes/x.pak' };
  const r = await extractPackageScripts(env, pak);
  assert.equal(r.supported, false);
  assert.equal(r.complete, false);
  assert.equal(env.SCENES.gets.size, 0, 'nothing is read');
  assert.equal((await extractPackageScripts(env, { id: SIM })).supported, false, 'no format at all');
  await assert.rejects(readPackageScripts(env, pak), (e) => e.code === 'unsupported');
});

test('the key comes from the manifests\' bytes when the record does not carry their hashes', async () => {
  const b = bucket();
  const env = { SCENES: b };
  const sim = await listing(b, SIM, { spaces: [{ name: 'S', chunks: [v1([['A.luau', 'a()']])] }] });
  const sha = sim.world.manifest_sha256;
  const bare = { ...sim, world: { manifest_key: `universes/${SIM}/legacy/manifest.json` } };
  b.objects.set(bare.world.manifest_key, b.objects.get(sim.world.manifest_key));
  const r = await extractPackageScripts(env, bare);
  assert.equal(r.complete, true);
  assert.equal(r.key, sha);
});
