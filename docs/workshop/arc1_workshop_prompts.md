# ARC-1 Reactor — Workshop AI Prompts
# Three-Phase Learning Pipeline: Observe → Analyse → Codify

These prompts are pasted verbatim into the **Eustress Workshop** (Simulation mode).
The AI drives the engine through MCP tools and writes its findings back to disk.
Each phase builds on the previous one.  Do not skip phases.

---

## Pre-flight Checklist

Before starting any phase, confirm:
- [ ] Engine is running with an `ArcReactorCore` entity loaded in the active space
- [ ] Simulation mode is active in the Workshop
- [ ] `arc_reactor_experiment_runner.rune` is attached as a SoulScript on the reactor entity
- [ ] Recording output folder exists: `docs/arc1/data/`
- [ ] The reactor is in steady state: `arc1.neutron_population ≈ 1.0`, `arc1.is_scrammed = 0`

---

## Phase 1 — Observation (7 Canonical Experiments)

### Prompt 1-A · Baseline Steady State

```
You are studying the ARC-1 fission reactor.  Your job is to record the
exact steady-state operating point so we can use it as a reference for
all future experiments.

Steps:
1. Check the current reactor state: call get_sim_value for each of these
   watchpoints and record ALL values to a markdown table in your reply:
     arc1.neutron_population, arc1.reactivity, arc1.core_temp_celsius,
     arc1.coolant_temp_celsius, arc1.thermal_power_watts,
     arc1.electrical_output_w, arc1.battery_soc_pct, arc1.load_demand_watts,
     arc1.rod_bank_a_pct, arc1.rod_bank_b_pct, arc1.coolant_flow_pct,
     arc1.total_efficiency, arc1.power_balance_watts

2. If the reactor is not in steady state (|n − 1.0| > 0.05 or |T − 847| > 50),
   call set_sim_value("arc1.cmd.set_neutron_sp", 1.0) and wait 10 simulation
   seconds (use control_simulation action=play, time_scale=10, then pause).
   Re-read all watchpoints.

3. Call execute_rune on the reactor's SoulScript with command "begin_session"
   and session_id "baseline_001".  This starts the data recorder.

4. Let the simulation run for 30 simulation seconds at 10× speed.

5. Call execute_rune with command "end_session".  This saves
   docs/arc1/data/baseline_001.json.

6. Summarise: at nominal 50%/50% rod insertion and 70% coolant flow, what is
   the exact electrical output and what is the power balance?  Is the battery
   charging or discharging?
```

---

### Prompt 1-B · Load Step — Demand Surge (+720W)

```
You are studying how the ARC-1 responds to a sudden increase in load demand.
This tests the AI PID controller's ability to ramp power quickly without
causing a temperature excursion.

Steps:
1. Confirm baseline: read arc1.electrical_output_w and arc1.load_demand_watts.
   The output should be approximately 420W and load ≈ 280W.

2. Begin a recording session: execute_rune command="begin_session"
   session_id="load_step_up_001".

3. Apply the disturbance: set_sim_value("arc1.cmd.set_load_watts", 1000.0)

4. Run the simulation for 60 simulation seconds at 20× speed.
   Every 10 seconds (wall-clock), pause and read:
     arc1.neutron_population, arc1.core_temp_celsius,
     arc1.electrical_output_w, arc1.rod_bank_a_pct, arc1.coolant_flow_pct
   Record each snapshot in a table: [sim_time | n | T_core | P_elec | rod_A | flow]

5. End the session: execute_rune command="end_session".

6. Observe and report:
   a) How many seconds did it take for P_elec to reach 1000W (settling time)?
   b) Did the core temperature exceed 1000°C during the transient?
   c) What was the peak neutron population during the ramp?
   d) Did the PID overshoot? If so, by how much in °C?
   e) What was the final steady rod insertion and coolant flow?

Hypothesis to test: the reactivity PID should withdraw rods to increase power,
while the thermal PID increases coolant flow to absorb the extra heat.
Confirm or refute this from your observations.
```

---

### Prompt 1-C · Load Step — Demand Drop (−800W)

```
You are studying the ARC-1 response to a sudden load decrease.  This is
the inverse of 1-B and tests whether the reactor safely reduces power
without going supercritical.

Steps:
1. Start from the end state of 1-B (load=1000W, P_elec≈1000W) OR
   manually set: set_sim_value("arc1.cmd.set_load_watts", 1000.0) and wait
   for steady state (30s at 10× speed).

2. Begin recording: execute_rune command="begin_session" session_id="load_step_down_001".

3. Apply the disturbance: set_sim_value("arc1.cmd.set_load_watts", 200.0)

4. Run 60 simulation seconds at 20× speed, pausing every 10s to record:
   [sim_time | n | T_core | P_elec | rod_A | rod_B | flow | battery_soc]

5. End session.

6. Report:
   a) Did the reactor insert rods to reduce power? What was the final rod %?
   b) Did battery SoC increase (surplus power charging the buffer)?
   c) Was there any temperature dip below ambient from over-cooling?
   d) Settling time to P_elec ≈ 200W?
   e) Compare asymmetry: was the step-up or step-down faster to settle?

Critical safety question: if load drops to zero (pure battery charge), does
the reactor safely insert rods to near-critical, or does it keep burning fuel
into the battery until SCRAM temperature?  Test this: set load to 0W and
observe for 20 simulation seconds.  Report max temperature reached.
```

---

### Prompt 1-D · Coolant Degradation Stress Test

```
You are stress-testing the ARC-1 thermal safety system.  You will reduce
coolant flow to simulate a pump degradation and observe whether the Doppler
feedback + auto-SCRAM prevents a runaway before human (operator) intervention.

Safety note: this experiment will likely trigger an auto-SCRAM.  That is the
correct outcome.  Record the exact temperature at SCRAM and the decay heat profile.

Steps:
1. Start from baseline (rod=50%, flow=70%, load=280W).
   Begin session: session_id="coolant_stress_001".

2. Reduce coolant flow in steps.  After each step, wait 15 simulation seconds
   and record [flow_pct | T_core | T_coolant | P_th | n | reactivity]:
   - set_sim_value("arc1.coolant_flow_pct", 50.0)  → wait 15s
   - set_sim_value("arc1.coolant_flow_pct", 30.0)  → wait 15s
   - set_sim_value("arc1.coolant_flow_pct", 15.0)  → wait 15s
   - set_sim_value("arc1.coolant_flow_pct", 5.0)   → wait 15s (likely SCRAM)

3. After SCRAM (or at 5% flow), continue recording for 120 simulation seconds
   to capture the decay-heat profile.  Record every 10s:
   [sim_time | is_scrammed | T_core | decay_heat_watts | rod_A | rod_B]

4. End session.

5. Report:
   a) At what flow % did Doppler feedback become insufficient and T started
      rising uncontrollably?  (i.e., the passive-safety boundary)
   b) What was T_core at the moment of auto-SCRAM?
   c) After SCRAM, how long did it take for T_core to drop below 900°C?
   d) What was the peak decay heat in watts?
   e) Plot (in text) the decay heat curve: t=1s,10s,30s,60s,120s values.
      Compare against the Way-Wigner formula: Q_d = 0.066 · 3200 · t^(-0.2)
```

---

### Prompt 1-E · Rod Reactivity Sensitivity Sweep

```
You are mapping the rod-position → reactivity → power relationship.
This data will directly feed the deterministic feedforward control law in Phase 3.

Goal: measure electrical output at 10 different rod insertion levels,
holding coolant at 70% and load demand at the measured output (no deficit).

Steps:
1. For each rod insertion value in [20, 30, 35, 40, 45, 50, 55, 60, 65, 70]:
   a) Set both banks: set_sim_value("arc1.rod_bank_a_pct", VALUE)
                      set_sim_value("arc1.rod_bank_b_pct", VALUE)
   b) Set load to match expected output: set_sim_value("arc1.cmd.set_load_watts",
      <your best estimate based on previous readings>)
   c) Wait 20 simulation seconds at 10× speed for steady state.
   d) Read and record: [rod_pct | n | reactivity | T_core | P_th | P_elec | battery_soc]

2. Disable the AI controller for this experiment to avoid it fighting your
   manual inputs: set_sim_value("arc1.ai_override_enabled", 0.0)
   Remember to re-enable at the end: set_sim_value("arc1.ai_override_enabled", 1.0)

3. Save all 10 data points.

4. Analyse the curve:
   a) What is the mathematical relationship between rod_pct and P_elec?
      Is it linear? Quadratic?  Fit a simple polynomial to the 10 points.
   b) What rod insertion gives exactly 280W output? 500W? 1000W? 2000W?
   c) At what insertion level does the reactor go subcritical (n→0)?
   d) Is there a "sweet spot" rod position where dn/dt ≈ 0 exactly?

This data is the foundation of the steady-state feedforward map.
```

---

### Prompt 1-F · Efficiency Sensitivity Sweep

```
You are mapping how thermoelectric and Stirling efficiency affect the
output power for a fixed thermal budget.  This isolates the conversion
stage from the nuclear stage.

Steps:
1. Fix the nuclear state: rods=50%, flow=70%, AI disabled.
   Wait for steady state: n≈1.0, T≈847°C.

2. Sweep te_efficiency over [0.05, 0.08, 0.11, 0.14, 0.17, 0.20, 0.22]:
   For each value:
   - set_sim_value("arc1.te_efficiency", VALUE)  (note: this requires a new
     MCP tool or you can set via entity property update_entity)
   - Wait 5 simulation seconds.
   - Record [te_eff | stirling_eff | total_eff | P_th | P_elec]

3. Sweep stirling_efficiency over [0.10, 0.15, 0.20, 0.25, 0.28, 0.32, 0.38, 0.40]
   keeping te_efficiency fixed at 0.14.

4. Compute η_total = η_TE + η_St · (1 − η_TE) for each row and verify it
   matches arc1.total_efficiency.

5. Report: which efficiency levers give the most gain per unit change?
   Is there diminishing returns at high η?

This data feeds the power-budget calculation in the deterministic control law.
```

---

### Prompt 1-G · Cold Start Startup Sequence

```
You are studying the controlled startup from a cold, subcritical state.
This is the most safety-critical operation: if rods are withdrawn too fast
the reactor can go supercritical before the thermal feedback stabilises.

Steps:
1. Force a full SCRAM to get to subcritical state:
   set_sim_value("arc1.manual_scram", 1.0)
   Wait 5 simulation seconds.

2. Confirm SCRAM: arc1.is_scrammed = 1.0, arc1.rod_bank_a_pct = 100.0

3. Begin session: session_id="cold_start_001".

4. Perform a controlled startup by withdrawing rods in steps:
   Keep coolant at 100% during startup.
   For each step, wait 15 simulation seconds and record
   [rod_pct | n | T_core | reactivity | P_th | P_elec]:
   - rods → 90% (approach critical)
   - rods → 80%
   - rods → 70%
   - rods → 60%
   - rods → 50% (expected near-critical)
   Note: first criticality (n > 0.01) is a milestone — record the exact rod %

5. Once critical, transition to AI control:
   set_sim_value("arc1.ai_override_enabled", 1.0)
   set_sim_value("arc1.cmd.set_load_watts", 280.0)
   Wait for steady state (60s at 10× speed).

6. End session.

7. Report:
   a) At what exact rod % did the reactor go critical?
   b) How long did it take to reach n=1.0 from first criticality?
   c) Was there any temperature overshoot above 1000°C?
   d) Compare to the dashboard startup sequence: rods to 30% → criticality → power ascent.
      Which is safer?
   e) What is the recommended rod withdrawal rate (% per second) for safe startup?
```

---

## Phase 2 — Pattern Analysis

### Prompt 2-A · Load Session Snapshot Review

```
You are now analysing the data collected in Phase 1 to find the governing
control law for the ARC-1 reactor.

Available data files (load them with read_file):
  docs/arc1/data/baseline_001.json
  docs/arc1/data/load_step_up_001.json
  docs/arc1/data/load_step_down_001.json
  docs/arc1/data/rod_sweep_<timestamp>.json    (from 1-E)

Your task:
1. From baseline_001 and the rod sweep data, build a table:
   | load_W | rod_pct | flow_pct | T_core | n | P_th |

2. Fit a linear model: rod_pct = A · load_W + B
   Use the rod-sweep data points to compute A and B.
   Show your working.

3. Verify the fit: apply your formula to load=280W, 500W, 1000W, 2000W and
   compare against the recorded rod_pct values.  What is the mean absolute error?

4. Derive the steady-state coolant law: what flow_pct minimises T_core
   deviation from 847°C for each load level?
   Fit: flow_pct = C · P_th + D

5. Compute the full feedforward law:
   Given load_demand_W and η_total:
     P_th_needed  = load_demand_W / η_total
     rod_pct      = A · load_demand_W + B   (from step 2)
     flow_pct     = C · P_th_needed + D     (from step 4)

   Does this feedforward alone keep n≈1.0 without the PID?  Predict what
   steady-state error remains.

6. Write the coefficients A, B, C, D to docs/arc1/feedforward_coefficients.toml
```

---

### Prompt 2-B · Transient Analysis — Settling and Overshoot

```
Analyse the step-response data from 1-B and 1-C.

Load docs/arc1/data/load_step_up_001.json and load_step_down_001.json.

For each experiment:
1. Find the settling time: first time P_elec stays within 5% of the new
   setpoint for at least 5 consecutive seconds.

2. Find the peak overshoot in temperature: max T_core during the transient
   minus the final steady-state T_core.

3. Find the undershoot (if any) in neutron population: min n during the step.

4. Compute the time constant τ: fit P(t) = P_final · (1 − e^(-t/τ))
   to the power ramp-up curve.  τ is the first-order approximation of
   the system lag.

5. Use τ to recommend new PID gains:
   For a first-order system, Ziegler-Nichols step-response method gives:
     Kp = 1.2 · τ / (K · L)
     Ki = Kp / (2 · L)
     Kd = Kp · L / 2
   where K = steady-state gain = ΔP / Δrod, L = dead time.
   Estimate K from the 1-E sweep data.

6. Compare the Ziegler-Nichols gains against the current PID constants in
   nuclear/constants.rs:
     PID_REACTIVITY_KP = 0.08, KI = 0.012, KD = 0.04
   Are the current gains over- or under-damped?  What do you recommend?

Write recommendations to docs/arc1/pid_tuning_report.md
```

---

### Prompt 2-C · Safety Margin Mapping

```
Using data from 1-D (coolant stress) and 1-G (cold start), map the
safety margins.

1. From the coolant stress data, identify:
   a) The minimum safe coolant flow (below which T exceeds 1200°C)
   b) The passive safety floor: at what flow does Doppler feedback
      alone keep the reactor from reaching SCRAM temperature?

2. From the cold start data, identify:
   a) The maximum safe rod withdrawal rate (% per second)
   b) The "approach to critical" warning zone: rod% range where
      dn/dt > 0 but control is still safe

3. Build a 2D safety envelope: a table of (rod_pct, flow_pct) combinations
   and whether each is SAFE / WARNING / UNSAFE.
   Use 5% resolution for both axes.

4. Identify the nominal operating point and draw the distance to the
   nearest boundary for each axis.

5. Output the safety envelope to docs/arc1/safety_envelope.toml
   Format: array of {rod_pct, flow_pct, zone: "safe"|"warning"|"unsafe"}
```

---

## Phase 3 — Rule Codification

### Prompt 3-A · Write the Deterministic Control Law

```
You have completed all Phase 1 experiments and Phase 2 analysis.
It is time to replace the AI PID controller with a deterministic rule-based
system that provably keeps the reactor stable.

Read the following files first:
- docs/arc1/feedforward_coefficients.toml    (from 2-A)
- docs/arc1/pid_tuning_report.md             (from 2-B)
- docs/arc1/safety_envelope.toml             (from 2-C)

Then write the implementation of the deterministic control law into:
  eustress/crates/common/src/realism/nuclear/control_law.rs

The law must implement the `ReactorControlLaw` trait with these methods:
  fn compute_rod_insertion(&self, load_w: f32, eta_total: f32) -> f32
  fn compute_coolant_flow(&self, p_thermal: f32, t_core: f32) -> f32
  fn safety_check(&self, state: &ReactorState) -> ControlAction

Rules to implement:
1. FEEDFORWARD: compute rod_pct and flow_pct from the fitted coefficients A,B,C,D
2. CORRECTION: a proportional-only (no integral) residual correction on n
3. SAFETY OVERRIDE: hard limits from the safety envelope — if the computed
   rod or flow would enter WARNING zone, clamp to the safe boundary

The control law should be STATELESS (no integrators, no history).
Given (load_W, T_core, n, η) it always returns the same (rod_pct, flow_pct).
This is the key property that makes it deterministic and verifiable.

After writing the code, update nuclear/mod.rs to offer both:
  ReactorControlMode::Regulation    (PID — for learning phases)
  ReactorControlMode::DeterministicLaw  (new — for production)

Add a watchpoint arc1.control_mode so the Workshop can observe which mode is active.
```

---

### Prompt 3-B · Verification Run

```
You have written the DeterministicLaw control mode.  Now verify it works
at least as well as the PID across the same scenarios.

1. Switch the reactor to DeterministicLaw mode:
   set_sim_value("arc1.cmd.set_control_mode", 2.0)  // 2 = DeterministicLaw

2. Repeat experiments 1-B (load step +720W) and 1-C (load step -800W)
   with the deterministic law active.  Record sessions:
   session_id="deterministic_step_up_001" and "deterministic_step_down_001"

3. Compare key metrics side by side (PID vs Deterministic):
   | Metric           | PID     | Deterministic |
   |------------------|---------|---------------|
   | Settling time    |         |               |
   | T overshoot (°C) |         |               |
   | Peak n           |         |               |
   | Steady-state err |         |               |

4. If deterministic performance is equal or better: write a brief certification
   note to docs/arc1/VERIFICATION_REPORT.md confirming the law is production-ready.

5. If worse: identify the specific scenario where the law fails and report
   what additional correction term is needed (which may require one more
   phase of data collection with Prompt 1-E repeated at finer resolution).

6. Final decision: recommend either:
   a) "Promote DeterministicLaw to default mode" — update ArcReactorAIController
      default in components.rs
   b) "Keep PID as default, DeterministicLaw as optional" — document why
```

---

## Appendix — Watchpoint Quick Reference

All readable via `get_sim_value("arc1.<name>")`:

| Watchpoint | Unit | Description |
|-----------|------|-------------|
| `neutron_population` | n | Normalised (1.0 = full power) |
| `reactivity` | Δk/k | Positive → supercritical |
| `core_temp_celsius` | °C | Centre-line temperature |
| `coolant_temp_celsius` | °C | Bulk coolant exit |
| `thermal_power_watts` | W | Heat generated in core |
| `electrical_output_w` | W | Net power to bus |
| `battery_soc_pct` | % | V-Cell state of charge |
| `load_demand_watts` | W | Current load setpoint |
| `power_balance_watts` | W | +surplus / −deficit |
| `rod_bank_a_pct` | % | Bank A insertion (0=out, 100=in) |
| `rod_bank_b_pct` | % | Bank B insertion |
| `coolant_flow_pct` | % | Mass-flow rate |
| `total_efficiency` | frac | η_TE + η_St(1−η_TE) |
| `decay_heat_watts` | W | Post-SCRAM decay power |
| `is_scrammed` | bool | 1.0 if reactor is shut down |

Writable operator commands:

| Command watchpoint | Effect |
|--------------------|--------|
| `arc1.cmd.set_load_watts` | Change load demand |
| `arc1.cmd.set_neutron_sp` | Change neutron setpoint |
| `arc1.cmd.set_control_mode` | 0=Standby, 1=Regulation(PID), 2=DeterministicLaw |
| `arc1.manual_scram` | Write 1.0 to trigger immediate SCRAM |
| `arc1.ai_override_enabled` | Write 0.0 to disable AI (manual mode) |
