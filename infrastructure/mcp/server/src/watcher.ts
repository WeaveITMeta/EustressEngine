// SPDX-License-Identifier: MIT
//
// File watcher — the engine behind `resources/updated` notifications.
//
// MCP clients subscribe to a resource URI; when the underlying file
// changes on disk, we emit a notification and the client refreshes its
// pinned copy. This is the killer-feature part of resources: it makes
// them **subscriptions**, not RPCs.
//
// We use chokidar for cross-platform correctness. Node's built-in
// `fs.watch` is unreliable on Linux (no recursive watch) and flaky on
// network mounts; chokidar papers over those.
//
// One watcher per active Universe. When the default Universe swaps
// mid-session (via `eustress_set_default_universe`), the WatcherManager
// is responsible for tearing down the old watcher and starting a new
// one lazily — but only if any subscriptions exist. No subscribers =
// no watcher running, which keeps the cost of a just-launched server
// near zero.

import chokidar, { type FSWatcher } from 'chokidar';
import * as path from 'node:path';
import { pathToUri } from './resources.js';

export type UpdateEmitter = (uri: string) => void;

export class UniverseWatcher {
  private watcher: FSWatcher | null = null;
  constructor(
    public readonly universe: string,
    private readonly onUpdate: UpdateEmitter,
  ) {}

  start(): void {
    if (this.watcher) return;  // idempotent; safe to call repeatedly

    // We watch every text-ish file that maps onto an MCP resource kind.
    // The glob is intentionally narrow — we skip `node_modules`, dot
    // folders, binary assets — so chokidar doesn't inode-blow-up on
    // large meshes or installed deps.
    const globs = [
      path.join(this.universe, 'Spaces', '**', '*.rune'),
      path.join(this.universe, 'Spaces', '**', '*.luau'),
      path.join(this.universe, 'Spaces', '**', '*.soul'),
      path.join(this.universe, 'Spaces', '**', '*.md'),
      path.join(this.universe, 'Spaces', '**', '*.toml'),
      path.join(this.universe, '.eustress', 'knowledge', 'sessions', '*.json'),
    ];

    this.watcher = chokidar.watch(globs, {
      ignoreInitial: true,  // we already know what's on disk at startup
      ignored: [/node_modules/, /target/, /\.git\//],
      awaitWriteFinish: {
        stabilityThreshold: 120,   // don't fire until a write has settled
        pollInterval: 50,
      },
      // Polling fallback — for exFAT/network drives where inotify/FSEvents
      // don't deliver events reliably. 5% CPU penalty for paranoia.
      usePolling: false,
    });

    const handle = (absPath: string) => {
      const uri = pathToUri(this.universe, absPath);
      if (uri) this.onUpdate(uri);
    };
    this.watcher.on('add',    handle);
    this.watcher.on('change', handle);
    this.watcher.on('unlink', handle);
  }

  async stop(): Promise<void> {
    if (!this.watcher) return;
    const w = this.watcher;
    this.watcher = null;
    await w.close();
  }
}

/**
 * Tracks which URIs are actively subscribed and ensures exactly one
 * watcher runs per Universe whenever at least one subscription exists.
 *
 * The manager is the single integration point between `index.ts`'s
 * `resources/subscribe` / `resources/unsubscribe` handlers and the
 * actual filesystem watcher.
 */
export class SubscriptionManager {
  private subscribed = new Set<string>();
  private watcher: UniverseWatcher | null = null;

  constructor(
    private readonly onNotify: (uri: string) => void,
  ) {}

  /**
   * Call when `state.currentUniverse` changes. Tears down the old
   * watcher (if any) and starts a new one keyed on the new Universe —
   * but only if there's anyone to notify.
   */
  async retargetUniverse(universe: string | null): Promise<void> {
    if (this.watcher?.universe === universe) return;
    if (this.watcher) {
      await this.watcher.stop();
      this.watcher = null;
    }
    if (universe && this.subscribed.size > 0) {
      this.watcher = new UniverseWatcher(universe, this.emit);
      this.watcher.start();
    }
  }

  /** Add a subscription and spin up the watcher if it wasn't already running. */
  subscribe(uri: string, universe: string | null): void {
    const was = this.subscribed.size;
    this.subscribed.add(uri);
    if (was === 0 && universe && !this.watcher) {
      this.watcher = new UniverseWatcher(universe, this.emit);
      this.watcher.start();
    }
  }

  /** Drop a subscription; stop the watcher if no one's listening any more. */
  async unsubscribe(uri: string): Promise<void> {
    this.subscribed.delete(uri);
    if (this.subscribed.size === 0 && this.watcher) {
      await this.watcher.stop();
      this.watcher = null;
    }
  }

  /** Emit only for URIs the client actually asked about. */
  private emit = (uri: string): void => {
    if (this.subscribed.has(uri)) {
      this.onNotify(uri);
    }
  };

  async shutdown(): Promise<void> {
    this.subscribed.clear();
    await this.watcher?.stop();
    this.watcher = null;
  }
}
