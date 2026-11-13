# Staging-only site files

Files here are served by the staging website (`staging.eustress-cbb.pages.dev`,
Pages project `eustress`, branch `staging`) and never by production
(`eustress.dev`, branch `main`). Trunk does not copy this folder:
`index.html` copies `.well-known/` from the crate root into every build, so a
file that must stay off production lives here instead.
`eustress/scripts/browser-player-staging.sh` copies this folder's
`.well-known/` into `dist/.well-known/` after the build, and nothing else does.

- `.well-known/assetlinks.json`: lets the debug Android Player
  (`dev.eustress.player.debug`, signed with
  `eustress/crates/player-android/debug.keystore`, alias `androiddebugkey`)
  use staging passkeys (`delegate_permission/common.get_login_creds`). The API
  accepts the same app's sign-ins through `PASSKEY_APP_ORIGINS` in
  `[env.staging.vars]` of `infrastructure/cloudflare/api/wrangler.toml`.
  That keystore is in the repository, so this file stays off production.

A production deploy is built fresh (`trunk build --release` replaces `dist`).
Never deploy a `dist` the staging script built to branch `main`.
