// The shell every Eustress HTML email is built in (src/email_shell.mjs), and
// every template that fills it: the verification link, the address-change
// notices, the key notices, the administrators' summary and the registration
// email. What is held here, for each template:
//
//   * it comes out of the shell (header and footer markers, ONE image, which is
//     the header gear, the body slot filled);
//   * its plain-text twin has no markup and carries the same links;
//   * everything it was given is escaped, however hostile;
//   * no script, no other image, no tracking pixel, beacon or query parameter;
//   * the links it carries are the links it carried before the shell, and no
//     others (the key notices have no link where there was none);
//   * explicit background and text colours on every block, so a mail client that
//     inverts colours or drops CSS leaves it readable.

import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {
  renderEmail, html, escapeHtml, formatWhen, parseWhen, UNKNOWN_DATE, paragraph, button, anchor, urlLine, facts, bullets, code,
  ASSET_ORIGIN, LOGO_PATH, FOOTER_LINKS,
} from '../src/email_shell.mjs';
import {
  renderEmailNotice, renderKeyNotice, renderAdminActionNotice, buildNoticeMime, KEY_NOTICE_KINDS, EMAIL_NOTICE_KINDS,
} from '../src/account_notices.mjs';
import { buildIdentityEmail } from '../src/identity_email.mjs';

const SRC = path.resolve(import.meta.dirname, '..', 'src');
const LOGO = ASSET_ORIGIN + LOGO_PATH;
const SITE = 'https://eustress.dev';
const SUPPORT_HREF = 'mailto:support@eustress.dev';
const T = Date.UTC(2026, 9, 2, 12, 0, 0);

const VERIFY_LINK = `${SITE}/verify-email?token=0b6a1f6e-6b1d-4e0c-9d57-3c1f2f0a9b11.9f2c4d8a7b6e5f4a3b2c1d0e9f8a7b6c`;
const CANCEL_LINK = `${SITE}/cancel-email-change?token=abc.def`;
const UNDO_LINK = `${SITE}/undo-key-replacement?token=aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee.0123456789abcdef`;

const unescapeHtml = (s) => s.replace(/&(amp|lt|gt|quot|#39);/g, (_, e) => ({ amp: '&', lt: '<', gt: '>', quot: '"', '#39': "'" }[e]));
const hrefsOf = (h) => [...h.matchAll(/<a\b[^>]*\shref="([^"]*)"/g)].map((m) => unescapeHtml(m[1]));
const urlsIn = (text) => (text.match(/https?:\/\/[^\s<>"')]+/g) || []);
const bodySlot = (h) => h.slice(h.indexOf('<!-- eustress-email:body -->'), h.indexOf('<!-- eustress-email:footer -->'));
/// The attribute names of every tag, with quoted values removed, so `onerror` inside an escaped value is not mistaken for a handler.
const attributeNames = (h) => {
  const names = [];
  for (const tag of h.matchAll(/<[a-z][a-z0-9]*\b((?:"[^"]*"|'[^']*'|[^>"'])*)>/gi)) {
    for (const a of tag[1].replace(/"[^"]*"|'[^']*'/g, '""').matchAll(/\s([^\s=\/>"']+)/g)) names.push(a[1].toLowerCase());
  }
  return names;
};
const stripHtmlish = (s) => /<\/?[A-Za-z!][^>]*>/.test(s);

// Hostile values, each with a marker so an escape of one cannot pass for another.
const HOSTILE = ['<script>alert(1)</script>', '"><img src=x onerror=1>', 'Q&amp;Z'];

// ── The templates ────────────────────────────────────────────────────────────
// `data` is a plain sample; `shown` are the fields the HTML prints (each is made
// hostile alone and must come out escaped); `urls` are fields that are links;
// `links` is the exact set of addresses the template links to besides the
// footer's; `footer` is the shell footer it uses.

const NOTICE = {
  username: 'McKale', at: '2026-10-01T10:00:00.000Z', replacedAt: '2026-09-30T10:00:00.000Z',
  oldFingerprint: 'a1b2 c3d4 e5f6 0718', newFingerprint: '9988 7766 5544 3322',
  valueFrozenUntil: T, payoutPinnedUntil: T, revoked: { api_keys: 1, webhooks: 2, discord: true },
  caseRef: 'SUP-1042', undoLink: UNDO_LINK, undoUntil: T,
};
const ADMIN = {
  adminName: 'admin_user', adminId: '33333333-3333-4333-8333-333333333333', targetName: 'McKale', targetId: '11111111-1111-4111-8111-111111111111',
  caseRef: 'SUP-1042', reason: 'ID checked on a video call', at: '2026-10-01T10:00:00.000Z',
};
const EMAIL = {
  username: 'McKale', link: VERIFY_LINK, expiresAt: new Date(T).toISOString(), newMasked: 'm***@gmail.com', cancelledMasked: 'm***@gmail.com',
  sessionsEnded: true, effectiveAt: T, cancelLink: CANCEL_LINK,
};

const TEMPLATES = [
  { name: 'email_verify', footer: 'full', data: EMAIL, render: (d) => renderEmailNotice('email_verify', d), shown: ['username', 'link'], urls: ['link'], links: (d) => [d.link], inBody: 'McKale' },
  { name: 'email_change_existing', footer: 'plain', data: EMAIL, render: (d) => renderEmailNotice('email_change_existing', d), shown: ['username', 'newMasked', 'cancelLink'], urls: ['cancelLink'], links: (d) => [d.cancelLink], inBody: 'm***@gmail.com' },
  { name: 'email_change_pending', footer: 'plain', data: EMAIL, render: (d) => renderEmailNotice('email_change_pending', d), shown: ['username'], urls: [], links: () => [], inBody: 'McKale' },
  { name: 'email_changed', footer: 'plain', data: EMAIL, render: (d) => renderEmailNotice('email_changed', d), shown: ['username', 'newMasked'], urls: [], links: () => [SUPPORT_HREF], inBody: 'm***@gmail.com' },
  { name: 'email_change_cancelled', footer: 'plain', data: EMAIL, render: (d) => renderEmailNotice('email_change_cancelled', d), shown: ['username', 'cancelledMasked'], urls: [], links: () => [], inBody: 'McKale' },
  { name: 'rotation', footer: 'plain', data: NOTICE, render: (d) => renderKeyNotice('rotation', d), shown: ['username', 'oldFingerprint', 'newFingerprint'], urls: [], links: () => [SUPPORT_HREF], inBody: 'a1b2 c3d4 e5f6 0718' },
  { name: 'admin_replacement', footer: 'plain', data: NOTICE, render: (d) => renderKeyNotice('admin_replacement', d), shown: ['username', 'oldFingerprint', 'newFingerprint', 'caseRef', 'undoLink'], urls: ['undoLink'], links: (d) => [d.undoLink, SUPPORT_HREF], inBody: 'SUP-1042' },
  { name: 'admin_replacement (no undo link)', footer: 'plain', data: { ...NOTICE, undoLink: '' }, render: (d) => renderKeyNotice('admin_replacement', d), shown: ['username', 'caseRef'], urls: [], links: () => [SUPPORT_HREF], inBody: 'SUP-1042' },
  { name: 'key_restored', footer: 'plain', data: NOTICE, render: (d) => renderKeyNotice('key_restored', d), shown: ['username'], urls: [], links: () => [SUPPORT_HREF], inBody: 'McKale' },
  { name: 'passkey_added', footer: 'plain', data: { ...NOTICE, passkeyName: 'Work laptop' }, render: (d) => renderKeyNotice('passkey_added', d), shown: ['username', 'passkeyName'], urls: [], links: () => [SUPPORT_HREF], inBody: 'Work laptop' },
  { name: 'passkey_removed', footer: 'plain', data: { ...NOTICE, passkeyName: 'Work laptop' }, render: (d) => renderKeyNotice('passkey_removed', d), shown: ['username', 'passkeyName'], urls: [], links: () => [SUPPORT_HREF], inBody: 'Work laptop' },
  { name: 'admin_action', footer: 'plain', data: ADMIN, render: (d) => renderAdminActionNotice(d), shown: ['adminName', 'adminId', 'targetName', 'targetId', 'caseRef', 'reason', 'at'], urls: [], links: () => [], inBody: 'ID checked on a video call' },
  {
    name: 'registration', footer: 'full', data: { username: 'McKale', toml_content: 'private_key = "0000000000000000000000000000000000000000000000000000000000000000"\n', host: undefined },
    render: (d) => buildIdentityEmail(d), shown: ['username'], urls: [], links: () => [`${SITE}/login`], inBody: 'McKale',
    // The registration email strips < > " ' from the name before it is used as a file name; what is left is escaped.
    shownAs: (value) => escapeHtml(String(value).replace(/[<>"']/g, '')),
  },
];

const shownAs = (t, value) => (t.shownAs ? t.shownAs(value) : escapeHtml(value));
const asField = (t, field, payload) => (t.urls.includes(field) ? `https://example.test/x?q=${payload}` : payload);

// ── Every template comes out of the shell ────────────────────────────────────

test('every template renders inside the shell: header, one gear image, body slot filled, footer', () => {
  for (const t of TEMPLATES) {
    const r = t.render(t.data);
    assert.ok(r.html && r.text, t.name);
    assert.match(r.html, /^<!DOCTYPE html>/, t.name);
    assert.match(r.html, /<meta name="color-scheme" content="dark">/, t.name);
    const marks = ['<!-- eustress-email:header -->', '<!-- eustress-email:body -->', '<!-- eustress-email:footer -->'].map((m) => r.html.indexOf(m));
    assert.ok(marks.every((i) => i > 0) && marks[0] < marks[1] && marks[1] < marks[2], `${t.name}: header, body, footer in order`);
    assert.equal((r.html.match(/<img\b/gi) || []).length, 1, `${t.name}: exactly one image`);
    assert.ok(r.html.includes(`<img src="${LOGO}" width="80" height="80"`), `${t.name}: the one image is the header gear`);
    assert.ok(r.html.indexOf('<img') < marks[1], `${t.name}: the gear is in the header`);
    assert.match(r.html.slice(marks[0], marks[1]), /Eustress Engine/, `${t.name}: wordmark in the header`);
    assert.ok(bodySlot(r.html).includes(t.inBody), `${t.name}: the body slot holds ${t.inBody}`);
    assert.match(r.html.slice(marks[2]), /Creation at the speed of thought\./, `${t.name}: tagline in the footer`);
    assert.match(r.html.slice(marks[2]), /&copy; \d{4} Eustress Engine\. All rights reserved\./, `${t.name}: copyright in the footer`);
    if (t.footer === 'full') {
      for (const word of ['Product', 'Community', 'Company', 'Legal', 'Discord', 'GitHub']) assert.ok(r.html.slice(marks[2]).includes(word), `${t.name}: footer has ${word}`);
    } else {
      assert.ok(!/Product|Community|Company/.test(r.html.slice(marks[2])), `${t.name}: the plain footer has no nav columns`);
    }
  }
});

test('every template has a plain-text twin with no markup, the same facts and the same links', () => {
  for (const t of TEMPLATES) {
    const r = t.render(t.data);
    assert.ok(r.text.length > 80, t.name);
    assert.ok(!stripHtmlish(r.text), `${t.name}: no HTML tags in the text`);
    assert.ok(!/&(amp|lt|gt|quot|#39);/.test(r.text), `${t.name}: no entities in the text`);
    assert.match(r.text, /^EUSTRESS ENGINE\n/, `${t.name}: the text opens with the wordmark`);
    assert.match(r.text, /Creation at the speed of thought\./, t.name);
    // Every link in the HTML is in the text; mailto links as the address.
    for (const href of hrefsOf(r.html)) {
      assert.ok(r.text.includes(href.startsWith('mailto:') ? href.slice(7) : href), `${t.name}: the text carries ${href}`);
    }
    // And the text carries no web address the HTML does not link to.
    const linked = new Set(hrefsOf(r.html));
    for (const url of urlsIn(r.text)) assert.ok(linked.has(url), `${t.name}: the text has ${url} but the HTML does not link to it`);
    assert.ok(r.html.includes(t.inBody) && r.text.includes(t.inBody), `${t.name}: the same fact in both: ${t.inBody}`);
  }
});

// ── Escaping ─────────────────────────────────────────────────────────────────

test('every field a template prints is escaped, whatever it holds', () => {
  for (const t of TEMPLATES) {
    for (const field of t.shown) {
      for (const payload of HOSTILE) {
        const value = asField(t, field, payload);
        const r = t.render({ ...t.data, [field]: value });
        const where = `${t.name}.${field} <- ${payload}`;
        assert.ok(!r.html.includes(value), `${where}: raw in the HTML`);
        assert.ok(r.html.includes(shownAs(t, value)), `${where}: shown, escaped`);
      }
    }
  }
});

test('hostile values in every field at once leave no script, no other image and no live attribute', () => {
  for (const t of TEMPLATES) {
    for (const payload of HOSTILE) {
      const data = Object.fromEntries(Object.entries(t.data).map(([k, v]) => [k, typeof v === 'string' && k !== 'toml_content' ? asField(t, k, payload) : v]));
      const r = t.render(data);
      const where = `${t.name} <- ${payload}`;
      assert.ok(!r.html.includes('<script'), `${where}: script`);
      assert.ok(!r.html.includes('"><img'), `${where}: broke out of an attribute`);
      assert.ok(!r.html.includes('<img src=x'), `${where}: image`);
      assert.ok(!attributeNames(r.html).some((n) => n.startsWith('on')), `${where}: an event handler`);
      assert.ok(!r.html.includes('Q&amp;Z'), `${where}: an entity written through`);
      assert.equal((r.html.match(/<img\b/gi) || []).length, 1, `${where}: one image`);
      assert.equal((r.html.match(/<a\b/gi) || []).length, hrefsOf(r.html).length, `${where}: every anchor has one plain href`);
    }
  }
});

test('the shell escapes its own inputs: title, heading, label, preheader and the asset origin', () => {
  for (const payload of HOSTILE) {
    const r = renderEmail({
      title: `T${payload}`, heading: `H${payload}`, label: `L${payload}`, preheader: `P${payload}`,
      body: paragraph('x'), text: 'x', footer: 'plain', assetOrigin: payload,
    });
    for (const letter of 'THLP') {
      assert.ok(!r.html.includes(`${letter}${payload}`), `${letter} raw: ${payload}`);
      assert.ok(r.html.includes(`${letter}${escapeHtml(payload)}`), `${letter} escaped: ${payload}`);
    }
    assert.ok(!r.html.includes('<script') && !r.html.includes('"><img'), payload);
    assert.ok(!attributeNames(r.html).some((n) => n.startsWith('on')), payload);
    assert.ok(r.html.includes(`<img src="${LOGO}"`), 'an origin that is not an origin falls back to the production host');
    assert.equal((r.html.match(/<img\b/gi) || []).length, 1);
  }
});

test('the html tag escapes every value but markup, and a link is only ever a web or mail address', () => {
  assert.equal(String(html`<p>${'<b>'}${1}${null}${false}${['<', html`<i>`]}</p>`), '<p>&lt;b&gt;1&lt;<i></p>');
  assert.match(String(anchor('javascript:alert(1)', 'x')), /href="#"/);
  assert.match(String(anchor('data:text/html,<script>', 'x')), /href="#"/);
  assert.match(String(button('javascript:alert(1)', 'x')), /href="#"/);
  assert.match(String(anchor('mailto:support@eustress.dev', 'x')), /href="mailto:support@eustress\.dev"/);
  assert.ok(!String(facts([['<k>', '<v>']])).includes('<k>'));
  assert.ok(String(bullets(['<x>'])).includes('&lt;x&gt;') && !String(bullets(['<x>'])).includes('<x>'));
  assert.ok(String(code('<c>')).includes('&lt;c&gt;'));
  assert.ok(String(urlLine('<a>', '<u>')).includes('&lt;u&gt;'));
});

test('the shell refuses a body that is a plain string and an email without its plain-text twin', () => {
  assert.throws(() => renderEmail({ title: 't', body: '<p>raw</p>', text: 'x' }), /html tag/);
  assert.throws(() => renderEmail({ title: 't', body: paragraph('x') }), /plain-text twin/);
  assert.throws(() => renderEmail({ title: 't', body: paragraph('x'), text: '   ' }), /plain-text twin/);
  assert.throws(() => renderEmail({ body: paragraph('x'), text: 'x' }), /title/);
  assert.throws(() => renderEmail({ title: 't', body: paragraph('x'), text: 'x', footer: 'wide' }), /footer/);
  assert.ok(renderEmail({ title: 't', body: paragraph('x'), text: 'x' }).html.length > 500);
});

// ── No tracking, no extra remote content ─────────────────────────────────────

test('no template carries a script, a second image, a tracking pixel, a beacon or a tracking parameter', () => {
  for (const t of TEMPLATES) {
    const r = t.render(t.data);
    for (const [part, body] of [['html', r.html], ['text', r.text]]) {
      assert.ok(!/<script/i.test(body), `${t.name} ${part}: script`);
      assert.ok(!/track|pixel|beacon/i.test(body), `${t.name} ${part}: track, pixel or beacon`);
    }
    assert.ok(!/<(link|iframe|form|video|audio|object|embed|base|meta\s+http-equiv)\b/i.test(r.html), `${t.name}: remote or active element`);
    assert.ok(!/url\(|@import|background=/i.test(r.html), `${t.name}: a CSS or attribute background that loads something`);
    assert.equal((r.html.match(/\ssrc=/gi) || []).length, 1, `${t.name}: one src, the gear`);
    const addresses = [...hrefsOf(r.html), ...urlsIn(r.text), LOGO];
    for (const address of addresses) {
      assert.ok(!/[?&](utm_\w+|fbclid|gclid|mc_[ec]id|_hsenc|ref|trk|track\w*|source|campaign|email|uid|id)=/i.test(address), `${t.name}: tracking parameter in ${address}`);
    }
    assert.ok(!LOGO.includes('?') && r.html.includes(`src="${LOGO}"`), `${t.name}: the gear is a static asset with no query string`);
  }
});

// ── The links each template carries ──────────────────────────────────────────

test('each template links to the addresses it linked to before the shell, and a plain footer adds none', () => {
  for (const t of TEMPLATES) {
    const r = t.render(t.data);
    const hrefs = hrefsOf(r.html);
    const expected = t.links(t.data);
    if (t.footer === 'plain') {
      assert.deepEqual([...hrefs].sort(), [...expected].sort(), `${t.name}: exactly its own links`);
    } else {
      const rest = hrefs.filter((h) => !FOOTER_LINKS.includes(h));
      assert.deepEqual([...rest].sort(), [...expected].sort(), `${t.name}: its own links, then only the footer's`);
      assert.ok(FOOTER_LINKS.every((h) => hrefs.includes(h) && r.text.includes(h)), `${t.name}: the footer's links in the HTML and the text`);
    }
  }
});

test('the key notices keep the links they had: the replacement notice has the undo link, the others have none to act on', () => {
  const rotation = renderKeyNotice('rotation', NOTICE);
  assert.deepEqual(hrefsOf(rotation.html), [SUPPORT_HREF]);
  assert.ok(!urlsIn(rotation.text).length, 'no web address in the rotation text');
  const restored = renderKeyNotice('key_restored', NOTICE);
  assert.deepEqual(hrefsOf(restored.html), [SUPPORT_HREF]);
  assert.ok(!urlsIn(restored.text).length);
  const replaced = renderKeyNotice('admin_replacement', NOTICE);
  assert.deepEqual(hrefsOf(replaced.html), [UNDO_LINK, SUPPORT_HREF]);
  assert.deepEqual(urlsIn(replaced.text), [UNDO_LINK]);
  assert.ok(replaced.html.includes('<b>If you did ask support to replace your key, do NOT use this link: it puts your OLD key back.</b>'));
  assert.ok(replaced.text.includes('If you DID ask support to replace your key, do NOT use this link: it puts your OLD key back.'));
  const second = renderAdminActionNotice(ADMIN);
  assert.deepEqual(hrefsOf(second.html), []);
  assert.ok(!urlsIn(second.text).length);
  for (const r of [rotation, restored, replaced, second]) {
    assert.ok(!/[0-9a-f]{64}/i.test(r.html + r.text), 'a fingerprint, never a key in full');
  }
});

test('the key notices print the fingerprints, the case and what changed, in the HTML and the text', () => {
  const r = renderKeyNotice('admin_replacement', NOTICE);
  for (const fact of ['a1b2 c3d4 e5f6 0718', '9988 7766 5544 3322', 'SUP-1042', 'October 1, 2026 at 10:00 AM UTC', 'October 2, 2026 at 12:00 PM UTC', '1 API key(s), 2 webhook endpoint(s), the Discord link']) {
    assert.ok(r.html.includes(fact), `HTML: ${fact}`);
    assert.ok(r.text.includes(fact), `text: ${fact}`);
  }
});

// ── The verification email ───────────────────────────────────────────────────

test('the verification email has a real button whose href is the link exactly, and the address spelled out under it', () => {
  const r = renderEmailNotice('email_verify', EMAIL);
  const buttonCell = /<td align="center" bgcolor="#1f6feb"[^>]*><a href="([^"]*)" target="_blank"[^>]*><font color="#ffffff">Confirm this address<\/font><\/a><\/td>/.exec(r.html);
  assert.ok(buttonCell, 'the button is a table cell with a background colour around the anchor');
  assert.equal(buttonCell[1], VERIFY_LINK, 'the href is the link, exactly');
  assert.match(r.html, /<table role="presentation" align="center"[^>]*bgcolor="#161b22"[^>]*>\s*<tr><td align="center" bgcolor="#1f6feb"/, 'inside a table');
  // The spelled-out address, in a text node of its own.
  assert.ok(new RegExp(`copy this address into your browser:<br><span[^>]*>${VERIFY_LINK.replace(/[.?]/g, '\\$&')}</span>`).test(r.html), 'the raw URL fallback line');
  assert.ok(r.text.includes(`\n${VERIFY_LINK}\n`), 'the raw URL in the text, on its own line');
  assert.equal(urlsIn(r.text).filter((u) => u === VERIFY_LINK).length, 1);
  assert.match(r.subject, /^Confirm your Eustress email address$/);
});

test('the link is escaped in the button and the fallback line, and is the same link once unescaped', () => {
  const link = `${SITE}/verify-email?token=a.b&next="x"&y='z'<>`;
  const r = renderEmailNotice('email_verify', { ...EMAIL, link });
  const anchorHref = /<a href="([^"]*)" target="_blank"/.exec(r.html)[1];
  assert.equal(anchorHref, escapeHtml(link));
  assert.equal(unescapeHtml(anchorHref), link, 'the link a client follows is the link given');
  assert.ok(!r.html.includes(link), 'never raw');
  assert.ok(!r.html.includes('"x"') && !r.html.includes('<>'), 'no quote or bracket of the link is live');
  assert.ok(r.html.includes(`<span style="color:#79c0ff;font-family:'SF Mono',Consolas,Monaco,monospace;font-size:12px;word-break:break-all;">${escapeHtml(link)}</span>`));
  assert.ok(r.text.includes(link), 'plain in the text');
  // A link that is not a web address is not made live.
  assert.match(renderEmailNotice('email_verify', { ...EMAIL, link: 'javascript:alert(1)' }).html, /<a href="#" target="_blank"/);
});

// ── Times ────────────────────────────────────────────────────────────────────

const WHEN = '2026-10-02T21:14:00Z';
const UTC_WHEN = 'October 2, 2026 at 9:14 PM UTC';
const PHOENIX_WHEN = 'October 2, 2026 at 2:14 PM MST (America/Phoenix)';

test('formatWhen: month, day, year, a 12-hour clock and the zone, in the account zone', () => {
  // Arizona has no daylight saving time: MST all year.
  assert.equal(formatWhen(WHEN, 'America/Phoenix'), PHOENIX_WHEN);
  assert.equal(formatWhen('2026-07-04T16:05:00Z', 'America/Phoenix'), 'July 4, 2026 at 9:05 AM MST (America/Phoenix)');
  assert.equal(formatWhen('2026-01-15T16:05:00Z', 'America/Phoenix'), 'January 15, 2026 at 9:05 AM MST (America/Phoenix)');
  // New York: EDT in summer, EST in winter.
  assert.equal(formatWhen('2026-07-04T16:05:00Z', 'America/New_York'), 'July 4, 2026 at 12:05 PM EDT (America/New_York)');
  assert.equal(formatWhen('2026-01-15T16:05:00Z', 'America/New_York'), 'January 15, 2026 at 11:05 AM EST (America/New_York)');
  // The day changes with the zone, and a half-hour offset keeps its minutes.
  assert.match(formatWhen(WHEN, 'Asia/Kolkata'), /^October 3, 2026 at 2:44 AM (GMT\+5:30|IST) \(Asia\/Kolkata\)$/);
  assert.equal(formatWhen('2026-10-02T02:00:00Z', 'America/Phoenix'), 'October 1, 2026 at 7:00 PM MST (America/Phoenix)');
  // The zone is printed as the account spelled it, in the zone's own letter case.
  assert.equal(formatWhen(WHEN, 'america/phoenix'), PHOENIX_WHEN);
  assert.equal(formatWhen(WHEN, '  America/Phoenix '), PHOENIX_WHEN);
  assert.ok(formatWhen(WHEN, 'Asia/Kolkata').endsWith('(Asia/Kolkata)'), 'not an older alias of it');
});

test('formatWhen: with no zone, or one that is not a zone, UTC; never a 24-hour clock', () => {
  assert.equal(formatWhen(WHEN), UTC_WHEN);
  assert.equal(formatWhen(WHEN, undefined), UTC_WHEN);
  assert.equal(formatWhen(WHEN, 'UTC'), UTC_WHEN);
  for (const garbage of ['Not/AZone', '', '   ', null, 7, true, {}, [], ['America/Phoenix'], '<script>alert(1)</script>', '"><img src=x onerror=1>', 'Q&amp;Z', 'x'.repeat(100), 'America/Phoenix\nBcc: x@y.test', 'America/Phoenix; DROP']) {
    assert.equal(formatWhen(WHEN, garbage), UTC_WHEN, `zone ${JSON.stringify(garbage)}`);
  }
  for (const [iso, zone, clock] of [
    ['2026-10-02T00:00:00Z', undefined, '12:00 AM'], ['2026-10-02T12:00:00Z', undefined, '12:00 PM'],
    ['2026-10-02T00:30:00Z', undefined, '12:30 AM'], ['2026-10-02T12:30:00Z', undefined, '12:30 PM'],
    ['2026-10-02T07:00:00Z', 'America/Phoenix', '12:00 AM'], ['2026-10-02T19:00:00Z', 'America/Phoenix', '12:00 PM'],
    ['2026-10-02T01:00:00Z', undefined, '1:00 AM'], ['2026-10-02T13:00:00Z', undefined, '1:00 PM'], ['2026-10-02T23:59:00Z', undefined, '11:59 PM'],
  ]) {
    const shown = formatWhen(iso, zone);
    assert.ok(shown.includes(` at ${clock} `), `${iso} ${zone}: ${shown}`);
    assert.match(shown, /^[A-Z][a-z]+ \d{1,2}, \d{4} at \d{1,2}:\d{2} (AM|PM) \S+/, shown);
    assert.ok(!/\b(1[3-9]|2[0-4]):\d{2}\b/.test(shown), `no 24-hour clock: ${shown}`);
  }
});

test('formatWhen: ISO strings, milliseconds, numeric strings and dates read the same; a time with no zone is UTC', () => {
  const same = [WHEN, '2026-10-02T21:14:00.000Z', '2026-10-02T14:14:00-07:00', '2026-10-03T02:44:00+05:30', '2026-10-02T21:14:00', '2026-10-02 21:14', Date.UTC(2026, 9, 2, 21, 14), String(Date.UTC(2026, 9, 2, 21, 14)), new Date(Date.UTC(2026, 9, 2, 21, 14))];
  for (const input of same) assert.equal(formatWhen(input), UTC_WHEN, String(input));
  assert.equal(formatWhen('2026-10-02', 'America/Phoenix'), 'October 1, 2026 at 5:00 PM MST (America/Phoenix)', 'a bare ISO date is midnight UTC');
  // The host's own zone changes nothing.
  const host = process.env.TZ;
  try {
    for (const tz of ['Pacific/Auckland', 'America/Los_Angeles', 'Asia/Kolkata']) {
      process.env.TZ = tz;
      assert.equal(formatWhen(WHEN), UTC_WHEN, tz);
      assert.equal(formatWhen('2026-10-02T21:14:00'), UTC_WHEN, tz);
      assert.equal(formatWhen(WHEN, 'America/Phoenix'), PHOENIX_WHEN, tz);
    }
  } finally { if (host === undefined) delete process.env.TZ; else process.env.TZ = host; }
});

test('formatWhen: a value that is not a time gives a safe placeholder, never throws, never "Invalid Date"', () => {
  for (const bad of [undefined, null, '', ' ', 'soon', 'not a date', 'May', '12', '<script>alert(1)</script>', 'Q&amp;Z', NaN, Infinity, -Infinity, 0, -5, {}, [], true, 10n, Symbol('t'), 8.64e15 + 1, new Date(NaN), '2026-13-45T10:00:00Z', '0000-00-00']) {
    let shown;
    assert.doesNotThrow(() => { shown = formatWhen(bad, 'America/Phoenix'); }, String(typeof bad));
    assert.equal(shown, UNKNOWN_DATE, String(typeof bad));
    assert.ok(!/Invalid|NaN|undefined|null|1970|1969/.test(shown), shown);
  }
  assert.ok(Number.isNaN(parseWhen('soon')) && Number.isNaN(parseWhen(0)) && parseWhen(WHEN) === Date.UTC(2026, 9, 2, 21, 14));
});

test('the verification email states the expiry through formatWhen: month, day, year, 12-hour time, AM or PM and the zone', () => {
  const utc = renderEmailNotice('email_verify', { ...EMAIL, expiresAt: WHEN });
  for (const part of [utc.html, utc.text]) {
    assert.ok(part.includes(`expires on ${UTC_WHEN}`), 'UTC when the account has no zone');
    assert.ok(!part.includes('2026-10-02'), 'not the bare ISO date');
    assert.ok(!/\b\d{2}:\d{2} UTC\b/.test(part), 'not a 24-hour clock');
  }
  const phoenix = renderEmailNotice('email_verify', { ...EMAIL, expiresAt: WHEN, timeZone: 'America/Phoenix' });
  for (const part of [phoenix.html, phoenix.text]) {
    assert.ok(part.includes(`expires on ${PHOENIX_WHEN}`), 'the account zone, named');
    assert.match(part, /expires on October 2, 2026 at 2:14 PM MST \(America\/Phoenix\)/);
    assert.ok(!part.includes('UTC'), 'no bare UTC when a zone is known');
  }
  // Milliseconds read the same, and a zone that is not one is UTC.
  assert.ok(renderEmailNotice('email_verify', { ...EMAIL, expiresAt: Date.parse(WHEN) }).html.includes(UTC_WHEN));
  assert.ok(renderEmailNotice('email_verify', { ...EMAIL, expiresAt: WHEN, timeZone: 'Not/AZone' }).text.includes(`expires on ${UTC_WHEN}`));
  // Not a time: no date is invented.
  for (const bad of [undefined, null, '', 'soon', NaN, 0, '<script>']) {
    const e = renderEmailNotice('email_verify', { ...EMAIL, expiresAt: bad });
    assert.ok(!/Invalid|NaN|undefined|1970|unknown date/.test(e.html + e.text), `no junk for ${String(bad)}`);
    assert.ok(e.text.includes('it works once and expires after a short time'), String(bad));
  }
});

const TIMES = {
  email_verify: ['expiresAt'],
  email_change_existing: ['effectiveAt'],
  email_change_pending: ['effectiveAt'],
  rotation: ['at', 'valueFrozenUntil', 'payoutPinnedUntil'],
  admin_replacement: ['at', 'valueFrozenUntil', 'payoutPinnedUntil', 'undoUntil'],
  'admin_replacement (no undo link)': ['at', 'valueFrozenUntil', 'payoutPinnedUntil'],
  key_restored: ['replacedAt', 'at', 'valueFrozenUntil'],
  admin_action: ['at'],
};
for (const t of TEMPLATES) t.times = TIMES[t.name] || [];

test('every template that shows a time shows it through formatWhen: the account zone when it has one, UTC when not', () => {
  assert.ok(TEMPLATES.filter((t) => t.times.length).length >= 8, 'the templates that show a time');
  for (const t of TEMPLATES) {
    for (const zone of [undefined, 'America/Phoenix', 'America/New_York', 'Asia/Kolkata']) {
      const r = t.render({ ...t.data, timeZone: zone });
      for (const field of t.times) {
        const expected = formatWhen(t.data[field], zone);
        const where = `${t.name}.${field} in ${zone || 'no zone'}`;
        assert.match(expected, /^[A-Z][a-z]+ \d{1,2}, \d{4} at \d{1,2}:\d{2} (AM|PM) \S+/, where);
        assert.ok(zone ? expected.endsWith(`(${zone})`) : expected.endsWith(' UTC'), where);
        assert.ok(r.html.includes(expected), `${where}: in the HTML`);
        assert.ok(r.text.includes(expected), `${where}: in the text`);
      }
      assert.ok(!/\b\d{4}-\d{2}-\d{2}/.test(r.html + r.text), `${t.name}: a bare ISO date is left in`);
      assert.ok(!/\b\d{2}:\d{2} UTC\b/.test(r.html + r.text), `${t.name}: a 24-hour clock is left in`);
      if (zone && t.times.length) assert.ok(!/ UTC\b/.test(r.html + r.text), `${t.name}: a bare UTC beside the zone ${zone}`);
    }
  }
});

test('a hostile zone is not a zone: every template falls back to UTC and prints none of it', () => {
  for (const t of TEMPLATES) {
    for (const payload of [...HOSTILE, 'Not/AZone']) {
      const r = t.render({ ...t.data, timeZone: payload });
      assert.ok(!r.html.includes(payload) && !r.text.includes(payload), `${t.name} <- ${payload}`);
      for (const field of t.times) {
        assert.ok(r.html.includes(formatWhen(t.data[field])) && r.text.includes(formatWhen(t.data[field])), `${t.name}.${field}: UTC`);
      }
    }
  }
});

test('a notice serves its gear from the origin it is given, and from the production host otherwise', () => {
  const STAGING = 'https://eustress-api-staging.workers.dev';
  for (const t of TEMPLATES.filter((x) => x.name !== 'registration')) {
    assert.ok(t.render({ ...t.data, assetOrigin: STAGING }).html.includes(`<img src="${STAGING}/assets/eustress-gear.png" width="80"`), t.name);
    assert.ok(t.render(t.data).html.includes(`<img src="${LOGO}" width="80"`), t.name);
    for (const bad of ['javascript:alert(1)', '"><img src=x onerror=1>', 'https://x.test/path?q=1', 'ftp://x.test', 7, null, '']) {
      const r = t.render({ ...t.data, assetOrigin: bad });
      assert.ok(r.html.includes(`<img src="${LOGO}" width="80"`), `${t.name}: ${String(bad)}`);
      assert.equal((r.html.match(/<img\b/gi) || []).length, 1, t.name);
    }
  }
});

// ── The registration email ───────────────────────────────────────────────────

test('the registration email keeps its look: gear over the wordmark, tagline, social row, nav columns, copyright', () => {
  const r = buildIdentityEmail({ username: 'McKale', toml_content: '# toml\n' });
  const header = r.html.slice(r.html.indexOf('<!-- eustress-email:header -->'), r.html.indexOf('<!-- eustress-email:body -->'));
  assert.ok(header.indexOf('<img') < header.indexOf('Eustress Engine'), 'the gear above the wordmark');
  assert.ok(header.includes('Identity Registration'));
  assert.match(r.html, /<title>Eustress Identity Registration<\/title>/);
  assert.ok(r.html.includes(`<img src="${LOGO}" width="80" height="80" alt=""`), 'the production hostname by default');
  for (const word of ['Welcome, <strong>McKale</strong>.', 'eustress-McKale.toml', 'Your Identity File', 'How to use your registration', 'Never share your', 'X</a>', 'Discord</a>', 'GitHub</a>', 'Product', 'Community', 'Company', 'Legal', 'Press Kit']) {
    assert.ok(r.html.includes(word), word);
  }
  assert.deepEqual(hrefsOf(r.html).filter((h) => !FOOTER_LINKS.includes(h)), [`${SITE}/login`]);
  assert.equal(r.subject, 'Your Eustress Identity Registration: McKale');
  assert.deepEqual(r.attachment, { filename: 'eustress-McKale.toml', mime: 'application/toml', content: '# toml\n' });
  assert.ok(r.text.includes('Sign in at https://eustress.dev/login'));
});

test('the registration email serves its gear from the host it was reached on, and never prints the file or its key', () => {
  const toml = 'private_key = "1111111111111111111111111111111111111111111111111111111111111111"\n';
  const dev = buildIdentityEmail({ username: 'a', toml_content: toml, host: 'https://eustress-api-dev.workers.dev' });
  assert.ok(dev.html.includes('<img src="https://eustress-api-dev.workers.dev/assets/eustress-gear.png"'));
  for (const bad of ['javascript:alert(1)', '"><img src=x>', 'https://x.test/path?q=1', 'ftp://x.test']) {
    assert.ok(buildIdentityEmail({ username: 'a', toml_content: toml, host: bad }).html.includes(`<img src="${LOGO}"`), bad);
  }
  assert.ok(!dev.html.includes('1111111111') && !dev.text.includes('1111111111'), 'the key rides in the attachment only');
  const anonymous = buildIdentityEmail({ username: '', toml_content: toml });
  assert.ok(anonymous.html.includes('eustress-Creator.toml'));
});

// ── Dark mode and no-CSS clients ─────────────────────────────────────────────

test('every block has an explicit background and every text block an explicit colour', () => {
  for (const t of TEMPLATES) {
    const r = t.render(t.data);
    assert.match(r.html, /<meta name="supported-color-schemes" content="dark">/, t.name);
    const bodyTag = /<body\b[^>]*>/.exec(r.html)[0];
    assert.match(bodyTag, /\sbgcolor="#[0-9a-f]{6}"/i, `${t.name}: body bgcolor`);
    assert.match(bodyTag, /\stext="#[0-9a-f]{6}"/i, `${t.name}: body text colour attribute`);
    assert.match(bodyTag, /background-color:#[0-9a-f]{6}/i, `${t.name}: body background`);
    assert.match(bodyTag, /[;"]color:#[0-9a-f]{6}/i, `${t.name}: body colour`);
    const blocks = r.html.match(/<(table|td)\b[^>]*>/gi) || [];
    assert.ok(blocks.length >= 7, `${t.name}: the shell tables and cells are all there`);
    for (const tag of blocks) {
      assert.match(tag, /\sbgcolor="#[0-9a-f]{6}"/i, `${t.name}: ${tag.slice(0, 90)} has no bgcolor attribute`);
      assert.match(tag, /background-color:#[0-9a-f]{6}/i, `${t.name}: ${tag.slice(0, 90)} has no background-color`);
    }
    const texts = r.html.match(/<(p|div|li|a|span|code|h1|h2|ul|ol)\b[^>]*>/gi) || [];
    assert.ok(texts.length >= 8, `${t.name}: text blocks are all there`);
    for (const tag of texts) assert.match(tag, /[;"]color:#[0-9a-f]{3,6}/i, `${t.name}: ${tag.slice(0, 90)} has no text colour`);
  }
});

test('the shell, the plain footer and every template are ASCII, so a part labelled 7bit is valid', () => {
  for (const t of TEMPLATES) {
    const r = t.render(t.data);
    assert.ok(/^[\x00-\x7f]*$/.test(r.html), `${t.name}: HTML`);
    assert.ok(/^[\x00-\x7f]*$/.test(r.text), `${t.name}: text`);
    assert.ok(!/\u2014/.test(r.html + r.text + r.subject), `${t.name}: no em dash`);
  }
});

// ── The message ──────────────────────────────────────────────────────────────

test('a notice goes out as both parts, text first, each carrying what the template made', () => {
  for (const t of TEMPLATES.filter((x) => x.name !== 'registration')) {
    const r = t.render(t.data);
    const raw = buildNoticeMime({ from: 'identity@eustress.dev', to: 'owner@example.com', ...r });
    const parts = [...raw.matchAll(/Content-Type: (text\/(?:plain|html)); charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n([A-Za-z0-9+/=\r\n]+?)(?=\r\n--)/g)]
      .map((m) => [m[1], Buffer.from(m[2].replace(/\r\n/g, ''), 'base64').toString()]);
    assert.deepEqual(parts.map((p) => p[0]), ['text/plain', 'text/html'], t.name);
    assert.equal(parts[0][1], r.text, t.name);
    assert.equal(parts[1][1], r.html, t.name);
  }
});

// ── No template is added without the shell ───────────────────────────────────

test('every notice kind is listed and renders from the shell; an unlisted kind is refused', () => {
  const sample = { ...EMAIL, ...NOTICE };
  for (const kind of EMAIL_NOTICE_KINDS) {
    assert.ok(TEMPLATES.some((t) => t.name === kind), `${kind} has a template in this test`);
    assert.ok(renderEmailNotice(kind, sample).html.includes('<!-- eustress-email:header -->'), kind);
  }
  for (const kind of KEY_NOTICE_KINDS) {
    assert.ok(TEMPLATES.some((t) => t.name === kind), `${kind} has a template in this test`);
    assert.ok(renderKeyNotice(kind, sample).html.includes('<!-- eustress-email:header -->'), kind);
  }
  assert.throws(() => renderEmailNotice('email_unlisted', sample), /unknown notice kind/);
  assert.throws(() => renderKeyNotice('unlisted', sample), /unknown notice kind/);
  assert.ok(TEMPLATES.some((t) => t.name === 'admin_action') && TEMPLATES.some((t) => t.name === 'registration'));
});

test('no source file builds an HTML document of its own, except the shell and the builders other work owns', () => {
  // Owned elsewhere (listings and moderation mail). They are listed so a new
  // account email cannot be added beside them without the shell.
  const ELSEWHERE = new Set(['moderation.mjs', 'moderation_email.mjs']);
  const own = [];
  for (const file of fs.readdirSync(SRC)) {
    if (!/\.(m?js)$/.test(file)) continue;
    const source = fs.readFileSync(path.join(SRC, file), 'utf8');
    if (/<!doctype html/i.test(source) && file !== 'email_shell.mjs' && !ELSEWHERE.has(file)) own.push(file);
  }
  assert.deepEqual(own, [], 'an HTML document outside the shell');
  for (const file of ['account_notices.mjs', 'identity_email.mjs']) {
    const source = fs.readFileSync(path.join(SRC, file), 'utf8');
    assert.ok(!/<html|<body|<head/i.test(source), `${file} builds a body only`);
    assert.match(source, /email_shell\.mjs/, `${file} imports the shell`);
  }
});
