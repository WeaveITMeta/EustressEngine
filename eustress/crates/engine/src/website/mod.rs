//! # Website
//!
//! A Space owns its numbers. This module is how a website gets them.
//!
//! An author marks values in the Explorer as References inside the `Website`
//! service. On publish they bake into one small JSON object beside the
//! Universe `.pak`, and a website fetches that one document and updates every
//! value it displays from it.
//!
//! The problem being solved is retyping. A site that quotes a Space's numbers
//! currently copies them by hand, and copied numbers drift: the V-Cell site
//! carried 907 Wh/kg and 699 cycles for a week after the specification said 953
//! and 237, because nothing connected the two.
//!
//! The design constraint that shapes all of it: **one fetch, not one per
//! value.** A page quoting twenty-five numbers costs one request, and the
//! twenty-sixth costs nothing.
//!
//! ## The rule that everything else follows from
//!
//! **A failed reference fails the publish.** No `null`, no previous value
//! carried forward, no reference quietly skipped. The failure this exists to
//! prevent is a website confidently showing a stale number, and a bake that
//! degrades silently reintroduces it somewhere nobody is watching. Every error
//! names the reference, the source it could not resolve, and the nearest
//! candidates in the tree, so the message is a fix rather than a ticket.
//!
//! ## What the manifest key does
//!
//! It names the caller. Traffic is attributed and rate limited per key, and a
//! key can be revoked, which cuts off one consumer without touching any other.
//!
//! It does not make the manifest secret. A key a public website sends from
//! browser JavaScript is in the page source and in the network tab, readable by
//! anyone who opens devtools. What this buys is revocable attribution and abuse
//! control, which is real and worth having, and it is not confidentiality. See
//! [`key`] for the whole picture, including what to reach for when
//! confidentiality is the actual requirement.
//!
//! ## Table of Contents
//!
//! 1. [`key`] - minting, storage, rotation, revocation, verification
//! 2. [`resolve`] - the five reference kinds, dependency order, cycle naming
//! 3. [`bake`] - the manifest shape, formatting, and the publish seam
//!
//! ## Calling it
//!
//! One call from `do_publish`, which holds the `World`, between
//! `prepare_publish_manifests` and the upload thread:
//!
//! ```ignore
//! let pending = match website::bake_from_world(world, &space_root, &universe_root) {
//!     Ok(Some(p)) => Some(p),
//!     Ok(None) => None,                  // no Website service, or no references
//!     Err(e) => { notify.error(e.to_string()); return; }
//! };
//! ```
//!
//! Then inside the upload thread, once the `.pak` digest and the simulation id
//! exist:
//!
//! ```ignore
//! let baked = pending.finalize(&sim_id, &website::blake3_publish_hash(&pak_bytes));
//! put(baked.object_key(),  baked.manifest_json()?);   // the numbers
//! put(baked.pointer_key(), baked.pointer_json()?);    // which publish is current
//! ```
//!
//! The manifest is never the simulation listing record. They are separate
//! objects written by separate requests, because a manifest written over the
//! listing takes the Space out of the marketplace and nothing surfaces that
//! until somebody goes looking.
//!
//! ## Cost when nothing is authored
//!
//! No plugin, no systems, no resources: this module runs only when publish
//! calls it. A Space with no `Website` service, or with the scaffolded folder
//! and no References in it, returns `Ok(None)` after two content reads and
//! changes nothing about the publish.

pub mod bake;
pub mod key;
pub mod resolve;

pub use bake::{
    bake_from_world, bake_from_world_at, blake3_publish_hash, format_scalar,
    manifest_object_key, namespace_pointer_key, BakeError, BakedManifest, ManifestValue,
    NamespacePointer, PendingManifest, WebsiteManifest, WebsiteServiceConfig,
    MANIFEST_OBJECT_NAME, NAMESPACE_PREFIX, SERVICE_FOLDER,
};
pub use key::{
    fingerprint, KeyKind, KeyRecord, KeySet, KeyVerdict, MintedKey, BROWSER_PREFIX, BUILD_PREFIX,
    DEFAULT_BURST, DEFAULT_OVERLAP_DAYS, DEFAULT_RATE_LIMIT_PER_MINUTE,
};
pub use resolve::{
    did_you_mean, glob_matches, resolve_all, Datamodel, ErrorKind, Reference, ReferenceKind,
    ResolveError, Scalar, WorldDatamodel,
};
