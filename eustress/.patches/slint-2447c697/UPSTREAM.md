# Upstream drafts (not posted)

Posting waits for McKale's OK. Repository: https://github.com/slint-ui/slint

## Issue

**Title:** Software renderer panics when a Text or TextInput draws past 32,767 physical pixels

**Body:**

The software renderer keeps physical coordinates as `i16`. In
`internal/renderers/software/lib.rs`, `SceneBuilder::draw_glyph_run` converts
the item's offset from the window and each glyph's offset from its item with
euclid's `.cast()`, and `fill_rectangle` converts its rectangle the same way.
`.cast()` panics when the value does not fit (euclid `vector.rs`, the `unwrap`
in `cast`).

Lines of a Text are skipped only when they fall outside the item's own box
(`sharedparley`, `paragraph_line_within_box`), so the panic is reachable with
ordinary content:

1. A Text whose height follows its content (`height: self.preferred-height`)
   with 10,000 short lines.
2. A Text with one line of 131,000 characters, wrapped or not.
3. A multi-line `TextInput` as tall as its text inside a `Flickable` (a code
   editor), with 10,000 lines: on the first frame, after scrolling 150,000 px
   down, and after `select-all()`.

Reproduction: a `MinimalSoftwareWindow` (either `RepaintBufferType`), 800 x 600,
scale factor 1.0 or 1.5, the component below, render once.

```slint
export component Repro inherits Window {
    in property <string> body;
    Text { x: 0px; y: 0px; width: 380px; height: self.preferred-height;
           text: root.body; wrap: word-wrap; font-size: 14px; }
}
```

with `body` set to `"line of text\n".repeat(10_000)`. Expected: the visible
lines draw. Actual: panic in `draw_glyph_run`. At revision 2447c697 all of the
cases above panic at 1.0x and 1.5x with both buffer kinds.

With an application that shows text it did not write (a log, a file, a chat),
the panic is reachable from outside the application.

## Pull request

**Title:** software renderer: skip glyphs and rectangles outside the i16 pixel space instead of panicking

**Body:**

Fixes #<issue>.

`draw_glyph_run` now works out each glyph's position in `f32`, truncating
where the previous `i16` conversions truncated, and skips a glyph whose
rectangle does not fit `i16`. `fill_rectangle` clips a rectangle that does not
fit to the current clip before converting it, and skips it when nothing is
left. A glyph or rectangle outside the `i16` space cannot be visible, so no
on-screen pixel changes; anything that fits takes exactly the values it took
before.

Tested by rendering the issue's cases through `MinimalSoftwareWindow` at 1.0x
and 1.5x with `NewBuffer` and `ReusedBuffer`: every case draws, the scrolled
editor shows its visible lines, and on content the previous code could draw the
changed-pixel counts are identical.

The diff is `lib.rs.patch` beside this file (applies to 2447c697). Before
posting, add a test in the renderer's own test suite in Slint's style, and run
`cargo fmt`.
