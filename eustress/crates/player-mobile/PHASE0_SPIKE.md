# Phase 0 mobile spike

Branch `mobile/phase0-spike`. Spike only. No release, no store upload, no production deploy.

## Which integration pattern worked

**Pattern (a), Skia on Bevy's wgpu device, is the one that builds.** Bevy 0.19 owns the window (`android-game-activity` / stock `GameActivity`). `SkiaWGPURenderer` draws a Slint label and a button into an `Rgba8Unorm` texture on that same device. Bevy samples the texture on an unlit quad in front of a lit cube.

Evidence it links:

- `cargo ndk -t arm64-v8a build --release -p eustress-player-mobile --features android` finished. `libeustress_player_mobile.so` (aarch64, 66,334,216 bytes) exports `android_main`.
- The same command for `x86_64` finished. That `.so` is what the emulator loaded.
- Host `cargo check -p eustress-player-mobile --features android` finished (dev profile, 1.34s) before the Android links.

**FemtoVG-wgpu does not build for Android.** Slint 1.17.1 at `2447c697` cfg-gates the module:

```
error[E0433]: cannot find `femtovg_renderer` in `platform`
note: found an item that was configured out
   --> api/rs/slint/lib.rs:437:9
    | not(target_os = "android"),
440 |     pub mod femtovg_renderer {
```

Probe: `eustress/spikes/femtovg-android`. Exit 101.

**Pattern (b) typechecks on the host and was not run.** `cargo check -p eustress-player-mobile --features android,pattern-b` finished in 1.51s (`/tmp/pattern-b-host.log`). `pattern_b.rs` builds `RenderCreation::manual` from a Slint wgpu instance/device/queue and a 640x480 scene texture. It is not `android_main`. Slint's winit backend is not compiled for Android, and `backend-android-activity` would be a second activity next to GameActivity. **Android runtime of pattern (b): UNVERIFIED.**

## Patches

| Patch | Used? |
| --- | --- |
| `.patches/slint-2447c697` (software renderer) | Yes, when Slint is `branch = "master"` so the lockfile stays on `2447c697`. The successful graph compiled `i-slint-renderer-software` from that path. |
| `.patches/wgpu`, `.patches/wgpu-hal` | Not applied. They are wgpu **28.0.0**. Bevy 0.19 and this Slint pin use wgpu **29**. Applying them would split the device. |

`slint` with `rev = "2447c697..."` is a different git source from the patch's `i-slint-core` (`branch = "master"`). The arm64 link then failed:

```
ld.lld: error: duplicate symbol: slint_new_path_elements
>>> libi_slint_core-cfbb04e426a29804.rlib
>>> libi_slint_core-185a122e4644bbfd.rlib
```

Same failure for `BasicBorderRectangleVTable` and the other Slint vtables. Switching the dependency to `branch = "master"` (lockfile still pins `2447c697`) cleared it.

## What built

- Android cdylib, crate-type `cdylib` only, feature `android` (android_logger). Bevy 0.19, `default-features = false`, `android-game-activity`, 3D scene features. Slint git master / lock `2447c697`, features `std`, `compat-1-2`, `renderer-software`, `renderer-skia`, `unstable-wgpu-29`. Not the workspace Slint line (`renderer-femtovg` is cfg'd out on Android and would unify badly).
- Debug APK `eustress/crates/player-android/app/build/outputs/apk/debug/app-debug.apk`, **58,040,278 bytes**. `applicationId` `dev.eustress.player` (debug suffix `.debug`). ABIs `arm64-v8a` and `x86_64`. Stock `com.google.androidgamesdk.GameActivity` from `androidx.games:games-activity:4.4.0` (the version android-activity 0.6.1 documents). `androidx.appcompat:appcompat:1.7.0` because GameActivity extends `AppCompatActivity` and the games-activity POM lists no dependencies. No Kotlin sources. `MainActivity.kt` removed.
- `android.app.lib_name` = `eustress_player_mobile`.

## What failed

**games-activity 2.0.2** (the old Gradle pin) loads, then aborts. Native glue from android-activity 0.6.1 does not match that Java class:

```
java.lang.NoSuchMethodError: no static or non-static method
"Lcom/google/androidgamesdk/GameActivity;.onTouchEventNative(JLandroid/view/MotionEvent;IIIIIJJIIIIIIFF)Z"
GameActivity: RegisterNatives failed for 'com/google/androidgamesdk/GameActivity'; aborting...
```

Before appcompat was added, launch died earlier:

```
java.lang.ClassNotFoundException: Didn't find class "com.google.androidgamesdk.GameActivity"
Suppressed: java.lang.NoClassDefFoundError: Failed resolution of: Landroidx/appcompat/app/AppCompatActivity;
```

**Emulator frame.** The APK installed and `GameActivity` started on an Android 14 `google_apis` x86_64 AVD (`phase0`), software GPU, **TCG** (`-accel off`). KVM is unusable on this host: `kernel BUG at arch/x86/kvm/x86.c:702` in `kvm_arch_vcpu_create`.

Two GPU attempts, neither produced a cube or the Slint overlay on screen:

1. Default backends `VULKAN|GL`, device created up front with `initialize_renderer` and no window. logcat (`EustressPlayer`): wgpu downlevel warning (`SURFACE_VIEW_FORMATS` missing), then `GPU clustering isn't supported on this device`. The render thread then blocked in the emulator's gfxstream encoder: `libvulkan_enc.so` `vkCreateImageView` → `qemu_pipe_read`. The activity was covered by "System UI isn't responding". **The lit cube and the Slint label were not visible.**
2. Fresh emulator with `-feature -Vulkan`, and `lib/x86_64/wrap.sh` setting `WGPU_BACKEND=gl`, device created by Bevy (`RenderCreation::Automatic`) and cloned for Skia once the render world has it. logcat:

```
phase0 panic: panicked at bevy_render-0.19.0/src/renderer/mod.rs:286:36:
Unable to find a GPU! Make sure you have installed required drivers!
```

That panic is ~5s **before** `GameActivity: onSurfaceCreated_native`. GLES does not enumerate without the surface, and Bevy asks for the adapter first. Screenshot `/opt/cursor/artifacts/phase0-spike.png` is the System UI ANR dialog on a black portrait frame, not the scene.

**Rendered frame: UNVERIFIED.**

AGP does not pack a non-`.so` file from `jniLibs`. `wrap.sh` is added to the APK after `packageDebug` (`zip` + `zipalign` + `apksigner`). Source copy: `app/src/emulator-jni/x86_64/wrap.sh`. arm64 has no wrap script, so a device still uses `VULKAN|GL`.

## Luau

mlua **0.10.5**, features `luau,serialize,send`. NDK r27c. Dev profile.

| Target | Result |
| --- | --- |
| `aarch64-linux-android` | Built. Probe `eustress/spikes/mlua-android` finished in 30.52s. |
| `armv7-linux-androideabi` | Built. Same probe finished in 29.04s. |
| `aarch64-apple-ios` | **UNVERIFIED.** `cc-rs: failed to find tool "xcrun"`. No Mac SDK on this machine. |

`eustress-common --no-default-features --features mlua` does **not** build for Android by itself. Bevy pulls `android-activity` with neither backend feature:

```
error: Either "game-activity" or "native-activity" must be enabled as features
error[E0583]: file not found for module `activity_impl`
```

With `--features mlua,bevy/android-game-activity` (still `--no-default-features`):

- `aarch64-linux-android`: `Finished dev profile in 9m 54s`. `libeustress_common.rlib` produced.
- `armv7-linux-androideabi`: `Finished dev profile in 10m 29s`. `libeustress_common.rlib` produced.

Default features of `eustress-common` (model-import, geotiff, streaming) were not part of this check.

## Phase 1

- Ship pattern (a): Skia wgpu 29 on the Bevy device. Do not use FemtoVG on Android. Do not vendor the wgpu 28 patches onto this stack.
- Keep one Slint git source (`branch = "master"`, lockfile on `2447c697`) so `.patches/slint-2447c697` stays in the graph.
- Keep GameActivity 4.4.0 plus appcompat. No Kotlin. Match any future `android-activity` bump to the Java artifact.
- Create the wgpu device after `onSurfaceCreated` when the backend is GLES. On this emulator, Vulkan comes up and then hangs in `vkCreateImageView`; GLES is invisible until the surface exists.
- Pattern (b) only if Slint owns the activity. That fights stock GameActivity.
- Luau can cross-compile for both Android ABIs. `eustress-common` needs an explicit `bevy/android-game-activity` (or `native-activity`) feature on Android. iOS stays UNVERIFIED until a Mac with Xcode (`xcrun`) exists.
- `ui.rs`, `touch.rs`, and `gestures.rs` are the old stub and are not in the module tree. Replace them when input is real; the spike forwards pointer hits on the quad from `lib.rs` only.
