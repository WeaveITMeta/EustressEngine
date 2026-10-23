# Patched android-activity 0.6.1

A copy of android-activity 0.6.1 from crates.io with one change. The workspace points at it from `[patch.crates-io]` in `eustress/Cargo.toml`.

`games-activity` 4.4.0 stays. That Java library matches this crate's native glue. A newer android-activity would need a different games-activity, so this stays on 0.6.1.

## What changed

`map_input_state_to_text_event_callback` built a slice from `GameTextInputState.text_UTF8` with `slice::from_raw_parts`. GameTextInput leaves that pointer null until the field has text. The first read, on the frame a `TextInput` focuses and `showSoftInput` runs, aborted:

`slice::from_raw_parts` precondition violated (null pointer), `android-activity-0.6.1/src/game_activity/mod.rs:717`.

A null pointer or a length of 0 is now an empty slice. The decoded text is `""`. Nothing else in the crate changed.

## Why

The caller cannot catch this. The abort is inside the crate, on the native thread, before `TextInputState` is returned. Polling `text_input_state` is how the Player reads GameActivity's text buffer: winit drops the text event after clearing the dirty flag.
