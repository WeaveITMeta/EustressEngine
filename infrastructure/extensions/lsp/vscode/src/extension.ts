// SPDX-License-Identifier: MIT
//
// Eustress Rune LSP — VS Code extension entry point.
//
// All the language intelligence lives in the `eustress-lsp` binary (see
// infrastructure/extensions/lsp/ARCHITECTURE.md). This file is a thin
// client whose job is:
//   1. Resolve a server-binary path.
//   2. Preflight its existence BEFORE spawning, so a missing binary is
//      one calm actionable notification rather than the three-toast
//      failure storm vscode-languageclient otherwise produces.
//   3. Keep a persistent status-bar indicator so the user always knows
//      whether language features are live.
//
// Keep it small. Anything that feels like "knowledge about Rune" belongs
// in the Rust analyzer, not here.

import * as path from 'node:path';
import * as fs from 'node:fs';
import * as net from 'node:net';
import {
  workspace,
  ExtensionContext,
  window,
  commands,
  Uri,
  StatusBarItem,
  StatusBarAlignment,
  ThemeColor,
} from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  StreamInfo,
  TransportKind,
  State,
} from 'vscode-languageclient/node';

// ─── Module-scoped state ──────────────────────────────────────────────

// One client per Universe port (or the single string 'stdio' for the
// fallback). This is the core of multi-Universe routing: when you have
// two engines running and you open files from both, each file's
// requests land on its Universe's LSP — not the first one we found.
const clients = new Map<string | number, LanguageClient>();
let statusBar: StatusBarItem | undefined;
let currentCtx: ExtensionContext | undefined;
const OUTPUT_CHANNEL_NAME = 'Eustress (Rune)';
// "Learn" page that documents install + launch. The LSP binary ships inside
// the Eustress Engine installer now — there's no separate download — so this
// URL is a help link, not a binary download target.
const ENGINE_LEARN_URL = 'https://eustress.dev/learn/ide';
const ENGINE_DOWNLOAD_URL = 'https://eustress.dev/download';

/**
 * Walk up from `startPath` looking for `.eustress/lsp.port`. Used to
 * figure out which Universe a file belongs to. Returns the port (TCP)
 * or `null` if none is found before hitting the filesystem root.
 */
function findPortFor(startPath: string): number | null {
  let cur = path.resolve(startPath);
  const seen = new Set<string>();
  for (let i = 0; i < 64 && !seen.has(cur); i++) {
    seen.add(cur);
    const candidate = path.join(cur, '.eustress', 'lsp.port');
    try {
      if (fs.existsSync(candidate)) {
        const port = parseInt(fs.readFileSync(candidate, 'utf8').trim(), 10);
        if (Number.isFinite(port) && port > 0 && port < 65536) return port;
      }
    } catch { /* unreadable — keep walking */ }
    const parent = path.dirname(cur);
    if (parent === cur) break;
    cur = parent;
  }
  return null;
}

/**
 * Also walk up to find the Universe root (the directory containing
 * `Spaces/` AND `.eustress/`). Used to scope a per-Universe client's
 * `documentSelector` so requests for that Universe's files — and only
 * those — reach its LSP.
 */
function findUniverseRoot(startPath: string): string | null {
  let cur = path.resolve(startPath);
  const seen = new Set<string>();
  for (let i = 0; i < 64 && !seen.has(cur); i++) {
    seen.add(cur);
    if (fs.existsSync(path.join(cur, 'Spaces'))) {
      return cur;
    }
    const parent = path.dirname(cur);
    if (parent === cur) break;
    cur = parent;
  }
  return null;
}

// ─── Binary resolution ────────────────────────────────────────────────

type Resolved =
  | { kind: 'file';     path: string }              // concrete path, exists on disk (or found via PATH walk)
  | { kind: 'missing'                   };          // nothing to try — UX prompts for setup

/**
 * Resolve the server in the order documented in ARCHITECTURE.md.
 * Returns a tagged union so callers can distinguish "we have a concrete
 * file" from "we're about to hope PATH works" — the UX branches are
 * different.
 */
function resolveServerPath(ctx: ExtensionContext): Resolved {
  const exeSuffix = process.platform === 'win32' ? '.exe' : '';
  const binName = `eustress-lsp${exeSuffix}`;

  // 1. User override (absolute path from settings).
  const userPath = workspace.getConfiguration('eustress').get<string>('serverPath');
  if (userPath && userPath.trim().length > 0) {
    return fs.existsSync(userPath)
      ? { kind: 'file', path: userPath }
      : { kind: 'missing' };   // user explicitly set a bad path → report
  }

  // 2. Environment variable (shell / nix integrations).
  const envPath = process.env.EUSTRESS_LSP_PATH;
  if (envPath && fs.existsSync(envPath)) {
    return { kind: 'file', path: envPath };
  }

  // 3. Workspace-local dev build (release preferred over debug).
  const wsFolder = workspace.workspaceFolders?.[0]?.uri.fsPath;
  if (wsFolder) {
    const release = path.join(wsFolder, 'target', 'release', binName);
    if (fs.existsSync(release)) return { kind: 'file', path: release };
    const debug = path.join(wsFolder, 'target', 'debug', binName);
    if (fs.existsSync(debug))   return { kind: 'file', path: debug };
  }

  // 4. Bundled binary inside the VSIX (opt-in via `build-vsix.sh --bundle`).
  const bundled = path.join(ctx.extensionPath, 'server', binName);
  if (fs.existsSync(bundled)) return { kind: 'file', path: bundled };

  // 5. Walk PATH ourselves. If we find it, return a file result so the
  //    status-bar gets the concrete path; otherwise report missing so
  //    the user sees a proper prompt instead of an opaque spawn error.
  const onPath = whichSync(binName);
  if (onPath) return { kind: 'file', path: onPath };

  return { kind: 'missing' };
}

/**
 * Cross-platform PATH lookup. Node doesn't expose this, and shelling out
 * to `where` / `which` is both slow and adds a shell dependency.
 */
function whichSync(name: string): string | null {
  const PATH = process.env.PATH ?? '';
  const sep = process.platform === 'win32' ? ';' : ':';
  const extensions = process.platform === 'win32'
    ? (process.env.PATHEXT ?? '.COM;.EXE;.BAT;.CMD').split(';')
    : [''];
  for (const dir of PATH.split(sep)) {
    if (!dir) continue;
    for (const ext of extensions) {
      const candidate = path.join(dir, name.endsWith(ext) ? name : name + ext);
      try {
        if (fs.existsSync(candidate) && fs.statSync(candidate).isFile()) {
          return candidate;
        }
      } catch { /* ignore */ }
    }
  }
  return null;
}

// ─── Status bar ───────────────────────────────────────────────────────

type Status =
  | { state: 'waiting-for-engine' }  // engine not running; no port file yet
  | { state: 'starting'; path: string }
  | { state: 'running';  path: string }
  | { state: 'error';    message: string };

function renderStatus(s: Status): void {
  if (!statusBar) return;
  switch (s.state) {
    case 'waiting-for-engine':
      // The LSP ships bundled with Eustress Engine — if we got here, Eustress
      // simply isn't running yet (or isn't open on a Universe). Not an error
      // condition, just "waiting." Click routes to the setup help.
      statusBar.text = '$(debug-start) Rune LSP: engine not running';
      statusBar.tooltip =
        'The Rune language server is bundled with Eustress Engine. ' +
        'Launch Eustress Engine and open a Universe — the extension will connect automatically.\n\n' +
        'Click for setup help.';
      statusBar.backgroundColor = new ThemeColor('statusBarItem.warningBackground');
      statusBar.command = 'eustress.setupServer';
      break;
    case 'starting':
      statusBar.text = '$(sync~spin) Rune LSP: starting…';
      statusBar.tooltip = `Launching ${s.path}`;
      statusBar.backgroundColor = undefined;
      statusBar.command = 'eustress.showServerPath';
      break;
    case 'running':
      statusBar.text = '$(check) Rune LSP';
      statusBar.tooltip = `Connected — ${s.path}`;
      statusBar.backgroundColor = undefined;
      statusBar.command = 'eustress.restartServer';
      break;
    case 'error':
      statusBar.text = '$(error) Rune LSP: error';
      statusBar.tooltip = `${s.message}\n\nClick to see details.`;
      statusBar.backgroundColor = new ThemeColor('statusBarItem.errorBackground');
      statusBar.command = 'eustress.showOutput';
      break;
  }
  statusBar.show();
}

// ─── Start / stop ─────────────────────────────────────────────────────

/**
 * Ensure a LanguageClient exists for the Universe that owns `filePath`.
 * Each distinct Universe — live TCP port OR the single `stdio` fallback —
 * gets its own client, keyed by port number (or the string `'stdio'`).
 * Same-key calls reuse the already-running client. This is what lets
 * a single Windsurf window route files to the correct engine when
 * multiple engine instances are running on different Universes.
 */
async function ensureClientForFile(filePath: string): Promise<void> {
  if (!currentCtx) return;

  const universeRoot = findUniverseRoot(filePath);
  const tcpPort = universeRoot
    ? findPortFor(universeRoot)
    : findPortFor(path.dirname(filePath));

  const key: string | number = tcpPort ?? 'stdio';
  if (clients.has(key)) {
    // Already have a client for this routing target — nothing to do.
    return;
  }

  let serverOptions: ServerOptions;
  let transportLabel: string;
  let selectorPattern: string | undefined;

  if (typeof key === 'number') {
    const port = key;
    serverOptions = () => new Promise<StreamInfo>((resolve, reject) => {
      const socket = net.createConnection({ host: '127.0.0.1', port });
      // Hard cap the connect attempt so a stale `.eustress/lsp.port` (pointing
      // at a port nobody's listening on — prior engine crashed, engine not
      // running yet) rejects fast instead of wedging extension activation.
      // Windows' default TCP connect retry blocks for ~21s; we want sub-second.
      const TCP_CONNECT_TIMEOUT_MS = 1500;
      const timer = setTimeout(() => {
        socket.destroy();
        reject(new Error(
          `timeout connecting to 127.0.0.1:${port} after ${TCP_CONNECT_TIMEOUT_MS}ms ` +
          `(stale .eustress/lsp.port? engine not running?)`,
        ));
      }, TCP_CONNECT_TIMEOUT_MS);
      socket.once('connect', () => {
        clearTimeout(timer);
        resolve({ writer: socket, reader: socket });
      });
      socket.once('error', (err) => {
        clearTimeout(timer);
        reject(err);
      });
    });
    transportLabel = `tcp://127.0.0.1:${port} (${universeRoot ? path.basename(universeRoot) : 'engine'})`;
    // Scope THIS client to files under the matching Universe so documents
    // from a different Universe don't accidentally hit this port.
    if (universeRoot) {
      selectorPattern = path.join(universeRoot, '**/*.rune').split(path.sep).join('/');
    }
  } else {
    // No port file = no running engine in this Universe. The LSP is
    // bundled with Eustress Engine and is spawned by the engine — so the
    // correct action is "launch Eustress Engine," not "install a separate
    // binary." We attempt a last-ditch stdio fallback to the bundled
    // binary if it's findable on disk (power-users running the LSP
    // standalone for CI/headless work); otherwise we render the
    // waiting-for-engine state and surface setup help.
    const resolved = resolveServerPath(currentCtx);
    if (resolved.kind === 'missing') {
      renderStatus({ state: 'waiting-for-engine' });
      await offerServerSetup();
      return;
    }
    transportLabel = resolved.path;
    serverOptions = {
      run:   { command: resolved.path, transport: TransportKind.stdio },
      debug: { command: resolved.path, transport: TransportKind.stdio },
    };
  }

  renderStatus({ state: 'starting', path: transportLabel });

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      selectorPattern
        ? { scheme: 'file', language: 'rune', pattern: selectorPattern }
        : { scheme: 'file', language: 'rune' },
    ],
    outputChannelName: `Eustress (${transportLabel})`,
    synchronize: {
      configurationSection: 'eustress',
      fileEvents: workspace.createFileSystemWatcher('**/*.rune'),
    },
  };

  const c = new LanguageClient(
    `eustress-rune-lsp-${key}`,
    `Eustress Rune LSP (${transportLabel})`,
    serverOptions,
    clientOptions,
  );

  c.onDidChangeState((event) => {
    if (event.newState === State.Running) {
      renderStatus({ state: 'running', path: transportLabel });
    } else if (event.newState === State.Stopped && event.oldState === State.Starting) {
      renderStatus({ state: 'error', message: `Server exited during startup (${transportLabel})` });
      clients.delete(key);
    }
  });

  clients.set(key, c);
  try {
    await c.start();
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    renderStatus({ state: 'error', message: msg });
    clients.delete(key);
    const pick = await window.showErrorMessage(
      `Rune LSP failed to start (${transportLabel}): ${msg}. ` +
      `Most common cause: Eustress Engine isn't running (the engine spawns the LSP).`,
      'Show Output',
      'Help',
      'Advanced: configure path',
    );
    if (pick === 'Show Output') {
      await commands.executeCommand('eustress.showOutput');
    } else if (pick === 'Help') {
      await commands.executeCommand('vscode.open', Uri.parse(ENGINE_LEARN_URL));
    } else if (pick === 'Advanced: configure path') {
      await commands.executeCommand('workbench.action.openSettings', 'eustress.serverPath');
    }
  }
}

/**
 * Shut down every client and clear the map. Used by the restart command
 * and extension deactivation.
 */
async function stopAllClients(): Promise<void> {
  await Promise.all(
    Array.from(clients.values()).map(c => c.stop().catch(() => {})),
  );
  clients.clear();
}

/**
 * Shown once per activation when no running engine is found and no bundled
 * binary can be located on PATH. The Rune language server ships inside the
 * Eustress Engine installer, so the primary action is "install / launch
 * Eustress," not "download a separate server." Collapses the multi-toast
 * failure path vscode-languageclient would otherwise produce into a single
 * notification.
 */
async function offerServerSetup(): Promise<void> {
  const pick = await window.showInformationMessage(
    'Rune language features come from Eustress Engine — the engine launches the Rune LSP ' +
    'automatically whenever a Universe is open. No separate download needed. If you don\'t ' +
    'have Eustress Engine installed yet, grab it from eustress.dev/download.',
    'Install Eustress Engine',
    'How it works',
    'Advanced: configure path',
    'Dismiss',
  );
  if (pick === 'Install Eustress Engine') {
    await commands.executeCommand('vscode.open', Uri.parse(ENGINE_DOWNLOAD_URL));
  } else if (pick === 'How it works') {
    await commands.executeCommand('vscode.open', Uri.parse(ENGINE_LEARN_URL));
  } else if (pick === 'Advanced: configure path') {
    await commands.executeCommand('workbench.action.openSettings', 'eustress.serverPath');
  }
}

// ─── Activation ───────────────────────────────────────────────────────

export async function activate(ctx: ExtensionContext): Promise<void> {
  currentCtx = ctx;

  // Status bar item first — it's the canonical "is the LSP up?" signal
  // and must be visible even when the server fails to start.
  statusBar = window.createStatusBarItem(StatusBarAlignment.Right, 100);
  ctx.subscriptions.push(statusBar);

  ctx.subscriptions.push(
    commands.registerCommand('eustress.restartServer', async () => {
      await stopAllClients();
      // Re-fire for every currently-open .rune document so clients respawn.
      for (const doc of workspace.textDocuments) {
        if (doc.languageId === 'rune') await ensureClientForFile(doc.uri.fsPath);
      }
    }),
    commands.registerCommand('eustress.setupServer',   () => offerServerSetup()),
    commands.registerCommand('eustress.showServerPath', () => {
      const r = resolveServerPath(ctx);
      const msg = r.kind === 'file' ? r.path : '<not found>';
      const live = Array.from(clients.keys()).join(', ') || '(none)';
      window.showInformationMessage(
        `Rune LSP — stdio binary: ${msg}\nLive clients: ${live}`,
      );
    }),
    commands.registerCommand('eustress.showOutput', () => {
      // Show the first active client's output channel; for multi-Universe
      // workflows each client has its own labelled channel and the user
      // picks from the output dropdown.
      const c = clients.values().next().value;
      c?.outputChannel.show(true);
    }),
  );

  // Route on every .rune document open. `onDidOpenTextDocument` fires
  // for every document the user opens (or that's already open at
  // activation time for the first .rune file they touch), so we get
  // per-file routing without extra polling.
  ctx.subscriptions.push(
    workspace.onDidOpenTextDocument((doc) => {
      // Same no-await-in-event-handler pattern as the initial sweep below —
      // if the TCP probe is slow, we don't block VS Code's event loop.
      if (doc.languageId === 'rune') {
        ensureClientForFile(doc.uri.fsPath).catch((err) => {
          console.error('[eustress-rune-lsp] onDidOpen client start failed:', err);
        });
      }
    }),
  );

  // Kick off clients for documents that were already open when activation
  // fired. Do NOT await these — a stale port file or a slow TCP probe would
  // otherwise wedge the extension in "Activating..." indefinitely. The
  // status bar surfaces success/failure for each client independently, and
  // the `Rune: Restart` command is always available for recovery.
  for (const doc of workspace.textDocuments) {
    if (doc.languageId === 'rune') {
      ensureClientForFile(doc.uri.fsPath).catch((err) => {
        console.error('[eustress-rune-lsp] initial client start failed:', err);
      });
    }
  }
}

export async function deactivate(): Promise<void> {
  statusBar?.dispose();
  await stopAllClients();
}
