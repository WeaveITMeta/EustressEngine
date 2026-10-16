//! # When Workshop stops to ask
//!
//! Auto mode (`GlobalSoulSettings::workshop_auto_approve`) saves the clicks on
//! tools that work inside the Space: creating and editing parts, writing
//! scripts and files, starting a simulation. It never saves the click on a tool
//! that runs commands or code, deletes things, or goes online. Those show the
//! approval card whatever the switch says, because their effect reaches past
//! the Space and past anything the person can undo from the History panel.
//!
//! The rule is enforced twice, so one mistake cannot open it:
//!
//! 1. [`needs_approval_card`] decides whether a call waits for a click. A tool
//!    classified Execute, Destructive or Network always does.
//! 2. [`auto_run_permissions`] is the grant a call runs under when it did NOT
//!    wait for a click. It holds Read and Write only, so a sensitive call that
//!    somehow skipped the card is refused at dispatch by
//!    `tools::capability::authorize_call` instead of running.
//!
//! A call the person approves runs under the full grant: they saw what it was
//! (`card_review` makes sure of that).
//!
//! Write includes writing scripts and files inside the Space, which a person
//! later runs with Play. Auto saves the clicks on writing and never the click
//! on running.

use eustress_tools::{Capability, Permissions};

/// Whether this call must wait for the person's click before it runs.
///
/// * `declared_approval` is the tool's own `requires_approval` flag.
/// * `class` is the call's capability class
///   (`tools::capability::capability_of_call`, which reads the arguments too),
///   or `None` for a tool that is not classified.
/// * `auto_mode` is the Auto switch.
///
/// A tool that runs code, deletes, or goes online (Execute, Destructive,
/// Network) needs a card even when its own flag says otherwise and even in auto
/// mode. Any other class follows the tool's flag, and auto mode waives it.
///
/// An unclassified tool keeps the earlier behaviour, the flag and the switch.
/// A card for it would never lead anywhere, since dispatch refuses a tool that
/// has no classification, so it is left to run into that refusal and report it
/// to the model.
pub fn needs_approval_card(declared_approval: bool, class: Option<Capability>, auto_mode: bool) -> bool {
    match class {
        Some(Capability::Execute | Capability::Destructive | Capability::Network) => true,
        Some(Capability::Read | Capability::Write) | None => declared_approval && !auto_mode,
    }
}

/// The grant for a call that runs without waiting for a click: Read and Write.
///
/// Used for the context of the auto-run worker. A call the person approved on a
/// card runs under `Permissions::full()` instead (see `ToolDispatchTasks`).
pub fn auto_run_permissions() -> Permissions {
    Permissions::standard().for_principal("workshop-session")
}

/// The two model calls the bridge answers itself, outside the tool registry:
/// the advisor (a second model asked a question) and the Gauntlet critic (a
/// clean-context model scoring evidence). They send text to the provider the
/// chat already uses, under the person's own key, so no card or grant stands in
/// front of them, and in auto mode a model could call one in a loop and spend
/// tokens with no click. A cap stands in front of them instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeCall {
    Advisor,
    Critic,
}

/// Advisor consultations allowed per user message, and per conversation.
pub const ADVISOR_PER_TURN: usize = 3;
pub const ADVISOR_PER_SESSION: usize = 20;
/// Critic calls allowed per user message and per conversation. One Gauntlet
/// run is at most `gauntlet::MAX_ITERATIONS_TOTAL` (nine) critic calls, so the
/// per-message cap is that, and the conversation allows five such runs.
pub const CRITIC_PER_TURN: usize = 9;
pub const CRITIC_PER_SESSION: usize = 45;

impl BridgeCall {
    /// The bridge call a tool name stands for, if it is one.
    pub fn from_tool_name(name: &str) -> Option<Self> {
        match name {
            "consult_advisor" => Some(Self::Advisor),
            "gauntlet_critic" => Some(Self::Critic),
            _ => None,
        }
    }

    pub fn tool_name(self) -> &'static str {
        match self {
            Self::Advisor => "consult_advisor",
            Self::Critic => "gauntlet_critic",
        }
    }

    /// How many calls of this kind one user message may make.
    pub fn per_turn(self) -> usize {
        match self {
            Self::Advisor => ADVISOR_PER_TURN,
            Self::Critic => CRITIC_PER_TURN,
        }
    }

    /// How many calls of this kind one conversation may make.
    pub fn per_session(self) -> usize {
        match self {
            Self::Advisor => ADVISOR_PER_SESSION,
            Self::Critic => CRITIC_PER_SESSION,
        }
    }
}

/// How often a bridge call has already been made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BridgeUse {
    /// Since the person's last message.
    pub this_turn: usize,
    /// In the whole conversation.
    pub this_session: usize,
}

/// Count the earlier calls of one kind from the conversation itself, so the
/// count resets with the conversation and needs no state of its own. Refused
/// calls leave a card too and count, so once over a cap a call stays refused.
pub fn bridge_use(messages: &[super::ChatMessage], call: BridgeCall) -> BridgeUse {
    let is_call = |m: &super::ChatMessage| m.mcp_endpoint.as_deref() == Some(call.tool_name());
    let this_session = messages.iter().filter(|m| is_call(m)).count();
    let this_turn = messages
        .iter()
        .rev()
        .take_while(|m| m.role != super::MessageRole::User)
        .filter(|m| is_call(m))
        .count();
    BridgeUse { this_turn, this_session }
}

/// Whether one more call of this kind may run. The refusal is worded for the
/// model, which reads it as the tool's result.
pub fn bridge_call_allowed(used: BridgeUse, call: BridgeCall) -> Result<(), String> {
    if used.this_turn >= call.per_turn() {
        return Err(format!(
            "Not run: {} was already used {} times for this message (limit {}). Answer from what you have, or ask the user.",
            call.tool_name(),
            used.this_turn,
            call.per_turn()
        ));
    }
    if used.this_session >= call.per_session() {
        return Err(format!(
            "Not run: {} was already used {} times in this conversation (limit {}). Answer from what you have, or ask the user.",
            call.tool_name(),
            used.this_session,
            call.per_session()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workshop::{ChatMessage, MessageRole};
    use Capability::*;

    const SENSITIVE: [Capability; 3] = [Execute, Destructive, Network];

    fn card(tool: &str) -> ChatMessage {
        ChatMessage { role: MessageRole::Mcp, mcp_endpoint: Some(tool.to_string()), ..Default::default() }
    }

    fn user() -> ChatMessage {
        ChatMessage { role: MessageRole::User, ..Default::default() }
    }

    #[test]
    fn an_unclassified_tool_is_refused_under_every_grant() {
        // The approved path runs under the full grant; the auto path under Read and
        // Write. A name nobody classified runs under neither.
        use eustress_tools::capability::authorize_call;
        let input = serde_json::json!({});
        for (grant, label) in [
            (Permissions::full(), "full"),
            (auto_run_permissions(), "auto-run"),
            (Permissions::standard(), "standard"),
        ] {
            let denial = authorize_call("a_tool_nobody_classified", &input, &grant)
                .expect_err(&format!("{label} grant ran an unclassified tool"));
            assert_eq!(denial.required, None, "{label}");
        }
    }

    #[test]
    fn the_advisor_is_capped_per_message_and_per_conversation() {
        let call = BridgeCall::Advisor;
        // Nothing yet: allowed.
        assert_eq!(bridge_call_allowed(bridge_use(&[user()], call), call), Ok(()));

        // Three consultations since the person's last message: the fourth is refused.
        let mut chat = vec![user()];
        for _ in 0..ADVISOR_PER_TURN {
            chat.push(card("consult_advisor"));
        }
        let used = bridge_use(&chat, call);
        assert_eq!(used, BridgeUse { this_turn: 3, this_session: 3 });
        let refusal = bridge_call_allowed(used, call).unwrap_err();
        assert!(refusal.contains("consult_advisor") && refusal.contains("limit 3"), "{refusal}");

        // A new message from the person resets the turn count, not the session count.
        chat.push(user());
        let used = bridge_use(&chat, call);
        assert_eq!(used, BridgeUse { this_turn: 0, this_session: 3 });
        assert_eq!(bridge_call_allowed(used, call), Ok(()));
    }

    #[test]
    fn the_session_cap_stops_a_slow_loop_across_many_messages() {
        let call = BridgeCall::Advisor;
        let mut chat = Vec::new();
        for _ in 0..ADVISOR_PER_SESSION {
            chat.push(user());
            chat.push(card("consult_advisor"));
        }
        let used = bridge_use(&chat, call);
        assert_eq!(used.this_turn, 1);
        let refusal = bridge_call_allowed(used, call).unwrap_err();
        assert!(refusal.contains("in this conversation"), "{refusal}");
    }

    #[test]
    fn the_critic_and_the_advisor_are_counted_separately() {
        let chat = vec![user(), card("gauntlet_critic"), card("gauntlet_critic"), card("consult_advisor")];
        assert_eq!(bridge_use(&chat, BridgeCall::Critic).this_turn, 2);
        assert_eq!(bridge_use(&chat, BridgeCall::Advisor).this_turn, 1);
        // A full Gauntlet run fits under the critic's per-message cap.
        let mut run = vec![user()];
        for _ in 0..CRITIC_PER_TURN - 1 {
            run.push(card("gauntlet_critic"));
        }
        assert_eq!(bridge_call_allowed(bridge_use(&run, BridgeCall::Critic), BridgeCall::Critic), Ok(()));
        run.push(card("gauntlet_critic"));
        assert!(bridge_call_allowed(bridge_use(&run, BridgeCall::Critic), BridgeCall::Critic).is_err());
    }

    #[test]
    fn only_the_two_bridge_names_are_bridge_calls() {
        assert_eq!(BridgeCall::from_tool_name("consult_advisor"), Some(BridgeCall::Advisor));
        assert_eq!(BridgeCall::from_tool_name("gauntlet_critic"), Some(BridgeCall::Critic));
        assert_eq!(BridgeCall::from_tool_name("run_bash"), None);
    }

    #[test]
    fn auto_mode_runs_a_read_or_write_tool_without_a_card() {
        for class in [Read, Write] {
            assert!(!needs_approval_card(true, Some(class), true), "{class:?} that asks, auto on");
            assert!(!needs_approval_card(false, Some(class), true), "{class:?} that never asks, auto on");
        }
    }

    #[test]
    fn auto_mode_off_follows_the_tools_own_flag() {
        for class in [Read, Write] {
            assert!(needs_approval_card(true, Some(class), false), "{class:?} that asks, auto off");
            assert!(!needs_approval_card(false, Some(class), false), "{class:?} that never asks, auto off");
        }
    }

    #[test]
    fn a_tool_that_runs_deletes_or_goes_online_always_shows_the_card() {
        for class in SENSITIVE {
            for declared in [true, false] {
                for auto_mode in [true, false] {
                    assert!(
                        needs_approval_card(declared, Some(class), auto_mode),
                        "{class:?} (flag {declared}, auto {auto_mode}) must wait for a click"
                    );
                }
            }
        }
    }

    #[test]
    fn an_unclassified_tool_keeps_the_earlier_behaviour() {
        // Dispatch refuses it either way; a card would only stall the turn.
        assert!(!needs_approval_card(false, None, false));
        assert!(!needs_approval_card(true, None, true));
        assert!(needs_approval_card(true, None, false));
    }

    #[test]
    fn the_auto_run_grant_is_read_and_write_and_nothing_more() {
        let grant = auto_run_permissions();
        assert!(grant.allows(Read));
        assert!(grant.allows(Write));
        for class in SENSITIVE {
            assert!(!grant.allows(class), "{class:?} must not be in the auto-run grant");
        }
        assert_eq!(grant.principal.as_deref(), Some("workshop-session"));
    }

    #[test]
    fn the_real_classifications_decide_the_named_tools() {
        use eustress_tools::capability::capability_of_call;
        let input = serde_json::json!({});
        let card = |tool: &str, declared: bool| {
            needs_approval_card(declared, capability_of_call(tool, &input), true)
        };
        // Auto mode is on for every line below.
        assert!(card("run_bash", true), "a shell command");
        assert!(card("execute_luau", true), "running a script");
        assert!(card("delete_entity", true), "deleting");
        assert!(card("http_request", true), "going online");
        // Flagged as never asking, still asks: the class decides, not the flag.
        assert!(card("moderation_queue", false), "a moderation read leaves the machine");
        // The work Auto mode exists for.
        assert!(!card("create_entity", false));
        assert!(!card("write_file", false));
        assert!(!card("run_simulation", true), "inside the Space, so Auto waives its card");
    }

    /// The whole shipped catalogue rather than a sample. A tool added later that
    /// runs code, deletes or goes online is covered the moment it is classified.
    #[test]
    fn no_registered_tool_that_runs_deletes_or_goes_online_can_skip_the_card() {
        use eustress_tools::capability::{authorize_call, capability_of_call};
        let registry = crate::workshop::build_tool_registry();
        let input = serde_json::json!({});
        let auto_grant = auto_run_permissions();

        let mut sensitive_tools = 0;
        for def in registry.all_tools() {
            // A tool with no classification fails `every_engine_registered_tool_is_classified`.
            let Some(class) = capability_of_call(def.name, &input) else { continue };
            let sensitive = SENSITIVE.contains(&class);
            if sensitive {
                sensitive_tools += 1;
                for auto_mode in [true, false] {
                    assert!(
                        needs_approval_card(def.requires_approval, Some(class), auto_mode),
                        "{} ({class:?}) could run without a card (flag {}, auto {auto_mode})",
                        def.name,
                        def.requires_approval
                    );
                }
            }
            // The second lock: a call that did skip the card is still refused.
            assert_eq!(
                authorize_call(def.name, &input, &auto_grant).is_ok(),
                !sensitive,
                "{} ({class:?}) is judged wrongly under the auto-run grant",
                def.name
            );
        }
        assert!(sensitive_tools > 0, "the sweep saw no sensitive tool, so it proved nothing");
    }
}
