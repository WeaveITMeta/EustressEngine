//! Animation tracks across machines.
//!
//! A track is a pure function of its control state and the clock
//! (`eustress_common::datamodel::animation`), so only control changes travel,
//! never poses:
//!
//! ```text
//! a player's script   Load / Control / Unload  ->  ToHost::Tracks
//! the host            checks each, applies it to its own tree, relays it
//! every machine       applies what others sent to a track of its own
//! ```
//!
//! The host's own scripts' tracks (an NPC, a server-played emote) go to every
//! player that can see the Animator, the player it plays on included. A
//! player's own tracks never come back to it: each relayed op names the peer
//! it came from.
//!
//! Times travel in host ticks. Each machine converts at the moment it sends
//! or applies, through [`ClockLink`]: where its own `frame.time` stands
//! against the host's tick at that moment.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use eustress_common::datamodel::animation::{Priority, TrackControl, TrackOp};
use eustress_common::datamodel::{DataModel, InstanceId};

use super::host::{HostReplicator, Outgoing};
use super::id::NetId;
use super::motion::TICK_HZ;
use super::ops::{Audience, ReplOp};
use crate::wire::{PeerId, HOST_PEER};

/// Longest content id a player may name.
pub const MAX_CONTENT_LEN: usize = 512;
/// Longest track name kept.
pub const MAX_TRACK_NAME: usize = 100;
/// Tracks one player may have loaded on the host at once.
pub const MAX_TRACKS_PER_PEER: usize = 256;
/// Track changes a player may send per second, and in a burst.
pub const TRACK_RATE: f64 = 30.0;
pub const TRACK_BURST: f64 = 60.0;
/// Most changes one `ToHost::Tracks` message carries.
pub const MAX_TRACK_OPS_PER_MESSAGE: usize = 64;

/// One moment seen on two clocks: this machine's `frame.time` and the host's
/// tick, fractional.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockLink {
    pub frame_now: f64,
    pub tick_now: f64,
}

impl ClockLink {
    pub fn to_tick(&self, frame_time: f64) -> f64 {
        self.tick_now + (frame_time - self.frame_now) * TICK_HZ
    }

    pub fn to_frame(&self, tick: f64) -> f64 {
        self.frame_now + (tick - self.tick_now) / TICK_HZ
    }
}

/// A track's control state on the wire: [`TrackControl`] with its two clock
/// times in host ticks and its priority as Roblox's enum value.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WireControl {
    pub playing: bool,
    pub looped: bool,
    pub held: bool,
    pub priority: u32,
    pub anchor: f64,
    pub anchor_time: f32,
    pub speed: f32,
    pub fade_start: f64,
    pub fade_from: f32,
    pub fade_to: f32,
    pub fade_secs: f32,
    pub end_fade: f32,
    pub plays: u32,
    pub stops: u32,
}

impl WireControl {
    pub fn from_control(c: &TrackControl, clock: ClockLink) -> Self {
        Self {
            playing: c.playing,
            looped: c.looped,
            held: c.held,
            priority: c.priority.roblox_value(),
            anchor: clock.to_tick(c.anchor),
            anchor_time: c.anchor_time,
            speed: c.speed,
            fade_start: clock.to_tick(c.fade_start),
            fade_from: c.fade_from,
            fade_to: c.fade_to,
            fade_secs: c.fade_secs,
            end_fade: c.end_fade,
            plays: c.plays,
            stops: c.stops,
        }
    }

    /// The control state on this machine's clock, or `None` when a value is
    /// out of bounds: a number that is not finite, a speed past ±100, a weight
    /// outside 0 to 10, a fade outside 0 to 60 seconds, an unknown priority.
    pub fn to_control(&self, clock: ClockLink) -> Option<TrackControl> {
        let priority = Priority::from_roblox_value(self.priority)?;
        let finite = self.anchor.is_finite()
            && self.fade_start.is_finite()
            && [self.anchor_time, self.speed, self.fade_from, self.fade_to, self.fade_secs, self.end_fade]
                .iter()
                .all(|v| v.is_finite());
        let weight = |w: f32| (0.0..=10.0).contains(&w);
        let fade = |s: f32| (0.0..=60.0).contains(&s);
        let within = self.speed.abs() <= 100.0
            && weight(self.fade_from)
            && weight(self.fade_to)
            && fade(self.fade_secs)
            && fade(self.end_fade);
        if !(finite && within) {
            return None;
        }
        Some(TrackControl {
            playing: self.playing,
            looped: self.looped,
            priority,
            anchor: clock.to_frame(self.anchor),
            anchor_time: self.anchor_time,
            speed: self.speed,
            held: self.held,
            fade_start: clock.to_frame(self.fade_start),
            fade_from: self.fade_from,
            fade_to: self.fade_to,
            fade_secs: self.fade_secs,
            end_fade: self.end_fade,
            plays: self.plays,
            stops: self.stops,
        })
    }
}

/// One track change on the wire. `track` is the id its sender gave the
/// track, unique for that sender.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TrackWire {
    Load { track: u64, animator: NetId, animation: Option<NetId>, content: String, name: String },
    Control { track: u64, control: WireControl },
    Unload { track: u64 },
}

/// A clip a player may name: one inside the published world, or a Roblox
/// asset id in any form Roblox writes one (`rbxassetid://N`,
/// `http://www.roblox.com/asset/?id=N`, ...), which the world maps. A
/// registered sequence (`active://`) exists only on the machine that
/// registered it, so it never travels.
pub fn content_allowed(content: &str) -> bool {
    if content.len() > MAX_CONTENT_LEN || content.contains("..") || content.chars().any(char::is_control) {
        return false;
    }
    ["space://", "bundled://", "rig://"].iter().any(|p| content.starts_with(p))
        || eustress_common::animation::content::roblox_asset_id(content).is_some()
}

fn clean_name(name: &str) -> String {
    name.chars().filter(|c| !c.is_control()).take(MAX_TRACK_NAME).collect()
}

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    at: f64,
}

impl Bucket {
    fn allow(&mut self, now: f64) -> bool {
        self.tokens = (self.tokens + (now - self.at).max(0.0) * TRACK_RATE).min(TRACK_BURST);
        self.at = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// The host's side: its own scripts' tracks out, players' tracks in.
#[derive(Debug, Default)]
pub struct HostTracks {
    /// Tracks players sent, by (peer, their id).
    by_sender: HashMap<(PeerId, u64), InstanceId>,
    /// Where each of those came from, for relaying and late joiners.
    origin: HashMap<InstanceId, (PeerId, u64)>,
    rates: HashMap<PeerId, Bucket>,
}

/// What a player's track changes did on the host.
#[derive(Debug, Default)]
pub struct TracksApplied {
    /// Relays for the other players.
    pub out: Outgoing,
    /// Changes refused as malformed or not the player's to make.
    pub refused: usize,
    /// Changes dropped for the player's rate.
    pub over_rate: usize,
}

impl HostTracks {
    /// The host's own scripts' track changes since the last call, for every
    /// player that can see the Animator. Call after the replicator's
    /// `observe`, so an Animator made this frame is on players first.
    pub fn host_ops(&mut self, dm: &mut DataModel, rep: &HostReplicator, clock: ClockLink) -> Outgoing {
        self.prune(dm);
        let mut out = Outgoing::new();
        for op in dm.take_track_ops() {
            match op {
                TrackOp::Load { animator, track, animation, content, name } => {
                    let Some((animator, a)) = rep.live_of(animator) else { continue };
                    let animation = animation.and_then(|id| rep.live_of(id)).map(|(n, _)| n);
                    let op = TrackWire::Load { track: track.0, animator, animation, content, name };
                    out.push((a, ReplOp::Track { from: HOST_PEER, op }));
                }
                TrackOp::Control { track, control } => {
                    let Some((_, a)) = dm.animation.track(track).and_then(|t| rep.live_of(t.animator)) else { continue };
                    let op = TrackWire::Control { track: track.0, control: WireControl::from_control(&control, clock) };
                    out.push((a, ReplOp::Track { from: HOST_PEER, op }));
                }
                // The Animator may be gone already. A player that never had
                // the track ignores its Unload.
                TrackOp::Unload { track } => {
                    out.push((Audience::Everyone, ReplOp::Track { from: HOST_PEER, op: TrackWire::Unload { track: track.0 } }));
                }
            }
        }
        out
    }

    /// A player's track changes: each checked, applied to the host's tree,
    /// and relayed to everyone else who can see the Animator. `player` is the
    /// peer's `Player`; a track may only play on an Animator inside its
    /// current character.
    #[allow(clippy::too_many_arguments)]
    pub fn player_ops(
        &mut self,
        dm: &mut DataModel,
        rep: &HostReplicator,
        peer: PeerId,
        player: Option<InstanceId>,
        ops: Vec<TrackWire>,
        clock: ClockLink,
        now: f64,
    ) -> TracksApplied {
        self.prune(dm);
        let mut applied = TracksApplied::default();
        let character = player.and_then(|p| dm.get_prop(p, "Character")).and_then(|v| v.as_instance()).filter(|m| dm.exists(*m));
        for op in ops.into_iter().take(MAX_TRACK_OPS_PER_MESSAGE) {
            let bucket = self.rates.entry(peer).or_insert(Bucket { tokens: TRACK_BURST, at: now });
            if !bucket.allow(now) {
                applied.over_rate += 1;
                continue;
            }
            match op {
                TrackWire::Load { track, animator, animation, content, name } => {
                    let local_animator = rep.local_of(animator).filter(|a| {
                        dm.class_of(*a) == Some("Animator") && character.is_some_and(|c| dm.is_descendant_of(*a, c))
                    });
                    let loaded = self.by_sender.keys().filter(|(p, _)| *p == peer).count();
                    let (Some(local_animator), true, true) =
                        (local_animator, content_allowed(&content), loaded < MAX_TRACKS_PER_PEER)
                    else {
                        applied.refused += 1;
                        continue;
                    };
                    if let Some(old) = self.by_sender.remove(&(peer, track)) {
                        self.origin.remove(&old);
                        dm.unload_track(old);
                    }
                    let local_animation = animation.and_then(|n| rep.local_of(n)).filter(|a| dm.class_of(*a) == Some("Animation"));
                    let name = clean_name(&name);
                    let local = dm.load_remote_track(local_animator, local_animation, &content, &name);
                    self.by_sender.insert((peer, track), local);
                    self.origin.insert(local, (peer, track));
                    let a = rep.live_of(local_animator).map_or(Audience::Everyone, |(_, a)| a);
                    let animation = local_animation.and(animation);
                    let op = TrackWire::Load { track, animator, animation, content, name };
                    applied.out.push((a, ReplOp::Track { from: peer, op }));
                }
                TrackWire::Control { track, control } => {
                    let Some(local) = self.by_sender.get(&(peer, track)).copied() else {
                        applied.refused += 1;
                        continue;
                    };
                    let Some(c) = control.to_control(clock) else {
                        applied.refused += 1;
                        continue;
                    };
                    dm.set_remote_track_control(local, c);
                    let a = dm.animation.track(local).and_then(|t| rep.live_of(t.animator)).map_or(Audience::Everyone, |(_, a)| a);
                    applied.out.push((a, ReplOp::Track { from: peer, op: TrackWire::Control { track, control } }));
                }
                TrackWire::Unload { track } => {
                    let Some(local) = self.by_sender.remove(&(peer, track)) else { continue };
                    self.origin.remove(&local);
                    dm.unload_track(local);
                    applied.out.push((Audience::Everyone, ReplOp::Track { from: peer, op: TrackWire::Unload { track } }));
                }
            }
        }
        applied
    }

    /// A player left: its rate goes; its tracks went with its character.
    pub fn forget_peer(&mut self, dm: &mut DataModel, peer: PeerId) {
        self.rates.remove(&peer);
        let gone: Vec<(PeerId, u64)> = self.by_sender.keys().filter(|(p, _)| *p == peer).copied().collect();
        for key in gone {
            if let Some(local) = self.by_sender.remove(&key) {
                self.origin.remove(&local);
                dm.unload_track(local);
            }
        }
    }

    /// Every loaded track a player joining now can see, as a Load and its
    /// Control, so it lands at the same phase. Appended to its catch-up.
    pub fn snapshot(&self, dm: &DataModel, rep: &HostReplicator, player: Option<InstanceId>, clock: ClockLink) -> Vec<ReplOp> {
        let mut ops = Vec::new();
        for (id, t) in dm.animation.tracks() {
            if t.is_inert() {
                continue;
            }
            let Some((animator, a)) = rep.live_of(t.animator) else { continue };
            if matches!(a, Audience::Owner(p) if Some(p) != player) {
                continue;
            }
            let (from, track) = self.origin.get(&id).copied().unwrap_or((HOST_PEER, id.0));
            let animation = t.animation.and_then(|n| rep.live_of(n)).map(|(n, _)| n);
            let name = dm.name_of(id).unwrap_or("Animation").to_string();
            let load = TrackWire::Load { track, animator, animation, content: t.content.clone(), name };
            ops.push(ReplOp::Track { from, op: load });
            let control = WireControl::from_control(&t.control, clock);
            ops.push(ReplOp::Track { from, op: TrackWire::Control { track, control } });
        }
        ops
    }

    /// Forget tracks the tree no longer has (their Animator went away).
    fn prune(&mut self, dm: &DataModel) {
        self.origin.retain(|local, _| dm.animation.track(*local).is_some());
        self.by_sender.retain(|_, local| dm.animation.track(*local).is_some());
    }
}

/// A player's side: other machines' tracks in, its own character's out.
#[derive(Debug, Default)]
pub struct PlayerTracks {
    local: HashMap<(PeerId, u64), InstanceId>,
    /// This player's own tracks the host was told about.
    sent: HashSet<InstanceId>,
}

impl PlayerTracks {
    /// Apply one relayed change. `me` is this player's peer, whose own
    /// tracks are never applied back; `local_of` maps the host's ids.
    pub fn apply(
        &mut self,
        dm: &mut DataModel,
        me: Option<PeerId>,
        from: PeerId,
        op: &TrackWire,
        clock: Option<ClockLink>,
        local_of: &dyn Fn(NetId) -> Option<InstanceId>,
    ) -> Result<(), String> {
        if Some(from) == me {
            return Ok(());
        }
        match op {
            TrackWire::Load { track, animator, animation, content, name } => {
                let Some(animator) = local_of(*animator).filter(|a| dm.exists(*a)) else {
                    return Err(format!("track {track}: its Animator is not here"));
                };
                if let Some(old) = self.local.remove(&(from, *track)) {
                    dm.unload_track(old);
                }
                let animation = animation.and_then(|n| local_of(n)).filter(|a| dm.exists(*a));
                let local = dm.load_remote_track(animator, animation, content, name);
                self.local.insert((from, *track), local);
            }
            TrackWire::Control { track, control } => {
                let Some(local) = self.local.get(&(from, *track)).copied() else { return Ok(()) };
                let Some(clock) = clock else { return Err("a track change arrived before the host's clock".into()) };
                let Some(c) = control.to_control(clock) else { return Err(format!("track {track}: a control out of bounds")) };
                dm.set_remote_track_control(local, c);
            }
            TrackWire::Unload { track } => {
                if let Some(local) = self.local.remove(&(from, *track)) {
                    dm.unload_track(local);
                }
            }
        }
        Ok(())
    }

    /// This player's scripts' track changes on its own character, for the
    /// host. Tracks on anything else stay on this machine, as in Roblox.
    /// `net_of` maps this tree's ids to the host's.
    pub fn outgoing(&mut self, dm: &mut DataModel, clock: ClockLink, net_of: &dyn Fn(InstanceId) -> Option<NetId>) -> Vec<TrackWire> {
        self.local.retain(|_, local| dm.animation.track(*local).is_some());
        let character = dm
            .local_player
            .and_then(|p| dm.get_prop(p, "Character"))
            .and_then(|v| v.as_instance())
            .filter(|m| dm.exists(*m));
        let mut out = Vec::new();
        for op in dm.take_track_ops() {
            match op {
                TrackOp::Load { animator, track, animation, content, name } => {
                    let mine = character.is_some_and(|c| dm.is_descendant_of(animator, c));
                    let (true, Some(animator)) = (mine, net_of(animator)) else { continue };
                    if !content_allowed(&content) {
                        continue;
                    }
                    self.sent.insert(track);
                    let animation = animation.and_then(|a| net_of(a));
                    out.push(TrackWire::Load { track: track.0, animator, animation, content, name: clean_name(&name) });
                }
                TrackOp::Control { track, control } => {
                    if self.sent.contains(&track) {
                        out.push(TrackWire::Control { track: track.0, control: WireControl::from_control(&control, clock) });
                    }
                }
                TrackOp::Unload { track } => {
                    if self.sent.remove(&track) {
                        out.push(TrackWire::Unload { track: track.0 });
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clips_a_player_may_name() {
        let allowed = [
            "rig://walk",
            "bundled://idle",
            "space://Animations/Wave.anim",
            "rbxassetid://507766666",
            "http://www.roblox.com/asset/?id=507766666",
        ];
        for content in allowed {
            assert!(content_allowed(content), "{content}");
        }
        let refused = ["space://../../secret", "active://Wave", "file:///C:/Windows/win.ini", "https://example.com/wave", "rbxassetid://", ""];
        for content in refused {
            assert!(!content_allowed(content), "{content}");
        }
    }

    #[test]
    fn a_control_keeps_its_phase_across_clocks_and_out_of_bounds_values_are_refused() {
        let control = TrackControl { playing: true, anchor: 12.5, fade_start: 12.0, speed: 1.5, fade_to: 1.0, fade_secs: 0.3, ..Default::default() };
        // The sender's clock reads 10 s at host tick 600; the receiver's reads
        // 250 s at that same tick.
        let wire = WireControl::from_control(&control, ClockLink { frame_now: 10.0, tick_now: 600.0 });
        let there = wire.to_control(ClockLink { frame_now: 250.0, tick_now: 600.0 }).unwrap();
        assert!((there.anchor - 252.5).abs() < 1e-9 && (there.fade_start - 252.0).abs() < 1e-9);
        assert_eq!((there.speed, there.playing, there.priority), (1.5, true, control.priority));
        let clock = ClockLink { frame_now: 0.0, tick_now: 0.0 };
        let bad = [
            WireControl { speed: 101.0, ..wire },
            WireControl { speed: f32::NAN, ..wire },
            WireControl { fade_to: 11.0, ..wire },
            WireControl { fade_secs: 61.0, ..wire },
            WireControl { anchor: f64::INFINITY, ..wire },
            WireControl { priority: 7, ..wire },
        ];
        for w in bad {
            assert!(w.to_control(clock).is_none(), "{w:?}");
        }
    }
}
