//! # Workshop Model Selection
//!
//! Which model answers Workshop's conversational agentic loop: a distinct
//! axis from [`eustress_common::soul::ModelTier`], which drives the Soul
//! *build pipeline*'s complexity-derived Haiku/Sonnet/Opus selection (English
//! to Rune codegen) and the Workshop session-title Haiku helper. Those stay
//! untouched; `WorkshopModel` is purely "which model the user picked to chat
//! with in Workshop."
//!
//! ## Where the list comes from
//!
//! Two sources, and only two, both of them the user's or Eustress's own:
//!
//! * **The curated table** ([`Catalog::curated`]), compiled into the engine:
//!   the models Eustress knows, with a display name, a tagline and the list
//!   price from each provider's published pricing.
//! * **The provider's own model list**, fetched with the user's own API key
//!   by [`super::model_catalog`]. A curated model is offered only if its
//!   provider lists it for that key, and a model the table does not know
//!   appears only if its provider lists it.
//!
//! No language model, web page or Eustress server decides what is offered.
//! That is deliberate: the list used to be compiled nightly by Grok reading
//! live web search, which could only ever be as honest as the pages it read.
//! It published an alias moving everyone on Claude Opus 5 to Opus 5.5 while
//! Opus 5 was still served, and a page planting a convincing fake id would
//! have passed every shape and range check. A provider's authenticated
//! `/v1/models` cannot be planted.
//!
//! `WorkshopModel` stays `Copy`: it is a `&'static ModelSpec` into a catalog
//! leaked when it is installed, so it crosses into the agentic worker thread
//! by value exactly as the old enum did, and a turn already running keeps its
//! model even if a fresh list is installed underneath it.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

/// Which backend a [`WorkshopModel`] talks to.
///
/// Closed on purpose. Every provider needs a wire-format client and a key to
/// be routable, so a vendor that is not a variant here has nowhere to send a
/// request, and [`Provider::from_id`] refuses anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    Anthropic,
    Xai,
    OpenAi,
}

impl Provider {
    /// Every provider, in the order the picker groups them.
    pub const ALL: [Provider; 3] = [Provider::Anthropic, Provider::Xai, Provider::OpenAi];

    /// Resolve a provider id. `None` for anything off the list.
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

/// How a model came to be in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelOrigin {
    /// In the compiled-in table.
    Curated,
    /// Not in the table: the provider listed it for the user's key, and it is
    /// newer than anything curated from that provider. Shown as new.
    Discovered,
}

/// One model the user can select to power Workshop's conversational loop.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelSpec {
    /// The exact wire model id sent to the provider's API. Also what's
    /// persisted in `GlobalSoulSettings::workshop_model`.
    pub id: String,
    /// Human-readable label: what the Workshop toolbar pill shows.
    pub display_name: String,
    pub provider: Provider,
    /// One short sentence on what this model is FOR, shown under the name in
    /// the picker. May be empty; the menu lays out without it.
    pub tagline: String,
    /// USD per million input tokens, standard rate. `None` when no trusted
    /// source states it: a discovered model from a provider whose model list
    /// carries no prices. Unknown is shown as unknown, never guessed.
    pub input_price_per_mtok: Option<f64>,
    /// USD per million output tokens, standard rate. `None` as above.
    pub output_price_per_mtok: Option<f64>,
    /// Per-request output token cap. Reasoning models whose thinking shares
    /// the budget get more headroom.
    pub max_tokens: u32,
    /// HTTP request timeout. Advisor calls on hard questions can run minutes.
    pub timeout_secs: u64,
    pub vision: bool,
    pub origin: ModelOrigin,
}

/// The full model list, plus the two roles the bridge needs to resolve.
#[derive(Debug, Clone)]
pub struct Catalog {
    pub models: Vec<ModelSpec>,
    /// Selected when the user has never chosen, or when their stored id
    /// resolves to nothing at all.
    pub default_model: String,
    /// The model the `consult_advisor` tool targets. Kept as a *role* rather
    /// than a hardcoded identity so changing the advisor is a table edit.
    /// Empty when the advisor is not available to this user's key, which
    /// withholds the tool.
    pub advisor_model: String,
    /// Retired or superseded id to its replacement, so a user whose settings
    /// hold an old id is upgraded rather than silently reassigned to the
    /// default (another provider, another price, no notice). Written only by
    /// a person editing [`Catalog::curated`], or declared by a provider's own
    /// model list; never inferred.
    pub aliases: HashMap<String, String>,
}

/// A curated table entry, kept short so the table below reads as a table.
fn curated(
    id: &str,
    display_name: &str,
    provider: Provider,
    tagline: &str,
    (input, output): (f64, f64),
    max_tokens: u32,
    timeout_secs: u64,
) -> ModelSpec {
    ModelSpec {
        id: id.into(),
        display_name: display_name.into(),
        provider,
        tagline: tagline.into(),
        input_price_per_mtok: Some(input),
        output_price_per_mtok: Some(output),
        max_tokens,
        timeout_secs,
        vision: true,
        origin: ModelOrigin::Curated,
    }
}

impl Catalog {
    /// The compiled-in table.
    ///
    /// What an engine offers before the providers answer, when offline, and
    /// for any provider whose key is not set. Prices are each provider's
    /// published standard rate (Anthropic's as of 2026-09-25, where Sonnet 5's
    /// launch price of $2/$10 became its standard price). Ordered cheapest
    /// first within each provider: the picker renders this order.
    pub fn curated() -> Self {
        use Provider::*;
        let models = vec![
            curated("claude-sonnet-5", "Sonnet 5", Anthropic,
                "Balanced speed and depth. The everyday driver.", (2.0, 10.0), 16384, 180),
            curated("claude-opus-5-5", "Opus 5.5", Anthropic,
                "The newest Opus. Deep reasoning at a lower price.", (4.0, 20.0), 32000, 300),
            curated("claude-opus-5", "Opus 5", Anthropic,
                "Deeper reasoning for work that has to be right.", (5.0, 25.0), 32000, 300),
            curated("claude-fable-5-1", "Fable 5.1", Anthropic,
                "Always-on thinking. The advisor on hard calls.", (10.0, 50.0), 32000, 360),
            curated("grok-4.6", "Grok 4.6", Xai,
                "Fast and cheap, with live search built in.", (2.0, 6.0), 16384, 180),
            curated("gpt-6-astra", "GPT-6 Astra", OpenAi,
                "OpenAI flagship. Long context, agentic reasoning.", (10.0, 50.0), 32000, 300),
        ];

        let aliases = [
            ("grok-4.5", "grok-4.6"),
            // Asked for by the user: Fable 5 conversations move to 5.1, the
            // same tier at the same price.
            ("claude-fable-5", "claude-fable-5-1"),
        ]
        .into_iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();

        Self {
            models,
            default_model: "claude-sonnet-5".into(),
            advisor_model: "claude-fable-5-1".into(),
            aliases,
        }
    }

    /// Refuse a catalog that could not serve a picker. Cheap insurance on
    /// every install: a resolution bug must degrade to the curated table, not
    /// an empty menu or a cost estimate built on a NaN.
    pub(crate) fn is_usable(&self) -> Result<(), String> {
        if self.models.is_empty() {
            return Err("catalog has no models".to_string());
        }
        for m in &self.models {
            if m.id.trim().is_empty() || m.display_name.trim().is_empty() {
                return Err("catalog has a model with no id or no display name".to_string());
            }
            let prices = [m.input_price_per_mtok, m.output_price_per_mtok];
            if prices.iter().flatten().any(|p| !p.is_finite() || *p < 0.0) {
                return Err(format!("model {} has an invalid price", m.id));
            }
            if m.max_tokens == 0 || m.timeout_secs == 0 {
                return Err(format!("model {} has a zero token cap or timeout", m.id));
            }
        }
        if !self.models.iter().any(|m| m.id == self.default_model) {
            return Err(format!("default_model {} is not in the catalog", self.default_model));
        }
        Ok(())
    }
}

/// The compiled-in table, leaked once on first use.
static CURATED: OnceLock<&'static Catalog> = OnceLock::new();

/// The catalog resolved against the user's provider lists, once one has been
/// installed. `None` until then, which reads as the curated table.
static INSTALLED: RwLock<Option<&'static Catalog>> = RwLock::new(None);

/// Install a catalog resolved against the providers' own lists.
///
/// Each install leaks one catalog (a few kilobytes). That is what lets
/// [`WorkshopModel`] stay a `Copy` reference: a turn already running holds a
/// model from the previous catalog, and that reference must stay valid. It
/// happens at launch and when a key changes, so the total stays small.
///
/// Returns `false`, keeping the current list, if the catalog is unusable.
pub fn install_catalog(catalog: Catalog) -> bool {
    if let Err(why) = catalog.is_usable() {
        tracing::warn!("Workshop: ignoring an unusable model catalog ({why}); keeping the current list");
        return false;
    }
    let count = catalog.models.len();
    let leaked: &'static Catalog = Box::leak(Box::new(catalog));
    *INSTALLED.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(leaked);
    tracing::info!("Workshop: model list installed ({count} models)");
    true
}

/// The active catalog: the installed one, or the curated table before any
/// provider has answered.
pub fn catalog() -> &'static Catalog {
    let installed = *INSTALLED.read().unwrap_or_else(|poisoned| poisoned.into_inner());
    installed.unwrap_or_else(|| *CURATED.get_or_init(|| Box::leak(Box::new(Catalog::curated()))))
}

/// A model the user can select to power Workshop's conversational loop.
///
/// A `Copy` handle into a leaked [`Catalog`], so it keeps the ergonomics the
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
    /// provider, cheapest first inside each group, unknown prices last.
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

    /// Human-readable label: what the Workshop toolbar pill shows.
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

    /// USD per million input tokens, standard rate, when known.
    pub fn input_price_per_mtok(&self) -> Option<f64> {
        self.0.input_price_per_mtok
    }

    /// USD per million output tokens, standard rate, when known.
    pub fn output_price_per_mtok(&self) -> Option<f64> {
        self.0.output_price_per_mtok
    }

    /// Whether this model can be sent images.
    pub fn vision(&self) -> bool {
        self.0.vision
    }

    /// Listed by the provider but not in the curated table.
    pub fn is_discovered(&self) -> bool {
        self.0.origin == ModelOrigin::Discovered
    }

    /// True for the model the `consult_advisor` tool targets. A role read from
    /// the catalog, not a hardcoded identity.
    pub fn is_advisor(&self) -> bool {
        self.0.id == catalog().advisor_model
    }

    /// The advisor itself, for the bridge's out-of-band consult call. `None`
    /// when the user's key cannot reach it.
    pub fn advisor() -> Option<WorkshopModel> {
        Self::from_api_id(&catalog().advisor_model)
    }

    /// The cheapest model from one specific provider.
    ///
    /// For call sites pinned to a single vendor's endpoint: the code-summary
    /// helper posts straight to `api.anthropic.com`, so it needs an Anthropic
    /// id specifically. Using [`WorkshopModel::default`] there would break the
    /// day the default became another vendor's model, by sending that vendor's
    /// id to Anthropic.
    ///
    /// Relies on the catalog order (cheapest first within a provider, unknown
    /// prices last), which `curated_is_grouped_by_provider_then_cheapest_first`
    /// pins and `model_catalog::resolve` keeps.
    pub fn cheapest_for(provider: Provider) -> Option<WorkshopModel> {
        catalog()
            .models
            .iter()
            .find(|m| m.provider == provider)
            .map(WorkshopModel)
    }

    /// Estimate the USD cost of one call from its token usage. `None` for a
    /// model whose price no trusted source states.
    pub fn estimate_cost(&self, input_tokens: u32, output_tokens: u32) -> Option<f64> {
        Some(
            (input_tokens as f64 / 1_000_000.0) * self.input_price_per_mtok()?
                + (output_tokens as f64 / 1_000_000.0) * self.output_price_per_mtok()?,
        )
    }

    /// Resolve a stored API id (from `GlobalSoulSettings::workshop_model`)
    /// back to a model.
    ///
    /// Also follows the catalog's alias table for superseded models. Without
    /// it, a user whose settings still hold a retired id would fail the
    /// lookup, fall through `effective_workshop_model`'s `unwrap_or_default()`
    /// and be silently moved to the default: a different provider at a
    /// different price, with no notice.
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
    /// somehow absent. Never panics: [`Catalog::is_usable`] refuses any
    /// catalog without models before it can be installed.
    fn default() -> Self {
        let cat = catalog();
        cat.models
            .iter()
            .find(|m| m.id == cat.default_model)
            .or_else(|| cat.models.first())
            .map(WorkshopModel)
            .expect("an installed catalog is never empty")
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
        // not fall through to the default: that would move a user to another
        // provider, at another price, without telling them.
        let grok = WorkshopModel::from_api_id("grok-4.5").expect("grok-4.5 aliases forward");
        assert_eq!(grok.api_id(), "grok-4.6");
        assert_eq!(grok.provider(), Provider::Xai);

        let fable = WorkshopModel::from_api_id("claude-fable-5").expect("fable-5 aliases forward");
        assert_eq!(fable.api_id(), "claude-fable-5-1");
        assert_eq!(fable.provider(), Provider::Anthropic);
    }

    #[test]
    fn a_still_served_model_is_never_aliased_away() {
        // The Grok-compiled list aliased claude-opus-5 to claude-opus-5-5
        // while Opus 5 was still served, moving its users to a different model
        // without asking. The curated table offers both.
        assert_eq!(
            WorkshopModel::from_api_id("claude-opus-5").map(|m| m.api_id()),
            Some("claude-opus-5")
        );
        assert!(!Catalog::curated().aliases.contains_key("claude-opus-5"));
    }

    #[test]
    fn unknown_ids_still_reject() {
        // The alias table must not turn into a catch-all that hides typos.
        assert_eq!(WorkshopModel::from_api_id("gpt-4"), None);
        assert_eq!(WorkshopModel::from_api_id(""), None);
    }

    #[test]
    fn curated_covers_every_provider() {
        // Every provider the engine can route to needs a model in the offline
        // list, or a user with only that vendor's key has an empty picker.
        let curated = Catalog::curated();
        for provider in Provider::ALL {
            assert!(
                curated.models.iter().any(|m| m.provider == provider),
                "curated table has no {} model",
                provider.label()
            );
        }
    }

    #[test]
    fn curated_is_grouped_by_provider_then_cheapest_first() {
        // The picker renders catalog order directly, so the order IS the
        // contract: sections stay put and the cheapest option reads first.
        let curated = Catalog::curated();
        let mut seen: Vec<Provider> = Vec::new();
        for group in curated.models.chunk_by(|a, b| a.provider == b.provider) {
            assert!(
                !seen.contains(&group[0].provider),
                "{} models are split across the list instead of grouped",
                group[0].provider.label()
            );
            seen.push(group[0].provider);
            let prices: Vec<f64> = group.iter().filter_map(|m| m.input_price_per_mtok).collect();
            assert!(
                prices.windows(2).all(|w| w[0] <= w[1]),
                "{} models are not cheapest-first: {prices:?}",
                group[0].provider.label()
            );
        }
    }

    #[test]
    fn curated_names_a_default_and_an_advisor_that_exist() {
        let curated = Catalog::curated();
        curated.is_usable().expect("the curated table must be usable");
        assert!(
            curated.models.iter().any(|m| m.id == curated.advisor_model),
            "advisor_model {} is not in the curated table",
            curated.advisor_model
        );
    }

    #[test]
    fn curated_prices_are_all_known() {
        // Unknown is for models a provider lists that Eustress has never seen.
        // Everything in the table carries its published price.
        for m in Catalog::curated().models {
            assert!(m.input_price_per_mtok.is_some() && m.output_price_per_mtok.is_some(), "{}", m.id);
        }
    }

    #[test]
    fn curated_aliases_all_point_at_live_models() {
        // An alias to a model that no longer exists strands the very user it
        // was written to rescue.
        let curated = Catalog::curated();
        for (from, to) in &curated.aliases {
            assert!(
                curated.models.iter().any(|m| &m.id == to),
                "alias {from} -> {to} points at a model not in the table"
            );
            assert!(
                !curated.models.iter().any(|m| &m.id == from),
                "alias {from} shadows a model that is still live"
            );
        }
    }

    #[test]
    fn unusable_catalogs_are_refused() {
        // Each of these would have produced a broken or empty picker.
        let mut empty = Catalog::curated();
        empty.models.clear();
        assert!(empty.is_usable().is_err(), "an empty catalog must be refused");

        let mut dangling_default = Catalog::curated();
        dangling_default.default_model = "nonexistent-model".into();
        assert!(
            dangling_default.is_usable().is_err(),
            "a default naming no model must be refused"
        );

        let mut nan_price = Catalog::curated();
        nan_price.models[0].input_price_per_mtok = Some(f64::NAN);
        assert!(
            nan_price.is_usable().is_err(),
            "a non-finite price must be refused before it reaches a cost estimate"
        );

        let mut unknown_price = Catalog::curated();
        unknown_price.models[0].input_price_per_mtok = None;
        assert!(unknown_price.is_usable().is_ok(), "an unknown price is allowed, not invalid");
    }

    #[test]
    fn provider_ids_reject_unknown_vendors() {
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
        for provider in Provider::ALL {
            let model = WorkshopModel::cheapest_for(provider)
                .unwrap_or_else(|| panic!("no {} model", provider.label()));
            assert_eq!(model.provider(), provider);
            let cheapest = WorkshopModel::all()
                .into_iter()
                .filter(|m| m.provider() == provider)
                .filter_map(|m| m.input_price_per_mtok())
                .fold(f64::INFINITY, f64::min);
            assert_eq!(model.input_price_per_mtok(), Some(cheapest));
        }
    }

    #[test]
    fn cost_estimate_uses_both_rates() {
        let model = WorkshopModel::from_api_id("claude-sonnet-5").expect("table has sonnet");
        // 1M in at $2 + 1M out at $10.
        let cost = model.estimate_cost(1_000_000, 1_000_000).expect("sonnet has a price");
        assert!((cost - 12.0).abs() < 1e-9, "expected $12.00, got {cost}");
    }
}
