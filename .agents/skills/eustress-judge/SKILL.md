---
name: eustress-judge
description: >-
  Emulates the Eustress listing review (the Guardian Code judge) so a creator knows what
  Eustress lists before they build, and can check a Space before they publish. Use when
  someone asks "what is allowed on Eustress", "will this pass review", "why was my listing
  not listed", "what does T3 or T4 mean", "write my Qualification Statement", "check my
  Space before I publish", "run the judge on this", or is starting a new Space and wants
  it to be listable. At the start of a project it explains the Code in plain words and
  shapes the idea so it can pass; before publishing it reads the Universe the way the
  real review does, scores it against the Code, runs the live verdict rules, and shows
  the same findings Studio's Update Universe dialog would (title, Why, What to change).
  It is an emulation: the real review decides, and passing it is not approval.
---

# Eustress judge, emulated

Every public Eustress listing is reviewed automatically against the **Eustress Guardian
Code** before it appears in the Gallery. This skill lets a creator meet that review early:
before building (so the idea is listable) and before publishing (so the first real review
is not a surprise).

**Say this to the creator every time you give a verdict:** this is an emulation. The real
review decides. Passing here is not approval, and a finding here is not a rejection.

## 0. Ground truth, never memory

The skill carries the judge's own rules, generated from the files the live review runs on:

| File | What it is | Where it comes from |
|---|---|---|
| `references/code.md` | The whole Guardian Code, word for word, exactly as the judge receives it | `docs/architecture/AI_GUARDIAN_POLICY_v1.0.md` |
| `references/findings.json` | Every finding Studio can show (title, why, what to change), the statement categories, the fiction tags, the tag rules | `infrastructure/cloudflare/api/src/moderation.mjs` |
| `scripts/verdict.mjs` | Runs the **live** verdict rules (`moderation.mjs`) on your scores and prints Studio's findings | imports the repository's rules; no copy |

**In Studio, without this repository:** the Eustress MCP carries this skill as three tools.
`judge_guide` gives the Code in plain words, the review steps, the statement format, or any
article word for word (`topic`: overview, review, statement, code, or an id such as B3, T4,
Part IX). `judge_check_listing` runs the deterministic listing checks (statement lines,
category, fiction label, tags, lengths, horror words, links, crypto addresses) on the saved
listing or on text you pass. `judge_verdict` takes your Part VII judge answer and the listing
and returns the findings Studio would show, from the deployed review's own rules
(`POST /api/moderation/emulate`). Use them in that order: at the start of a project, while
building, and before publishing.

**Read `references/code.md` in full before you explain a rule or score a Space.** Quote it
when a creator asks why. Never paraphrase a rule from memory, and never invent one: if the
Code does not say it, it is not a rule. Check `policy_version` in `findings.json` and name
it (for example "Guardian Code v2.7") in what you tell the creator.

## 1. Which mode

- **Starting out** ("I want to make X", "what is allowed", "will a game about X pass"):
  section 2, then section 3.
- **Before publishing** ("check my Space", "will this pass", "run the judge"): section 4.
- **After a real review** ("why was it not listed", "what does this finding mean"): read
  `<Universe>/.eustress/review.toml`, which holds the last real review Studio received,
  explain each finding from `findings.json` and the Code, then help with section 4.

## 2. The Code in plain words

Explain this in the creator's terms, then point at the part of `references/code.md` it
comes from. Keep the Code's own article and test names (T3, B5, Part III): they are what
Studio's findings will cite.

**What Eustress lists (Part 0).** Eustress is for simulation, digital twins and STEM. A
listed Space does something for real life: it teaches a skill that transfers, models a
real place or system, tests an idea about something real, or helps solve a real problem.
Labelled fiction is listed too when it carries an idea worth the visitor's time.
Entertainment with nothing behind it is not listed. The author carries the burden of proof:
nothing is listed just because nothing is wrong with it.

**The four tests (Part II).** Each is scored 0 to 3. A listing needs **every test at 2 or
above**.

| Test | The question | What earns a 2 |
|---|---|---|
| T1 Build quality | Is it built deliberately for its purpose? | Deliberate and sound: coherent scale, working mechanics, nothing broken or placeholder. Plain blocks are fine when plainness serves the purpose. |
| T2 Objective | Can a visitor tell what it is for and do it? | The Space gives the controls, readouts, route or story. "Survive", "collect more", "have fun" are not objectives. A 3 shows the visitor's result against a standard or reality. |
| T3 Lesson | What can a visitor learn or practise, named specifically? | The skill, concept, fact or method is **in the Space**, not only in the description. For a Tool, what it lets a person do; for Fiction, the idea the story carries; for a game, the skill its play exercises (a simple skill game such as Pong practises reaction time and spatial awareness: a 2, named or not). |
| T4 Real-world effect | How does time here connect to real life? | A real connection, present: (a) a real place with named source data, (b) a real system the scripts compute, (c) measured data from a named source, (d) a real skill with real decisions and real consequences, (e) a real problem, (f) real knowledge taught accurately, (g) a verified business's product, (h) real use: a working tool the visitor brings their own input to, (i) a real skill through play: reaction time, coordination, spatial awareness, timing, memory or logic (Pong is a 2; a 3 measures the skill against a standard). Theme or decoration alone is a 1. Fiction is scored on its idea and its label, or by (i) when that is higher. |

How tests decide (Part IV): any test at **0 is not listed**; **two or more at 1 is not
listed**; **exactly one at 1 is "changes requested"** naming that change. Unlabelled
fiction is the exception the creator should know: its T4 for Fiction is 0, but the outcome
is "label it", not a rejection.

**The Qualification Statement (Part III).** Four labelled lines in the listing
description, each on its own line. The review extracts them by machine, so the format is
exact:

```
Category: <one of the list below>
Objective: one sentence
Lesson: one sentence
Real-world link: one sentence
```

Category is exactly one of: Digital twin, Simulation, STEM lab, Training, Learning, Tool,
Cooperative challenge, Game, Concept or prototype, Product, Fiction, Adult (the list in
`findings.json` `statement.categories` is authoritative). Product needs a business
Eustress has verified. A missing line means "Qualification Statement missing"; a line the
Space disproves is B9 and is **not listed**. The statement is shown with the listing, so
write it for visitors.

**Fiction and genre labels (Part III).** Fiction (science fiction, fantasy, any story) is
listed when it is labelled: the tag `fiction` plus at least one genre tag such as
`science-fiction` or `fantasy` (the full list is `findings.json` `fiction.genre_tags`; any
tag ending in `-fiction` or `-fi` also counts), and Category: Fiction. The review decides
whether a Space is fiction from what it is (`is_fiction` in Part VII), not only from what
the listing calls it.

**Title, description and tags.** There is no title length rule; the rules are honesty and
the bright lines:
- Say what it is (B9): the title, description, thumbnail, tags and statement must not
  claim anything the Space does not show. A concept that states its assumptions is fine.
- No meme or brainrot names, phrases or characters in the title, tags or GUI (B4).
- Selling it as horror or a jumpscare game is itself B1. "Spooky", "creepy" and "haunted" are mild fright and are fine.
- Tags: at most 8, lowercase letters, digits and single hyphens, describing what the Space
  contains. Never a slur, an insult, a phone number, an email or a handle. Leave them
  empty and the review chooses up to 5.
- Name a real place, business or patent only when the Space really models it: names in
  the listing are looked up.
- Every file the description says ships (for example "the generator ships in
  `src/generator`") must be in the package.
- Links (Part IX): a URL in the description or on the page is shown only after the link
  review approves it ("[link pending review]" until then); one the review removes is
  rewritten to "[link removed]" and the author is told why. Use plain https:// links to the
  destination itself (no shorteners, redirects, IP addresses or ports). Allowed: real
  businesses and stores, engineering and maker shops, licensed medical providers,
  pharmacies that require a prescription, legal gun stores, research, creators' pages,
  socials, docs and press. Removed: anything black market or risky (Part IX's flags).
  Link labels are plain text with no URL.

**What is refused (Part I).** Group A is real-world harm and the law: child sexual content,
terrorism, attack planning, real crime instructions, non-consensual sexual content of real
people, doxxing and threats, fraud templates, self-harm instructions, hate incitement,
security detail of sensitive real places, legal removals, military targeting, crime
rehearsal on a real place, stalking a private person. A story or simulation *about* a
harmful subject is not Group A; real instruction, targeting or rehearsal is.

Group B is what Eustress does not carry, however well built. The ones a game hits most:
- **B1 Fright beyond mild**: mild fright is fine (spooky places, eerie sound, monsters and zombies the visitor can fight or outrun, an occasional startle). Cited: startles the visitor did not choose more than once in any two minutes, or as the main thing; a pursuer built to terrify that the visitor can neither fight nor escape; a listing sold as horror or a jumpscare game; gore or mutilated bodies as decoration.
- **B3 Combat beyond its limits**: combat (scoring by hitting, knocking out or killing characters) is allowed in **fiction** (invented people, factions, creatures and events) or **history** (a real conflict that ended at least 25 years ago, between its combatants, portrayed as it was), and never against real life. It is cited when any of these fails:
  - no real, identifiable person (public figures included), real protected group, real organisation or present-day real place as a target;
  - no attack on civilians, children, a school, a place of worship, a hospital or a crowd, even in fiction; no mass shooting, bombing, assassination or terror attack as play;
  - combatants only: nothing gained from harming a child, a civilian, a captive or a surrendered or helpless character; no torture or execution;
  - no excessive gore: blood, hit effects and brief stylised pieces from a kill are fine (rated); realistic dismemberment or mutilation lingered on, body parts or remains left in the scene, harm to bodies after death, or gore as the point are not.
  Training set in the real present is scored on the lawful decision, never on hits.
- **Combat also brings**: no loot boxes, crates, cases, chests or spins at all, bought **or earned**, and no betting (B6(b)); a rating of at least teen_13, and at least mature_17 for realistic violence or blood (Part V); and the four tests as usual (fiction labelled and scored on its idea; history taught accurately).
- **B4 Meme bait**: meme or brainrot characters, names, sounds, images.
- **B5 Hollow loops**: main progress that grows only by clicking, a repeat that cannot fail, rebirth or prestige; daily or login rewards, streaks, timers that gate progress, refilling energy. Passive income (a generator, a shop, an investment the visitor built or chose, earning while they play or are away) is fine.
- **B6 Unfair economy**: buying progress, power or chance; paid skips; unasked purchase prompts; countdown sales; hidden prices; rewards for inviting; a cryptocurrency address or an unlock code sold off Eustress. Since v2.8, off-site links (a website, socials, shop, payment and donation pages) are allowed once the link review passes them (Part IX); a payment link inside the Space must be the same URL as an approved link on the listing's page, otherwise it is "changes requested: add it to your Website links, or remove it". No links on a listing made for children; no shop or payment link on an Adult listing.
- **B7 Copies** and **B8 Slop**: clones, asset flips, untouched templates; empty, default-only or generator output with nothing authored.
- **B9 Misrepresentation**, **B2 Tragedy as play**, **B10 Distasteful content**, **B11 Hard drugs** (no exceptions beyond medical care, first aid and accurate prevention).

B5, B6, a B9 wording fault and lawful-decision training under B3 are "changes requested"
when the rest of the Space passes: remove the feature, fix the wording. The others are
not listed.

**Ratings (Part V).** The review sets the age band from what it sees (all_ages up to
adult_21). Adult content is off for every account until it is turned on and the age is
verified.

## 3. Shape the idea so it can pass

Work through these with the creator before anything is built. Show your answers.

1. **Pick the Category** that is true of the idea.
2. **Write the four statement lines first.** If the Lesson or Real-world link cannot be
   written honestly, the idea is not yet listable: change the idea, not the wording.
3. **For each test, name what in the Space will earn a 2**: the object, the script, the
   readout, the on-screen text. "The description explains it" never earns T3.
4. **Walk the Group B list** against the mechanics: scoring, progression, economy,
   enemies, timers, monetisation.
5. **Decide on fiction.** If it is a story or a game with no real connection, label it
   (`fiction` plus a genre tag, Category: Fiction) and make its idea present in the Space.
6. **Plan the captures.** The review sees four renders of the open Space at rest, in Edit
   mode, plus every script. A HUD does not show; what the Space does should be visible
   at rest (a worked example, a labelled board) and clear from the scripts.

Turning a plain game into a listable one usually means adding a real connection the
visitor practises and can get wrong, measured and shown: a ball sport with real rules and
physics and a readout against the real standard, a driving course scored on real traffic
rules. Offer two or three such directions, each with its four statement lines.

## 4. Before publishing: emulate the review

Never open, unpack or run a published `.echk` package. Read the Universe's source folders.

**Step 1. Find what ships.** A publish ships the **whole Universe**: every Space under
`<Universe>/Spaces/`, not only the open one. List every Space. A second Space the creator
forgot is reviewed too, and too many scripts across Spaces is its own finding ("publish
fewer Spaces per Universe").

**Step 2. Get the listing** exactly as it will be in the Update Universe dialog: title,
description, tags (empty means the review chooses). Tags live only in the dialog, so ask.
For the title and description, in this order:
- the creator's new text, if they are changing it;
- `listing.name` and `listing.description` in `<Universe>/.eustress/moderation-dossier.json`,
  which is byte for byte what Studio sent at the last publish;
- `[listing]` in `<Universe>/.eustress/publish.toml`, read with a real TOML parser. Its
  line ends are written as `\r` escapes; copied by hand they become a literal backslash
  and `r`, the Category line no longer matches, and the statement reads as missing.

Write it to a file:

```json
{ "name": "Title", "description": "Full description, statement lines included", "tags": [] }
```

`verdict.mjs` prints the statement it extracted; if it says a line is missing that you can
see, check the description's line ends first.

**Step 3. Read the evidence the judge reads** (Code, "Spatial Intelligence Review"):
- **Every script in every Space, in full**: `.luau`, `.lua`, `.rune`, `.rn`, `.soul`, and
  the source inside SoulScript folders. Note what each does, what it contacts, what it
  collects, and whether it does what the listing claims.
- **Supporting files**: every `.py`, `.json`, `.md`, `.txt`, `.csv` up to 1 MB.
- **The scene**: the Workspace's parts, names, materials and variety; GUI and billboard text.
- **Captures**: when Studio is open and the Eustress MCP is connected, take four views of
  the Space at rest (`ai_camera_set_pose`, then `ai_camera_capture` twice: the first
  returns the previous shot). Otherwise use `<Universe>/.eustress/moderation/capture-*.png`
  from the last publish and say they may be stale.
- **The dossier**: `<Universe>/.eustress/moderation-dossier.json` from the last publish
  holds the digest (part counts, default fractions, strings, links) the judge saw. Its
  digest covers only the Space that was open, and its scripts are excerpts that may be
  cut short: read the scripts from the Space folders. Compare its `generated_at` with the
  files' modified times; anything changed since is not what the dossier describes.

**More than one Space.** The Code judges the package: other Spaces "from their scripts and
the digest". An article that applies in any Space that ships applies to the listing.
Score the four tests on the listing as a whole, the Space the title and captures present,
and name every other Space in `suggested_edit_to_publish`. The real review's attention to
a second Space has varied (see section 5), so the strongest advice is always one listing,
one Space: move the others to their own Universe.

Do **not** read `<Universe>/.eustress/review.toml` yet. It is the last real review; reading
it first anchors your scores. Compare in Step 6.

**Step 4. Judge.** Write the judge's answer, the JSON object of the Code's **Part VII**, to
a file. Follow Part I, Part II and Part VII exactly:
- Cite every Part I article that applies, each with a confidence and specific evidence.
  Each article is a bright line: cite it when its words describe the Space, not when you
  only wonder. A citation decides by itself: a Group B article at 0.75 or above is "not
  listed" (or, for B5, B6 and a B9 wording fault, "changes requested"), and **any Group B
  citation below 0.75 is "changes requested" on its own** (rule 6). So a passing doubt
  cited at 0.6 turns a listed Space into a changes-requested one. If you cannot point at
  the words of the article that the Space meets, do not cite it; mention the doubt in
  your report instead. A tag misdescribes the Space (B9) only when it claims a topic,
  genre or audience the Space does not have.
- Score every test 0 to 3 (or `null` with what to show), each with specific evidence: a
  script path and what its lines do, a named part, a string, a capture. Evidence that
  could be written without looking is not evidence.
- `statement_check`: `missing` when any line is absent, `contradicted` when the Space
  disproves a line, `matches` otherwise.
- `is_fiction` from what the Space is. `suggested_tags` only when the creator gave none.
  `suggested_statement` when the statement is missing or does not match.
  `suggested_edit_to_publish`: the specific change, written to the creator.
- `confidence` honest: below 0.75 is itself "changes requested".
- `policy_version` from `findings.json`; `policy_hash` is `findings.json` `code_sha256`
  (the emulator does not check it).
- Score what is there, then let the rules decide. Never work back from the outcome you expect.

**Step 5. Run the verdict rules.**

```bash
node <this skill>/scripts/verdict.mjs --listing listing.json --judge judge.json --dossier "<Universe>/.eustress/moderation-dossier.json"
```

It validates your answer the way the live review does (a malformed answer exits 2 with the
reason: fix it and run again), applies the live Part IV rules and prints the findings as
Studio would show them, plus the statement it extracted and the rule trail. Add `--json`
to see everything the rules read. Two differences from the live run: your answer is
final (the live review asks an unsure judge a second time; here an unsure answer goes
straight through the rules, as that second reading does), and there is no text
classifier, so only your own citations count. It needs the Eustress repository on disk; set
`EUSTRESS_REPO` if the skill is installed elsewhere. If it exits 3 (no repository), apply
Part IV from `references/code.md` by hand, in order, the first rule that applies deciding,
and render each finding from `findings.json` `author_copy` (keys: `test_<test>`,
`confirm_<test>`, `statement_missing`, `fiction_label_missing`, `code_<article>`,
`not_yet_<article>`, `statement_contradicted`, `review_unconfirmed`). Two traps when doing
it by hand: unlabelled fiction with T4 for Fiction at 0 is "changes requested" with
`fiction_label_missing`, not a rule 4 rejection; and "changes requested" names **every**
change at once (one test at 1, the missing statement, the fiction label).

**Step 6. Report.** Give the creator, in this order:
1. The banner: emulated, Guardian Code version, the real review decides.
2. The likely outcome: Listed, Changes requested, or Not listed.
3. The findings exactly as `verdict.mjs` printed them (title, Why, What to change,
   Suggested edit, suggested statement, chosen tags).
4. What to do next, in plain words, most important first.
5. How sure the emulation is, and why: what it could not see (captures it did not take,
   the text classifier and the real-place research it does not run, scripts it could not
   read).
6. Then, if `<Universe>/.eustress/review.toml` exists, read it and compare: same outcome?
   same findings? Say plainly where they differ.

## 5. Limits of the emulation

- The real review is a model too (the judge, a text classifier, a researcher for real
  places and businesses, a script reader). Its scores vary run to run near a boundary; a
  test you score 2 it may score 1. Say so when a test is borderline.
- The emulator cannot run the classifier or the research. A real place, a business, a
  patent or anything near Group A: tell the creator the real review looks harder there.
- Captures you take are not the captures the publish takes. The review never scores a
  behaviour absent because a still image does not show it, and neither should you.
- Legal-lane outcomes (held for a person, removed) are never predicted as certain. If
  something might be Group A, say so plainly and point at the article; do not soften it.
- Fiction or not is a judgement call for a plain game: the real review has read a plain
  arcade game as fiction (so its T4 was "label your fiction", not a rejection). When you
  are unsure, tell the creator both outcomes.

**Calibration: real reviews.** Use these to check your scoring, never to copy it. Both were
decided under Code v2.5. Since then v2.6 allowed combat in fiction and history and v2.7
allowed mild fright, gore that is not excessive, passive income and skill games: under v2.7
the zombie Space below is no longer B1, B3 or B5 on what it ships (brief gibs and blood, an
occasional boss roar, generator income), and Pong's play is scored as practising reaction
time and spatial awareness (T3 and T4 at 2).

| Listing | What shipped | Real review (Code v2.5) |
|---|---|---|
| Pong, 2026-11-14 | "A simple game of pong.", no tags, no statement; the Universe also shipped a second Space, a zombie wave shooter | Changes requested: Lesson (T3 at 1), Qualification Statement missing, Label your fiction; the suggested edit said T4 was 0 and named the second Space. No Group B article was cited for the second Space. |
| The Super Station, 2026-11-13 | Category Tool, all four statement lines, tags eustress, simulation, education, tool; two scripts and the generator shipped | Listed, all ages: every test at 2, statement matching, no article cited (the tag `simulation` was not treated as a fault). |

Run blind, the emulator agreed on every test it shared with these reviews and on both
statements, and was stricter in two places: it cited the second Space's zombies (B1, B3,
B5), which the real review did not, and it cited the `simulation` tag at 0.65, which alone
made the Super Station "changes requested". The first follows the Code's words and is the
safe advice; the second is why the citation rule in Step 4 exists.

## 6. Keeping this skill in sync with the Code

`references/code.md` and `references/findings.json` are generated; never edit them. When
the Code, the author copy or the statement and fiction rules change, run, in
`infrastructure/cloudflare/api`:

```bash
npm run sync-judge-skill
```

It rewrites the references from the live sources and mirrors this whole skill into
`.windsurf/skills/` and `.claude/skills/`. Edit `SKILL.md` and `scripts/` only here in
`.agents/skills/eustress-judge/`. The API's tests fail when any copy drifts
(`tests/judge_skill_sync.test.mjs`), and `verdict.mjs` imports the live rules, so the
verdict logic cannot drift at all.
