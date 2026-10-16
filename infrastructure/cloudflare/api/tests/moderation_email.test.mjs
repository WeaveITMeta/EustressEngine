// The email an author gets when a listing is decided. It is rendered only from
// the author view, so nothing about how the review works can reach an inbox,
// and every author-supplied value is escaped.

import test from 'node:test';
import assert from 'node:assert/strict';
import { renderModerationEmail } from '../src/moderation_email.mjs';

const REVIEW = 'https://eustress.dev/projects?review=8acb281a-44a7-40cd-b468-f6f2a973dfb5';
const UNSUB = 'https://api.eustress.dev/api/notifications/unsubscribe?u=user-1&t=abcd';

const views = {
  listed: { status: 'listed', headline: 'Your Universe is listed in the Gallery.', reasons: [], suggested_edit: null, note: null, rating: 'all_ages', can_appeal: false },
  not_listed: { status: 'not_listed', headline: 'Your Universe was not listed in the Gallery.', reasons: [{ title: 'Content that breaks the rules', why: 'It shows something the Gallery does not allow.', what_to_change: 'Remove it and publish again.' }], suggested_edit: 'Replace the poster.', note: 'Thank you for the work you put in.', rating: null, can_appeal: true },
  changes_requested: { status: 'changes_requested', headline: 'Your Universe needs a few changes before it can be listed.', reasons: [{ title: 'Original lighting', why: '', what_to_change: 'Add lighting you made.' }], suggested_edit: null, note: null, rating: null, can_appeal: true },
};

const render = (view, extra = {}) => renderModerationEmail({ view, kind: 'decision', simName: 'Harbor Town', username: 'ann', reviewUrl: REVIEW, unsubUrl: UNSUB, ...extra });

test('each outcome has its own subject', () => {
  assert.equal(render(views.listed).subject, 'Your Universe "Harbor Town" is live in the Gallery');
  assert.equal(render(views.not_listed).subject, '"Harbor Town" was not listed in the Gallery');
  assert.equal(render(views.changes_requested).subject, 'Changes needed before "Harbor Town" can be listed');
  assert.equal(render({ ...views.listed }, { kind: 'appeal_decided' }).subject, 'Appeal accepted: "Harbor Town" is now listed');
  assert.equal(render({ ...views.not_listed, can_appeal: false }, { kind: 'appeal_decided' }).subject, 'Your appeal for "Harbor Town" was reviewed');
});

test('the text and the HTML carry the same facts', () => {
  const { text, html } = render(views.not_listed);
  for (const fact of ['Hi ann,', views.not_listed.headline, 'Content that breaks the rules', 'It shows something the Gallery does not allow.', 'Remove it and publish again.', 'Replace the poster.', 'Thank you for the work you put in.', REVIEW]) {
    assert.ok(text.includes(fact), `text lacks: ${fact}`);
    assert.ok(html.includes(fact.replace(/&/g, '&amp;')), `html lacks: ${fact}`);
  }
  assert.match(text, /appeal/i);
  assert.match(html, /appeal/i);
  assert.ok(text.includes(`Stop these emails: ${UNSUB}`));
  assert.ok(html.includes('Stop these emails'));
});

test('a listed email shows the age rating, and a changes email shows no rating', () => {
  assert.match(render(views.listed).text, /Age rating: All ages/);
  assert.match(render(views.listed).html, /Age rating: <strong[^>]*>All ages/);
  assert.doesNotMatch(render(views.changes_requested).text, /Age rating/);
});

test('the appeal hint appears only when an appeal is possible', () => {
  assert.match(render(views.not_listed).text, /you can appeal/);
  assert.doesNotMatch(render(views.listed).text, /appeal/);
  assert.doesNotMatch(render(views.listed).html, /appeal/i);
});

test('an empty "why" prints no empty label', () => {
  const { text, html } = render(views.changes_requested);
  assert.doesNotMatch(text, /Why:/);
  assert.doesNotMatch(html, />Why</);
  assert.match(html, />What to change</);
});

test('every author-supplied value is escaped in the HTML, and the subject stays on one line', () => {
  const hostile = '<script>alert(1)</script>';
  const view = { ...views.not_listed, headline: hostile, reasons: [{ title: hostile, why: hostile, what_to_change: hostile }], suggested_edit: hostile, note: hostile };
  const { html, subject } = render(view, { simName: `Harbor${'\r\n'}Bcc: x@y.example <b>`, username: hostile });
  assert.doesNotMatch(html, /<script>/);
  assert.ok(html.includes('&lt;script&gt;alert(1)&lt;/script&gt;'));
  assert.doesNotMatch(subject, /[\r\n]/, 'a listing name cannot write its own headers');
  assert.doesNotMatch(html, /<b>/);
});

test('a link that is not http or https is never printed as a link', () => {
  for (const bad of ['javascript:alert(1)', 'data:text/html,x', '//evil.example', '', undefined, 'https://a.example/"onmouseover="x']) {
    const { html } = render(views.listed, { reviewUrl: bad, unsubUrl: bad });
    assert.doesNotMatch(html, /javascript:|data:text|onmouseover/);
    assert.match(html, /href="#"/);
  }
  assert.match(render(views.listed).html, new RegExp(`href="${REVIEW.replace(/[?.]/g, '\\$&')}"`));
});

test('an email never mentions how the review works, and never uses an em dash', () => {
  for (const [name, view] of Object.entries(views)) {
    for (const kind of ['decision', 'appeal_decided']) {
      const { text, html, subject } = render(view, { kind });
      // The HTML is checked as a reader sees it: tags and their styles (which
      // hold numbers like 0.06em) are not part of what an author reads.
      const visible = html.replace(/<title>[\s\S]*?<\/title>/g, ' ').replace(/<[^>]+>/g, ' ');
      for (const [part, body] of [['text', text], ['html', visible], ['subject', subject]]) {
        assert.doesNotMatch(body, /0\.\d\d|judge|jev|grok|polic|threshold|spatial|classif|quarantin/i, `${name} ${kind} ${part}`);
        assert.ok(!body.includes('—'), `${name} ${kind} ${part} has an em dash`);
      }
    }
  }
});

test('the HTML is a complete, small document with a button to the review page', () => {
  const { html } = render(views.listed);
  assert.match(html, /^<!doctype html>/);
  assert.match(html, /<html lang="en">/);
  assert.match(html, /<title>/);
  assert.match(html, />See the review details<\/a>/);
  assert.ok(html.length < 8000, `html is ${html.length} bytes`);
  // The headline is the hidden preheader mail clients show next to the subject.
  assert.match(html, /display:none[^>]*>Your Universe is listed in the Gallery\./);
});
