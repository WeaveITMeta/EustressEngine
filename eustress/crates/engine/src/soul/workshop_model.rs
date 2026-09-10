//! # Workshop Model Selection
//!
//! Which model answers Workshop's conversational agentic loop — a distinct
//! axis from [`eustress_common::soul::ModelTier`], which drives the Soul
//! *build pipeline*'s complexity-derived Haiku/Sonnet/Opus selection (English
//! → Rune codegen) and the Workshop session-title Haiku helper. Those stay
//! untouched; `WorkshopModel` is purely "which model the user picked to chat
//! with in Workshop."
//!
//! ## The list is data, not code
//!
//! This used to be a hardcoded enum with one variant per model, which meant a
//! frontier release nobody could use until someone edited Rust, recompiled and
//! shipped a build. The list now comes from a catalog: a JSON document that
//! `api.eustress.dev` recompiles nightly (Grok 4.6 with live search, validated
//! against a provider whitelist — see the MODEL CATALOG section in
//! `infrastructure/cloudflare/api/src/index.js`).
//!
//! Three properties are deliberate:
//!
//! * **The seed is compiled in.** [`Catalog::seed`] is a byte-for-byte match
//!   for the worker's `SEED_CATALOG`. An engine that has never reached the
//!   network, or is running behind a firewall, still gets a full picker. The
//!   catalog only ever *widens* the list.
//! * **A refresh lands at the next launch, not mid-session.** The fetch writes
//!   a cache file; startup reads it. A model list that reshuffles under a user
//!   halfway through a turn is a bug, not freshness.
//! * **`WorkshopModel` is still `Copy`.** It is a `&'static ModelSpec` into a
//!   catalog leaked once at startup, so it still crosses a thread boundary
//!   into the agentic worker by value, exactly as the enum did.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

/// The catalog shape this build understands. A document declaring anything
/// else is refused outright rather than half-read: an engine that guesses at a
/// schema it was not built for will guess wrong about prices.
pub const CATALOG_SCHEMA: u32 = 1;

/// Which backend a [`WorkshopModel`] talks to.
///
/// This is the whitelist, and it is closed on purpose. Every provider needs a
/// wire-format client and a key to be routable, so a vendor that is not a
/// variant here is not merely unsupported — there is nowhere to send it. The
/// worker drops unknown providers before they are ever published, and
/// [`Provider::from_id`] drops any that slip through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Anthropic,
    Xai,
    #[serde(rename = "openai")]
    OpenAi,
}

impl Provider {
    /// Resolve the catalog's provider id. `None` for anything off the
    /// whitelist, which drops the entry.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "anthropic" => Some(Provider::Anthropic),
            "xai" => Some(Provider::Xai),
            "openai" => Some(Provider::OpenAi),
            _ => None,
        }
    }

    /// Vendor name for the picker's section headers.
    pub fn label(&self) -> &'static str {
        match self {
            Provider::Anthropic => "Anthropic",
            Provider::Xai => "xAI",
            Provider::OpenAi => "OpenAI",
        }
    }

    /// Which BYOK key in `GlobalSoulSettings` this provider spends, named as
    /// the settings page labels it. Used for the "no key configured" message,
    /// so the error tells the user which field to go and fill in.
    pub fn key_label(&self) -> &'static str {
        match self {
            Provider::Anthropic => "Anthropic",
            Provider::Xai => "xAI (Grok)",
            Provider::OpenAi => "OpenAI",
        }
    }
}

/// One model the user can select to power Workshop's conversational loop.
///
/// Owned `String`s rather than `&'static str`: these arrive from a JSON
/// document at runtime. The whole catalog is leaked once at startup so
/// [`WorkshopModel`] can stay a `Copy` reference into it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpec {
    /// The exact wire model id sent to the provider's API. Also what's
    /// persisted in `GlobalSoulSettings::workshop_model`.
    pub id: String,
    /// Human-readable label — what the Workshop toolbar pill shows.
    pub display_name: String,
    pub provider: Provider,
    /// One short sentence on what this model is FOR, shown under the name in
    /// the picker. May be empty; the menu lays out without it.
    #[serde(default)]
    pub tagline: String,
    /// USD per million input tokens, standard rate.
    pub input_price_per_mtok: f64,
    /// USD per million output tokens, standard rate.
    pub output_price_per_mtok: f64,
    /// Per-request output token cap. Reasoning models whose thinking shares
    /// the budget get more headroom.
    pub max_tokens: u32,
    /// HTTP request timeout. Advisor calls on hard questions can run minutes.
    pub timeout_secs: u64,
    #[serde(default = "default_true")]
    pub vision: bool,
}

fn default_true() -> bool {
    true
}

/// The full model list, plus the two roles the bridge needs to resolve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub updated_at: String,
    /// What produced this list: `"seed"`, a model id, or `"rollback:{date}"`.
    #[serde(default)]
    pub source: String,
    pub models: Vec<ModelSpec>,
    /// Selected when the user has never chosen, or when their stored id
    /// resolves to nothing at all.
    pub default_model: String,
    /// The model the `consult_advisor` tool targets. Kept as a *role* rather
    /// than a hardcoded variant so retiring the advisor is a catalog edit.
    pub advisor_model: String,
    /// Retired id → its replacement. A user whose settings still hold a
    /// retired id must be UPGRADED, never silently reassigned to the default:
    /// that would move them to another provider, at another price, with no
    /// notice. Retiring a model must upgrade its users.
    #[serde(default)]
    pub aliases: HashMap<String, String>,
}

impl Catalog {
    /// The compiled-in list. Byte-for-byte the worker's `SEED_CATALOG`; the
    /// two must not drift.
    ///
    /// This is what an engine uses offline, on first run before any fetch has
    /// landed, and whenever a downloaded catalog fails to parse. It is a
    /// floor, never a ceiling.
    pub fn seed() -> Self {
        let models = vec![
            ModelSpec {
                id: "claude-sonnet-5".into(),
                display_name: "Sonnet 5".into(),
                provider: Provider::Anthropic,
                tagline: "Balanced speed and depth. The everyday driver.".into(),
                input_price_per_mtok: 3.0,
                output_price_per_mtok: 15.0,
                max_tokens: 16384,
                timeout_secs: 180,
                vision: true,
            },
            ModelSpec {
                id: "claude-opus-5".into(),
                display_name: "Opus 5".into(),
                provider: Provider::Anthropic,
                tagline: "Deeper reasoning for work that has to be right.".into(),
                input_price_per_mtok: 5.0,
                output_price_per_mtok: 25.0,
                max_tokens: 32000,
                timeout_secs: 300,
                vision: true,
            },
            ModelSpec {
                id: "claude-fable-5-1".into(),
                display_name: "Fable 5.1".into(),
                provider: Provider::Anthropic,
                tagline: "Always-on thinking. The advisor on hard calls.".into(),
                input_price_per_mtok: 10.0,
                output_price_per_mtok: 50.0,
                max_tokens: 32000,
                timeout_secs: 360,
                vision: true,
            },
            ModelSpec {
                id: "grok-4.6".into(),
                display_name: "Grok 4.6".into(),
                provider: Provider::Xai,
                tagline: "Fast and cheap, with live search built in.".into(),
                input_price_per_mtok: 2.0,
                output_price_per_mtok: 6.0,
                max_tokens: 16384,
                timeout_secs: 180,
                vision: true,
            },
            ModelSpec {
                id: "gpt-6-astra".into(),
                display_name: "GPT-6 Astra".into(),
                provider: Provider::OpenAi,
                tagline: "OpenAI flagship. Long context, agentic reasoning.".into(),
                input_price_per_mtok: 10.0,
                output_price_per_mtok: 50.0,
                max_tokens: 32000,
                timeout_secs: 300,
                vision: true,
            },
        ];

        let aliases = [
            ("grok-4.5", "grok-4.6"),
            ("claude-fable-5", "claude-fable-5-1"),
        ]
        .into_iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();

        Self {
            schema: CATALOG_SCHEMA,
            version: 1,
            updated_at: "2026-09-07T00:00:00Z".into(),
            source: "seed".into(),
            models,
            default_model: "claude-sonnet-5".into(),
            advisor_model: "claude-fable-5-1".into(),
            aliases,
        }
    }

    /// Reject a downloaded catalog that could not serve a picker.
    ///
    /// The worker validates far more thoroughly before publishing; this is the
    /// client's own floor, because "the server checked it" is not a property
    /// this side can verify. Cheap, and it means a truncated or half-written
    /// cache file falls back to the seed instead of emptying the menu.
    fn is_usable(&self) -> Result<(), String> {
        if self.schema != CATALOG_SCHEMA {
            return Err(format!(
                "catalog schema {} but this build understands {CATALOG_SCHEMA}",
                self.schema
            ));
        }
        if self.models.is_empty() {
            return Err("catalog has no models".to_string());
        }
        for m in &self.models {
            if m.id.trim().is_empty() || m.display_name.trim().is_empty() {
                return Err("catalog has a model with no id or no display name".to_string());
            }
            if !(m.input_price_per_mtok.is_finite() && m.output_price_per_mtok.is_finite()) {
                return Err(format!("model {} has a non-finite price", m.id));
            }
            if m.max_tokens == 0 || m.timeout_secs == 0 {
                return Err(format!("model {} has a zero token cap or timeout", m.id));
            }
        }
        if !self.models.iter().any(|m| m.id == self.default_model) {
            return Err(format!(
                "default_model {} is not in the catalog",
                self.default_model
            ));
        }
        Ok(())
    }
}

/// The process-wide catalog. Leaked once so [`WorkshopModel`] can be a `Copy`
/// borrow of it for the life of the process.
static CATALOG: OnceLock<&'static Catalog> = OnceLock::new();

/// Install a catalog read from the on-disk cache. Call once during startup,
/// BEFORE anything reads the model list.
///
/// Returns `false` if a catalog is already in place, which means something
/// resolved a model before startup finished. The installed list then stands
/// for the rest of the session — deliberately: swapping the list mid-session
/// would change the picker, and possibly the price, under a running turn.
pub fn install_catalog(catalog: Catalog) -> bool {
    if let Err(why) = catalog.is_usable() {
        tracing::warn!("Workshop: ignoring unusable model catalog ({why}); keeping the built-in list");
        return false;
    }
    let version = catalog.version;
    let count = catalog.models.len();
    if CATALOG.set(Box::leak(Box::new(catalog))).is_err() {
        tracing::warn!("Workshop: model catalog already resolved, keeping it for this session");
        return false;
    }
    tracing::info!("Workshop: model catalog v{version} installed ({count} models)");
    true
}

/// The active catalog, falling back to the compiled-in seed.
pub fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(|| Box::leak(Box::new(Catalog::seed())))
}

/// A model the user can select to power Workshop's conversational loop.
///
/// A `Copy` handle into the leaked [`Catalog`], so it keeps the ergonomics the
/// enum had: passed by value, moved into the agentic thread, stored in
/// `AgenticInFlight`.
#[derive(Debug, Clone, Copy)]
pub struct WorkshopModel(&'static ModelSpec);

impl PartialEq for WorkshopModel {
    /// Compared by id rather than by pointer: two handles naming the same
    /// model are the same model, whichever catalog they were resolved from.
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}
impl Eq for WorkshopModel {}

impl WorkshopModel {
    /// Every selectable model, in the catalog's own order: grouped by
    /// provider, cheapest first inside each group.
    pub fn all() -> Vec<WorkshopModel> {
        catalog().models.iter().map(WorkshopModel).collect()
    }

    pub fn spec(&self) -> &'static ModelSpec {
        self.0
    }

    pub fn provider(&self) -> Provider {
        self.0.provider
    }

    /// The exact wire model id sent to the provider's API. Also what's
    /// persisted in `GlobalSoulSettings::workshop_model`.
    pub fn api_id(&self) -> &'static str {
        &self.0.id
    }

    /// Human-readable label — what the Workshop toolbar pill shows.
    pub fn display_name(&self) -> &'static str {
        &self.0.display_name
    }

    /// One short sentence on what this model is for. May be empty.
    pub fn tagline(&self) -> &'static str {
        &self.0.tagline
    }

    /// Per-request output token cap.
    pub fn max_tokens(&self) -> u32 {
        self.0.max_tokens
    }

    /// HTTP request timeout.
    pub fn timeout_secs(&self) -> u64 {
        self.0.timeout_secs
    }

    /// USD per million input tokens, standard rate.
    pub fn input_price_per_mtok(&self) -> f64 {
        self.0.input_price_per_mtok
    }

    /// USD per million output tokens, standard rate.
    pub fn output_price_per_mtok(&self) -> f64 {
        self.0.output_price_per_mtok
    }

    /// Whether this model can be sent images.
    pub fn vision(&self) -> bool {
        self.0.vision
    }

    /// True for the model the `consult_advisor` tool targets. A role read from
    /// the catalog, not a hardcoded identity, so retiring the advisor is a
    /// catalog edit rather than a code change.
    pub fn is_advisor(&self) -> bool {
        self.0.id == catalog().advisor_model
    }

    /// The advisor itself, for the bridge's out-of-band consult call.
    pub fn advisor() -> Option<WorkshopModel> {
        Self::from_api_id(&catalog().advisor_model)
    }

    /// The cheapest model from one specific provider.
    ///
    /// For call sites pinned to a single vendor's endpoint — the code-summary
    /// helper posts straight to `api.anthropic.com`, so it needs an Anthropic
    /// id specifically. Using [`WorkshopModel::default`] there would break the
    /// day the catalog's default became another vendor's model, by sending
    /// that vendor's id to Anthropic.
    ///
    /// Relies on the catalog being sorted cheapest-first within a provider,
    /// which `seed_is_grouped_by_provider_then_cheapest_first` pins and the
    /// worker enforces on every published catalog.
    pub fn cheapest_for(provider: Provider) -> Option<WorkshopModel> {
        catalog()
            .models
            .iter()
            .find(|m| m.provider == provider)
            .map(WorkshopModel)
    }

    /// Estimate the USD cost of one call using this model's token usage.
    pub fn estimate_cost(&self, input_tokens: u32, output_tokens: u32) -> f64 {
        (input_tokens as f64 / 1_000_000.0) * self.input_price_per_mtok()
            + (output_tokens as f64 / 1_000_000.0) * self.output_price_per_mtok()
    }

    /// Resolve a stored API id (from `GlobalSoulSettings::workshop_model`)
    /// back to a model.
    ///
    /// Also follows the catalog's alias table for superseded models. Without
    /// it, a user whose settings still hold a retired id would fail the
    /// lookup, fall through `effective_workshop_model`'s `unwrap_or_default()`
    /// and be silently moved to the default — a different provider at a
    /// different price, with no notice. Retiring a model must upgrade its
    /// users, not quietly reassign them.
    pub fn from_api_id(id: &str) -> Option<Self> {
        let cat = catalog();
        if let Some(m) = cat.models.iter().find(|m| m.id == id) {
            return Some(WorkshopModel(m));
        }
        let replacement = cat.aliases.get(id)?;
        cat.models
            .iter()
            .find(|m| &m.id == replacement)
            .map(WorkshopModel)
    }

    /// Resolve a Slint-facing display name back to a model.
    pub fn from_display_name(name: &str) -> Option<Self> {
        catalog()
            .models
            .iter()
            .find(|m| m.display_name == name)
            .map(WorkshopModel)
    }
}

impl Default for WorkshopModel {
    /// The catalog's nominated default, or its first entry if that id is
    /// somehow absent. Never panics: the picker must always resolve to
    /// something, and [`Catalog::is_usable`] already refused any downloaded
    /// catalog that could not.
    fn default() -> Self {
        let cat = catalog();
        cat.models
            .iter()
            .find(|m| m.id == cat.default_model)
            .or_else(|| cat.models.first())
            .map(WorkshopModel)
            .expect("the seed catalog is never empty")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_ids_and_display_names_round_trip() {
        for model in WorkshopModel::all() {
            assert_eq!(WorkshopModel::from_api_id(model.api_id()), Some(model));
            assert_eq!(
                WorkshopModel::from_display_name(model.display_name()),
                Some(model)
            );
        }
    }

    #[test]
    fn retired_ids_upgrade_rather_than_silently_reverting() {
        // A settings file written before a rename must land on the successor,
        // not fall through to the default — that would move a user to another
        // provider, at another price, without telling them.
        let grok = WorkshopModel::from_api_id("grok-4.5").expect("grok-4.5 aliases forward");
        assert_eq!(grok.api_id(), "grok-4.6");
        assert_eq!(grok.provider(), Provider::Xai);

        let fable = WorkshopModel::from_api_id("claude-fable-5").expect("fable-5 aliases forward");
        assert_eq!(fable.api_id(), "claude-fable-5-1");
        assert_eq!(fable.provider(), Provider::Anthropic);
    }

    #[test]
    fn unknown_ids_still_reject() {
        // The alias table must not turn into a catch-all that hides typos.
        assert_eq!(WorkshopModel::from_api_id("gpt-4"), None);
        assert_eq!(WorkshopModel::from_api_id(""), None);
    }

    #[test]
    fn seed_covers_every_whitelisted_provider() {
        // Every provider the engine can route to needs a model in the offline
        // list, or a user with only that vendor's key has an empty picker.
        let seed = Catalog::seed();
        for provider in [Provider::Anthropic, Provider::Xai, Provider::OpenAi] {
            assert!(
                seed.models.iter().any(|m| m.provider == provider),
                "seed catalog has no {} model",
                provider.label()
            );
        }
    }

    #[test]
    fn seed_is_grouped_by_provider_then_cheapest_first() {
        // The picker renders catalog order directly, so the order IS the
        // contract: sections stay put and the cheapest option reads first.
        let seed = Catalog::seed();
        let mut seen: Vec<Provider> = Vec::new();
        for group in seed.models.chunk_by(|a, b| a.provider == b.provider) {
            assert!(
                !seen.contains(&group[0].provider),
                "{} models are split across the list instead of grouped",
                group[0].provider.label()
            );
            seen.push(group[0].provider);
            let prices: Vec<f64> = group.iter().map(|m| m.input_price_per_mtok).collect();
            assert!(
                prices.windows(2).all(|w| w[0] <= w[1]),
                "{} models are not cheapest-first: {prices:?}",
                group[0].provider.label()
            );
        }
    }

    #[test]
    fn seed_names_a_default_and_an_advisor_that_exist() {
        let seed = Catalog::seed();
        seed.is_usable().expect("the seed catalog must be usable");
        assert!(
            seed.models.iter().any(|m| m.id == seed.advisor_model),
            "advisor_model {} is not in the seed catalog",
            seed.advisor_model
        );
    }

    #[test]
    fn seed_aliases_all_point_at_live_models() {
        // An alias to a model that no longer exists strands the very user it
        // was written to rescue.
        let seed = Catalog::seed();
        for (from, to) in &seed.aliases {
            assert!(
                seed.models.iter().any(|m| &m.id == to),
                "alias {from} -> {to} points at a model not in the catalog"
            );
            assert!(
                !seed.models.iter().any(|m| &m.id == from),
                "alias {from} shadows a model that is still live"
            );
        }
    }

    #[test]
    fn unusable_catalogs_are_refused() {
        // Each of these would have produced a broken or empty picker.
        let mut empty = Catalog::seed();
        empty.models.clear();
        assert!(empty.is_usable().is_err(), "an empty catalog must be refused");

        let mut wrong_schema = Catalog::seed();
        wrong_schema.schema = CATALOG_SCHEMA + 1;
        assert!(
            wrong_schema.is_usable().is_err(),
            "a future schema must be refused, not half-read"
        );

        let mut dangling_default = Catalog::seed();
        dangling_default.default_model = "nonexistent-model".into();
        assert!(
            dangling_default.is_usable().is_err(),
            "a default naming no model must be refused"
        );

        let mut nan_price = Catalog::seed();
        nan_price.models[0].input_price_per_mtok = f64::NAN;
        assert!(
            nan_price.is_usable().is_err(),
            "a non-finite price must be refused before it reaches a cost estimate"
        );
    }

    #[test]
    fn provider_whitelist_rejects_unknown_vendors() {
        assert_eq!(Provider::from_id("anthropic"), Some(Provider::Anthropic));
        assert_eq!(Provider::from_id("xai"), Some(Provider::Xai));
        assert_eq!(Provider::from_id("openai"), Some(Provider::OpenAi));
        assert_eq!(Provider::from_id("acme-labs"), None);
        assert_eq!(Provider::from_id("Anthropic"), None, "ids are lowercase");
    }

    #[test]
    fn cheapest_for_stays_inside_its_provider() {
        // The code-summary helper posts to a vendor-specific endpoint, so a
        // lookup that ever returned another vendor's id would 404 there.
        for provider in [Provider::Anthropic, Provider::Xai, Provider::OpenAi] {
            let model = WorkshopModel::cheapest_for(provider)
                .unwrap_or_else(|| panic!("seed has no {} model", provider.label()));
            assert_eq!(model.provider(), provider);
            let cheapest = WorkshopModel::all()
                .into_iter()
                .filter(|m| m.provider() == provider)
                .map(|m| m.input_price_per_mtok())
                .fold(f64::INFINITY, f64::min);
            assert_eq!(model.input_price_per_mtok(), cheapest);
        }
    }

    #[test]
    fn cost_estimate_uses_both_rates() {
        let model = WorkshopModel::from_api_id("claude-sonnet-5").expect("seed has sonnet");
        // 1M in at $3 + 1M out at $15.
        let cost = model.estimate_cost(1_000_000, 1_000_000);
        assert!((cost - 18.0).abs() < 1e-9, "expected $18.00, got {cost}");
    }
}
