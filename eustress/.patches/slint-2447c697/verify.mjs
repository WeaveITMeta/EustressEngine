// Checks this vendored crate against the Slint revision it was made from:
// every file but lib.rs, Cargo.toml and the license copies is identical to
// upstream, upstream's lib.rs is the one the patch was made against, and the
// vendored lib.rs is that file with lib.rs.patch applied (by hash).
//   node verify.mjs            (upstream from Cargo's git checkout)
//   SLINT_SRC=<dir> node verify.mjs   (internal/renderers/software of a clone)
// Exit 0 when it all holds; 1 with the reasons when it does not.
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const VENDORED = path.join(HERE, "i-slint-renderer-software");
const REVISION = "2447c697327e7f87c5044af229e0be6c18608fcc";
const UPSTREAM_LIB_SHA256 = "015947a2d166ce82b69740b445d2768f6428a71deb8696ac82028ac3506fb51a";
const PATCHED_LIB_SHA256 = "22808f5f3988008d6d36371eac1fb40307e8471c5d7a0c21df77bfe3e2024b59";
const OWN = new Set(["lib.rs", "Cargo.toml"]); // differ on purpose
const sha256 = (file) => crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");

function upstreamDir() {
  if (process.env.SLINT_SRC) return process.env.SLINT_SRC;
  const home = process.env.CARGO_HOME || path.join(os.homedir(), ".cargo");
  const checkouts = path.join(home, "git", "checkouts");
  for (const repo of fs.existsSync(checkouts) ? fs.readdirSync(checkouts) : []) {
    if (!repo.startsWith("slint-")) continue;
    const dir = path.join(checkouts, repo, REVISION.slice(0, 7), "internal", "renderers", "software");
    if (fs.existsSync(dir)) return dir;
  }
  return null;
}

function files(dir, rel = "") {
  return fs.readdirSync(path.join(dir, rel), { withFileTypes: true }).flatMap((e) => {
    const r = path.join(rel, e.name);
    return e.isDirectory() ? files(dir, r) : [r];
  });
}

const problems = [];
const upstream = upstreamDir();
if (!upstream) {
  problems.push(`no upstream checkout of ${REVISION} found (build once, or set SLINT_SRC)`);
} else {
  for (const rel of files(upstream)) {
    if (OWN.has(rel) || rel.startsWith("LICENSES")) continue;
    const ours = path.join(VENDORED, rel);
    if (!fs.existsSync(ours)) problems.push(`missing ${rel}`);
    else if (sha256(ours) !== sha256(path.join(upstream, rel))) problems.push(`${rel} differs from upstream`);
  }
  for (const rel of files(VENDORED)) {
    if (!OWN.has(rel) && !rel.startsWith("LICENSES") && !fs.existsSync(path.join(upstream, rel))) problems.push(`extra file ${rel}`);
  }
  if (sha256(path.join(upstream, "lib.rs")) !== UPSTREAM_LIB_SHA256) problems.push("upstream lib.rs is not the one the patch was made against");
}
if (sha256(path.join(VENDORED, "lib.rs")) !== PATCHED_LIB_SHA256) problems.push("vendored lib.rs is not upstream plus lib.rs.patch");
for (const name of ["GPL-3.0-only.txt", "LicenseRef-Slint-Royalty-free-2.0.md", "LicenseRef-Slint-Software-3.0.md"]) {
  if (!fs.existsSync(path.join(VENDORED, "LICENSES", name))) problems.push(`license text ${name} missing`);
}

if (problems.length) {
  console.error("NOT VERIFIED:\n  " + problems.join("\n  "));
  process.exit(1);
}
console.log(`verified against ${REVISION} (${upstream})`);
