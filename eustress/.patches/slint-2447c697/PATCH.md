# Patched Slint software renderer (i-slint-renderer-software 1.17.1)

A copy of `internal/renderers/software` from slint-ui/slint at
`2447c697327e7f87c5044af229e0be6c18608fcc` (the commit `Cargo.lock` resolves the
`master` branch to), with two functions changed. The workspace points at it
from `[patch."https://github.com/slint-ui/slint"]` in `eustress/Cargo.toml`.
Only this crate is replaced: its `i-slint-core` and `i-slint-common`
dependencies come from the same git source as the rest of Slint, so the graph
keeps one copy of each.

## What changed

`i-slint-renderer-software/lib.rs`, in `impl GlyphRenderer for SceneBuilder`:

- `fill_rectangle` (text selection, the text cursor, underline and strikethrough):
  a rectangle that does not fit the `i16` pixel space keeps only its part inside
  the current clip; with nothing there it is not drawn.
- `draw_glyph_run`: each glyph's position is worked out in `f32`, truncated
  where the `i16` conversions it replaces truncated, and a glyph that does not
  fit the `i16` pixel space is skipped.

Plus two helpers beside the `Physical*` type aliases: `fits_i16` and
`fits_i16_rect`. Every changed line is marked `EUSTRESS PATCH`. `Cargo.toml`
writes out the fields Slint inherits from its workspace; the license files are
copies of the repository's `LICENSES` (the crate's own are symlinks). Nothing
else differs: `diff -r` against the checkout shows only those files.

## Why

The software renderer, which Studio draws its whole UI with
(`engine/src/ui/slint_gpu.rs`, `MinimalSoftwareWindow`), keeps physical pixel
coordinates as `i16`. `draw_glyph_run` converted the item's offset from the
window and every glyph's offset from its Text with euclid's `.cast()`, which
panics when a value does not fit; `fill_rectangle` did the same for its
rectangle. Lines are skipped only when they fall outside the Text item's own
box, not when they are scrolled out of view, so:

- a Text sized to its content panics once its text is taller than about
  32,767 physical px: 10,000 short lines, or about 1,500 lines of 14 px text at
  150% display scale;
- one line wider than that panics in any Text: about 4,000 characters;
- a TextInput as tall as its text, which is how the script editor is built,
  panics on its first frame with a long script, and on scrolling deep into it
  or on Select All even when the glyphs drawn are on screen, because the
  item's own offset no longer fits.

In Studio the panic aborts the process (exit 0xC0000409). Any string a person
does not type themselves can carry it: a file, a Space property, a model's
reply, a tool's output, a peer's chat. The engine also bounds such strings
before they reach Slint (`engine/src/ui/display_text.rs`); the patch is what
makes the rest safe, including the script editor and text fields.

A glyph or rectangle past the `i16` space cannot be on screen (no window is
32,767 px across), so skipping it changes no visible pixel. Anything that fits
takes the same values as before.

## Test

- `engine/src/ui/display_text.rs`, `render_tests::text_past_the_i16_limit_draws`:
  renders a 10,000-line Text, a 131,000-character Text wrapped and unwrapped,
  and a 10,000-line TextInput in a Flickable (at the top, scrolled 150,000 px
  down, and with everything selected) through `MinimalSoftwareWindow` at 150%,
  with both buffer kinds, and asserts each frame draws and shows text. With the
  stock renderer every one of those cases panics.
- Measured before landing (2026-10-01, scratch probe built against this build's
  own Slint rlibs): stock panics in all 36 raw cases (6 cases, 1.0x and 1.5x,
  new and reused buffers); patched draws all of them, and on strings both can
  draw the two give the same changed-pixel counts.

## Provenance

- `verify.mjs` (`node .patches/slint-2447c697/verify.mjs`) checks this copy against
  the revision it was made from, found in Cargo's git checkout (or `SLINT_SRC`).
  Every file but `lib.rs`, `Cargo.toml` and the license copies must be identical to
  upstream. Upstream's `lib.rs` must be the one the patch was made against, and the
  vendored `lib.rs` must be that file plus `lib.rs.patch`; both are checked by
  SHA-256. It exits 1 with the reasons otherwise.
- `engine/src/ui/display_text.rs`, `slint_patch::slint_stays_on_the_revision_the_patch_was_made_for`:
  while the `[patch]` entry is in `eustress/Cargo.toml`, every Slint crate in
  `Cargo.lock` must come from `2447c697`. A `cargo update` that moves Slint fails
  the tests until the patch is checked against the new revision.

## License

Slint is licensed GPL-3.0-only, or under its Royalty-free 2.0 or Software 3.0
licenses; the texts are in `i-slint-renderer-software/LICENSES`. Whether Eustress's
licence for Slint permits shipping a modified copy of this crate is McKale's call,
and it is asked with this patch. Posting upstream (`UPSTREAM.md`) also waits for him.

## When to drop it

When the Slint revision in `Cargo.lock` contains an equivalent fix (the
upstream issue and pull request are drafted in `UPSTREAM.md`). To check: build
without the `[patch]` entry and run the test above; if it passes, delete this
folder and the `[patch."https://github.com/slint-ui/slint"]` entry. Moving the
Slint revision while this patch is in place needs the patch reapplied to that
revision's `lib.rs` (the `EUSTRESS PATCH` markers say where).
