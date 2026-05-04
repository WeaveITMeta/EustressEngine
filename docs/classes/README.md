# Class Documentation

Two active docs cover the Eustress class system; everything else in
this directory is a redirect stub kept for backlink stability.

## Active

| Doc | What it covers | Read when |
|---|---|---|
| [`CLASS_EXTENSIBILITY.md`](CLASS_EXTENSIBILITY.md) | Canonical guide to **adding a new class**. Three tiers (template-only / template + enum / template + enum + `ExtraSectionClaim`) with a complete worked example. | You want to ship a new class type. |
| [`../development/CLASS_CONVERSION.md`](../development/CLASS_CONVERSION.md) | The Studio conversion-tool's semantics — the matrix that decides which classes can be converted to which (`Part` → `Seat`, `Folder` → `Model`, etc.) and which TOML sections survive each conversion. | You want to change an *existing* instance's class, not add a new one to the registry. |

## Live registry

The actual class list isn't doc-maintained — it's whatever
`.defaults.toml` files live in
[`eustress/crates/common/assets/class_schema/`](../../eustress/crates/common/assets/class_schema/).
`common/build.rs` globs that directory at compile time and generates
`BUILTIN_TEMPLATES`. To answer "what classes ship today?" run:

```bash
ls eustress/crates/common/assets/class_schema/*.defaults.toml
```

## Reference

- [`eustress_common::class_schema`](../../eustress/crates/common/src/class_schema/mod.rs) —
  registry resource, `ExtraSectionClaim` trait, dispatcher system.
- [`eustress_common::classes::ClassName`](../../eustress/crates/common/src/classes.rs) —
  enum + per-class component definitions. Adding a Tier 2 class
  requires a variant here.
- [`instance_loader::spawn_instance`](../../eustress/crates/engine/src/space/instance_loader.rs) —
  where `PendingExtraSections` gets attached so plugin claims fire.

If you find a doc anywhere in the repo that contradicts
[`CLASS_EXTENSIBILITY.md`](CLASS_EXTENSIBILITY.md), treat it as stale
and prefer the canonical guide. The pre-2026 hardcoded-class-registry
docs (`ADDING_NEW_CLASSES.md`, `CLASSES_GUIDE.md`, `CLASSES_EXTENDED.md`,
`CLASS_SYSTEM_COMPLETE.md`, `README_ROBLOX_CLASSES.md`,
`QUICKSTART_CLASSES.md`) were deleted during the cleanup that produced
this index — they don't exist any more.
