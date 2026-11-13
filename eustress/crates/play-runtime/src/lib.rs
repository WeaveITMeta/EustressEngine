//! # The shared Play runtime
//!
//! The draw side of a Play session, which Studio's Play and the Player both
//! run, so a joined player sees what the host sees
//! (`docs/architecture/SHARED_PLAY_RUNTIME.md`): the step that draws the
//! session's DataModel tree into the ECS, the scripted camera, sounds,
//! particles, part materials and part colliders. The simulating side too,
//! which Studio's Play and a Player playing alone both run: NPC humanoids,
//! seats, VehicleSeat input and scripted joints.

pub mod apply;
pub mod assemblies;
pub mod assembly_mass;
pub mod billboard_pipeline;
pub mod billboards;
pub mod body_easing;
pub mod camera;
pub mod crash_report;
pub mod gui_images;
pub mod hud;
pub mod hud_input;
pub mod joint_trace;
pub mod joints;
pub mod material_registry;
pub mod materials;
pub mod npc;
pub mod part_colliders;
pub mod part_mass;
pub mod particle_render;
pub mod particles;
pub mod pull;
pub mod screen_menu;
pub mod screen_notice;
pub mod seats;
pub mod sound_bridge;
pub mod sound_player;
pub mod space_paths;
pub mod vehicle_seat_input;

use bevy::prelude::*;
use eustress_common::play_session::{PlayDataModel, PlayRole, PlayScriptSet};

/// Everything a Play session draws, for an app that runs the shared runtime
/// whole: the Player, as a [`PlayRole::Replica`] when it joins a host and as
/// the [`PlayRole::Authority`] when it plays a world alone. It owns the order
/// of the frame's sets (`Pull`, `Scripts`, `Apply`, `End`, after the avatar
/// moves), draws the tree, and shows its camera, HUD, billboards, sounds and
/// particles. As the Authority it also pulls its own physics into the tree
/// (poses, `Touched`, assembly mass), drives NPC humanoids, decides who sits
/// where, reads a VehicleSeat's keys and builds scripted joints, as Studio's
/// Play does. Studio registers the same systems itself, around its own seed,
/// scripts and Output panel, and never adds this plugin.
pub struct PlayRuntimePlugin {
    pub role: PlayRole,
}

impl Plugin for PlayRuntimePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.role)
            .init_resource::<material_registry::MaterialRegistry>()
            .add_plugins((
                hud::HudPlugin,
                hud_input::HudInputPlugin,
                billboards::BillboardDrawPlugin,
                billboard_pipeline::BillboardPipelinePlugin,
                materials::MaterialSyncPlugin,
                particle_render::ParticleRenderPlugin,
                // What the Player says when something stops it.
                screen_menu::ScreenMenuPlugin,
                screen_notice::ScreenNoticePlugin,
            ))
            // Sounds: the player, and the session's side of it.
            .add_plugins((sound_player::SoundPlayerPlugin, sound_bridge::SoundBridgePlugin))
            .configure_sets(
                Update,
                (PlayScriptSet::Pull, PlayScriptSet::Scripts, PlayScriptSet::Apply, PlayScriptSet::End)
                    .chain()
                    .run_if(resource_exists::<PlayDataModel>),
            )
            // The avatar has moved for this frame before scripts read it.
            .configure_sets(Update, PlayScriptSet::Pull.after(eustress_common::avatar::AvatarSystems::Locomotion))
            .add_systems(Update, advance_play_clock.in_set(PlayScriptSet::Pull))
            // The Authority simulates, so its physics reaches the tree before
            // scripts read it, in Studio's order. A replica is sent the host's.
            .add_systems(
                Update,
                (
                    pull::pull_poses,
                    pull::pull_drawn_poses,
                    pull::pull_collisions,
                    assembly_mass::pull_assembly_mass,
                )
                    .chain()
                    .after(advance_play_clock)
                    .in_set(PlayScriptSet::Pull)
                    .run_if(simulates),
            )
            .add_systems(
                Update,
                (
                    apply::apply_frame,
                    camera::ensure_play_camera,
                    camera::show_script_camera_without_avatar,
                    camera::apply_scripted_camera,
                    particles::drive_particles,
                    log_play_output,
                )
                    .chain()
                    .in_set(PlayScriptSet::Apply),
            )
            // A part the host resized collides at its new size.
            .add_systems(
                Update,
                part_colliders::rebuild_collider_on_size_change.after(PlayScriptSet::Apply).before(PlayScriptSet::End),
            )
            .add_systems(Update, apply::end_frame.in_set(PlayScriptSet::End));
        add_simulated_characters(app);
        // Joints act only on bodies simulated here; a replica draws the poses
        // it is sent. The plugin brings part mass, body easing and weld groups
        // with it, and needs Avian's schedules, which the app added first.
        if self.role == PlayRole::Authority {
            app.add_plugins(joints::ScriptedJointsPlugin);
        }
    }
}

/// What only the machine that simulates the session runs, in Studio's sets
/// and order: NPC humanoids after the draw step, seating once this frame's
/// touches and the local character are in the tree, and each VehicleSeat's
/// keys after the pull and before any script. A replica's host does all of it
/// (`docs/networking/SEATS.md`), so each runs only for the Authority.
fn add_simulated_characters(app: &mut App) {
    app.init_resource::<npc::NpcControllers>()
        .add_plugins(seats::SeatPlugin)
        .configure_sets(Update, seats::SeatSystems::Touches.run_if(simulates))
        .configure_sets(
            Update,
            seats::SeatSystems::Seat
                .after(eustress_common::animation::character::sync_local_character)
                .run_if(simulates),
        )
        .add_systems(
            Update,
            npc::drive_npc_humanoids
                .in_set(PlayScriptSet::Apply)
                .after(apply::apply_frame)
                .before(camera::ensure_play_camera)
                .run_if(simulates),
        )
        .add_systems(
            Update,
            vehicle_seat_input::drive_vehicle_seats
                .after(PlayScriptSet::Pull)
                .before(PlayScriptSet::Scripts)
                .run_if(simulates),
        );
}

/// This app simulates the session: it plays it alone, as its Authority.
fn simulates(role: Res<PlayRole>) -> bool {
    *role == PlayRole::Authority
}

/// The session clock, which `time()`, the task scheduler, Heartbeat and the
/// animation tracks read, in either role. Studio's pull advances Studio's,
/// and Studio never adds this plugin.
fn advance_play_clock(dm: Option<Res<PlayDataModel>>, time: Res<Time>) {
    let Some(dm) = dm else { return };
    let mut g = dm.dm.lock();
    g.frame.dt = time.delta_secs_f64();
    g.frame.time += g.frame.dt;
    g.frame.frame += 1;
}

/// Script output to the log, on an app with no Output panel. Studio's
/// panel drains it there instead.
fn log_play_output(dm: Option<Res<PlayDataModel>>) {
    let Some(dm) = dm else { return };
    let lines = std::mem::take(&mut dm.dm.lock().output);
    for line in lines {
        match line.level {
            eustress_common::datamodel::OutputLevel::Error => warn!("[{}] {}", line.source, line.text),
            _ => info!("[{}] {}", line.source, line.text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use avian3d::prelude::{LinearVelocity, LockedAxes};
    use eustress_common::datamodel::{new_shared, DmValue, HumanoidCommand, InstanceId, SharedDataModel, SitRequest};
    use eustress_common::scripting::Vector3;

    /// The frame's sets and the simulating systems, as [`PlayRuntimePlugin`]
    /// registers them, for `role`, around a tree holding one NPC: a Model with
    /// a `HumanoidRootPart` bound to a body and a `Humanoid` walking 5 m/s.
    fn npc_session(role: PlayRole) -> (App, SharedDataModel, InstanceId, Entity) {
        let mut app = App::new();
        app.insert_resource(role).configure_sets(
            Update,
            (PlayScriptSet::Pull, PlayScriptSet::Scripts, PlayScriptSet::Apply, PlayScriptSet::End)
                .chain()
                .run_if(resource_exists::<PlayDataModel>),
        );
        add_simulated_characters(&mut app);
        let body = app.world_mut().spawn((Transform::default(), LinearVelocity::ZERO)).id();
        let tree = new_shared();
        let humanoid = {
            let mut g = tree.lock();
            let ws = g.get_service("Workspace").expect("Workspace");
            let model = g.create_virtual("Model", "Npc", Some(ws));
            g.create_bound("Part", "HumanoidRootPart", body.to_bits(), Some(model), Vec::new());
            let humanoid = g.create_virtual("Humanoid", "Humanoid", Some(model));
            g.set_prop(humanoid, "WalkSpeed", DmValue::Number(5.0)).unwrap();
            humanoid
        };
        app.insert_resource(PlayDataModel { dm: tree.clone() });
        (app, tree, humanoid, body)
    }

    /// The Authority walks an NPC on `Humanoid:MoveTo` and `Humanoid:Move`
    /// within the frame the script asked, upright.
    #[test]
    fn authority_drives_npc_humanoids() {
        let (mut app, tree, humanoid, body) = npc_session(PlayRole::Authority);
        tree.lock().humanoid_commands.push(HumanoidCommand::MoveTo { humanoid, target: Vector3::new(10.0, 0.0, 0.0) });
        app.update();
        let v = app.world().get::<LinearVelocity>(body).unwrap().0;
        assert!((v.x - 5.0).abs() < 1e-4 && v.z.abs() < 1e-4, "MoveTo walks toward the target at WalkSpeed: {v:?}");
        assert!(app.world().get::<LockedAxes>(body).is_some(), "the NPC stays upright");
        assert!(tree.lock().humanoid_commands.is_empty(), "the command was taken");

        tree.lock().humanoid_commands.push(HumanoidCommand::Move { humanoid, direction: Vector3::new(0.0, 0.0, -1.0) });
        app.update();
        let v = app.world().get::<LinearVelocity>(body).unwrap().0;
        assert!(v.x.abs() < 1e-4 && (v.z + 5.0).abs() < 1e-4, "Move walks that way: {v:?}");
        let moving = tree.lock().get_prop(humanoid, "MoveDirection").and_then(|v| v.as_vector3());
        assert_eq!(moving.map(|d| d.z), Some(-1.0), "MoveDirection reports it");
    }

    /// A joined Player is a replica: the host drives its NPCs, so neither
    /// command moves the body here, and both stay queued, unread.
    #[test]
    fn authority_drives_npc_a_replica_never_does() {
        let (mut app, tree, humanoid, body) = npc_session(PlayRole::Replica);
        tree.lock().humanoid_commands.push(HumanoidCommand::MoveTo { humanoid, target: Vector3::new(10.0, 0.0, 0.0) });
        tree.lock().humanoid_commands.push(HumanoidCommand::Move { humanoid, direction: Vector3::new(0.0, 0.0, -1.0) });
        app.update();
        app.update();
        assert_eq!(app.world().get::<LinearVelocity>(body).unwrap().0, Vec3::ZERO);
        assert!(app.world().get::<LockedAxes>(body).is_none());
        assert_eq!(tree.lock().humanoid_commands.len(), 2);
    }

    /// A Space with no avatar (The Super Station loads no character) is seen
    /// only through the play camera, solo or joined: one camera, active, and
    /// a Scriptable `CurrentCamera` moves it. Without it a solo Player draws
    /// its HUD over black.
    #[test]
    fn a_player_without_an_avatar_sees_through_the_scripted_camera_in_either_role() {
        for role in [PlayRole::Authority, PlayRole::Replica] {
            let mut app = App::new();
            let tree = new_shared();
            {
                let mut g = tree.lock();
                let ws = g.get_service("Workspace").unwrap();
                let cam = g.create_virtual("Camera", "Camera", Some(ws));
                g.set_prop(cam, "CameraType", DmValue::String("Enum.CameraType.Scriptable".into())).unwrap();
                let eye = Vector3::new(40.0, 34.0, 52.0);
                let cf = eustress_common::scripting::CFrame::look_at(eye, Vector3::new(0.0, 13.0, 0.0), None);
                g.set_prop(cam, "CFrame", DmValue::CFrame(cf)).unwrap();
            }
            app.insert_resource(role).insert_resource(PlayDataModel { dm: tree.clone() }).add_systems(
                Update,
                (camera::ensure_play_camera, camera::show_script_camera_without_avatar, camera::apply_scripted_camera)
                    .chain(),
            );
            app.update();
            app.update();
            let mut cams = app.world_mut().query_filtered::<(&Camera, &Transform), With<camera::ScriptedPlayCamera>>();
            let found: Vec<_> = cams.iter(app.world()).map(|(c, t)| (c.is_active, t.translation)).collect();
            assert_eq!(found.len(), 1, "{role:?}: exactly one play camera");
            let (active, at) = found[0];
            assert!(active, "{role:?}: the play camera is the one drawing");
            assert!(at.distance(Vec3::new(40.0, 34.0, 52.0)) < 1e-3, "{role:?}: the script placed it: {at:?}");
        }
    }

    /// `Seat:Sit` seats a character on the Authority and nowhere else.
    #[test]
    fn authority_seats_a_sit_request_a_replica_does_not() {
        for (role, seats) in [(PlayRole::Authority, true), (PlayRole::Replica, false)] {
            let (mut app, tree, humanoid, _) = npc_session(role);
            let seat = {
                let mut g = tree.lock();
                let ws = g.get_service("Workspace").unwrap();
                let seat = g.create("Seat");
                g.set_parent(seat, Some(ws)).unwrap();
                g.sit_requests.push(SitRequest { seat, humanoid });
                seat
            };
            app.update();
            let occupant = tree.lock().get_prop(seat, "Occupant").and_then(|v| v.as_instance());
            assert_eq!(occupant == Some(humanoid), seats, "{role:?}");
        }
    }
}
