# V-Cell Technology Readiness Ledger

The loop target is TRL 9. This file records where the cell actually stands,
what evidence each level needs, and what is blocking the next one. It is
updated each loop iteration; a level is claimed only when its exit evidence
exists and is linked here.

## Current level: TRL 2 — technology concept formulated

| Level | Name | Status |
|---|---|---|
| 1 | Basic principles observed | Done — Li-metal / argyrodite sulfide / Li2S chemistry is in the literature |
| 2 | Concept formulated | Done — `crates/engine/src/simulation/electrochemistry.rs` models five coupled loss channels |
| 3 | Analytical + experimental proof of concept | **Blocked** — see below |
| 4 | Component validation in a lab | Needs coin cells / single-layer pouches |
| 5 | Component validation in a relevant environment | Needs multilayer pouches under stack pressure, thermal cycling |
| 6 | System prototype in a relevant environment | Needs a module with BMS and pressure fixture |
| 7 | System prototype in an operational environment | Needs a pack in a test vehicle |
| 8 | System complete and qualified | Needs UN 38.3, UL 2580 / ECE R100, abuse tests, PPAP |
| 9 | System proven in operation | Needs a fleet in service with field-return data |

No simulation result, however good, moves the cell past TRL 3. Levels 4 to
9 each need physical hardware and measured data.

## Why TRL 3 is not claimed yet

TRL 3 needs the analytical model to be shown to predict something measured.
Today it does not:

1. **Uncalibrated parameters.** These defaults set the life numbers and none
   of them is fitted to published data yet
   (`crates/common/src/realism/particles/components.rs`):

   | Parameter | Value | Controls |
   |---|---|---|
   | `DEFAULT_CRACK_K` | 1.0e-4 | Cathode fatigue per unit charge |
   | depth exponent in `f_depth` | 0.8 (→ D^1.8 per cycle) | Lithium loss against depth of discharge |
   | `roughness_k` fallback | 105.7 | Calibrated to an *assumed* CE of 0.995, not a measured one |
   | `crack_pressure_exponent` | unset (sign unknown) | Whether pressure protects or fractures the cathode |
   | `DEFAULT_CREEP_K`, `CREEP_EXPONENT` | 5.29e-10, 6.6 | Lithium creep knee |
   | `DEFAULT_BRIDGE_CHI`, `_WEIBULL_M`, `_SCALE` | 0.25, 4.0, 0.35 | Separator bridging (soft shorts) |

2. **No regression tests.** There are zero `#[test]` functions in the model
   or its component file. A change to any mechanism can move the life number
   silently.

3. **Build not verified in the cloud.** The engine crate needs more than 15 GB
   of memory to type-check (rustc is killed with SIGKILL even with `-j1`), so
   the last two desktop build failures cannot be checked from the cloud
   session. `eustress-common`, which holds the constants, does compile.

### TRL 3 exit criteria

- [ ] Each parameter above is fitted to, or bounded by, a cited dataset
      (published Li-metal/argyrodite cycling, Li2S cathode fatigue, Li creep).
- [ ] The degradation math is pulled into a pure function with unit tests that
      reproduce at least two published cycling curves within stated error.
- [ ] The engine builds green, and the build is recorded here.

## What the model's own defaults already say

Worked from the code by hand, not from a run. All figures assume stack
pressure at the 2 MPa reference and no authored pressure exponent.

- Cathode crack damage per full cycle is `2 · crack_k · D^2.5`. At 100 % depth
  of discharge that is 2e-4 per cycle, so **cracking alone reaches 80 %
  retention in about 1,000 cycles**. The lithium reservoir does not help: it
  replaces lithium, not a fractured cathode.
- Assuming about 330 miles per full-pack cycle, miles to 80 % from cracking
  alone ≈ 330,000 · D^-1.5. **A million miles needs the cells run at about
  48 % depth of discharge or less**, which means a pack roughly twice the size
  of the daily need.
- So the binding constraint on the million-mile / 10-year target is the Li2S
  cathode's ~80 % volume swing, not lithium inventory. The reservoir decision
  was right for the lithium channel and does nothing for this one.

These are only as good as `DEFAULT_CRACK_K`, which is uncalibrated. Calibrating
it is the single most important TRL 3 task, because it decides whether the
target is reachable with this cathode at all.

## Log

| Date | Iteration | Change | TRL |
|---|---|---|---|
| 2026-09-30 | 1 | Ledger created; cracking identified as binding limit; cloud build blocked by memory | 2 |
