//! # Soul Engine Integration
//!
//! Build pipeline and Rune scripting for Soul natural language scripts.
//!
//! ## Active Components
//!
//! - **Build Pipeline**: Claude generates Rune code from markdown
//! - **Claude Client**: HTTP client for Anthropic API
//! - **Rune API**: Rune script execution and validation
//! - **Scope**: Script location and system prompt building
//!
//! ## Stub Components (legacy, kept for compatibility)
//!
//! - builder, cache, codegen, validator, hot_compile, soul_context

pub mod builder;
pub mod cache;
pub mod codegen;
pub mod validator;
pub mod audit_log;
pub mod claude_client;
pub mod xai_client;
pub mod openai_client;
pub mod workshop_model;
pub mod model_catalog;
/// AI provider keys in the system credential store: what the screen
/// shows, and the one-time move out of `soul_settings.json`.
pub mod key_store;
pub mod hot_compile;
pub mod build_pipeline;
pub mod scope;
pub mod soul_context;
pub mod rune_api;
pub mod error_tracker;
pub mod vm_pool;
pub mod rune_ecs_module;
/// `eustress::dm`: Rune's access to the live Play DataModel Luau shares.
#[cfg(feature = "realism-scripting")]
pub mod rune_datamodel;
/// `eustress::terrain`: Rune's edits and reads of the Space's terrain in Play.
#[cfg(feature = "realism-scripting")]
pub mod rune_terrain;
/// Per-frame Rune execution for Play / Run mode — bridge install, lifecycle
/// callbacks, and effect drain, all inside ONE Bevy system. See the module
/// docs for why the previous multi-system split could not work.
pub mod rune_play;
pub mod physics_bridge;
pub mod gui_bridge;
pub mod parallel_execution;
/// L12 Kernel — Rune rewrite validator (agent-RSI accept/reject gate).
/// See `kernel/mod.rs` for the scope distinction vs. universe-level law RSI.
pub mod kernel;

pub use builder::*;
pub use cache::*;
pub use codegen::*;
pub use validator::*;
pub use claude_client::{ClaudeClient, ClaudeError, GenerationResult};
pub use xai_client::{XaiClient, XaiConfig};
pub use openai_client::{OpenAiClient, OpenAiConfig};
pub use workshop_model::{Catalog, ModelSpec, Provider, WorkshopModel};
pub use hot_compile::{HotCompiler, HotCompileConfig, CompileResult, HotCompilePlugin};
pub use build_pipeline::{SoulBuildPipeline, BuildStage, BuildRequest, PipelineResult, SoulBuildPipelinePlugin, TriggerBuildEvent, CommandBarBuildEvent, CommandBarBuildState, CommandBarResult};
pub use scope::{ScriptLocation, ScriptScope, AvailableEvents, SystemPromptBuilder};
pub use soul_context::{SoulContext, EntityHandle, Value, Shape, LightType, Easing, CommandScript, SoulServiceScript, SoulEntityScript, SoulWorkspaceScript};
pub use rune_api::{RuneScriptEngine, ScriptCommand, SpawnPhysics, execute_rune_script, validate_rune_script, EntityData, InputData, PhysicsData, update_world_state, update_input_state, update_mouse_raycast};
pub use error_tracker::{RuneErrorTracker, ErrorCategory, TrackedError, SuggestedAction, ErrorStats};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[allow(unused_imports)]
use eustress_common::soul::{SoulConfig, ScriptRegistry, SoulPlugin as CommonSoulPlugin, SoulAST};

// ============================================================================
// Global Soul Settings (Persisted to ~/.eustress_engine/soul_settings.json)
// ============================================================================

/// Global Soul settings that persist across sessions and spaces
/// Stored in ~/.eustress_engine/soul_settings.json, which holds no key of
/// this build's: the keys live in the system credential store (see
/// [`key_store`]). `Debug` is written by hand there, so no key reaches a
/// log. A missing field takes its default, so a file this build wrote
/// (with no key fields) reads back whole.
#[derive(Resource, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GlobalSoulSettings {
    /// This session's Anthropic key, from the credential store. serde
    /// never reads or writes it.
    #[serde(skip)]
    pub global_api_key: String,
    /// Whether the global API key has been validated
    #[serde(skip)]
    pub api_key_valid: Option<bool>,
    /// Auto-fill new spaces with global API key
    pub use_global_for_new_spaces: bool,
    /// Which model powers Workshop's conversational loop — the exact API id
    /// (see `WorkshopModel::api_id`). `#[serde(default)]` is required: a
    /// settings file saved before this field existed must still deserialize
    /// cleanly, since `load()` falls back to `Self::default()` (silently
    /// wiping the saved API key) on ANY parse failure.
    #[serde(default = "default_workshop_model")]
    pub workshop_model: String,
    /// This session's xAI (Grok) key, from the credential store. Global
    /// only. serde never reads or writes it.
    #[serde(skip)]
    pub global_xai_api_key: String,
    /// This session's OpenAI key, from the credential store. Global only,
    /// for the same reason: the model picker is a global preference, so the
    /// key it spends has no per-space meaning. serde never reads or writes it.
    #[serde(skip)]
    pub global_openai_api_key: String,
    /// Whether Workshop's Gauntlet (AAA verify-loop) mode is on. Defaults to
    /// OFF — it deliberately spends extra tokens per artifact, so it must be
    /// an explicit opt-in, never a silent default.
    #[serde(default)]
    pub workshop_gauntlet: bool,
    /// Whether Workshop runs tools that work inside the Space (read and write)
    /// without asking.
    ///
    /// Defaults to OFF, and stays an explicit opt-in for the same reason the
    /// gauntlet does. Turning it on trades the per-call confirmation for speed,
    /// which is the right trade during a long working session and the wrong one
    /// by default.
    ///
    /// It covers Read and Write tools ONLY. A tool that runs commands or code,
    /// deletes, or goes online (Execute, Destructive, Network) shows its
    /// approval card whatever this says: `workshop::approval_policy` decides
    /// that, and a call that runs without a card holds a Read and Write grant,
    /// so `tools::capability::authorize_call` refuses anything wider at
    /// dispatch. An unclassified tool is refused there too.
    ///
    /// Write includes writing scripts and files inside the Space, which a person
    /// later runs with Play. Auto saves the clicks on writing, never the click
    /// on running.
    #[serde(default)]
    pub workshop_auto_approve: bool,
    /// The keys an older build left in the settings file, as read at
    /// start-up, until each has moved into the credential store. serde
    /// skips them; `key_store` writes an unmoved one back as it was.
    #[serde(skip)]
    pub legacy_keys: [key_store::LegacyKey; 3],
    /// Each provider's key on screen: busy, warning, failure line. Holds
    /// no key.
    #[serde(skip)]
    pub key_slots: [key_store::KeySlot; 3],
}

fn default_workshop_model() -> String {
    WorkshopModel::default().api_id().to_string()
}

impl Default for GlobalSoulSettings {
    fn default() -> Self {
        Self {
            global_api_key: String::new(),
            api_key_valid: None,
            use_global_for_new_spaces: true,
            workshop_model: default_workshop_model(),
            global_xai_api_key: String::new(),
            global_openai_api_key: String::new(),
            // Opt-in only, per the field's contract.
            workshop_gauntlet: false,
            // Likewise: the tools behind the approval gate are the ones that
            // write files and run commands, so this is never a silent default.
            workshop_auto_approve: false,
            legacy_keys: Default::default(),
            key_slots: Default::default(),
        }
    }
}

impl GlobalSoulSettings {
    /// Get the settings file path (~/.eustress_engine/soul_settings.json)
    pub fn settings_path() -> Option<PathBuf> {
        if let Some(home) = dirs::home_dir() {
            let settings_dir = home.join(".eustress_engine");
            Some(settings_dir.join("soul_settings.json"))
        } else {
            None
        }
    }
    
    /// Load settings from file or create default. Never fails; the keys
    /// are not in the file (see [`key_store`]).
    pub fn load() -> Self {
        Self::load_with_report().0
    }

    /// [`Self::load`], with what start-up needs to move an older build's
    /// keys out of the file.
    pub fn load_with_report() -> (Self, key_store::SettingsLoad) {
        match Self::settings_path() {
            Some(path) => Self::load_from(&path),
            None => {
                warn!("Could not determine home directory. Using default Soul settings.");
                (Self::default(), key_store::SettingsLoad::default())
            }
        }
    }

    /// Save settings to file. Writes no key of this build's; an older
    /// build's key that has not moved yet is written back as it was.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::settings_path()
            .ok_or_else(|| "Could not determine home directory".to_string())?;
        self.save_to(&path, None)?;
        info!("Saved global Soul settings to {:?}", path);
        Ok(())
    }
    
    /// Check if the global API key is set
    pub fn has_api_key(&self) -> bool {
        !self.global_api_key.trim().is_empty()
    }

    /// Resolve `workshop_model` to a `WorkshopModel`, falling back to the
    /// default (Sonnet 5) for an empty/unrecognized stored value.
    pub fn effective_workshop_model(&self) -> WorkshopModel {
        WorkshopModel::from_api_id(&self.workshop_model).unwrap_or_default()
    }

    /// Trimmed xAI API key, or `None` if unset.
    pub fn effective_xai_api_key(&self) -> Option<String> {
        let key = self.global_xai_api_key.trim();
        if key.is_empty() { None } else { Some(key.to_string()) }
    }

    /// Trimmed OpenAI API key, or `None` if unset.
    pub fn effective_openai_api_key(&self) -> Option<String> {
        let key = self.global_openai_api_key.trim();
        if key.is_empty() { None } else { Some(key.to_string()) }
    }

    /// The BYOK key this model's provider spends, or `None` if that field is
    /// empty. Centralised so a new provider is one arm here rather than a
    /// hunt through every call site that needs a key.
    pub fn key_for_provider(&self, provider: Provider) -> Option<String> {
        match provider {
            Provider::Anthropic => {
                let key = self.global_api_key.trim();
                if key.is_empty() { None } else { Some(key.to_string()) }
            }
            Provider::Xai => self.effective_xai_api_key(),
            Provider::OpenAi => self.effective_openai_api_key(),
        }
    }
}

// ============================================================================
// Per-Space SoulService Settings (UI-editable, saved with scene)
// ============================================================================

/// Default telemetry to enabled (opt-out)
fn default_telemetry_enabled() -> bool {
    true
}

/// Resource for the SoulService settings of the open Space.
///
/// It holds no key. A Space never carries an API key: Soul spends the one
/// global key per provider, kept in the system credential store, and a key
/// found in a Space's SoulService file is removed and never used (see
/// `space::service_loader::take_legacy_soul_properties`).
#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub struct SoulServiceSettings {
    /// Whether the API key is valid (set after validation)
    #[serde(skip)]
    pub api_key_valid: Option<bool>,
    /// Whether to send anonymous error reports to help improve Eustress (opt-out, default on)
    #[serde(default = "default_telemetry_enabled")]
    pub telemetry_enabled: bool,
}

impl SoulServiceSettings {
    /// The Anthropic key Soul spends: the global one. Trimmed so a stray
    /// newline cannot break an HTTP header.
    pub fn effective_api_key(&self, global: &GlobalSoulSettings) -> String {
        global.global_api_key.trim().to_string()
    }

    /// Initialize from global settings (for new spaces). Carries no key.
    pub fn from_global(global: &GlobalSoulSettings) -> Self {
        if global.use_global_for_new_spaces && global.has_api_key() {
            Self {
                api_key_valid: None,
                telemetry_enabled: true, // Opt-out, default on
            }
        } else {
            Self::default()
        }
    }
}

// ============================================================================
// Soul Script Data Component
// ============================================================================

/// Determines the scripting runtime for a SoulScript.
/// Rune is the primary/recommended runtime for Soul scripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum SoulRunContext {
    /// Rune VM — primary Soul scripting runtime (compiled from markdown → Rune)
    #[default]
    Rune,
    /// Luau VM — Roblox-compatible scripting for `.luau` / `.lua` files.
    /// Hot-reloaded through `execute_chunk` on save.
    Luau,
    /// Server-side execution context
    Server,
    /// Client-side execution context
    Client,
}

impl SoulRunContext {
    /// String representation for display and TOML serialization
    pub fn as_str(&self) -> &'static str {
        match self {
            SoulRunContext::Rune => "Rune",
            SoulRunContext::Luau => "Luau",
            SoulRunContext::Server => "Server",
            SoulRunContext::Client => "Client",
        }
    }
}

/// Component attached to SoulScript entities containing the script source and compiled state
#[derive(Component, Debug, Clone, Default, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct SoulScriptData {
    /// The markdown source code
    pub source: String,
    /// Whether the script has been modified since last save
    pub dirty: bool,
    /// Compiled AST (if successfully parsed)
    #[serde(skip)]
    #[reflect(ignore)]
    pub ast: Option<SoulAST>,
    /// Generated Rust code (if successfully compiled)
    pub generated_code: Option<String>,
    /// Build status
    pub build_status: SoulBuildStatus,
    /// Errors from last build attempt
    pub errors: Vec<String>,
    /// Scripting runtime context — defaults to Rune (primary Soul runtime)
    #[serde(default)]
    pub run_context: SoulRunContext,
}

/// Build status for a Soul script
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum SoulBuildStatus {
    #[default]
    NotBuilt,
    Building,
    Built,
    Failed,
    Stale,
}

// ============================================================================
// Engine Soul Plugin
// ============================================================================

/// Soul scripting plugin for Eustress Engine (Studio)
pub struct EngineSoulPlugin;

impl Plugin for EngineSoulPlugin {
    fn build(&self, app: &mut App) {
        // Add common Soul plugin first
        app.add_plugins(CommonSoulPlugin);
        
        // Add hot compile plugin
        app.add_plugins(HotCompilePlugin);
        
        // Add build pipeline plugin (includes Claude integration)
        app.add_plugins(SoulBuildPipelinePlugin);
        
        // The Workshop model list is resolved against each provider's own
        // model list, fetched with the user's own keys (see `model_catalog`).
        // Until the providers answer, the compiled-in table stands.
        model_catalog::remove_retired_catalog_cache();

        // Load global Soul settings from disk
        // The keys are not in the file: each is looked up in the system
        // credential store off the UI thread, and its field shows
        // "checking" until the store answers.
        let (mut global_settings, settings_load) = GlobalSoulSettings::load_with_report();
        let mut soul_keys = key_store::SoulKeyStore::new(GlobalSoulSettings::settings_path());
        soul_keys.start_load(&mut global_settings, &settings_load);
        
        // Initialize space settings from global (respects "use global for new spaces" setting)
        let space_settings = SoulServiceSettings::from_global(&global_settings);
        
        // Add engine-specific resources
        app
            .init_resource::<SoulBuilder>()
            .init_resource::<SoulCache>()
            .insert_resource(global_settings)
            .insert_resource(soul_keys)
            .insert_resource(space_settings)
            .add_message::<BuildRequestEvent>()
            .add_message::<BuildCompleteEvent>()
            .add_systems(Update, (
                process_build_requests,
                update_build_status,
                cleanup_removed_soulscripts,
                sync_space_root_to_audit,
                poll_soul_keys,
                remove_space_soul_keys,
            ));

        // Ask the providers which models the user's keys can use: at launch,
        // and again whenever a key changes.
        app.init_resource::<model_catalog::ModelCatalogRevision>()
            .add_systems(Update, (
                model_catalog::rediscover_on_key_change,
                model_catalog::apply_discovered_models,
            ).chain());

        info!("EngineSoulPlugin initialized - Claude API + Hot Compile ready");
    }
}

/// Propagate [`crate::space::SpaceRoot`] changes into the audit module's
/// thread-local cell so Claude calls (which run outside Bevy's resource
/// system) know where to write their `.log.toml` entries.
fn sync_space_root_to_audit(
    space_root: Option<Res<crate::space::SpaceRoot>>,
) {
    if let Some(sr) = space_root {
        if sr.is_changed() || sr.is_added() {
            audit_log::set_current_space(Some(sr.0.clone()));
        }
    }
}

/// Pick up what the credential store answered, and send its lines to the
/// Output panel. Marks the settings changed only when something on screen
/// did, so the Slint sync systems do not run every frame.
fn poll_soul_keys(
    mut store: ResMut<key_store::SoulKeyStore>,
    mut global: ResMut<GlobalSoulSettings>,
    mut output: Option<ResMut<crate::ui::OutputConsole>>,
) {
    if store.bypass_change_detection().poll(global.bypass_change_detection()) {
        global.set_changed();
    }
    for line in store.bypass_change_detection().take_lines() {
        match line {
            key_store::Line::Info(text) => {
                info!("{}", text);
                if let Some(out) = output.as_mut() {
                    out.info(text);
                }
            }
            key_store::Line::Warn(text) => {
                warn!("{}", text);
                if let Some(out) = output.as_mut() {
                    out.warn(text);
                }
            }
        }
    }
}

/// A SoulService that loaded with a legacy key property (see
/// `service_loader::take_legacy_soul_properties`): remove it from the Space's
/// file at once, and from the Space database's record once that database is
/// open; compare the key with the saved one for the notice. The key is never
/// used, stored or set. The file half runs once per Space open. The database
/// half waits while a database open is in flight, and warns only for a
/// database-backed Space whose record could not be updated.
fn remove_space_soul_keys(
    mut commands: Commands,
    mut found: Query<(
        Entity,
        &crate::space::service_loader::ServiceComponent,
        &mut crate::space::service_loader::SoulServiceLegacyKey,
    )>,
    mut store: ResMut<key_store::SoulKeyStore>,
    auth: Option<Res<crate::auth::AuthState>>,
    pending_open: Option<Res<PendingSpaceDatabase>>,
) {
    use crate::space::service_loader::{self as loader, RecordStep};
    if found.is_empty() {
        return;
    }
    let open_settled = space_database_open_settled(pending_open);
    let stamp = auth.as_deref().and_then(crate::space::instance_loader::current_stamp);
    for (entity, service, mut legacy) in &mut found {
        let path = service.toml_path.clone();
        if !legacy.file_done {
            legacy.file_done = true;
            if let Err(e) = loader::remove_legacy_from_file(&path, stamp.as_ref()) {
                store.warn(format!("The SoulService file still holds a removed API key setting: {e}"));
            }
            if let Some(removed) = legacy.api_key.take() {
                store.check_space_key(removed);
            }
        }
        let step = loader::record_cleanup_step(
            open_settled,
            crate::space::active_db::is_active(),
            legacy.frames_waited,
            || loader::remove_legacy_from_record(&path, stamp.as_ref()),
            || crate::space::active_db::get_instance_text(&path).is_some_and(|text| loader::text_holds_legacy_soul_keys(&text)),
        );
        match step {
            RecordStep::Wait => legacy.frames_waited += 1,
            RecordStep::Done => {
                commands.entity(entity).remove::<crate::space::service_loader::SoulServiceLegacyKey>();
            }
            RecordStep::Warn(why) => {
                let space = path.parent().and_then(|p| p.parent()).unwrap_or(&path).display().to_string();
                store.warn(format!(
                    "The Space at {space} may still hold a removed API key in its local database until it is cleared: {why}."
                ));
                commands.entity(entity).remove::<crate::space::service_loader::SoulServiceLegacyKey>();
            }
        }
    }
}

/// The Space database open in flight, when the build has the database.
#[cfg(feature = "world-db")]
type PendingSpaceDatabase = crate::space::world_db_plugin::PendingWorldDbOpen;

/// A build without the Space database never has an open in flight.
#[cfg(not(feature = "world-db"))]
#[derive(Resource, Default)]
struct PendingSpaceDatabase;

#[cfg(feature = "world-db")]
fn space_database_open_settled(pending: Option<Res<PendingSpaceDatabase>>) -> bool {
    pending.map(crate::space::world_db_plugin::world_db_open_settled).unwrap_or(true)
}

#[cfg(not(feature = "world-db"))]
fn space_database_open_settled(_pending: Option<Res<PendingSpaceDatabase>>) -> bool {
    true
}


// ============================================================================
// Events
// ============================================================================

/// Request to build a script
#[derive(Event, Message, Debug, Clone)]
pub struct BuildRequestEvent {
    /// Scene name
    pub scene: String,
    /// Script path (optional - None = all scripts in scene)
    pub script: Option<String>,
    /// Force rebuild (ignore cache)
    pub force: bool,
}

/// Build completed
#[derive(Event, Message, Debug, Clone)]
pub struct BuildCompleteEvent {
    /// Scene name
    pub scene: String,
    /// Script path
    pub script: String,
    /// Success or failure
    pub success: bool,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Build duration (ms)
    pub duration_ms: u64,
}

// ============================================================================
// Systems
// ============================================================================

/// Process build requests
fn process_build_requests(
    mut events: MessageReader<BuildRequestEvent>,
    mut builder: ResMut<SoulBuilder>,
    _config: Res<SoulConfig>,
) {
    for event in events.read() {
        info!("Build request: scene={}, force={}", event.scene, event.force);
        
        builder.queue_build(BuildJob {
            scene: event.scene.clone(),
            script: event.script.clone(),
            force: event.force,
            status: BuildJobStatus::Pending,
            started_at: None,
        });
    }
}

/// Update build status
fn update_build_status(
    mut builder: ResMut<SoulBuilder>,
    mut complete_events: MessageWriter<BuildCompleteEvent>,
    _config: Res<SoulConfig>,
) {
    // Process pending builds
    if let Some(result) = builder.poll_result() {
        complete_events.write(BuildCompleteEvent {
            scene: result.scene,
            script: result.script,
            success: result.success,
            error: result.error,
            duration_ms: result.duration_ms,
        });
    }
}

/// Clean up compiled modules when SoulScript entities are removed
fn cleanup_removed_soulscripts(
    mut removed: RemovedComponents<SoulScriptData>,
    mut compiler: ResMut<HotCompiler>,
    _names_query: Query<&Name>,
) {
    for entity in removed.read() {
        // Try to get the name of the removed entity for the module name
        // Note: The entity is already despawned, so we can't query it directly
        // We use a naming convention: module name = "soul_<entity_id>"
        let module_name = format!("soul_{}", entity.index());
        
        // Check if this module exists and clean it up
        if compiler.has_module(&module_name) {
            compiler.cleanup_module(&module_name);
        }
        
        info!("🗑️ SoulScript entity {:?} removed, cleaned up module", entity);
    }
}


// ============================================================================
// API key check (Settings > Soul > Test Connection)
// ============================================================================

/// How one provider answered a key check.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyProbeOutcome {
    NotSet,
    Ok,
    /// The key was accepted but the account is rate limited right now.
    RateLimited,
    /// The provider refused the key (xAI answers a bad key with 400, the
    /// others with 401 or 403).
    Rejected(u16),
    ProviderError(u16),
    Unreachable(String),
}

/// One provider's result.
#[derive(Debug, Clone)]
pub struct KeyProbe {
    /// "Anthropic", "xAI" or "OpenAI".
    pub provider: &'static str,
    pub outcome: KeyProbeOutcome,
    pub millis: u128,
}

impl KeyProbe {
    /// A key the provider accepted (rate limiting included).
    pub fn accepted(&self) -> bool {
        matches!(self.outcome, KeyProbeOutcome::Ok | KeyProbeOutcome::RateLimited)
    }

    pub fn failed(&self) -> bool {
        matches!(
            self.outcome,
            KeyProbeOutcome::Rejected(_) | KeyProbeOutcome::ProviderError(_) | KeyProbeOutcome::Unreachable(_)
        )
    }

    pub fn summary(&self) -> String {
        match &self.outcome {
            KeyProbeOutcome::NotSet => format!("{}: not set", self.provider),
            KeyProbeOutcome::Ok => format!("{}: OK ({} ms)", self.provider, self.millis),
            KeyProbeOutcome::RateLimited => format!("{}: key accepted, rate limited right now", self.provider),
            KeyProbeOutcome::Rejected(code) => format!("{}: key rejected (HTTP {code})", self.provider),
            KeyProbeOutcome::ProviderError(code) => format!("{}: provider error (HTTP {code})", self.provider),
            KeyProbeOutcome::Unreachable(e) => format!("{}: unreachable ({e})", self.provider),
        }
    }
}

/// Check a key by listing the provider's models: a free, read-only request
/// that spends no tokens. Blocking; call it off the main thread. The key is
/// sent only to its own provider and never appears in the result.
pub fn probe_api_key(provider: &'static str, key: &str) -> KeyProbe {
    let key = key.trim();
    if key.is_empty() {
        return KeyProbe { provider, outcome: KeyProbeOutcome::NotSet, millis: 0 };
    }
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(10))
        .build();
    let request = match provider {
        "Anthropic" => agent
            .get("https://api.anthropic.com/v1/models?limit=1")
            .set("x-api-key", key)
            .set("anthropic-version", "2023-06-01"),
        "xAI" => agent.get("https://api.x.ai/v1/models").set("Authorization", &format!("Bearer {key}")),
        _ => agent.get("https://api.openai.com/v1/models").set("Authorization", &format!("Bearer {key}")),
    };
    let started = std::time::Instant::now();
    let outcome = match request.call() {
        Ok(_) => KeyProbeOutcome::Ok,
        Err(ureq::Error::Status(429, _)) => KeyProbeOutcome::RateLimited,
        Err(ureq::Error::Status(code @ (400 | 401 | 403), _)) => KeyProbeOutcome::Rejected(code),
        Err(ureq::Error::Status(code, _)) => KeyProbeOutcome::ProviderError(code),
        Err(e) => KeyProbeOutcome::Unreachable(e.to_string()),
    };
    KeyProbe { provider, outcome, millis: started.elapsed().as_millis() }
}
