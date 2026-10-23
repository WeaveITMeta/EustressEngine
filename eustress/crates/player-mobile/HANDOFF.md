# Handoff: `mobile/player-parity`

This branch is the mobile Player on top of `mobile/vendor-patches`. The vendored Bevy and android-activity crates, their `[patch.crates-io]` entries, and the lockfile source/checksum removals are the parent commit `2e272f97856ef815aa66df5c7d8dff9a3752e85c` (`chore(vendor): patch Bevy and android-activity for the Android Player`). Read `eustress/.patches/MOBILE_VENDOR_HANDOFF.md` before changing those crates. Do not add another copy of them here.

Everything above that commit is app and engine code: `crates/player-mobile`, `crates/player-android`, `crates/bridge-android`, `crates/bridge-ios`, `crates/bridge-core`, `crates/player-ios`, `eustress-common`, `eustress-avatar-schema`, and the shared net and play-runtime pieces the Player uses.

Draft PR #17 stays a draft against `mobile/vendor-patches`. Do not merge it, and do not retarget it at `main`.

The tree still has the parity13 fixes:

- The floor writes its lifted pitch back into `AvatarCamera`, so a later upward drag leaves the ground.
- Jump apex and take-off follow stride. The dressed full-height body is 0.943 m / 4.302 m/s. The undressed default body is 0.886 m / 4.168 m/s.
- An IME `Update` does not show the soft keyboard again, and a letter missing from the IME buffer is still typed.
- `eustress-player://space/<slug>` opens a published Space with no host or port, and every incoming link is logged with `ticket` and `key` redacted.
- The follow camera pulls in when the avatar walks into a solid wall.
