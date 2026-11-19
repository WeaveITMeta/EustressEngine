# World play token

Status: written and tested in the Worker, **not deployed**. The Player side
(`eustress/crates/client/src/systems/space_fetch.rs`) is written and **not
built**. Security reviews this before any deploy.

## What it is for

A Player downloads a published world with one `GET .../world/manifest` and
then one `GET .../world/chunks/{blake3}` per chunk. Every one of those
requests ran the whole serving gate (`world.mjs` `servableListing`):

| Read | Store | Per request |
|---|---|---|
| the listing record `sim:{id}` | KV | 1 |
| the legal lock `locks/legal/{id}` | R2 head | 1 |
| the deletion mark | R2 head | 1 |
| the approval binding `locks/approved/{id}.json` | R2 get | 1 |
| the manifest's identity (`identOf(r2_key)`) | R2 head | 1 |
| a server credential (`esrv_`), when one is sent | KV | 2 |
| a play pass, when one is sent | KV | 1 or more |

The answer for chunk N is the answer the manifest got a moment earlier. The
manifest GET now returns a short-lived token, and a chunk GET that presents
it skips the KV half of that gate: the listing record and any credential
lookup. The R2 reads that carry refusals (legal lock, deletion mark,
approval binding) are still made on every chunk.

## Wire format

Manifest answer headers:

```text
X-Eustress-Play-Token: wpt1.<base64url(JSON claims)>.<base64url(HMAC-SHA256)>
X-Eustress-Play-Token-Expires: <Unix seconds>
```

Claims: `{ "v": 1, "sim": <listing id>, "sha": <manifest SHA-256 hex>,
"ident": <approval binding content identity>, "exp": <Unix seconds> }`.
The MAC covers the literal `wpt1.` plus the base64url claims.

Chunk request header: `X-Eustress-Play-Token: <the token>`. A chunk served
on a token carries `X-Eustress-Chunk-Gate: play-token`.

Key: `HMAC-SHA256(JWT_SECRET, "world-play-token-v1")`, derived once per
isolate. The same pattern and secret as the signed adult image URLs
(`adult_content.mjs`, label `adult-image-url-v1`), under its own label, so
a play token is never a valid session JWT or image signature and the other
way round. No new secret binding is needed. With no `JWT_SECRET` nothing is
minted or accepted.

Lifetime: 900 seconds (`PLAY_TOKEN_TTL_SECONDS`). A token whose expiry is
more than one lifetime in the future is refused as not minted by this code.

## When a token is minted

Only when all of these hold on the manifest GET (`world.mjs`
`playTokenFor`):

1. The full gate served the manifest and found the listing **listable**
   (approved and public, after the legal lock, the deletion mark and the
   approval binding were applied by `simForServing`).
2. The listing is **not adult content** and its rating is **within the
   solo signed-out cap** (`mature_17` or lower, `SOLO_SIGNED_OUT_CAP`; it
   was `ages_10` until McKale's 2026-11-19 decision that solo play needs no
   sign-in). This is exactly the case in which `refuseAdult` lets a request
   through on the world routes, which pass `SOLO`, with no viewer at all.
3. An approval binding exists, is not revoked, and names a content identity.

So a token is only ever issued for content that any signed-out stranger may
already download. This is the core of the threat model: the token is a
bearer token, and leaking, sharing or replaying it grants nothing that the
same person could not get by asking the Worker with no credential.

No token for: an author or admin viewing a pending or private listing, a
play pass on a private listing, a server credential on an unlisted listing,
adult content (`adult_18`, `adult_21`, or no rating), a listing without a
binding.
Those chunk GETs keep the full gate on every request.

## What redeeming checks (`world.mjs` `chunkByPlayToken`)

1. The MAC verifies (`crypto.subtle.verify`, constant time), the prefix is
   `wpt1`, the token is at most 1024 characters, the claims parse.
2. `sim` equals the listing in the URL.
3. `exp` is in the future and not more than one lifetime away.
4. Fresh from R2, in parallel: no legal lock, no deletion mark, the
   approval binding exists, is not revoked, and its `content` identity is
   the one in the token, and the token was minted after the listing's
   play-token floor (`locks/play-token-floor/{id}.json`). Every rating write
   (`moderation.mjs` `applyToSim`: a decision, or `moderation_set_rating`)
   moves the floor to one second past now (`retirePlayTokens`). The floor is
   its own object, never a field of the binding, so writing it cannot race a
   write or revocation of the binding. A floor that cannot be read honours
   no token.
5. The chunk is named by the manifest the token names
   (`universes/{id}/manifests/{sha}.json`). A chunk only the source world
   names never takes the token path.

Any failure is not an error: the request falls through to the full gate,
which gives the same answer it always gave (403, 404 and so on).

## Threats and answers

| Threat | Answer | Test |
|---|---|---|
| Token used on another listing | `sim` is in the MAC'd claims and must equal the URL | `a token for one listing is nothing on another` |
| Token used for a chunk of another world, or a stray stored chunk | The chunk must be in the token's own manifest; a token naming an unstored manifest serves nothing | `a token serves only the chunks its own manifest names`, `a token bound to another manifest hash serves nothing` |
| Expired token, or one dated far ahead | `exp` checked against now and now + TTL | `an expired token is refused...` |
| Forged token: claims edited, MAC under `JWT_SECRET` itself, under another secret, truncated, wrong prefix, a session JWT, bad base64, oversized | MAC under a label-derived key, verified in constant time; shape checks first | `a forged or altered token is refused` |
| Approval revoked after the mint | Binding read fresh from R2 on every chunk | `an approval revoked after the mint ends the token at once` |
| Legal lock (serve block or content lock) or deletion after the mint | Lock and deletion mark read fresh from R2 on every chunk | `a legal lock or a deletion after the mint ends the token at once` |
| Republish, reviewed and approved again | The new binding's content identity differs from the token's | `a new approval (a reviewed republish) retires tokens...` |
| Adult or above-cap content handed to a minor via a shared token | Never minted for those ratings | `no token where the manifest was not open to anyone signed out` |
| A token minted at `mature_17` used after a re-rate to `adult_18` | The rating write moves the play-token floor; a token minted at or before it takes the full gate | `a play token minted at mature_17 is refused after moderation_set_rating re-rates the listing adult_18` (tests/adult_page_and_token_floor.test.mjs) |
| Token leaked in a log or shared | Grants only what a signed-out stranger already has; dies in 15 minutes | by construction (minting rule) |

Every test above was checked against a deliberate mutation of the code it
guards (12 mutations, each made at least one test fail; see the report that
landed this change).

## What the token does not see until it expires

These changes live only in the KV listing record, which the token path does
not read. They reach every **manifest** GET at once (so no new download
starts), and reach chunk GETs made on an existing token within at most 15
minutes:

- the author makes the listing private, or archives it;
- a moderation status change on the KV marker that was not accompanied by a
  revoked binding, a legal lock or a deletion mark.

A rating change is no longer in this list (review finding, 2026-11-19): every
rating write moves the listing's play-token floor in R2, which the token path
reads on every request, so a token minted before it is not honoured.

The test `documented window: a change only the KV record holds...` pins this
behaviour so it cannot change silently. Removals that matter for safety
(takedowns, legal holds, quarantines, revoked approvals, deletions) are all
written to R2, which the token path reads on every request.

If Security wants the window shorter, lower `PLAY_TOKEN_TTL_SECONDS`; a few
MB world downloads in seconds, so 300 seconds would also work.

## Cost effect

Per chunk on the token path: 0 KV reads (was 1, or 3 or more with a
credential), R2 reads 3 (lock head, deletion head, binding get; was 4 with
the identity head), plus the chunk itself. The manifest GET pays one extra
R2 get (the binding, to mint). Worker request count is unchanged.

## Not done

- Not deployed to staging or production.
- The browser Player cannot read the header yet: the Worker's CORS answer
  exposes only `ETag` (`index.js`, `Access-Control-Expose-Headers`).
  Exposing `X-Eustress-Play-Token` is a one-line follow-up for the browser
  build; the native Player reads it directly.
- The dedicated server's own world download (`engine/src/space/gallery_edit.rs`
  `download_for_host`) does not send the token yet.
- The Player change in `space_fetch.rs` is not compiled.
