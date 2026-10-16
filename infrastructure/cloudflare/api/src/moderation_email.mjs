// The email an author receives when their listing is decided: listed, not
// listed, changes requested, removed, or an appeal decided. Rendered ONLY from the
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
  removed: name => `"${name}" was removed from Eustress`,
  appeal_listed: name => `Appeal accepted: "${name}" is now listed`,
  appeal_upheld: name => `Your appeal for "${name}" was reviewed`,
};

/// A link only if it is http or https; anything else becomes "#". Both URLs
/// come from the Worker, so this is a guard against a future caller, not a
/// filter on today's input.
const safeUrl = u => (/^https?:\/\/[^\s<>"']+$/.test(String(u ?? '')) ? String(u) : '#');

/// How the status reads in the chip at the top of the email.
const STATUS_CHIP = {
  listed:            { label: "Listed",            color: "#81c784", bg: "#12261a" },
  approved_private:  { label: "Private",           color: "#81c784", bg: "#12261a" },
  not_listed:        { label: "Not listed",        color: "#ef9a9a", bg: "#2a1517" },
  changes_requested: { label: "Changes requested", color: "#ffcc80", bg: "#2b2113" },
  removed:           { label: "Removed",           color: "#ef9a9a", bg: "#2a1517" },
  appeal_listed:     { label: "Appeal accepted",   color: "#81c784", bg: "#12261a" },
  appeal_upheld:     { label: "Appeal reviewed",   color: "#9fb0c0", bg: "#1b2430" },
};

export function renderModerationEmail({ view, kind, simName, username, reviewUrl, unsubUrl }) {
  const name = oneLine(simName || "your Universe", 80);
  const key = kind === "appeal_decided"
    ? (view.status === "listed" ? "appeal_listed" : "appeal_upheld")
    : view.status;
  const subject = oneLine((SUBJECTS[key] || SUBJECTS.not_listed)(name), 150);
  const hello = username ? `Hi ${oneLine(username, 40)},` : "Hi,";
  const review = safeUrl(reviewUrl);
  const unsub = safeUrl(unsubUrl);
  const rating = view.status === "listed" && view.rating ? (RATING_LABEL[view.rating] || view.rating) : null;
  const appealHint = "If you think this is a mistake, you can appeal. Open the review page and use the appeal form.";

  // Plain text: the same facts, in the order a person reads them.
  const lines = [hello, "", view.headline, ""];
  if (rating) lines.push(`Age rating: ${rating}`, "");
  for (const r of view.reasons || []) {
    lines.push(`* ${r.title}`);
    if (r.why) lines.push(`  Why: ${r.why}`);
    if (r.what_to_change) lines.push(`  What to change: ${r.what_to_change}`);
    lines.push("");
  }
  if (view.suggested_edit) lines.push(`Suggested edit: ${view.suggested_edit}`, "");
  // The tags the review chose because the author gave none: one line (Guardian Code v2.3 Part III).
  if (view.tags_chosen) lines.push(oneLine(view.tags_chosen, 300), "");
  if (view.note) lines.push(view.note, "");
  if (view.can_appeal) lines.push(appealHint, "");
  if (view.reference) lines.push(`Case reference: ${oneLine(view.reference, 40)}`, "");
  lines.push(`Review details: ${review}`, "", "--", "Eustress Gallery", `Stop these emails: ${unsub}`);
  const text = lines.join("\n");

  // HTML: tables and inline styles, because mail clients ignore most else.
  // Every value printed is escaped.
  const chip = STATUS_CHIP[key] || STATUS_CHIP.appeal_upheld;
  const label = "font-size:11px;font-weight:700;letter-spacing:0.06em;text-transform:uppercase;color:#8b949e;";
  const block = (inner) => `<tr><td style="padding:0 28px 16px;">${inner}</td></tr>`;
  const reasonsHtml = (view.reasons || []).map(r => block(`
        <div style="background:#0d1117;border:1px solid #30363d;border-radius:8px;padding:14px 16px;">
          <div style="font-size:15px;font-weight:700;color:#f0f6fc;">${esc(r.title)}</div>
          ${r.why ? `<div style="margin-top:8px;"><div style="${label}">Why</div><div style="font-size:14px;line-height:1.55;color:#c9d1d9;">${esc(r.why)}</div></div>` : ""}
          ${r.what_to_change ? `<div style="margin-top:8px;"><div style="${label}">What to change</div><div style="font-size:14px;line-height:1.55;color:#c9d1d9;">${esc(r.what_to_change)}</div></div>` : ""}
        </div>`)).join("");
  const html = `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="dark light"><meta name="supported-color-schemes" content="dark light"><title>${esc(subject)}</title></head>
<body style="margin:0;padding:0;background:#0d1117;">
  <div style="display:none;max-height:0;overflow:hidden;opacity:0;mso-hide:all;">${esc(view.headline)}</div>
  <table role="presentation" width="100%" cellspacing="0" cellpadding="0" style="background:#0d1117;"><tr><td align="center" style="padding:24px 12px;">
    <table role="presentation" width="100%" cellspacing="0" cellpadding="0" style="max-width:560px;background:#161b22;border:1px solid #30363d;border-radius:10px;font-family:Segoe UI,Helvetica,Arial,sans-serif;color:#c9d1d9;">
      <tr><td style="padding:22px 28px 6px;font-size:13px;font-weight:800;letter-spacing:0.14em;color:#f0f6fc;">EUSTRESS</td></tr>
      <tr><td style="padding:10px 28px 4px;"><span style="display:inline-block;padding:3px 10px;border-radius:999px;background:${chip.bg};color:${chip.color};font-size:11px;font-weight:700;letter-spacing:0.06em;text-transform:uppercase;">${esc(chip.label)}</span></td></tr>
      <tr><td style="padding:10px 28px 6px;font-size:14px;color:#8b949e;">${esc(hello)}</td></tr>
      <tr><td style="padding:0 28px 16px;font-size:20px;line-height:1.35;font-weight:700;color:#f0f6fc;">${esc(view.headline)}</td></tr>
      ${rating ? block(`<span style="font-size:14px;color:#c9d1d9;">Age rating: <strong style="color:#f0f6fc;">${esc(rating)}</strong></span>`) : ""}
      ${reasonsHtml}
      ${view.suggested_edit ? block(`<div style="${label}">Suggested edit</div><div style="font-size:14px;line-height:1.55;color:#c9d1d9;">${esc(view.suggested_edit)}</div>`) : ""}
      ${view.tags_chosen ? block(`<div style="font-size:14px;line-height:1.55;color:#c9d1d9;">${esc(oneLine(view.tags_chosen, 300))}</div>`) : ""}
      ${view.note ? block(`<div style="font-size:14px;line-height:1.55;color:#c9d1d9;">${esc(view.note)}</div>`) : ""}
      ${view.can_appeal ? block(`<div style="font-size:14px;line-height:1.55;color:#c9d1d9;">${esc(appealHint)}</div>`) : ""}
      ${view.reference ? block(`<span style="font-size:13px;color:#8b949e;">Case reference: <strong style="color:#c9d1d9;">${esc(oneLine(view.reference, 40))}</strong></span>`) : ""}
      <tr><td style="padding:8px 28px 24px;"><a href="${esc(review)}" style="display:inline-block;padding:11px 20px;border-radius:8px;background:#3cba54;color:#0d1117;font-size:14px;font-weight:700;text-decoration:none;">See the review details</a></td></tr>
      <tr><td style="padding:14px 28px 22px;border-top:1px solid #30363d;font-size:12px;line-height:1.5;color:#8b949e;">Eustress Gallery. You get this because you published a simulation. <a href="${esc(unsub)}" style="color:#8b949e;">Stop these emails</a>.</td></tr>
    </table>
  </td></tr></table>
</body></html>`;
  return { subject, text, html };
}

// Eustress's own age bands (Guardian Code v2.2 Part V), named by age, not after any outside rating board.
const RATING_LABEL = { all_ages: 'All ages', ages_10: 'Ages 10+', teen_13: 'Ages 13+', mature_17: 'Ages 17+', adult_18: 'Adults only (18+)', adult_21: 'Adults only (21+)' };

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
