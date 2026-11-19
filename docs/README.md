# Eustress Documentation

Eustress is an AI-native simulation substrate. Studio is the editor, the Player is the runtime, and a world is a Space. The source is available under PolyForm Shield 1.0.0 (see [`LICENSE`](../LICENSE)).

This folder holds the design docs, subsystem references, launch checklists and policy drafts.

## Toolchain

| Part | Version | Read it from |
|---|---|---|
| Rust | 1.100.0 | `eustress/rust-toolchain.toml` |
| Bevy | 0.19 | `eustress/Cargo.toml` |
| Physics | Avian 0.7 | `eustress/Cargo.toml` |
| Scripting | mlua 0.12.2 with Luau (luau0-src 0.22.0, Luau 740) | `eustress/Cargo.toml`, `eustress/Cargo.lock` |
| UI | Slint (git master, locked at 1.17.1) | `eustress/Cargo.toml`, `eustress/Cargo.lock` |
| Studio | 0.3.8 | `eustress/crates/engine/Cargo.toml` |
| Player | 0.2.0 | `eustress/crates/client/Cargo.toml` |

The native unit is the metre: one engine unit is one metre. A stud (0.28 m) is a display unit. See [UNITS](UNITS.md).

## Where to start

| Goal | Read |
|---|---|
| Fix a Windows build that fails with os error 32 | [getting-started/BUILD_FIX](getting-started/BUILD_FIX.md) |
| Download Studio or the Player | [downloads/EUSTRESS_ENGINE](downloads/EUSTRESS_ENGINE.md), [downloads/EUSTRESS_PLAYER](downloads/EUSTRESS_PLAYER.md) |
| Understand how a Space is stored | [development/SPACE_ARCHITECTURE](development/SPACE_ARCHITECTURE.md), [development/FILE_SYSTEM_FIRST](development/FILE_SYSTEM_FIRST.md) |
| Add a class | [classes/CLASS_EXTENSIBILITY](classes/CLASS_EXTENSIBILITY.md) |
| Host a multiplayer session | [networking/LOCAL_SERVER](networking/LOCAL_SERVER.md), [networking/README](networking/README.md) |
| Script a Space | [services/README](services/README.md), [development/SCRIPTING_API_CHECKLIST](development/SCRIPTING_API_CHECKLIST.md) |
| Run a Space without a window | [architecture/HEADLESS_RUNTIME](architecture/HEADLESS_RUNTIME.md) |

## Classes

Each class is a folder under `eustress/crates/common/assets/class_schema/` that holds an `_instance.toml`. `eustress/crates/common/build.rs` scans that directory at compile time and registers every such folder as a built-in template, so a new class is a new folder. A folder without `_instance.toml` is skipped with a build warning.

| Document | What it covers |
|---|---|
| [classes/README](classes/README.md) | Index of the class docs |
| [classes/CLASS_EXTENSIBILITY](classes/CLASS_EXTENSIBILITY.md) | Adding a new class |
| [development/CLASS_CONVERSION](development/CLASS_CONVERSION.md) | Changing an existing instance from one class to another in Studio |

## Folders

| Folder | Holds |
|---|---|
| [`getting-started/`](getting-started/) | Build troubleshooting and settings notes. |
| [`downloads/`](downloads/) | What each download is and how to get it. |
| [`architecture/`](architecture/) | System designs and plans. [SCALING_ARCHITECTURE](architecture/SCALING_ARCHITECTURE.md) is the plan for scale; [SHARED_PLAY_RUNTIME](architecture/SHARED_PLAY_RUNTIME.md) covers the Play session shared by Studio and the Player; [BEVY_019_MIGRATION](architecture/BEVY_019_MIGRATION.md) plans the Bevy 0.18 to 0.19 migration. |
| [`development/`](development/) | Subsystem references: terrain, selection, lighting, simulation, the file watcher, Slint UI, toolsets. Browse the folder; no index is maintained. |
| [`services/`](services/) | One page per default service in a Space, with an [index](services/README.md). |
| [`classes/`](classes/) | The class system docs listed above. |
| [`networking/`](networking/) | Hosted sessions, server authority, listings, joining. Start at [networking/README](networking/README.md). |
| [`assets/`](assets/) | [ASSET_DEVELOPER_GUIDE](assets/ASSET_DEVELOPER_GUIDE.md) and [asset_hosting](assets/asset_hosting.md). |
| [`api/`](api/) | API references, including the scripting API of the Website service. |
| [`design/`](design/) | Design notes for the road system, animation, the script editor, terrain tools and the sim page. |
| [`launch/`](launch/) | Alpha checklists, staging tests and live-play goals. |
| [`moderation/`](moderation/) | The moderation [playbook](moderation/PLAYBOOK.md), its API and the AI agents that review listings. |
| [`monetization/`](monetization/) | Commerce, currency, subscriptions, cash-out and the financial design. |
| [`legal/`](legal/) | Compliance notes (CCPA, COPPA, CSAM, DMCA, GDPR, TIDA). |
| [`policy/`](policy/) | Draft community standards. |
| [`validation/`](validation/) | The [proof standard](validation/PROOF_STANDARD.md) and validation reports. |
| [`AUDIT/`](AUDIT/) | Dated point-in-time audits. Start at [MASTER](AUDIT/MASTER.md). |
| [`archive/`](archive/) | Retired docs, including the phase records in [`archive/phase-history/`](archive/phase-history/). |
| [`marketing/`](marketing/) | Public-facing pages and images; owned by the founder. |
| [`PROMPTS/`](PROMPTS/) | The prompt library and harness that agent sessions run from. |
| `convergence/`, `circumstances/`, `scenarios/`, `orbital-navigation/`, `innovation/`, `workshop/`, `finance/`, `benchmarks/`, `documents/`, `process/` | Smaller topic folders; browse each directly. |

The files in this folder's root are standalone designs and references: [UNITS](UNITS.md), [EEP_SPECIFICATION](EEP_SPECIFICATION.md), [FEATURE_PARITY](FEATURE_PARITY.md), [LAW-ORDER](LAW-ORDER.md), [generative-world-layer](generative-world-layer.md), [world-shards-design](world-shards-design.md), [host-migration-design](host-migration-design.md), [host-bliss-payouts-design](host-bliss-payouts-design.md), [idle-host-preferences-design](idle-host-preferences-design.md).

## Code

- Studio: [`eustress/crates/engine`](../eustress/crates/engine/)
- Player: [`eustress/crates/client`](../eustress/crates/client/)
- Shared runtime and classes: [`eustress/crates/common`](../eustress/crates/common/)

## Adding a doc

1. Put it in the folder that matches its subject (the table above).
2. Write what is true today. Record a decision or a history in `archive/` or in a dated audit.
3. Name every path you cite in full from the repository root, and check that it exists.
4. Use "source-available", metres as the unit, and Studio, Player and Space as the terms.
