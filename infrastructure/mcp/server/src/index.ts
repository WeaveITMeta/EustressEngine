#!/usr/bin/env node
// SPDX-License-Identifier: MIT
//
// Eustress MCP server — stdio entry point.
//
// Protocol shell. All language logic lives in `tools.ts` / `resources.ts`.
// This file registers handlers, resolves the active Universe, and hands
// notifications from the file watcher up to the MCP transport.

import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
  ListResourcesRequestSchema,
  ListResourceTemplatesRequestSchema,
  ReadResourceRequestSchema,
  SubscribeRequestSchema,
  UnsubscribeRequestSchema,
  ListPromptsRequestSchema,
} from '@modelcontextprotocol/sdk/types.js';
import * as path from 'node:path';
import * as os from 'node:os';
import { TOOLS, type ServerState } from './tools.js';
import { findUniverseRoot, discoverUniverses } from './universe.js';
import { listResources, readResource } from './resources.js';
import { URI_TEMPLATES } from './uri.js';
import { SubscriptionManager } from './watcher.js';

function resolveInitialUniverse(): string | null {
  const argIdx = process.argv.indexOf('--universe');
  const fromArg = argIdx >= 0 ? process.argv[argIdx + 1] : undefined;
  const fromEnv = process.env.EUSTRESS_UNIVERSE;
  if (fromArg) return path.resolve(fromArg);
  if (fromEnv) return path.resolve(fromEnv);
  return findUniverseRoot(process.cwd());
}

function resolveSearchRoots(): string[] {
  const fromEnv = process.env.EUSTRESS_UNIVERSES_PATH;
  if (fromEnv) return fromEnv.split(path.delimiter).filter(Boolean);
  const home = os.homedir();
  return [
    path.join(home, 'Eustress'),
    path.join(home, 'Documents', 'Eustress'),
    home,
  ];
}

async function main(): Promise<void> {
  const state: ServerState = {
    currentUniverse: resolveInitialUniverse(),
    searchRoots: resolveSearchRoots(),
  };

  const server = new Server(
    { name: 'eustress-mcp-server', version: '0.2.1' },
    {
      capabilities: {
        tools: {},
        resources: {
          // We emit `notifications/resources/updated` when subscribed
          // files change. Tell clients they can rely on that.
          subscribe: true,
          listChanged: false,  // listing itself is stable per-universe
        },
        prompts: {},
      },
    },
  );

  // Subscription manager — one watcher per active Universe, lazily
  // started when the first subscription arrives.
  const subs = new SubscriptionManager((uri) => {
    // Server SDK auto-framing: send the canonical notification.
    server.notification({
      method: 'notifications/resources/updated',
      params: { uri },
    }).catch(() => { /* client disconnected — drop */ });
  });

  // ─── Tools ──────────────────────────────────────────────────────────
  server.setRequestHandler(ListToolsRequestSchema, async () => ({
    tools: TOOLS.map(t => ({
      name: t.name,
      description: t.description,
      inputSchema: t.inputSchema,
    })),
  }));

  server.setRequestHandler(CallToolRequestSchema, async (req) => {
    const { name, arguments: args } = req.params;
    const tool = TOOLS.find(t => t.name === name);
    if (!tool) {
      return { content: [{ type: 'text', text: `Unknown tool: ${name}` }], isError: true };
    }
    // When a tool changes state.currentUniverse, we need to re-target
    // the subscription watcher so subsequent updates land on the new
    // Universe's file events.
    const priorUniverse = state.currentUniverse;
    try {
      const result = await tool.handler((args as Record<string, unknown>) ?? {}, state);
      if (state.currentUniverse !== priorUniverse) {
        await subs.retargetUniverse(state.currentUniverse);
      }
      return result;
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return { content: [{ type: 'text', text: `Tool '${name}' threw: ${msg}` }], isError: true };
    }
  });

  // Auto-resolve a Universe if none is set. Clients that call
  // `resources/list` or `resources/read` before `eustress_set_default_universe`
  // shouldn't be punished with an empty list — the AI would see "no
  // resources" and give up rather than discover what's on disk.
  //
  // Strategy: walk cwd first (most specific), then sweep the search
  // roots (broadest). If we find anything, commit to the first result.
  // The auto-decision is logged to stderr so operators can see what
  // happened; a subsequent `eustress_set_default_universe` overrides.
  const autoResolveUniverse = async (): Promise<string | null> => {
    if (state.currentUniverse) return state.currentUniverse;
    const byCwd = findUniverseRoot(process.cwd());
    if (byCwd) {
      state.currentUniverse = byCwd;
      process.stderr.write(`[eustress-mcp] auto-resolved Universe from cwd → ${byCwd}\n`);
      await subs.retargetUniverse(byCwd);
      return byCwd;
    }
    const found = await discoverUniverses(state.searchRoots);
    if (found.length > 0) {
      state.currentUniverse = found[0];
      process.stderr.write(
        `[eustress-mcp] auto-resolved Universe from search roots → ${found[0]}` +
        (found.length > 1 ? ` (${found.length - 1} others available, use eustress_list_universes)` : '') +
        `\n`,
      );
      await subs.retargetUniverse(found[0]);
      return found[0];
    }
    return null;
  };

  // ─── Resources ──────────────────────────────────────────────────────
  server.setRequestHandler(ListResourcesRequestSchema, async () => {
    const universe = await autoResolveUniverse();
    if (!universe) {
      // Still nothing. Surface a single synthetic "help" resource so the
      // AI isn't left guessing — MCP has no `info` field on an empty
      // list, and we want the LLM to see actionable text.
      return {
        resources: [{
          uri: 'eustress://help/setup',
          name: 'Getting started',
          description:
            'No Universe found on disk. Call the `eustress_list_universes` tool to discover, or ' +
            '`eustress_set_default_universe` to point at one explicitly.',
          mimeType: 'text/markdown',
        }],
      };
    }
    return { resources: await listResources(universe) };
  });

  server.setRequestHandler(ListResourceTemplatesRequestSchema, async () => ({
    resourceTemplates: URI_TEMPLATES,
  }));

  server.setRequestHandler(ReadResourceRequestSchema, async (req) => {
    const uri = req.params.uri;

    // Help pseudo-resource — exists only when no Universe is configured.
    if (uri === 'eustress://help/setup') {
      return {
        contents: [{
          uri,
          mimeType: 'text/markdown',
          text:
            '# Eustress MCP — Getting started\n\n' +
            'The server is running but has no Universe selected, so there are no\n' +
            'Spaces, Scripts, or entities to browse.\n\n' +
            '**Next steps** (pick one):\n\n' +
            '1. Call the `eustress_list_universes` tool — it scans the configured\n' +
            '   search roots (`EUSTRESS_UNIVERSES_PATH` env var; defaults to\n' +
            '   `~/Eustress`, `~/Documents/Eustress`, home) and any Universe enclosing\n' +
            '   the current working directory.\n' +
            '2. Call `eustress_set_default_universe` with an absolute path to a folder\n' +
            '   that contains `Spaces/`.\n' +
            '3. Restart the MCP server with `EUSTRESS_UNIVERSE=/path/to/Universe` or\n' +
            '   `--universe /path/to/Universe`.\n\n' +
            'Once a Universe is selected, `resources/list` will return the Spaces,\n' +
            'scripts, conversations, and briefs in that Universe.',
        }],
      };
    }

    const universe = await autoResolveUniverse();
    if (!universe) {
      throw new Error(
        'No Universe configured. Call `eustress_list_universes` / ' +
        '`eustress_set_default_universe` first.',
      );
    }
    const block = await readResource(universe, uri);
    return { contents: [block] };
  });

  // ─── Subscriptions ──────────────────────────────────────────────────
  server.setRequestHandler(SubscribeRequestSchema, async (req) => {
    subs.subscribe(req.params.uri, state.currentUniverse);
    return {};
  });
  server.setRequestHandler(UnsubscribeRequestSchema, async (req) => {
    await subs.unsubscribe(req.params.uri);
    return {};
  });

  // ─── Prompts (empty, present to silence client probes) ─────────────
  server.setRequestHandler(ListPromptsRequestSchema, async () => ({ prompts: [] }));

  // ─── Shutdown hygiene ──────────────────────────────────────────────
  const shutdown = async () => {
    await subs.shutdown();
    process.exit(0);
  };
  process.on('SIGINT', shutdown);
  process.on('SIGTERM', shutdown);

  const transport = new StdioServerTransport();
  await server.connect(transport);

  process.stderr.write(
    `[eustress-mcp] v0.2.1 ready — ` +
    `universe=${state.currentUniverse ?? '(none; set via tool)'}, ` +
    `tools=${TOOLS.length}, ` +
    `resources=live (subscribe+update), ` +
    `search_roots=[${state.searchRoots.join(', ')}]\n`,
  );
}

main().catch((err) => {
  process.stderr.write(`[eustress-mcp] fatal: ${err?.stack ?? err}\n`);
  process.exit(1);
});
