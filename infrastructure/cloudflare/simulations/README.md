# eustress-simulations - RETIRED

**Status**: superseded by `eustress-api`. Do not deploy this worker.

Everything this directory describes has moved to
`infrastructure/cloudflare/api/src/index.js`, served on `api.eustress.dev`.
The files are kept so the history of the key namespace stays readable, not
because anything here still runs.

## What actually happened

`eustress-simulations` was deployed on 2 April 2026 to `simulations.eustress.dev`.
**That hostname never had a DNS record**, so the worker was never reachable from
the internet. Nothing ever called it.

It bound the bucket `eustress-simulations` as `SIMULATIONS` and read and wrote a
key namespace of the form `{id}/manifest.json`, `{id}/scene.eustress`,
`{id}/thumbnail.webp`, `{id}/versions/v{n}/...`.

**Nothing ever wrote that namespace either.** The engine publishes to
`api.eustress.dev`, which binds the same bucket as `SCENES` and writes
`universes/{id}/...` and `thumbnails/{id}/...`. So the two workers shared one
bucket and disagreed about its layout, and only one of them was ever reachable.

The README that used to live here documented a third generation again: an
`eustress-experiences` bucket behind `/api/experience/:id` routes, matching
neither deployment. That text is gone rather than corrected, because a document
describing a system nobody built is worse than no document.

## Where each route went

| retired route | now |
|---|---|
| `GET /api/simulation/:id` | `GET /api/simulations/:id` on `eustress-api`. Reads the listing record from KV (`sim:{id}`), not an R2 `manifest.json`. There is no `manifest.json` anywhere in the bucket and there never was. |
| `GET /api/simulation/:id/download` | `GET /api/simulations/:id/download`. Streams `sim.r2_key`, always `universes/{id}/universe.pak`. |
| `GET /api/simulation/:id/thumbnail` | `GET /api/simulations/:id/thumbnail`. **Ported**, because the api worker wrote `thumbnails/{id}/thumb.{ext}` and had no route that read it back. |
| `GET /api/simulation/:id/versions` | Nowhere. Nothing has ever written a `versions/` prefix, so this listed an empty set on every call. |
| `POST /api/simulation/publish` | `POST /api/simulations/publish`. The api worker publishes eagerly, so there is no `pending-manifest.json` stage. |
| `PUT /api/simulation/:id` | Nowhere. The api worker has no update path: every publish mints a fresh UUID. Known gap, tracked with the `sync.toml` `experience_id` work. |
| `POST /api/simulation/upload/:key` | The four typed upload routes: `PUT .../space`, `PUT .../space/multipart/{create,part,complete}`, `PUT .../spaces/{name}`, `PUT .../thumbnail`. Deliberately not ported: the retired handler took the R2 key straight from the URL path and wrote it verbatim. |
| `POST /api/simulation/:id/commit` | Nowhere, and nothing is missing. Publishing is eager. |
| `DELETE /api/simulation/:id` | Nowhere. The api worker has no delete route. Known gap. |

## Not to be confused with

`eustress-api` serves `GET /api/simulation/{id}/manifest` and
`GET /api/simulation/{namespace}/latest/manifest`, on the same singular
`/api/simulation/` prefix this worker used. Those are **new** routes for the
Website manifest, unrelated to the `{id}/manifest.json` object this worker read.
See `docs/design/WEBSITE_SERVICE.md`.

## The consolidated shape

One worker: `eustress-api` on `api.eustress.dev`.
One bucket: `eustress-simulations`, bound as `SCENES`.
One key namespace:

```
universes/{id}/universe.pak              published Universe package
universes/{id}/spaces/{name}.pak         a single published Space
universes/{id}/website-manifest.json     the Website manifest
thumbnails/{id}/thumb.{ext}              gallery thumbnail
ledger-backups/{date}.json               nightly ledger snapshot
```

The simulation listing record is **not** an R2 object. It lives in KV as
`SOCIAL:sim:{id}`. Keeping the Website manifest under its own R2 key matters for
the same reason: a publish must never overwrite the record that puts a Space in
the marketplace.

## To finish the retirement

1. Delete the `eustress-simulations` Worker in the Cloudflare dashboard.
2. Remove the `simulations.eustress.dev` route binding.
3. Delete the `simulations.eustress.dev` DNS record if one was ever added.
4. Leave the bucket alone. `eustress-api` uses it.
