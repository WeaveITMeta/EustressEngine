# Studio sign-in with the Eustress identity key

Status: **plan, build 17; Security approved it 2026-10-01.** McKale's decision
(2026-10-01): Studio proves the account by signing the API's challenge with
the user's Eustress identity key, the same account as the website and the
gallery, with no trip through a browser. Owners: Multiplayer (Studio:
`engine/src/auth.rs`, the sign-in handlers in `engine/src/ui/slint_ui.rs`,
`common/src/api_base.rs`); Website (the Worker's challenge and verify, and the
website's own sign-in in `web/src/pages/login.rs`).

## What Studio does today

The Login dialog signs in with an identity file (`identity.toml`): the
handlers `LoginWithIdentity` and `SwitchIdentity`, and the startup restore,
in `slint_ui.rs`. Each reads the file's `public_key` and `private_key`, asks
`POST /api/auth/challenge` for a challenge, signs it, and sends the
signature to `POST /api/auth/verify-challenge`, which answers with a 72-hour
token (a JWT) and the account. The Worker's half is sound: a challenge is 32
random bytes, lives 5 minutes, and is consumed by the first verify.

The gaps, all on Studio's side except the third:

1. **A failed sign-in still signs in.** When the challenge fails (the key is
   not registered, the file has no private key, or the API is unreachable),
   Studio sets its token to the PUBLIC key, marks itself signed in, and shows
   "Signed in as" the username written in the file. A public key is public,
   so a file holding someone else's key and name shows Studio signed in as
   them. Every API call then fails.
2. **The token is never saved.** The Player reads its sign-in from Studio's
   saved login file (`<local data>/EustressEngine/auth_token`, per API since
   build 16). The identity sign-in never writes it, so the Player is never
   signed in: it mints no identity ticket, joins as a guest who cannot buy,
   and a gallery session refuses it (`no_ticket`, HOSTED_ADMISSION.md).
3. **A signature names no API.** The signed message is the bare challenge.
   A Studio pointed at another API (`EUSTRESS_API_URL`) can be handed
   production's challenge for the user's key, and the signature it returns
   signs the user in on production. The website's sign-in signs the bare
   challenge too.
4. **Sign-in and switching run on the frame**, which freezes Studio for up
   to 12 s when the API is slow.
5. **A restored token leaves `jwt_valid` false**, so Bliss earns nothing
   after a restart.
6. **`DEV_MODE` (`auth.rs`, `const DEV_MODE: bool = true`).** The
   email-and-password and Steam paths call routes the Worker does not have
   (`/api/auth/login`, `/api/auth/steam`) and then sign in a fake local user
   with a fake token, which they also write to the login file the Player
   reads. No part of the UI calls them, but they ship in the binary.

## The rule

Studio is signed in only when the API it talks to issued it a token for the
account whose identity key signed that API's own challenge. Nothing else
counts as signed in: no fake user, no public key standing in for a token, no
name taken from a file.

## Studio

One sign-in, in `auth.rs`, on a thread of its own, with its result applied by
`auth_poll_system` as every other sign-in result is:

1. **Read the identity file.** `public_key` and `private_key`, 64 hex digits
   each. The private key must produce the public key; otherwise the sign-in
   stops with "This identity file's keys do not match." A file without a
   private key stops with "This identity file has no private key, so it
   cannot sign in." Nothing is sent for a bad file.
2. **Ask for a challenge:** `POST /api/auth/challenge {public_key}`.
3. **Sign the login message**, the UTF-8 bytes of three lines:
   ```
   eustress-login-v1
   <origin>
   <challenge>
   ```
   `<origin>` is the API Studio is talking to, from `api_origin()` in
   `common/src/api_base.rs`: the WHATWG URL origin, computed with the `url`
   crate by the same rules the Worker's `new URL()` uses (scheme and host in
   lower case, the default port dropped: `https://api.eustress.dev`,
   `https://api-staging.eustress.dev`, `http://127.0.0.1:8787`). It always
   comes from Studio's own setting, never from the API's reply, so a relaying
   API cannot choose it. Studio signs only a 64-hex challenge, and its
   sign-in requests follow no redirects.
4. **Verify:** `POST /api/auth/verify-challenge {public_key, challenge,
   signature, format: "eustress-login-v1"}` returns `{token, user}`.
5. **Save the token** for this API (`save_token`: `auth_token` for
   production, `auth_token.<scope>` for any other API; readable by this user
   only). The Player on this machine then shares the sign-in.
6. **Signed in:** the token, the account's id and username from the API
   (never the file's), and `jwt_valid` true.

A failure leaves Studio signed out, showing why:

| Cause | Shown |
|---|---|
| API unreachable | "Could not reach Eustress. Studio works offline; sign in again when you are back online." |
| Key not registered (404) | "This identity is not registered with Eustress." |
| Account suspended (403) | The API's reason. |
| Bad identity file | The file's problem, as in step 1. |
| Signature refused (401) | "Eustress did not accept this identity's signature." |
| Request refused (400: a used or expired challenge, an origin off the API's list) | The API's own words. |

- **Startup:** one thread, in order. A saved token is checked with
  `GET /api/auth/me`: valid, Studio is signed in. Refused (401), Studio signs
  in with the saved identity file, if there is one. Unreachable, Studio stays
  signed out and works offline.
- **Switching identity:** sign out here (this API's login file is removed;
  the account stays signed in elsewhere), then sign in with the chosen file.
- **Signing out** (`AuthState::sign_out_here`) signs out of this machine:
  this API's login file is removed, which signs the Player here out too, and
  the account stays signed in on other devices and the website.
- **Signing out of every device** (`AuthState::sign_out_everywhere`) also
  asks the API (`POST /api/auth/logout`) to end every token the account
  holds, in Studio, the Player and the website, everywhere. A sign-in
  started meanwhile waits for that request, so it can never end the new
  token. If the API cannot be reached, the Output says so ("Signed out here.
  Eustress could not be reached to end the session on your other devices; it
  ends on its own within 72 hours."). Whether Studio's Sign Out does this, or a separate "Sign out of all
  devices" control does, is McKale's call; Sign Out calls `sign_out_here`
  until then, and the switch is one line in its handler.
- **Removed outright:** `DEV_MODE`, `mock_login`, `mock_steam_login`,
  `do_email_login`, `try_real_login`, `do_steam_login`,
  `extract_token_from_url`, `submit_login`, `login_with_steam` and
  `LoginForm`. No dev-only feature replaces them: a developer signs in with a
  real identity on staging (`EUSTRESS_API_URL`), whose login is kept apart
  from production's.
- **The private key** stays in the user's file. Studio keeps only the file's
  path in its settings, reads the key only to sign, and never logs, shows or
  sends it. The signing key wipes itself when dropped (ed25519-dalek's
  `zeroize`); the file's text and the parsed copies of the key are freed
  without wiping, so in memory the key is as exposed as in the file on disk.
  The file being plain text is the risk to fix (see Later).
- `slint_ui.rs` keeps its three handlers, which now call `auth.rs`;
  `do_challenge_auth` and `parse_identity_toml` move into `auth.rs`. No
  `.slint` file changes.

## Worker (Website)

- `verify-challenge` with `format: "eustress-login-v1"` checks the signature
  over the three-line message, with `<origin>` set to
  `new URL(request.url).origin`: the host the request actually reached. A
  signature made for another origin fails, which closes the relay.
- That origin must also be on the deployment's `LOGIN_ORIGINS` list
  (production: `https://api.eustress.dev`; staging:
  `https://api-staging.eustress.dev`; a local `wrangler dev` adds
  `http://127.0.0.1:8787`), and `workers_dev = false` goes on production and
  staging, so a second hostname serving the same Worker and data
  (`*.workers.dev`) cannot count as a sign-in. Any other `format`, such as
  `eustress-login-v2`, is refused (400 `login_format_required`), never read
  as the bare format; an origin off the list is 400
  `login_origin_not_allowed`.
- Pending challenges are filed under a hash of the challenge, so asking for
  a challenge in someone's name no longer replaces theirs.
- `POST /api/auth/logout` (bearer token) sets the account's floor:
  `verifyAuth` refuses a token issued strictly before it, and a sign-in's
  token is never stamped before it. Bans and account deletions use the same
  floor.
- The bare format goes away in one step: the website's sign-in signs the
  same message with the API's origin, and in the same deploy the Worker
  refuses a request without `format`. There is no window in which both are
  accepted. Staging gets it first; production only on McKale's deploy go,
  with the Worker's other pending security fixes.

## Tests

Studio (`engine`, run in the orchestrator's batch; `common` lock-free):

- `login_message_is_bound_to_the_api`: a fixed test key, challenge and
  origin give a fixed message and a fixed signature (Ed25519 signatures are
  deterministic). The same vector goes into the Worker's tests, so both
  sides agree on every byte.
- `a_signature_for_another_api_is_not_this_ones`: the same challenge for
  another origin is another message, and its signature fails this one.
- `identity_files_are_checked_before_anything_is_sent`: mismatched keys,
  bad hex and a missing private key each give their message.
- `a_failed_sign_in_is_signed_out`: after a failed result, `is_logged_in` is
  false, `get_token` is `None` and `jwt_valid` is false; after a successful
  one, `jwt_valid` is true.
- `api_origin_is_normalised` (common): case, a trailing slash and a default
  port.

Worker (Website): the v1 message is accepted; one signed for another origin
is refused; a used challenge is refused; an expired one is refused; a request
without `format` is refused (or accepted only inside the transition window);
the shared vector verifies.

Live, on staging: Studio signs in with a staging identity; the Player on the
same machine joins a gallery session and is admitted, the two-machine test
of HOSTED_ADMISSION.md.

## Later

From Security's review, outside this change, in their order of risk:

1. The identity-backup email attaches the plain-text private key, so whoever
   reads that inbox owns the account: send a passphrase-encrypted file, or
   no key.
2. The website's sign-in takes the private key into the page, so a script
   injection means key theft: enforce the Content-Security-Policy (it is
   report-only now) and import the key as a non-extractable WebCrypto key.
3. `identity.toml` is plain text on disk: encrypt it, or keep the key in the
   operating system's credential store with the file as an export.

## Order

1. Security reviews this plan (approved 2026-10-01, with the changes above).
2. Website: the Worker's v1 and the website's sign-in, on staging.
3. Studio: build 17, on the orchestrator's wave.
4. The live run on staging; production only on McKale's go.
