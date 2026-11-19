# Editing a published listing on the website

Status: **asked for by McKale 2026-11-14.** The Worker half is built and tested (`infrastructure/cloudflare/api`); the site
half is written in `eustress/crates/web`. Neither is deployed.

The author of a listing opens its page (`/simulation/<id>`), presses **Edit listing**, and changes the title, the
description, who can see it, and the tags. A new title or description is reviewed before the listing is back in the
Gallery, exactly as new text from a Studio publish is.

McKale's example: add `https://rust-justice.miksunbot.workers.dev/` to The Justice Super Station's description, and the
review runs on the new text.

This page is the contract. The Worker enforces it, and the site builds against it.

## The route

`PATCH /api/simulations/{id}/listing`, signed in as the author, with any of:

```json
{ "name": "The Justice Super Station", "description": "...", "is_public": true }
```

- **`name`**: the title. Trimmed; 1 to 120 characters.
- **`description`**: trimmed; at most 4000 characters. Empty is allowed.
- **`is_public`**: `true` or `false`.
- Characters are counted as JavaScript counts them (UTF-16 code units), after trimming. These are the limits a Studio commit
  applies (`world.mjs` `MAX_LISTING`), but a commit cuts text that is too long and this route refuses it.
- Other keys are ignored. A body with none of the three is refused.
- **One bad value refuses the whole request** and nothing is written.

Answers:

| Status | Body | When |
|---|---|---|
| 200 | `{ ok, id, name, description, is_public, archived, changed, review, status }` | Stored. `changed` lists the fields that changed (empty when nothing did). `review` is true when the change sends the listing back to review. `status` is the author-facing status (`listed`, `approved_private`, `in_review`, ...). |
| 400 | `{ error, code, field }` | `bad_request`, `bad_name`, `bad_description`, `bad_visibility`, `too_long`. `error` is a sentence the site shows as it is. |
| 401 | | Not signed in. |
| 404 | | No such listing, or not the author's. The two are never told apart. |
| 409 | `{ code: "in_review" }` | A review is running, or a hold is on the listing. The two read the same, as everywhere else. |

The route is class `fresh` in `src/route_policy.mjs`, like the other routes that change listing text.

## What is reviewed

- **A changed title or description** is text a review reads. The listing goes back to `pending` (`sendBackToReview`) and
  leaves the Gallery until the review passes the new text.
- **The same text again** (after trimming) is no change: nothing is written, nothing is reviewed.
- **`is_public` is not reviewed.** A listing that passed review is listed or kept private at once. An archived listing stays
  private, and the choice is what Unarchive restores (as a commit's is).
- **Tags** keep their own route, `PATCH /api/simulations/{id}/tags` ([LISTING_TAGS.md](LISTING_TAGS.md)).

The edit does not start the review itself. The site asks for it next with `POST /api/simulations/{id}/submit`, the request
Studio makes after a publish, with every limit that route has: the daily number of AI verdicts, the gap between requests,
and a case a person holds staying with the person. A review that is not started then is started by the moderation sweep,
which reviews pending listings on its own.

## A pending appeal is withdrawn

McKale 2026-11-14 ("Withdraw it"). An appeal is about the version that was decided. When the listing changes while an
appeal waits for a person, the changed listing gets its own review, and the appeal is withdrawn:
- the run that reviews it (trigger `publish` or `sweep`) marks the appeal `withdrawn` and logs `appeal_withdrawn`;
- it takes the appeal out of the Review Workspace;
- a person's later `moderation_resolve_appeal` is refused ("no pending appeal"), so it can never overwrite the newer
  decision.

This covers every change: a web edit of the title or description, a tags change, and a Studio republish. A staff rerun
does not withdraw it, and a hold or quarantine is never reviewed this way.

The creator sees the appeal status `withdrawn`, with "You changed this listing, so your appeal was withdrawn and the new
version was reviewed instead." While an appeal is pending, the edit dialog says that saving a new title or description
withdraws it; a change to visibility alone keeps it. The creator may appeal the new decision.

## Never mid-review

A review run judges the title and description it read when it began, and its verdict is applied to the listing as it is
when it ends. The run's content fingerprint covers the package, not the text. So an edit written while a run is working
would go live on a verdict about the old text.

The edit therefore takes the run's own lease (`acquireLease`) for the moment it writes:

- a run that holds the lease refuses the edit (409 `in_review`);
- a run that starts after the edit reads the new text;
- the listing is read again under the lease, and the legal lock is checked again there.

## Studio's copy of the text

Studio sends the title and description from its own `publish.toml` with every publish. After a web edit that copy is out
of date, and its next publish would put the old text back.

`src/listing_guard.mjs` stops that. Every write that replaces listing text outside Studio records what it replaced in
`sim.listing_fix`: the web edit (`by: "author_web"`), the review's automatic fix (`auto_fix`), and the link review. A
commit that sends back a replaced text, while the listing holds another, keeps the listing's text for that field. Any other
text the author writes in Studio is theirs, and is reviewed. Once a commit sends nothing stale, the guard ends.

`listing_fix` holds `before` (the text the latest write replaced, by field) and `superseded` (every text replaced before
that, newest last, at most 8 a field). So a Studio that missed several changes is still caught.

One consequence: while the guard stands, an author who wants an old text back from Studio does not get it. They set it on
the website, where it becomes the listing's own text.

## The site

`eustress/crates/web/src/pages/listing_edit.rs`. **Edit listing** shows on the listing's page for its author only. The
dialog has the title (with a count to 120), the description (with a count to 4000), Public or Private, the two notes
below, and the tag editor from `/projects`.

- Save sends the edit. When `review` is true it asks for the review and says what the review answered: listed, kept
  private, changes asked for, not listed, still running, or the day's limit reached.
- The tag editor saves on its own route; the dialog then asks for the review too (a request with nothing new to review
  starts nothing).
- Closing after a save reloads the page, so it shows what the Worker stored.
- The notes: a new title or description goes back to review, and Studio's own copy is kept by a stale publish but
  replaced by new text written in Studio.

## Not covered here

- **Share Source** stays in Studio: turning it on needs the source world, which only Studio uploads.
- **The sim page's theme, banner and links** (the Website service) stay in Studio, with their own review.
- **Links in the description** are shown as text. Whether one is shown as a link, and checked, is the link review's
  (`src/link_review.mjs`, [SIM_PAGE_THEME_AND_LINKS.md](../design/SIM_PAGE_THEME_AND_LINKS.md)).
