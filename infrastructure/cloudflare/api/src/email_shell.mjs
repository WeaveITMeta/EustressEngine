// =============================================================================
// The shell every Eustress HTML email is built in
// =============================================================================
//
// One header, one footer, one palette, one plain-text twin. The registration
// email and every account email (verification, change of address, key notices,
// the administrators' summary) fill the body slot of `renderEmail` and nothing
// else, so they cannot drift apart.
//
// Brand cues match `crates/web/src/components/footer.rs`: industrial dark theme,
// the gear above the centred "EUSTRESS ENGINE" wordmark, the tagline, a social
// row, four nav columns, a copyright line. The gear is a hosted PNG
// (`/assets/eustress-gear.png`, served by this Worker): Gmail strips inline
// `<svg>`, so PNG-by-URL is the only reliable header image.
//
// Rules the shell holds for every email built in it:
//   * One remote image: the header gear, a static asset with no query string.
//     No tracking pixel, no beacon, no tracking query parameter, no script.
//   * Everything a caller gives the shell is escaped. Body markup is built with
//     the `html` tag, which escapes every interpolated value unless it is
//     itself markup the tag or a helper here produced; the shell refuses a
//     body that is a plain string.
//   * The plain-text part is required. The shell refuses an email with no twin.
//   * Dark-mode inversion safe: `color-scheme` is declared, and every table,
//     cell and text block carries an explicit background or text colour (as an
//     attribute where the client may ignore CSS).
//   * Two footers. `full` carries the social row and nav columns (links to the
//     public site). `plain` carries the wordmark, tagline and copyright and no
//     link at all, for notices about an account's key or address, where a link
//     in the email is what a phisher sends (docs/architecture/
//     IDENTITY_KEY_ROTATION_AND_RECOVERY.md, "The notice rule").
// =============================================================================

/// The production hostname. A caller that was reached on another host (the
/// registration email, on a workers.dev URL) passes its own `assetOrigin`.
export const ASSET_ORIGIN = 'https://api.eustress.dev';
export const LOGO_PATH = '/assets/eustress-gear.png';
export const TAGLINE = 'Creation at the speed of thought.';

// ─── Escaping and markup ────────────────────────────────────────────────────

const ESCAPES = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
export const escapeHtml = (value) => String(value ?? '').replace(/[&<>"']/g, (c) => ESCAPES[c]);

/// Markup that is already safe: produced by the `html` tag or a helper below.
/// Only this class passes through the escaping unchanged.
class Markup {
  constructor(value) { this.value = value; }
  toString() { return this.value; }
}
const markup = (value) => new Markup(value);

/// A value as it goes into HTML: markup as it is, a list joined, `false`, `null`
/// and `undefined` as nothing, anything else escaped.
function part(value) {
  if (value instanceof Markup) return value.value;
  if (Array.isArray(value)) return value.map(part).join('');
  if (value === false || value === null || value === undefined) return '';
  return escapeHtml(value);
}

/// Tagged template for body markup. The literal parts are the author's markup;
/// every `${value}` is escaped unless it is markup from this tag or a helper.
///   html`<p>Hello ${username}</p>`
export function html(strings, ...values) {
  let out = strings[0];
  for (let i = 0; i < values.length; i++) out += part(values[i]) + strings[i + 1];
  return markup(out);
}

/// A link target that is only ever a web or mail address.
const safeHref = (value) => (/^(https?:\/\/|mailto:)/i.test(String(value ?? '')) ? String(value) : '#');

// ─── Times ──────────────────────────────────────────────────────────────────
// An email cannot know the reader's zone, so a time is shown in the zone the
// account saved (`timeZone`, an IANA name) with the zone named beside it, or in
// UTC when there is none. Always month name, day, year, a 12-hour clock with
// AM or PM, and the zone; never a bare date, never a 24-hour clock.

/// What is shown for a value that is not a time.
export const UNKNOWN_DATE = 'an unknown date';

const ISO_TIME = /^\d{4}-\d{2}-\d{2}(?:[T ]\d{2}:\d{2}(?::\d{2}(?:\.\d{1,9})?)?(?:Z|[+-]\d{2}:?\d{2})?)?$/;

/// The instant, in milliseconds, of an ISO 8601 string, a number of
/// milliseconds, a numeric string or a Date. NaN for anything else (no loose
/// date words), and for zero or less (an unset time).
export function parseWhen(value) {
  let ms;
  if (value instanceof Date) ms = value.getTime();
  else if (typeof value === 'number') ms = value;
  else if (typeof value === 'string' && /^\d{10,}$/.test(value.trim())) ms = Number(value.trim());
  else if (typeof value === 'string' && ISO_TIME.test(value.trim())) {
    // A time with no zone is UTC, not the host's.
    let iso = value.trim().replace(' ', 'T');
    if (/T\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?$/.test(iso)) iso += 'Z';
    ms = Date.parse(iso);
  } else ms = NaN;
  return Number.isFinite(ms) && ms > 0 ? ms : NaN;
}

const ZONE_NAME = /^[A-Za-z0-9_+:\/-]{1,64}$/;

/// `timeZone` as an IANA name Intl accepts, or null. Never throws. The name is
/// printed as the account spelled it, with only its letter case taken from Intl
/// ("america/phoenix" reads "America/Phoenix"); Intl's own canonical name is not
/// used, since it can be an older alias ("Asia/Calcutta" for "Asia/Kolkata").
function knownZone(timeZone) {
  if (typeof timeZone !== 'string') return null;
  const zone = timeZone.trim();
  if (!ZONE_NAME.test(zone)) return null;
  try {
    const resolved = new Intl.DateTimeFormat('en-US', { timeZone: zone }).resolvedOptions().timeZone;
    return resolved.toLowerCase() === zone.toLowerCase() ? resolved : zone;
  } catch {
    return null;
  }
}

/// `October 2, 2026 at 2:14 PM MST (America/Phoenix)` from an ISO string or a
/// number of milliseconds and an optional IANA zone. With no zone, or a zone
/// Intl rejects, `October 2, 2026 at 9:14 PM UTC`. A value that is not a time
/// gives `UNKNOWN_DATE`. Never throws. The text is assembled from the parts
/// Intl returns, so it does not depend on a locale's punctuation.
export function formatWhen(when, timeZone) {
  const ms = parseWhen(when);
  if (Number.isNaN(ms)) return UNKNOWN_DATE;
  const zone = knownZone(timeZone);
  try {
    const parts = new Intl.DateTimeFormat('en-US', {
      timeZone: zone || 'UTC', month: 'long', day: 'numeric', year: 'numeric', hour: 'numeric', minute: '2-digit', hour12: true, timeZoneName: 'short',
    }).formatToParts(new Date(ms));
    const get = (type) => parts.find((p) => p.type === type)?.value;
    const [month, day, year, hour, minute, period, name] = ['month', 'day', 'year', 'hour', 'minute', 'dayPeriod', 'timeZoneName'].map(get);
    if (![month, day, year, hour, minute, period, name].every(Boolean)) return UNKNOWN_DATE;
    const stamp = `${month} ${day}, ${year} at ${hour}:${minute} ${period.toUpperCase()} ${name}`;
    return zone && zone !== name ? `${stamp} (${zone})` : stamp;
  } catch {
    return UNKNOWN_DATE;
  }
}

// ─── Palette ────────────────────────────────────────────────────────────────
// Every colour the shell and its helpers use. Written out in each style so a
// client that drops CSS keeps the attribute (`bgcolor`) and a client that
// inverts colours finds an explicit value on every block.

const PAGE = '#0d1117';
const CARD = '#161b22';

const SOCIAL = [
  ['X', 'https://twitter.com/eustressengine'],
  ['Discord', 'https://discord.gg/DGP9my8DYN'],
  ['GitHub', 'https://github.com/WeaveITMeta/EustressEngine'],
];

const COLUMNS = [
  ['Product', [['Gallery', 'https://eustress.dev/gallery'], ['Learn', 'https://eustress.dev/learn'], ['Bliss', 'https://eustress.dev/bliss'], ['Premium', 'https://eustress.dev/premium']]],
  ['Community', [['Groups', 'https://eustress.dev/groups'], ['Discord', 'https://discord.gg/DGP9my8DYN'], ['X', 'https://x.com/search?q=%23EustressEngine'], ['Forums', 'https://x.com/simbuilder']]],
  ['Company', [['About', 'https://eustress.dev/about'], ['Careers', 'https://eustress.dev/careers'], ['Contact', 'https://eustress.dev/contact'], ['Press Kit', 'https://eustress.dev/press']]],
  ['Legal', [['Terms', 'https://eustress.dev/terms'], ['Privacy', 'https://eustress.dev/privacy'], ['Cookies', 'https://eustress.dev/cookies'], ['Acts', 'https://eustress.dev/acts']]],
];

/// Every address the `full` footer links to.
export const FOOTER_LINKS = [...SOCIAL, ...COLUMNS.flatMap(([, links]) => links)].map(([, url]) => url);

// ─── Body helpers ───────────────────────────────────────────────────────────
// Each returns markup. Text given to them is escaped; markup passes through.

/// A paragraph of body copy.
export const paragraph = (content) => markup(`<p style="color:#c9d1d9;font-size:14px;margin:0 0 16px;line-height:1.6;">${part(content)}</p>`);

/// A quieter paragraph: the fine print under the main copy.
export const note = (content) => markup(`<p style="color:#8b949e;font-size:13px;margin:0 0 16px;line-height:1.6;">${part(content)}</p>`);

/// Bold text that keeps the surrounding colour.
export const strong = (content) => markup(`<b>${part(content)}</b>`);

/// A link in body copy. Only http, https and mailto targets are linked.
export const anchor = (href, label) => markup(`<a href="${escapeHtml(safeHref(href))}" style="color:#58a6ff;text-decoration:none;border-bottom:1px solid #2a4a6a;">${part(label)}</a>`);

/// Inline monospace.
export const code = (content) => markup(`<code style="background-color:#21262d;padding:2px 6px;border-radius:4px;color:#79c0ff;font-family:'SF Mono',Consolas,Monaco,monospace;font-size:12px;">${part(content)}</code>`);

/// A bulleted list; each item is text or markup.
export const bullets = (items) => markup(`<ul style="color:#c9d1d9;font-size:14px;margin:0 0 16px;padding-left:20px;line-height:1.7;">${items.filter((i) => i !== false && i !== null && i !== undefined && i !== '').map((i) => `<li style="color:#c9d1d9;font-size:14px;">${part(i)}</li>`).join('')}</ul>`);

/// Label and value rows in a bordered panel, values in monospace. Used for
/// fingerprints and the administrators' summary.
export const facts = (rows) => markup(`<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" bgcolor="${PAGE}" style="margin:0 0 20px;background-color:${PAGE};border:1px solid #30363d;border-radius:8px;">
<tr><td bgcolor="${PAGE}" style="padding:14px 18px;background-color:${PAGE};">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" bgcolor="${PAGE}" style="background-color:${PAGE};">
${rows.map(([label, value]) => `<tr><td valign="top" bgcolor="${PAGE}" style="padding:4px 14px 4px 0;background-color:${PAGE};color:#8b949e;font-size:12px;letter-spacing:0.04em;white-space:nowrap;">${part(label)}</td><td valign="top" bgcolor="${PAGE}" style="padding:4px 0;background-color:${PAGE};color:#c9d1d9;font-size:13px;font-family:'SF Mono',Consolas,Monaco,monospace;word-break:break-all;">${part(value)}</td></tr>`).join('\n')}
</table>
</td></tr>
</table>`);

/// A table-based button that works without CSS support: the colour is a
/// `bgcolor` on the cell and the padding is the cell's. The address is the
/// `href` exactly, escaped for the attribute.
export const button = (href, label) => markup(`<table role="presentation" align="center" cellpadding="0" cellspacing="0" border="0" bgcolor="${CARD}" style="margin:8px auto 18px;background-color:${CARD};">
<tr><td align="center" bgcolor="#1f6feb" style="background-color:#1f6feb;border-radius:8px;padding:14px 32px;"><a href="${escapeHtml(safeHref(href))}" target="_blank" style="display:inline-block;color:#ffffff;font-size:15px;font-weight:700;letter-spacing:0.02em;line-height:1.2;text-decoration:none;"><font color="#ffffff">${part(label)}</font></a></td></tr>
</table>`);

/// The address spelled out, for a mail client that does not draw the button.
export const urlLine = (lead, url) => markup(`<p style="color:#8b949e;font-size:12px;margin:0 0 16px;line-height:1.6;text-align:center;">${part(lead)}<br><span style="color:#79c0ff;font-family:'SF Mono',Consolas,Monaco,monospace;font-size:12px;word-break:break-all;">${part(url)}</span></p>`);

// ─── The shell ──────────────────────────────────────────────────────────────

const headerBlock = ({ iconUrl, label }) => `
          <!-- eustress-email:header -->
          <tr>
            <td align="center" bgcolor="#141b2d" style="background-color:#141b2d;background-image:linear-gradient(135deg,#1a1f2e 0%,#0f1729 50%,#16213e 100%);padding:40px 32px 32px;text-align:center;border-bottom:1px solid #30363d;">
              <img src="${iconUrl}" width="80" height="80" alt="" style="display:block;margin:0 auto 18px;border:0;outline:none;text-decoration:none;">
              <div style="color:#f0f6fc;font-size:18px;font-weight:700;letter-spacing:0.12em;text-transform:uppercase;line-height:1;">Eustress Engine</div>${label ? `
              <div style="color:#8b949e;font-size:12px;margin-top:8px;letter-spacing:0.02em;">${label}</div>` : ''}
            </td>
          </tr>`;

const socialBlock = () => `
          <tr>
            <td align="center" bgcolor="${PAGE}" style="padding:30px 32px 14px;border-top:1px solid #30363d;background-color:${PAGE};text-align:center;">
              <div style="color:#f0f6fc;font-size:13px;font-weight:700;letter-spacing:0.10em;text-transform:uppercase;margin:0 0 8px;">Eustress Engine</div>
              <div style="color:#6e7681;font-size:12px;margin:0 0 18px;">${TAGLINE}</div>
              <div style="color:#c9d1d9;font-size:12px;">${SOCIAL.map(([name, url]) => `<a href="${escapeHtml(url)}" style="color:#c9d1d9;font-size:12px;text-decoration:none;letter-spacing:0.02em;padding:0 10px;">${name}</a>`).join('\n                <span style="color:#30363d;">&middot;</span>\n                ')}
              </div>
            </td>
          </tr>`;

const columnsBlock = () => `
          <tr>
            <td bgcolor="${PAGE}" style="padding:8px 8px 24px;background-color:${PAGE};">
              <table role="presentation" align="center" cellpadding="0" cellspacing="0" border="0" bgcolor="${PAGE}" style="margin:0 auto;background-color:${PAGE};">
                <tr>${COLUMNS.map(([heading, links]) => `
                  <td valign="top" align="center" bgcolor="${PAGE}" style="padding:0 10px;background-color:${PAGE};">
                    <p style="color:#8b949e;font-size:10px;font-weight:700;text-transform:uppercase;letter-spacing:0.10em;margin:0 0 8px;text-align:center;">${heading}</p>${links.map(([name, url], i) => `
                    <p style="margin:0${i === links.length - 1 ? '' : ' 0 6px'};text-align:center;color:#c9d1d9;"><a href="${escapeHtml(url)}" style="color:#c9d1d9;text-decoration:none;font-size:12px;">${name}</a></p>`).join('')}
                  </td>`).join('')}
                </tr>
              </table>
            </td>
          </tr>`;

const plainFooterBlock = () => `
          <tr>
            <td align="center" bgcolor="${PAGE}" style="padding:30px 32px 18px;border-top:1px solid #30363d;background-color:${PAGE};text-align:center;">
              <div style="color:#f0f6fc;font-size:13px;font-weight:700;letter-spacing:0.10em;text-transform:uppercase;margin:0 0 8px;">Eustress Engine</div>
              <div style="color:#6e7681;font-size:12px;margin:0;">${TAGLINE}</div>
            </td>
          </tr>`;

const copyrightBlock = (year) => `
          <tr>
            <td align="center" bgcolor="${PAGE}" style="padding:14px 32px 22px;background-color:${PAGE};border-top:1px solid #30363d;text-align:center;">
              <span style="color:#6e7681;font-size:11px;">&copy; ${year} Eustress Engine. All rights reserved.</span>
            </td>
          </tr>`;

const textFooter = (footer, year) => [
  '--',
  'Eustress Engine',
  TAGLINE,
  ...(footer === 'full'
    ? [
      '',
      ...SOCIAL.map(([name, url]) => `${name}: ${url}`),
      ...COLUMNS.flatMap(([heading, links]) => ['', heading, ...links.map(([name, url]) => `  ${name}: ${url}`)]),
    ]
    : []),
  '',
  `Copyright ${year} Eustress Engine. All rights reserved.`,
];

/// One email, in the shell.
///
///   title      the document title (escaped)
///   heading    optional headline at the top of the body (escaped)
///   label      the line under the wordmark in the header (escaped)
///   preheader  the preview line a mail list shows (escaped); keep it free of
///              anything private
///   body       markup built with `html` and the helpers (required)
///   text       the plain-text twin of the body, same facts, same links
///              (required)
///   footer     `full` (social row and nav columns) or `plain` (no link)
///   assetOrigin  where the header gear is served from
///
/// Returns `{ html, text }`.
export function renderEmail({ title, heading = '', label = '', preheader = '', body, text, footer = 'full', assetOrigin = ASSET_ORIGIN, year } = {}) {
  if (!(body instanceof Markup)) throw new TypeError('the body of an email is built with the html tag or the helpers in email_shell.mjs');
  if (typeof text !== 'string' || !text.trim()) throw new TypeError('an email needs its plain-text twin');
  if (footer !== 'full' && footer !== 'plain') throw new TypeError(`unknown footer ${footer}`);
  const docTitle = title || heading;
  if (!docTitle) throw new TypeError('an email needs a title');

  const origin = /^https?:\/\/[A-Za-z0-9.-]+(:\d{1,5})?$/.test(String(assetOrigin)) ? String(assetOrigin) : ASSET_ORIGIN;
  const iconUrl = escapeHtml(origin + LOGO_PATH);
  const stamp = Number.isInteger(year) && year >= 2000 && year <= 9999 ? year : new Date().getUTCFullYear();

  const out = `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1.0">
  <meta name="color-scheme" content="dark">
  <meta name="supported-color-schemes" content="dark">
  <title>${escapeHtml(docTitle)}</title>
  <style>:root{color-scheme:dark;supported-color-schemes:dark;}</style>
</head>
<body bgcolor="${PAGE}" text="#c9d1d9" link="#58a6ff" style="margin:0;padding:0;background-color:${PAGE};font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;color:#c9d1d9;">${preheader ? `
  <div hidden style="display:none;max-height:0;max-width:0;overflow:hidden;opacity:0;font-size:1px;line-height:1px;color:${PAGE};mso-hide:all;">${escapeHtml(preheader)}</div>` : ''}
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" bgcolor="${PAGE}" style="background-color:${PAGE};">
    <tr>
      <td align="center" bgcolor="${PAGE}" style="padding:32px 16px;background-color:${PAGE};">
        <table role="presentation" width="600" cellpadding="0" cellspacing="0" border="0" bgcolor="${CARD}" style="max-width:600px;width:100%;background-color:${CARD};border:1px solid #30363d;border-radius:14px;overflow:hidden;box-shadow:0 20px 60px rgba(0,0,0,0.5);">
${headerBlock({ iconUrl, label: escapeHtml(label) })}

          <!-- eustress-email:body -->
          <tr>
            <td bgcolor="${CARD}" style="padding:32px 32px 16px;background-color:${CARD};">${heading ? `
              <h1 style="color:#f0f6fc;font-size:20px;font-weight:700;margin:0 0 16px;line-height:1.3;">${escapeHtml(heading)}</h1>` : ''}
              ${body.value}
            </td>
          </tr>

          <!-- eustress-email:footer -->${footer === 'full' ? socialBlock() + columnsBlock() : plainFooterBlock()}${copyrightBlock(stamp)}

        </table>
      </td>
    </tr>
  </table>
</body>
</html>`;

  const lines = ['EUSTRESS ENGINE', ...(label ? [label] : []), '', ...(heading ? [heading, ''] : []), text.trim(), '', ...textFooter(footer, stamp)];
  return { html: out, text: lines.join('\n') };
}
