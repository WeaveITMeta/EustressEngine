# Patched avian3d 0.7.0

A copy of avian3d 0.7.0 from crates.io with SIX changes. The workspace points at it from
`[patch.crates-io]` in `eustress/Cargo.toml`.

## What changed

1. `src/dynamics/solver/joint_graph/plugin.rs`, `on_disable_joint_collision` (the
   `On<Add, JointCollisionDisabled>` observer, lines 247 to 297 of the stock 0.7.0
   file): the contacts it removes are unlinked from their islands too.
2. `src/dynamics/solver/contact/mod.rs`, `ContactConstraint::solve`: when the solver
   estimates a contact's separation within a step, a contact point's turn with its body
   counts only where it takes the contact deeper.
3. `src/dynamics/integrator/mod.rs`, `solve_gyroscopic_torque`: two Newton steps of the
   implicit midpoint rule replace semi-implicit Euler with the angular momentum's length
   held.
4. `src/collision/collider/parry/contact_query.rs`, `contact_manifolds`: a cuboid much
   larger than the shape it touches is queried by the part of it near that shape (a new
   `NearPart` and `near_part_of_cuboid` in the same file).
5. A collider attached by hand to a body that is not its ancestor (Eustress merges each
   weld group into one body this way): a `ManualColliderTransform` marker
   (`src/collision/collider/collider_transform/mod.rs`, in the prelude), and in
   `collider_transform/plugin.rs`, `collider_hierarchy/mod.rs` and
   `src/dynamics/rigid_body/mass_properties/system_param.rs` such a collider is placed,
   weighed and released.
6. Mesh colliders wait until `Assets<Mesh>` exists. `clear_unused_colliders` runs only
   when `AssetEvent<Mesh>` is registered, and the two constructor systems take
   `Option<Res<Assets<Mesh>>>`. Mobile adds `MeshPlugin` from `SurfaceGate::finish`,
   after physics plugins have already built.

Nothing else differs from the published crate. Changes 2 to 4 were measured together in
headless labs (rustc only, against this crate, 60 Hz, 6 substeps, the engine's settings);
the numbers below come from them.

## 1. Removed contacts left linked in their islands

When `JointCollisionDisabled` is added to a joint, the stock observer removes every
contact between the joint's two bodies from the `ContactGraph` and the `ConstraintGraph`
but never from `PhysicsIslands`. It takes no islands parameter. The island's contact list
keeps an id that no longer exists, and the next contact that joins or leaves the island
unwraps it: `islands/mod.rs` panics in `add_contact` (the head contact) or in
`remove_contact` (a neighbouring contact).

It needs a touching contact that is already linked into an island between the two bodies
(`ContactEdge::island` is `Some`), so it appears once the bodies have been simulated
together for a few steps. A welded car crashed the engine a second after it was built.

The patch unlinks each contact from its island at the same point and in the same order as
Avian's own removal paths in the narrow phase (pop the manifolds from the constraint
graph, then `PhysicsIslands::remove_contact`, then remove the edge), and wakes the island
if it was asleep, as the narrow phase does when a pair stops touching. The islands
resource stays optional, so it works when islands are off.

Upstream: fixed on `main` by a redesign, not a small change: avianphysics/avian #989,
"Decouple contacts and solver" (commit 747a7e8c, 2026-07-19). The observer there records a
`ContactStatusChange::StoppedGeneratingConstraints`, and the solver plugin removes the
contact's constraints and unlinks it from its island (`src/dynamics/solver/plugin.rs`).
That code depends on the new status queue, so it cannot be mirrored into 0.7.0 as a few
lines; this patch has the same effect with 0.7.0's own calls. Reported as
avianphysics/avian #1076.

## 2. Fast-spinning round bodies lose their contacts within a step

The solver applies contact impulses at fixed anchors (the contact point's offset from each
body's centre, in world axes, at the start of the step), which is right for rolling. But
to estimate how far apart the bodies are at each substep it turned those anchors by each
body's whole rotation since the step began:

```rust
let r1 = body1.delta_rotation * point.anchor1;
let r2 = body2.delta_rotation * point.anchor2;
let delta_separation = delta_translation + (r2 - r1);
let separation = delta_separation.dot(self.normal) + point.initial_separation;
```

On a round body that carries the contact point around the body, although the body still
touches at the same place: the point at the bottom of a wheel turning by an angle θ
climbs `R (1 - cos θ)` along the normal. A 0.33 m wheel at 30 m/s turns 87 degrees in a
60 Hz step, so after its first substep the solver reads it as lifted off the road. The
normal constraint lets it go, the wheel has no normal impulse for the rest of the step,
and friction, which is bounded by the normal impulse, has nothing to work with. The next
step's narrow phase finds the wheel sunk into the road and pushes it out.

The patch counts the turn only where it takes the contact deeper:
`separation = (delta_translation + anchor2 - anchor1) . n + min(0, (turned2 - turned1) . n)
+ initial_separation`, with `turned = delta_rotation * anchor - anchor`. A round body's
contact point lies along the normal from its centre, so its turn only ever lifts it, and
that no longer counts; a corner turning into the ground still counts in full. Two other
rules were measured and kept the bodies on the road as well: the turn to first order (the
rotation vector crossed with the anchor) and its tangential part alone. Over 48 boxes
thrown tumbling, the energy each kept through its first impact averaged 0.258 with this
rule, 0.256 with the stock rule, 0.266 and 0.270 with the other two, and 0.23 to 0.26 at
600 and 1200 Hz, where every rule agrees: the same within the spread between throws.

| | stock | patched |
|---|---|---|
| ball rolled at 30 m/s: centre (radius 0.33) | 0.3167 m, 29.82 m/s after 5 s | 0.3300 m, 30.00 m/s |
| ball spun at 200 rad/s on the spot: lowest centre | 0.2803 m | 0.3294 m |
| cylinder wheel rolled at 30 m/s: centre | 0.3044 to 0.3305 m | 0.3300 m |
| ten-box tower over 10 s: drift, worst speed | 0.0000 m, 0.0151 m/s | 0.0000 m, 0.0085 m/s |
| tall box tipped over an edge, 20 starts: deepest corner | 4.07 cm | 4.07 cm |
| Coupe (four ball wheels, 1515 kg): top speed | 44.6 m/s, 23 % wheel slip | 53.1 m/s, 2.6 % |
| Coupe on cylinder wheels: top speed | spins out | 52.4 m/s, 3.5 % |

Upstream: `main` still turns the anchors by the full delta rotation (checked 2026-10-03,
`src/dynamics/solver/contact/mod.rs`). Not yet reported.

## 3. Gyroscopic motion gains or loses energy

`solve_gyroscopic_torque` stepped the body-frame angular momentum by semi-implicit Euler
and rescaled it to its old length, after Jolt. That keeps `|L|` but not the energy,
`L . I^-1 L / 2`, so each step's error carries a spin off the axis it is on. A free 0.33 m
cylinder wheel spun about its axle (its axis of most inertia, where a spin is stable) at
91 rad/s with a 0.5 rad/s wobble ended tumbling at 150 rad/s off its axle, with 90 percent
more energy. A 2 by 0.5 by 1 m box spun about its axis of most inertia gained 18 percent
and flipped; about its axis of least inertia (also stable) it lost 70 percent.

The patch takes two Newton steps of the implicit midpoint rule in the body frame,
`I (w2 - w1) + h w_m x I w_m = 0` with `w_m = (w1 + w2) / 2`. The midpoint rule keeps
every quadratic invariant of a free spin, the energy and `|L|` both, so spins about the
axes of least and most inertia stay put and a spin about the middle axis flips over and
back as it should (the Dzhanibekov effect). Should the two steps land on more energy than
the spin had, the result is scaled back to it; a Jacobian close to singular (a spin about
the middle axis outrunning the step, 1,200 rad/s for that box at 360 Hz) keeps the spin as
it is. The singularity check is relative to the inertia's own determinant, so it holds at
any size.

Over 5 s, for that box (energy at the end against the start):

| spun about | stock | implicit Euler, one step | patched |
|---|---|---|---|
| least inertia, 100 rad/s | -70.3 % | -0.3 % | -0.02 % |
| most inertia, 100 rad/s | +17.6 %, flips | -0.08 % | -0.02 % |
| middle, 20 rad/s | -0.05 %, 4 flips | -22 %, 4 flips | -0.02 %, 4 flips |
| middle, 100 rad/s | -0.05 %, 16 flips | -93 %, 6 flips | -0.03 %, 19 flips |
| middle, 1000 rad/s | -0.08 % | +554 % (blew up) | -2.5 %, 214 flips |

The cost, on 20,000 tumbling boxes (the step runs once per body per substep): integrating
velocities took 3.67 ms a frame with the stock step and 4.34 ms with the patched one
(0.67 ms more, about 5.6 ns per body per substep); whole frames 17.5 and 18.4 ms.

Upstream: `main` has the stock step (checked 2026-10-03, `src/dynamics/integrator/mod.rs`).
Not yet reported.

## 4. Noisy contact normals against large cuboids

Parry finds the contacts between a cuboid and a cylinder, cone, capsule or convex hull
(Eustress's wedges included) with GJK and EPA, and GJK's support points are the cuboid's
corners. On a ground 4 km across those sit 2 km from the contact, and in `f32` the
direction of a sub-millimetre gap measured from them is noise. A cylinder wheel resting
on such a ground read normals up to 32 degrees off vertical, one pointing sideways, and
penetrations of 6 m. With change 2 the wheel rests exactly on the road, at the gap where
this is worst, and was thrown 6 m into the air; stock hid it by sinking the wheel 2 cm,
where EPA's normals are sound. A 400 m ground did the same. Wedges sliding on such a
ground veered 12.7 m aside; resting, they crept 6.7 cm in 10 s.

The patch queries such a cuboid by the part of it near the other shape. Over the middle of
a face it is that face's half-space, which parry solves exactly: the other shape's support
feature against a plane. That gives the cuboid's own contacts when the shape lies over the
face, keeps clear of the side faces by more than it is deep, and is no deeper than the
cuboid's mid-plane, since then no edge or side face is within reach and the face is the
shortest way out. Near an edge or a corner the cuboid is cut down to the shape's
neighbourhood: the cut's own faces are farther from the shape than the prediction
distance and than any way out through a face kept whole, so the contacts are the same,
measured from corners a few metres away. A cuboid qualifies only when some half-extent of
it exceeds four times the sum of the shape's size and the prediction distance. Cuboid against ball,
cuboid or triangle has exact paths of its own in parry and is left alone, as are compound
shapes, meshes and height fields.

On a 4 km ground (stock, then patched): a wheel rolled at 30 m/s, centre 0.2941 m and
29.71 m/s after 5 s, then 0.3300 m and 30.00 m/s; a wedge slid at 20 m/s, 12.7 m aside
after 13.1 m, then 0.00001 m aside after 31.34 m (friction says 31.37 m); a wedge, a
capsule and an upright cylinder at rest for 10 s, drift 6.7 cm, 0.12 mm and 0.12 mm, then
none; a wheel rolled off the edge of a 40 m box rolls off and falls either way.

Upstream: Avian's `main` passes the shapes straight to parry (checked 2026-10-03,
`src/collision/collider/parry/contact_query.rs`). Not yet reported.

## 5. Colliders attached to a body by hand

Avian's docs say a collider can be attached to a body by inserting `ColliderOf` by hand.
Eustress does it to make a weld group one body: the group's heaviest part keeps its body,
and every other part keeps its own entity and collider (so a touch or a raycast still names
that part) but attaches the collider to the hub, its own body disabled. Stock 0.7.0 does
not finish the job in five places:

- `update_child_collider_position` moves a collider only when it has no `RigidBody` and
  its body has `Children`. A collider on a disabled body of its own, or on a body with no
  children, stays where it was put.
- `propagate_collider_transforms` rewrites the `ColliderTransform` of every collider in a
  hierarchy whenever its `Transform` changes, from the hierarchy, not from the body it was
  attached to.
- `MassPropertyHelper::total_mass_properties` sums the body and its descendants only. A
  1260 kg car merged on its 771 kg spine weighed 771 kg.
- `RigidBodyColliders` is a `linked_spawn` relationship target: despawning the body
  despawned every collider attached to it, here every other part of the car.
- A collider that leaves a body (its `ColliderOf` changed or removed) does not make that
  body recompute its mass: the spine went on weighing 1260 kg after its parts left.

The patch adds `ManualColliderTransform`, a marker for a collider whose `ColliderTransform`
its owner sets. `update_child_collider_position` places every collider that has no body of
its own or carries the marker (the bodies it reads are the ones without it, which keeps the
two queries disjoint), whether or not its body has children. Propagation leaves a marked
collider's transform alone. The mass helper adds the marked colliders in the body's
`RigidBodyColliders` that are not already its descendants. `RigidBodyColliders` is no
longer `linked_spawn` (a child collider still goes with its body as a child), and
`ColliderOf`'s discard hook queues `RecomputeMassProperties` on the body it leaves.

`ColliderOf`'s insert hook still sets the `ColliderTransform` from the two global
transforms, scale included, so a body with a non-uniform scale gives a wrong frame; the
owner inserts the transform after `ColliderOf`, as Eustress does.

Measured in a headless lab (rustc only, against this crate, 60 Hz, 6 substeps): the Coupe
(771 kg spine, 489 kg of other parts) and a 390 kg deck carrying 840 kg of parts, merged,
weigh 1260 kg and 1230 kg, and keep every part on the hub to 0.0 mm resting, driving,
dropped from 6 m and driven into a wall (a star of joints from the same hub: 7.3 mm and
32.8 mm into the wall). Split again, the hubs weigh 771 kg and 390 kg. Despawning a hub
leaves all 7 parts standing (stock: all 7 despawned).

Upstream: Avian's `main` has the same `update_child_collider_position` filters (checked
2026-10-04, `src/collision/collider/collider_transform/plugin.rs`). Not yet reported.

## Regression tests

`eustress/crates/play-runtime/src/joints.rs`, run with
`cargo test -p eustress-play-runtime --lib joints::`:

- `a_weld_between_touching_bodies_leaves_no_stale_contact_in_an_island` (1)
- `a_no_collision_joint_between_touching_bodies_leaves_no_stale_contact_in_an_island` (1)
- `the_marker_added_to_an_existing_joint_leaves_no_stale_contact_in_an_island` (1)
- `a_ball_rolling_at_speed_stays_on_the_road` (2)
- `a_ball_spun_on_the_road_grips_without_sinking` (2)
- `a_cylinder_wheel_rolling_on_a_wide_ground_neither_sinks_nor_hops` (2 and 4)
- `a_wedge_slides_straight_across_a_wide_ground` (4)
- `a_free_wheel_keeps_spinning_about_its_axle` (3)
- `a_box_spun_about_its_axis_of_most_inertia_stays_on_it` (3)
- `a_welded_pair_is_one_body_that_weighs_both` (5)
- `a_coupe_welded_through_its_seat_settles_stays_together_and_drives` (5)
- `anchoring_a_merged_part_splits_its_group` (5)
- `despawning_a_hub_leaves_its_merged_parts` (5)

The first three settle two stacked cubes into one island, add the joint with collisions
off, then drop a third cube on the stack; on stock 0.7.0 each panics in `islands/mod.rs`.
Each of the rest fails on stock 0.7.0, by the numbers above.

## When to drop it

Change 1: when the workspace moves to an avian3d release that contains #989 (the first
release after 0.7.0 that does). Changes 2 to 4: when a release has its own answer to each
(contact separation that does not carry anchors around round bodies, a gyroscopic step
that keeps the energy, contact normals that hold against large cuboids); until then,
carry them forward onto the new release. Change 5: when a release places, weighs and
releases colliders attached by hand. Then remove the `avian3d` line from
`[patch.crates-io]` if nothing is left to carry, and run the tests above on the stock
crate before landing.

## The diffs

```diff
--- a/src/dynamics/solver/joint_graph/plugin.rs
+++ b/src/dynamics/solver/joint_graph/plugin.rs
@@ -250,6 +250,10 @@
     joint_graph: Res<JointGraph>,
     mut contact_graph: ResMut<ContactGraph>,
     mut constraint_graph: ResMut<ConstraintGraph>,
+    // EUSTRESS PATCH: needed to unlink the removed contacts from their islands (see PATCH.md).
+    mut islands: Option<ResMut<PhysicsIslands>>,
+    mut body_islands: Query<&mut BodyIslandNode, Or<(With<Disabled>, Without<Disabled>)>>,
+    mut commands: Commands,
 ) {
     let entity = trigger.entity;
 
@@ -268,14 +272,14 @@
         (colliders2, body1)
     };
 
-    let contacts_to_remove: Vec<(ContactId, usize)> = colliders
+    let contacts_to_remove: Vec<(ContactId, usize, bool)> = colliders
         .iter()
         .flat_map(|collider| {
             contact_graph
                 .contact_edges_with(collider)
                 .filter_map(|edge| {
                     if edge.body1 == Some(other_body) || edge.body2 == Some(other_body) {
-                        Some((edge.id, edge.constraint_handles.len()))
+                        Some((edge.id, edge.constraint_handles.len(), edge.island.is_some()))
                     } else {
                         None
                     }
@@ -283,16 +287,40 @@
         })
         .collect();
 
-    for (contact_id, num_constraints) in contacts_to_remove {
+    let mut islands_to_wake: Vec<IslandId> = Vec::new();
+
+    for (contact_id, num_constraints, has_island) in contacts_to_remove {
         // Remove the contact from the constraint graph.
         for _ in 0..num_constraints {
             constraint_graph.pop_manifold(&mut contact_graph.edges, contact_id, body1, body2);
         }
 
+        // EUSTRESS PATCH: unlink the contact from its island, as the narrow phase does
+        // when a pair stops touching. Without this the island's contact list keeps an id
+        // that no longer exists, and the island's next contact change unwraps it
+        // (`islands/mod.rs`, `add_contact` and `remove_contact`).
+        if has_island && let Some(islands) = &mut islands {
+            let island = islands.remove_contact(
+                contact_id,
+                &mut body_islands,
+                &mut contact_graph.edges,
+                &joint_graph,
+            );
+
+            // Wake the island if it was sleeping, as the narrow phase does.
+            if island.is_sleeping {
+                islands_to_wake.push(island.id);
+            }
+        }
+
         // Remove the contact from the contact graph.
         let pair_key = PairKey::new(body1.index_u32(), body2.index_u32());
         contact_graph.remove_edge_by_id(&pair_key, contact_id);
     }
+
+    if !islands_to_wake.is_empty() {
+        commands.queue(WakeIslands(islands_to_wake));
+    }
 }
 
 /// Update the joint graph when the entities of a joint change.
```

```diff
--- a/src/dynamics/solver/contact/mod.rs
+++ b/src/dynamics/solver/contact/mod.rs
@@ -283,12 +283,23 @@ impl ContactConstraint {
 
         // Normal impulses
         for point in self.points.iter_mut() {
-            let r1 = body1.delta_rotation * point.anchor1;
-            let r2 = body2.delta_rotation * point.anchor2;
-
-            // Compute current saparation.
-            let delta_separation = delta_translation + (r2 - r1);
-            let separation = delta_separation.dot(self.normal) + point.initial_separation;
+            // EUSTRESS PATCH (see PATCH.md): the anchors' turn with their bodies counts
+            // only where it takes the contact deeper. Turned by the whole rotation
+            // since the step began, a round body's contact point climbs around it,
+            // R(1 - cos θ) along the normal, although the body still touches at the
+            // same place: a wheel at 30 m/s turns 87 degrees in a 60 Hz step, reads as
+            // lifted off the road after its first substep, and loses its normal
+            // impulse, and with it its friction, for the rest of the step. Balls sank
+            // 1.3 cm rolling at 30 m/s and 5 cm spun in place, and cylinder wheels
+            // spun out. A corner that turns into the ground still counts in full.
+            let turned1 = body1.delta_rotation * point.anchor1 - point.anchor1;
+            let turned2 = body2.delta_rotation * point.anchor2 - point.anchor2;
+
+            // Compute current separation.
+            let delta_separation = delta_translation + (point.anchor2 - point.anchor1);
+            let separation = delta_separation.dot(self.normal)
+                + (turned2 - turned1).dot(self.normal).min(0.0)
+                + point.initial_separation;
 
             // Fixed anchors
             let r1 = point.anchor1;
```

```diff
--- a/src/dynamics/integrator/mod.rs
+++ b/src/dynamics/integrator/mod.rs
@@ -412,51 +412,58 @@ pub fn solve_gyroscopic_torque(
     // - Jolt Physics - MotionProperties::ApplyGyroscopicForceInternal
     //   https://github.com/jrouwe/JoltPhysics/blob/d497df2b9b0fa9aaf41295e1406079c23148232d/Jolt/Physics/Body/MotionProperties.inl#L102
     //
-    // Erin Catto's GDC presentation suggests using implicit Euler for gyroscopic torque,
-    // as semi-implicit Euler can easily blow up with larger time steps due to extrapolating velocity.
-    // The extrapolation diverges quickly because gyroscopic torque is quadratic in the angular velocity.
+    // EUSTRESS PATCH (see PATCH.md): the implicit midpoint rule, two Newton steps.
+    // The stock step was semi-implicit Euler with the angular momentum's length
+    // held: that keeps |L| but not the energy, so each step's error carries the
+    // spin off the axis it is on. A free 0.33 m wheel spun about its axle at
+    // 91 rad/s with a 0.5 rad/s wobble ended tumbling at 150 rad/s with 90 percent
+    // more energy; a 2 by 0.5 by 1 m box spun about its axis of most inertia gained
+    // 18 percent and flipped, and about its axis of least inertia lost 70 percent.
+    // The midpoint rule keeps every quadratic invariant of a free spin, the energy
+    // and |L| both: spins about the axes of least and most inertia stay put, and
+    // one about the middle axis flips over and back as it should (the Dzhanibekov
+    // effect), within 0.1 percent of its energy over 5 s up to 520 rad/s.
     //
-    // However, implicit Euler is measurably more expensive than semi-implicit Euler.
-    // We instead take inspiration from Jolt, and use semi-implicit Euler integration,
-    // clamping the magnitude of the angular momentum to remain the same.
-    // This is efficient, prevents energy from being introduced into the system,
-    // and produces reasonably accurate results for game purposes.
-
-    // Convert angular velocity to body space so that we can use the local angular inertia.
-    let local_ang_vel = rotation.inverse() * *ang_vel;
-
-    // Compute body-space angular momentum.
-    let local_momentum = local_inertia.tensor() * local_ang_vel;
-
-    // The gyroscopic torque is given by:
-    //
-    // T = -ω x I ω = -ω x L
-    //
-    // where ω is the angular velocity, I is the angular inertia tensor,
-    // and L is the angular momentum.
-    //
-    // The change in angular momentum is given by:
-    //
-    // ΔL = T Δt = -ω x L Δt
-    //
-    // Thus, we can compute the new angular momentum as:
-    //
-    // L' = L + ΔL = L - Δt (ω x L)
-    let mut new_local_momentum = local_momentum - delta_secs * local_ang_vel.cross(local_momentum);
-
-    // Make sure the magnitude of the angular momentum remains the same to avoid introducing
-    // energy into the system due to the extrapolation done by semi-implicit Euler integration.
-    let new_local_momentum_length_squared = new_local_momentum.length_squared();
-    if new_local_momentum_length_squared == 0.0 {
-        *ang_vel = Vector::ZERO;
+    // In the body frame, with I constant there and w_m = (w1 + w2) / 2:
+    //   f(w2) = I (w2 - w1) + h w_m x I w_m = 0
+    //   J     = I + h/2 (skew(w_m) I - skew(I w_m))
+    let w1 = rotation.inverse() * *ang_vel;
+    let inertia = local_inertia.tensor().to_mat3();
+    let skew = |v: Vector| {
+        Matrix::from_cols(
+            Vector::new(0.0, v.z, -v.y),
+            Vector::new(-v.z, 0.0, v.x),
+            Vector::new(v.y, -v.x, 0.0),
+        )
+    };
+    let mut w2 = w1;
+    for _ in 0..2 {
+        let mid = (w1 + w2) * 0.5;
+        let mid_momentum = inertia * mid;
+        let residual = inertia * (w2 - w1) + mid.cross(mid_momentum) * delta_secs;
+        let jacobian = inertia + (skew(mid) * inertia - skew(mid_momentum)) * (0.5 * delta_secs);
+        // J turns singular only when a spin about the middle axis outruns the step
+        // (at 1,200 rad/s for that box at 360 Hz). Measured against the inertia's
+        // own determinant the check holds at any size; near it, the step keeps
+        // what it has.
+        if !(jacobian.determinant() > 0.25 * inertia.determinant()) {
+            break;
+        }
+        w2 -= jacobian.inverse() * residual;
+    }
+    // Two Newton steps are not the exact solution: should they land on more
+    // energy than the spin had, the result is scaled back to it.
+    let energy1 = w1.dot(inertia * w1);
+    let energy2 = w2.dot(inertia * w2);
+    if energy2 > energy1 && energy2 > 0.0 {
+        w2 *= (energy1 / energy2).sqrt();
+    }
+    if !w2.is_finite() {
         return;
     }
-    new_local_momentum *=
-        (local_momentum.length_squared() / new_local_momentum_length_squared).sqrt();
 
     // Convert back to world-space angular velocity.
-    let local_inverse_inertia = local_inertia.inverse();
-    *ang_vel = rotation * (local_inverse_inertia * new_local_momentum);
+    *ang_vel = rotation * w2;
 }
 
 // NOTE: If the majority of bodies have clamped velocities, it would be more efficient
```

```diff
--- a/src/collision/collider/parry/contact_query.rs
+++ b/src/collision/collider/parry/contact_query.rs
@@ -171,13 +171,42 @@ pub fn contact_manifolds(
     let isometry2 = make_pose(position2, rotation2);
     let isometry12 = isometry1.inv_mul(&isometry2);
 
+    // EUSTRESS PATCH (see PATCH.md): a cuboid much larger than the shape it touches
+    // goes to the query as the part of it near that shape.
+    let shape1 = collider1.shape_scaled().0.as_ref();
+    let shape2 = collider2.shape_scaled().0.as_ref();
+    let near1 = shape1
+        .as_cuboid()
+        .and_then(|cuboid| near_part_of_cuboid(cuboid, shape2, &isometry12, prediction_distance));
+    let near2 = match near1 {
+        Some(_) => None,
+        None => shape2.as_cuboid().and_then(|cuboid| {
+            near_part_of_cuboid(cuboid, shape1, &isometry12.inverse(), prediction_distance)
+        }),
+    };
+    let (query_shape1, query_shape2, query_pose12, near_pose1) = match (&near1, &near2) {
+        (Some((part, at)), _) => (
+            part.shape(),
+            shape2,
+            parry::math::Pose::from_translation(-*at) * isometry12,
+            Some(parry::math::Pose::from_translation(*at)),
+        ),
+        (None, Some((part, at))) => (
+            shape1,
+            part.shape(),
+            isometry12 * parry::math::Pose::from_translation(*at),
+            None,
+        ),
+        (None, None) => (shape1, shape2, isometry12, None),
+    };
+
     // TODO: Reuse manifolds from previous frame to improve performance
     let mut new_manifolds =
         Vec::<parry::query::ContactManifold<(), ()>>::with_capacity(manifolds.len());
     let result = parry::query::DefaultQueryDispatcher.contact_manifolds(
-        &isometry12,
-        collider1.shape_scaled().0.as_ref(),
-        collider2.shape_scaled().0.as_ref(),
+        &query_pose12,
+        query_shape1,
+        query_shape2,
         prediction_distance,
         &mut new_manifolds,
         &mut None,
@@ -230,7 +259,8 @@ pub fn contact_manifolds(
             return None;
         }
 
-        let subpos1 = manifold.subshape_pos1.unwrap_or_default();
+        // EUSTRESS PATCH (see PATCH.md): points on a first cuboid's near part are in the part's frame.
+        let subpos1 = manifold.subshape_pos1.or(near_pose1).unwrap_or_default();
         let local_normal: Vector = (subpos1.rotation * manifold.local_n1).normalize();
         let normal = rotation1 * local_normal;
 
@@ -256,6 +286,126 @@ pub fn contact_manifolds(
     }));
 }
 
+/// EUSTRESS PATCH (see PATCH.md): the part of a large cuboid that a contact query
+/// against a much smaller shape is made with.
+enum NearPart {
+    /// The half-space bounded by the face the shape is over.
+    Face(parry::shape::HalfSpace),
+    /// The cuboid cut down to the shape's neighbourhood, near an edge or a corner.
+    Cut(parry::shape::Cuboid),
+}
+
+impl NearPart {
+    fn shape(&self) -> &dyn parry::shape::Shape {
+        match self {
+            Self::Face(half_space) => half_space,
+            Self::Cut(cuboid) => cuboid,
+        }
+    }
+}
+
+/// EUSTRESS PATCH (see PATCH.md): the part of `cuboid` near `other`, and where it
+/// sits in the cuboid's frame, or `None` to query the whole cuboid.
+///
+/// Parry finds the contacts between a cuboid and a cylinder, cone, capsule or
+/// convex hull with GJK and EPA, whose support points are the cuboid's corners. On
+/// a ground 4 km across those sit 2 km from the contact, and in `f32` the normal
+/// of a sub-millimetre gap measured from them is noise: a wheel resting on such a
+/// ground read a normal 32 degrees off vertical and was thrown 6 m into the air.
+/// Cutting the cuboid down to a few metres only shrinks the noise, because a shape
+/// resting on a face is exactly the case where GJK divides by a vanishing gap.
+///
+/// So over the middle of a face, the query is made with that face's half-space,
+/// which parry solves exactly: the other shape's support feature against a plane.
+/// It gives the cuboid's own contacts when the shape lies over the face, keeps
+/// clear of the side faces by more than it is deep, and is no deeper than the
+/// cuboid's mid-plane, since then no edge or side face is within reach and the
+/// face is the shortest way out. Near an edge or a corner the cuboid is cut down
+/// to the shape's neighbourhood instead: the cut's own faces are farther from the
+/// shape than the prediction distance and than any way out through a face kept
+/// whole, so the contacts are the same, measured from corners a few metres away.
+///
+/// Cuboid against ball, cuboid or triangle has exact paths of its own in parry
+/// and is left alone.
+fn near_part_of_cuboid(
+    cuboid: &parry::shape::Cuboid,
+    other: &dyn parry::shape::Shape,
+    other_in_cuboid: &parry::math::Pose,
+    prediction_distance: Scalar,
+) -> Option<(NearPart, Vector)> {
+    use parry::shape::ShapeType;
+
+    if !other.is_convex()
+        || matches!(
+            other.shape_type(),
+            ShapeType::Ball | ShapeType::Cuboid | ShapeType::Triangle | ShapeType::HalfSpace
+        )
+    {
+        return None;
+    }
+    let aabb = other.compute_aabb(other_in_cuboid);
+    let extent = (aabb.maxs - aabb.mins).max_element();
+    let half = cuboid.half_extents;
+    let near = extent + prediction_distance;
+    // Only a cuboid reaching far past the other shape is worth replacing.
+    let far_axes = half.cmpgt(Vector::splat(4.0 * near));
+    if !far_axes.any() || !near.is_finite() {
+        return None;
+    }
+
+    // The face the shape is over: of the faces it could be resolved through, the
+    // one it is least deep behind.
+    let mut face: Option<(usize, Scalar, Scalar)> = None;
+    for axis in 0..half.to_array().len() {
+        for side in [-1.0, 1.0] {
+            let deepest = if side > 0.0 {
+                aabb.mins[axis]
+            } else {
+                -aabb.maxs[axis]
+            };
+            let depth = half[axis] - deepest;
+            if depth > half[axis] {
+                continue;
+            }
+            let clearance = depth.max(0.0) + prediction_distance;
+            let over = (0..half.to_array().len()).all(|other_axis| {
+                other_axis == axis
+                    || (aabb.mins[other_axis] - clearance >= -half[other_axis]
+                        && aabb.maxs[other_axis] + clearance <= half[other_axis])
+            });
+            if over && face.is_none_or(|(_, _, least)| depth < least) {
+                face = Some((axis, side, depth));
+            }
+        }
+    }
+    if let Some((axis, side, _)) = face {
+        let mut normal = Vector::ZERO;
+        normal[axis] = side;
+        return Some((
+            NearPart::Face(parry::shape::HalfSpace::new(normal)),
+            normal * half[axis],
+        ));
+    }
+
+    // Near an edge or a corner: cut the far axes down to the shape's neighbourhood.
+    let kept_thickness = Vector::select(far_axes, Vector::ZERO, 2.0 * half).max_element();
+    let reach = near + extent + kept_thickness;
+    let lo = Vector::select(
+        far_axes,
+        (aabb.mins - Vector::splat(reach)).max(-half),
+        -half,
+    );
+    let hi = Vector::select(far_axes, (aabb.maxs + Vector::splat(reach)).min(half), half);
+    if hi.cmple(lo).any() {
+        // Out of reach of each other: the whole cuboid gives the same empty answer.
+        return None;
+    }
+    Some((
+        NearPart::Cut(parry::shape::Cuboid::new((hi - lo) * 0.5)),
+        (lo + hi) * 0.5,
+    ))
+}
+
 /// Information about the closest points between two [`Collider`]s.
 ///
 /// The closest points can be computed using [`closest_points`].
--- a/src/collision/mod.rs
+++ b/src/collision/mod.rs
@@ -87,7 +87,7 @@
         ContactManifoldContext, IntoCollider, LayerMask, PhysicsLayer, ScalableCollider, Sensor,
         SimpleCollider,
         collider_hierarchy::{ColliderHierarchyPlugin, ColliderOf, RigidBodyColliders},
-        collider_transform::{ColliderTransform, ColliderTransformPlugin},
+        collider_transform::{ColliderTransform, ColliderTransformPlugin, ManualColliderTransform},
     };
     #[cfg(all(
         feature = "default-collider",
--- a/src/collision/collider/collider_transform/mod.rs
+++ b/src/collision/collider/collider_transform/mod.rs
@@ -7,6 +7,17 @@
 use crate::prelude::*;
 use bevy::prelude::*;
 
+/// Marks a collider whose [`ColliderTransform`] is set by hand: transform
+/// propagation leaves it alone.
+///
+/// With a [`ColliderOf`] inserted by hand as well, it attaches the collider to
+/// a rigid body that is not its ancestor, even one with a [`RigidBody`] of its
+/// own (kept disabled with [`RigidBodyDisabled`]): every step the collider is
+/// placed at its `ColliderTransform` in that body's frame, and it weighs in the
+/// body's mass properties. Its own `Transform` is left to its owner.
+#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
+pub struct ManualColliderTransform;
+
 /// The transform of a collider relative to the rigid body it's attached to.
 /// This is in the local space of the body, not the collider itself.
 ///
--- a/src/collision/collider/collider_transform/plugin.rs
+++ b/src/collision/collider/collider_transform/plugin.rs
@@ -63,6 +63,10 @@
     }
 }
 
+/// Places every collider that is not its own body's at its [`ColliderTransform`]
+/// in that body's frame: a child collider, or one attached by hand
+/// ([`ManualColliderTransform`]), which need not be the body's descendant and
+/// may be a disabled body itself.
 #[allow(clippy::type_complexity)]
 pub(crate) fn update_child_collider_position(
     mut collider_query: Query<
@@ -72,9 +76,9 @@
             &mut Rotation,
             &ColliderOf,
         ),
-        Without<RigidBody>,
+        Or<(Without<RigidBody>, With<ManualColliderTransform>)>,
     >,
-    rb_query: Query<(&Position, &Rotation), (With<RigidBody>, With<Children>)>,
+    rb_query: Query<(&Position, &Rotation), (With<RigidBody>, Without<ManualColliderTransform>)>,
 ) {
     for (collider_transform, mut position, mut rotation, collider_of) in &mut collider_query {
         let Ok((rb_pos, rb_rot)) = rb_query.get(collider_of.body) else {
@@ -115,6 +119,7 @@
             Ref<Transform>,
             Option<&mut ColliderTransform>,
             Option<&Children>,
+            Has<ManualColliderTransform>,
         ),
         (With<ChildOf>, ShouldPropagate),
     >,
@@ -189,6 +194,7 @@
             Ref<Transform>,
             Option<&mut ColliderTransform>,
             Option<&Children>,
+            Has<ManualColliderTransform>,
         ),
         (With<ChildOf>, ShouldPropagate),
     >,
@@ -223,7 +229,7 @@
         //
         // Even if these A and B start two separate tasks running in parallel, one of them will panic before attempting
         // to mutably access E.
-        let Ok((transform_ref, collider_transform, children)) =
+        let Ok((transform_ref, collider_transform, children, manual)) =
             (unsafe { collider_query.get_unchecked(entity) })
         else {
             return;
@@ -231,7 +237,9 @@
 
         changed |=
             transform_ref.is_changed() || collider_transform.as_ref().is_some_and(|t| t.is_added());
+        // A collider attached by hand keeps the transform its owner gave it.
         if changed
+            && !manual
             && let Some(mut collider_transform) = collider_transform
             && *collider_transform != transform
         {
--- a/src/collision/collider/collider_hierarchy/mod.rs
+++ b/src/collision/collider/collider_hierarchy/mod.rs
@@ -5,6 +5,7 @@
 
 pub use plugin::ColliderHierarchyPlugin;
 
+use crate::dynamics::rigid_body::mass_properties::components::RecomputeMassProperties;
 use crate::prelude::*;
 use bevy::{
     ecs::{
@@ -168,6 +169,11 @@
             }
         }
         let body = world.entity(entity).get::<Self>().unwrap().get();
+        // The body weighs without the collider now: a collider moved to another
+        // body (or back to its own) would otherwise still count in this one.
+        if body != entity && world.get_entity(body).is_ok() {
+            world.commands().entity(body).try_insert(RecomputeMassProperties);
+        }
         if let Ok(mut body_mut) = world.get_entity_mut(body)
             && let Some(mut relationship_target) = body_mut.get_mut::<Self::RelationshipTarget>()
         {
@@ -203,9 +209,14 @@
 /// You should not modify this component directly to avoid desynchronization.
 /// Instead, modify the [`ColliderOf`] components on the colliders.
 ///
+/// Despawning the body does not despawn its colliders through this
+/// relationship: a child collider goes with it as a child, and a collider
+/// attached by hand ([`ManualColliderTransform`]) is an entity of its own that
+/// only loses its [`ColliderOf`].
+///
 /// [`Relationship`]: bevy::ecs::relationship::Relationship
 #[derive(Component, Clone, Debug, Default, PartialEq, Reflect)]
-#[relationship_target(relationship = ColliderOf, linked_spawn)]
+#[relationship_target(relationship = ColliderOf)]
 #[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
 #[cfg_attr(feature = "serialize", reflect(Serialize, Deserialize))]
 #[reflect(Debug, Component, Default, PartialEq)]
--- a/src/dynamics/rigid_body/mass_properties/system_param.rs
+++ b/src/dynamics/rigid_body/mass_properties/system_param.rs
@@ -50,6 +50,11 @@
         ),
     >,
     children: Query<'w, 's, Read<Children>>,
+    /// Colliders attached to a body by hand ([`ManualColliderTransform`]), which
+    /// need not be its descendants.
+    attached: Query<'w, 's, Read<RigidBodyColliders>>,
+    manual: Query<'w, 's, (), With<ManualColliderTransform>>,
+    parents: Query<'w, 's, Read<ChildOf>>,
 }
 
 impl MassPropertyHelper<'_, '_> {
@@ -144,16 +149,35 @@
                     .iter_descendants(entity)
                     .map(|child| self.local_mass_properties(child)),
             )
+            .chain(
+                self.attached_by_hand(entity)
+                    .map(|collider| self.local_mass_properties(collider)),
+            )
             .flatten()
             .sum()
     }
 
+    /// The colliders attached to `entity` by hand ([`ManualColliderTransform`]),
+    /// other than its own and its descendants (counted already).
+    fn attached_by_hand(&self, entity: Entity) -> impl Iterator<Item = Entity> + '_ {
+        self.attached
+            .get(entity)
+            .into_iter()
+            .flat_map(|colliders| colliders.iter())
+            .filter(move |&collider| {
+                collider != entity
+                    && self.manual.contains(collider)
+                    && !self.parents.iter_ancestors(collider).any(|ancestor| ancestor == entity)
+            })
+    }
+
     /// Computes the total mass properties of the descendants of the given entity.
     ///
     /// This ignores the [`NoAutoMass`], [`NoAutoAngularInertia`], and [`NoAutoCenterOfMass`] marker components.
     pub fn descendants_mass_properties(&self, entity: Entity) -> MassProperties {
         self.children
             .iter_descendants(entity)
+            .chain(self.attached_by_hand(entity))
             .filter_map(|child| self.local_mass_properties(child))
             .sum()
     }
```
