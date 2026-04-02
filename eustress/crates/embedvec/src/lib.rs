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
mod knowledge;
mod ledger;
mod memory;
mod ontology;
#[cfg(feature = "persistence")]
mod persistence;
#[cfg(feature = "persistence-rocksdb")]
mod rocksdb_store;
mod plugin;
mod resource;
mod spatial;
mod systems;

pub use components::*;
pub use embedder::*;
pub use error::*;
pub use knowledge::{ConceptNode, KnowledgeGraph, RelationEdge, RelationType};
pub use ledger::{
    DiffResult, ProvenanceRecord, ProvenanceSource, RollbackPolicy, TraitDelta, TraitLedger,
    TraitRevision, TraitValue,
};
pub use memory::{Memory, MemoryQuery, MemoryStore, MemoryType};
pub use ontology::*;
#[cfg(feature = "persistence")]
pub use persistence::{
    IndexStats, KnowledgeStore, PersistenceConfig, PersistentIndex, PersistentOntologyIndex,
};
#[cfg(feature = "persistence-rocksdb")]
pub use rocksdb_store::{RocksConfig, RocksIndex, RocksOntologyIndex};
pub use plugin::{AutoIndexPlugin, EmbedvecAppExt, EmbedvecBuilder, EmbedvecPlugin};
#[cfg(feature = "persistence")]
pub use plugin::{KnowledgeStorePlugin, KnowledgeStoreResource, SpaceRootRef};
pub use resource::*;
pub use spatial::*;
pub use systems::*;

/// Prelude for convenient imports
pub mod prelude {
    pub use crate::components::{EmbeddedComponent, EmbeddingMetadata};
    pub use crate::embedder::{PropertyEmbedder, ReflectPropertyEmbedder, SimpleHashEmbedder};
    pub use crate::error::{EmbedvecError, Result};
    pub use crate::knowledge::{ConceptNode, KnowledgeGraph, RelationEdge, RelationType};
    pub use crate::ledger::{
        ProvenanceRecord, ProvenanceSource, TraitDelta, TraitLedger, TraitValue,
    };
    pub use crate::memory::{Memory, MemoryQuery, MemoryStore, MemoryType};
    pub use crate::ontology::{
        InstancePath, OntologyIndex, OntologyNode, OntologyTree, PropertySchema, PropertyType,
    };
    #[cfg(feature = "persistence")]
    pub use crate::persistence::{
        IndexStats, KnowledgeStore, PersistenceConfig, PersistentIndex, PersistentOntologyIndex,
    };
    #[cfg(feature = "persistence-rocksdb")]
    pub use crate::rocksdb_store::{RocksConfig, RocksIndex, RocksOntologyIndex};
    pub use crate::plugin::{AutoIndexPlugin, EmbedvecPlugin};
    pub use crate::resource::{EmbedvecIndex, EmbedvecResource, IndexConfig, SearchResult};
    pub use crate::spatial::{SpatialContextEmbedder, SpatialFeatures, SpatialTrainingRecord};
    pub use embedvec::quantization::Quantization;
}
