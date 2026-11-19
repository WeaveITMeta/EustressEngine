# Signed updates: what Studio trusts, and how a release earns it

How Eustress Studio decides that an update came from Eustress and was not
changed on the way. Every Studio release publishes a signed copy of its
manifest; Studio installs an update only after the signature verifies
against a public key it carries.

Nothing here is live. The release workflow steps are written and their shell
was run on a throwaway key (see "State"); the Studio side
(`eustress/crates/engine/src/update_trust.rs` and `updater.rs`) is being
written separately and has not been compiled or tested. No signing key
exists yet, `TRUSTED_KEYS` is empty, and no release has been signed. The
owner generates the key (see "The signing key").

## What is signed and what is checked, in order

The signed object is `v<version>/latest.json`, the release's own manifest,
byte for byte. It is signed with Ed25519. The root `latest.json` stays
where it is for older Studios and is only a pointer to the version: it
carries no trust.

Studio checks, in this order, and stops at the first failure:

1. It reads the root `latest.json` and learns the newest version.
2. It fetches `v<version>/latest.json` and `v<version>/latest.json.sig`
   from the download host.
3. It reads the signature line, finds the key whose id matches in its own
   `TRUSTED_KEYS`, and verifies the signature over the exact bytes it
   fetched. No matching key, or a bad signature, refuses the update.
4. It parses the verified manifest and requires `"product": "studio"`. Any
   other product is refused.
5. It downloads the installer for its platform and compares the SHA-256
   and size against the verified manifest.

Everything after step 3 reads only the verified bytes. Because the
manifest holds each file's SHA-256, one signature covers every installer in
the release.

## File formats

`latest.json.sig` is a text file with one line:

```text
<key id> <signature>
```

- `key id`: the lowercase hex of the first 8 bytes of SHA-256 of the 32 raw
  public key bytes (16 hex characters).
- `signature`: the Ed25519 signature, 128 lowercase hex characters.

Lines starting with `#` and blank lines are allowed and ignored.

The signature is made with OpenSSL over the exact bytes of `latest.json`:

```text
openssl pkeyutl -sign -rawin -inkey <private key pem> -in latest.json
```

`latest.json` carries `"product": "studio"`. The root copy and the
versioned copy are the same bytes. The download host serves any file named
`[A-Za-z0-9._-]+` under `/v<X.Y.Z>/`, so the signature needs no host change.

## Where the public key lives

`eustress/crates/engine/src/update_trust.rs` holds `TRUSTED_KEYS`, a list of
`TrustedKey { id: "<16 hex>", key: [..32 bytes..] }`. It is empty today.

To add a key, the owner hands over the public half only. Print the id and
the 32 bytes from the private PEM on a trusted machine (nothing printed
here is secret):

```text
openssl pkey -in <private key pem> -pubout -outform DER | tail -c 32 | od -An -tx1
openssl pkey -in <private key pem> -pubout -outform DER | tail -c 32 | sha256sum | cut -c1-16
```

The first command gives the 32 key bytes; the second gives the key id.
Add one `TrustedKey { .. }` entry, build, and ship a Studio release that
carries it. The release workflow refuses to publish when the key that signed
a release is not listed in this file, because Studio would reject it.

## The signing key

The owner generates it, on his own machine:

```text
openssl genpkey -algorithm ed25519 -out eustress-update-signing.pem
```

- The private PEM goes into the GitHub Actions secret
  `EUSTRESS_UPDATE_SIGNING_KEY` (the whole file, including the BEGIN and END
  lines).
- A second copy is kept offline, for example on an encrypted drive in a safe
  place. Losing both copies means no release can be signed for the keys in
  Studio, and every installed Studio needs a manual reinstall.
- It never goes into the repository, a chat, an issue, a log, or an AI
  session. Only the public half is handed around.

The release workflow reads the secret in one step, writes it under the
runner's temp folder with mode 600, signs, verifies the signature with the
matching public key, checks that the key id is listed in `update_trust.rs`,
and deletes the file in a step that runs even when the job fails.

## Key rotation

Rotation is three releases, so no installed Studio is ever left without a
key it trusts:

1. Ship a release whose `TRUSTED_KEYS` lists both the old key and the new
   key. That release is signed by the old key, which every installed Studio
   already trusts.
2. Once Studios have taken that release, sign later releases with the new
   key (replace the secret).
3. Later, ship a release that drops the old key from `TRUSTED_KEYS`.

A Studio that skipped step 1 cannot verify a release signed by the new key.
It has to be reinstalled by hand.

## What the Automatic updates setting does

Specified, not yet built or tested. The setting is on by default.

- Studio checks at launch and every six hours.
- It downloads and verifies the update in the background.
- It installs when the user quits Studio.
- It never restarts Studio by itself.

Turning the setting off stops the checks and downloads.

## Development builds

A development build never updates itself. It would otherwise replace a
working tree's binary with a released one.

## Platforms

- Windows and Linux: Studio installs the verified update when the user
  quits.
- macOS: Studio notifies only. It tells the user a verified release exists
  and links to the download; the user installs it.

## An unsigned release

Until `TRUSTED_KEYS` lists a key, the workflow publishes an unsigned
release with a warning and no `latest.json.sig`. A Studio with an empty
`TRUSTED_KEYS` has no key to verify against, so it installs no update by
itself. Once a key is listed, the
workflow fails the job when `EUSTRESS_UPDATE_SIGNING_KEY` is absent, so an
unsigned release cannot be published by accident. An unsigned release never
installs itself on a Studio that carries a key.

## What this does not protect against

- A stolen signing key. Whoever holds it can sign a release every Studio
  accepts. The defence is the key's storage (a GitHub secret plus an offline
  copy) and rotation after any suspected exposure.
- A release frozen at an old version. An attacker who controls the download
  host could keep serving a genuine, older, signed manifest, so a Studio is
  held at a version with a known flaw. The signature proves a manifest is
  genuine, not that it is current.
- A compromised GitHub workflow or runner. The workflow signs whatever it
  builds.

## State

| Piece | Where | State |
|---|---|---|
| Signing, verifying and uploading steps | `.github/workflows/release.yml` | Written. The signing step's shell was run locally against a throwaway key and a sample manifest, including the cases with and without the secret. The workflow itself has not run on GitHub |
| Trusted key list, verification | `eustress/crates/engine/src/update_trust.rs` | Being written by another worker; not compiled |
| Updater, setting, install on quit | `eustress/crates/engine/src/updater.rs` | Being written by another worker; not compiled or tested |
| The signing key | GitHub secret and offline copy | Does not exist yet; the owner generates it |
