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
