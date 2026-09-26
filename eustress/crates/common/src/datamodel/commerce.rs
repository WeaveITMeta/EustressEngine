//! # Commerce in Play
//!
//! The queues between `MarketplaceService` in a Play VM and the engine's
//! client for the Commerce API (`engine/src/play_datamodel/commerce.rs`),
//! which sells the products a creator listed for a published simulation.
//!
//! ```text
//! script  PromptProductPurchase(player, 3)  -> prompts    -> engine buys it
//! engine  a purchase to grant               -> receipts   -> ProcessReceipt
//! script  ProcessReceipt's decision         -> decisions  -> engine fulfills it
//! engine  the prompt closed                 -> outcomes   -> Prompt...Finished
//! ```
//!
//! Nothing loads until a script uses `MarketplaceService` (or sets
//! `ProcessReceipt`), so a session that sells nothing makes no request.
//! Scripts name a product by its number within the simulation, as Roblox
//! scripts name a developer product by its id.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// A `UserId` as a map key: whole, non-negative and exact in an `f64`.
pub fn user_key(user_id: f64) -> Option<u64> {
    (user_id.is_finite() && user_id >= 0.0 && user_id.fract() == 0.0 && user_id < 9_007_199_254_740_992.0)
        .then_some(user_id as u64)
}

/// Whether a product is granted each time it is bought or owned once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductKind {
    /// Granted through `ProcessReceipt` each time (a developer product).
    Consumable,
    /// Owned once (a game pass).
    Pass,
}

impl ProductKind {
    pub fn from_api(kind: &str) -> Self {
        if kind == "pass" {
            Self::Pass
        } else {
            Self::Consumable
        }
    }
}

/// A product the simulation sells, as the catalog lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct CommerceProduct {
    /// The API's `prod_` id.
    pub id: String,
    /// What scripts pass to `PromptProductPurchase`.
    pub number: u64,
    pub name: String,
    pub description: String,
    /// Whole Tickets.
    pub price: u64,
    pub kind: ProductKind,
    pub icon: Option<String>,
    /// On sale to players. Drafts are testable in Studio only.
    pub active: bool,
}

/// A script asked to show a purchase prompt.
#[derive(Debug, Clone, PartialEq)]
pub struct PurchasePrompt {
    /// The `UserId` of the Player the prompt is for.
    pub user_id: f64,
    /// The product number the script passed.
    pub product: u64,
    /// Which prompt the script called: `PromptGamePassPurchase` expects a
    /// pass, `PromptProductPurchase` a consumable, `PromptPurchase` either.
    pub expects: Option<ProductKind>,
}

/// A purchase the simulation has to grant: what `ProcessReceipt` receives.
#[derive(Debug, Clone, PartialEq)]
pub struct Receipt {
    /// The API's `pur_` id (`receiptInfo.PurchaseId`).
    pub purchase_id: String,
    pub user_id: f64,
    pub product: u64,
    /// Tickets spent (`receiptInfo.CurrencySpent`).
    pub price: u64,
    pub sim_id: String,
    /// The Space it was bought in, when the API recorded one.
    pub space: Option<String>,
}

/// What `ProcessReceipt` decided about a receipt.
#[derive(Debug, Clone, PartialEq)]
pub struct ReceiptDecision {
    pub purchase_id: String,
    /// `PurchaseGranted`. Anything else (`NotProcessedYet`, an error) leaves
    /// the purchase waiting, to be offered again.
    pub granted: bool,
}

/// How a prompt ended, for the `Prompt...PurchaseFinished` signals.
#[derive(Debug, Clone, PartialEq)]
pub struct PromptOutcome {
    pub user_id: f64,
    pub product: u64,
    /// The prompt's `expects`, which picks the signal to fire.
    pub expects: Option<ProductKind>,
    pub purchased: bool,
}

/// Where the session's commerce stands.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum CommerceStatus {
    /// No script has used `MarketplaceService`; nothing is loaded.
    #[default]
    Idle,
    /// A script asked; the catalog is on its way, and scripts that need it
    /// wait for it.
    Loading,
    Ready,
    /// Nothing can be sold in this session, and why (not published, not
    /// signed in, the API unreachable).
    Unavailable(String),
}

/// The session's commerce state, inside the [`DataModel`](super::DataModel).
#[derive(Debug, Default)]
pub struct CommerceState {
    pub status: CommerceStatus,
    pub catalog: Vec<CommerceProduct>,
    /// Pass numbers the local player owns.
    pub owned_passes: BTreeSet<u64>,
    /// Pass numbers each player who joined a multiplayer host owns, by
    /// `UserId` (see [`user_key`]).
    pub peer_passes: BTreeMap<u64, BTreeSet<u64>>,
    /// Joined players whose passes are still on their way, by `UserId`:
    /// `UserOwnsGamePassAsync` waits for them rather than answer too early.
    pub peer_passes_pending: BTreeSet<u64>,
    /// Scripts to engine: prompts to show.
    pub prompts: Vec<PurchasePrompt>,
    /// Engine to scripts: purchases waiting for `ProcessReceipt`. They stay
    /// here until a script sets the callback.
    pub receipts: VecDeque<Receipt>,
    /// A Luau script set `MarketplaceService.ProcessReceipt`. A simulation has
    /// one receipt handler, as in Roblox: when Luau has one, Rune's
    /// `process_receipt` is not called.
    pub luau_process_receipt: bool,
    /// Scripts to engine: what `ProcessReceipt` decided.
    pub decisions: Vec<ReceiptDecision>,
    /// Engine to scripts: prompts that closed.
    pub outcomes: Vec<PromptOutcome>,
}

impl CommerceState {
    /// A script used `MarketplaceService`: start loading, once.
    pub fn wake(&mut self) {
        if self.status == CommerceStatus::Idle {
            self.status = CommerceStatus::Loading;
        }
    }

    pub fn product(&self, number: u64) -> Option<&CommerceProduct> {
        self.catalog.iter().find(|p| p.number == number)
    }

    /// Whether player `user_id` owns pass `number`: the local player's passes
    /// are `owned_passes`, a joined player's are in `peer_passes`.
    pub fn owns_pass(&self, user_id: f64, local_user_id: Option<f64>, number: u64) -> bool {
        if Some(user_id) == local_user_id {
            return self.owned_passes.contains(&number);
        }
        user_key(user_id)
            .and_then(|key| self.peer_passes.get(&key))
            .map_or(false, |passes| passes.contains(&number))
    }

    /// Whether player `user_id`'s passes are still on their way. Only while the
    /// catalog is in: a session that cannot sell has nothing to wait for.
    pub fn passes_pending(&self, user_id: f64) -> bool {
        self.status == CommerceStatus::Ready
            && user_key(user_id).is_some_and(|key| self.peer_passes_pending.contains(&key))
    }

    /// Receipts and outcomes waiting for scripts.
    pub fn waiting_for_scripts(&self) -> usize {
        self.receipts.len() + self.outcomes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_user_id_is_a_key_only_when_whole_and_exact() {
        assert_eq!(user_key(0.0), Some(0));
        assert_eq!(user_key(42.0), Some(42));
        assert_eq!(user_key(9_007_199_254_740_991.0), Some(9_007_199_254_740_991));
        assert_eq!(user_key(9_007_199_254_740_992.0), None, "2^53 is where f64 stops being exact");
        assert_eq!(user_key(-1.0), None);
        assert_eq!(user_key(1.5), None);
        assert_eq!(user_key(f64::NAN), None);
        assert_eq!(user_key(f64::INFINITY), None);
    }

    #[test]
    fn the_local_players_passes_and_a_joined_players_are_kept_apart() {
        let mut state = CommerceState::default();
        state.owned_passes.insert(7);
        state.peer_passes.entry(42).or_default().insert(9);
        // The local player (UserId 1) owns 7 and not 9.
        assert!(state.owns_pass(1.0, Some(1.0), 7));
        assert!(!state.owns_pass(1.0, Some(1.0), 9));
        // A joined player (UserId 42) owns 9 and not the local player's 7.
        assert!(state.owns_pass(42.0, Some(1.0), 9));
        assert!(!state.owns_pass(42.0, Some(1.0), 7));
        // Nobody else owns anything, and a UserId that is no key never does.
        assert!(!state.owns_pass(43.0, Some(1.0), 9));
        assert!(!state.owns_pass(42.5, Some(1.0), 9));
    }

    #[test]
    fn passes_are_pending_only_while_the_catalog_is_in() {
        let mut state = CommerceState::default();
        state.peer_passes_pending.insert(42);
        // Nothing to wait for while commerce is idle, loading or unavailable.
        assert!(!state.passes_pending(42.0));
        state.wake();
        assert_eq!(state.status, CommerceStatus::Loading);
        assert!(!state.passes_pending(42.0));
        state.status = CommerceStatus::Unavailable("not published".into());
        assert!(!state.passes_pending(42.0));
        state.status = CommerceStatus::Ready;
        assert!(state.passes_pending(42.0));
        assert!(!state.passes_pending(43.0), "only the player whose passes are on their way");
        state.peer_passes_pending.remove(&42);
        assert!(!state.passes_pending(42.0));
    }

    #[test]
    fn waking_twice_does_not_restart_a_load() {
        let mut state = CommerceState::default();
        state.wake();
        state.status = CommerceStatus::Ready;
        state.wake();
        assert_eq!(state.status, CommerceStatus::Ready);
    }
}
