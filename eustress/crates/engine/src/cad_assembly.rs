//! CAD Assemblies — mates that map onto Avian joints.
//!
//! Fusion-style assembly constraints expressed as ECS components that
//! spawn / update real physics joints so assemblies articulate live in
//! the viewport (CAD_PLATFORM_PLAN Phase D differentiator).
//!
//! ## Mate kinds (v0)
//!
//! | Mate            | Avian joint        |
//! |-----------------|--------------------|
//! | Coincident      | FixedJoint (weld)  |
//! | Revolute / Hinge| RevoluteJoint      |
//! | Prismatic       | PrismaticJoint     |
//! | Ball            | SphericalJoint     |
//! | Distance        | DistanceJoint      |
//!
//! Insert via Drafting ribbon or `CadCreateMateEvent`.

use bevy::prelude::*;
use avian3d::prelude::{
    DistanceJoint, FixedJoint, PrismaticJoint, RevoluteJoint, SphericalJoint,
};

use eustress_common::classes::{ClassName, Instance};

use crate::notifications::NotificationManager;
use crate::selection_sync::SelectionSyncManager;
use crate::ui::MenuActionEvent;

// ============================================================================
// Components
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MateKind {
    /// Lock two parts rigidly (weld).
    Coincident,
    /// Hinge about an axis (default Y).
    Revolute,
    /// Slider along an axis (default X).
    Prismatic,
    /// Ball joint (3 rotational DOF).
    Ball,
    /// Maintain distance between anchors.
    Distance,
}

/// Assembly mate between two part entities.
#[derive(Component, Debug, Clone)]
pub struct CadMate {
    pub kind: MateKind,
    pub part_a: Entity,
    pub part_b: Entity,
    /// Local anchor on A (relative to part transform).
    pub anchor_a: Vec3,
    /// Local anchor on B.
    pub anchor_b: Vec3,
    /// Axis for revolute/prismatic (world-ish; applied as local to A).
    pub axis: Vec3,
    /// For Distance mate — rest length in meters. `None` = current distance.
    pub distance: Option<f32>,
    pub enabled: bool,
}

// ============================================================================
// Events
// ============================================================================

#[derive(Event, Message, Debug, Clone)]
pub struct CadCreateMateEvent {
    pub kind: MateKind,
    /// If None, uses current multi-selection (first two parts).
    pub part_a: Option<Entity>,
    pub part_b: Option<Entity>,
    /// Local-space anchors (default ZERO = part origin).
    pub anchor_a: Option<Vec3>,
    pub anchor_b: Option<Vec3>,
    /// False only when the event is replayed by the undo system's
    /// redo path — the stack already owns that mate's entry, and
    /// recording again would duplicate it.
    pub record_undo: bool,
}

/// Serializable snapshot of a created mate, stored inside
/// `undo::Action::CadMateCreate`. Content-matched on undo (rather
/// than by entity id) so undo→redo→undo cycles survive id churn.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MateSpec {
    pub kind: MateKind,
    pub part_a: u64,
    pub part_b: u64,
    pub anchor_a: [f32; 3],
    pub anchor_b: [f32; 3],
}

impl MateSpec {
    pub fn to_event(&self, record_undo: bool) -> CadCreateMateEvent {
        CadCreateMateEvent {
            kind: self.kind,
            part_a: Some(Entity::from_bits(self.part_a)),
            part_b: Some(Entity::from_bits(self.part_b)),
            anchor_a: Some(Vec3::from_array(self.anchor_a)),
            anchor_b: Some(Vec3::from_array(self.anchor_b)),
            record_undo,
        }
    }
}

// ============================================================================
// Plugin
// ============================================================================

pub struct CadAssemblyPlugin;

impl Plugin for CadAssemblyPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<CadCreateMateEvent>()
            .add_systems(Update, (route_mate_menu_actions, handle_create_mate));
    }
}

fn route_mate_menu_actions(
    mut events: MessageReader<MenuActionEvent>,
    mut create: MessageWriter<CadCreateMateEvent>,
) {
    // MenuActionEvent uses Action enum — string mates are routed from
    // slint_ui via CadCreateMateEvent directly. Keep this for future
    // Action::CadMate* keybindings.
    let _ = (&mut events, &mut create);
}

fn handle_create_mate(
    mut events: MessageReader<CadCreateMateEvent>,
    mut commands: Commands,
    selection: Option<Res<SelectionSyncManager>>,
    transforms: Query<&GlobalTransform>,
    instances: Query<(Entity, &Instance)>,
    mut pending_anchors: Option<ResMut<crate::cad_mate_tool::PendingMateAnchors>>,
    mut notifications: Option<ResMut<NotificationManager>>,
    mut undo: Option<ResMut<crate::undo::UndoStack>>,
) {
    for event in events.read() {
        let (a, b) = match (event.part_a, event.part_b) {
            (Some(a), Some(b)) => (a, b),
            _ => {
                // Pull first two selected part-like entities.
                let Some(sel) = selection.as_ref() else {
                    warn_mate(&mut notifications, "no selection manager");
                    continue;
                };
                let ids = sel.0.read().get_selected();
                let mut ents = Vec::new();
                for id in ids {
                    if let Some((e, inst)) = instances.iter().find(|(e, _)| {
                        format!("{}v{}", e.index(), e.generation()) == id
                    }) {
                        if !inst.class_name.is_adornment() {
                            ents.push(e);
                        }
                    }
                }
                if ents.len() < 2 {
                    warn_mate(
                        &mut notifications,
                        "Assembly mate needs ≥2 selected parts",
                    );
                    continue;
                }
                (ents[0], ents[1])
            }
        };

        if a == b {
            warn_mate(&mut notifications, "cannot mate a part to itself");
            continue;
        }

        let ta = transforms.get(a).ok().map(|g| g.translation()).unwrap_or(Vec3::ZERO);
        let tb = transforms.get(b).ok().map(|g| g.translation()).unwrap_or(Vec3::ZERO);

        // Anchors: event fields → pending resource from pick tool → origin.
        let (anchor_a, anchor_b) = if let (Some(aa), Some(ab)) = (event.anchor_a, event.anchor_b) {
            (aa, ab)
        } else if let Some(ref pending) = pending_anchors {
            if pending.part_a == a && pending.part_b == b {
                (pending.anchor_a, pending.anchor_b)
            } else {
                (Vec3::ZERO, Vec3::ZERO)
            }
        } else {
            (Vec3::ZERO, Vec3::ZERO)
        };
        // Consume one-shot pending anchors.
        if pending_anchors.is_some() {
            commands.remove_resource::<crate::cad_mate_tool::PendingMateAnchors>();
        }

        let axis = match event.kind {
            MateKind::Prismatic => Vec3::X,
            _ => Vec3::Y,
        };
        let distance = match event.kind {
            MateKind::Distance => {
                let wa = ta + anchor_a;
                let wb = tb + anchor_b;
                Some((wb - wa).length().max(0.01))
            }
            _ => None,
        };

        let mate = CadMate {
            kind: event.kind,
            part_a: a,
            part_b: b,
            anchor_a,
            anchor_b,
            axis,
            distance,
            enabled: true,
        };

        let label = format!("Mate:{:?}", event.kind);
        let mut ec = commands.spawn((
            Instance {
                name: label.clone(),
                class_name: ClassName::Folder, // non-visual container identity
                archivable: true,
                id: 0,
                ai: false,
                uuid: String::new(),
            },
            mate.clone(),
            Transform::default(),
            Visibility::Hidden,
            Name::new(label.clone()),
        ));

        // Insert the matching Avian joint with real entity endpoints.
        match event.kind {
            MateKind::Coincident => {
                let joint = FixedJoint::new(a, b)
                    .with_local_anchor1(anchor_a)
                    .with_local_anchor2(anchor_b);
                ec.insert(joint);
            }
            MateKind::Revolute => {
                let joint = RevoluteJoint::new(a, b)
                    .with_hinge_axis(axis)
                    .with_local_anchor1(anchor_a)
                    .with_local_anchor2(anchor_b);
                ec.insert(joint);
            }
            MateKind::Prismatic => {
                let joint = PrismaticJoint::new(a, b)
                    .with_slider_axis(axis)
                    .with_local_anchor1(anchor_a)
                    .with_local_anchor2(anchor_b);
                ec.insert(joint);
            }
            MateKind::Ball => {
                let joint = SphericalJoint::new(a, b)
                    .with_local_anchor1(anchor_a)
                    .with_local_anchor2(anchor_b);
                ec.insert(joint);
            }
            MateKind::Distance => {
                let dist = distance.unwrap_or(1.0);
                let joint = DistanceJoint::new(a, b)
                    .with_local_anchor1(anchor_a)
                    .with_local_anchor2(anchor_b)
                    .with_limits(dist * 0.99, dist * 1.01);
                ec.insert(joint);
            }
        }

        if event.record_undo {
            if let Some(ref mut u) = undo {
                let spec = MateSpec {
                    kind: event.kind,
                    part_a: a.to_bits(),
                    part_b: b.to_bits(),
                    anchor_a: anchor_a.to_array(),
                    anchor_b: anchor_b.to_array(),
                };
                if let Ok(spec_json) = serde_json::to_string(&spec) {
                    u.push_labeled(
                        format!("{:?} mate", event.kind),
                        crate::undo::Action::CadMateCreate { spec_json },
                    );
                }
            }
        }

        if let Some(ref mut n) = notifications {
            n.success(format!(
                "Assembly: {:?} mate between parts (live Avian joint)",
                event.kind
            ));
        }
        info!("🔗 Created {:?} mate {:?} ↔ {:?}", event.kind, a, b);
    }
}

fn warn_mate(notifications: &mut Option<ResMut<NotificationManager>>, msg: &str) {
    if let Some(ref mut n) = notifications {
        n.warning(msg);
    } else {
        warn!("🔗 {msg}");
    }
}
