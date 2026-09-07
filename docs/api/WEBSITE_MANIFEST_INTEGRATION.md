# Wiring a website to a Eustress manifest

**Audience**: whoever maintains the consuming website. Voltec is the first.
**Status**: the engine bakes and uploads; the read route ships in `eustress-api`.
**Date**: August 27, 2026

A Eustress Space owns its numbers. This is how a website displays them without
retyping them, and what it must do when the network is not there.

The whole design rests on one rule, so it goes first:

> **The page must already be correct before the fetch.** Bake the current values
> into the HTML at build time and let the manifest correct them. A page that
> renders empty until a fetch resolves renders empty when the fetch fails, and a
> numeric specification that flashes blank reads as broken to exactly the
> audience it exists to convince.

Everything below follows from that.

---

## 1. What you are given

The Space author gives you two things:

| | example | where it comes from |
|---|---|---|
| namespace | `vcell` | Website service Properties, in the Space |
| auth key | `eus_pk_...` | minted in the same panel, shown once |

The key is **not a secret**. You will send it from browser JavaScript, so anyone
who opens devtools can read it. What it buys is revocable attribution and rate
limiting: the author can see which consumer is calling, a limit applies per
consumer, and rotating cuts a consumer off. Do not build anything on the
assumption that it keeps the manifest private, because it does not.

---

## 2. The endpoint

```
GET https://api.eustress.dev/api/simulation/{namespace}/latest/manifest
X-Eustress-Key: eus_pk_...
```

`?key=` is accepted where a header is impossible, such as a no-code embed or a
CMS URL field. Prefer the header: a key in a query string lands in access logs,
`Referer` headers and browser history.

Response:

```json
{
  "namespace": "vcell",
  "schema_version": 3,
  "publish_hash": "blake3:1c9fa83...",
  "baked_at": "2026-08-27T07:41:00Z",
  "values": {
    "specific_energy": {
      "value": 1032,
      "display": "1,032",
      "unit": "Wh/kg",
      "label": "Specific energy, cell level",
      "basis": "derived",
      "source": "Workspace/V-Cell/V1/Core/Enclosure#material.custom.wh_per_kg"
    }
  }
}
```

Every entry carries both `value` and `display`.

**Render `display`. Compute with `value`.** Formatting `value` yourself will
drift from the author's own rounding, and parsing `display` back into a number
breaks the first time a unit appears in it.

`basis` travels with every value because a figure without its basis is not a
figure. You can render `simulated` differently from `measured` without keeping
your own list of which is which.

---

## 3. Marking up the page

Mark what a value belongs to. Do not fetch per element.

```html
<span data-eus="vcell:specific_energy">1,032</span>
<span data-eus="vcell:specific_energy" data-eus-field="unit">Wh/kg</span>
<span data-eus="vcell:cycles_at_design_rate" data-eus-field="basis"></span>
```

The element's existing text is the fallback **and must already be correct**.
Twenty-five values cost one request. Adding a twenty-sixth costs nothing.

---

## 4. The hydration script

One script, once, at the end of the document.

```js
const NAMESPACE = 'vcell';
const KEY = 'eus_pk_...';                 // public by design, see section 1
const SCHEMA = 3;                         // pin it; a rename is opt-in
const URL = `https://api.eustress.dev/api/simulation/${NAMESPACE}/latest/manifest`;

async function hydrate() {
  let m;
  try {
    const res = await fetch(URL, {
      headers: { 'X-Eustress-Key': KEY },
      cache: 'default',                   // let the browser revalidate
    });
    if (res.status === 401) {             // key wrong, missing or rotated
      console.warn('eustress: auth key rejected, keeping baked values');
      return;
    }
    if (res.status === 429) {             // rate limited, try again later
      console.warn('eustress: rate limited, keeping baked values');
      return;
    }
    if (!res.ok) return;                  // keep the baked values
    m = await res.json();
  } catch {
    return;                               // offline: keep the baked values
  }

  if (m.schema_version !== SCHEMA) {      // a rename is opt-in, not automatic
    console.warn('eustress: schema', m.schema_version, 'expected', SCHEMA);
    return;                               // do NOT partially apply
  }

  for (const el of document.querySelectorAll('[data-eus]')) {
    const [ns, key] = el.dataset.eus.split(':');
    if (ns !== m.namespace) continue;
    const v = m.values[key];
    if (!v) { console.warn('eustress: no manifest key', key); continue; }
    const field = el.dataset.eusField || 'display';
    if (v[field] !== undefined) el.textContent = v[field];
  }
  document.documentElement.dataset.eusHash = m.publish_hash;
}
hydrate();
```

Note what every failure branch does: it **returns and leaves the page alone**.
That is the whole contract. The fetch can only improve a page that is already
correct.

---

## 5. Caching

The worker sets `max-age=300, stale-while-revalidate=86400` and an `ETag` equal
to the publish hash, so:

- a repeat visit inside five minutes makes **no** network request
- after five minutes the browser serves the cached copy **and** revalidates
  behind it, so nobody waits
- an unchanged manifest returns `304` with no body
- a publish changes the hash, so the next revalidation returns `200`

**Do not add a cache-busting query string on every load.** It defeats the
revalidation path, turns a free `304` into a full download on every visit, and
spends your rate limit on nothing.

### Pinning

A page that must show one exact state pins the hash:

```
.../manifest?v=blake3:1c9fa83...
```

A pinned response is immutable and cacheable for a year. Use it for anything
quoting a specific revision, a signed document, or a figure a reader may return
to. If the stored hash no longer matches, a pin returns `404` rather than
silently serving a different state.

---

## 6. Rate limits

120 requests per minute per key. That clears well over a thousand manifests in
ten minutes, so a large catalogue refreshes comfortably at deploy.

Two things keep you well inside it:

- **Send `If-None-Match`.** A 304 is the cheap path and the one the design
  wants. Cache-busting is what actually costs you.
- **One fetch per namespace, not per value.** If you find yourself fetching in a
  loop over elements, the markup is wrong, not the limit.

A `429` returns `Retry-After`. Back off; do not retry immediately.

---

## 7. Failure

| condition | behaviour |
|---|---|
| network unreachable | keep baked values, no visible change |
| `401` key missing, wrong or rotated | keep baked values, log |
| `429` rate limited | keep baked values, log, back off |
| `!res.ok` otherwise | keep baked values, log |
| `schema_version` mismatch | keep baked values, log, do **not** partially apply |
| key missing from manifest | leave that element, log the key |
| value `null` | impossible: a failed reference fails the publish |

One rule underneath all of it: **the page is already correct before the fetch,
and the fetch can only improve it.** Anything else turns a network problem into
a credibility problem.

---

## 8. Build-time baking

The same manifest feeds the build, which is what keeps the fallbacks honest:

```
fetch manifest  ->  write values into the HTML  ->  deploy
```

Run it as a pre-deploy step and **fail the build if a `data-eus` key has no
manifest entry**. That catches a renamed reference at build time instead of on a
visitor's screen, and it means the committed HTML always shows the state of the
last publish.

With build-time baking in place, runtime hydration becomes a correction for
publishes that happened after the last deploy rather than the primary path. Both
are worth having: the build keeps the page correct, the fetch keeps it current.

---

## 9. Key rotation

When the author rotates, the outgoing key keeps working for a grace window,
30 days by default. You will not be cut off mid-deploy.

Update `KEY` and ship. If you see a sustained `401`, the window closed: ask the
author for the current value from the Website service Properties.

An author who believes a key is compromised can rotate with a zero-day window,
which cuts the old key off immediately. That is deliberate.

---

## 10. Papers and artifacts

Documents that quote the same values fall into two groups.

**Hosted on the site**: identical treatment to any other page. Same `data-eus`
attributes, same hydration, same fallbacks.

**Published as Claude artifacts**: these run under a strict CSP that blocks
every external host, so they **cannot** fetch a manifest. They are
point-in-time documents. Regenerate them on publish and stamp `publish_hash` and
`baked_at` in the footer, so a reader can tell which state a paper describes and
whether a newer one exists.

Do not try to work around the CSP. A paper that silently fails to update is
worse than a paper that says which day it was true.

---

## 11. Checklist

1. Get the namespace and key from the Space author. Pin `schema_version`.
2. Mark values with `data-eus="{ns}:{key}"`, with the **current correct value as
   the element's text**.
3. Add the hydration script once, at the end of the document.
4. Add the build-time bake and make a missing key fail the build.
5. **Verify with the network disabled that every number still reads correctly.**

Step 5 is the one that matters. If the page is right with the network off, the
manifest is an improvement. If it is not, the manifest is a dependency, and you
have built the thing this design exists to avoid.
