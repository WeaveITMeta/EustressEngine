// Package the browser Player for a page: the /player/ folder a site serves.
//
//   node package_player.mjs --workspace <eustress root> --out <empty dir>
//        [--webgpu <eustress_player.wasm>] [--webgl2 <eustress_player.wasm>]
//        [--materials] [--keep <the /player/ folder the site serves now>]
//
// Each --webgpu / --webgl2 is the .wasm cargo built for that feature
// (target/wasm32-unknown-unknown/<profile>/eustress_player.wasm). Writes:
//   <out>/host.js                  the page loader (crates/player-web/js)
//   <out>/test.html                the local test page; never deploy it
//   <out>/webgpu/<hash>/eustress_player.js and _bg.wasm, and the snippets/
//                                  folder holding net.js (wasm-bindgen
//                                  --target web), in a folder named by the
//                                  build's content; host.js names it
//   <out>/webgl2/<hash>/...        the same, for the WebGL2 build
//   with --keep: every build the site serves now, so a page holding an
//                                  older host.js in its cache (hours, on
//                                  eustress.dev) still loads a whole build
//   <out>/assets/parts/*.glb       the part meshes a Space draws
//   <out>/assets/characters/*.glb  the avatar bodies and their clips
//   <out>/assets/materials/...     with --materials: the bundled material
//                                  library (about 136 MB) that bundled://
//                                  textures load from
// It refuses when the installed wasm-bindgen CLI is not the version
// Cargo.lock pins: the CLI reads only the schema its own version writes.
//
// To try it locally: serve <out> from localhost (a secure context, which
// WebTransport needs), e.g. `python -m http.server 8080 --directory <out>`,
// open http://localhost:8080/test.html, and paste the join link a Studio
// host prints.
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import crypto from 'node:crypto';

const args = parse(process.argv.slice(2));
const root = need(args.workspace, '--workspace');
const out = need(args.out, '--out');
if (!args.webgpu && !args.webgl2) fail('give --webgpu and/or --webgl2');
if (fs.existsSync(out) && fs.readdirSync(out).length > 0) fail(`${out} is not empty`);

const crate = path.join(root, 'crates', 'player-web');
const lockVersion = lockedVersion(path.join(root, 'Cargo.lock'), 'wasm-bindgen');
const cli = spawnSync('wasm-bindgen', ['--version'], { encoding: 'utf8' });
if (cli.status !== 0) fail('no wasm-bindgen CLI on PATH');
const cliVersion = cli.stdout.trim().split(/\s+/)[1];
if (cliVersion !== lockVersion) {
  fail(`wasm-bindgen CLI ${cliVersion} does not match Cargo.lock's wasm-bindgen ${lockVersion}`);
}

fs.mkdirSync(out, { recursive: true });
// --keep: the package the site serves now. Its builds are copied in first, so
// a page holding the older host.js in its cache still finds the build it names.
if (args.keep) keepPrevious(args.keep);
const dirs = {};
for (const api of ['webgpu', 'webgl2']) {
  if (!args[api]) continue;
  if (!fs.existsSync(args[api])) fail(`no such file: ${args[api]}`);
  const tmp = path.join(out, `${api}.new`);
  const run = spawnSync(
    'wasm-bindgen',
    // The build keeps its sections for wasm-bindgen to read (Cargo.toml's
    // eustress-player-web release profile); the function names and the
    // producers list are dropped here, as `strip` would have.
    ['--target', 'web', '--no-typescript', '--remove-name-section', '--remove-producers-section',
     '--out-name', 'eustress_player', '--out-dir', tmp, args[api]],
    { stdio: 'inherit' },
  );
  if (run.status !== 0) fail(`wasm-bindgen failed for the ${api} build`);
  // Each build in a folder named by its glue and wasm, so the glue a browser
  // cached for one build is never paired with another build's wasm.
  const hash = crypto.createHash('sha256');
  for (const name of ['eustress_player.js', 'eustress_player_bg.wasm']) hash.update(fs.readFileSync(path.join(tmp, name)));
  dirs[api] = `${api}/${hash.digest('hex').slice(0, 16)}/`;
  const dest = path.join(out, dirs[api]);
  fs.rmSync(dest, { recursive: true, force: true });
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  fs.renameSync(tmp, dest);
  const wasm = path.join(dest, 'eustress_player_bg.wasm');
  console.log(`${api}: ${(fs.statSync(wasm).size / 1048576).toFixed(1)} MiB of wasm in ${dirs[api]}`);
}
const host = fs.readFileSync(path.join(crate, 'js', 'host.js'), 'utf8');
const line = /^const BUILD_DIRS = \{ webgpu: "webgpu\/", webgl2: "webgl2\/" \};$/m;
if (!line.test(host)) fail('host.js has no BUILD_DIRS line to point at the builds');
const named = { webgpu: dirs.webgpu || 'webgpu/', webgl2: dirs.webgl2 || 'webgl2/' };
fs.writeFileSync(
  path.join(out, 'host.js'),
  host.replace(line, `const BUILD_DIRS = { webgpu: ${JSON.stringify(named.webgpu)}, webgl2: ${JSON.stringify(named.webgl2)} };`),
);
fs.copyFileSync(path.join(crate, 'js', 'test.html'), path.join(out, 'test.html'));
fs.mkdirSync(path.join(out, 'assets'), { recursive: true });
// Part meshes are what a Space's Parts draw. They are small. The material
// texture library is not: it is the optional --materials copy below, and a
// missing texture is a flat color rather than a missing part.
const partsOut = path.join(out, 'assets', 'parts');
fs.mkdirSync(partsOut, { recursive: true });
for (const name of fs.readdirSync(path.join(root, 'crates', 'common', 'assets', 'parts'))) {
  if (name.endsWith('.glb')) fs.copyFileSync(path.join(root, 'crates', 'common', 'assets', 'parts', name), path.join(partsOut, name));
}
// Character bodies and clips. The avatar runtime loads them through
// bundled://. Scripts and the source FBX files are not assets.
copyGlbs(path.join(root, 'crates', 'common', 'assets', 'characters'), path.join(out, 'assets', 'characters'));
copyGlbs(
  path.join(root, 'crates', 'common', 'assets', 'characters', 'animations'),
  path.join(out, 'assets', 'characters', 'animations'),
);
if (args.materials) {
  fs.cpSync(path.join(root, 'crates', 'common', 'assets', 'materials'), path.join(out, 'assets', 'materials'), {
    recursive: true,
  });
}
console.log(`packaged into ${out}`);

/// Copy every build the site serves now: webgpu/ and webgl2/ whole. Any of
/// them may be named by a host.js a browser still holds in its cache (every
/// earlier package's, for up to the cache's four hours), and the new build
/// lands beside them in its own folder. A build folder whose host.js was
/// replaced more than a day ago can be deleted from the served folder.
function keepPrevious(previous) {
  if (!fs.existsSync(path.join(previous, 'host.js'))) fail(`--keep ${previous} has no host.js`);
  for (const api of ['webgpu', 'webgl2']) {
    const from = path.join(previous, api);
    if (!fs.existsSync(from)) continue;
    fs.cpSync(from, path.join(out, api), { recursive: true });
    const named = fs.readdirSync(from).filter((n) => /^[0-9a-f]{16}$/.test(n));
    console.log(`kept the served ${api} builds (${named.length} named, plus any from before named folders)`);
  }
}

function lockedVersion(lock, name) {
  const text = fs.readFileSync(lock, 'utf8').replace(/\r\n/g, '\n');
  const block = text.split('\n[[package]]\n').find((b) => b.startsWith(`name = "${name}"\n`));
  const version = block && /^version = "([^"]+)"/m.exec(block)?.[1];
  if (!version) fail(`Cargo.lock has no ${name}`);
  return version;
}

function parse(list) {
  const out = {};
  for (let i = 0; i < list.length; i++) {
    const key = list[i].replace(/^--/, '');
    if (key === 'materials') out.materials = true;
    else out[key] = list[++i];
  }
  return out;
}

function need(value, flag) {
  if (!value) fail(`missing ${flag}`);
  return value;
}

function copyGlbs(from, to) {
  fs.mkdirSync(to, { recursive: true });
  for (const name of fs.readdirSync(from)) {
    if (name.endsWith('.glb')) fs.copyFileSync(path.join(from, name), path.join(to, name));
  }
}

function fail(message) {
  console.error(`package_player: ${message}`);
  process.exit(1);
}
