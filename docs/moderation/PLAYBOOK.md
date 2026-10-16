# Eustress Moderation Playbook v2.3

**Scope:** every Universe submitted for public listing on the Eustress Gallery.
**Readers:** the people who work the Review Workspace, administrators driving the `moderation_*` tools from the admin route or an MCP session, and anyone who needs to know what the pipeline does with a case.
**Constitution:** `docs/architecture/AI_GUARDIAN_POLICY_v1.0.md` (the Eustress Guardian Code v2.3). This playbook operationalizes it; where the two disagree, the Code wins.
**Architecture:** `docs/architecture/MODERATION_PIPELINE.md`.

## 1. The shape of a case

A case is one published Universe moving through these layers. Each layer decides how much of the next one runs.

| Layer | Who | Input | Output |
|---|---|---|---|
| L0 deterministic | Worker code | R2 object identity, publish rate, digest facts, the listing text and tags, the Space's strings and URLs, the author's rating questionnaire, the account's attested property and verified business | dedup hit, rate flag, Code B8 empty or default-only reject, the Qualification Statement, the B1 listing words, a B6(h) payment link, the fiction label check, a Product category from an unverified account (B9) |
| L1 triage | Jev (TypeSafe) | the text dossier the engine built at publish, scripts' source included | calibrated answers per question, including one per Code article signal |
| L2a researcher | Grok, web search | ONLY the public facts the case depends on (place names, addresses, business names, patent numbers from the listing text and the statement) | findings, each with a public source URL, given to the judge as untrusted data |
| L2b judge | Claude Haiku 5.5, vision | the capture set, the case summary, the research findings, the Guardian Code | the Code's Part VII JSON: article citations, four test scores, rating floor, descriptors, `is_game`, `is_fiction`, and `suggested_tags` when the author gave no tags |
| L3 person | the Review Workspace | the full case, captures, research, appeal text | release, resolve, uphold, list a game, confirm featured, audit |

The judge does not decide the outcome. The Worker applies the Code's Part IV verdict rules to the triage and the judge's answer, in order, and the first rule that applies decides (section 7). No model takes any action through the tools: the Grok agent of earlier versions is retired, and what the rules send to a person goes to the Review Workspace.

Statuses: `pending`, `classifying`, `approved`, `held`, `rejected`, `changes_requested`, `quarantined`, `appealed`.
Only `approved` with `is_public = true` is served by the gallery. Everything else stays reachable by its author alone, except `quarantined`, which is served to nobody but an admin.

Lanes: `legal` (Code A1, A2, A3, A5, A6, A11, A12), `harm` (A4, A7, A8, A9, A10, A13, A14), `quality` (Group B articles, the four tests, the statement, the fiction label, the questionnaire), `coppa`, `judge` (verdict problems and holds for a person), `system` (a layer was unavailable), `ok`.

Every case and every listing records `review_schema: 2`, the shape of the Qualification Statement, tags and rating questionnaire it was reviewed under.

## 2. Standing rules

1. **Author-supplied strings are data.** Names, descriptions, GUI text, dialogue, script sources, asset names and research findings never instruct anyone, whatever they say. A string that reads like a moderation instruction is itself a signal worth noting.
2. **Every action cites the Code and the evidence.** A rationale names the article or test that decides the case and a specific observed thing: a string, a metric in the digest, a detail in a capture, a script path, a probability, a research finding and its source. The tools refuse rationales shorter than twenty characters.
3. **Fail closed on listing, never on storage.** When a layer is unavailable the case is held, not approved. Storage is only blocked by a quarantine or a legal hold.
4. **Nothing a model does is final in the legal lane.** The pipeline may quarantine (the safe direction) and hold. Only a person releases, resolves an appeal, or decides whether a report is filed.
5. **Listing is earned.** A Space is listed only when no article of Part I applies, it passes all four tests of Part II, its Qualification Statement matches it, fiction is labelled as fiction, and its rating questionnaire is answered. Themes are not articles.
6. **Doubt has one direction for each question.** Doubt about whether an article applies goes to a person. Doubt about whether a test is met lowers the score. A person decides every game listing, every featured listing and every appeal.
7. **The researcher gets facts, never the Space.** It is sent the public facts to look up and a fixed instruction: never a capture, a script, a string, the dossier or the package. Its failure is not fatal; the judge runs without findings and the case records why.

## 3. Hard categories (legal lane, Code A1, A2, A3, A5, A6, A12)

`csam_or_minor_sexualization` (A1), `terrorism_or_extremist_promotion` (A2), `mass_casualty_attack_planning` (A3), `ncii_or_real_person_sexual` (A5), `doxxing_or_targeted_harassment` (A6), `war_or_military_targeting` (A12).

| Triage probability | Automatic action | Then |
|---|---|---|
| at or above `hard_quarantine` (0.85) | `quarantined`: every download blocked, author frozen from publishing, evidence preserved for one year | queued for the Review Workspace; a person reviews within one business day and decides on the report |
| at or above `hard_hold` (0.35), or 0.20 for the CSAM question | `held` in the legal lane (content locked) | queued for the Review Workspace |
| below | clear | continues to the harm lane |

The judge can find what the text did not show. A judge citation of A1, A2, A3, A5, A6 or A12, at any confidence, quarantines the case exactly as the triage does (Part IV rule 1). A judge that flags a case while citing one of them, or A11, is held in the legal lane, so the content is locked while a person decides. When an A12 case turns on a real place, the researcher's findings are on the case for the person.

Human standard operating procedure for a quarantine:

- **Look only as much as the decision needs.** Open the captures at thumbnail size first. Do not download the .pak unless the captures and the dossier cannot settle it.
- **Preserve, never delete.** The case sets `legal_hold.preserve_until` one year out. Nothing under a legal hold is deleted, including by the author.
- **Apparent child sexual abuse material:** the designated reporter files with NCMEC's CyberTipline (18 U.S.C. 2258A). Record `reported: true` and the report id on the case. Do not distribute the material further. Counsel is looped in on the first such case and on any uncertainty.
- **Terrorism, attack planning or war targeting:** preserve, escalate to counsel, follow their instruction on law-enforcement referral.
- **Non-consensual intimate imagery of a real person:** a TAKE IT DOWN request is honoured outside this review (`/api/takedown/requests`, src/takedown.mjs). A takedown's hold is lifted only by the takedown review (`/api/admin/takedowns`): `moderation_release` and an overturned appeal are refused while one holds the listing.
- **Doxxing or credible threats:** quarantine stands until the private data or the threat is gone.
- **Release** only with `moderation_release` and a rationale that names why the material is not what the classifier or the judge read it as. Release lifts the author's freeze.

## 4. The harm lane (Code A4, A7, A8, A9, A10, A13, A14)

`real_crime_instructions` (A4), `fraud_or_scam_facilitation` (A7), `self_harm_instructions` (A8), `hate_incitement` (A9); and the place articles `sensitive_real_place` (A10), `crime_rehearsal` (A13), `stalking_private_person` (A14).

The test, from the Code: is this a story, simulation, artwork or critique **about** the theme, or a real-world manual, template, rehearsal or call to action **using** it?

| Condition | Action |
|---|---|
| A4, A7, A8 or A9 at or above `soft_reject` (0.80), `real_world_intent` at or above 0.50 and `needs_human_context` below 0.50 | automatic `rejected` (appealable), with a suggested edit that keeps the fiction and removes the real-world part |
| A4, A7, A8 or A9 at or above `soft_escalate` (0.40), or above 0.80 with fiction-leaning intent or ambiguity | a Group A signal: the judge runs; it cites the article (rule 1) or the case is held for a person (rule 6) |
| A10, A13 or A14 at or above `article_signal` (0.50) | a Group A signal: the researcher looks up the place, then the judge runs; it cites the article or the case is held for a person |
| a judge citation of A4, A7, A8, A9, A13 or A14 at 0.75 or above | `rejected`, harm (rule 1) |
| a judge citation of one of them below 0.75 | `held` (rule 1: an uncertain Group A citation is held, not rejected) |
| a judge citation of A10 | `held` |
| a judge citation of A10, A13 or A14 when the author carries a property-ownership attestation | `held`: a person checks whether the citation is about the attested property |

**Property-ownership attestation.** An author who proved they own, lease or protect a property carries an attestation (`property-attestation:{accountId}`, set by an administrator through `/api/admin/moderation/property-attestation/{accountId}` until its own flow exists). A10, A13 and A14 never apply to the author's own attested property. The judge sees the attestations in the case summary.

Guidance for a person in the gray band:

- A heist in an invented place is fiction, not A13. The same heist in a named real bank, with its cameras and vault, is A13.
- Weapons as props, crime as narrative: not A4. Synthesis steps, quantities, sourcing: reject with a suggested edit. Hard drugs shown at all are B11, whatever A4 says (section 5).
- Hate: depicted-and-condemned is not A9; advocacy is.
- When the reading is genuinely unclear, keep the case held and write down the specific ambiguity. Never resolve ambiguity by rejecting.

## 5. What Eustress does not carry (Code Part I, Group B)

Each Group B article is a bright line: if its words describe the Space, it applies. B1 horror; B2 tragedy as experience; B3 killing as play; B4 brainrot and meme bait; B5 hollow loops; B6 unfair economy; B7 copies; B8 slop; B9 misrepresentation and false teaching; B10 distasteful content; B11 hard drugs. The Code's text is the definition.

| Article | Jev questions |
|---|---|
| B1 | `scripted_startle`, `pursuing_entity`, `horror_framing`, and `fear_level` "frightening" at confidence 0.55 or above |
| B2 | `playable_victims` |
| B3 | `harm_scored`; `lawful_decision_training` marks the case a person decides |
| B4 | `meme_content` |
| B5 | `accumulation_loop`, `return_reward` |
| B6 | one per sub-item: `b6a_paid_progress`, `b6b_paid_chance`, `b6c_paid_skip`, `b6d_unasked_prompt`, `b6e_sales_pressure`, `b6f_hidden_prices`, `b6g_social_reach_reward`, `b6h_offsite_payment` |
| B7 | `copied_signal` |
| B8 | `repeated_single_asset`, `generated_without_authorship` |
| B9 | `false_teaching`; `tag_fit` and `tag_content` for the author's own tags |
| B10 | `distasteful` |
| B11 | `hard_drug_depiction`; `drug_education` is its exception |

A Jev answer at `article_system_cite` (0.85) is a **system citation**: it decides under rule 2 whatever the judge says, and only a person can clear it. A Jev answer at `article_signal` (0.50) is an **article signal**: the judge must cite the article, or the case is held for a person (rule 6). The judge's own Group B citation decides at `article_judge_cite` (0.75); below it, the case is held.

**B6(h), payment off Eustress.** Allowed only through a verified business connection registered with Eustress (`verified-business:{accountId}` with a `payment_connection`), and never in an Adult listing. With such a connection on a listing that is not Adult, Jev's `b6h_offsite_payment` answer is recorded and not cited; if the rating turns out `adult_18` or `adult_21`, it is cited after all.

**B11, hard drugs.** No hard drugs, ever: the use, sale, making or glamorising of opioids outside medical care, cocaine or crack, methamphetamine, or cannabis. It decides under rule 2 like any rejecting Group B article and is never a feature to remove: a Space whose subject is hard drugs is rejected, and a single hard-drug prop in a Space that otherwise qualifies is still B11. Its exception is teaching that saves lives: medical care (a nurse giving prescribed morphine), overdose first aid (naloxone), and the science of addiction, its treatment and its prevention, taught accurately, rated `mature_17`. When Jev's `drug_education` answer is at `substance_rating` (0.60), it lowers Jev's B11 answer one step: a system citation becomes a signal the judge or a person decides, and a signal is dropped. The judge's own B11 citation is never lowered. Psychedelics (psilocybin, LSD, DMT, mescaline, MDMA, ketamine) are not B11; showing their use is rated `adult_21`. An author who answers the questionnaire's drugs question with hard drugs is told at once that such a Space cannot be listed, and the answer is a B11 signal. A raw payment or donation link, a cryptocurrency address or an unlock code sold off Eustress is always a B6 system citation, whatever the account.

Deterministic findings:

- **B1(c):** the listing's name, description or tags contain horror, scary, creepy, haunted, jumpscare or jump scare. An article signal.
- **B6(h):** a payment or donation link (ko-fi.com, patreon.com, paypal.me, buymeacoffee.com, cash.app, venmo.com), a Bitcoin or Ethereum address, or an unlock code offered for money. A system citation.
- **B8(a):** an entity count of zero, or default material, colour and name on every part with no scripts and no text. Rejected at triage.
- **B8 and B7 by band:** Jev's `quality_band` at `empty_or_broken` or `test_or_scratch_junk`, with confidence at or above 0.70 and `spatial_intent` below 0.30, is rejected under B8; at `unmodified_template_or_asset_flip` it is cited under B7 and held for a person. A shakier band, mass-produced filler, or a publish rate past the daily limit is a B8 (or B7) signal.
- **B9, Product:** the Category line says Product and the account has no verified business. A wording fault: the change is the category.

**B9, the author's tags.** Jev answers two questions about tags the author chose: `tag_fit` (do the tags describe what the Space contains?) and `tag_content` (is any tag a slur, an insult aimed at a person or group, or personal information about a real person?). A `tag_fit` of "no" at `article_system_cite` (0.85), or a `tag_content` of "yes" at 0.85, is a B9 system citation and a wording fault: **not yet**, "Fix your tags" (`not_yet_tags`). Neither is read when the review chooses the tags. Below 0.85 they decide nothing: tags are for finding a Space, and they never set its rating. A phone number, an email address or a handle is refused as a tag before any review (src/tags.mjs).

What a citation does (rule 2): B7, and B3 training whose score is a lawful decision, are held for a person. B5, B6 and a B9 wording fault (a mislabel, a questionnaire answer the evidence disproves, a missing fiction label, the author's tags), when every test is 2 or above, are **not yet**: the author is told the feature to remove or the wording to fix. When a test is `null`, they are held for a person. Every other citation is rejected, citing the article. Jev's `false_teaching` is never a wording fault.

## 6. The four tests, the statement, fiction and the questionnaire (Code Parts II, III and V)

The judge scores each test 0 to 3, or `null` when the captures and Jev's answers cannot decide it: **T1 build quality**, **T2 objective**, **T3 lesson**, **T4 real-world effect** (`build_quality`, `objective`, `lesson`, `real_world`). For fiction, T4 is **T4 for Fiction**: the idea the Space carries and its label.

Every listing carries a **Qualification Statement**: four labelled lines in its description.

```
Category: Digital twin | Simulation | STEM lab | Training | Learning | Tool | Cooperative challenge | Game | Concept or prototype | Product | Fiction | Adult
Objective: one sentence
Lesson: one sentence
Real-world link: one sentence
```

The Worker extracts it from the whole description. It is missing when any line is absent or the category is not one of the twelve. The judge reports `statement_check`: `matches`, `missing` or `contradicted`.

**Fiction.** A Space is fiction when its Category is Fiction, the judge sets `is_fiction`, or Jev's `is_fiction` is at or above 0.60. It is labelled when its tags carry `fiction` and at least one genre tag (`science-fiction`, `fantasy` and the other genre tags the Worker names, or any tag ending in `-fiction` or `-fi`). Unlabelled fiction gets **not yet**: the change is to label it. While the label is missing, a T4 for Fiction of 0 does not reject the Space; the label is asked for first.

**Tags the review chooses.** Tags are a listing's only categories. When the author gives none (or the ones the listing has are the review's own), the judge chooses up to 5 (`suggested_tags`). The Worker normalizes each like an author's tag and drops what is not a valid tag, personal information included. For fiction it puts `fiction` first and keeps the judge's first genre tag, and the fiction label is checked on the chosen tags, after they are assigned. They are written to the listing with `tags_source: "judge"` when the case is decided, for every outcome except a quarantine, a harm (Group A) rejection and any other legal-lane outcome, and the author is told in one line, "Tags chosen for your Space: a, b, c", in the review email and the case history. The author may send them back as they are (they stay the judge's, and nothing is reviewed again) or change them (they become the author's, and the listing is reviewed again).

**The rating questionnaire.** The author answers ten questions about what the Space contains (violence, blood, language, romance and sexual material, nudity, alcohol and tobacco, drugs, games of chance, purchases, players together) through `POST /api/simulations/{id}/rating-questionnaire`. Each answer maps to a band and to the Code's content descriptors: alcohol or tobacco is `adult_21` with the `alcohol` or `tobacco` descriptor; psychedelic use shown is `adult_21` with `psychedelics`; drugs only as life-saving teaching is `mature_17`; hard drugs name B11, and the route answers with a notice that such a Space cannot be listed. Unanswered, the listing gets **not yet**.

## 7. The decision table (Code Part IV)

`T` is the triage outcome. When the triage leaves the case to the judge (`judge`, or `escalate` for a gray band or a Group A signal), the researcher runs when the case turns on public facts, then the judge, then these rules in order; the first that applies decides.

| T | Rule | Condition | Result |
|---|---|---|---|
| quarantine | | (judge not run) | `quarantined`, Review Workspace |
| hold | | (judge not run) | `held`, Review Workspace |
| reject | | (judge not run) | `rejected`, appealable |
| changes_requested | | (judge not run) | `changes_requested`, author (COPPA) |
| judge | 0 | malformed (including a refusal, an answer cut off at max_tokens, no ANTHROPIC_API_KEY), `flag_for_human_review`, `flagged_for_review`, or confidence below 0.60 | `held` (legal lane when it cites A1, A2, A3, A5, A6, A11 or A12) |
| judge | 1 | A1, A2, A3, A5, A6 or A12 cited | `quarantined` |
| judge | 1 | A10, A13 or A14 cited, and the author has attested property | `held` |
| judge | 1 | A4, A7, A8, A9, A13 or A14 cited at 0.75 or above | `rejected`, harm |
| judge | 1 | one of those below 0.75; A10 or A11 cited | `held` |
| judge | 2 | a system citation, or a Group B citation at 0.75 or above | B7 or lawful-decision B3: `held`; B5, B6 or a B9 wording fault with every test 2 or above: `changes_requested`; with a test `null`: `held`; otherwise `rejected` |
| judge | 3 | `statement_check` is `contradicted` | `rejected` under B9 |
| judge | 4 | any test at 0, or T4 at 1 with the statement present | `rejected`, naming the test |
| judge | 5 | two or more tests at 1 | `rejected`, naming them |
| judge | 6 | a Group B citation below 0.75; an article signal the judge did not cite; a test scored `null`; scripts Jev could not read in full; judge confidence below 0.75 | `held` |
| judge | 7 | exactly one of T1, T2, T3 at 1, the statement missing, the fiction label missing, or the questionnaire unanswered | `changes_requested`, naming each change; a test at 1 with no suggested edit is `held` |
| judge | 8 | every test at 2 or above, the statement matches, the questionnaire answered | a game: `held` (`game_needs_a_person`); otherwise `approved` |

**The rating.** The highest of three: the questionnaire's band, Jev's rating, and the judge's `rating_floor`, in Eustress's own bands `all_ages`, `ages_10`, `teen_13`, `mature_17`, `adult_18`, `adult_21`. Jev's rating is its `content_rating`, raised to the floor its drug and substance answers set at `substance_rating` (0.60): `drug_education` to `mature_17`, `psychedelic_use` and `alcohol_tobacco` to `adult_21`. The listing also records the descriptors of the questionnaire and the judge together.

**Games (Code Part VI).** A case is a game when Jev's `content_kind` is `interactive_experience`, the judge sets `is_game`, or Jev's `game_mechanics` is at or above 0.60. A person decides every game listing.

**Featured (rule 9).** Never set automatically. When the judge proposes it and every test is 2 or above, with T1, T3 and T4 at 3, and the rating is below `adult_18`, the case records `featured_candidate` for a person, who sets `featured` through `moderation_approve`.

**Held reasons a person sees.** `judge_flagged`, `judge_refusal`, `judge_max_tokens` and the judge's malformed codes (rule 0); A10, A11 and uncertain or attested-property Group A citations (rule 1); `code_B7`, a lawful-decision `code_B3`, and a removable article with an unscored test (rule 2); `article_unsure`, `article_signal_not_cited`, `test_not_scored`, `scripts_not_read_in_full` and `judge_unsure_to_list` (rule 6); `judge_not_yet_without_change` (rule 7); `game_needs_a_person` (rule 8); `judge_statement_inconsistent`; `review_limit`. The author of a held case is told only that a person is reviewing it.

## 8. The Review Workspace

"Held for a person" means the case goes to the Review Workspace: a published Space where an assigned person works the case. Its configuration is `workspace:review` (`{ space_id, members }`), set by an administrator with `PUT /api/admin/moderation/workspace`. Every hold, every quarantine, every appeal, and every approval and rejection sampled for audit is queued there, and every member is told in their notification feed and by email. `GET /api/admin/moderation/workspace/queue` (the `moderation_workspace_queue` tool) lists what is waiting; an entry leaves the queue when its case is decided.

The tools a person works with:

- Read: `moderation_get_case`, `moderation_list_queue`, `moderation_workspace_queue`.
- Decide: `moderation_approve`, `moderation_reject`, `moderation_hold`, `moderation_request_changes`, `moderation_set_rating`, `moderation_quarantine`, `moderation_escalate_legal`, `moderation_author_notice`.
- A person only: `moderation_release`, `moderation_resolve_appeal`, `moderation_audit_resolve`, `moderation_rerun`, `moderation_backfill`, `moderation_bind`, `moderation_binding_report`.
- Forum posts, a person only: `moderation_forum_case` (read) and `moderation_forum_resolve` (decide).

**Forum posts.** The Eustress Forums post first and block only what is illegal, and no person is asked to read illegal content. The text check refuses a forum title or post for `csam`, `nonconsensual_sexual`, `personal_info` or `threat` from a 0.50 answer (band `borderline` below 0.85, `high` from 0.85; only `high` is ever reported to an authority) and for `scam` or `spam` from 0.85; it never holds forum text for a person. A report cites one category: `csam`, `nonconsensual_sexual`, `personal_info`, `threat`, `scam`, `spam` or `adult_sexual`. Every reported post is a forum case (`forumcase:{postId}`, with a case reference) queued in the Review Workspace as kind `forum`, as a record readable without opening the post: the reports, and a sealed evidence pointer (the post id, an HMAC-SHA256 of its body under a key derived from the Worker's secret, the times), never the body. Members are told at the first report and again when the post is hidden, removed or gated. A post reported as `csam` or `nonconsensual_sexual` is hidden at the first report, and a `csam` report also goes to staff; one reported by 3 distinct accounts (accounts younger than 24 hours are recorded and not counted) as `csam`, `nonconsensual_sexual`, `personal_info`, `threat` or `scam` is removed; `spam` is never hidden by reports and a person reviews it; 3 distinct reports of `adult_sexual` put the post behind the Adult content gate. Hate, harassment, insults and criticism of Eustress are not categories, and no report hides a post for them. `moderation_forum_resolve` decides `restore` (the appeal path for a post hidden or removed wrongly), `remove`, `gate` or `ungate`; `remove` needs one of `csam`, `nonconsensual_sexual`, `personal_info`, `threat`, `scam` or `spam`, or a TAKE IT DOWN lock on the post, and `restore` is refused while that lock holds it. A reporter's note is for the reviewer only; the post's author never sees a reporter or a note. While any report on a post cites `csam`, its author is told nothing (no case history entry, no notice) until a person restores the post or removes it under another category, so a report to NCMEC is never preceded by a warning.

`moderation_approve` refuses anyone but a person any case with a hard category at or above its hold floor, any legal-lane triage, any quarantine, any game, any case without a valid judge verdict carrying spatial evidence, any case the Part IV rules would not list, and any request to set `featured`. `moderation_reject` cites the Code: lane `harm` with A4, A7, A8, A9, A13 or A14, or lane `quality` with a Group B article or a test.

## 9. Rating and child safety

- `adult_18` and `adult_21` listings are adult content: shown only to accounts that have turned Adult content on in their Profile, and never featured. An `adult_21` listing (alcohol, tobacco, psychedelic use) also needs a verified age of 21 or over, worked out from the identity document's date of birth each time it is shown, so an account sees it from its 21st birthday. A listing with no rating reads as `adult_21`. Guest play is `all_ages` only.
- `child_directed` (Jev at or above 0.60) means the COPPA "directed to children" factors apply. A child-directed experience with off-platform links or contact details, personal-data collection, chance-based or real-money mechanics, or a rating above `all_ages`, gets `changes_requested` with that list.
- Child-directed and teen-appeal listings never feed targeted advertising or engagement notifications; moderation records the flags, product enforces them.

## 10. Appeals

An author may appeal `rejected` and `changes_requested` once at a time, with 10 to 2000 characters of text. An appeal asks one question: was the article or test applied correctly to this evidence?

- Every appeal goes to the Review Workspace, and a person decides it against the Code's exact text. No model reviews an appeal.
- A person closes it with `moderation_resolve_appeal` as `overturned` (lists it) or `upheld` (returns it to the prior status). The decision is recorded under the case's reference, which never names the reviewer. Every overturn is a false rejection, used to tune the thresholds, the question wording and the Code.

## 11. Audits

A sample of automatic approvals and of automatic rejections (`audit_rate`, 3%) is queued for the Review Workspace. A person reads the case, and closes the audit with `moderation_audit_resolve`; a person who disagrees also holds or rejects the case, or lists it on appeal. The comparison is what calibrates the pipeline.

The shadow judge is the other calibration: with `JUDGE_SHADOW_MODEL` set, a second model judges the same input, decides nothing, and `GET /api/admin/moderation/shadow` reports how often it agrees with the judge, by outcome.

## 12. Backfill

Listings that predate the gate carry no case. The nightly sweep classifies a bounded number, oldest first. With a single-angle thumbnail as the only capture, the judge's confidence is lower by design; expect more of these to be held.

## 13. What this playbook does not do

It never sends a report to any authority, never deletes an object, never touches an account beyond the publish freeze, and never runs code from a Universe. Each of those is a person's act, recorded on the case.
