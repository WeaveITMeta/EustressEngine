// The email an author receives when their listing is decided: listed, not
// listed, changes requested, or an appeal decided. Rendered ONLY from the
// author view (moderation.mjs authorView), so nothing internal can reach an
// inbox: no probabilities, thresholds, reviewer ids or policy text.
//
// Owner of the wording and the HTML: the Website session. Owner of the
// sending path and the headers: moderation. The contract between them is
// renderModerationEmail's input and output; change the copy freely, but keep
// every value it prints coming from `view`, `simName` and the two URLs.

const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
// Header-safe: a listing name is author-supplied, and a CR or LF in a
// Subject line would let it write its own headers.
const oneLine = (s, n = 120) => String(s ?? '').replace(/[\r\n\t]+/g, ' ').replace(/[^\x20-\x7e -￿]/g, '').trim().slice(0, n);

const SUBJECTS = {
  listed: name => `Your Universe "${name}" is live in the Gallery`,
  not_listed: name => `"${name}" was not listed in the Gallery`,
  changes_requested: name => `Changes needed before "${name}" can be listed`,
  appeal_listed: name => `Appeal accepted: "${name}" is now listed`,
  appeal_upheld: name => `Your appeal for "${name}" was reviewed`,
};

export function renderModerationEmail({ view, kind, simName, username, reviewUrl, unsubUrl }) {
  const name = oneLine(simName || 'your Universe', 80);
  const key = kind === 'appeal_decided'
    ? (view.status === 'listed' ? 'appeal_listed' : 'appeal_upheld')
    : view.status;
  const subject = oneLine((SUBJECTS[key] || SUBJECTS.not_listed)(name), 150);
  const hello = username ? `Hi ${oneLine(username, 40)},` : 'Hi,';

  const lines = [hello, '', view.headline, ''];
  if (view.status === 'listed' && view.rating) lines.push(`Age rating: ${RATING_LABEL[view.rating] || view.rating}`, '');
  for (const r of view.reasons || []) {
    lines.push(`* ${r.title}`);
    if (r.why) lines.push(`  Why: ${r.why}`);
    if (r.what_to_change) lines.push(`  What to change: ${r.what_to_change}`);
    lines.push('');
  }
  if (view.suggested_edit) lines.push(`Suggested edit: ${view.suggested_edit}`, '');
  if (view.note) lines.push(view.note, '');
  if (view.can_appeal) lines.push('If you think this is a mistake, you can appeal from your projects page.', '');
  lines.push(`Review details: ${reviewUrl}`, '', '--', 'Eustress Gallery', `Stop these emails: ${unsubUrl}`);
  const text = lines.join('\n');

  const reasonsHtml = (view.reasons || []).map(r => `
      <li style="margin:0 0 12px;"><strong>${esc(r.title)}</strong>${r.why ? `<br><span style="color:#8b949e;">Why:</span> ${esc(r.why)}` : ''}${r.what_to_change ? `<br><span style="color:#8b949e;">What to change:</span> ${esc(r.what_to_change)}` : ''}</li>`).join('');
  const html = `<!doctype html><html><body style="margin:0;padding:24px;background:#0d1117;color:#c9d1d9;font-family:Segoe UI,Helvetica,Arial,sans-serif;">
  <div style="max-width:560px;margin:0 auto;background:#161b22;border:1px solid #30363d;border-radius:8px;padding:24px;">
    <p style="margin:0 0 12px;">${esc(hello)}</p>
    <p style="margin:0 0 16px;font-size:16px;color:#f0f6fc;">${esc(view.headline)}</p>
    ${view.status === 'listed' && view.rating ? `<p style="margin:0 0 16px;">Age rating: ${esc(RATING_LABEL[view.rating] || view.rating)}</p>` : ''}
    ${reasonsHtml ? `<ul style="padding-left:18px;margin:0 0 16px;">${reasonsHtml}</ul>` : ''}
    ${view.suggested_edit ? `<p style="margin:0 0 16px;"><span style="color:#8b949e;">Suggested edit:</span> ${esc(view.suggested_edit)}</p>` : ''}
    ${view.note ? `<p style="margin:0 0 16px;">${esc(view.note)}</p>` : ''}
    ${view.can_appeal ? '<p style="margin:0 0 16px;">If you think this is a mistake, you can appeal from your projects page.</p>' : ''}
    <p style="margin:0 0 24px;"><a href="${esc(reviewUrl)}" style="color:#00bcd4;">Review details</a></p>
    <p style="margin:0;font-size:12px;color:#8b949e;">Eustress Gallery. <a href="${esc(unsubUrl)}" style="color:#8b949e;">Stop these emails</a>.</p>
  </div></body></html>`;
  return { subject, text, html };
}

const RATING_LABEL = { all_ages: 'All ages', teen_13: 'Teen (13+)', mature_17: 'Mature (17+)', adult_18: 'Adults only (18+)' };

function b64Utf8(s) {
  const bytes = new TextEncoder().encode(s);
  let bin = '';
  for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
  return btoa(bin).replace(/(.{76})/g, '$1\r\n');
}

// RFC 5322 multipart/alternative with RFC 8058 one-click unsubscribe. Bodies
// are base64 UTF-8 so a non-ASCII listing name can never break 7bit transport.
export function buildModerationMime({ from, fromName, to, subject, text, html, unsubUrl, messageId }) {
  const boundary = '----=_eustress_mod_' + crypto.randomUUID().replace(/-/g, '');
  const encSubject = /^[\x20-\x7e]*$/.test(subject) ? subject : `=?UTF-8?B?${b64Utf8(subject).replace(/\r\n/g, '')}?=`;
  return [
    `From: ${oneLine(fromName, 60)} <${from}>`,
    `To: ${to}`,
    `Subject: ${encSubject}`,
    `Message-ID: <${messageId}>`,
    'MIME-Version: 1.0',
    `List-Unsubscribe: <${unsubUrl}>`,
    'List-Unsubscribe-Post: List-Unsubscribe=One-Click',
    `Content-Type: multipart/alternative; boundary="${boundary}"`,
    '',
    `--${boundary}`,
    'Content-Type: text/plain; charset=utf-8',
    'Content-Transfer-Encoding: base64',
    '',
    b64Utf8(text),
    `--${boundary}`,
    'Content-Type: text/html; charset=utf-8',
    'Content-Transfer-Encoding: base64',
    '',
    b64Utf8(html),
    `--${boundary}--`,
    '',
  ].join('\r\n');
}
