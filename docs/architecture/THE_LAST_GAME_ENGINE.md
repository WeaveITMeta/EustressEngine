# ⚠️ DEPRECATED — see SCALING_ARCHITECTURE.md

> This document described an early vision (a Python client/server that turned RON-scene
> primitives into photoreal assets over HTTP via FLUX + TripoSR). **It no longer reflects
> the engine.** The real engine is Bevy 0.18 deferred PBR, a Fjall WorldDb (rkyv cores,
> Morton encoder), `_instance.toml` + file-watcher instancing, and a distance-band LOD
> cascade — and the genuinely hard goal (10M entities · 60 FPS · photorealistic) is a
> systems/streaming problem this doc never addressed.
>
> **The current, grounded plan lives in
> [SCALING_ARCHITECTURE.md](SCALING_ARCHITECTURE.md).**
>
> The historical content below is retained for context only. Do not implement from it.

---

# 🔥 THE LAST GAME ENGINE - Complete Implementation Guide (HISTORICAL)

**Status**: ✅ **Phases 1-4 COMPLETE** - Ready to paste and build

## What You Have Now

A complete AI-powered game engine with:
- ✅ Enhanced scene format with quest graphs and atmosphere
- ✅ Distance-based enhancement chunking (only enhances nearby nodes)
- ✅ Background-threaded asset generation (zero frame drops)
- ✅ SHA256 cache (never regenerates twice)
- ✅ LLM-powered quest graph execution
- ✅ Production generation server (stub + real modes)
- ✅ Example scenes ready to load

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                  EUSTRESS ENGINE                        │
│  (The Thinker's Tool - Nodes + Prompts + Connections)  │
└─────────────────────────────────────────────────────────┘
                          │
                          │ .ron file
                          ▼
┌─────────────────────────────────────────────────────────┐
│                  EUSTRESS CLIENT                        │
│        (The Magic - Turns Prompts into AAA)             │
└─────────────────────────────────────────────────────────┘
```

> The remaining sections (generation server, RON scene format, FLUX/TripoSR/DeepSeek
> endpoints, quest-graph executor) are obsolete. See SCALING_ARCHITECTURE.md §6 for how
> AI asset generation is now folded in as an offline bake-time path, and §0–§5 for the
> data-layer and rendering architecture that replaces everything above.
