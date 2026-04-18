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
  TransportKind,
  State,
} from 'vscode-languageclient/node';

// ─── Module-scoped state ──────────────────────────────────────────────

let client: LanguageClient | undefined;
let statusBar: StatusBarItem | undefined;
const OUTPUT_CHANNEL_NAME = 'Eustress (Rune)';
const DOWNLOAD_URL = 'https://eustress.dev/learn#ide-integration';

// ─── Binary resolution ────────────────────────────────────────────────

type Resolved =
  | { kind: 'file';     path: string }              // exists on disk
  | { kind: 'pathName'; name: string }              // bare name, hope it's in PATH
  | { kind: 'missing'                   };          // nothing to try

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
  | { state: 'missing' }
  | { state: 'starting'; path: string }
  | { state: 'running';  path: string }
  | { state: 'error';    message: string };

function renderStatus(s: Status): void {
  if (!statusBar) return;
  switch (s.state) {
    case 'missing':
      statusBar.text = '$(warning) Rune LSP: not installed';
      statusBar.tooltip = 'The `eustress-lsp` binary was not found. Click for setup options.';
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

async function startClient(ctx: ExtensionContext): Promise<void> {
  if (client) {
    try { await client.stop(); } catch { /* swallow — we're replacing it */ }
    client = undefined;
  }

  const resolved = resolveServerPath(ctx);

  if (resolved.kind === 'missing') {
    renderStatus({ state: 'missing' });
    await offerServerSetup();
    return;
  }

  const serverCommand = resolved.path;
  renderStatus({ state: 'starting', path: serverCommand });

  const serverOptions: ServerOptions = {
    run:   { command: serverCommand, transport: TransportKind.stdio },
    debug: { command: serverCommand, transport: TransportKind.stdio },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: 'file', language: 'rune' }],
    outputChannelName: OUTPUT_CHANNEL_NAME,
    synchronize: {
      configurationSection: 'eustress',
      fileEvents: workspace.createFileSystemWatcher('**/*.rune'),
    },
    // revealOutputChannelOn defaults to Error — that suppresses the
    // "Show Output"-style toasts vscode-languageclient would otherwise
    // pile on top of our own. We handle error surfacing via the status
    // bar + a single notification, not the output panel auto-reveal.
  };

  client = new LanguageClient(
    'eustress-rune-lsp',
    'Eustress Rune LSP',
    serverOptions,
    clientOptions,
  );

  client.onDidChangeState((event) => {
    if (event.newState === State.Running) {
      renderStatus({ state: 'running', path: serverCommand });
    } else if (event.newState === State.Stopped && event.oldState === State.Starting) {
      // Server died during handshake. Surface a single message with a
      // "Show Output" action instead of letting vscode-languageclient
      // cascade its own toasts.
      renderStatus({
        state: 'error',
        message: `Server exited during startup (${serverCommand})`,
      });
    }
  });

  try {
    await client.start();
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    renderStatus({ state: 'error', message: msg });
    const pick = await window.showErrorMessage(
      `Rune LSP failed to start: ${msg}`,
      'Show Output',
      'Reconfigure',
      'Download Server',
    );
    if (pick === 'Show Output') {
      await commands.executeCommand('eustress.showOutput');
    } else if (pick === 'Reconfigure') {
      await commands.executeCommand('workbench.action.openSettings', 'eustress.serverPath');
    } else if (pick === 'Download Server') {
      await commands.executeCommand('vscode.open', Uri.parse(DOWNLOAD_URL));
    }
  }
}

/**
 * Shown once per activation when the binary can't be resolved. Collapses
 * the multi-toast failure path vscode-languageclient would otherwise
 * produce into a single, actionable notification.
 */
async function offerServerSetup(): Promise<void> {
  const pick = await window.showInformationMessage(
    'Rune language features need the `eustress-lsp` server. Run the Eustress engine (which ships the binary), download it separately, or configure a path.',
    'Download Server',
    'How it works',
    'Configure Path',
    'Dismiss',
  );
  if (pick === 'Download Server') {
    await commands.executeCommand('vscode.open', Uri.parse(DOWNLOAD_URL));
  } else if (pick === 'How it works') {
    await commands.executeCommand(
      'vscode.open',
      Uri.parse('https://eustress.dev/learn#ide-integration'),
    );
  } else if (pick === 'Configure Path') {
    await commands.executeCommand('workbench.action.openSettings', 'eustress.serverPath');
  }
}

// ─── Activation ───────────────────────────────────────────────────────

export async function activate(ctx: ExtensionContext): Promise<void> {
  // Status bar item first — it's the canonical "is the LSP up?" signal
  // and must be visible even when the server fails to start.
  statusBar = window.createStatusBarItem(StatusBarAlignment.Right, 100);
  ctx.subscriptions.push(statusBar);

  ctx.subscriptions.push(
    commands.registerCommand('eustress.restartServer', () => startClient(ctx)),
    commands.registerCommand('eustress.setupServer',   () => offerServerSetup()),
    commands.registerCommand('eustress.showServerPath', () => {
      const r = resolveServerPath(ctx);
      const msg = r.kind === 'file' ? r.path : '<not found>';
      window.showInformationMessage(`Rune LSP server: ${msg}`);
    }),
    commands.registerCommand('eustress.showOutput', () => {
      client?.outputChannel.show(true);
    }),
  );

  await startClient(ctx);
}

export function deactivate(): Thenable<void> | undefined {
  statusBar?.dispose();
  return client?.stop();
}
