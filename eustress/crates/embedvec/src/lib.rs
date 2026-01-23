//! # eustress-embedvec
//!
//! Vector database integration for Eustress Engine with HNSW indexing,
//! property embeddings, and semantic search capabilities.
//!
//! ## Features
//! - `EmbedvecResource`: Bevy Resource wrapping embedvec index
//! - `PropertyEmbedder`: Trait for custom embedding strategies
//! - `AutoIndexPlugin`: Automatic indexing of Reflect components
//! - `EmbeddedComponent`: Component for entity embeddings
//! - Serialization hooks for save/load integration
//!
//! ## Table of Contents
//! 1. Error types (`error`)
//! 2. Resource wrapper (`resource`)
//! 3. Embedding traits (`embedder`)
//! 4. Components (`components`)
//! 5. Systems (`systems`)
//! 6. Plugin (`plugin`)

mod components;
mod embedder;
mod error;
mod ontology;
#[cfg(feature = "persistence")]
mod persistence;
mod plugin;
mod resource;
mod spatial;
mod systems;

pub use components::*;
pub use embedder::*;
pub use error::*;
pub use ontology::*;
#[cfg(feature = "persistence")]
pub use persistence::*;
pub use plugin::*;
pub use resource::*;
pub use spatial::*;
pub use systems::*;

/// Prelude for convenient imports
pub mod prelude {
    pub use crate::components::{EmbeddedComponent, EmbeddingMetadata};
    pub use crate::embedder::{PropertyEmbedder, ReflectPropertyEmbedder, SimpleHashEmbedder};
    pub use crate::error::{EmbedvecError, Result};
    pub use crate::ontology::{OntologyTree, OntologyNode, OntologyIndex, PropertySchema, PropertyType, InstancePath};
    #[cfg(feature = "persistence")]
    pub use crate::persistence::{PersistentIndex, PersistentOntologyIndex, PersistenceConfig, IndexStats};
    pub use crate::plugin::{AutoIndexPlugin, EmbedvecPlugin};
    pub use crate::resource::{EmbedvecIndex, EmbedvecResource, SearchResult};
    pub use crate::spatial::{SpatialContextEmbedder, SpatialFeatures, SpatialTrainingRecord};
}
