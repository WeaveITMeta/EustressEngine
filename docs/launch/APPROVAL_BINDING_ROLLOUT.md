# Rolling out required approval bindings

An approved listing serves only while the objects stored are the ones its approval was bound to
(`docs/architecture/MODERATION_PIPELINE.md`, section 7, item 7). `REQUIRE_APPROVAL_BINDING` decides what
happens to a listing that has **no** binding yet:

| value | an approved listing with no binding |
|---|---|
| unset or `"false"` | serves as before; the hourly sweep binds it to what is stored now (50 a run) |
| `"true"` | reads as pending (only its author and an admin are served) until a person binds it |

Any approved listing that exists on the day a Worker with bindings first deploys has no binding. Turning
the flag on in that same deploy would hide all of them at once. So production rolls out in two deploys.

## Order

1. **Deploy with `REQUIRE_APPROVAL_BINDING = "false"`** (the value in `wrangler.toml` as shipped). Nothing
   changes for listings that already exist. Every new approval is bound from this moment.
2. **Let the sweep bind the rest.** The hourly cron binds up to 50 approved listings a run. Ask for the
   count with the admin tool `moderation_binding_report`:
   `approved`, `bound`, `revoked`, `unbound` and the first 20 `unbound_ids`. Wait until `unbound` is 0, or
   bind the stragglers by hand.
3. **Spot-check a sample.** The sweep binds whatever is stored when it runs, which is only as good as the
   storage at that time. A person opens a sample of listings (the package, the thumbnail, the review
   captures) and, for each, runs `moderation_bind` twice: first without `expected` (it shows the
   identities stored now and writes nothing), then again with those identities as `expected` (it binds
   exactly what was shown, and refuses if anything changed in between).
4. **Deploy with `REQUIRE_APPROVAL_BINDING = "true"`.** From here an approved listing with no binding
   reads as pending, the sweep binds nothing, and a person binds with `moderation_bind`.
5. **Confirm**: `moderation_binding_report` shows `required: true` and `unbound: 0`.

If a production environment has no approved listings on the day of the first deploy, steps 2 and 3 are
empty and the flag can ship as `"true"` at once. Check the count first; do not assume it.

## Staging checks before production (the scheme depends on real R2)

The binding compares what R2 returns from `head()` with what it returns from `get()`. If `get()` did not return the same `checksums` and `version` as `head()` for the same object, every hashed download would compare an `etag:` identity against a `sha256:` binding and the public would get 403 on every listing (a fail-closed outage, not a leak). So, on staging, before production:

1. A single-put `.pak` is approved; `locks/approved/{id}.json` holds `content` as `sha256:...`; an anonymous download returns 200.
2. A `.pak` assembled by multipart is approved; `content` is `etag:...:v...` (a literal `v` followed by nothing, or `undefined`, means `head()` gave no version); an anonymous download returns 200.
3. The thumbnail is served to an anonymous request as the image, not the placeholder.
4. Six parallel `POST /submit` on one listing start one run, and a killed run is taken over after its lease expires.

## What is not covered

- A package assembled from parts has no whole-object SHA-256. It is identified by its etag and R2's
  per-upload version, which a swap cannot keep, and it is never deduplicated.
- The gallery and author list pages show the listing record's text; only the routes that send content
  compare objects.
- Captures, thumbnails and covers are images the author's client supplies. A binding fixes which images
  were reviewed; it does not prove they show the content (the server-side render job does that).
