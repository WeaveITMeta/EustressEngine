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

use std::collections::{BTreeSet, VecDeque};

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
    /// Scripts to engine: prompts to show.
    pub prompts: Vec<PurchasePrompt>,
    /// Engine to scripts: purchases waiting for `ProcessReceipt`. They stay
    /// here until a script sets the callback.
    pub receipts: VecDeque<Receipt>,
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

    /// Receipts and outcomes waiting for scripts.
    pub fn waiting_for_scripts(&self) -> usize {
        self.receipts.len() + self.outcomes.len()
    }
}
