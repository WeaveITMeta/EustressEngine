// Package the browser Player for a page: the /player/ folder a site serves.
//
//   node package_player.mjs --workspace <eustress root> --out <empty dir>
//        [--webgpu <eustress_player.wasm>] [--webgl2 <eustress_player.wasm>]
//        [--materials]
//
// Each --webgpu / --webgl2 is the .wasm cargo built for that feature
// (target/wasm32-unknown-unknown/<profile>/eustress_player.wasm). Writes:
//   <out>/host.js                  the page loader (crates/player-web/js)
//   <out>/test.html                the local test page; never deploy it
//   <out>/webgpu/eustress_player.js and _bg.wasm, and the snippets/ folder
//                                  holding net.js (wasm-bindgen --target web)
//   <out>/webgl2/...               the same, for the WebGL2 build
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
for (const api of ['webgpu', 'webgl2']) {
  if (!args[api]) continue;
  if (!fs.existsSync(args[api])) fail(`no such file: ${args[api]}`);
  const run = spawnSync(
    'wasm-bindgen',
    ['--target', 'web', '--no-typescript', '--out-name', 'eustress_player', '--out-dir', path.join(out, api), args[api]],
    { stdio: 'inherit' },
  );
  if (run.status !== 0) fail(`wasm-bindgen failed for the ${api} build`);
  const wasm = path.join(out, api, 'eustress_player_bg.wasm');
  console.log(`${api}: ${(fs.statSync(wasm).size / 1048576).toFixed(1)} MiB of wasm`);
}
for (const file of ['host.js', 'test.html']) fs.copyFileSync(path.join(crate, 'js', file), path.join(out, file));
fs.mkdirSync(path.join(out, 'assets'), { recursive: true });
// Part meshes are what a Space's Parts draw. They are small. The material
// texture library is not: it is the optional --materials copy below, and a
// missing texture is a flat color rather than a missing part.
const partsOut = path.join(out, 'assets', 'parts');
fs.mkdirSync(partsOut, { recursive: true });
for (const name of fs.readdirSync(path.join(root, 'crates', 'common', 'assets', 'parts'))) {
  if (name.endsWith('.glb')) fs.copyFileSync(path.join(root, 'crates', 'common', 'assets', 'parts', name), path.join(partsOut, name));
}
if (args.materials) {
  fs.cpSync(path.join(root, 'crates', 'common', 'assets', 'materials'), path.join(out, 'assets', 'materials'), {
    recursive: true,
  });
}
console.log(`packaged into ${out}`);

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

function fail(message) {
  console.error(`package_player: ${message}`);
  process.exit(1);
}
