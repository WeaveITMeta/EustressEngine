# eustress-embedvec

Vector database integration for Eustress Engine with HNSW indexing, property embeddings, and semantic search capabilities.

## Features

- **EmbedvecResource**: Thread-safe Bevy Resource wrapping the vector index
- **PropertyEmbedder**: Trait for custom embedding strategies (hash-based, ML-based, etc.)
- **AutoIndexPlugin**: Automatic indexing of entities with Reflect components
- **EmbeddedComponent**: Component for storing entity embeddings
- **Semantic Search**: Natural language queries over entity properties
- **Metadata Filtering**: Combine vector similarity with property filters

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
eustress-embedvec = { path = "../embedvec" }
```

## Quick Start

```rust
use bevy::prelude::*;
use eustress_embedvec::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_embedvec()              // Add core plugin
        .add_embedvec_auto_index()   // Enable auto-indexing
        .add_systems(Startup, setup)
        .add_systems(Update, search_entities)
        .run();
}

fn setup(mut commands: Commands) {
    // Spawn entity with auto-embed marker
    commands.spawn((
        Name::new("Warrior"),
        AutoEmbed::with_auto_update(),
    ));
    
    // Or manually provide embedding
    commands.spawn((
        Name::new("Mage"),
        EmbeddedComponent::new(vec![0.1; 128]),
    ));
}

fn search_entities(
    embedvec: Res<EmbedvecResource>,
    query: Query<&Name>,
) {
    // Semantic search
    let results = embedvec.query("high damage warrior", 5).unwrap();
    
    for result in results {
        if let Ok(name) = query.get(result.entity) {
            println!("Found: {} (score: {:.3})", name, result.score);
        }
    }
}
```

## Custom Embedder

Implement `PropertyEmbedder` for custom embedding strategies:

```rust
use eustress_embedvec::prelude::*;
use std::collections::HashMap;
use serde_json::Value;

struct MyEmbedder {
    dimension: usize,
}

impl PropertyEmbedder for MyEmbedder {
    fn dimension(&self) -> usize {
        self.dimension
    }

    fn embed_properties(&self, properties: &HashMap<String, Value>) -> Result<Vec<f32>> {
        // Your embedding logic here (e.g., call an ML model)
        Ok(vec![0.0; self.dimension])
    }

    fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        // Embed search queries
        Ok(vec![0.0; self.dimension])
    }
}

// Use custom embedder
fn main() {
    let resource = EmbedvecBuilder::new()
        .dimension(256)
        .build_with_embedder(MyEmbedder { dimension: 256 });
    
    App::new()
        .add_embedvec_resource(resource)
        .run();
}
```

## Filtered Search

Combine semantic search with metadata filters:

```rust
fn search_npcs(embedvec: Res<EmbedvecResource>) {
    let results = embedvec.query_filtered(
        "aggressive enemy",
        10,
        |meta| meta.has_tag("npc") && meta.get_property::<i32>("level").unwrap_or(0) > 5
    ).unwrap();
}
```

## Find Similar Entities

```rust
fn find_similar(
    embedvec: Res<EmbedvecResource>,
    selected: Query<Entity, With<Selected>>,
) {
    for entity in selected.iter() {
        let similar = embedvec.read().find_similar(entity, 5).unwrap();
        for result in similar {
            println!("Similar entity: {:?} (score: {:.3})", result.entity, result.score);
        }
    }
}
```

## Serialization Integration

The `EmbeddedComponent` is serializable and works with save/load systems:

```rust
#[derive(Component, Serialize, Deserialize, Reflect)]
struct SavedEntity {
    // Your data
}

// EmbeddedComponent is automatically serialized with the entity
// On load, mark as dirty to re-index
fn on_load(mut query: Query<&mut EmbeddedComponent>) {
    for mut embedded in query.iter_mut() {
        embedded.mark_dirty();
    }
}
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     EmbedvecResource                        │
│  ┌─────────────────┐  ┌─────────────────────────────────┐  │
│  │ PropertyEmbedder│  │        EmbedvecIndex            │  │
│  │  (SimpleHash/   │  │  ┌─────────────────────────┐   │  │
│  │   Reflect/ML)   │  │  │ Entity → (Embedding,    │   │  │
│  └─────────────────┘  │  │          Metadata)      │   │  │
│                       │  └─────────────────────────┘   │  │
│                       └─────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                              ↑
                              │ Systems
        ┌─────────────────────┼─────────────────────┐
        │                     │                     │
┌───────┴───────┐  ┌──────────┴──────────┐  ┌──────┴───────┐
│ auto_embed_   │  │ index_dirty_        │  │ remove_      │
│ reflected     │  │ embeddings          │  │ despawned    │
└───────────────┘  └─────────────────────┘  └──────────────┘
```

## License

MIT
