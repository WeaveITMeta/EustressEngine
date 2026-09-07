# Website Service - Scripting API

**Class name:** `Website`
**Service folder:** `Website`
**Category:** Data
**Design spec:** [`docs/design/WEBSITE_SERVICE.md`](../design/WEBSITE_SERVICE.md)
**Consumer contract:** `Voltec/docs/WEBSITE_MANIFEST_API.md`
**Checklist row:** `docs/development/SCRIPTING_API_CHECKLIST.md` section 16

A Space owns its numbers. The Website service is where an author marks which of
those numbers a website is allowed to read. Each marked value is a `Reference`.
On publish every Reference resolves and the whole set bakes into one
`website-manifest.json` beside the `.pak`, and a website fetches that single
document to update every number it displays. One fetch, never one per value.

This document is the scripting reference for that service: the object model, the
properties, the functions, the events, and the `Reference` object itself. The
authoring workflow and the bake contract are in the design spec. The browser side
is in the consumer contract.

---

## 0. Status legend, and what exists today

Every symbol in this document carries one of three states.

| mark | meaning |
|---|---|
| ✅ **LIVE** | in the tree today. Callable or servable now. |
| 🔶 **PARTIAL** | present, and incomplete, stubbed, or contradicted by something else in the tree. |
| ⬜ **SPEC** | designed here and not in the tree. Calling it today fails. |

⬜ SPEC here maps to ❌ Not Started in `SCRIPTING_API_CHECKLIST.md`, which uses the
repository-wide legend. The two documents describe the same facts; this one adds
the signatures.

### 0.1 What is real as of 2026-08-26

Checked against the working tree, not against the plan. Section 11 gives the
commands that re-derive every row.

| piece | state |
|---|---|
| `Website` row in `SERVICE_FOLDERS` (`engine/src/space/space_ops.rs`) | 🔶 present, and it declares the wrong class. See 0.2. |
| `common/assets/service_templates/Website/_service.toml` | ✅ LIVE. This is the store. |
| `common/assets/service_properties/Website.toml` | ✅ LIVE. This is the panel schema. |
| Manifest routes on the `eustress-api` worker | ✅ LIVE in source. `infrastructure/cloudflare/api/src/index.js`. |
| `pub mod website;` in `engine/src/lib.rs` | 🔶 declared, and `engine/src/website/` does not exist, so the crate does not build. |
| `engine/src/website/` resolver and bake | ⬜ SPEC |
| `class_schema/Reference/_instance.toml` | ⬜ SPEC |
| `engine/assets/icons/website.svg` | ⬜ SPEC. `icon = "website"` currently resolves to nothing. |
| Every Rune `website_*` function | ⬜ SPEC |
| Luau `game:GetService("Website")` | ⬜ SPEC. Raises `Service 'Website' not found`. |
| EventBus topics `website.*` | ⬜ SPEC |

**No scripting call in this document runs today.** The service assets and the
serving side exist; the engine module between them does not, and neither runtime
has a binding. Everything in sections 1, 3, 4, 5 and 7 is ⬜ SPEC unless a row
says otherwise. Section 2 describes properties that ✅ LIVE on disk and are not yet
reachable from a script.

### 0.2 Two contradictions in the tree, stated rather than smoothed over

**The class name disagrees with itself.** `SERVICE_FOLDERS` declares
`class: "WebsiteService"`. The landed template declares `class_name = "Website"`
and explains why at length: the Explorer row is built from the disk folder name
while three lookups key off the class name, `build_service_properties` loads
`service_properties/<class_name>.toml`, and the landed panel file is
`Website.toml`. With `WebsiteService` the panel looks for a `WebsiteService.toml`
that does not exist and the Properties panel renders blank. Every service that
works today has folder equal to class.

This document uses `Website`, following the template. The fix belongs in
`space_ops.rs`, one word.

**The web documentation page quotes routes the worker does not serve.**
`eustress/crates/web/src/pages/docs_website.rs` shows
`GET /api/simulations/{id}/website-manifest` and
`GET /api/simulations/vcell/latest/website-manifest`. The worker serves
`GET /api/simulation/{id}/manifest` and `GET /api/simulation/{namespace}/latest/manifest`:
singular `simulation`, and the path ends in `/manifest`. A consumer following the
web page gets a 404. Section 4.9 documents what the worker actually does, quoted
from its source.

### 0.3 Names are a contract

Where this document and the tree disagree, the tree wins. Sources of truth, in
order:

| what | source of truth |
|---|---|
| store fields | `eustress/crates/common/assets/service_templates/Website/_service.toml` |
| panel rows | `eustress/crates/common/assets/service_properties/Website.toml` |
| serving behaviour | `infrastructure/cloudflare/api/src/index.js`, the `WEBSITE MANIFEST` section |
| manifest bytes | `docs/design/WEBSITE_SERVICE.md` section 4.1 |
| Rune functions | `eustress/crates/engine/src/soul/rune_ecs_module.rs` |
| Luau service table | `eustress/crates/common/src/luau/runtime.rs`, `inject_storage_services` |

---

## 1. Obtaining the service

The two script runtimes reach services in genuinely different ways, and pretending
otherwise produces code that compiles in one and fails in the other.

### 1.1 Rune: free functions, no service handle

⬜ SPEC

Rune has no `game` root and no `GetService`. Every namespace the Rune VM installs
is listed in `engine_rune_modules()` (`engine/src/soul/rune_api.rs`), and there are
four: `eustress`, `eustress::realism::<domain>`, `event_bus`, and an `std` io
override. Services in Rune are flat free functions carrying a service-name prefix,
which is how `datastore_service_get`, `tween_service_create`, and
`workspace_get_gravity` already work.

The Website service follows `workspace_*`, not `datastore_*`: a Space has exactly
one Website service, so there is nothing for an accessor to return that a prefix
does not already say.

```rune
use eustress::{website_namespace, website_references};

// There is no website_service() call. The prefix is the service.
let ns = website_namespace();
let refs = website_references();
```

The `use` line matters. `create_ecs_module()` builds the module with
`Module::with_crate("eustress")`, so every function lands at `::eustress::<name>`
and a script either imports it or writes the full path. The working plugin example
at `docs/examples/plugins/hello_rune.rune` opens with exactly that import.

All Rune functions in this document are declared under
`#[cfg(feature = "realism-scripting")]`, matching every other scripting binding in
`rune_ecs_module.rs`. A build without that feature has none of them.

### 1.2 Luau: `game:GetService("Website")`

⬜ SPEC

`game:GetService(name)` is ✅ LIVE, and its whole implementation is a table lookup:
`game:GetService("X")` returns `game.X` and raises `Service 'X' not found` when the
field is nil. A service is reachable from Luau if and only if something called
`game.set("X", table)` during injection.

Nothing sets `Website`, so this raises today:

```lua
local Website = game:GetService("Website")  -- Service 'Website' not found
```

When it lands it must be a live bridge to the Space's `Website` folder, not another
detached stub. The existing injected tables (`ReplicatedStorage`, `ServerStorage`,
`Lighting`, and the `Starter*` set) are literals built once at VM start with
hardcoded entity ids in the 100001 to 100007 range and no reconciliation against
the real datamodel: `Lighting.Brightness` is the constant `2.0` regardless of what
the Space says. A Website table built that way would report an empty reference list
in a Space holding forty references, which is worse than raising.

### 1.3 MCP and the engine bridge

⬜ SPEC

No MCP tool addresses the Website service. The tool that should exist is
`website_bake`, returning the bake report as JSON so an agent can publish and read
back what it published.

Note for whoever adds it: a new MCP tool must also be added to `capability_of()` in
the tools crate's `capability.rs`, or it is advertised in the tool list and then
refused at dispatch. That trap has already cost this repository seventeen silently
unreachable Workshop tools.

---

## 2. Service properties

The rows below are ✅ LIVE on disk in `service_properties/Website.toml`, backed by
the store fields in `service_templates/Website/_service.toml`. Reading and writing
them **from a script** is ⬜ SPEC.

Two mechanics from the landed files that a script author needs:

- **Sections and rows render alphabetically**, not in file order. The panel builder
  walks a `toml::map::Map` and `toml` 0.8 is pulled without `preserve_order`, so
  that map is a `BTreeMap`.
- **Reading honours an explicit `source`; writing ignores it** and re-derives the
  store key from the row name with a per-capital snake caser. An editable row must
  therefore be named so that `to_snake_case(RowName)` equals its store key, which is
  why `source` appears only on readonly rows.

### 2.1 `[Data]`

| Row | Store key | Type | Access | Default | Description |
|---|---|---|---|---|---|
| `Name` | - | string | read only | `"Website"` | Service instance name. |
| `ClassName` | `class_name` | string | read only | `"Website"` | Class discriminator. See 0.2. |

### 2.2 `[Publishing]`

| Row | Store key | Type | Access | Default | Description |
|---|---|---|---|---|---|
| `Namespace` | `namespace` | string | read write | `""` | The identifier the manifest publishes under, and the name a consumer writes in `data-eus="{namespace}:{key}"`. Publish fails while it is empty rather than publishing under a blank namespace. |
| `SchemaVersion` | `schema_version` | int | read write | `1` | Bump when the **meaning** of a key changes rather than its value, so a rename becomes a break a consumer opts into. Issuing or rotating a key never bumps it: transport is not meaning. |

**Namespace rules, enforced by the worker at upload time** and worth validating in
the panel so an author learns before a publish rather than during one:

- `^[a-z0-9][a-z0-9_-]{0,63}$`. One to sixty-four characters of lowercase letters,
  digits, hyphen, or underscore, starting with a letter or digit.
- **Must not be UUID-shaped.** The read route tells a simulation id from a
  namespace by shape, so a UUID-shaped namespace would be unreachable.
- **A namespace is claimed by the first author who publishes it.** A second author
  publishing the same namespace gets `409 Namespace already claimed`. Without that
  claim, any authenticated author could point their own Space at someone else's
  namespace and rewrite the numbers on that person's website.
- Renaming releases the old namespace, through a `website-ns-of:{sim_id}` reverse
  pointer, so an author can reuse it.
- The namespace index is Cloudflare KV, which reaches every edge in roughly a
  minute. A namespace claimed seconds ago can still 404 elsewhere in the world.
  Harmless against a 300 second `max-age`, and a publish UI must avoid presenting
  the namespace URL as instantly live.

### 2.3 `[Access]`

| Row | Store key | Type | Access | Default | Description |
|---|---|---|---|---|---|
| `AccessEnforced` | `access_enforced` | bool | read write | `false` | When on, the worker serves the manifest only to a request carrying a valid key. Issue a key first. |
| `KeyId` | `key_id` | string | read only | `""` | Short stable handle for the current key, for example `wk_3f8a12`. This is the name that appears in request and rate-limit logs, so it is what you use when talking about a consumer. Empty until you mint a key. |
| `KeyPreview` | `key_preview` | string | read only | `""` | First and last few characters of the current key, so you can eyeball-match the value your site is sending. |
| `KeyCreated` | `key_created_at` | string | read only | `""` | RFC 3339 UTC. Stamped once at the first mint and never again. |
| `KeyRotated` | `key_rotated_at` | string | read only | `""` | RFC 3339 UTC. Moves on every rotation. Keeping both distinguishes "issued a year ago, rotated yesterday" from "minted yesterday". |
| `PreviousKeyId` | `previous_key_id` | string | read only | `""` | The key rotation replaced. It keeps working for `KeyOverlapDays`. Empty when no rotation is in flight. |
| `KeyOverlapDays` | `key_overlap_days` | int | read write | `30` | How long the previous key keeps working after a rotation, in whole days. Set it to cover your slowest consumer's redeploy, so rotating is a migration instead of an outage. |
| `RotateKey` | `rotate_key` | bool | read write | `false` | An **action**, not a setting. Turning it on mints a new key, shows it once, and switches itself back off. |

Store-only, deliberately absent from the panel:

| Store key | Type | Default | Description |
|---|---|---|---|
| `build_key_id` | string | `""` | Identity of a second credential that CI holds and that never reaches a browser. It is the one of the two that behaves like a real secret. |
| `build_key_created_at` | string | `""` | RFC 3339 UTC. |

**The raw key is never written into the Space.** Only its identity is. The template
gives three reasons and each one is load-bearing:

1. `_service.toml` ships inside the published Universe. `package_universe_to_pak`
   tars the whole directory with no exclusion, and the `.pak` is served publicly
   from `/api/simulations/{id}/download`. A raw key written there is downloadable by
   anyone who never visits the site it was issued for.
2. Rotation is this key's single lever, and it works only while the current key
   lives in one place the author controls. A key copied into every published `.pak`
   snapshot has already reached an audience nobody can enumerate.
3. The engine already holds this line elsewhere: the Claude key dialog says it is
   saved to local app data and never inside a Space, and `class_schema/ExportTarget`
   stores the name of a credential rather than the credential.

So the raw key is shown once at mint time, registered with the worker, and kept in
local app data keyed by `key_id`. An author who loses it rotates. That is what every
key console does.

**`RotateKey` needs an interception the UI layer has yet to add.** The properties
grammar dispatches ten widget types and none of them is a button, so the action is
a bool the service property write path must intercept, following the pattern CAD
feature deletion already uses. Without that arm the generic path stores `true`, the
toggle latches on forever, and no key is ever minted. The row would look like it
worked. The landed `Website.toml` spells out the six steps that arm must perform.

`AccessEnforced` needs a smaller guard in the same place: refuse to set it true
while `KeyId` is empty, and say why. Enforcing against a key that was never issued
locks out the author along with every consumer.

### 2.4 What the key is, plainly

A key that a public website sends from browser JavaScript is visible to anyone who
opens developer tools. The manifest is served with
`Access-Control-Allow-Origin: *`, so any page may request it.

What the key genuinely buys:

- **Attribution.** Requests are grouped per `KeyId`, so you can see which consumer
  is calling.
- **Rate limiting.** A Cloudflare rule can key on the `X-Eustress-Key` header and
  throttle one noisy consumer without touching the rest.
- **Revocation.** Rotating cuts a consumer off on the next request, and
  `KeyOverlapDays` gives an honest consumer time to redeploy.

What it does not buy: confidentiality. Real confidentiality would need a
server-side proxy holding a secret the browser never sees, or short-lived signed
tokens minted per session. This is neither. Nothing belongs in a Reference that the
author would keep off a public page.

This supersedes `docs/design/WEBSITE_SERVICE.md` section 7, which states that the
manifest does not authenticate. With `AccessEnforced` on it does, in the narrow
sense above and in no wider sense. The `build_key` is the different case: CI holds
it, it never reaches a browser, and it behaves like a real secret.

### 2.5 Reading properties from a script

⬜ SPEC

```rune
use eustress::{
    website_namespace, website_schema_version,
    website_manifest_url, website_access_enforced,
    website_key_id, website_key_preview,
};

let ns       = website_namespace();        // -> String
let version  = website_schema_version();   // -> i64
let url      = website_manifest_url();     // -> String, empty before the first publish
let enforced = website_access_enforced();  // -> bool
let key_id   = website_key_id();           // -> String, "" until a key is minted
let preview  = website_key_preview();      // -> String, e.g. "eus_pk_7c1d...4f2a"
```

```lua
local Website = game:GetService("Website")
print(Website.Namespace, Website.SchemaVersion)
print(Website.KeyId, Website.AccessEnforced)
```

Luau exposes the rows as fields on the service table. Rune exposes them as
functions, because a Rune module has no property protocol on a namespace, only on
an `Any` type, and there is no service handle type here.

There is no `website_key()`. The raw key is not in the Space, so no script running
inside the Space can return it. `website_rotate_key` is the one call that ever sees
raw key material, and it returns it exactly once.

---

## 3. The `Reference` object

⬜ SPEC. No `class_schema/Reference/_instance.toml` exists yet.

A `Reference` is an instance with `class_name = "Reference"` living in the `Website`
service folder:

```toml
[properties]
name = "specific_energy"
class_name = "Reference"

[reference]
kind   = "instance"
source = "Workspace/V-Cell/V1/Core/Enclosure#material.custom.wh_per_kg"
label  = "Specific energy, pack level"
unit   = "Wh/kg"
format = "{:.0}"
basis  = "derived"
```

The manifest key defaults to the instance name. An explicit `key` in `[reference]`
overrides it.

> **Check the table survives a round trip before committing to this shape.** The
> Website service template records that the **service** loader runs each `[service]`
> value through `toml_to_property_value`, which accepts scalars and 3-or-4-float
> arrays and drops tables, and that the signed saver then rewrites the file from
> that map, so the first save of any service deletes a nested section outright.
> Lighting, Workspace, and SoulService have each already lost their `[properties]`
> blocks that way. The instance path is a different loader, and the failure mode is
> quiet enough that it is worth proving rather than assuming: write a `Reference`,
> save, reload, and confirm `[reference]` is still there. If it is not, the fields
> flatten to `reference_kind`, `reference_source`, and so on, and this section
> changes with them.

### 3.1 Common properties

Present on every kind.

| Property | Type | Access | Default | Description |
|---|---|---|---|---|
| `Key` | string | read write | instance name | Key this value appears under in `manifest.values`. Unique in the namespace. |
| `Kind` | string | read only | required | One of `instance`, `sim`, `count`, `measure`, `expr`. Fixed at creation; to change it, delete and re-add. |
| `Source` | string | read write | required | Interpretation depends on `Kind`. See 3.2. |
| `Label` | string | read write | `""` | Human sentence the site can render beside the number. Copied to the manifest. |
| `Unit` | string | read write | `""` | Unit string, copied to the manifest verbatim. Not parsed, not converted, except on `measure` where a unit conversion is the point. |
| `Format` | string | read write | `"{}"` | Rust format spec applied to the typed value to produce `display`. `"{:.0}"` gives `953`, `"{:.2}"` gives `953.00`. |
| `Basis` | string | read write | `"measured"` | One of `measured`, `derived`, `simulated`, `assumed`. Travels with the value so a site can render a simulated figure differently from a measured one without keeping its own list. |

`Basis` is required rather than optional on purpose. A figure without its basis is
not a figure, and the manifest is the last place that can be attached without a
person remembering to.

### 3.2 Properties by kind

**`kind = "instance"`** - one property on one instance.

| Property | Type | Notes |
|---|---|---|
| `Source` | string | `<path>#<section>.<field>`, path relative to the Space root. Dotted fields index into TOML tables. Resolves against the **live datamodel**, so a value the engine computed or reconciled is what bakes. |

**`kind = "sim"`** - a published simulation value.

| Property | Type | Access | Default | Notes |
|---|---|---|---|---|
| `Source` | string | read write | required | Simulation value key, for example `battery.capacity_retention`. |
| `RunLabel` | string | read write | `""` | **Required.** Empty fails the bake. A number lifted from whatever happened to be in memory is how a figure reaches a website with no way to reproduce it. |
| `At` | string | read write | `"final"` | `final`, `min`, `max`, `mean`, or `at_cycle:<n>`. |

**`kind = "count"`** - entities matching a path glob.

| Property | Type | Access | Default | Notes |
|---|---|---|---|---|
| `Source` | string | read write | required | Path glob, for example `Workspace/V-Cell/V1/Assembly/**`. |
| `Filter` | string | read write | `""` | Predicate over instance fields, for example `class_name = Part`. Empty counts every match. |

**`kind = "measure"`** - a geometric measure over a subtree.

| Property | Type | Access | Default | Notes |
|---|---|---|---|---|
| `Source` | string | read write | required | `<measure>:<path>`, for example `bbox:Workspace/V-Cell/V1/Assembly`. |
| `Axis` | string | read write | `"x"` | `x`, `y`, `z`, `volume`, or `surface`. |
| `Unit` | string | read write | `"m"` | The engine is meter-native. A `mm` unit converts on resolve; the manifest carries the converted number and the unit you set. |

Measures resolve against the same tree the viewport draws, which is what makes "the
drawing and the datasheet agree" a property of the system rather than something a
person checks.

**`kind = "expr"`** - an expression over other references.

| Property | Type | Notes |
|---|---|---|
| `Source` | string | Infix expression over other reference keys in the same namespace, for example `energy_wh / pack_mass_kg`. Evaluation is a directed acyclic graph. A cycle fails the bake and the message names the loop. |

### 3.3 Reference methods

In Rune these are free functions taking the handle first, matching the
`datastore_get(store, key)` idiom. That shape is also the only one the in-engine API
Browser can see: its parser matches the literal line `#[rune::function]` and skips
`#[rune::function(instance)]`, so instance methods never reach the catalog.

| Rune | Luau | Returns | Notes |
|---|---|---|---|
| `website_ref_set_label(r, label)` | `ref.Label = label` | `bool` | `false` when the reference no longer exists. |
| `website_ref_set_unit(r, unit)` | `ref.Unit = unit` | `bool` | |
| `website_ref_set_format(r, format)` | `ref.Format = format` | `bool` | `false` when the format spec is malformed. |
| `website_ref_set_basis(r, basis)` | `ref.Basis = basis` | `bool` | `false` on a value outside the four allowed. |
| `website_ref_set_source(r, source)` | `ref.Source = source` | `bool` | Not validated here. An unresolvable source surfaces at preview or bake, where the message can name candidates. |
| `website_ref_set_run_label(r, run_label)` | `ref.RunLabel = label` | `bool` | `false` unless `Kind` is `sim`. |
| `website_ref_set_at(r, at)` | `ref.At = at` | `bool` | `false` unless `Kind` is `sim`, or on an unparsable selector. |
| `website_ref_set_filter(r, filter)` | `ref.Filter = filter` | `bool` | `false` unless `Kind` is `count`. |
| `website_ref_set_axis(r, axis)` | `ref.Axis = axis` | `bool` | `false` unless `Kind` is `measure`. |
| `website_ref_resolve(r)` | `ref:Resolve()` | `ResolvedValue` | Resolves this one reference now. Section 4.3. |
| `website_ref_delete(r)` | `ref:Delete()` | `bool` | Removes the instance from the service and from disk. `false` when it was already gone. |

Getters in Rune are `#[rune(get)]` fields on the handle, read as `r.key`, `r.kind`,
`r.source`, `r.label`, `r.unit`, `r.format`, `r.basis`, `r.run_label`, `r.at`,
`r.filter`, `r.axis`. Fields that do not apply to the kind read as the empty string
rather than raising, so a loop over mixed kinds needs no guard.

### 3.4 Type registration notes

For whoever implements `ReferenceRune`, `ResolvedValueRune`, and `BakeReportRune`:

- every `#[derive(rune::Any)]` type **must** carry `#[rune(item = ::eustress)]`. A
  type registered into a namespaced module without it lands at the crate root, and
  every one of its constructors resolves to `Missing item ::eustress::T::new` while
  free functions in the same module keep working. The failure is silent, total, and
  looks like a working module.
- `module.ty::<T>()?` installs only the field protocol. Every `#[rune::function]` on
  the type is registered by hand or it fails when a script calls it.
- Rune needs a manual `impl rune::alloc::clone::TryClone`. `#[derive(Clone)]` alone
  is not enough.

---

## 4. Functions

⬜ SPEC for all of section 4.

### 4.0 Two conventions

**Imports.** The short Rune snippets below omit the `use eustress::{...};` header
for brevity. A real script needs it, as shown in sections 1.1, 4.1 and 7.

**Failure.** These functions do not raise. They return a status object or a
sentinel, and the reason lands in the log and in the returned object's `error`
field. That matches every other service binding in `rune_ecs_module.rs`, where a
missing service produces a `warn!` and a fallback rather than a VM error, and it is
what lets a bake loop report every failure instead of stopping at the first.

The hard rule inherited from the design spec: **a failed reference fails the bake.**
It does not emit `null` and it does not carry the previous value forward. The
failure this feature exists to prevent is a website confidently displaying a stale
number, and a bake that quietly degrades reintroduces it one layer down.

### 4.1 `website_add_reference`

Create a `Reference` in the service.

| | |
|---|---|
| **Rune** | `website_add_reference(key: &str, kind: &str, source: &str) -> Option<ReferenceRune>` |
| **Luau** | `Website:AddReference(key: string, kind: string, source: string) -> Reference?` |

| Parameter | Type | Description |
|---|---|---|
| `key` | string | Manifest key and instance name. Unique within the namespace. |
| `kind` | string | `instance`, `sim`, `count`, `measure`, or `expr`. |
| `source` | string | Kind-dependent, per section 3.2. |

**Returns** the new reference, or `None` when `key` is already taken, `key` is
empty, or `kind` is not one of the five. The source is not validated here; an
unresolvable source surfaces at preview or bake, where the message can name
candidates.

```rune
use eustress::{
    website_add_reference, website_ref_set_label, website_ref_set_unit,
    website_ref_set_format, website_ref_set_basis, log_error,
};

let r = website_add_reference(
    "specific_energy",
    "instance",
    "Workspace/V-Cell/V1/Core/Enclosure#material.custom.wh_per_kg",
);
match r {
    Some(r) => {
        website_ref_set_label(r, "Specific energy, pack level");
        website_ref_set_unit(r, "Wh/kg");
        website_ref_set_format(r, "{:.0}");
        website_ref_set_basis(r, "derived");
    }
    None => log_error("specific_energy already exists, or the kind is unknown"),
}
```

### 4.2 `website_remove_reference`

| | |
|---|---|
| **Rune** | `website_remove_reference(key: &str) -> bool` |
| **Luau** | `Website:RemoveReference(key: string) -> boolean` |

**Returns** `true` when a reference was removed, `false` when no reference had that
key. Removal deletes the instance folder. It does not touch an already-published
manifest, so a site still reading the removed key keeps seeing the last published
value until the next publish drops it. Removing a key is a schema change: bump
`SchemaVersion` so consumers opt in rather than silently losing an element.

```rune
if website_remove_reference("draft_estimate") {
    log_info("removed draft_estimate; bump SchemaVersion before publishing");
}
```

### 4.3 `website_resolve` - preview one

Resolve a single reference against the live datamodel right now, without baking or
publishing. This is what the Properties panel's preview column calls, and it is how
an author sees a wrong path before publishing instead of after.

| | |
|---|---|
| **Rune** | `website_resolve(key: &str) -> ResolvedValueRune` |
| **Luau** | `Website:Resolve(key: string) -> ResolvedValue` |

**Returns** a `ResolvedValue`, always. A missing key returns one with `ok = false`
and `error = "no reference with key '...'"`.

`ResolvedValue` fields, readable in both runtimes:

| Field | Type | Description |
|---|---|---|
| `key` | string | The reference key. |
| `ok` | bool | `false` when resolution failed. Every other field except `error` is then meaningless. |
| `is_number` | bool | `true` when the resolved value is numeric. |
| `value` | float | The typed value when `is_number`, otherwise `0.0`. |
| `text` | string | The typed value as text. The whole value when `is_number` is `false`. |
| `display` | string | `value` run through `Format`. This is what a page renders. |
| `unit` | string | Copied from the reference. |
| `label` | string | Copied from the reference. |
| `basis` | string | Copied from the reference. |
| `source` | string | Copied from the reference, so a failure message can quote it. |
| `error` | string | Empty when `ok`. Otherwise the reason, naming the source and the nearest candidates in the tree. |

Both `value` and `text` are always present because Rune's numeric types are `f64`
and `i64` while a manifest value may be a string, and `is_number` is cheaper for a
caller to branch on than an `Option`.

```rune
let v = website_resolve("specific_energy");
if v.ok {
    log_info(`${v.key} = ${v.display} ${v.unit} (${v.basis})`);
} else {
    log_error(`${v.key}: ${v.error}`);
}
```

### 4.4 `website_resolve_all` - preview everything

| | |
|---|---|
| **Rune** | `website_resolve_all() -> Vec<ResolvedValueRune>` |
| **Luau** | `Website:ResolveAll() -> {ResolvedValue}` |

**Returns** one `ResolvedValue` per reference, in dependency order so an `expr`
reference always follows its operands. Resolution continues past a failure, so one
call reports every broken reference rather than the first. A dependency cycle yields
an entry per member of the cycle, each with `ok = false` and an `error` naming the
loop.

This is the pre-flight check. Run it, see zero failures, then publish.

```rune
let failures = 0;
for v in website_resolve_all() {
    if !v.ok {
        failures += 1;
        log_error(`${v.key}: ${v.error}`);
    }
}
if failures == 0 {
    log_info("all references resolve; safe to publish");
}
```

### 4.5 `website_bake` - write the manifest

Resolve everything in dependency order and write `website-manifest.json` into the
Space's `.eustress/` directory. Bake does not upload. Publish calls bake after the
`.pak` is packaged and before upload, and aborts the upload when the bake fails.

| | |
|---|---|
| **Rune** | `website_bake() -> BakeReportRune` |
| **Luau** | `Website:Bake() -> BakeReport` |

`BakeReport` fields:

| Field | Type | Description |
|---|---|---|
| `ok` | bool | `true` only when every reference resolved. |
| `namespace` | string | Namespace baked. |
| `schema_version` | int | Schema version baked. |
| `value_count` | int | Number of entries in `values`. `0` when `ok` is `false`. |
| `publish_hash` | string | `sha256:...` over the manifest bytes. Empty when `ok` is `false`. |
| `baked_at` | string | RFC 3339. Empty when `ok` is `false`. |
| `manifest_path` | string | Local path written. Empty when `ok` is `false`. |
| `size_bytes` | int | Serialised size. The worker refuses an upload above 1048576 bytes, so a bake that exceeds it is a bake that cannot publish. |
| `failed` | list of string | One `"key: message"` line per failed reference. Empty when `ok`. |

**Conditions that fail a bake**

| condition | message shape |
|---|---|
| `Namespace` empty | `namespace is empty; set it in the Website service properties` |
| `Namespace` malformed or UUID-shaped | `namespace "8f3a-..." is UUID-shaped; the read route tells an id from a namespace by shape` |
| `SchemaVersion` below 1 | `schema_version must be a whole number of at least 1` |
| instance path not found | `specific_energy: no instance at Workspace/.../Enclosur (did you mean Enclosure?)` |
| field missing on the instance | `specific_energy: Enclosure has no material.custom.wh_per_kg` |
| `sim` reference with empty `RunLabel` | `cycles: sim references must name a run_label` |
| type mismatch | `cycles: expected a number, found "n/a"` |
| `expr` cycle | `energy_wh -> pack_mass_kg -> energy_wh is a cycle` |
| duplicate keys | `two references claim key 'specific_energy'` |
| over the size cap | `manifest is 1.2 MB; the worker refuses anything over 1 MB` |

```rune
let report = website_bake();
if report.ok {
    log_info(`baked ${report.value_count} values, hash ${report.publish_hash}`);
} else {
    for line in report.failed {
        log_error(line);
    }
}
```

**`publish_hash` must change whenever the values change.** The worker refuses an
upload that reuses a hash over different content with
`409 publish_hash unchanged but manifest content changed`, because every consumer
holding the old ETag would get a `304` forever and every pinned consumer would cache
the old state for a year. Hash the manifest body; do not hash the `.pak` and reuse
it.

**Do not put key material in the manifest.** The worker strips the top-level fields
`auth_key`, `key`, `key_hash`, `api_key`, `secret`, and `token` before storing, and
reports what it removed in `stripped_fields`. A reference legitimately named `key`
lives under `values` and is untouched. Anything stripped never reached a consumer,
and a bake that produces stripped fields is a bake with a bug in it.

### 4.6 `website_last_bake`

| | |
|---|---|
| **Rune** | `website_last_bake() -> BakeReportRune` |
| **Luau** | `Website:GetLastBake() -> BakeReport` |

**Returns** the report from the most recent bake in this session, or a report with
`ok = false` and an empty `failed` list when nothing has baked yet.

This exists because Rune has no way to subscribe to an event (section 5). A Rune
script that wants to act after a bake polls this instead of connecting a handler.

### 4.7 `website_rotate_key`

Mint a new key, keep the outgoing one working for an overlap window, and return the
new raw key **once**.

| | |
|---|---|
| **Rune** | `website_rotate_key(overlap_days: i64) -> String` |
| **Luau** | `Website:RotateKey(overlapDays: number?) -> string` |

| Parameter | Type | Description |
|---|---|---|
| `overlap_days` | int | How long the outgoing key keeps working, in whole days. `0` cuts it immediately. Clamped to 365. Pass `-1`, or omit in Luau, to use the `KeyOverlapDays` property. |

**Returns** the new raw key, or the empty string when rotation failed.

This is the only call in the entire API that ever produces raw key material, and it
produces it exactly once. The value is not stored in the Space (section 2.3), so a
caller that discards the return value has to rotate again to get another. Treat it
the way the reveal dialog does: hand it to the caller, and do not log it.

What rotation writes:

| store field | after |
|---|---|
| `key_id` | the new handle, for example `wk_9c1f04` |
| `key_preview` | first and last characters of the new key |
| `key_created_at` | unchanged after the first mint |
| `key_rotated_at` | now, RFC 3339 UTC |
| `previous_key_id` | the outgoing handle |
| `key_overlap_days` | unchanged unless `overlap_days` was passed |

Then it fires `website.key_rotated`.

**The new key reaches consumers on the next publish**, because the key travels to
the worker with the manifest upload. Rotate, publish, then redeploy the site inside
the overlap window.

An overlap of `0` is right for exactly one situation: a key you believe is being
abused, where breaking an honest consumer for a few minutes is cheaper than letting
the abuse continue.

```rune
let key = website_rotate_key(-1);        // use the KeyOverlapDays property
if key == "" {
    log_error("rotation refused; see the log for why");
} else {
    // Hand it to the caller. Do not log it: logs outlive the reveal dialog.
    log_info(`minted ${website_key_id()}; the previous key works for ${website_key_overlap_days()} more days`);
    log_info("publish now so the new key reaches the served manifest");
}
```

Related accessors: `website_key_id()`, `website_key_preview()`,
`website_key_created()`, `website_key_rotated()`, `website_previous_key_id()`,
`website_key_overlap_days()`, `website_set_key_overlap_days(days)`.

### 4.8 `website_access_enforced` and `website_set_access_enforced`

| | |
|---|---|
| **Rune** | `website_access_enforced() -> bool` |
| **Rune** | `website_set_access_enforced(enforced: bool) -> bool` |
| **Luau** | `Website.AccessEnforced` |

**Returns** the value now in effect. `website_set_access_enforced(true)` returns
`false` and refuses when `KeyId` is empty, because enforcing against a key that was
never issued locks out the author along with every consumer.

How the flag reaches the worker. The upload accepts three headers and the default
is the safe one:

| header | effect |
|---|---|
| `X-Eustress-Key` | raw key, hashed at the edge and discarded |
| `X-Eustress-Key-Hash` | `sha256:` plus 64 lowercase hex, for a publisher that prefers the raw key never leave the author's machine |
| `X-Eustress-Key-Clear: true` | the only way to make a keyed manifest open again |
| none of the three | the existing key is **preserved**, so a routine republish cannot silently un-protect a manifest |

Since the Space stores identity rather than key material, the engine sends
`X-Eustress-Key-Hash` from local app data on a publish where `AccessEnforced` is on,
`X-Eustress-Key-Clear: true` where it is off and a key was previously set, and
nothing at all otherwise. A raw key under 16 characters is refused with `400`,
because a guessable key defeats revocation as surely as no key at all.

The upload response reports `key_required`, `key_rotated`, and `key_cleared`, so
"I set a key" and "this manifest is open" can never be confused for each other.

### 4.9 `website_manifest_url`

| | |
|---|---|
| **Rune** | `website_manifest_url() -> String` |
| **Rune, pinned** | `website_manifest_url_pinned(publish_hash: &str) -> String` |
| **Rune, by id** | `website_manifest_url_by_id() -> String` |
| **Luau** | `Website:GetManifestUrl(publishHash: string?) -> string` |

**Returns** the public URL of the baked manifest, or the empty string before the
first successful publish, because the namespace is not claimed until then.

#### The routes the worker actually serves

✅ LIVE in `infrastructure/cloudflare/api/src/index.js`.

```
GET  https://api.eustress.dev/api/simulation/{namespace}/latest/manifest
GET  https://api.eustress.dev/api/simulation/{namespace}/manifest
GET  https://api.eustress.dev/api/simulation/{simulation_id}/manifest
GET  .../manifest?v={publish_hash}
PUT  https://api.eustress.dev/api/simulations/{simulation_id}/website-manifest
```

Read routes are on the **singular** `/api/simulation/` prefix and always end in
`/manifest`. That is deliberate: the segment may be a namespace rather than a UUID,
and a namespace spelled in hex, `decade` or `beef`, would otherwise be swallowed by
the `/api/simulations/[a-f0-9-]+` route. The `/latest` form is the documented one; a
bare namespace resolves identically, because a consumer who drops `/latest` should
get their manifest rather than a 404 they cannot explain.

The upload route is the **plural** `/api/simulations/` prefix, authenticated and
owner-checked like every other upload, and it writes one R2 object plus the
namespace index.

Storage: `universes/{simulation_id}/website-manifest.json` in the
`eustress-simulations` bucket, bound as `SCENES` on the `eustress-api` worker.

**This is a separate object from the simulation listing record**, which lives in KV
under `sim:{id}` and drives the marketplace. They share a bucket and a prefix; they
are not the same document, and writing one must never touch the other, because
overwriting the listing takes the Space out of the gallery.

#### Response headers

| header | value |
|---|---|
| `ETag` | `"{publish_hash}"` from R2 `customMetadata` |
| `Cache-Control`, unpinned | `public, max-age=300, stale-while-revalidate=86400` |
| `Cache-Control`, pinned | `public, max-age=31536000, immutable` |
| `Vary` | `X-Eustress-Key` |
| `Access-Control-Allow-Origin` | `*` |
| `Access-Control-Allow-Methods` | `GET, OPTIONS` |
| `Access-Control-Allow-Headers` | `Content-Type, If-None-Match, X-Eustress-Key` |
| `Access-Control-Expose-Headers` | `ETag` |
| `Access-Control-Max-Age` | `86400` |
| `X-Content-Type-Options` | `nosniff` |
| `Referrer-Policy` | `strict-origin-when-cross-origin` |

`Vary: X-Eustress-Key` is there because `public` plus authentication by request
header is a cache-poisoning shape unless the cache keys on that header. Without it a
shared cache could hand a keyed `200` to a keyless caller and the key would be
decorative.

`Access-Control-Expose-Headers: ETag` is there because cross-origin JavaScript
cannot read `ETag` otherwise. The browser revalidates on its own; a build-time baker
wants the hash it just saw.

The key gate runs before anything is served, a `304` included, so a caller without
the key cannot poll the ETag to learn when a publish happened.

#### Error responses

Every one carries `Cache-Control: no-store`, so a rejection never sticks in a shared
cache and a rotated key takes effect on the next request.

| status | `error` code | when |
|---|---|---|
| `400` | `bad_namespace` | the segment is not a UUID and not a valid namespace |
| `401` | `auth_key_required` | the manifest has a key and the request presented none |
| `401` | `auth_key_invalid` | the presented key does not match |
| `404` | `namespace_not_found` | no Space publishes under that namespace |
| `404` | `manifest_not_found` | that Space has never published a website manifest |
| `404` | `pinned_hash_not_available` | `?v=` names a state this manifest does not currently hold |
| `500` | `namespace_index_corrupt` | the KV index entry could not be parsed. Republish to rewrite it. |

Upload failures: `409 Namespace already claimed`, `409 publish_hash unchanged but
manifest content changed`, `413` over 1 MB, `400` for a missing `namespace`,
`values`, or `publish_hash`, `403` when the simulation belongs to another author.

**A stale pin is a `404`, not a redirect.** A pinned URL is immutable by contract,
so serving a different state under that promise is worse than refusing: the consumer
pinned precisely because it must not drift. Drop `?v=` to follow the current
manifest, or pin again to the current hash. Both sides normalise the `sha256:`
prefix, so pinning with or without it works.

> **One open point, stated rather than hidden.**
>
> **The simulation id is not persisted.** `execute_publish_upload`
> (`engine/src/ui/file_event_handler.rs`) receives the id from
> `POST /api/simulations/publish` and puts it only into a progress string. Nothing
> writes `sync.toml` `[remote] experience_id`, which is the field three call sites
> read. Two consequences: each full publish creates a fresh listing id, and the
> incremental Space publish path fails with "Universe not published yet" every time.
>
> The manifest **read** URL survives this, because the namespace form needs no id.
> The manifest **upload** does not: `PUT /api/simulations/{id}/website-manifest`
> needs the id, and a new id per publish means the namespace claim moves to a new
> Space record on every publish while the old R2 object is orphaned. Persisting that
> id is a prerequisite for this feature behaving.

### 4.10 Enumeration

| Rune | Luau | Returns |
|---|---|---|
| `website_reference(key)` | `Website:GetReference(key)` | `Option<ReferenceRune>` |
| `website_references()` | `Website:GetReferences()` | `Vec<ReferenceRune>`, sorted by key |
| `website_reference_count()` | `Website.ReferenceCount` | `i64` |

---

## 5. Events

⬜ SPEC for all four.

### 5.1 The honest cross-runtime position

The one real event mechanism both runtimes can see is the EventBus
(`common/src/events.rs`, Rune namespace `event_bus`). Website bake fires topics
there.

- **Luau can subscribe.** `create_signal` in the Luau runtime is a genuine
  implementation with `Connect`, `Once`, and a connection carrying `Disconnect` and
  `Connected`. The Website service table exposes signals built with it.
- **Rune cannot subscribe.** The `event_bus` Rune module binds `fire`,
  `fire_number`, `fire_bool`, `event_names`, and `connection_count`. There is no
  `connect` binding, even though `EventBus::connect` and `once` exist and are tested
  on the Rust side. A Rune script can fire a topic and count its listeners, and
  cannot receive one.

A stub `Connect` that accepts a callback and drops it is not an acceptable
substitute. This repository already carries two of those and they are a known trap.
Until the EventBus gains a Rune `connect` binding, a Rune script polls
`website_last_bake()`.

**Payloads are JSON strings.** `event_bus::fire` takes a `String`, and `SignalArg`
carries only `String`, `Number`, and `Bool`. There is no table payload, so each
payload below is one JSON document serialised into the string argument.

### 5.2 `website.reference_failed`

Fires once per reference that fails to resolve, during `website_resolve_all` and
during a bake. Several fire in one bake when several references are broken.

```json
{
  "key": "specific_energy",
  "kind": "instance",
  "source": "Workspace/V-Cell/V1/Core/Enclosur#material.custom.wh_per_kg",
  "error": "no instance at Workspace/V-Cell/V1/Core/Enclosur (did you mean Enclosure?)",
  "during": "bake"
}
```

`during` is `preview` or `bake`, so a handler can stay quiet during authoring and
speak up during a publish.

Luau signal: `Website.ReferenceFailed`.

### 5.3 `website.baked`

Fires once, after a successful bake writes the file and before publish uploads it.

```json
{
  "namespace": "vcell",
  "schema_version": 3,
  "value_count": 25,
  "publish_hash": "sha256:1c9fa83...",
  "baked_at": "2026-08-26T07:41:00Z",
  "manifest_path": ".eustress/website-manifest.json",
  "size_bytes": 4218
}
```

Luau signal: `Website.BakeCompleted`.

### 5.4 `website.bake_failed`

Fires once, after a bake fails. Publish aborts before upload, so a handler seeing
this knows nothing was uploaded.

```json
{
  "namespace": "vcell",
  "failed_count": 2,
  "first_error": "specific_energy: no instance at Workspace/V-Cell/V1/Core/Enclosur (did you mean Enclosure?)",
  "failed": [
    "specific_energy: no instance at Workspace/V-Cell/V1/Core/Enclosur (did you mean Enclosure?)",
    "cycles_at_design_rate: sim references must name a run_label"
  ]
}
```

Luau signal: `Website.BakeFailed`.

### 5.5 `website.key_rotated`

Fires on a successful `website_rotate_key`. **The payload carries no key material.**
It names handles, which is what `key_id` exists for. A handler that needs the raw
key takes it from the return value of the rotate call, which is the only place it
appears.

```json
{
  "namespace": "vcell",
  "key_id": "wk_9c1f04",
  "previous_key_id": "wk_3f8a12",
  "overlap_days": 30,
  "rotated_at": "2026-08-26T07:41:00Z",
  "requires_publish": true
}
```

`requires_publish` is always `true` and is present so a handler reads it rather than
remembering it: the rotation reaches consumers on the next publish, not on rotation.

Luau signal: `Website.KeyRotated`.

### 5.6 Subscribing

```lua
-- Luau: real subscription
local Website = game:GetService("Website")

local conn = Website.BakeCompleted:Connect(function(payloadJson)
    local payload = HttpService:JSONDecode(payloadJson)
    print("baked", payload.value_count, "values as", payload.publish_hash)
end)

-- later
conn:Disconnect()
```

`HttpService` is reached as a global there on purpose. It is injected with
`globals.set` and never with `game.set`, so `game:GetService("HttpService")` raises
`Service 'HttpService' not found` while the bare global works. ✅ LIVE, and easy to
break by tidying.

Two caveats about the shared Luau signal helper, both ✅ LIVE and both worth knowing
before you rely on them:

- `Signal:Wait()` is a stub. It returns `0.0` immediately instead of yielding, so a
  script that waits on a bake spins.
- `Signal:Once()` registers a wrapper that calls the callback and never removes
  itself, so it behaves like `Connect` and fires every time. Track the connection and
  disconnect it yourself.

```rune
// Rune: introspection only. This does not subscribe.
let topics = event_bus::event_names();
let listeners = event_bus::connection_count("website.baked");

// The Rune path to "did it bake" is a poll:
let report = website_last_bake();
if report.ok {
    log_info(`last bake: ${report.value_count} values`);
}
```

---

## 6. What a bake writes

Repeated here so a script author reading only this page knows what the fields on
`ResolvedValue` become. The authoritative version is design spec section 4.1 and the
consumer contract.

```json
{
  "namespace": "vcell",
  "schema_version": 3,
  "space_id": "8f3a...",
  "publish_hash": "sha256:1c9fa83...",
  "baked_at": "2026-08-26T07:41:00Z",
  "engine_version": "0.1.0",
  "values": {
    "specific_energy": {
      "value": 953,
      "unit": "Wh/kg",
      "label": "Specific energy, pack level",
      "basis": "derived",
      "format": "{:.0}",
      "display": "953",
      "source": "Workspace/V-Cell/V1/Core/Enclosure#material.custom.wh_per_kg"
    },
    "cycles_at_design_rate": {
      "value": 237,
      "unit": "cycles",
      "label": "Cycles to 80 % retention, 0.25C charge",
      "basis": "simulated",
      "display": "237",
      "source": "battery.capacity_retention",
      "run_label": "I_life_25C_5MPa_res0"
    }
  }
}
```

Both `value` and `display` are emitted so a consumer never reimplements the author's
formatting and never parses a formatted string back into a number.

The worker requires `namespace`, `values`, and `publish_hash`, and rejects the
upload without them. `schema_version` is optional to the worker and required in
practice, because a consumer that cannot pin a schema version has no way to opt into
a rename.

---

## 7. Worked example, end to end

⬜ SPEC. Nothing in 7.1 through 7.3 runs today. 7.4 runs against a manifest that a
working engine has published.

### 7.1 Author the references

`Website/setup.rune`, run once from the command bar or the script editor.

```rune
use eustress::{
    website_add_reference,
    website_ref_set_label, website_ref_set_unit, website_ref_set_format,
    website_ref_set_basis, website_ref_set_run_label, website_ref_set_at,
    website_ref_set_filter,
    log_error,
};

pub fn main() {
    // Marking a value the datasheet quotes.
    let energy = website_add_reference(
        "specific_energy",
        "instance",
        "Workspace/V-Cell/V1/Core/Enclosure#material.custom.wh_per_kg",
    );
    match energy {
        Some(r) => {
            website_ref_set_label(r, "Specific energy, pack level");
            website_ref_set_unit(r, "Wh/kg");
            website_ref_set_format(r, "{:.0}");
            website_ref_set_basis(r, "derived");
        }
        None => log_error("specific_energy already exists"),
    }

    // A simulated figure must name the run it came from, or the bake fails.
    let cycles = website_add_reference(
        "cycles_at_design_rate",
        "sim",
        "battery.capacity_retention",
    );
    match cycles {
        Some(r) => {
            website_ref_set_run_label(r, "I_life_25C_5MPa_res0");
            website_ref_set_at(r, "final");
            website_ref_set_label(r, "Cycles to 80 % retention, 0.25C charge");
            website_ref_set_unit(r, "cycles");
            website_ref_set_format(r, "{:.0}");
            website_ref_set_basis(r, "simulated");
        }
        None => log_error("cycles_at_design_rate already exists"),
    }

    // The site quotes a part count, which is a tree query rather than a scalar.
    let parts = website_add_reference(
        "part_count",
        "count",
        "Workspace/V-Cell/V1/Assembly/**",
    );
    match parts {
        Some(r) => {
            website_ref_set_filter(r, "class_name = Part");
            website_ref_set_label(r, "Parts in the assembly");
            website_ref_set_format(r, "{}");
            website_ref_set_basis(r, "measured");
        }
        None => log_error("part_count already exists"),
    }
}
```

### 7.2 Preview before publishing

```rune
use eustress::{website_resolve_all, log_info, log_error};

pub fn main() {
    let broken = 0;
    for v in website_resolve_all() {
        if v.ok {
            log_info(`${v.key} = ${v.display} ${v.unit} [${v.basis}]`);
        } else {
            broken += 1;
            log_error(`${v.key}: ${v.error}`);
        }
    }
    if broken > 0 {
        log_error(`${broken} references will fail the publish; fix them first`);
    } else {
        log_info("clean; publish will bake");
    }
}
```

Expected output on a healthy Space:

```
specific_energy = 953 Wh/kg [derived]
cycles_at_design_rate = 237 cycles [simulated]
part_count = 2341  [measured]
clean; publish will bake
```

### 7.3 Publish

Publish from the File menu, or bake locally first to see the report:

```rune
use eustress::{website_bake, website_manifest_url, log_info, log_error};

pub fn main() {
    let report = website_bake();
    if !report.ok {
        for line in report.failed {
            log_error(line);
        }
        return;
    }
    log_info(`${report.value_count} values, ${report.publish_hash}`);
    log_info(website_manifest_url());
}
```

Publish runs the same bake, refuses to upload when it fails, and on success PUTs the
manifest to `/api/simulations/{id}/website-manifest`, which writes
`universes/{id}/website-manifest.json` and claims the namespace.

### 7.4 Consume

The page ships with the current values already in the HTML. The manifest corrects
them; it does not supply them. A page that renders empty until a fetch resolves is a
page that renders empty when the fetch fails, and a numeric specification that
flashes blank reads as broken to exactly the audience it exists to convince.

```html
<span data-eus="vcell:specific_energy">953</span>
<span data-eus="vcell:specific_energy" data-eus-field="unit">Wh/kg</span>
<span data-eus="vcell:cycles_at_design_rate">237</span>
<span data-eus="vcell:part_count">2341</span>
```

```js
const MANIFEST = 'https://api.eustress.dev/api/simulation/vcell/latest/manifest';
const KEY = 'eus_pk_9c1f04a7d2b84e6fa5c30918be27d4af';  // published, not secret

async function hydrate() {
  let m;
  try {
    const res = await fetch(MANIFEST, {
      cache: 'default',
      headers: { 'X-Eustress-Key': KEY },
    });
    if (!res.ok) return;                 // keep the baked values
    m = await res.json();
  } catch { return; }                    // offline: keep the baked values

  if (m.schema_version !== 3) {          // a rename is opt-in, not automatic
    console.warn('manifest schema', m.schema_version, 'expected 3');
    return;
  }

  for (const el of document.querySelectorAll('[data-eus]')) {
    const [ns, key] = el.dataset.eus.split(':');
    if (ns !== m.namespace) continue;
    const v = m.values[key];
    if (!v) { console.warn('no manifest key', key); continue; }
    const field = el.dataset.eusField || 'display';
    if (v[field] !== undefined) el.textContent = v[field];
  }
  document.documentElement.dataset.eusHash = m.publish_hash;
}
hydrate();
```

Twenty-five values cost one request. Adding a twenty-sixth costs nothing.

**Cost of sending the key.** `X-Eustress-Key` is not a CORS-safelisted header, so a
cross-origin request carrying it triggers an `OPTIONS` preflight: two round trips
instead of one on the first request. The worker short-circuits that preflight before
route dispatch and answers with `Access-Control-Allow-Headers: X-Eustress-Key` and
`Access-Control-Max-Age: 86400`, so the cost lands once a day per browser rather
than once per fetch.

**The query form exists and costs something real.** `?key=` is accepted for
consumers that cannot set headers, such as a no-code embed or a CMS URL field. A key
in a query string lands in access logs, `Referer` headers, and browser history. It
is accepted because the key is not a secret, not because query strings are a safe
place for secrets. Prefer the header.

With no key set on the manifest, the gate does not run at all: drop the header and
there is no preflight either.

**Do not add a cache-busting query string on every load.** It defeats the
revalidation path and turns a free `304` into a full download on every visit.
Publishing changes `publish_hash`, so the next conditional request already returns
`200`.

---

## 8. Failure behaviour a consumer must implement

From the consumer contract, restated because a script author is often the same
person who writes the page.

| condition | behaviour |
|---|---|
| network unreachable | keep the baked values, no visible change |
| response not ok | keep the baked values, log |
| `schema_version` mismatch | keep the baked values, log, apply nothing partially |
| `401 auth_key_required` | keep the baked values, log; the manifest now has a key this deploy does not send |
| `401 auth_key_invalid` | keep the baked values, log; the key was rotated and this deploy is stale |
| `404 namespace_not_found` | keep the baked values, log; the namespace was renamed or never claimed |
| `404 pinned_hash_not_available` | keep the baked values, log; the pin is stale. Drop `?v=` or re-pin. |
| key missing from the manifest | leave that element, log the key |
| value present and `null` | should be impossible; a failed reference fails the bake |

One rule underneath all of them: **the page is already correct before the fetch, and
the fetch can only improve it.** Any other arrangement turns a network problem into a
credibility problem.

---

## 9. What this service does not do

- **It does not push.** A website learns about a change on its next fetch. With a
  five minute `max-age` that is the update latency, and raising it is a consumer
  decision.
- **It does not version values.** The manifest is current state. History lives in the
  Space's git and in the run records. A consumer wanting a time series should be
  reading telemetry.
- **It does not make the manifest private.** See section 2.4. The key gives
  attribution, rate limiting, and revocation. The `build_key` is the separate case
  that behaves like a real secret, because CI holds it and it never reaches a
  browser.
- **It does not reach published Claude artifacts.** Those run under a strict content
  security policy that blocks external hosts, so a paper published that way cannot
  fetch a manifest. Regenerate such papers on publish and stamp `publish_hash` and
  `baked_at` in the footer, so a reader can tell which state the paper describes.

---

## 10. Symbol index

Status as of 2026-08-26.

### Rune, namespace `eustress`

| Symbol | Signature | Status |
|---|---|---|
| `website_namespace` | `() -> String` | ⬜ SPEC |
| `website_schema_version` | `() -> i64` | ⬜ SPEC |
| `website_manifest_url` | `() -> String` | ⬜ SPEC |
| `website_manifest_url_by_id` | `() -> String` | ⬜ SPEC |
| `website_manifest_url_pinned` | `(publish_hash: &str) -> String` | ⬜ SPEC |
| `website_key_id` | `() -> String` | ⬜ SPEC |
| `website_key_preview` | `() -> String` | ⬜ SPEC |
| `website_key_created` | `() -> String` | ⬜ SPEC |
| `website_key_rotated` | `() -> String` | ⬜ SPEC |
| `website_previous_key_id` | `() -> String` | ⬜ SPEC |
| `website_key_overlap_days` | `() -> i64` | ⬜ SPEC |
| `website_set_key_overlap_days` | `(days: i64) -> i64` | ⬜ SPEC |
| `website_rotate_key` | `(overlap_days: i64) -> String` | ⬜ SPEC |
| `website_access_enforced` | `() -> bool` | ⬜ SPEC |
| `website_set_access_enforced` | `(enforced: bool) -> bool` | ⬜ SPEC |
| `website_add_reference` | `(key, kind, source) -> Option<ReferenceRune>` | ⬜ SPEC |
| `website_remove_reference` | `(key: &str) -> bool` | ⬜ SPEC |
| `website_reference` | `(key: &str) -> Option<ReferenceRune>` | ⬜ SPEC |
| `website_references` | `() -> Vec<ReferenceRune>` | ⬜ SPEC |
| `website_reference_count` | `() -> i64` | ⬜ SPEC |
| `website_resolve` | `(key: &str) -> ResolvedValueRune` | ⬜ SPEC |
| `website_resolve_all` | `() -> Vec<ResolvedValueRune>` | ⬜ SPEC |
| `website_bake` | `() -> BakeReportRune` | ⬜ SPEC |
| `website_last_bake` | `() -> BakeReportRune` | ⬜ SPEC |
| `website_ref_set_label` | `(r: &ReferenceRune, label: &str) -> bool` | ⬜ SPEC |
| `website_ref_set_unit` | `(r: &ReferenceRune, unit: &str) -> bool` | ⬜ SPEC |
| `website_ref_set_format` | `(r: &ReferenceRune, format: &str) -> bool` | ⬜ SPEC |
| `website_ref_set_basis` | `(r: &ReferenceRune, basis: &str) -> bool` | ⬜ SPEC |
| `website_ref_set_source` | `(r: &ReferenceRune, source: &str) -> bool` | ⬜ SPEC |
| `website_ref_set_run_label` | `(r: &ReferenceRune, run_label: &str) -> bool` | ⬜ SPEC |
| `website_ref_set_at` | `(r: &ReferenceRune, at: &str) -> bool` | ⬜ SPEC |
| `website_ref_set_filter` | `(r: &ReferenceRune, filter: &str) -> bool` | ⬜ SPEC |
| `website_ref_set_axis` | `(r: &ReferenceRune, axis: &str) -> bool` | ⬜ SPEC |
| `website_ref_resolve` | `(r: &ReferenceRune) -> ResolvedValueRune` | ⬜ SPEC |
| `website_ref_delete` | `(r: &ReferenceRune) -> bool` | ⬜ SPEC |

### Rune types

| Type | Fields | Status |
|---|---|---|
| `ReferenceRune` | `key`, `kind`, `source`, `label`, `unit`, `format`, `basis`, `run_label`, `at`, `filter`, `axis` | ⬜ SPEC |
| `ResolvedValueRune` | `key`, `ok`, `is_number`, `value`, `text`, `display`, `unit`, `label`, `basis`, `source`, `error` | ⬜ SPEC |
| `BakeReportRune` | `ok`, `namespace`, `schema_version`, `value_count`, `publish_hash`, `baked_at`, `manifest_path`, `size_bytes`, `failed` | ⬜ SPEC |

### Luau

| Symbol | Status |
|---|---|
| `game:GetService` | ✅ LIVE (table lookup on `game`) |
| `game:GetService("Website")` | ⬜ SPEC. Raises `Service 'Website' not found`. |
| `Website.Namespace`, `.SchemaVersion`, `.ReferenceCount` | ⬜ SPEC |
| `Website.KeyId`, `.KeyPreview`, `.KeyCreated`, `.KeyRotated`, `.PreviousKeyId`, `.KeyOverlapDays`, `.AccessEnforced` | ⬜ SPEC |
| `Website:AddReference(key, kind, source)` | ⬜ SPEC |
| `Website:RemoveReference(key)` | ⬜ SPEC |
| `Website:GetReference(key)` / `:GetReferences()` | ⬜ SPEC |
| `Website:Resolve(key)` / `:ResolveAll()` | ⬜ SPEC |
| `Website:Bake()` / `:GetLastBake()` | ⬜ SPEC |
| `Website:GetManifestUrl(publishHash?)` | ⬜ SPEC |
| `Website:RotateKey(overlapDays?)` | ⬜ SPEC |
| `Reference` fields `.Key .Kind .Source .Label .Unit .Format .Basis .RunLabel .At .Filter .Axis` | ⬜ SPEC |
| `ref:Resolve()`, `ref:Delete()` | ⬜ SPEC |
| `Website.ReferenceFailed`, `.BakeCompleted`, `.BakeFailed`, `.KeyRotated` | ⬜ SPEC |

### EventBus topics

| Topic | Status |
|---|---|
| `website.reference_failed` | ⬜ SPEC |
| `website.baked` | ⬜ SPEC |
| `website.bake_failed` | ⬜ SPEC |
| `website.key_rotated` | ⬜ SPEC |
| `event_bus::fire`, `fire_number`, `fire_bool`, `event_names`, `connection_count` | ✅ LIVE |
| `event_bus` Rune subscribe | ❌ absent. `EventBus::connect` exists in Rust and has no Rune binding. |

### Service properties, store fields, and routes

| Item | Status |
|---|---|
| `service_properties/Website.toml` rows `[Data] [Publishing] [Access] [Description]` | ✅ LIVE on disk |
| `service_templates/Website/_service.toml` store fields | ✅ LIVE on disk |
| `RotateKey` write-path interception in `slint_ui.rs` | ⬜ SPEC. Without it the toggle latches and mints nothing. |
| `AccessEnforced` guard against an empty `KeyId` | ⬜ SPEC |
| `GET /api/simulation/{id\|namespace}[/latest]/manifest` | ✅ LIVE in worker source |
| `PUT /api/simulations/{id}/website-manifest` | ✅ LIVE in worker source |
| `Website` row in `SERVICE_FOLDERS` | 🔶 present, declares `WebsiteService`, should be `Website` |
| `engine/src/website/` resolver and bake | ⬜ SPEC |
| `class_schema/Reference/_instance.toml` | ⬜ SPEC |
| `engine/assets/icons/website.svg` | ⬜ SPEC |
| MCP tool `website_bake` | ⬜ SPEC |

---

## 11. Verifying this document

Each status above is derived from a command, so a reader can re-derive it instead of
trusting the date at the top. Run these from the repository root.

```bash
# Rune bindings: no output means every website_* symbol is still SPEC.
grep -n "website_" eustress/crates/engine/src/soul/rune_ecs_module.rs

# Luau service table: no "Website" line means game:GetService("Website") raises.
grep -n 'game.set("' eustress/crates/common/src/luau/runtime.rs

# The engine module: the declaration exists, the directory may not.
grep -n "pub mod website" eustress/crates/engine/src/lib.rs
ls eustress/crates/engine/src/website/

# Service assets, both landed.
cat eustress/crates/common/assets/service_properties/Website.toml
cat eustress/crates/common/assets/service_templates/Website/_service.toml

# The class-name contradiction in 0.2.
grep -n 'name: "Website"' eustress/crates/engine/src/space/space_ops.rs
grep -n 'class_name' eustress/crates/common/assets/service_templates/Website/_service.toml

# Worker routes and behaviour.
grep -n "website-manifest\|/manifest" infrastructure/cloudflare/api/src/index.js

# The simulation id gap described in 4.9.
grep -rn "experience_id" --include=*.rs eustress/crates/engine/src/

# Reference class schema and the service icon.
ls eustress/crates/common/assets/class_schema/Reference/
ls eustress/crates/engine/assets/icons/website.svg
```

Two more facts worth re-checking when a symbol lands, both of which decide whether
it appears in the in-engine API Browser and the Workshop agent's prompt:

- `engine/src/workshop/api_reference.rs` parses a fixed `SOURCES` list at compile
  time. A Website module in a new file is invisible to the catalog until that file is
  added to `SOURCES`, even though the functions work at runtime.
- The same parser matches the literal line `#[rune::function]` and nothing else. A
  function declared `#[rune::function(instance)]` runs and never appears in the
  reference. Every Website function in this document is a free function partly for
  that reason.

---

## 12. Related

- [`docs/design/WEBSITE_SERVICE.md`](../design/WEBSITE_SERVICE.md) - why the service
  exists, the bake contract, the build order.
- `Voltec/docs/WEBSITE_MANIFEST_API.md` - the consumer half: binding, caching,
  failure, build-time baking.
- [`docs/development/SCRIPTING_API_CHECKLIST.md`](../development/SCRIPTING_API_CHECKLIST.md) -
  repository-wide Luau and Rune status, section 16 for Eustress extensions.
- [`docs/services/README.md`](../services/README.md) - the per-service pages and the
  in-engine Services Browser.
- [`docs/services/soul-service.md`](../services/soul-service.md) - the service that
  hosts the Rune VM these bindings run inside.
