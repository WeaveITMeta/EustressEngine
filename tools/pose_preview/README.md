# Pose preview

Renders the authored climb poses on the Y Bot without running the engine, so a
pose can be judged by eye before it ships.

```bash
python tools/pose_preview/climb_scenes.py
```

The pose tables are read straight out of
`eustress/crates/common/src/avatar/ik.rs`. Each scene places the body and a
wall, a bar or a block the way `climb.rs` places them, and runs the same
two-bone IK the engine runs for the hands and feet. The output folder
(`tools/pose_preview/out` by default) gets a back view and a side view of every
scene, and `sheet.png` with all of them.

- `--only NAME ...` renders only the scenes whose names start with those words,
  for example `--only mantle`.
- `BLENDER` names the Blender executable. The default is Blender 4.4 at its
  Windows install path.
- `EUSTRESS_IK_RS` names another copy of `ik.rs`, to preview poses that are not
  in the tree yet.

The scenes are braced and free hangs, a free hang mid-swing, the gather before
a leap, the three keys of a pull-up (the pull, the knee on the lip for either
leg, the stand) and the two keys of a vault. The layers laid over one another
in each scene are the ones the engine lays at that moment.

## How a pose maps onto the rig

A pose gives the direction each bone points in the body frame: +X right, +Y
up, +Z back, so -Z points forward into the wall. The imported Y Bot faces -Y in
Blender with its left at +X, so body `(x, y, z)` is Blender `(-x, z, y)`.
Entries apply in list order, each aiming its bone so its child lies along the
given direction, which is what `apply_pose` does in the engine.
