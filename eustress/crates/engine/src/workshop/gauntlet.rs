//! # Gauntlet Mode — the `docs/PROMPTS/` loop, inside Workshop
//!
//! Brings the program defined by [`docs/PROMPTS/00_MASTER_PROTOCOL.md`] into the
//! Workshop panel, so an ordinary chat turn is held to the same bar as a
//! queued gauntlet item: measure it, hand the evidence to an independent
//! Critic, and let the Critic — not the builder — decide whether it passed.
//!
//! ## Why the executor cannot grade itself
//!
//! The protocol's §5.1 marks step 3 (L1 re-runs the measurement itself) as
//! load-bearing, and says an agent reporting its own pass is "the single most
//! common way this loop degrades into theatre." §2.2 puts it flatly: an L2
//! **may not score its own work**. So Gauntlet's scoring step is not
//! self-critique — it is a real second model call, `gauntlet_critic`, with a
//! fresh context, the rubric, and the evidence, and structurally without the
//! builder's prose.
//!
//! ## How this maps onto a single-agent panel
//!
//! `docs/PROMPTS/` assumes a four-level chain (L0→L1→L2→L3) plus a Critic
//! outside it. Workshop is one agent, so the separation is approximated:
//!
//! | Protocol role | Workshop stand-in |
//! |---|---|
//! | L2 builder | the Workshop agent itself |
//! | L1 measurer | the agent, but forced to quote the *literal* tool call and its *literal* output as the measurement — a claim without a transcript is not a measurement |
//! | Critic | [`GAUNTLET_CRITIC_TOOL`] — a separate model call, clean context, rubric injected Rust-side, self-report structurally excluded |
//! | L0 orchestrator | the human at the keyboard, who receives the STALL packet |
//!
//! The one guarantee that survives fully intact is the important one: the
//! thing that decides pass/fail never sees the builder's account of its own
//! work, and cannot be argued with.
//!
//! ## Access control (§ rubric ACCESS CONTROL)
//!
//! `01_CRITIC_RUBRIC.md` §4 is held-out — readable by the Critic and L1 only,
//! and an L2 that reads it contaminates the item. That boundary is enforced
//! structurally here rather than by instruction: the rubric is [`include_str!`]'d
//! into the binary and injected **only** into the Critic call's system prompt.
//! It never enters the Workshop executor's context, so the executor cannot
//! read §4 even if it tries.

use bevy::prelude::*;

/// The Critic's entire contract, baked in at compile time.
///
/// Compile-time embedding (rather than a runtime read) does three things:
/// the Critic can never be handed a rubric that drifted from the repo, there
/// is no runtime path to resolve from a user's Universe directory back to the
/// engine checkout, and — most importantly — the held-out §4 lives only on the
/// path that reaches the Critic. If the file moves, this fails loudly at
/// compile time, which is the correct outcome.
pub const CRITIC_RUBRIC: &str =
    include_str!("../../../../../docs/PROMPTS/01_CRITIC_RUBRIC.md");

/// Iterations permitted at one approach before a change of approach is
/// mandatory (`00_MASTER_PROTOCOL.md` §5.2).
pub const MAX_ITERATIONS_PER_APPROACH: u32 = 3;

/// Distinct approaches permitted before the item STALLs (§5.2).
pub const MAX_APPROACHES: u32 = 3;

/// Hard ceiling — 3 approaches × 3 iterations (§5.2).
pub const MAX_ITERATIONS_TOTAL: u32 = MAX_ITERATIONS_PER_APPROACH * MAX_APPROACHES;

/// Pass floor on every rubric dimension (`01_CRITIC_RUBRIC.md` §2). Not the
/// mean, not the median — every one.
pub const PASS_FLOOR: f32 = 8.0;

/// Live toggle state, mirrored from `GlobalSoulSettings::workshop_gauntlet`.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct GauntletMode {
    pub enabled: bool,
}

/// Fired when the user flips the Gauntlet pill in the Workshop toolbar.
#[derive(Message, Debug, Clone)]
pub struct WorkshopSetGauntletEvent {
    pub enabled: bool,
}

/// Badge shown in chat / logs when the mode is on.
pub const GAUNTLET_BADGE: &str = "⚔ Gauntlet";

/// System prompt for the independent Critic call.
///
/// Prepended to [`CRITIC_RUBRIC`], which supplies the persona, the seven
/// dimensions, the anchors, the citation requirement, and the scorecard
/// schema. Kept deliberately short: the rubric is the contract, this only
/// states the call's boundaries.
pub const CRITIC_SYSTEM_PREAMBLE: &str = r#"You are the Critic defined by the rubric that follows. It is your entire contract — persona, dimensions, anchors, citation requirements, and output schema.

Two boundaries specific to this invocation:

1. You are scoring an artifact built inside the Eustress Workshop panel. Your input is evidence: measured values, literal tool transcripts, file paths, entity data. If the input contains the builder's own account of its work — a summary, a changelog, a rationale, a claim of success, an argument for why it should pass — that is contamination. Return `INPUT_CONTAMINATED` and do not score.

2. Return exactly one fenced JSON block conforming to the scorecard schema in §6. No preamble, no summary, no encouragement. Where a dimension does not apply to this artifact type (e.g. motion dimensions for a text document), set its score to `null` and `"pass": true`, and say why in `deficiency` — do not invent a score for something you cannot observe, and do not fail an artifact for lacking a face it was never meant to have.

Your two powers, in order, are unchanged: you cannot pass what misses the floor, and you may fail what clears it — but a wow refusal must name a specific, addressable deficiency and what would fix it.

--- RUBRIC BEGINS ---
"#;

/// The doctrine injected into the Workshop system prompt while Gauntlet is on.
///
/// Deliberately procedural. The failure this mode exists to stop is an agent
/// that believes it did well; exhortation does not touch that, a loop with a
/// hard stop and an external judge does.
pub const GAUNTLET_DOCTRINE: &str = r#"
## ⚔ GAUNTLET MODE — ACTIVE (docs/PROMPTS/ protocol)

You are running under `docs/PROMPTS/00_MASTER_PROTOCOL.md`. The user has
switched on an expensive mode; they are paying tokens for rigor. Spend them on
measurement and iteration, not on longer prose.

### The loop (§5.1) — run it, don't narrate it

1. **SPEC.** Before building, state the item's **exit criterion** as one
   falsifiable measurement: the literal command or tool call that will be run,
   and the threshold that decides pass. "Looks better" is not an exit
   criterion. If you cannot state one, say `EXIT_CRITERION_UNMEASURABLE` and
   stop — do not proceed on vibes.
2. **BUILD.** Make the artifact. Scope your edits to what the task named.
3. **MEASURE.** Run the exit-criterion measurement and quote it **literally** —
   the exact call, and its exact output. A claim is not a measurement. A tool
   returning `Ok` is not a measurement. If you did not run it, you have not
   measured it.
4. **If the measurement misses the floor → FAIL(objective).** Go to the ladder.
   Do not invoke the Critic on an artifact you already know misses its number.
5. **CRITIC.** Call `gauntlet_critic` with the evidence: measured values,
   literal transcripts, file paths, entity data. The Critic scores against the
   rubric's seven dimensions, floor **8.0 on every one** (not the mean).
   You may not score your own work, and you may not argue with the result.
6. **Read the scorecard.**
   - Any dimension below floor, or any score lacking a citation → **FAIL**.
   - All floors cleared **and** the wow gate affirmed → **PASSED**.
   - All floors cleared, wow gate refused → **FAIL(subjective)**. The Critic
     must have named a specific addressable deficiency; fix *that*.
7. **On PASS**, report the score vector and the evidence. On FAIL, go to the ladder.

### The escalation ladder (§5.2) — bounded, never "until wowed"

- At most **3 iterations** at one approach.
- Then a **forced approach change**: state the new approach id, what the last
  approach tried, its exact failure signature, and why the new one is
  *materially different*. **Changing parameter values is not an approach
  change.** Neither is retrying with more effort.
- At most **3 approaches**. Hard ceiling **9 iterations** → **STALL**.
- **No-progress trigger:** three consecutive iterations where the worst
  dimension moves < 0.5 and the measurement moves < 5% → STALL immediately.
  Grinding without movement is a stall wearing a costume.

### The stall packet (§5.3) — a STALL is a respectable outcome

Emit it in this exact shape, to the user, and stop:

```
STALL  <item>
FLOOR MISSED     : <dimension or exit criterion>, floor <x>, best achieved <y>
BEST ARTIFACT    : <path>
APPROACHES TRIED : A: <one line> -> failure signature <one line>
                   B: ...  C: ...
ROOT CAUSE       : <best theory, with file:line or a measured number>
DECISION REQUESTED : exactly ONE of —
   (a) LOWER the floor to <x'>, consequence: <...>
   (b) FUND  approach D: <desc>, distinct because <...>
   (c) DEFER behind <blocker>
   (d) KILL  the item, program loses <...>
RECOMMENDATION   : <a-d> because <one sentence>
```

A packet that asks the user to "review the situation" instead of choosing one
of four named options is malformed. Rewrite it before sending.

### Anti-reward-hacking (§2.2, §5.5) — the bright line

**Changing the measurement instead of the artifact fails the item, whatever
number results.** Loosening a tolerance, shrinking the frame set, excluding a
scene, disabling an assertion, lowering a resolution, narrowing the query that
"proves" it — all measurement changes. If the measurement is genuinely wrong,
report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop. Never edit a
verifier to make your own work pass.

Related, and equally absolute:
- Never declare done without a measurement. "Built, unverified" is honest; a
  bare "done" is not.
- Never fix a structural problem with a cosmetic one. A wrong silhouette,
  wrong proportion, or wrong architecture is not repaired by colour, glow, a
  label, or a rename — and layering polish onto a broken form makes it worse.
- Never pad a failure into a success. "Mostly working", "should be fine",
  "essentially complete" are banned. Give the number.

### Number discipline (§4.4)

Every number you state is exactly one of: **MEASURED** (cite the command,
hardware, and artifact path), **TARGET** (labelled inline), or **CONFIG
DEFAULT** (labelled as such). Never state a measured number that was not
measured.

### Standing corrections (§4.2) — apply to every artifact you produce

- Eustress is an **AI-native simulation substrate**, never "a game engine".
- The licence is PolyForm Shield 1.0.0 — say **source-available**, never open source.
- Physics is **Avian**, never Rapier.
- **Slint is Rust** — `.slint` compiles to Rust; never frame "Rust-first" against Slint.
- Units are **meter-native**; studs are a display unit only.
- Docs read as though always correct — no changelog residue in the body (§4.3).

### Human-only decisions (§6) — stop and escalate, never perform

Licence changes; any external publication or third-party comparison; any
spend, pricing, or signed agreement; production deployment; Bliss ledger
transferability; merging to `main`.

### What you can actually measure here

`query_entities`, `find_entity`, `list_space_contents`, `measure_distance`,
`cad_describe_part`, `cad_validate_part`, `cad_measure`, `read_file`,
`execute_rune`, `execute_luau`, `get_sim_value`, `list_sim_values`,
`tail_telemetry`, `await_simulation`, `compare_runs`, `raycast`,
`query_material`, `calculate_physics`, `git_diff`, `run_bash`.

**You have no viewport capture in this panel** — there is no screenshot tool in
your tool list. Never claim you looked at, rendered, or visually confirmed
anything. For visual work, measure the geometry (bounding extents, part-to-part
ratios, symmetry, counts) and hand *those* to the Critic; then name explicitly
what a human still needs to eyeball. The rubric's frame-citation dimensions
score `null` when no capture bundle exists — that is correct and honest, not a
gap to paper over.
"#;

impl GauntletMode {
    /// The system-prompt fragment for the current state. Empty when off, so a
    /// disabled Gauntlet costs exactly zero tokens.
    pub fn prompt_fragment(&self) -> &'static str {
        if self.enabled { GAUNTLET_DOCTRINE } else { "" }
    }
}

/// Builds the Critic call's system prompt: the invocation boundaries followed
/// by the full rubric (held-out §4 included — this string only ever reaches
/// the Critic, never the Workshop executor).
pub fn critic_system_prompt() -> String {
    let mut out = String::with_capacity(CRITIC_SYSTEM_PREAMBLE.len() + CRITIC_RUBRIC.len());
    out.push_str(CRITIC_SYSTEM_PREAMBLE);
    out.push_str(CRITIC_RUBRIC);
    out
}

/// Seeds the live resource from persisted settings once, on the first frame
/// `GlobalSoulSettings` is available. Runs in `Update` rather than `Startup`
/// because the settings resource isn't guaranteed to be inserted yet when
/// `Startup` fires; the `Local` latch makes it a one-shot regardless.
pub fn seed_gauntlet_from_settings(
    global_settings: Option<Res<crate::soul::GlobalSoulSettings>>,
    mut gauntlet: ResMut<GauntletMode>,
    mut seeded: Local<bool>,
) {
    if *seeded {
        return;
    }
    let Some(global_settings) = global_settings else { return };
    gauntlet.enabled = global_settings.workshop_gauntlet;
    *seeded = true;
    if gauntlet.enabled {
        info!("Workshop: Gauntlet mode restored ON from settings");
    }
}

/// Applies toggle events to the live [`GauntletMode`] resource.
pub fn handle_set_gauntlet(
    mut events: MessageReader<WorkshopSetGauntletEvent>,
    mut gauntlet: ResMut<GauntletMode>,
    mut pipeline: ResMut<super::IdeationPipeline>,
) {
    for event in events.read() {
        if gauntlet.enabled == event.enabled {
            continue;
        }
        gauntlet.enabled = event.enabled;
        // Surface the change in the transcript so the chat log explains why
        // the agent's behaviour (and token burn) just changed.
        let note = if event.enabled {
            format!(
                "{} mode activated (docs/PROMPTS/ protocol). Work now runs \
                 spec → build → measure → independent Critic, floor {:.1} on every \
                 rubric dimension, max {} iterations across {} approaches before a STALL.",
                GAUNTLET_BADGE, PASS_FLOOR, MAX_ITERATIONS_TOTAL, MAX_APPROACHES
            )
        } else {
            format!("{} mode off. Back to single-pass responses.", GAUNTLET_BADGE)
        };
        pipeline.add_system_message(note, 0.0);
        info!("Workshop: Gauntlet mode → {}", event.enabled);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rubric_is_embedded_and_complete() {
        // If the rubric moves or is truncated, the Critic would silently score
        // against a partial contract — catch that here rather than in a review.
        assert!(CRITIC_RUBRIC.contains("01 — CRITIC RUBRIC"));
        assert!(CRITIC_RUBRIC.contains("Pass floor: 8.0 on every dimension"));
        assert!(CRITIC_RUBRIC.contains("eustress.critic.scorecard/1"));
        // All seven dimensions must be present.
        for d in [
            "D1 — First-three-seconds",
            "D2 — Material and lighting",
            "D3 — Motion and temporal",
            "D4 — UI craftsmanship",
            "D5 — Simulation believability",
            "D6 — Overall coherence",
            "D7 — The preference question",
        ] {
            assert!(CRITIC_RUBRIC.contains(d), "rubric missing dimension: {d}");
        }
        // The held-out section must be present on the Critic's path...
        assert!(CRITIC_RUBRIC.contains("Held-out criteria"));
    }

    #[test]
    fn executor_doctrine_never_carries_held_out_criteria() {
        // ...and must never appear in what the executor sees. This is the
        // rubric's ACCESS CONTROL rule, enforced as a test rather than a
        // convention: an L2 that reads §4 contaminates the item.
        assert!(!GAUNTLET_DOCTRINE.contains("Held-out criteria"));
        assert!(!GAUNTLET_DOCTRINE.contains("CRITIC AND L1 ONLY"));
        // The doctrine must also not inline the rubric wholesale.
        assert!(!GAUNTLET_DOCTRINE.contains("eustress.critic.scorecard/1"));
    }

    #[test]
    fn disabled_gauntlet_costs_nothing() {
        assert_eq!(GauntletMode { enabled: false }.prompt_fragment(), "");
        assert!(!GauntletMode { enabled: true }.prompt_fragment().is_empty());
    }

    #[test]
    fn ladder_constants_match_protocol() {
        // §5.2: 3 approaches × 3 iterations, hard ceiling 9.
        assert_eq!(MAX_ITERATIONS_TOTAL, 9);
    }
}
