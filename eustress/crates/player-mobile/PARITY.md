# Mobile Player parity

Compared with the desktop Player (`crates/client`, `crates/play-runtime`, `crates/player-web`) and Studio's identity sign-in (`crates/engine/src/auth.rs`). Status is what this branch actually does.

| Desktop feature | Mobile status | Notes |
| --- | --- | --- |
| API base `EUSTRESS_API_URL` / `--api-url` | done | Mobile also accepts `--env production\|staging`. Compile-time default is staging (`https://api-staging.eustress.dev`). Desktop default stays production. Set before `api_base()`'s `OnceLock`. |
| Gallery browse and search | done | `GET /api/gallery?q=&limit=` and, with an empty search, `GET /api/gallery/featured`. Same JSON aliases as `crates/web/src/api/gallery.rs` (`simulations`, `author_name`, `play_count`). Slint list. |
| Identity sign-in | done | `POST /api/auth/challenge`, `POST /api/auth/verify-challenge` with `format: eustress-login-v1`, signature over `login_message`, then `GET /api/auth/me`. Token key is `login_file()` so staging and production do not share a file. |
| Token storage | partial | Android: EncryptedSharedPreferences with a Keystore `MasterKey` (`bridge-android` → `Bridge.java`). iOS: Keychain functions are stubs and return errors. Host `cargo check` uses an in-memory store. |
| Download and cache a published Space | done | `eustress_networking::published::fetch_published`, the same manifest, chunk, and `.pak` requests as `space_fetch`. Chunks land in `{files}/eustress/echk/{hash}.echk`. A legacy `.pak` unpacks under `pak/{id}`. |
| Open the Space mesh | not done | The opener is still `client/src/systems/space_world.rs`. Mobile caches the world and spawns the avatar in a stand-in yard. `WorldDownloaded` is emitted for a later opener. |
| Join over WebTransport | done | `native::start_join` + `session::begin_join` + `NetPlugin`. `Hello` uses `wire::PROTOCOL_VERSION` (5). Same link parser: `split_link_extras` / `parse_join_link`. |
| Deep link `eustress-player://` | partial | Android intent-filter and `PlayerActivity.onNewIntent`. `api=` is checked, not applied, same as the desktop Player. iOS URL scheme is in `crates/player-ios/Info.plist` and is unverified (no Apple SDK). |
| `input_actions` and touch | done | `eustress_common::input_actions`. Dynamic thumbstick, jump circle, right-half camera drag at 0.01 rad/logical px, pinch zoom. Slint is dispatched the pointer; finger ownership is one owner for the finger's life. Gallery sets `capture_all`. |
| Desktop keyboard and mouse | done | WASD, Space, Shift, Ctrl, mouse-look pixels × `sensitivity` 0.003, wheel zoom. Arrow-key orbit and Escape are unchanged. |
| Follow camera | done | `avatar::control::camera_follow` via `AvatarRuntimePlugin` (`AvatarHost::Client`). Same offset, eye height, and smoothing as desktop. |
| Character avatar | partial | `SpawnAvatar` with the default descriptor. Capsule, locomotion, and camera come from the shared runtime. The rig GLB is `bundled://` and is not packaged in the APK, so the mesh 404s until those assets are added. `view-look` (TonyMcMapface / SMAA) is off; the camera falls back to Reinhard. |
| Safe area | partial | `SafeArea` trait. Android reads system bars and cutout, divided by density. iOS UIKit call is written and unverified. |
| Haptics | partial | `Haptics` trait. Android `performHapticFeedback` plus `VibrationEffect` for a custom pulse. iOS feedback generators are written and unverified. Jump triggers `ImpactMedium`. |
| Suspend / resume | done | `SurfaceGate` still waits for `onSurfaceCreated`. Suspend sets `SwapchainHold` and drops the overlay texture slot. The Skia renderer is not cleared. |
| Luau `InputContext` scripting | not done | The action layer is the Rust contexts from the input design (CoreCharacter, CoreCamera, World, TextFocus, SystemMenu). Script-defined contexts are not loaded. |
| iOS app compile | unverified | `crates/player-ios` and `crates/bridge-ios` host-check. No `xcrun` on this machine. Keychain is an explicit stub. |

No store listing, no release, and this branch does not target `main`.
