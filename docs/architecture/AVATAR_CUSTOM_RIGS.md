# Avatar identities and custom humanoid rigs

Avatars use exactly three identity codes: **M**, **F**, **R**. Height and build
change physical proportions; they do not change identity. The defaults are
M → Mixamo Y Bot, F → Mixamo X Bot, R → Voltec Supreme.

The shared `eustress-avatar-schema` crate defines the descriptor, identity and
rig definition. Both native shells and the Leptos website use it. Version-one
descriptors keep their original body; selecting an identity upgrades them to v2.

## Website and account storage

The private Avatar tab on the owner's profile provides identity, character rig,
height, build and animation selectors. The model preview contains real embedded
Idle, Walk, Run and Jump clips. Save Avatar writes through authenticated
`PUT /api/avatar`; `GET /api/avatar` retrieves the descriptor for that account.
Storage uses a separate `avatar:<authenticated user id>` KV key and responses
are private/no-store. The endpoint rejects unknown identities, mismatched rigs,
invalid proportions, traversal paths and bodies larger than 32 KiB.

Studio Play and the Client request the saved descriptor using their current
session token and spawn through the same avatar runtime. A missing session or
failed request uses the existing default body; failures are logged. Fetches run
off the render thread and pending results are discarded when Play stops.
Changes appear at the next character spawn. Deploy the website assets and API
worker together, and package the native character assets with the engine.

`Export avatar` downloads the same descriptor for offline use. Set
`EUSTRESS_AVATAR_FILE` to that JSON file before launching Studio or the Client
to use it without an account request. For a quick local Voltec check, a file
containing the following also works because the remaining fields have defaults:

```json
{"schema_version":2,"identity":"R","base_body":"robot"}
```

## Install a custom rig

1. Export a skinned GLB with its bind-pose origin at the feet, Y up, Z forward.
   Use the Mixamo humanoid hierarchy, including finger joints, or define
   `bone_aliases` mapping exact node names to lowercase canonical bone names.
2. Supply four single-animation GLBs in idle/walk/run/jump order, each with its
   clip at `Animation0`. Clips must be authored for the mesh's bone rest frames
   and proportions. Name mapping is not arbitrary bind-pose retargeting.
3. Install mesh and clips under `common/assets/characters/` (or the packaged
   `assets/characters/` directory). Preserve paths in every runtime build.
4. Append a definition to the website's `assets/characters/rigs.json`, and put
   the preview GLB at the matching website path. The preview must contain named
   Idle/Walk/Run/Jump clips. The dropdown filters definitions by identity.

Example catalog entry:

```json
{
  "id": "industrial_robot",
  "label": "Industrial Robot",
  "identity": "R",
  "body_asset": "bundled://characters/industrial_robot.glb",
  "animations": [
    "bundled://characters/animations/industrial_idle.glb",
    "bundled://characters/animations/industrial_walking.glb",
    "bundled://characters/animations/industrial_running.glb",
    "bundled://characters/animations/industrial_jump.glb"
  ],
  "bone_aliases": [["Pelvis", "hips"], ["UpperArm_L", "leftarm"]]
}
```

Aliases must cover all nonstandard names used by the skeleton and clips, have
unique targets, and include fingers if those should animate. Standard Mixamo
names need no aliases. Non-humanoid creatures, arbitrary locomotion states and
automatic retargeting between different bind poses are outside this contract.
The runtime measures mesh bounds for height and scales the visual body with the
same authored height/build values used for its collider. Animation retargeting
uses private clip copies so multiple avatars cannot rewrite each other's data.

## Voltec source and rebuild

`common/assets/characters/voltec_supreme.blend` is the editable source. The model
is a reconstruction of the supplied front-view reference; unseen rear armor is
newly designed. The second art pass replaces primitive armor blobs with shaped,
tapered castings, a recessed Y visor, a formed chest with a planar branding face,
separate side armor, recessed fasteners, ribbed joint seals and armored boots.
Ceramic and carbon use packed albedo, roughness and tangent-normal textures.
Surface-conforming seams, chipped coating and lettering are part of the mesh.
The neckline has no raised collar tabs. The abdomen replaces the donor torso
with a continuous flexible housing and three graphite plates; upper-arm rods
and unsupported decorative lines have been removed. Seams are projected onto
the bind mesh and split at gaps instead of bridging empty space.
The breastplate sits close to the torso and has a continuous ceramic return
from its perimeter into the chest. Face and return share the same spine joint,
so the mounting remains closed as the torso moves.
The graphite housing below the breastplate is a single continuous mesh with
smoothly blended weights across all three spine joints. It replaces intersecting
rigid transition shells and removes the donor torso beneath the chest opening.
Close-up renders cover idle, walk, run and jump to inspect that seam in motion.
Armor islands have rigid weights; the donor underbody retains blended skinning.
The exported mesh is joined into material primitives to reduce draw calls.

Run with Blender 4.4 from the repository root:

```powershell
blender --background --python eustress/crates/common/assets/characters/scripts/build_voltec.py
blender --background --python eustress/crates/common/assets/characters/scripts/render_voltec_details.py
blender --background --python eustress/crates/common/assets/characters/scripts/sync_web_avatars.py
python eustress/crates/common/assets/characters/scripts/validate_avatar_assets.py
```

The first script uses `voltec_geometry.py` to rebuild the model, packed textures, source file, review images
and runtime clips. `mixamo_bake.py` transfers the existing FBX motions using
rest-relative rotations. The second builds X/Y Bot web previews without changing
their engine meshes and copies Voltec into the website. It updates the three
built-in catalog entries and preserves custom catalog additions.
The validator checks weights, joint coverage, all four moving clips and exact
curve parity between Voltec's embedded clips and its runtime files.
The build also checks the deformed mesh bounds at five poses in each animation
and writes `voltec_review/pose_bounds.json`, catching stretched geometry caused
by invalid donor terminal markers. The review includes front and three-quarter
renders; its browser controls support front/back views and pausing animation.

The torso uses a continuous flexible liner blended from hips through the spine,
with fitted abdominal and lumbar plates over it. A closed hip chassis supports
the groin and sacral armor. The boots have closed uppers, capped arched outsoles,
embedded support shanks, heel cups and modeled chevron traction pads. The closed
assembly builder checks that every edge is manifold before joining the rig mesh.
`render_voltec_details.py` renders back, abdomen, boot and illuminated underside
inspections, plus bent-pose checks. Pass view names after `--` to render a subset,
for example `-- voltec_back_detail voltec_soles voltec_back_run`.

`common/assets/characters/voltec_review/index.html` is a standalone interactive
review page. Serve the repository locally with `python -m http.server 8874
--bind 127.0.0.1`, then open
`http://127.0.0.1:8874/eustress/crates/common/assets/characters/voltec_review/index.html`.
It exercises the same packaged model-viewer library and preview GLBs; account
save/load belongs to the website's authenticated Avatar tab.

The repository's existing Mixamo model and animation licensing still applies
to the source and derived avatar assets. The preview library is locally hosted
@google/model-viewer 4.0.0 (Apache-2.0).
