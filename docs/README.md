# Eustress Engine Documentation

Welcome to the Eustress Engine documentation. This is a comprehensive game engine built on Bevy with Roblox-inspired class systems and AI-powered asset generation.

## 📚 Documentation Structure

```
docs/
├── getting-started/     # Quick start guides and setup
├── architecture/        # Engine design and systems
├── classes/            # Roblox-compatible class system
├── networking/         # Multiplayer, Forge, friends, parties
├── assets/             # Asset pipeline and hosting
├── development/        # Development guides and tools
└── archive/            # Historical documentation
    └── phase-history/  # Development phase records
```

## 🚀 Getting Started

| Document | Description |
|----------|-------------|
| [BUILD_FIX](getting-started/BUILD_FIX.md) | Common build issues and fixes (Windows file-lock error 32, etc.) |
| [SETTINGS_EXAMPLE](getting-started/SETTINGS_EXAMPLE.md) | Editor settings persistence example |

## 🏗️ Architecture

| Document | Description |
|----------|-------------|
| [THE_LAST_GAME_ENGINE](architecture/THE_LAST_GAME_ENGINE.md) | Complete implementation guide |
| [ENHANCEMENT_PIPELINE](architecture/ENHANCEMENT_PIPELINE.md) | AI enhancement pipeline setup |
| [LIGHTING_ENHANCEMENT_PROPOSAL](architecture/LIGHTING_ENHANCEMENT_PROPOSAL.md) | Lighting system design |
| [IMPROVEMENTS](architecture/IMPROVEMENTS.md) | Planned improvements |

## 🎮 Classes

The live class registry is `eustress/crates/common/assets/class_schema/*.defaults.toml`
— `common/build.rs` globs that directory at compile time and registers
each template as a class. See:

| Document | Description |
|----------|-------------|
| [classes/README](classes/README.md) | Directory index — entry point for class docs |
| [CLASS_EXTENSIBILITY](classes/CLASS_EXTENSIBILITY.md) | Canonical guide to adding a new class (template + `ExtraSectionClaim`) |
| [development/CLASS_CONVERSION](development/CLASS_CONVERSION.md) | Studio class-conversion-tool semantics (orthogonal: changing an existing instance's class, not adding one) |

## 🌐 Networking & Multiplayer

| Document | Description |
|----------|-------------|
| [Networking Overview](networking/README.md) | Complete networking guide |
| [Eustress Forge](networking/FORGE.md) | Orchestration platform deep dive |
| [Friends System](networking/FRIENDS.md) | Friend requests, presence, blocking |
| [Parties](networking/PARTIES.md) | Party creation, invites, teleportation |

## 📦 Assets

| Document | Description |
|----------|-------------|
| [ASSET_DEVELOPER_GUIDE](assets/ASSET_DEVELOPER_GUIDE.md) | Asset system developer guide |
| [asset_hosting](assets/asset_hosting.md) | Asset hosting and distribution |

## 🛠️ Development

`docs/development/` contains live system docs (architecture for
specific subsystems: file-watcher hot-reload, lighting, terrain,
selection, scripting, toolset, etc.). Browse the directory directly —
docs are added as systems land, no doc-maintained index.

## 📜 Archive

Historical documentation from development phases:

- [Phase 1 Complete](archive/phase-history/PHASE1_COMPLETE.md)
- [Phase 2 Kickoff](archive/phase-history/PHASE2_KICKOFF.md)
- [Phase 2 Progress](archive/phase-history/PHASE2_PROGRESS.md)
- [Phase 2 Week 1](archive/phase-history/PHASE2_WEEK1_COMPLETE.md)
- [Phase 2 Week 2](archive/phase-history/PHASE2_WEEK2_COMPLETE.md)
- [Phase 2 Week 3](archive/phase-history/PHASE2_WEEK3_COMPLETE.md)
- [Phase 3 Analysis](archive/phase-history/PHASE3_ANALYSIS.md)
- [Phase 3 PartManager Removal](archive/phase-history/PHASE3_PARTMANAGER_REMOVAL.md)
- [Phase 3 Progress](archive/phase-history/PHASE3_PROGRESS.md)

## 🔗 Quick Links

- **Main Repository**: [EustressEngine](../)
- **Engine Crate**: [eustress/engine](../eustress/engine/)
- **Common Crate**: [eustress/crates/common](../eustress/crates/common/)

## 📝 Contributing

When adding new documentation:

1. Place getting started guides in `getting-started/`
2. Place architecture docs in `architecture/`
3. Place class-related docs in `classes/`
4. Place asset-related docs in `assets/`
5. Place dev tools/guides in `development/`
6. Archive old phase docs in `archive/phase-history/`
