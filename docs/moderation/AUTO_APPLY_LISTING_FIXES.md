# Automatic listing fixes after "Changes requested"

Status: approved by McKale 2026-11-14 and built in the working tree; not deployed. Worker: `src/moderation.mjs`
(`autoFixPlan`, `setStatementLines`, `applyAutoFix`, `reviewAfterAutoFix`, `autoFixView`), `src/world.mjs` (`applyListing`
guard), `src/moderation_email.mjs`. Studio: `engine/src/moderation_dossier.rs` (`ListingReview.fixed`,
`adopt_fixed_description`). Tests: `tests/auto_fix.test.mjs`, the "auto-fix" tests in `tests/automated_review.test.mjs`,
`tests/world.test.mjs`, `tests/listing_tags_judge.test.mjs`.

## What it does

When a review comes back "Changes requested", Eustress makes one automatic pass over the listing's metadata and fixes the
findings that are about the listing's words, not the Space: the Qualification Statement, the fiction label, the tags and the
Category line. When those were the only findings, the listing goes back through the same review at once, and if it passes it
is listed without the creator doing anything. When a finding needs a change inside the Space (a lesson, a real-world effect, a
measured result, a broken build), the pass still makes the metadata fixes, then stops and tells the creator plainly what is left.

There is exactly one pass per publish. The re-review never starts a second pass.

## Where it runs: the Worker, at the end of the review run

The pass runs in `infrastructure/cloudflare/api/src/moderation.mjs`, right after `decideFinal` returns `changes_requested`
and before `setStatus` tells the author.

Why there and not in Studio:

- The judge has already written what the fix needs, on this case, from what it saw and read: `suggested_statement` (the four
  lines, drafted from the captures, the digest and every script), `suggested_tags`, and `is_fiction`. The pass reuses that
  answer. It makes no new model call to write text.
- It works when Studio is closed, from the website, and from an old Studio.
- The Worker already owns the lease, the content fingerprint, the daily verdict cap and the author's notifications, which are
  exactly what one pass and one re-review need. It already writes judge-chosen tags to a listing (`applyToSim` with
  `tags_source: "judge"`); this extends that precedent to the description.

Studio gets one small change (below) so the next publish does not undo the fix.

## What the pass may change

| Finding (code) | What the pass does | Source of the new text |
|---|---|---|
| `statement_missing` | Fills each missing line of the four (Category, Objective, Lesson, Real-world link). Lines the creator wrote are kept as written. The lines go at the end of the description, each on its own line. | The judge's `suggested_statement` |
| `statement_unclear` (the four lines are there but do not describe this Space) | Replaces the four lines with the judge's draft. The rest of the description is untouched. | The judge's `suggested_statement` |
| `fiction_label_missing` | Adds the tag `fiction` and one genre tag, and sets the Category line to Fiction. | The judge's `is_fiction` and its first genre tag in `suggested_tags` (`assignedTags`) |
| `not_yet_tags` (a tag misdescribes the Space, or is a slur, insult or personal information) | Replaces the tags with the judge's choice, the same as when a creator leaves tags empty. | The judge's `suggested_tags` (`assignedTags`) |

Never changed by the pass:

- **The title.** It stays the creator's, as open-ended as they wrote it. No rule asks for a narrower title, and the pass never
  touches it.
- **The creator's own description text.** Only the four labelled statement lines are added or replaced. Every other line stays
  byte for byte.
- **Anything inside the Space**: parts, scripts, captures, thumbnail, cover, rating answers.

Left to the creator, always: every `test_*` and `confirm_*` finding (T1 to T4), `review_unconfirmed`, `statement_contradicted`
(the statement says something the Space does not do), every Code article (`code_*`, `not_yet_B*`), `images_not_seen`, the
script findings (`scripts_not_read`, `republish_*`, `too_much_script`), and `code_not_met`.

A fixable finding the judge gave no material for stays with the creator: no draft (or a draft that fails `statementDraftOf`)
leaves `statement_missing`; no genre tag in `suggested_tags` leaves `fiction_label_missing`.

## Grounding: no invented claims

- Every word the pass writes comes from the judge's structured answer on this case, which the judge wrote from the four
  captures, the digest and 100% of the package's scripts. The Worker writes no prose of its own and asks no model to write any.
- Every line passes `statementDraftOf` (a Category from Part III's list, author-safe text through `safeAuthorText`, 300
  characters a line) and every tag passes `judgeTags` (normalized, personal information dropped), the same filters the author
  view uses today.
- The re-review checks the result: a listing is listed only when the judge's `statement_check` says the statement `matches` the
  Space (rule 8). A draft that overstates the Space cannot list itself.
- `statement_contradicted` and `confirm_real_world` are not fixed automatically, because fixing them would mean writing a claim
  the review could not confirm.

## The one-pass limit

- The case records `auto_fix: { pass: 1, at, applied: [...], left: [...], before: { description, tags }, after: { description, tags } }`.
- A run whose case already has `auto_fix` for the current publish makes no pass. The re-review is such a run, so it can never
  loop.
- A new publish by the creator (new content, or a description or tags they edited) starts a fresh case, and its own review may
  make its own one pass.
- The re-review counts against `max_ai_verdicts_per_day_per_listing` (3) like any review. When the cap is reached, the fixes are
  applied, the listing waits as `pending`, and the sweep reviews it once the day allows; nothing waits on a person.
- The pass is skipped entirely on any case under a legal hold, quarantine, or content lock (`contentLockedFor`), and on any
  outcome other than `changes_requested` in the quality lane.

## The re-review: the same review, nothing skipped

When every finding was fixed, the listing is set back to `pending` and goes through the normal review with trigger
`auto_fix`: Jev triage on the new description, the scripts stage, the judge, the Part IV rules, the coverage gate and the
approval binding. It starts as soon as the first run lets go of its lease, so the creator sees a result within minutes.
When it cannot start then (a tick past its deadline, or the listing's daily verdicts used up), the listing stays `pending`
and the stalled-pending sweep reviews it, as it does any publish whose review never started.

Writes to the listing follow the tags precedent: the patch is applied under the run's lease, and only while the listing's
description and tags are still the ones the run read. If the creator edited either during the run, their edit wins and the
pass is dropped.

## What the creator sees

In Studio's Listing panel, on the website's projects page and in the one decision email:

1. **Everything fixed, re-review running**: status "In review" with the line "We added your Qualification Statement and tags
   and are reviewing your listing again." No "Changes requested" email is sent for the first decision, so the creator gets one
   message, the final one.
2. **Re-review listed**: "Listed in the Gallery", plus an "Applied for you" list (for example: "Added the Qualification
   Statement", "Added tags: fiction, science-fiction", "Set the Category line to Fiction").
3. **Space changes still needed**: "Changes requested", with the "Applied for you" list first and then "Still needs a change in
   your Space", one finding per line in the Code's copy (for example "Lesson (Code T3)"). Appeal stays available.

The creator's earlier description and tags are kept on the case and in the author view (`auto_fix.description_before`,
`tags_before`), so the projects page can offer "Undo these changes" (not built: Website's to add). A later publish with the
creator's own edits always wins over the pass.

The author view (`GET .../moderation`) carries `auto_fix`: `applied` (each `{ category, title, detail }`), `reviewing_again`,
`applied_at`, `description_before`, `description_after`, `tags_before`, `tags_after`; null when no pass was made for this
publish, and always null on a hold.

Keeping the fix through the next publish, two ways:

- Worker: the listing records `listing_fix: { at, before, after }`. A commit that sends back exactly the description or tags
  from before the fix (a Studio that has not read it) keeps the fixed ones; any other text the creator sends is theirs and
  ends the guard. A consequence: the exact pre-fix text cannot be restored by a commit while the guard stands; any edit to it
  can.
- Studio: on reading a review with `auto_fix.description_after`, Studio writes it into `.eustress/publish.toml`
  (`listing.description`) if the local description still equals `description_before`, so the Publish dialog opens with the
  fixed text. Tags already pre-fill from the server listing. The Listing panel shows each fix as a "Fixed for you: ..." card
  above what is left; it shows cards only for "Changes requested" and "Not listed", so a listed result shows its fixes in
  Output and the notification, not as cards (a card for "Listed" needs a `listing_status.slint` change).

## The re-review's verdict: it may reject on quality (McKale, 2026-11-14)

"Quality" here means the Code's four tests, each scored 0 to 3: T1 build quality, T2 objective, T3 lesson, T4 real-world effect.
The rules turn the scores into an outcome:

- every test at 2 or more, and the statement matches the Space: listed;
- exactly one test at 1: "Changes requested";
- any test at 0, or two tests at 1: "Not listed".

The re-review is a full, fresh review, and its outcome stands whatever it is, a quality rejection included. A fix can therefore
end in "Not listed" where the first review said "Changes requested". Two ways that happens:

1. **The fix lets the judge score a test it set aside.** While a Space that looks like fiction is unlabelled, the rules do not
   reject on its T4 score; they ask for the label first. Once the pass labels it, T4 is scored as "T4 for Fiction" (does the
   Space carry a theme, question or speculative idea?), and a 0 there is "Not listed". Pong is this case: its T4 is 0.
2. **A fresh judge reading scores differently.** A test at 2 on the first reading can come back at 1 or 0.

A rejection after the pass reads like any rejection: "Not listed", the failed test in the Code's copy, the "Applied for you"
list, and Appeal. The creator can change the Space and publish again, and that publish gets its own review.

## Pong, the case that prompted this

Pong's review had three findings: Lesson (T3), Qualification Statement missing, and Label your fiction, and the judge's note
says T4 is 0. Under this design the pass would add the four statement lines and the fiction and genre tags, set Category, and
stop there, because T3 is a change inside the Space. The listing stays "Changes requested" with "Lesson (Code T3)" left and the
judge's T4 note. This feature does not make Pong listable: Pong needs a lesson built into the Space and a real-world link or a
fiction idea. Publishing Pong from a Universe that holds only Pong also keeps Box Head out of its package.

## Review and rollout

- Tests cover each fixable finding, each creator-only finding, the one-pass limit, the creator edit that wins mid-run, a
  quarantine applied mid-run, the daily cap, one notice for the author, the re-review's rejection standing, and the commit
  guard. 14 deliberate mutations of the guards, each run on a scratch copy against an unmutated control: 14 caught.
- Security reviews the Worker change before staging (it writes listing text from the moderation run).
- Staging first, on a test listing that fails only on its statement. Production only on McKale's word.
- The Guardian policy text and its hash do not change. `docs/policy/COMMUNITY_STANDARDS.md` gains one sentence telling creators
  that Eustress fixes listing wording once, automatically.
