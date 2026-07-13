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
    Fable5,
    Grok45,
}

impl WorkshopModel {
    pub const ALL: [WorkshopModel; 3] = [
        WorkshopModel::Sonnet5,
        WorkshopModel::Fable5,
        WorkshopModel::Grok45,
    ];

    pub fn provider(&self) -> Provider {
        match self {
            WorkshopModel::Sonnet5 | WorkshopModel::Fable5 => Provider::Anthropic,
            WorkshopModel::Grok45 => Provider::Xai,
        }
    }

    /// The exact wire model id sent to the provider's API. Also what's
    /// persisted in `GlobalSoulSettings::workshop_model`.
    pub fn api_id(&self) -> &'static str {
        match self {
            WorkshopModel::Sonnet5 => "claude-sonnet-5",
            WorkshopModel::Fable5 => "claude-fable-5",
            WorkshopModel::Grok45 => "grok-4.5",
        }
    }

    /// Human-readable label — what the Workshop toolbar pill shows.
    pub fn display_name(&self) -> &'static str {
        match self {
            WorkshopModel::Sonnet5 => "Sonnet 5",
            WorkshopModel::Fable5 => "Fable 5",
            WorkshopModel::Grok45 => "Grok 4.5",
        }
    }

    /// Per-request output token cap. Fable 5 gets more headroom than the
    /// others: its thinking is always on and counts toward the same budget,
    /// and turns can run for minutes.
    pub fn max_tokens(&self) -> u32 {
        match self {
            WorkshopModel::Sonnet5 => 16384,
            WorkshopModel::Fable5 => 32000,
            WorkshopModel::Grok45 => 16384,
        }
    }

    /// HTTP request timeout. Fable 5's advisor calls in particular can run
    /// several minutes on hard questions.
    pub fn timeout_secs(&self) -> u64 {
        match self {
            WorkshopModel::Sonnet5 => 180,
            WorkshopModel::Fable5 => 360,
            WorkshopModel::Grok45 => 180,
        }
    }

    /// USD per million input tokens, standard rate. Sonnet 5 currently also
    /// has intro pricing ($2/$10 per MTok) through 2026-08-31 that isn't
    /// reflected here — estimates run conservatively high until then, not
    /// under-counted.
    pub fn input_price_per_mtok(&self) -> f64 {
        match self {
            WorkshopModel::Sonnet5 => 3.0,
            WorkshopModel::Fable5 => 10.0,
            WorkshopModel::Grok45 => 2.0,
        }
    }

    /// USD per million output tokens, standard rate (see `input_price_per_mtok`
    /// for the Sonnet 5 intro-pricing caveat).
    pub fn output_price_per_mtok(&self) -> f64 {
        match self {
            WorkshopModel::Sonnet5 => 15.0,
            WorkshopModel::Fable5 => 50.0,
            WorkshopModel::Grok45 => 6.0,
        }
    }

    /// Estimate the USD cost of one call using this model's token usage.
    pub fn estimate_cost(&self, input_tokens: u32, output_tokens: u32) -> f64 {
        (input_tokens as f64 / 1_000_000.0) * self.input_price_per_mtok()
            + (output_tokens as f64 / 1_000_000.0) * self.output_price_per_mtok()
    }

    /// Resolve a stored API id (from `GlobalSoulSettings::workshop_model`)
    /// back to a model.
    pub fn from_api_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.api_id() == id)
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
