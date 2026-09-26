//! Remote calls from players: `FireServer`, `InvokeServer`, and the
//! unreliable kind. Every one is checked before a host script sees it.
//!
//! 1. The remote exists and the player can see it (the host's replicator
//!    knows; see [`super::host::HostReplicator::visible_to`]).
//! 2. The arguments are bounded ([`check_call`]).
//! 3. The player is within the remote's rate ([`RemoteRates`]).
//! 4. A remote with declared argument types gets each argument's type
//!    checked, and its numbers checked for NaN ([`check_signature`]).
//!
//! Calls from the host to players need none of this: they travel in the
//! world lane as [`super::ops::ReplOp::Remote`].

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::id::NetId;
use super::value::{ValueLimits, WireValue};
use crate::wire::PeerId;

/// Calls one player may make to one remote per second, sustained.
pub const REMOTE_RATE: f64 = 60.0;
/// ... and in a burst.
pub const REMOTE_BURST: f64 = 120.0;
/// Arguments one call may carry.
pub const MAX_REMOTE_ARGS: usize = 64;
/// Encoded size of one call's arguments, roughly (see [`super::host::op_weight`]).
pub const MAX_REMOTE_WEIGHT: usize = 64 * 1024;

/// A player's call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteCall {
    pub remote: NetId,
    pub args: Vec<WireValue>,
    /// For `InvokeServer`: the id the answer carries. 0 for an event.
    pub call: u32,
}

/// The host's answer to an `InvokeServer`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteReply {
    pub call: u32,
    /// `Err` carries the error the invoked function raised.
    pub result: Result<Vec<WireValue>, String>,
}

/// Bounded arguments, or why not.
pub fn check_call(call: &RemoteCall, limits: &ValueLimits) -> Result<(), String> {
    if call.remote.is_none() {
        return Err("a call to no remote".into());
    }
    if call.args.len() > MAX_REMOTE_ARGS {
        return Err(format!("{} arguments; at most {MAX_REMOTE_ARGS}", call.args.len()));
    }
    let op = super::ops::ReplOp::Remote { remote: call.remote, args: call.args.clone() };
    if super::host::op_weight(&op) > MAX_REMOTE_WEIGHT {
        return Err(format!("arguments larger than {} KiB", MAX_REMOTE_WEIGHT / 1024));
    }
    for (i, a) in call.args.iter().enumerate() {
        a.check(limits).map_err(|e| format!("argument {}: {e}", i + 1))?;
    }
    Ok(())
}

/// Check `args` against declared types (`RemoteEvent:SetArgumentTypes`).
/// Each entry is a `typeof` name (`"number"`, `"Vector3"`, `"Instance"`,
/// `"table"`, ...) or `"any"`, with `?` appended when nil is allowed. A
/// typed number must be a number (no NaN, no infinity), and a typed call may
/// not carry more arguments than it declares.
pub fn check_signature(args: &[WireValue], signature: &[String]) -> Result<(), String> {
    if args.len() > signature.len() {
        return Err(format!("{} arguments; the remote takes {}", args.len(), signature.len()));
    }
    for (i, want) in signature.iter().enumerate() {
        let (ty, optional) = match want.strip_suffix('?') {
            Some(t) => (t, true),
            None => (want.as_str(), false),
        };
        let got = args.get(i).unwrap_or(&WireValue::Nil);
        if matches!(got, WireValue::Nil) {
            if optional || ty == "any" || ty == "nil" {
                continue;
            }
            return Err(format!("argument {} is nil; expected {ty}", i + 1));
        }
        if ty != "any" && got.type_name() != ty {
            return Err(format!("argument {} is a {}; expected {ty}", i + 1, got.type_name()));
        }
        if let WireValue::Number(n) = got {
            if !n.is_finite() {
                return Err(format!("argument {} is not a finite number", i + 1));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    at: f64,
}

/// Token buckets, one per player and remote.
#[derive(Debug, Default)]
pub struct RemoteRates {
    buckets: HashMap<(PeerId, NetId), Bucket>,
}

impl RemoteRates {
    /// Spend one call of `peer` on `remote` at time `now` (seconds), or
    /// refuse it.
    pub fn allow(&mut self, peer: PeerId, remote: NetId, now: f64) -> bool {
        let b = self.buckets.entry((peer, remote)).or_insert(Bucket { tokens: REMOTE_BURST, at: now });
        b.tokens = (b.tokens + (now - b.at).max(0.0) * REMOTE_RATE).min(REMOTE_BURST);
        b.at = now;
        if b.tokens >= 1.0 {
            b.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// A player left.
    pub fn forget_peer(&mut self, peer: PeerId) {
        self.buckets.retain(|(p, _), _| *p != peer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_then_the_sustained_rate() {
        let mut r = RemoteRates::default();
        let ok = (0..200).filter(|_| r.allow(1, NetId(5), 0.0)).count();
        assert_eq!(ok, REMOTE_BURST as usize);
        // Another remote, and another player, have their own buckets.
        assert!(r.allow(1, NetId(6), 0.0));
        assert!(r.allow(2, NetId(5), 0.0));
        // A second later, a second's worth.
        let ok = (0..200).filter(|_| r.allow(1, NetId(5), 1.0)).count();
        assert_eq!(ok, REMOTE_RATE as usize);
        r.forget_peer(1);
        assert!(r.allow(1, NetId(5), 1.0), "a player who left and came back starts full");
    }

    #[test]
    fn typed_remotes_refuse_what_they_did_not_declare() {
        let sig: Vec<String> = ["Vector3", "number", "string?"].iter().map(|s| s.to_string()).collect();
        let v = WireValue::Vector3([1.0, 2.0, 3.0]);
        assert!(check_signature(&[v.clone(), WireValue::Number(4.0)], &sig).is_ok());
        assert!(check_signature(&[v.clone(), WireValue::Number(4.0), WireValue::String("x".into())], &sig).is_ok());
        assert!(check_signature(&[v.clone(), WireValue::Number(f64::NAN)], &sig).is_err());
        assert!(check_signature(&[WireValue::Number(1.0), WireValue::Number(4.0)], &sig).is_err());
        assert!(check_signature(&[v.clone()], &sig).is_err(), "a required argument missing");
        let extra = [v, WireValue::Number(1.0), WireValue::Nil, WireValue::Bool(true)];
        assert!(check_signature(&extra, &sig).is_err());
        let any: Vec<String> = vec!["any".into()];
        assert!(check_signature(&[WireValue::Map(vec![])], &any).is_ok());
    }

    #[test]
    fn oversized_calls_are_refused() {
        let limits = ValueLimits::default();
        let big = RemoteCall { remote: NetId(3), args: vec![WireValue::String("x".repeat(60 * 1024)); 2], call: 0 };
        assert!(check_call(&big, &limits).is_err());
        let many = RemoteCall { remote: NetId(3), args: vec![WireValue::Bool(true); MAX_REMOTE_ARGS + 1], call: 0 };
        assert!(check_call(&many, &limits).is_err());
        let fine = RemoteCall { remote: NetId(3), args: vec![WireValue::Number(1.0)], call: 0 };
        assert!(check_call(&fine, &limits).is_ok());
        assert!(check_call(&RemoteCall { remote: NetId::NONE, ..fine }, &limits).is_err());
    }
}
