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
reconstructs the supplied front-view concept: a white ceramic plate suit over a
black graphite chassis, with a recessed Y visor in xenon blue. The concept does
not show the back, which carries the ion thruster pack from the V-Supreme
product specification.

`voltec_forge.py` builds the armour around the Y Bot skeleton. Only the skeleton
is kept. The arm and leg chains are moved outward so the armour clears the
torso; this translates the bones without rotating them, so the baked motion is
unchanged. After baking, corrections fit the motion to the armour in every clip.
Each upper arm is held 12 degrees further from the body, which keeps the
running forearm clear of the breastplate. No knee folds past 115 degrees and no
elbow past 95: the Mixamo run kicks each heel to within 30 degrees of the thigh
and folds the elbows to 120, where the calf would sink into the thigh and the
forearm into the upper arm. Each forearm turns its palm 55 degrees toward the
back and the fingers curl into claws, as the concept holds its hands; with the
palms against the thighs, the claws would dig into the thigh plates.

The idle then settles into the concept's heavy stance: the arms hang 3.5 degrees
wider, the elbows bend a further 16 degrees, and the legs crouch. Each leg folds
in its own plane, the hips drop by the height the fold takes off the legs, and
a small outward swing at each hip returns the foot to its mark, so the feet stay
within a few millimetres of where the clip planted them. The walk keeps 12
degrees of the extra elbow bend, so the two blend without the arms snapping
straight. The torso armour is laid out against the source height of the hips
and lowered with them.

Proportions are measured off the concept against its breastplate, which the
camera's perspective does not distort relative to the helmet and pauldrons at
the same depth. The helmet is 0.28 m wide, about three fifths of the breastplate,
and each pauldron under half of it.

Every piece is authored in the idle pose, in the frame of the bone that carries
it, then mapped back to bind space through that bone's inverse deformation.
Left and right pieces come from the same code in their own bone frames, so they
are exact mirror images although the idle clip leans slightly. Positions come
from bone heads only: the glTF importer invents bone tails, and the finger end
markers carry positions tens of centimetres from the finger.

Armour is swept from superellipse cross-sections with exponents near 2.3, which
gives rounded ceramic shells rather than boxes. Plates stand off the chassis and
curl their edges inward. Plates whose edges are not simple rings, such as the
breastplate, collar, codpiece and knees, are cut to outlines taken from the
concept's front view, and panel lines are shallow grooves cut by thin shells
built from each plate's own sections. Every cut is made before the rims are
bevelled, because the exact boolean solver returns an empty mesh on the
bevel's slivers. Where an edge must follow a joint instead, as at the elbow, the
loft's sections slant along the limb so the edge dips at the front and the rim
stays whole: the gauntlet's top and the upper arm plate's lower edge leave a
black crease the folding forearm swings into. The chassis runs continuously
from boot to collar, so a gap between plates always shows black structure, and
its joints swell as broad as the plates either side of them, so a limb's
outline runs on through each joint. Plates are weighted rigidly to one bone; the
chassis blends across the spine, neck, elbows, wrists and knees. The hip is a
ball centred on the joint under a white cap, and the thigh plates start below
it, so a rising thigh turns in place under the belt instead of sweeping through
it. A high collar frames the helmet up to its cheeks, open at the throat. The
exported mesh is joined into one primitive per material.

Ceramic and graphite carry packed, tiling albedo, roughness and normal maps
generated with numpy, so they export to glTF exactly as rendered. The ceramic is
pitted with dark grunge where its coating has worn, with scratches and hairline
cracks, and each piece starts the tile at its own offset so the marks do not
repeat from plate to plate. The graphite is black marble: a dark base with pale
veins swirling through it at two scales, its roughness and relief varying with
the veins and mottling so highlights break up instead of reading as moulded
plastic. A fully matte finish would turn it grey, since a dielectric's sheen
spread by a matte surface lifts black under broad light. Xenon blue is emissive
and marks what is live: the visor's deep well and the Y-shaped lens standing in
it, the five-cell status array under the visor, the thruster throats and the
pack status strip.

The review renders use AgX with the Punchy look. Plain AgX lifts deep shadows
and shows the black as grey, where the engine's Reinhard and TonyMcMapface
tonemappers keep it dark.

Run with Blender 4.4 from the repository root:

```powershell
blender --background --python eustress/crates/common/assets/characters/scripts/build_voltec.py
blender --background --python eustress/crates/common/assets/characters/scripts/render_voltec_details.py
blender --background --python eustress/crates/common/assets/characters/scripts/sync_web_avatars.py
python eustress/crates/common/assets/characters/scripts/validate_avatar_assets.py
```

`build_voltec.py` bakes the Mixamo motions with `mixamo_bake.py`, which uses
rest-relative rotations, then forges the armour and writes the model, packed
textures, source file, review images and runtime clips. It checks the deformed
mesh bounds at five poses in each animation and writes
`voltec_review/pose_bounds.json`, catching stretched geometry.

`sync_web_avatars.py` builds X/Y Bot web previews without changing their engine
meshes and produces `voltec_supreme_preview.glb` with 1K texture maps for the
website. Geometry, skinning and animations remain byte-identical; the
full-resolution `voltec_supreme.glb` stays outside `web/assets` and Pages. The
sync removes the obsolete full-size website copy and rejects previews at 24 MiB,
below the Pages 25 MiB per-file cap. The customizer resolves the native Voltec
path to the preview URL without changing saved descriptors. Run only
`web_avatar_preview.py` with Blender to refresh Voltec's web preview. The full
sync updates the three built-in catalog entries and preserves custom catalog
additions.

The validator checks weights, joint coverage, all four moving clips and exact
curve parity between Voltec's embedded clips and its runtime files. Before
joining meshes, the builder standardizes their active texture-coordinate layer
to `Surface UV`, and the validator checks nonzero UV coverage on ceramic faces in
the exported GLB, so the armour cannot silently sample a single texel of its
embedded maps.

`render_voltec_details.py` renders chest, back, abdomen, boot and underside
inspections, plus poses from walk, run and jump. Pass view names after `--` to
render a subset, for example `-- voltec_back_detail voltec_soles voltec_back_run`.
`-- voltec_shape` produces a neutral clay render for judging shape without the
surface maps.

`common/assets/characters/voltec_review/index.html` is a standalone interactive
review page. Serve the repository locally with `python -m http.server 8874
--bind 127.0.0.1`, then open
`http://127.0.0.1:8874/eustress/crates/common/assets/characters/voltec_review/index.html`.
It exercises the same packaged model-viewer library and preview GLBs; account
save/load belongs to the website's authenticated Avatar tab.

The repository's existing Mixamo model and animation licensing still applies
to the source and derived avatar assets. The preview library is locally hosted
@google/model-viewer 4.0.0 (Apache-2.0).
