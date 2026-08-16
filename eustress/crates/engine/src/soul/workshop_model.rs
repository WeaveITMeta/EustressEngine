//! # Workshop Model Selection
//!
//! Which model answers Workshop's conversational agentic loop — a distinct
//! axis from [`eustress_common::soul::ModelTier`], which drives the Soul
//! *build pipeline*'s complexity-derived Haiku/Sonnet/Opus selection (English
//! → Rune codegen) and the Workshop session-title Haiku helper. Those stay
//! untouched; `WorkshopModel` is purely "which model the user picked to chat
//! with in Workshop."

/// Which backend a [`WorkshopModel`] talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Anthropic,
    Xai,
}

/// A model the user can select to power Workshop's conversational loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkshopModel {
    Sonnet5,
    Opus5,
    Fable5,
    Grok46,
}

impl WorkshopModel {
    /// Listed in ascending cost order — this is also the order the toolbar
    /// dropdown shows, so the cheapest option reads first.
    pub const ALL: [WorkshopModel; 4] = [
        WorkshopModel::Sonnet5,
        WorkshopModel::Opus5,
        WorkshopModel::Fable5,
        WorkshopModel::Grok46,
    ];

    pub fn provider(&self) -> Provider {
        match self {
            WorkshopModel::Sonnet5 | WorkshopModel::Opus5 | WorkshopModel::Fable5 => {
                Provider::Anthropic
            }
            WorkshopModel::Grok46 => Provider::Xai,
        }
    }

    /// The exact wire model id sent to the provider's API. Also what's
    /// persisted in `GlobalSoulSettings::workshop_model`.
    pub fn api_id(&self) -> &'static str {
        match self {
            WorkshopModel::Sonnet5 => "claude-sonnet-5",
            WorkshopModel::Opus5 => "claude-opus-5",
            WorkshopModel::Fable5 => "claude-fable-5",
            WorkshopModel::Grok46 => "grok-4.6",
        }
    }

    /// Human-readable label — what the Workshop toolbar pill shows.
    pub fn display_name(&self) -> &'static str {
        match self {
            WorkshopModel::Sonnet5 => "Sonnet 5",
            WorkshopModel::Opus5 => "Opus 5",
            WorkshopModel::Fable5 => "Fable 5",
            WorkshopModel::Grok46 => "Grok 4.6",
        }
    }

    /// Per-request output token cap. Fable 5 gets more headroom than the
    /// others: its thinking is always on and counts toward the same budget,
    /// and turns can run for minutes.
    pub fn max_tokens(&self) -> u32 {
        match self {
            WorkshopModel::Sonnet5 => 16384,
            WorkshopModel::Opus5 => 32000,
            WorkshopModel::Fable5 => 32000,
            WorkshopModel::Grok46 => 16384,
        }
    }

    /// HTTP request timeout. Fable 5's advisor calls in particular can run
    /// several minutes on hard questions.
    pub fn timeout_secs(&self) -> u64 {
        match self {
            WorkshopModel::Sonnet5 => 180,
            WorkshopModel::Opus5 => 300,
            WorkshopModel::Fable5 => 360,
            WorkshopModel::Grok46 => 180,
        }
    }

    /// USD per million input tokens, standard rate. Sonnet 5 currently also
    /// has intro pricing ($2/$10 per MTok) through 2026-08-31 that isn't
    /// reflected here — estimates run conservatively high until then, not
    /// under-counted.
    pub fn input_price_per_mtok(&self) -> f64 {
        match self {
            WorkshopModel::Sonnet5 => 3.0,
            // Opus 5 (released 2026-07-24) carries the same rates as Opus 4.8.
            WorkshopModel::Opus5 => 5.0,
            WorkshopModel::Fable5 => 10.0,
            WorkshopModel::Grok46 => 2.0,
        }
    }

    /// USD per million output tokens, standard rate (see `input_price_per_mtok`
    /// for the Sonnet 5 intro-pricing caveat).
    pub fn output_price_per_mtok(&self) -> f64 {
        match self {
            WorkshopModel::Sonnet5 => 15.0,
            WorkshopModel::Opus5 => 25.0,
            WorkshopModel::Fable5 => 50.0,
            WorkshopModel::Grok46 => 6.0,
        }
    }

    /// Estimate the USD cost of one call using this model's token usage.
    pub fn estimate_cost(&self, input_tokens: u32, output_tokens: u32) -> f64 {
        (input_tokens as f64 / 1_000_000.0) * self.input_price_per_mtok()
            + (output_tokens as f64 / 1_000_000.0) * self.output_price_per_mtok()
    }

    /// Resolve a stored API id (from `GlobalSoulSettings::workshop_model`)
    /// back to a model.
    ///
    /// Also accepts the api_ids of superseded models. Without this, a user
    /// whose settings still hold a retired id would fail the lookup, fall
    /// through `effective_workshop_model`'s `unwrap_or_default()`, and be
    /// silently moved to Sonnet 5 — a different provider at a different
    /// price, with no notice. Retiring a model must upgrade its users, not
    /// quietly reassign them.
    pub fn from_api_id(id: &str) -> Option<Self> {
        if let Some(model) = Self::ALL.into_iter().find(|m| m.api_id() == id) {
            return Some(model);
        }
        match id {
            // Grok 4.5 → 4.6 (xAI's flagship as of 2026-08); same $2/$6 rates.
            "grok-4.5" => Some(WorkshopModel::Grok46),
            _ => None,
        }
    }

    /// Resolve a Slint-facing display name back to a model.
    pub fn from_display_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.display_name() == name)
    }
}

impl Default for WorkshopModel {
    fn default() -> Self {
        WorkshopModel::Sonnet5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_ids_and_display_names_round_trip() {
        for model in WorkshopModel::ALL {
            assert_eq!(WorkshopModel::from_api_id(model.api_id()), Some(model));
            assert_eq!(
                WorkshopModel::from_display_name(model.display_name()),
                Some(model)
            );
        }
    }

    #[test]
    fn retired_grok_45_upgrades_rather_than_silently_reverting() {
        // A settings file written before the 4.6 rename must land on Grok,
        // not fall through to the Sonnet 5 default — that would move a user
        // to another provider without telling them.
        assert_eq!(
            WorkshopModel::from_api_id("grok-4.5"),
            Some(WorkshopModel::Grok46)
        );
        assert_eq!(WorkshopModel::from_api_id("grok-4.5").unwrap().provider(), Provider::Xai);
    }

    #[test]
    fn unknown_ids_still_reject() {
        // The alias table must not turn into a catch-all that hides typos.
        assert_eq!(WorkshopModel::from_api_id("gpt-4"), None);
        assert_eq!(WorkshopModel::from_api_id(""), None);
    }

    #[test]
    fn all_is_ordered_cheapest_first() {
        // The dropdown shows ALL in order; ascending cost is the contract.
        let costs: Vec<f64> = WorkshopModel::ALL
            .iter()
            .map(|m| m.input_price_per_mtok())
            .collect();
        // Grok is the outlier — cheapest of all but listed last as the only
        // non-Anthropic option, so check the Anthropic run is ascending.
        let anthropic: Vec<f64> = WorkshopModel::ALL
            .iter()
            .filter(|m| m.provider() == Provider::Anthropic)
            .map(|m| m.input_price_per_mtok())
            .collect();
        assert!(
            anthropic.windows(2).all(|w| w[0] <= w[1]),
            "Anthropic models must be listed cheapest-first, got {anthropic:?} (all: {costs:?})"
        );
    }
}
