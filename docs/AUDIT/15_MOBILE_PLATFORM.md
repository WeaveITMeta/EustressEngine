# 15 — Mobile Platform

> iOS + Android player runtime, Bevy-on-mobile, touch input, mobile-optimised UI,
> lifecycle (pause / resume / backgrounding), platform permissions, mobile build
> tooling. Distinct from [01_CLIENT_PLAYER](01_CLIENT_PLAYER.md) — that doc treats
> mobile as a sub-row, but mobile deployment is its own deep concern.

## Pass changelog

- **P3 (2026-05-14):** New doc; 12 features.

---

## Concept summary

Mobile is **architecturally distinct** from desktop. The same Rust core (`player-mobile` crate) compiles to a `cdylib`/`staticlib` linked from native shells: `player-android/` (Gradle / Java / JNI / `game-activity`) and `player-ios/` (Xcode / Swift / `UIApplication` / Metal). Touch input replaces mouse / keyboard; safe-area insets replace window chrome; pause-on-background replaces always-running. App-store distribution, code signing, push notifications, and platform permissions (camera, microphone, location, photos) live on platform terms.

State today: `player-mobile/lib.rs` is a stub (hardcoded camera + cube; no project load, no networking, no Soul VM). `player-android` and `player-ios` exist as **separate OS projects** (Gradle + Swift respectively) but **do not link the Rust core** — they are independent placeholders. The integration gap is the biggest block.

Mobile is a category-defining feature for a Roblox-like; shipping it half-baked destroys retention. The audit recommends treating it as P1 platform, not P2 "as time permits".

---

## Implementation snapshot

**Crates / shells:**
- [eustress/crates/player-mobile/](../../eustress/crates/player-mobile/) — shared Rust core; `crate-type = ["cdylib", "staticlib"]`; Bevy + game-activity
- [eustress/crates/player-android/](../../eustress/crates/player-android/) — Gradle project; `.idea/`, `app/build/`; **no Cargo.toml**
- [eustress/crates/player-ios/](../../eustress/crates/player-ios/) — Xcode project; `Info.plist`, `AppDelegate.swift`, `ExportOptions.plist`; **no Cargo.toml**

**Working in `player-mobile`:**
- Bevy DefaultPlugins with mobile features
- `#[bevy_main]` entry point
- Conditional `android_logger` on Android
- Hardcoded setup (camera at `(0, 5, 10)`, cube, directional light)

**Not working:**
- Loading a `.eustress` world container (or joining a server) — not implemented *(format pivoted from `.pak` 2026-05-16; see MASTER C17)*
- Touch input mapping to engine input events
- Networking (QUIC client on mobile)
- Soul VM (Rune / Luau on mobile)
- iOS-specific deps in `[target.'cfg(target_os = "ios")']`
- Linking from Java (`System.loadLibrary`) or Swift (XCFramework)
- Asset bundling inside APK / IPA

---

## Feature inventory

| # | Feature | State |
| ---: | --- | :-: |
| 1 | Shared Rust core (`player-mobile` crate) | 🟡 stub |
| 2 | Android shell links Rust `.so` | 🔴 |
| 3 | iOS shell links Rust `.a` / XCFramework | 🔴 |
| 4 | Touch input → engine input events | 🔴 |
| 5 | Safe-area insets + mobile-responsive UI | 🔴 |
| 6 | App lifecycle (pause / resume / backgrounding) | 🔴 |
| 7 | Platform permissions (camera / mic / location) | 🔴 |
| 8 | Mobile asset bundling (APK / IPA resources) | 🔴 |
| 9 | Battery / thermal management | 🔴 |
| 10 | Mobile rendering tier (LOD floor, force-LOD2) | 🔴 |
| 11 | App-store distribution (TestFlight / Play Store) | 🔴 |
| 12 | Push notifications | 🔴 |

---

## Detailed per-feature cards (top 6)

### Feature 1 — Shared Rust core

**State:** 🟡 stub · **Effort:** L · **Risk:** Med · **Touches:** [01], [15]
**Sub-features:** `run_game()` entry · feature flags `android` / `ios` · cdylib + staticlib output · Bevy mobile features · Rust-side Soul + GUI + networking

**Concept.** The `player-mobile` crate compiles to a native library callable from Java / Swift. It runs the same Bevy App as the desktop Client minus rendering integration (which is provided by the shell). The shell handles `MainActivity` (Android) / `UIApplicationMain` (iOS) lifecycle and surfaces input events into the Rust core.

**Forecasted feedback (R)**
- R1.1 Today's `run_game` is hardcoded scene; needs `.eustress`-world-container-aware or server-joining variants *(was `.pak`-aware pre-2026-05-16)*.
- R1.2 Feature flags `android` / `ios` declared but unused — should gate platform-specific deps.
- R1.3 iOS-side deps section in `Cargo.toml` is missing; iOS build will fail without it (`metal`, `objc`, `core-foundation`).
- R1.4 Bevy on mobile has tested-rev sensitivity; pin in `Cargo.lock`.
- R1.5 The Rust core needs an explicit "input bridge" trait so shells inject `Touch`, `Tap`, `Pinch`, `Swipe` events.

**Implications (I)**
- *Architectural:* extracting input/render/lifecycle traits makes mobile + desktop diverge cleanly.
- *Cross-system:* [01_CLIENT] should depend on this trait too; eliminates platform-specific desktop code.
- *Operational:* CI must build the `cdylib` for both targets; currently it builds nothing for mobile.
- *Support burden:* mobile crashes are nearly impossible to debug without minidump + symbolication.

**Risks (X)**
- X1.1 Bevy 0.18 → 0.19 (or newer) on mobile may break game-activity integration.
- X1.2 Audio (`bevy_audio`, memory `feedback_audio_required`) on iOS / Android needs `cpal` / `oboe` configuration.

**Mitigations (M)**
- M1.1 Pin Bevy mobile rev; CI smoke-test mobile build per PR.
- M1.2 Test audio on real device early; switch to Oboe (Android) if needed.

---

### Feature 2 / 3 — Native shells link Rust core

**State:** 🔴 · **Effort:** L · **Risk:** Med · **Touches:** [15], [12_INFRASTRUCTURE]
**Sub-features:** Android `System.loadLibrary("eustress_mobile")` · iOS XCFramework or staticlib · `cargo ndk` (Android) · `cargo lipo` / `cargo xcode` (iOS) · ABI: arm64-v8a + x86_64 (emulator) for Android; arm64 (device) + x86_64 (simulator) for iOS

**Concept.** Android shell loads the `.so` via JNI; iOS shell links the `.a` via XCFramework. JNI / Swift bridge passes lifecycle events and touch input into Rust.

**Forecasted feedback (R)**
- R2.1 `player-android/app/build.gradle` doesn't declare native lib dependency.
- R2.2 `player-ios/Podfile` (if any) doesn't link the staticlib.
- R2.3 Android NDK version pinning matters; mismatches between dev machines.
- R2.4 iOS Bitcode is deprecated but App Store still accepts; pick.
- R2.5 Emulator builds need `x86_64` target — heavier CI matrix.

**Implications (I)**
- *Architectural:* shells become thin (just lifecycle + bridge); all logic in Rust.
- *Operational:* dual-target CI; cross-compile in a Linux container produces both targets.
- *Strategic:* until this lands, mobile is *blocking*. Single biggest gap in the system.

**Risks (X)**
- X2.1 NDK / Xcode upgrades silently break the build between CI runs.
- X2.2 Linker ABI mismatch produces incomprehensible runtime errors.

**Mitigations (M)**
- M2.1 Pin NDK / Xcode versions in CI runner image.
- M2.2 Use `cargo-ndk` and `cargo-xcode` cargo subcommands.

---

### Feature 4 — Touch input

**State:** 🔴 · **Effort:** M · **Risk:** Low · **Touches:** [01], [15]
**Sub-features:** `winit::TouchEvent` (or platform-direct) → engine input · virtual joystick / D-pad · pinch-zoom · swipe / tap · multi-touch · gyro optional

**Concept.** Mobile players don't have a mouse. Standard mappings: virtual joystick (left thumb) → movement; right-hand swipe → look; tap → fire / interact; pinch → zoom (in vehicles). Slint panels (HUD) must handle touch as primary input.

**Forecasted feedback (R)**
- R4.1 `UserInputService.TouchEnabled` must work; today nothing maps `winit::TouchEvent` → engine input.
- R4.2 Slint touch-handling on mobile is partial; check version.
- R4.3 Virtual joystick UX needs hysteresis + dead-zone tuning.
- R4.4 Multi-touch (2-finger pinch + 1-finger tap simultaneously) drops events under load.

**Implications (I)**
- *Architectural:* the input bridge trait (R1.5) is the seam.
- *Cross-system:* Slint HUDs need a "touch-friendly" layout flag.

**Risks (X)** — X4.1 Touch latency on 120 Hz screens needs <16 ms response.

**Mitigations (M)** — M4.1 Touch events processed in `First` schedule slot.

---

### Feature 6 — App lifecycle

**State:** 🔴 · **Effort:** M · **Risk:** Med · **Touches:** [01], [15]
**Sub-features:** suspend on `onPause` (Android) / `applicationDidEnterBackground` (iOS) · resume on foreground · save game on suspend · memory-pressure handler · OOM-kill survival

**Concept.** When the user alt-tabs, the app must pause cleanly: stop ticking Bevy schedule, save game state, stop network traffic. On resume, restore. On memory-pressure warning, free non-essential assets.

**Forecasted feedback (R)**
- R6.1 Without pause, Soul VM keeps ticking in background → drains battery + loses input.
- R6.2 Mobile OS can kill the process at any time; auto-save must fire fast.
- R6.3 Network reconnect on resume is its own state machine.
- R6.4 iOS background time-budget (30 s typical) needs respect.

**Implications (I)**
- *Cross-system:* [16_PERSISTENCE_DATASTORE] Feature 6 (save data) coordinates.
- *Operational:* battery drain in background = bad reviews.

**Risks (X)** — X6.1 OOM-kill mid-save → corrupt save file.

**Mitigations (M)** — M6.1 Atomic-rename save writes.

---

### Feature 10 — Mobile rendering tier

**State:** 🔴 · **Effort:** M · **Risk:** Med · **Touches:** [01], [04_ASSETS], [05_SPACE_STREAMING], [15]
**Sub-features:** force LOD floor (mobile = LOD2 min) · texture format ASTC default · reduced active-cap (e.g. 200k vs. 2.1M) · post-FX off · disable shadow cascades · dynamic resolution

**Concept.** Mobile has 10× less RAM, 4× less VRAM, and 1/3 the compute of mid-tier desktop. The rendering policy auto-caps LOD floor, switches texture formats to ASTC, disables expensive post-FX, drops active-instance cap.

**Forecasted feedback (R)**
- R10.1 Per-platform settings TOML — what overrides per-project, what's locked.
- R10.2 ASTC requires per-platform texture variants ([04_ASSETS] gap 18).
- R10.3 Dynamic resolution (scale framebuffer up/down per-frame) — complex; defer.
- R10.4 Battery vs. quality slider for end users.

**Implications (I)** — *Strategic:* mobile cap is the biggest UX delta from desktop.

**Risks (X)** — X10.1 Mobile cap clipping looks bad on creator's published project; surface clearly.

**Mitigations (M)** — M10.1 Preview Studio in "mobile mode" toggle.

---

### Feature 11 — App-store distribution

**State:** 🔴 · **Effort:** L · **Risk:** Med · **Touches:** [09_ECONOMY], [12_INFRASTRUCTURE], [15]
**Sub-features:** TestFlight / Play Internal Testing · App Store Connect / Play Console · in-app purchase (App Store / Play Billing) · review-submission automation · OTA update strategy

**Concept.** Each platform has gatekeeping. App Store review: ~24 h average; Play review: ~hours. IAP is platform-mandated (App Store IAP for digital goods on iOS, Play Billing on Android). Steam IAP from [09_ECONOMY] doesn't apply on mobile — separate path.

**Forecasted feedback (R)**
- R11.1 App Store rejects apps that use external IAP for digital goods → Bliss purchases on iOS must use App Store IAP.
- R11.2 Both stores take ~30% (or 15% small-dev). Pricing math changes.
- R11.3 Family Sharing / Game Center / Game Pass integrations expected.
- R11.4 Apple's anti-tracking (ATT) prompts affect telemetry.

**Implications (I)** — *Cross-system:* [09_ECONOMY] needs a platform-aware IAP abstraction.

**Risks (X)** — X11.1 First app-store rejection drops launch date by 1–2 weeks.

**Mitigations (M)** — M11.1 Submit-early for review-time understanding; use TestFlight beta gate.

---

## Wiring / import gaps (top 8)

1. Android shell links `libeustress_mobile.so` via NDK
2. iOS shell links `libeustress_mobile.a` via XCFramework
3. Touch input bridge trait + `winit::TouchEvent` mapping
4. App lifecycle observer (pause / resume / memory-warning)
5. Per-platform asset variants (ASTC mobile, BC7 desktop)
6. Mobile rendering tier config + Studio "mobile preview" mode
7. Platform IAP abstraction (App Store / Play / Steam)
8. CI multi-target build (arm64-v8a + x86_64 Android, arm64 + x86_64 iOS)

---

## Cross-system dependencies

- **[01_CLIENT_PLAYER]** shared core (`player-mobile` is the mobile variant of Client).
- **[04_ASSET_PIPELINE]** ASTC / Basis compression per-platform.
- **[05_SPACE_STREAMING]** mobile cap on active instances.
- **[06_WEBSITE]** universal-link / app-link for `/play/{sim_id}` deep-link on mobile.
- **[09_ECONOMY]** App Store / Play IAP path.
- **[12_INFRASTRUCTURE]** multi-target CI; mobile-specific code signing (provisioning profiles).

---

## Open questions

- Q15.1 iOS minimum target (iOS 15? 16?).
- Q15.2 Android minimum API (33? 34?).
- Q15.3 Tablet support — first-class or scale-up phone UI?
- Q15.4 Foldables / large-screen detection?
- Q15.5 Cross-device save-sync ([16] Feature 6) requires backend; ship together?
- Q15.6 Mobile-only marketing campaign or wait for parity?
