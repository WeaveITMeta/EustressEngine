// A scripted src/package_scripts.mjs for the review's scripts stage (deps.packageScripts): a package extracted in full whose
// scripts are `scripts`. Guardian Code v2.4 lists a Space only when every script players receive was read in full from the
// published package, so a test that expects a listing passes one of these; a test about the dossier-only (.pak) path passes
// `legacyPackage()`.

import { createHash } from 'node:crypto';

export const DEFAULT_PACKAGE_SCRIPTS = [{ path: 'Main/console.luau', space: 'Main', audience: 'players', language: 'luau', source: 'local depth = basin + tide' }];

export function packageFrom(scripts = DEFAULT_PACKAGE_SCRIPTS) {
  const full = scripts.map(s => ({ bytes: s.source.length, sha256: createHash('sha256').update(s.source).digest('hex'), ...s }));
  const bytes = full.reduce((n, s) => n + s.source.length, 0);
  return {
    extractPackageScripts: async () => ({ supported: true, complete: true, key: 'test-package', chunks_total: 1, chunks_read: 1, scripts_total: full.length, script_bytes: bytes, distinct_scripts: full.length, distinct_bytes: bytes, errors: [] }),
    readPackageScripts: async () => full.map(s => ({ ...s })),
  };
}

export function legacyPackage() {
  return { extractPackageScripts: async () => ({ supported: false }), readPackageScripts: async () => { throw new Error('not supported'); } };
}
