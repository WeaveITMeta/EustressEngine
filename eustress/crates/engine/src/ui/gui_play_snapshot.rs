//! GUI class components come back on Stop.
//!
//! A script in Play changes a TextLabel's text and colours, a Frame's size,
//! a ScreenGui's Enabled, and so on, on the authored entities themselves.
//! Each GUI class component is snapshotted as Play starts from Edit and put
//! back when Edit mode returns, on every stop path, after the scripts'
//! `on_exit` has run, so nothing played outlives the session (and nothing
//! can save a played value). The drawn `GuiElementDisplay` follows its class
//! component again once restored; its own snapshot is `play_mode`'s.
//!
//! Entities spawned during Play are not in the snapshot; Stop despawns
//! them. The snapshot is taken on `play_mode::session_start()`, the
//! Edit-to-Play transition, which runs before `OnEnter(Playing)` (so before
//! the session seed hides StarterGui) and not on a resume from Pause, which
//! also enters Playing.

use bevy::ecs::component::Mutable;
use bevy::prelude::*;
use bevy::reflect::Reflect;

use eustress_common::classes::{
    BillboardGui, DocumentFrame, Frame, ImageButton, ImageLabel, ScreenGui, ScrollingFrame, SurfaceGui, TextBox,
    TextButton, TextLabel, UIAspectRatioConstraint, UICorner, UIGradient, UIGridLayout, UIListLayout, UIPadding,
    UIScale, UISizeConstraint, UIStroke, UITextSizeConstraint, VideoFrame, ViewportFrame, WebFrame,
};

use crate::play_mode::PlayModeState;

/// Snapshot and restore every GUI class component across a Play session.
pub fn add_gui_play_snapshots(app: &mut App) {
    add_play_snapshot::<ScreenGui>(app);
    add_play_snapshot::<BillboardGui>(app);
    add_play_snapshot::<SurfaceGui>(app);
    add_play_snapshot::<Frame>(app);
    add_play_snapshot::<ScrollingFrame>(app);
    add_play_snapshot::<TextLabel>(app);
    add_play_snapshot::<TextButton>(app);
    add_play_snapshot::<TextBox>(app);
    add_play_snapshot::<ImageLabel>(app);
    add_play_snapshot::<ImageButton>(app);
    add_play_snapshot::<ViewportFrame>(app);
    add_play_snapshot::<VideoFrame>(app);
    add_play_snapshot::<DocumentFrame>(app);
    add_play_snapshot::<WebFrame>(app);
    add_play_snapshot::<UIStroke>(app);
    add_play_snapshot::<UICorner>(app);
    add_play_snapshot::<UIGradient>(app);
    add_play_snapshot::<UIPadding>(app);
    add_play_snapshot::<UIListLayout>(app);
    add_play_snapshot::<UIGridLayout>(app);
    add_play_snapshot::<UIScale>(app);
    add_play_snapshot::<UIAspectRatioConstraint>(app);
    add_play_snapshot::<UISizeConstraint>(app);
    add_play_snapshot::<UITextSizeConstraint>(app);
}

/// One component type as it was on every entity when Play started.
#[derive(Resource)]
struct PlaySnapshot<T: Send + Sync + 'static> {
    saved: Vec<(Entity, T)>,
}

impl<T: Send + Sync + 'static> Default for PlaySnapshot<T> {
    fn default() -> Self {
        Self { saved: Vec::new() }
    }
}

/// Snapshot `T` as Play starts from Edit and restore it when Edit mode
/// returns, after the scripts' `on_exit`.
pub fn add_play_snapshot<T>(app: &mut App)
where
    T: Component<Mutability = Mutable> + Clone + Reflect,
{
    app.init_resource::<PlaySnapshot<T>>()
        .add_systems(crate::play_mode::session_start(), snapshot_on_play::<T>)
        .add_systems(
            OnEnter(PlayModeState::Editing),
            restore_on_stop::<T>.after(crate::soul::rune_api::cleanup_scripts_on_stop),
        );
}

fn snapshot_on_play<T: Component + Clone>(items: Query<(Entity, &T)>, mut snapshot: ResMut<PlaySnapshot<T>>) {
    snapshot.saved = items.iter().map(|(e, c)| (e, c.clone())).collect();
}

fn restore_on_stop<T>(mut snapshot: ResMut<PlaySnapshot<T>>, mut items: Query<&mut T>)
where
    T: Component<Mutability = Mutable> + Clone + Reflect,
{
    for (entity, authored) in std::mem::take(&mut snapshot.saved) {
        let Ok(mut live) = items.get_mut(entity) else { continue };
        // Written only when it differs, so an untouched element is not
        // redrawn (and, with TOML persistence on, not rewritten).
        if live.reflect_partial_eq(authored.as_partial_reflect()) != Some(true) {
            *live = authored;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::state::app::StatesPlugin;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin)).init_state::<PlayModeState>();
        add_gui_play_snapshots(&mut app);
        app.update();
        app
    }

    fn enter(app: &mut App, state: PlayModeState) {
        app.world_mut().resource_mut::<NextState<PlayModeState>>().set(state);
        app.update();
    }

    #[test]
    fn a_label_a_script_changed_comes_back_on_stop() {
        let mut app = app();
        let label = app.world_mut().spawn(TextLabel { text: "Score: 0".into(), ..Default::default() }).id();
        let frame = app.world_mut().spawn(Frame::default()).id();
        let frame_before = app.world().get::<Frame>(frame).unwrap().clone();

        enter(&mut app, PlayModeState::Playing);
        app.world_mut().get_mut::<TextLabel>(label).unwrap().text = "Score: 12".into();
        // A resume from Pause enters Playing again and must keep the first
        // snapshot, not take the played text.
        enter(&mut app, PlayModeState::Paused);
        enter(&mut app, PlayModeState::Playing);
        app.world_mut().get_mut::<TextLabel>(label).unwrap().text = "Score: 30".into();
        let spawned = app.world_mut().spawn(TextLabel { text: "made in Play".into(), ..Default::default() }).id();

        enter(&mut app, PlayModeState::Editing);
        assert_eq!(app.world().get::<TextLabel>(label).unwrap().text, "Score: 0");
        assert_eq!(app.world().get::<TextLabel>(spawned).unwrap().text, "made in Play", "Play's own entities are Stop's to despawn");
        let frame_after = app.world().get::<Frame>(frame).unwrap();
        assert_eq!(frame_after.reflect_partial_eq(frame_before.as_partial_reflect()), Some(true));
    }

    #[test]
    fn an_untouched_element_is_not_rewritten() {
        let mut app = app();
        let label = app.world_mut().spawn(TextLabel::default()).id();
        enter(&mut app, PlayModeState::Playing);
        enter(&mut app, PlayModeState::Editing);
        let tick = app.world().entity(label).get_change_ticks::<TextLabel>().unwrap().changed;
        let spawn_tick = app.world().entity(label).get_change_ticks::<TextLabel>().unwrap().added;
        assert_eq!(tick, spawn_tick, "restoring an equal value must not mark it changed");
    }
}
