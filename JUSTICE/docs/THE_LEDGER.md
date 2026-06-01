# THE LEDGER — Knowing the True State of Justice

> **Cheap, durable, honest instrumentation for a justice system — so it runs on measured truth,
> not on the confidence of the people with the most power and the least oversight.**
>
> Modeled on the WATER project's `WELL_METER.md` and the WAREHOUSE project's `TRACK_TRACE.md`:
> the same way India cannot manage an aquifer it does not measure, and a supply chain cannot run
> on inventory no one has verified, a justice system cannot be just if it does not know its own
> error rate. This document is the measurement layer that closes the data gaps in
> [`README.md`](README.md) §11. It is also where we draw — carefully, because the stakes are a
> human mind — the line between **measuring what a person did** and **surveilling who a person is.**

---

## Table of Contents

1. [The Problem: An Unmeasured System](#1-the-problem-an-unmeasured-system)
2. [Inspiration: What Already Works (and What Doesn't)](#2-inspiration-what-already-works-and-what-doesnt)
3. [Design Principles: Measure the System, Not the Soul](#3-design-principles-measure-the-system-not-the-soul)
4. [The Four Tiers of Measurement](#4-the-four-tiers-of-measurement)
5. [What Each Tier Can and Cannot Know](#5-what-each-tier-can-and-cannot-know)
6. [The Line: Conduct vs. Conscience](#6-the-line-conduct-vs-conscience)
7. [The Record: Append-Only, Attributable, Inspectable](#7-the-record-append-only-attributable-inspectable)
8. [Rust: The Error-Rate Sampler and the Conduct Record](#8-rust-the-error-rate-sampler-and-the-conduct-record)
9. [Risk Assessment, Honestly](#9-risk-assessment-honestly)
10. [The Guardrails: What We Will Not Build](#10-the-guardrails-what-we-will-not-build)
11. [Integration with the Justice System Flow Analysis](#11-integration-with-the-justice-system-flow-analysis)

---

## 1. The Problem: An Unmeasured System

From [`README.md`](README.md) — the single most important data gap, restated because it is the reason this document exists:

> *"The justice system does not know its own error rate. It does not know how many innocent people
> are in its prisons."*

**Current state in a typical system:**
- "Clearance rate" counts cases *closed* (often by arrest), not cases *correctly resolved.* A wrongful arrest improves the clearance rate.
- "Conviction rate" counts wins, and a coerced plea from an innocent person is a win.
- "Recidivism" is usually re-*arrest,* which measures how heavily someone is policed after release as much as whether they re-offended.
- The wrongful-conviction rate — the Type I rate, the deadliest number — **is not collected at all,** because the institution that would have to count it is the one that made the error (pride / infallibility, `README.md` §7).
- Conditions in custody are known only at the moments an outside inspector happened to look.
- The reasons cases exit the funnel (correct dismissal vs. "no one had time") are not recorded — only the counts.

**What measurement unlocks** (the same logic as WELL_METER and TRACK_TRACE, translated to justice):
- A true Type I rate → Blackstone's ratio (`README.md` §3) can be *calibrated* instead of merely asserted.
- True detention-days-of-the-never-convicted → the `JusticeLedger` sink (`README.md` §2, §10) becomes visible and answerable.
- Funnel exit *reasons* → correct attrition can be told from failure-attrition (`README.md` §5).
- Disaggregated stops/force/sentences → discriminatory enforcement stops hiding in the average.
- A defensible, attributable record → the basis for honest appeals, fair oversight, and public trust.

The principle is identical to the water and supply-chain projects': **you cannot be just about a thing you cannot see, and you cannot fix what you will not measure.** But justice adds a constraint the other two did not have, and it is the heart of this document:

> **In a warehouse you measure the goods. In a justice system you are tempted to measure the
> person — and there is a hard limit on how far into a person the state is allowed to look. The
> measurement that makes justice honest is measurement of the *system's* conduct and the *person's*
> demonstrated *acts.* The measurement that makes justice tyrannical is surveillance of the
> person's *mind.* This document builds the first and refuses the second.**

---

## 2. Inspiration: What Already Works (and What Doesn't)

WELL_METER stood on the USGS groundwater-monitoring model; TRACK_TRACE stood on GS1/EPCIS. The justice equivalents exist, and are worth standing on — and worth being honest about where they fall short.

### What existing practice does right
- **The National Registry of Exonerations** and DNA-exoneration research proved that the Type I rate is *measurable* — every exoneration is one confirmed false positive, and the causes (eyewitness error, false confession, informants, forensic overstatement, official misconduct) are now catalogued.
- **Conviction-integrity units** inside some prosecutors' offices institutionalize the search for the office's *own* errors — pride's antidote.
- **Court case-management systems** already timestamp every event in a case's life — the raw material for Little's Law (`README.md` §5).
- **Body-worn cameras** can make the scene (`README.md` §6, Stage 2) reviewable instead of merely narrated.
- **Independent custody inspection** (where it exists with real power) makes the dignity floor visible.

### What a system built for truth needs differently

| Existing practice | Truth-first adaptation |
|-------------------|------------------------|
| "Cleared by arrest" counts as solved | Track every case to its *outcome,* including later exoneration |
| Exoneration counted as an embarrassment | Exoneration counted as an error *found* — a sign of health (`README.md` §13) |
| Body camera the officer can switch off | Continuous capture; the *gap* in footage is itself a recorded, alarmed event |
| Recidivism = re-arrest | Three distinct measures: re-arrest / re-conviction / re-incarceration, each labeled |
| Complaint disappears into a file | Every complaint gets a tracking number and a published resolution (the `GRIEVANCE_PM` pattern) |
| Risk score *is* the decision | Risk score is one input a human must *justify* following or overriding, on the record (§9) |

The tools prove the concept. A system that *acts* on the data — that treats a wrongful conviction as a sink to be investigated rather than a file to be sealed — is the part that is rare. As in the sibling projects, the rare part is not the sensor. It is the willingness to believe the sensor when it says you were wrong.

---

## 3. Design Principles: Measure the System, Not the Soul

The same three constraints WELL_METER and TRACK_TRACE ranked, plus a fourth that only justice needs:

**1. Cheap — so it actually gets done.** Measurement that requires heroic effort will be skipped under load (the sloth failure, `README.md` §7). Lean on the timestamps the system *already* generates; sample where a census is too costly. The Type I rate does not need a full census to be useful — a rigorous *sample* (audit a random set of closed cases to modern evidentiary standard) gives a calibrated estimate, the way GRACE-FO gave the aquifer a mass-loss signal without metering every well.

**2. Durable / low-friction.** A measurement that depends on the good behavior of the party being measured will be gamed. Prefer records that are append-only and attributable (§7), and signals that are hard to suppress (a *gap* in continuous body-camera footage is itself evidence).

**3. Accurate enough for the decision — and honest about the residual.** Not certainty; *decision* confidence, with the uncertainty stated. A wrongful-conviction-rate estimate of "between 2% and 6%, and here is our method" is worth infinitely more than a confident "essentially zero" that was never measured.

**4. (Justice only) Bounded — measure conduct, never conscience.** This is the principle the other two documents never needed, and it is non-negotiable here. The system measures *acts and outcomes* — what happened, what each actor did, what the person being judged demonstrably did over time. It does **not** measure, infer, score, or store a person's *beliefs, thoughts, or inner character.* The reasons are developed in §6 and §10; the principle is stated here because it governs every design choice below.

---

## 4. The Four Tiers of Measurement

> Like the tag tiers in TRACK_TRACE, these are matched to the decision they inform. Over-measuring
> is not zeal; in justice, over-measuring is the first step toward the surveillance state this
> document exists to refuse (§10).

| Tier | What it captures | Answers | Who it measures | Legitimacy |
|------|------------------|---------|-----------------|------------|
| **T0 Identity** | The case and the actors, uniquely named | *Which case, which officer, which judge* | The system | Baseline — nothing can be anonymous, including the state's agents |
| **T1 Event** | What happened, when, where, recorded by whom | *Did the step occur as reported?* | The system's conduct | High — this is the conservation ledger (`README.md` §2) |
| **T2 Outcome** | Where the case ended, and *why* | *Correct resolution or failure?* | The system's accuracy | High — outcomes, including later exonerations |
| **T3 Conduct-over-time** | A released/incarcerated person's *demonstrated acts* across years | *Has this person demonstrably changed?* | The person's **acts** (never thoughts) | Bounded — legitimate only as demonstrated behavior, never as inferred interior state (§6) |

The discipline is **tier-matching with a hard ceiling.** T0–T2 measure the *system* and should be near-complete, because the state must be the most-watched actor in its own justice process — that is the inversion that keeps it honest. T3 measures the *person,* is strictly limited to demonstrated conduct, and stops dead at the boundary of the mind. There is no T4. **A tier that scores a person's character or predicts their soul does not exist in this design, by refusal, not by omission.**

---

## 5. What Each Tier Can and Cannot Know

Honesty about the limits of measurement is itself part of the measurement — a number trusted beyond its real capability is a new source of false confidence, and in justice a false confidence is a wrongful conviction waiting to happen.

```rust
/// What a given tier of justice-measurement can legitimately assert.
/// Claiming more than this is a measurement lie dressed as data — and in this
/// domain, a measurement lie costs a human life, not a misplaced pallet.
#[derive(Debug, Clone, Copy, PartialEq, Reflect, Serialize, Deserialize)]
pub enum MeasureTier { Identity, Event, Outcome, ConductOverTime }

impl MeasureTier {
    /// Can this tier establish that a system step actually happened as reported?
    pub fn verifies_system_conduct(&self) -> bool {
        matches!(self, MeasureTier::Event | MeasureTier::Outcome)
    }

    /// Can this tier establish demonstrated change in a person?
    /// Only via ACTS over time — and even then it is evidence, never proof of the soul.
    pub fn evidences_change(&self) -> bool {
        matches!(self, MeasureTier::ConductOverTime)
    }

    /// Can ANY tier read a person's thoughts, predict their "true self",
    /// or score their character? No. Not one. By design, not by limitation.
    pub fn reads_mind(&self) -> bool {
        false
    }
}
```

Key honest limits:
- **An event record knows that a step was *logged,* not that it was logged *truthfully*** — which is why every assertion is attributable (§7): the deterrent is that the lie has a name on it.
- **An outcome knows where a case ended, not always whether that ending was just** — a plea closes a case whether the pleader was guilty or merely cornered (`README.md` §10). The outcome tier must record *how* a conviction was obtained (trial / plea / the terms), or it will count coerced pleas as clean wins.
- **A conduct-over-time record knows what a person *did,* never what they *are*** — years of nonviolence, program completion, and restitution are strong evidence a person has changed; they are not a window into the soul, and must never be sold as one (§6).
- **No tier detects an injustice it was not designed to detect** — measurement narrows the truth gap; it never closes it to zero. The residual is permanent, and humility about it is the difference between a system that catches its errors and one that is merely sure it has none.

---

## 6. The Line: Conduct vs. Conscience

This is the center of the document, and it is the honest answer to the hardest thing in the founding brief — the wish to release people *"after systematically discovering their true selves through introspection, time, and data mining their thoughts."* The wish is decent at its root: it wants release to be **earned and evidenced,** not granted by gut or denied by bias. That root is right, and the system should be built to honor it. But the *method* — mining thoughts to read the true self — has to be split into the part that is sound and the part that is dangerous, because they are not the same thing, and conflating them is how good intentions build bad machines.

### The part that is sound: time and demonstrated conduct

A person's **acts over a long arc are real, measurable evidence of change**, and a system that ignores them — that releases or holds by hunch, mood, or prejudice — is failing the person *and* the public. This is legitimate, and it is most of what the wish was actually after:

- **Time** itself is evidence: desistance from crime is strongly age-related; the arc of years changes people, and the data shows it.
- **Demonstrated conduct:** violence avoided, programs completed, education and work undertaken, restitution paid, the disciplinary record, the sustained pattern.
- **Restorative acts:** facing the victim (at the victim's choice), making amends, the concrete repair of harm.

All of this is **T3 conduct-over-time** (§4). It is acts, it is on the record, it is contestable, and it is the right basis for the conditional, evidence-based release the framework calls for (`README.md` §9). Build this well. It is the answer to "how do we know who has changed" — *you watch what they do, for a long time, and you measure it honestly.*

### The part that is dangerous: mining the mind

"Data mining their thoughts" to find the "true self" — reading the interior, scoring character, predicting the soul — must be refused, and here is the honest, non-preachy reasoning, every step of which you can check:

1. **It is not reliable.** Interior states are not observable. Every instrument that claims to read them — from the polygraph to affect-detection to "criminal personality" scoring — is, on inspection, measuring something *else* (stress, demographics, the scorer's priors) and *labeling* it the soul. A measurement that cannot be validated against ground truth is not a measurement; it is a projection of the measurer's bias wearing a number (`README.md` §6, forensic overstatement).
2. **It punishes the wrong thing.** The system's foundational rule is that **we punish acts, not thoughts.** A person may harbor any thought and remain free; they are answerable for what they *do.* A release regime that turns on inferred thoughts re-punishes a person for their inner life — exactly the move a just system forbids at the front door, smuggled in at the back.
3. **It is unlawful at its root.** The privilege against self-incrimination exists *precisely* so the state cannot excavate a person's mind for grounds to hold their body. A regime that conditions liberty on mind-mining inverts that protection: silence or the "wrong" inner state becomes evidence against you.
4. **It will be used against whoever is disliked.** This is the decisive practical point, and it is the same logic as Blackstone's ratio (`README.md` §3): a tool that lets the state hold people based on *who it judges them to be* will, with certainty, be turned on the unpopular, the dissenting, and the wrongly-convicted-still-protesting-innocence. A system that claims to read true selves does not free the truly-reformed; it cages whomever it has decided is irredeemable — which is the "scum of the earth" intuition (`README.md` §7) handed an instrument.

> **The line, in one sentence:** *Measure what a person did, for as long as it takes to be sure;
> never claim to measure what a person is.* The first is the evidence base for mercy earned and
> danger contained. The second is the machinery of indefinite detention with a data dashboard.
> Honoring the decent root of the wish means building the first **so well** that no one is tempted
> to reach for the second.

```rust
/// The conduct/conscience boundary, enforced in the type system.
/// A release decision may rest on demonstrated acts. It may NOT rest on inferred
/// thoughts, predicted character, or any "true self" score — there is no field
/// for that here, because there must be no such field anywhere.
#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct DemonstratedConduct {
    /// Years since the offense (desistance is age- and time-related — real evidence).
    pub years_elapsed: f32,
    /// Programs completed (education, treatment, vocational) — acts, attributable.
    pub programs_completed: Vec<String>,
    /// Disciplinary record in custody — acts, dated.
    pub serious_infractions: u32,
    /// Restitution / restorative acts undertaken (at the victim's consent).
    pub restorative_acts: Vec<String>,
    // NOTE: there is deliberately NO field for "remorse_score", "true_self",
    // "predicted_character", or "thought_analysis". Their absence is the design.
}

impl DemonstratedConduct {
    /// Evidence of change is a function of ACTS OVER TIME, presented to a human
    /// decider who remains accountable for the outcome (§9). It is an input to
    /// judgment, never the judgment itself, and never a claim about the soul.
    pub fn is_evidence_of_change(&self) -> bool {
        self.years_elapsed > 0.0
            && self.serious_infractions == 0
            && !self.programs_completed.is_empty()
    }
}
```

---

## 7. The Record: Append-Only, Attributable, Inspectable

Every step in a case's life is an event answering **what / when / where / who-asserted-it** — the EPCIS object-event shape from TRACK_TRACE, with one addition justice demands: the state's own agents are the *most* identified parties, not the least. An append-only, attributable record is the technical form of the kept oath (`README.md` §8).

```sql
-- Append-only justice event log. Corrections are new rows, not edits.
-- The state's agents are named; anonymity flows to the watched, not the watcher.
CREATE TABLE justice_events (
    event_id      UUID PRIMARY KEY,
    case_id       TEXT NOT NULL,             -- WHAT case
    occurred_at   TIMESTAMPTZ NOT NULL,      -- WHEN
    stage         TEXT NOT NULL,             -- WHERE in the 8 stages (README §4)
    event_kind    TEXT NOT NULL,             -- WHY (arrest/charge/bail/plea/verdict/...)
    actor_role    TEXT NOT NULL,             -- officer/prosecutor/judge/defender/...
    actor_id      TEXT NOT NULL,             -- the NAMED state agent — attribution is the deterrent
    asserted_by   TEXT NOT NULL,             -- who entered this record
    evidence_ref  TEXT,                      -- pointer to raw evidence (footage/recording), not just summary
    corrects      UUID REFERENCES justice_events(event_id)  -- corrections are visible, not erasures
);

-- The footage GAP is itself an alarmable event: silence where capture was required.
CREATE INDEX idx_events_case_time ON justice_events (case_id, occurred_at);
CREATE INDEX idx_events_actor ON justice_events (actor_role, actor_id);
```

Two honesty features, inherited from TRACK_TRACE and sharpened for justice:
- **`actor_id` — every state action is attributable.** The noble-cause, blue-wall, and dishonest-reporting failures (`README.md` §7) are deterred when "the scene was secured" and "the evidence was disclosed" each have a name attached. In a just system, the people with power are the people most on the record.
- **`corrects` — history is append-only.** You cannot quietly re-write the past to make a conviction look clean; a correction is itself a visible, attributable event. The sealed file that hides the sink (`README.md` §2) is exactly what this forbids.

---

## 8. Rust: The Error-Rate Sampler and the Conduct Record

The two measurements the system most wants to avoid, built so the honest action is the default. First, the Type I rate — sampled, because a census is too costly and a sample is enough to calibrate (the GRACE-FO logic):

```rust
//! The error-rate sampler — estimating the system's Type I (wrongful-conviction)
//! rate by re-examining a random sample of closed cases to modern evidentiary
//! standard. This is the number no institution volunteers; measuring it is the
//! single highest-value act in the whole framework (README §11).

/// The verdict of an independent re-examination of one closed case.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReExamVerdict {
    /// Conviction well-supported by evidence on re-examination.
    Sound,
    /// Serious doubt — evidence would not support conviction today.
    Unsound,
    /// Indeterminate on available record.
    Unknown,
}

/// Estimate the Type I rate from a random, independently-re-examined sample.
/// Returns the rate AND its honest uncertainty — never a bare point estimate.
pub fn estimate_false_positive_rate(sample: &[ReExamVerdict]) -> RateEstimate {
    let n = sample.len() as f32;
    if n == 0.0 {
        return RateEstimate { rate: None, n: 0, note: "No sample — rate is UNKNOWN, not zero." };
    }
    let unsound = sample.iter().filter(|v| **v == ReExamVerdict::Unsound).count() as f32;
    // Point estimate; a real implementation reports a confidence interval.
    RateEstimate {
        rate: Some(unsound / n),
        n: sample.len(),
        note: "Lower bound — 'Unknown' cases may hide further errors. Report the interval.",
    }
}

#[derive(Debug, Clone)]
pub struct RateEstimate {
    /// None means UNMEASURED — which is the lowest confidence, never the highest.
    pub rate: Option<f32>,
    pub n: usize,
    pub note: &'static str,
}
```

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unmeasured_rate_is_unknown_not_zero() {
        let est = estimate_false_positive_rate(&[]);
        assert!(est.rate.is_none()); // the central honesty: silence ≠ zero error
    }

    #[test]
    fn the_rate_is_reported_with_its_humility() {
        use ReExamVerdict::*;
        // 100 cases re-examined; 4 unsound, 6 unknown, 90 sound.
        let mut sample = vec![Sound; 90];
        sample.extend(vec![Unsound; 4]);
        sample.extend(vec![Unknown; 6]);
        let est = estimate_false_positive_rate(&sample);
        assert!((est.rate.unwrap() - 0.04).abs() < 1e-3);
        // The 'Unknown' six are why the note calls 0.04 a LOWER bound, not the answer.
    }
}
```

The conduct record (§6) and its place in a decision are deliberately structured so the *human* decider stays accountable — the data informs, it never decides:

```rust
/// A release recommendation. The conduct evidence is an INPUT; a named human
/// makes — and signs — the decision, and must justify following OR overriding
/// any score (§9). The system never auto-releases and never auto-cages.
#[derive(Debug, Clone, Reflect)]
pub struct ReleaseReview {
    pub conduct: DemonstratedConduct,        // acts over time (§6)
    pub current_danger_assessment: DangerFinding,  // present risk, not permanent status
    pub decided_by: String,                  // the accountable human — attribution again
    pub rationale: String,                   // on the record, reviewable
}

#[derive(Debug, Clone, Copy, PartialEq, Reflect)]
pub enum DangerFinding { DemonstrablyDangerousNow, NoCurrentDangerShown, Insufficient }
```

---

## 9. Risk Assessment, Honestly

Actuarial risk tools are real, they are widely used (bail, parole), and they are neither magic nor evil. The honest position is the calibrated one:

**What they can do:** aggregate base rates better than an unaided human hunch, and *surface* a hunch's bias by making the inputs explicit.

**What they cannot do, and the failures to name without flinching:**
- ⚠️ **They inherit historical bias and launder it as objectivity.** Trained on past *arrest* data, they encode past discriminatory *enforcement* (`README.md` §6) and reproduce it with a number's false neutrality. → Audit for *disparate error rates* across groups (not just disparate scores); a tool that is wrong more often for one group is discriminating, however "neutral" its inputs.
- ⚠️ **They predict a *group* base rate and apply it to an *individual.*** The individual is not the average. → The score is an input a human must justify, never the verdict.
- ⚠️ **They confuse the predictable with the just.** That a factor *predicts* re-arrest (poverty, neighborhood) does not make it *just* to detain someone for it. → Exclude factors that are status, not conduct.
- ⚠️ **They can become unfalsifiable.** Detain on a high score and you never learn if the person would have been fine — the counterfactual is destroyed, so the tool can never be proven wrong. → Measure outcomes for the *released* to keep the tool honest; track its calibration over time.

> **The guardrail, stated as the framework's rule:** a risk score may be an *input* to a human
> decision, presented with its measured error rates and its audited disparities; it may never *be*
> the decision, and the human who decides must record a rationale they would defend in public.
> The moment a score silently decides who stays caged, you have rebuilt the bias of the past with
> the authority of math — and you have done it to people the system already failed once.

---

## 10. The Guardrails: What We Will Not Build

A measurement system for justice is one design error away from a surveillance state, so the refusals are as load-bearing as the features. Stated plainly, in the spirit of being objectively truthful about the danger in our own tools:

| We will build | We will **not** build | Why the line is here |
|---------------|----------------------|----------------------|
| A complete, attributable record of the **state's** conduct | A dossier of a **citizen's** beliefs, associations, or speech | The watched and the watcher are inverted in a just system: power is the most-recorded party |
| Demonstrated-conduct evidence for release (§6) | A "true self" / character / remorse **score** | Conduct is observable and contestable; the soul is neither (§6) |
| Risk as a human-supervised, audited **input** (§9) | Risk as an automated **verdict** | A score that decides launders past bias as objectivity |
| Sampled error-rate estimates with stated uncertainty | A confident "essentially zero error" never measured | Silence is not evidence of honesty (`README.md` §11) |
| Outcome tracking, including exonerations | Permanent records that make the sentence eternal | Collateral consequences are a life sentence served outside (`README.md` §6, Stage 8) |
| Continuous custody-condition monitoring (the floor) | Continuous **thought** monitoring of anyone | The dignity floor protects the body; nothing has standing to mine the mind |

> **The asymmetry is the whole point.** Measure the powerful most, the accused least, and the mind
> not at all. A justice measurement system that drifts the other way — light on the state, heavy on
> the citizen, reaching for the mind — has not improved justice; it has automated the police-state
> failure the founding brief explicitly condemned ("the manipulation of the police... the way they
> treat people in jail"). The instruments in this document point at the system first, because the
> system is the actor with the power, and power is the thing that must be watched.

---

## 11. Integration with the Justice System Flow Analysis

```
THE LEDGER measurements                   Justice System Flow Analysis (README.md)
──────────────────────────               ──────────────────────────────────────────
Sampled Type I error rate ─────────────► Blackstone's ratio CALIBRATED, not asserted (§3)
Detention-days of the dismissed ───────► The JusticeLedger sink, in the open (§2, §10)
Funnel exit REASONS ───────────────────► Correct attrition told from failure (§5)
Case timestamps ───────────────────────► Little's Law backlog L = λW (§5)
Disaggregated stops/force/sentences ───► Discriminatory enforcement stops hiding (§6)
Attributed event record ───────────────► The kept oath; noble-cause/blue-wall deterred (§7, §8)
Demonstrated conduct over time ────────► Evidence-based, conditional release (§9) — NOT soul-reading
Audited risk as supervised input ──────► Pretrial release on risk, not wealth (§6, PRETRIAL.md)
Published exonerations ────────────────► Transparency dashboard; error-found = health (§13)
```

### The critical dependency

From [`README.md`](README.md) §11:

> *"The single most important action is closing the measurement gap before claiming the system
> works... establish the true Type I error rate."*

**THE LEDGER is how that gap is closed** — and how it is closed *without* building the surveillance apparatus that would be the cure worse than the disease. Without it, every number in the master document is `Unverified`, Blackstone's ratio is a slogan rather than a calibrated dial, and the human failure modes (`README.md` §7) operate in the dark where they thrive. With it — and only within the guardrails of §10 — the system runs on measured truth about its own conduct, and an honest justice system is, in the end, also the one a free people can actually consent to, because **the most expensive thing a justice system ever pays for is a lie it believed about a human life — and the second most expensive is the freedom it trades away trying to measure a human soul.**

---

*Document created: May 29, 2026*
*Folder: JUSTICE — measurement/honesty layer for the justice-system flow analysis*
*Conceptual model: exoneration registries, conviction-integrity units, court case-management, body-camera review — rebuilt for truth at scale, after WATER/docs/WELL_METER.md and WAREHOUSE/docs/TRACK_TRACE.md*
*Bounded by a refusal the sibling documents never needed: measure the system's conduct and the person's demonstrated acts; never the citizen's mind.*
*All rates are illustrative or `Unverified` until measured from primary source (master doc §11)*
