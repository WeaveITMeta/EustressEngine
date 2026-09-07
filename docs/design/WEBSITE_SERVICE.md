# Website Service and the Published Manifest

**Status**: specification, engine track in progress
**Version**: 2.0
**Date**: August 26, 2026

A Space is the source of truth for its own numbers. A website that quotes those
numbers currently retypes them, and retyped numbers drift: the V-Cell site
carried 907 Wh/kg and 699 cycles for a week after the specification said 953 and
237, because nothing connected the two.

This specifies the connection. A new Service lets an author mark values in the
Explorer as **References**. On publish those references **bake** into
`universes/{id}/website-manifest.json`, which sits beside the Universe `.pak`
and is a separate object from the simulation listing record. A website fetches
that one document, with a key that says who is asking, and updates every value
it displays from it.

The design constraint that shapes everything below: **one fetch, not one per
value.** A page quoting twenty-five numbers must cost one request.

---

## 1. Why not the `.pak`

A `.pak` is tar + zstd of the Universe directory, published to
`universes/{id}/universe.pak`. For the V-Cell Space that is 58 MB across 2,356
instance files, and it exists so the Player can open the whole world. Asking a
browser to pull it, decompress it and parse TOML to recover twenty-five scalars
is the wrong shape by three orders of magnitude.

The `.pak` stays exactly as it is. The manifest is a second, small object
written by the same publish, to `universes/{id}/website-manifest.json`.

---

## 2. The Service

`Website` is a Service like `MaterialService` or `SoulService`: a folder at the
root of a Space, holding `Reference` instances.

```
Spaces/VCell/
  Website/
    _service.toml
    specific_energy/_instance.toml     class_name = "Reference"
    cycles_at_design_rate/_instance.toml
    part_count/_instance.toml
    can_dimensions/_instance.toml
```

### 2.1 `_service.toml`

```toml
[service]
class_name = "Website"

# Stable identifier the manifest publishes under. A Space may serve more than
# one site; each gets its own Website service in its own Space, or namespaces
# within one.
namespace = "vcell"

# Bumped by the author when the MEANING of a key changes rather than its value.
# Consumers pin against this, so a rename is a breaking change they opt into.
# Transport is not meaning: issuing or rotating a key never bumps this, or every
# consumer hard-stops on a day no number changed.
schema_version = 3

# The CURRENT key's short handle, never the key itself.
key_id = "wk_3f8a12"
key_created_at = "2026-07-02T09:15:00Z"
key_rotated_at = "2026-08-26T07:41:00Z"
```

Every value is a flat scalar under `[service]`. The loader runs each `[service]`
entry through `toml_to_property_value`, which accepts scalars and drops tables,
and the saver then rewrites the file from that map. A nested `[website.key]`
table would not survive the first save of the service.

`class_name` is `Website`, matching the folder. Three lookups key off the class
name while the Explorer row is built from the folder on disk, so a service whose
class and folder disagree resolves to nothing and renders a blank panel.

**The raw key is never written here.** `_service.toml` is tracked by git and
packaged into the `.pak`, and a `.pak` is downloadable by anyone, so a key
written into it is readable by people who never visit the site at all. That
would remove the only thing the key buys, which is knowing who is calling and
being able to cut them off. Rotation works only while the current key lives in
one place the author controls.

So the key is shown once, at mint time, in a reveal dialog, registered with the
worker as a SHA-256 digest, and never written back. An author who loses it
rotates. Only the handle, and the timestamps, live on disk.

`_service.toml` inside the Space is what publish reads for namespace, schema
version and key identity. `common/assets/service_properties/Website.toml` is the panel schema,
in the same `[Section]` and `Key = { type = ... }` shape every other service
uses, and it is what the Properties panel renders. The two change together,
because authoring a field in one alone produces either a value nothing displays
or a control that persists nothing.

### 2.2 A Reference

```toml
[metadata]
class_name = "Reference"

[attributes]
kind   = "instance"
source = "Workspace/V-Cell/V1/Core/Enclosure#material.custom.wh_per_kg"
label  = "Specific energy, pack level"
unit   = "Wh/kg"
format = "{:.0}"
basis  = "derived"
```

`key` defaults to the instance name. Explicit `key` overrides it.

---

## 3. Reference kinds

Five, because the numbers a site quotes come from five different places. A site
quoting "2,341 parts" and "300 × 100 × 100 mm" is reading the tree, not a
scalar, and a design that only handles scalars solves half the problem.

| kind | resolves | example source |
|---|---|---|
| `instance` | a property on one instance | `Workspace/.../Enclosure#electrochemical.capacity_ah` |
| `sim` | a published simulation value | `battery.specific_energy_wh_kg` |
| `count` | entities matching a path glob | `Workspace/V-Cell/V1/Assembly/**` |
| `measure` | a geometric measure over a subtree | `bbox:Workspace/V-Cell/V1/Assembly` |
| `expr` | an expression over other references | `energy_wh / pack_mass_kg` |

### 3.1 `instance`

`<path>#<section>.<field>`, with `<path>` relative to the Space root. Dotted
fields index into TOML tables. Resolution reads the **live datamodel**, not the
file, so a value that the engine computed or reconciled is what gets baked.

### 3.2 `sim`

Reads the published simulation namespace. A `sim` reference **must** name the
run it came from:

```toml
[reference]
kind = "sim"
source = "battery.capacity_retention"
run_label = "I_life_25C_5MPa_res0"
at = "final"          # final | min | max | mean | at_cycle:<n>
```

Without `run_label` the publish fails. A number lifted from whatever happened to
be in memory is how a figure ends up on a website with no way to reproduce it.

### 3.3 `count`

```toml
kind = "count"
source = "Workspace/V-Cell/V1/Assembly/**"
filter = "class_name = Part"
```

### 3.4 `measure`

```toml
kind = "measure"
source = "bbox:Workspace/V-Cell/V1/Assembly"
axis = "x"            # x | y | z | volume | surface
unit = "mm"
```

Measures resolve against the same tree the viewport draws, which is what makes
"the drawing and the datasheet agree" a property of the system rather than a
thing somebody checks.

### 3.5 `expr`

```toml
kind = "expr"
source = "energy_wh / pack_mass_kg"
```

Operands are other reference keys in the same namespace. Evaluation is a
directed acyclic graph; a cycle fails the publish and names the loop.

---

### 3.6 Basis, and how it propagates

Every reference carries a `basis` saying what kind of claim it makes. Five
kinds, and the ordering between them is what matters:

| basis | meaning | who checks it |
|---|---|---|
| `input` | an authored choice, nothing derives it | nobody. It is the axiom |
| `derived` | computed by `expr` from other references | recomputation at bake |
| `simulated` | measured in a run, which the reference names | the run must exist |
| `projected` | extrapolated from measured values by a stated model | the model must be named |
| `unverified` | no recoverable derivation | blocks publish if rendered |

**A `derived` reference inherits the weakest basis in its whole dependency
chain.** A figure computed from a projection is a projection, and the manifest
reports it as one:

```
lifetime_kwh_kg  =  specific_energy * cycles      basis derived
                    specific_energy               basis derived  -> input
                    cycles                        basis projected
                 -> provenance: projected
```

This is not bookkeeping. A V-Cell manifest published three cycle counts as
`simulated` when the fade rates under them were measured and the counts were
extrapolated. Six further figures derived from those counts, including the
headline the entire site was built around, and every one of them reached the
page labelled as a measurement. Nothing in the reference graph objected,
because each reference was individually plausible.

So the manifest carries **both** `basis` and `provenance`, and a page that
prints a value can print what kind of claim it is without the author having to
remember. An `unverified` reference, or anything resting on one, is refused at
bake if it has a `format`: a value with no basis underneath it must not reach
a reader, and the way to publish the page anyway is to drop the reference, not
to soften it.

---

## 4. Baking

Bake runs as part of publish, after the `.pak` is packaged and before upload.

```
resolve every Reference in dependency order
  └── unresolved, ambiguous or type-mismatched  →  PUBLISH FAILS
emit website-manifest.json
upload  universes/{id}/universe.pak
        universes/{id}/website-manifest.json
        universes/_namespaces/{namespace}.json
```

**The website manifest is a separate object from the simulation listing.** The
listing record is the marketplace entry: name, description, thumbnail, play
count. The manifest is the numbers a site quotes. They are written by different
requests to different keys and they never share one. A manifest written over the
listing takes the Space out of the marketplace, and nothing surfaces that until
somebody goes looking.

**A failed reference fails the publish.** It does not emit `null`, and it does
not carry the previous value forward. The failure mode this feature exists to
prevent is a website confidently displaying a stale number, and a bake that
silently degrades reintroduces it in a new place.

The failure message names the reference, the source it could not resolve, and
the nearest candidates in the tree, because "specific_energy: no instance at
Workspace/V-Cell/V1/Core/Enclosur (did you mean Enclosure?)" is a fix and
"resolution error" is a ticket.

### 4.1 What a bake writes

```json
{
  "namespace": "vcell",
  "schema_version": 3,
  "simulation_id": "8f3a1c04-...",
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

Both `value` (typed, for arithmetic) and `display` (formatted, for the DOM) are
emitted, so a consumer never reimplements the author's formatting and never
parses a formatted string back into a number.

`basis` travels with every value because the V-Cell campaign's hardest-won rule
is that a figure without its basis is not a figure. A site can render
`simulated` differently from `measured` without maintaining its own list of
which is which.

`simulation_id` is the UUID in the R2 key and in the route, not a Space
identifier. It is here so a reader of a saved manifest can find the publish it
came from.

**No key appears in the body.** A manifest carrying its own key hands that key
to everyone holding a cached copy, including every CDN and every archive, and
makes rotation pointless. The key travels on the request and never in the
response.

---

### 4.2 The drift gate

Resolving references keeps the manifest honest. It does nothing for the
documents beside it, and that is where the damage happens: a specification, a
README, a slide, a hardcoded constant in a generator. They quote the same
numbers and nothing re-resolves them.

So a Website service may register **resources**, which are files the bake reads
but never writes:

```toml
[[resource]]
path         = "Workspace/V-Cell/PATENT.md"
history_from = "## Revision record"
history_to   = "## Basis of this specification"
allow        = ["the study run at 11.5 mAh/cm2 that preceded"]
```

A reference may list `stale`, the strings that used to stand for it. If a
registered resource still contains one, **the publish fails and names the file
and line.**

Two exemptions, both explicit rather than inferred:

- Lines between `history_from` and `history_to` are skipped, because a
  revision record legitimately quotes what it superseded. The range is bounded
  on purpose. An unbounded "everything after this marker" exemption silenced
  97 % of a 2,085-line specification the first time it was tried, and the file
  reported clean while carrying nine stale figures.
- Lines matching an `allow` entry are skipped, for prose that deliberately
  cites an old value. Written down, not guessed from how the sentence reads.

The first run of this gate over an already-reviewed V-Cell revision found 51
stale references across eight files. Four changed conclusions: a capacity
computed at the previous areal loading, a fraction-of-theoretical carried from
the previous revision, a compression stroke that was one layer count relabelled
as another without recomputing, and an internal resistance divided by the wrong
number of layers.

None of those were reachable by reading. All four were figures derived from a
superseded input, kept because the figures beside them had been updated.

---

## 5. Serving

One Worker serves everything: `eustress-api` on `api.eustress.dev`, reading the
R2 bucket `eustress-simulations` through the binding `SCENES`. The
`eustress-simulations` Worker is retired and `simulations.eustress.dev` has no
DNS record; nothing new is added there.

```
GET /api/simulation/{namespace}/latest/manifest
GET /api/simulation/{namespace}/latest/manifest?v={publish_hash}
GET /api/simulations/{id}/website-manifest
```

**A site uses the namespace route.** `POST /api/simulations/publish` mints a
fresh UUID on every publish, so a URL with a UUID in it is correct until the
next publish and silently stale after. The namespace route reads a pointer
written by the same publish, so it always resolves to the current manifest.

| object | key |
|---|---|
| manifest | `universes/{id}/website-manifest.json` |
| namespace pointer | `universes/_namespaces/{namespace}.json` |

The pointer holds `simulation_id`, `publish_hash` and `updated_at`. It sits
under `universes/` so one prefix covers everything a publish writes, and
`_namespaces` cannot collide with a simulation id because a UUID contains no
underscore.

The UUID route exists for pinning one publish and for the engine to verify its
own upload. `GET /api/simulations/{id}` already returns the listing record;
`/website-manifest` is a distinct suffix over a distinct object, and folding the
two into one response is the conflation section 4 forbids.

The engine writes the manifest on publish, over the author's bearer token, the
same way it uploads the `.pak`:

```
PUT /api/simulations/{id}/website-manifest
```

The Worker writes the namespace pointer as part of serving that `PUT`, rather
than the engine making a second call. Two calls can half-succeed, and the
half that leaves a namespace pointing at a manifest which was never uploaded
serves a 404 to every consumer until somebody republishes.

| header | value | why |
|---|---|---|
| `ETag` | `"{publish_hash}"` | the engine already computes this and already skips uploads when it is unchanged |
| `Cache-Control` | `public, max-age=300, stale-while-revalidate=86400` | a repeat visitor costs nothing; a stale manifest still renders while the fresh one arrives |
| `Vary` | `X-Eustress-Key` | one key's response must never be served to another key, or per-key attribution is fiction |
| `Access-Control-Allow-Origin` | `*` | the manifest is published content; the key attributes the caller rather than restricting who may read |
| `Access-Control-Allow-Headers` | `Content-Type, X-Eustress-Key` | a custom header makes every browser fetch preflighted, so an omission here fails the consumer before the `GET` is sent |
| `Access-Control-Max-Age` | `86400` | without it the preflight repeats on every revalidation and a free `304` costs two round trips |

The shared CORS helper at `infrastructure/cloudflare/api/src/index.js:5277`
currently allows `Content-Type, Authorization, X-ID-Type`. `X-Eustress-Key` has
to join that list, and the symptom of forgetting is precisely the failure this
feature exists to kill: the preflight fails, the page keeps its baked values,
and the site shows last month's numbers with no error anyone sees.

**Force refresh** is a consequence of the hash, not a separate mechanism.
Publishing changes `publish_hash`, so the next conditional request returns 200
rather than 304. A consumer that needs determinism pins `?v={hash}` and gets an
immutable response it can cache for a year.

---

## 6. The manifest key

Every request for a manifest carries a key. It is worth being exact about what
that buys, because this is an easy place to write copy that promises more than
it delivers.

**What the key does.** It says who is calling. Traffic is attributed per key and
rate limited per key, and a key can be revoked, which cuts off one consumer
without touching any other. That is real: it is how an author sees which sites
are live on their numbers, and how they stop one that misbehaves.

**What the key does not do.** It does not make the manifest secret. A key sent
from browser JavaScript sits in the page source and in the network tab, so
anyone who opens devtools has it. A browser key is published the moment it
ships.

**If confidentiality is the actual requirement**, this is the wrong tool as
specified. Put a server-side proxy in front of the manifest holding a key the
browser never sees, or issue short-lived signed tokens minted per session. Both
are real designs, neither is this one, and a longer key string is not a
substitute for either.

### 6.1 Sending it

| context | how |
|---|---|
| browser or server fetch | `X-Eustress-Key: eus_pk_...` |
| header impossible | `?key=eus_pk_...` |

Prefer the header. Query strings land in access logs, in `Referer` headers on
outbound links, and in browser history, so the query form widens the exposure of
a value that is already not secret and returns nothing for it. It exists for the
places that genuinely cannot set a header, such as a feed reader or a CMS field
that accepts a URL and nothing else.

Two kinds of key, told apart by prefix so a leaked one is greppable and the two
can never be mistaken for each other:

| prefix | kind | where it lives |
|---|---|---|
| `eus_pk_` | browser key, public by design | the site's JavaScript |
| `eus_bk_` | build key, held as a CI secret | the deploy pipeline, never the page |

Same manifest, different exposure. They are separately revocable, so killing a
leaked browser key leaves the pipeline that keeps the committed HTML honest
running.

### 6.2 Verifying it

The Worker looks a key up by hash, never by value, because a dump of the key
store should not hand anyone a working key.

| response | condition |
|---|---|
| `200` | key recognised and live |
| `401` | key missing, unrecognised, or past its overlap window |
| `403` | key recognised but revoked, or not permitted for this namespace |
| `429` | per-key rate limit exceeded |

A consumer treats `401` and `403` identically, because from the page's side both
mean the key no longer works and the correct response to both is to keep the
values already on screen. The distinction is for the operator reading logs.

The default budget is 60 requests per minute per key, burst 120, adjustable per
key in Properties. With a five-minute `max-age` a site of any size sits far
under that, so reaching the limit means a loop, and a loop is what the limit is
for.

Counting is a lower bound rather than a ledger. A response served from a browser
or edge cache never reaches the Worker, so cached traffic is invisible to both
the counter and the revocation check. State the consequence plainly rather than
glossing it: **revoking a key takes effect within one `max-age` window, not
instantly**, and a pinned `?v=` response, immutable for a year, is effectively
unattributed after its first fetch.

### 6.3 Rotating it

Rotation moves `current` to `previous`, mints a new `current` and stamps
`rotated_at`. Both verify until `rotated_at` plus `overlap_days`, after which
`previous` returns `401`.

The overlap is not politeness. A consumer is a deployed website, and updating
one means a person editing a config and running a deploy. With no overlap,
rotation breaks every live site the instant it happens, so nobody rotates, so a
leaked key stays live forever. Thirty days is the default because it survives a
holiday.

Properties shows a new key in full exactly once and a masked form afterwards,
with **Copy**, **Rotate** and **Revoke**. Rotate names the outgoing key's expiry
date in its confirmation, because "rotate" without that date reads as free and
it is not.

**Revoke has no overlap.** It is the control for a key that is being abused, and
a grace period on that control defeats its only purpose.

---

## 7. Authoring flow

1. Select an instance or a property in the Explorer.
2. **Add to Website** in the context menu, which creates a `Reference` in the
   `Website` service pre-filled with the source path, and puts the cursor in
   the key field.
3. Set label, unit, format and basis in Properties, which is the same
   polymorphic panel every other class uses.
4. **Generate key** in Properties, once per site. Copy it then; afterwards
   Properties shows the masked form and the operations in section 6.3.
5. **Publish**. The bake resolves everything, or fails and says which.

The Website service shows a live preview column with each reference's current
resolved value, so an author sees a wrong path before publishing rather than
after.

---

## 8. What this does not do

**It does not push.** A website learns about a change on its next fetch. With a
five-minute `max-age` that is the update latency, and raising it is a consumer
decision.

**It does not version values.** The manifest is the current state. History lives
in the Space's git and in the run records; a consumer wanting a time series
should be reading telemetry, not this.

**It authenticates, and it does not make the manifest secret.** The key names
the caller, so traffic can be attributed, limited and cut off. That is revocable
attribution and abuse control, not confidentiality: a key a public website sends
from browser JavaScript is readable by anyone who opens devtools. Section 6 says
what to reach for when confidentiality is the real requirement.

**It does not make values private.** A published manifest is readable content,
like the `.pak`. Anything an author would not put on a public page does not
belong in a Reference, and no key changes that.

**It does not solve artifacts.** Published Claude artifacts run under a strict
CSP that blocks external hosts, so a paper published that way cannot fetch a
manifest, with a key or without one. Papers either live on the site, where they
can fetch, or are regenerated on publish and stamped with `publish_hash` so a
reader can tell which state they describe.

### 8.1 If it ever pushes

Live values are the obvious next ask: a run finishes and the site moves without
a republish. The constraint that makes it safe is worth writing down before
somebody builds it.

**Only `simulated` references may be pushed, and pushing one must re-run its
dependents.** A live measurement that lands beside a `derived` figure computed
from the previous measurement puts two numbers on the page that no longer agree,
and the derived one looks the more authoritative because it is rounder. Either
the whole dependent chain moves with the input or the push is rejected.

That also means a push has to fail the same way a bake does. A pushed value that
breaks a recomputation is not a new value, it is a broken manifest, and serving
it stale is better than serving it inconsistent.

**A push never changes a basis.** If a projection is replaced by a measurement,
that is an authoring change and it goes through publish, where the drift gate
can check that the documents quoting the old figure were updated too.

---

---

## 9. Build order

| # | Change | Where |
|---|---|---|
| 1 | `WebsiteService` and `Reference` classes, loader, Properties panel | `common/src/classes.rs`, `common/assets/class_schema/Reference/_instance.toml`, `common/assets/service_templates/Website/_service.toml`, `common/assets/service_properties/Website.toml`, `engine/src/space/instance_loader.rs` |
| 2 | The service appears in the Explorer for new Spaces, with its icon | `engine/src/space/space_ops.rs` `SERVICE_FOLDERS`, `engine/assets/icons/website.svg` |
| 3 | Resolver for `instance` and `sim` kinds | `engine/src/website/resolve.rs` |
| 4 | Key issue, rotation, revocation, masked display | `engine/src/website/`, Properties panel |
| 5 | Bake wired into publish, ahead of upload | `engine/src/ui/file_event_handler.rs:701` `execute_publish_upload`, `PUBLISH_API` at `:688` |
| 6 | Manifest and pointer routes, key check, `ETag`, `Vary`, CORS headers | `infrastructure/cloudflare/api/src/index.js`, routes beside `:824`, R2 writes beside `:4090` |
| 7 | `count`, `measure`, `expr` kinds | `engine/src/website/resolve.rs` |
| 8 | Explorer **Add to Website**, live preview column | `engine/src/ui/slint_ui.rs` |

Steps 1 to 6 are a working feature: an author can reference scalars and
simulation values, issue a key, and a site can consume them. Steps 7 and 8 are
what make it pleasant.

The publish flow this bake hooks into is recorded in
`docs/architecture/AVATAR_AND_PARITY.md`, which is the accurate account of what
the engine uploads and where it lands.

The consumer contract is specified separately in the Voltec website document,
and is deliberately generic: nothing in it is V-Cell specific.
