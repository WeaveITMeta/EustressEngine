# Patched bevy_light 0.19.0

A copy of bevy_light 0.19.0 from crates.io with one change. The workspace points at it from `[patch.crates-io]` in `eustress/Cargo.toml`.

## What changed

`LightPlugin::build` adds `LightGizmoPlugin` whenever the `bevy_gizmos` feature is on. That plugin schedules `update_gizmo_meshes::<LightGizmoConfigGroup>`, which requires `Assets<GizmoAsset>`. Only `GizmoPlugin` inserts that asset storage.

The mobile Player disables `GizmoPlugin` (it does not draw editor gizmos) but still enables `bevy_gizmos` through `eustress-common`, so `LightPlugin` was registering the light gizmo systems anyway. The first frame then panicked: `ResMut<Assets<GizmoAsset>>` failed validation.

When `EUSTRESS_DISABLE_LIGHT_GIZMOS` is set, `LightPlugin` does not add `LightGizmoPlugin`. Nothing else differs from the published crate. Desktop and Studio leave the variable unset.

## Why

Feature unification means the mobile crate cannot turn `bevy_gizmos` off for `bevy_light` while `eustress-common` turns it on. A Cargo feature on this patch would also unify into Studio when the workspace is built together. The environment variable is set only by `player-mobile`'s `build_game`.
