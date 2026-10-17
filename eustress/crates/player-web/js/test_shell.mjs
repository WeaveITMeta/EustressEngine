// Tests for host.js and net.js, run in Node with fakes for the browser APIs.
// They prove the shell's flow and its rule that a join link, a host's own
// text or an error never reaches a status. They are not a browser run.
//   node test_shell.mjs            (HOST_JS=<path> tests another host.js)
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { pathToFileURL, fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const HOST_JS = process.env.HOST_JS || path.join(here, 'host.js');
const SECRET = 'k3y-SECRET-0123';
const LINK = `eustress-player://join/127.0.0.1:7777?key=${SECRET}&pin=${'ab'.repeat(32)}`;
const API = 'https://api-staging.eustress.dev';
const SIM = '07d084d8-9098-4192-bb48-84d2c428db36';
let passed = 0;
async function test(name, fn) {
  await fn();
  passed++;
  console.log(`ok - ${name}`);
}

// ── Browser globals for host.js ─────────────────────────────────────────────
const win = new EventTarget();
globalThis.addEventListener = win.addEventListener.bind(win);
globalThis.removeEventListener = win.removeEventListener.bind(win);
globalThis.HTMLElement = class extends EventTarget {};
globalThis.fetch = async (url) => ({ ok: url instanceof URL && fs.existsSync(fileURLToPath(url)) });

function container() {
  const c = new HTMLElement();
  c.events = [];
  c.addEventListener('eustress-player-status', (e) => c.events.push(e.detail));
  return c;
}
const leaked = (value) => {
  const text = JSON.stringify(value);
  return text.includes(SECRET) || text.includes('eustress-player://') || text.includes('HOST SAYS');
};

// A fresh /player/ folder: host.js, and a fake build when given, in the
// folder this test's navigator (no WebGPU) picks unless another is named.
async function host(glue, folder = 'webgl2') {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'eustress-player-'));
  fs.copyFileSync(HOST_JS, path.join(dir, 'host.js'));
  if (glue !== undefined) {
    fs.mkdirSync(path.join(dir, folder));
    fs.writeFileSync(path.join(dir, folder, 'eustress_player.js'), glue);
  }
  return import(pathToFileURL(path.join(dir, 'host.js')).href);
}
// A fake build whose entry reports `states` in order, then returns a handle.
const reporting = (states) => `
  export default async function init() {}
  export async function start_player(args) {
    globalThis.__args = args;
    for (const s of ${JSON.stringify(states)}) args.onState(s);
    return { stop() { globalThis.__stops = (globalThis.__stops || 0) + 1; } };
  }`;

await test('host.js reports unavailable when no build is deployed', async () => {
  const { start } = await host();
  const c = container();
  const ctl = await start({ container: c, simId: SIM, apiBase: API });
  assert.equal(ctl.state, 'unavailable');
  assert.deepEqual(c.events.map((e) => e.state), ['loading', 'unavailable']);
});

await test('a browser without WebGPU, where only the WebGPU build is deployed, is told so', async () => {
  globalThis.__args = undefined;
  const { start } = await host(reporting(['joining']), 'webgpu');
  const c = container();
  const ctl = await start({ container: c, apiBase: API, join: LINK });
  assert.equal(ctl.state, 'unsupported-browser');
  assert.deepEqual(c.events.map((e) => e.state), ['loading', 'unsupported-browser']);
  assert.equal(globalThis.__args, undefined, 'the entry is never called');
});

await test('host.js passes the join link to the entry and keeps it nowhere else', async () => {
  const { start } = await host(reporting(['joining', 'in-world', `leak ${LINK}`]));
  const c = container();
  const ctl = await start({ container: c, apiBase: API, join: LINK });
  assert.equal(globalThis.__args.join, LINK, 'the entry gets the link as given');
  assert.ok(globalThis.__args.assetBase.endsWith('/assets/'), 'the entry gets the assets beside host.js');
  assert.deepEqual(c.events.map((e) => e.state), ['loading', 'starting', 'joining', 'in-world']);
  assert.equal(ctl.state, 'in-world');
  assert.ok(!leaked(c.events), 'no status carries the link');
  assert.ok(!Object.values(ctl).some((v) => v === LINK), 'the controller keeps no link');
  globalThis.__stops = 0;
  win.dispatchEvent(new Event('pagehide'));
  assert.equal(globalThis.__stops, 1, 'pagehide stops the session');
  ctl.stop();
  assert.equal(globalThis.__stops, 1, 'stop runs once');
  assert.equal(c.events.at(-1).state, 'stopped');
});

await test('a page runs one session', async () => {
  const { start } = await host(reporting(['joining']));
  const ctl = await start({ container: container(), apiBase: API, join: LINK });
  const again = container();
  await assert.rejects(start({ container: again, apiBase: API, join: LINK }), TypeError);
  assert.equal(again.events.length, 0, 'a second start begins nothing');
  ctl.stop();
});

await test('a published world alone is unavailable in this build', async () => {
  globalThis.__args = undefined;
  const { start } = await host(reporting(['joining']));
  const c = container();
  const ctl = await start({ container: c, simId: SIM, apiBase: API });
  assert.equal(ctl.state, 'unavailable');
  assert.equal(globalThis.__args, undefined, 'the entry is never called');
});

await test('a session lost after the world opened says so, not "could not join"', async () => {
  const { start } = await host(reporting(['joining', 'in-world', 'join-failed']));
  const c = container();
  const ctl = await start({ container: c, apiBase: API, join: LINK });
  assert.equal(ctl.state, 'disconnected');
  assert.equal(c.events.at(-1).message, 'The connection to the host was lost.');
  ctl.stop();
});

await test("a host's refusal shows only fixed text", async () => {
  for (const [reported, shown, message] of [
    ['refused:protocol', 'refused:protocol', 'This host runs a different version.'],
    ['refused:ticket-used', 'refused:ticket-used', 'This ticket was already used. Join again from the gallery.'],
    ['refused:HOST SAYS buy my coin', 'refused', 'The host did not let you in.'],
  ]) {
    const { start } = await host(reporting(['joining', reported]));
    const c = container();
    const ctl = await start({ container: c, apiBase: API, join: LINK });
    assert.equal(ctl.state, shown, reported);
    assert.equal(c.events.at(-1).message, message, reported);
    assert.ok(!leaked(c.events), reported);
    ctl.stop();
  }
});

await test('a failing join reports fixed text, never the error', async () => {
  const { start } = await host(`
    export default async function init() {}
    export async function start_player(args) { throw new Error('cannot parse ' + args.join); }`);
  const c = container();
  const ctl = await start({ container: c, apiBase: API, join: LINK });
  assert.equal(ctl.state, 'join-failed');
  assert.equal(c.events.at(-1).message, 'Could not join the session.');
  assert.ok(!leaked(c.events));
});

await test('a build that fails to load reports fixed text', async () => {
  const { start } = await host(`throw new Error('${SECRET}');`);
  const c = container();
  const ctl = await start({ container: c, simId: SIM, apiBase: API });
  assert.equal(ctl.state, 'load-failed');
  assert.deepEqual(c.events.map((e) => e.state), ['loading', 'load-failed']);
  assert.ok(!leaked(c.events));
});

await test('host.js refuses bad arguments without echoing them', async () => {
  const { start } = await host();
  const c = container();
  await assert.rejects(start({ container: c, apiBase: API, join: `https://elsewhere/${SECRET}` }), (e) => !leaked(e.message));
  await assert.rejects(start({ container: c, apiBase: API }), TypeError);
  await assert.rejects(start({ container: c, simId: SIM, apiBase: 'https://evil.example' }), TypeError);
  assert.equal(c.events.length, 0, 'nothing started');
});

// ── A fake WebTransport for net.js ──────────────────────────────────────────
class FakeTransport {
  static plan = {};
  constructor(url, options) {
    const plan = FakeTransport.plan;
    FakeTransport.last = this;
    this.url = url;
    this.options = options;
    this.ready = plan.ready ?? Promise.resolve();
    this.closed = new Promise((resolve, reject) => {
      this.finish = resolve;
      if (plan.closedRejects) plan.closedRejects.then(null, reject);
    });
    this.closes = 0;
    this.framesOut = [];
    this.datagramsOut = [];
    this.datagrams = plan.noDatagrams
      ? undefined
      : {
          maxDatagramSize: 1200,
          readable: new ReadableStream({ start: (q) => (plan.datagramsIn ?? []).forEach((d) => q.enqueue(d)) }),
          writable: new WritableStream({ write: (chunk) => this.datagramsOut.push(chunk) }),
        };
  }
  async createBidirectionalStream() {
    const plan = FakeTransport.plan;
    return {
      readable: new ReadableStream({
        start: (q) => {
          (plan.streamIn ?? []).forEach((b) => q.enqueue(b));
          if (plan.streamEnds) q.close();
        },
      }),
      // A write is delivered a moment after it starts, as the network sends
      // it, and one not yet delivered when the session closes is lost:
      // WebTransport.close() aborts its streams.
      writable: new WritableStream({
        write: (chunk) =>
          new Promise((resolve) =>
            setTimeout(() => {
              if (this.closes === 0) this.framesOut.push(chunk);
              resolve();
            }, 5),
          ),
      }),
    };
  }
  close() {
    this.closes++;
    this.finish();
  }
}
const { openLink } = await import(pathToFileURL(path.join(here, 'net.js')).href);
const URL_ = `https://127.0.0.1:7777/join/${SECRET}`;
const PIN = new Uint8Array(32).fill(7);
const recorder = () => {
  const got = { stream: [], datagrams: [], closed: [] };
  got.handlers = {
    onStreamBytes: (b) => got.stream.push(b),
    onDatagram: (d) => got.datagrams.push(d),
    onClosed: (c) => got.closed.push(c),
  };
  return got;
};
const code = (c) => (e) => e.code === c && e.message === c && !leaked(e.message);
const tick = () => new Promise((r) => setTimeout(r, 20));

await test('net.js fails by fixed codes, never by the URL, and a failed open reports once', async () => {
  const got = recorder();
  delete globalThis.WebTransport;
  await assert.rejects(openLink(URL_, PIN, got.handlers), code('no-webtransport'));
  globalThis.WebTransport = FakeTransport;
  await assert.rejects(openLink(`http://127.0.0.1:7777/join/${SECRET}`, PIN, got.handlers), code('bad-arguments'));
  await assert.rejects(openLink(URL_, new Uint8Array(31), got.handlers), code('bad-arguments'));
  const refused = Promise.reject(new Error(`refused ${URL_}`));
  FakeTransport.plan = { ready: refused, closedRejects: refused };
  await assert.rejects(openLink(URL_, PIN, got.handlers), code('connect-failed'));
  FakeTransport.plan = { ready: new Promise(() => {}) };
  await assert.rejects(openLink(URL_, PIN, got.handlers, { connectTimeoutMs: 30 }), code('connect-timeout'));
  FakeTransport.plan = { noDatagrams: true };
  await assert.rejects(openLink(URL_, PIN, got.handlers), code('no-datagrams'));
  assert.equal(FakeTransport.last.closes, 1, 'a session without datagrams is closed');
  await tick();
  assert.deepEqual(got.closed, [], 'a failed open never also reports through onClosed');
});

await test('net.js pins the certificate, opens one stream and moves bytes both ways', async () => {
  const streamIn = [new Uint8Array([1, 2, 3])];
  const datagramsIn = [new Uint8Array([9])];
  FakeTransport.plan = { streamIn, datagramsIn };
  const got = recorder();
  const link = await openLink(URL_, PIN, got.handlers);
  const t = FakeTransport.last;
  assert.deepEqual(t.options.serverCertificateHashes, [{ algorithm: 'sha-256', value: PIN }]);
  assert.equal(t.options.requireUnreliable, true, 'datagrams are required');
  await tick();
  assert.deepEqual(got.stream, streamIn);
  assert.deepEqual(got.datagrams, datagramsIn);
  await link.sendFrame(new Uint8Array([4, 0, 0, 0, 42]));
  assert.equal(t.framesOut.length, 1);
  assert.equal(link.maxDatagramSize, 1200);
  await assert.rejects(link.sendDatagram(new Uint8Array(1201)), code('datagram-too-large'));
  await link.sendDatagram(new Uint8Array(10));
  assert.equal(t.datagramsOut.length, 1, 'a datagram too large costs only itself');
  assert.equal(t.closes, 0, 'and the session stays open');
  assert.deepEqual(got.closed, []);
  link.close();
  await tick();
  assert.deepEqual(got.closed, ['closed'], 'the end is reported once');
});

await test('a frame sent just before close() reaches the host first', async () => {
  FakeTransport.plan = {};
  const got = recorder();
  const link = await openLink(URL_, PIN, got.handlers);
  const t = FakeTransport.last;
  const goodbye = new Uint8Array([1, 0, 0, 0, 9]);
  link.sendFrame(goodbye); // not awaited, as the wasm module sends it
  await link.close();
  assert.deepEqual(t.framesOut, [goodbye], 'the queued frame went out before the session closed');
  assert.ok(t.closes >= 1, 'and the session closed');
  await tick();
  assert.deepEqual(got.closed, ['closed'], 'its end is reported once');
});

await test('the frame stream ending, or a handler throwing, closes the session once', async () => {
  FakeTransport.plan = { streamIn: [new Uint8Array([1])], streamEnds: true };
  const ending = recorder();
  await openLink(URL_, PIN, ending.handlers);
  await tick();
  assert.deepEqual(ending.closed, ['closed'], 'the stream ended');
  assert.ok(FakeTransport.last.closes >= 1, 'and the session was closed');

  FakeTransport.plan = { datagramsIn: [new Uint8Array([2])] };
  const throwing = recorder();
  throwing.handlers.onDatagram = () => {
    throw new Error('handler bug');
  };
  await openLink(URL_, PIN, throwing.handlers);
  await tick();
  assert.deepEqual(throwing.closed, ['closed'], 'a throwing handler ends the session');
  assert.ok(FakeTransport.last.closes >= 1, 'and the session was closed');
});

console.log(`${passed} passed`);
