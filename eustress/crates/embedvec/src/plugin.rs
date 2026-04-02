//! Bevy plugins for embedvec integration
//!
//! ## Table of Contents
//! 1. EmbedvecPlugin - Core plugin with resource and systems
//! 2. AutoIndexPlugin - Plugin for automatic Reflect-based indexing
//! 3. PersistencePlugin - Plugin for save/load integration

use crate::components::EmbeddedComponent;
use crate::embedder::{PropertyEmbedder, ReflectPropertyEmbedder, SimpleHashEmbedder};
use crate::resource::{EmbedvecResource, IndexConfig};
use crate::systems::{
    auto_embed_reflected, index_dirty_embeddings, remove_despawned_entities, AutoEmbed,
    EmbedvecSet,
};
use bevy::prelude::*;

#[cfg(feature = "persistence")]
use crate::knowledge::KnowledgeGraph;
#[cfg(feature = "persistence")]
use crate::memory::MemoryStore;
#[cfg(feature = "persistence")]
use crate::persistence::{KnowledgeStore, PersistenceConfig};
#[cfg(feature = "persistence")]
use parking_lot::RwLock;
#[cfg(feature = "persistence")]
use std::sync::Arc;

/// Core embedvec plugin that sets up the resource and basic systems
pub struct EmbedvecPlugin {
    /// Index configuration
    pub config: IndexConfig,
    /// Whether to use the default hash embedder
    pub use_default_embedder: bool,
}

impl Default for EmbedvecPlugin {
    fn default() -> Self {
        Self {
            config: IndexConfig::default(),
            use_default_embedder: true,
        }
    }
}

impl EmbedvecPlugin {
    /// Create with custom configuration
    pub fn with_config(config: IndexConfig) -> Self {
        Self {
            config,
            use_default_embedder: true,
        }
    }

    /// Create with custom dimension
    pub fn with_dimension(dimension: usize) -> Self {
        Self {
            config: IndexConfig::default().with_dimension(dimension),
            use_default_embedder: true,
        }
    }

    /// Disable default embedder (user will insert custom EmbedvecResource)
    pub fn without_default_embedder(mut self) -> Self {
        self.use_default_embedder = false;
        self
    }
}

impl Plugin for EmbedvecPlugin {
    fn build(&self, app: &mut App) {
        // Register types for reflection
        app.register_type::<EmbeddedComponent>()
            .register_type::<AutoEmbed>();

        // Configure system sets
        app.configure_sets(
            PostUpdate,
            (
                EmbedvecSet::AutoEmbed,
                EmbedvecSet::Index,
                EmbedvecSet::Cleanup,
            )
                .chain(),
        );

        // Add core systems
        app.add_systems(
            PostUpdate,
            (
                index_dirty_embeddings.in_set(EmbedvecSet::Index),
                remove_despawned_entities.in_set(EmbedvecSet::Cleanup),
            ),
        );

        // Insert default resource if requested
        if self.use_default_embedder {
            let embedder = SimpleHashEmbedder::new(self.config.dimension);
            let resource = EmbedvecResource::new(self.config.clone(), embedder);
            app.insert_resource(resource);
        }

        tracing::info!(
            "EmbedvecPlugin initialized with dimension={}",
            self.config.dimension
        );
    }
}

/// Plugin for automatic indexing of entities with Reflect components
pub struct AutoIndexPlugin;

impl Plugin for AutoIndexPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            auto_embed_reflected.in_set(EmbedvecSet::AutoEmbed),
        );

        tracing::info!("AutoIndexPlugin initialized");
    }
}

/// Builder for creating EmbedvecResource with custom embedder
pub struct EmbedvecBuilder {
    config: IndexConfig,
}

impl EmbedvecBuilder {
    /// Create a new builder with default config
    pub fn new() -> Self {
        Self {
            config: IndexConfig::default(),
        }
    }

    /// Set the embedding dimension
    pub fn dimension(mut self, dimension: usize) -> Self {
        self.config.dimension = dimension;
        self
    }

    /// Set HNSW M parameter
    pub fn hnsw_m(mut self, m: usize) -> Self {
        self.config.m = m;
        self
    }

    /// Set HNSW ef_construction parameter
    pub fn hnsw_ef_construction(mut self, ef: usize) -> Self {
        self.config.ef_construction = ef;
        self
    }

    /// Enable persistence
    pub fn persistence(mut self, path: impl Into<String>) -> Self {
        self.config.persistence_path = Some(path.into());
        self
    }

    /// Build with the default hash embedder
    pub fn build_with_hash_embedder(self) -> EmbedvecResource {
        let embedder = SimpleHashEmbedder::new(self.config.dimension);
        EmbedvecResource::new(self.config, embedder)
    }

    /// Build with a reflect-aware embedder
    pub fn build_with_reflect_embedder(self) -> EmbedvecResource {
        let embedder = ReflectPropertyEmbedder::with_hash_embedder(self.config.dimension);
        EmbedvecResource::new(self.config, embedder)
    }

    /// Build with a custom embedder
    pub fn build_with_embedder<E: PropertyEmbedder>(self, embedder: E) -> EmbedvecResource {
        EmbedvecResource::new(self.config, embedder)
    }
}

impl Default for EmbedvecBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Extension trait for App to easily add embedvec functionality
pub trait EmbedvecAppExt {
    /// Add embedvec with default configuration
    fn add_embedvec(&mut self) -> &mut Self;

    /// Add embedvec with custom dimension
    fn add_embedvec_with_dimension(&mut self, dimension: usize) -> &mut Self;

    /// Add embedvec with custom resource
    fn add_embedvec_resource(&mut self, resource: EmbedvecResource) -> &mut Self;

    /// Add auto-indexing for Reflect components
    fn add_embedvec_auto_index(&mut self) -> &mut Self;
}

impl EmbedvecAppExt for App {
    fn add_embedvec(&mut self) -> &mut Self {
        self.add_plugins(EmbedvecPlugin::default())
    }

    fn add_embedvec_with_dimension(&mut self, dimension: usize) -> &mut Self {
        self.add_plugins(EmbedvecPlugin::with_dimension(dimension))
    }

    fn add_embedvec_resource(&mut self, resource: EmbedvecResource) -> &mut Self {
        self.add_plugins(EmbedvecPlugin::default().without_default_embedder())
            .insert_resource(resource)
    }

    fn add_embedvec_auto_index(&mut self) -> &mut Self {
        self.add_plugins(AutoIndexPlugin)
    }
}

// ============================================================================
// KnowledgeStoreResource + KnowledgeStorePlugin
// ============================================================================

/// Bevy Resource holding the per-space KnowledgeStore.
///
/// Loaded from `<universe>/knowledge/<space_name>/knowledge.db` when a Space
/// opens. Provides mutable access to `KnowledgeGraph`, `MemoryStore`, and
/// `TraitLedger` instances keyed by name.
#[cfg(feature = "persistence")]
#[derive(Resource)]
pub struct KnowledgeStoreResource {
    /// The active Sled-backed store
    pub store: Arc<RwLock<KnowledgeStore>>,
    /// The in-memory KnowledgeGraph (loaded from / flushed to store)
    pub graph: Arc<RwLock<KnowledgeGraph>>,
    /// The in-memory MemoryStore (loaded from / flushed to store)
    pub memories: Arc<RwLock<MemoryStore>>,
    /// Absolute path of the open knowledge.db (for display/logging)
    pub db_path: std::path::PathBuf,
}

#[cfg(feature = "persistence")]
impl KnowledgeStoreResource {
    /// Open (or create) a knowledge store for the given space root.
    pub fn open_for_space(space_root: &std::path::Path) -> Result<Self, crate::error::EmbedvecError> {
        let config = crate::persistence::PersistenceConfig::for_space(space_root);
        let db_path = std::path::PathBuf::from(&config.path);
        let store = KnowledgeStore::open(&config)?;

        // Load persisted data (or start fresh if first open)
        let graph = store.load_knowledge_graph()?.unwrap_or_default();
        let memories = store.load_memory_store()?;

        tracing::info!(
            path = %db_path.display(),
            memories = memories.len(),
            concepts = graph.node_count(),
            "KnowledgeStore opened"
        );

        Ok(Self {
            store: Arc::new(RwLock::new(store)),
            graph: Arc::new(RwLock::new(graph)),
            memories: Arc::new(RwLock::new(memories)),
            db_path,
        })
    }

    /// Flush in-memory graph and memories back to Sled.
    pub fn flush(&self) -> Result<(), crate::error::EmbedvecError> {
        let store = self.store.read();
        store.save_knowledge_graph(&*self.graph.read())?;
        store.save_memory_store(&*self.memories.read())?;
        store.flush()?;
        tracing::debug!(path = %self.db_path.display(), "KnowledgeStore flushed");
        Ok(())
    }
}

/// Bevy Resource tracking which `SpaceRoot` path the knowledge store was loaded for.
/// Used to detect space switches and reload the store.
#[cfg(feature = "persistence")]
#[derive(Resource, Default)]
struct LoadedKnowledgeSpacePath(Option<std::path::PathBuf>);

/// Plugin that loads and saves `KnowledgeStoreResource` per space.
///
/// ## Lifecycle
/// - **Space open** (`SpaceRoot` changed): closes old store, opens `knowledge.db`
///   from `<universe>/knowledge/<space_name>/knowledge.db`, inserts
///   `KnowledgeStoreResource` into the world.
/// - **App exit**: flushes to disk.
///
/// Add to the engine app alongside `EmbedvecPlugin`:
/// ```rust,ignore
/// app.add_plugins(KnowledgeStorePlugin);
/// ```
#[cfg(feature = "persistence")]
pub struct KnowledgeStorePlugin;

#[cfg(feature = "persistence")]
impl Plugin for KnowledgeStorePlugin {
    fn build(&self, app: &mut App) {
        // Sled flushes automatically via flush_every_ms and on Drop.
        // reload_knowledge_store_on_space_change handles per-space load.
        app.init_resource::<LoadedKnowledgeSpacePath>()
            .add_systems(PostUpdate, reload_knowledge_store_on_space_change);
    }
}

/// System: detect `SpaceRootRef` change and (re)load the knowledge store.
#[cfg(feature = "persistence")]
fn reload_knowledge_store_on_space_change(
    space_root: Option<Res<SpaceRootRef>>,
    mut loaded_path: ResMut<LoadedKnowledgeSpacePath>,
    mut commands: Commands,
) {
    let Some(space_root) = space_root else { return };
    if !space_root.is_changed() {
        return;
    }

    let path = space_root.0.clone();
    if loaded_path.0.as_deref() == Some(path.as_path()) {
        return;
    }

    match KnowledgeStoreResource::open_for_space(&path) {
        Ok(resource) => {
            loaded_path.0 = Some(path.clone());
            commands.insert_resource(resource);
            tracing::info!(space = %path.display(), "KnowledgeStore reloaded for space");
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to open KnowledgeStore for space");
        }
    }
}

/// Thin newtype so `eustress-embedvec` can accept the engine's `SpaceRoot`
/// without directly depending on `eustress-engine`.
///
/// The engine registers this by inserting:
/// ```rust,ignore
/// app.insert_resource(SpaceRootRef(space_root.0.clone()));
/// ```
/// whenever `SpaceRoot` changes (via an observer or system in the engine).
#[cfg(feature = "persistence")]
#[derive(Resource, Clone)]
pub struct SpaceRootRef(pub std::path::PathBuf);

#[cfg(feature = "persistence")]
impl SpaceRootRef {
    /// Construct from any path
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self(path.into())
    }
}
