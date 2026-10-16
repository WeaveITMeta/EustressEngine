// =============================================================================
// Notices about an account's key
// =============================================================================
//
// When the key an account signs in with is replaced, the owner is told on every
// address we have for them (docs/architecture/IDENTITY_KEY_ROTATION_AND_RECOVERY.md,
// "The notice rule"). The notice carries no link that does anything: a link in
// an email about an account takeover is what a phisher sends, so it says what
// happened and how to reach a person.
//
// Every notice in this file, and the registration email (src/identity_email.mjs),
// is built in the one shared shell, src/email_shell.mjs: the header, the footer,
// the palette and the plain-text twin are the shell's, and a notice here supplies
// its body and its text. The shell escapes what it is given; the `html` tag
// escapes every value interpolated into a body. The key and address notices use
// the shell's `plain` footer, which has no link in it, so a notice carries the
// links it carries here and no others.
// =============================================================================

import { takeMailSlot } from './mail_limits.mjs';
import { renderEmail, html, paragraph, note, strong, anchor, bullets, facts, button, urlLine, formatWhen, parseWhen } from './email_shell.mjs';

const SUPPORT = 'support@eustress.dev';

/// A time as a notice shows it: month, day, year, a 12-hour clock and the zone,
/// in the account's own zone (`d.timeZone`, an IANA name the caller reads from
/// the account) or in UTC when it has none, or one that is not a zone. An email
/// cannot know the reader's zone, so it is the account's saved one.
const showTime = (value, d) => formatWhen(value, d?.timeZone);
/// The same, or '' when `value` is not a time, so a sentence about it can be left out.
const maybeTime = (value, d) => (Number.isFinite(parseWhen(value)) ? formatWhen(value, d?.timeZone) : '');

/// Every kind `renderKeyNotice` and `renderEmailNotice` build. A kind is added
/// here and in its renderer together; tests/email_shell.test.mjs renders each
/// one and requires it to come out of the shell.
export const KEY_NOTICE_KINDS = ['rotation', 'admin_replacement', 'key_restored', 'passkey_added', 'passkey_removed'];
export const EMAIL_NOTICE_KINDS = ['email_verify', 'email_change_existing', 'email_change_pending', 'email_changed', 'email_change_cancelled'];

/// The subject, the plain text and the HTML of one notice. `kind` is
/// `rotation`, `admin_replacement` or `key_restored`. Every value that came from
/// anywhere is escaped in the HTML.
export function renderKeyNotice(kind, d) {
  if (!KEY_NOTICE_KINDS.includes(kind)) throw new Error(`unknown notice kind ${kind}`);
  if (kind === 'key_restored') return renderRestored(d);
  if (kind === 'passkey_added' || kind === 'passkey_removed') return renderPasskey(kind === 'passkey_added', d);
  const byAdmin = kind === 'admin_replacement';
  const headline = byAdmin
    ? `The identity key for your Eustress account "${d.username}" was replaced by Eustress support on ${showTime(d.at, d)}, after an identity check${d.caseRef ? ` (case ${d.caseRef})` : ''}.`
    : `The identity key for your Eustress account "${d.username}" was replaced on ${showTime(d.at, d)}.`;
  const frozen = maybeTime(d.valueFrozenUntil, d);
  const pinned = maybeTime(d.payoutPinnedUntil, d);
  const lines = [
    headline,
    '',
    `Old key: ${d.oldFingerprint}`,
    `New key: ${d.newFingerprint}`,
    '',
    'What this means:',
    '  - The old identity file no longer signs in to Eustress. Every sign-in the account held was ended.',
    '  - Use the new identity file on every computer you use.',
    ...(frozen ? [`  - Payouts, withdrawals and transfers are paused until ${frozen}.`] : []),
    ...(pinned ? [`  - The payout bank account cannot be changed until ${pinned}.`] : []),
    ...(d.revoked && (d.revoked.api_keys || d.revoked.webhooks || d.revoked.discord || d.revoked.passkeys)
      ? [`  - Revoked at your request: ${revokedList(d.revoked)}.`]
      : []),
    '',
    ...(byAdmin && d.undoLink
      ? [
        'If this was NOT you, or you did not ask support for it, put your earlier key back now' + (maybeTime(d.undoUntil, d) ? ' (the link works until ' + maybeTime(d.undoUntil, d) + ')' : '') + ':',
        d.undoLink,
        'If you DID ask support to replace your key, do NOT use this link: it puts your OLD key back.',
        'It ends every sign-in, keeps payouts and transfers paused, and marks the case for a person to review. It does nothing else.',
        'You can also write to ' + SUPPORT + ' from any address and say your username. Do not reply to this message with your identity file.',
      ]
      : [
        'If you did not do this, write to ' + SUPPORT + ' right away from any address and say your username. Do not reply to this',
        'message with your identity file. No email from Eustress contains a link to replace or recover a key.',
      ]),
    '',
    'Files you made earlier may still hold your birthday and an ID hash, and may be in old emails. Delete old copies, including the emails that carried them.',
  ];
  const text = lines.join('\n');
  const revoked = d.revoked && (d.revoked.api_keys || d.revoked.webhooks || d.revoked.discord || d.revoked.passkeys)
    ? `Revoked at your request: ${revokedList(d.revoked)}.`
    : '';
  const body = html`${paragraph(byAdmin
    ? html`The identity key for your account ${strong(d.username)} was replaced by Eustress support on ${showTime(d.at, d)}, after an identity check${d.caseRef ? ` (case ${d.caseRef})` : ''}.`
    : html`The identity key for your account ${strong(d.username)} was replaced on ${showTime(d.at, d)}.`)}
${facts([['Old key', d.oldFingerprint], ['New key', d.newFingerprint]])}
${bullets([
    'The old identity file no longer signs in to Eustress, and every sign-in the account held was ended.',
    'Use the new identity file on every computer you use.',
    frozen && `Payouts, withdrawals and transfers are paused until ${frozen}.`,
    pinned && `The payout bank account cannot be changed until ${pinned}.`,
    revoked,
  ])}
${byAdmin && d.undoLink
    ? paragraph(html`${strong('If this was not you,')} or you did not ask support for it, ${anchor(d.undoLink, 'put your earlier key back now')}${maybeTime(d.undoUntil, d) ? ` (the link works until ${maybeTime(d.undoUntil, d)})` : ''}. It ends every sign-in, keeps payouts and transfers paused, and marks the case for a person to review. It does nothing else. ${strong('If you did ask support to replace your key, do NOT use this link: it puts your OLD key back.')} You can also write to ${anchor('mailto:' + SUPPORT, SUPPORT)} from any address and say your username. Do not reply with your identity file.`)
    : paragraph(html`${strong('If you did not do this,')} write to ${anchor('mailto:' + SUPPORT, SUPPORT)} right away and say your username. Do not reply with your identity file. No email from Eustress contains a link to replace or recover a key.`)}
${note('Files you made earlier may still hold your birthday and an ID hash, and may be in old emails. Delete old copies, including the emails that carried them.')}`;
  const email = renderEmail({
    title: 'Your Eustress identity key was replaced',
    heading: 'Your Eustress identity key was replaced',
    label: 'Security Notice',
    preheader: 'The identity key for your Eustress account was replaced.',
    body,
    text,
    footer: 'plain',
    assetOrigin: d.assetOrigin,
  });
  return { subject: 'Your Eustress identity key was replaced', text: email.text, html: email.html };
}

/// What a replacement revoked, in words. Passkeys are named only when some were.
function revokedList(r) {
  return `${r.api_keys || 0} API key(s), ${r.webhooks || 0} webhook endpoint(s)${r.passkeys ? `, ${r.passkeys} passkey(s)` : ''}${r.discord ? ', the Discord link' : ''}`;
}

/// A passkey was added to the account, or removed from it. Like every key
/// notice it carries no link that does anything.
function renderPasskey(added, d) {
  const when = showTime(d.at, d);
  const name = d.passkeyName || 'Passkey';
  const headline = added
    ? `A passkey named "${name}" was added to your Eustress account "${d.username}" on ${when}. It signs in to the Eustress website on the device that holds it.`
    : `The passkey named "${name}" was removed from your Eustress account "${d.username}" on ${when}. It no longer signs in.`;
  const text = [
    headline,
    '',
    added
      ? 'Adding a passkey needs your identity file. If you did not do this, sign in with your identity file, remove the passkey in Settings, Security, and replace your identity file there.'
      : 'Removing a passkey needs your identity file. If you did not do this, sign in with your identity file and replace it in Settings, Security.',
    'Then write to ' + SUPPORT + ' from any address and say your username. Do not reply to this message with your identity file.',
  ].join('\n');
  const body = html`${paragraph(added
    ? html`A passkey named ${strong(name)} was added to your account ${strong(d.username)} on ${when}. It signs in to the Eustress website on the device that holds it.`
    : html`The passkey named ${strong(name)} was removed from your account ${strong(d.username)} on ${when}. It no longer signs in.`)}
${paragraph(html`${strong('If you did not do this,')} ${added
    ? 'sign in with your identity file, remove the passkey in Settings, Security, and replace your identity file there: adding a passkey needs it.'
    : 'sign in with your identity file and replace it in Settings, Security: removing a passkey needs it.'} Then write to ${anchor('mailto:' + SUPPORT, SUPPORT)} from any address and say your username. Do not reply with your identity file.`)}`;
  const title = added ? 'A passkey was added to your Eustress account' : 'A passkey was removed from your Eustress account';
  const email = renderEmail({
    title,
    heading: title,
    label: 'Security Notice',
    preheader: title + '.',
    body,
    text,
    footer: 'plain',
    assetOrigin: d.assetOrigin,
  });
  return { subject: title, text: email.text, html: email.html };
}

/// The notice that the key support put in place was taken out again by the link in the replacement notice.
function renderRestored(d) {
  const text = [
    `The identity key support put in place on your Eustress account "${d.username}" on ${showTime(d.replacedAt, d)} was taken out on ${showTime(d.at, d)}, at the request of someone who held the link in the first notice.`,
    '',
    'Your earlier identity file is the one that signs in again. Every sign-in the account held was ended, payouts and transfers are paused' + (maybeTime(d.valueFrozenUntil, d) ? ' until ' + maybeTime(d.valueFrozenUntil, d) : '') + ', and a person at Eustress will review the case.',
    '',
    'If this was not you, write to ' + SUPPORT + ' right away from any address and say your username.',
  ].join('\n');
  const body = html`${paragraph(html`The identity key support put in place on your account ${strong(d.username)} on ${showTime(d.replacedAt, d)} was taken out on ${showTime(d.at, d)}, at the request of someone who held the link in the first notice.`)}
${paragraph(`Your earlier identity file is the one that signs in again. Every sign-in the account held was ended, payouts and transfers are paused${maybeTime(d.valueFrozenUntil, d) ? ' until ' + maybeTime(d.valueFrozenUntil, d) : ''}, and a person at Eustress will review the case.`)}
${paragraph(html`${strong('If this was not you,')} write to ${anchor('mailto:' + SUPPORT, SUPPORT)} right away from any address and say your username.`)}`;
  const email = renderEmail({
    title: 'The key support put in place was taken out',
    heading: 'The key support put in place was taken out',
    label: 'Security Notice',
    preheader: 'The identity key support put in place on your Eustress account was taken out.',
    body,
    text,
    footer: 'plain',
    assetOrigin: d.assetOrigin,
  });
  return { subject: 'The key support put in place on your Eustress account was taken out', text: email.text, html: email.html };
}

/// The summary sent to the second channel (the security mailbox and the other
/// administrators) when an administrator replaces a key. It names who, what and
/// why, and carries no key and no link.
export function renderAdminActionNotice(d) {
  const text = [
    `An administrator is replacing an account's identity key.`,
    '',
    `Administrator: ${d.adminName} (${d.adminId})`,
    `Account: ${d.targetName} (${d.targetId})`,
    `Case: ${d.caseRef}`,
    `Reason: ${d.reason}`,
    `When: ${maybeTime(d.at, d) || d.at}`,
    '',
    'The account holder was told on every address and has a link that puts the earlier key back.',
  ].join('\n');
  const body = html`${facts([
    ['Administrator', `${d.adminName} (${d.adminId})`],
    ['Account', `${d.targetName} (${d.targetId})`],
    ['Case', d.caseRef],
    ['Reason', d.reason],
    ['When', maybeTime(d.at, d) || d.at],
  ])}
${paragraph('The account holder was told on every address and has a link that puts the earlier key back.')}`;
  const email = renderEmail({
    title: 'An administrator is replacing an account\'s identity key',
    heading: 'An administrator is replacing an account\'s identity key',
    label: 'Administrator Notice',
    preheader: 'An administrator is replacing the identity key of an account.',
    body,
    text,
    footer: 'plain',
    assetOrigin: d.assetOrigin,
  });
  return { subject: 'An administrator is replacing an account identity key', text: email.text, html: email.html };
}

const b64 = (text) => {
  const bytes = new TextEncoder().encode(text);
  let binary = '';
  for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
  return btoa(binary).replace(/(.{76})/g, '$1\r\n');
};

/// The raw message. The recipient is checked to be one plain address here as
/// well, so no caller can add a header through it.
export function buildNoticeMime({ from, to, subject, text, html, fromName = 'Eustress Security' }) {
  for (const value of [from, to, subject, fromName]) {
    if (/[\r\n]/.test(String(value))) throw new Error('header value contains a line break');
  }
  // A display name is plain words: a `<`, `>`, `"`, `,` or `;` in it could restructure the From header.
  if (typeof fromName !== 'string' || !/^[A-Za-z0-9 .-]{1,60}$/.test(fromName)) throw new Error('the sender name may hold only letters, digits, spaces, periods and hyphens');
  const boundary = '----=_eustress_notice_' + crypto.randomUUID().replace(/-/g, '');
  const encSubject = /^[\x20-\x7e]*$/.test(subject) ? subject : `=?UTF-8?B?${b64(subject).replace(/\r\n/g, '')}?=`;
  return [
    `From: ${fromName} <${from}>`,
    `To: ${to}`,
    `Subject: ${encSubject}`,
    `Message-ID: <${crypto.randomUUID()}@eustress.dev>`,
    'MIME-Version: 1.0',
    `Content-Type: multipart/alternative; boundary="${boundary}"`,
    '',
    `--${boundary}`,
    'Content-Type: text/plain; charset=utf-8',
    'Content-Transfer-Encoding: base64',
    '',
    b64(text),
    `--${boundary}`,
    'Content-Type: text/html; charset=utf-8',
    'Content-Transfer-Encoding: base64',
    '',
    b64(html),
    `--${boundary}--`,
    '',
  ].join('\r\n');
}

const ADDRESS = /^[^\s@<>,;"']{1,64}@[^\s@<>,;"']{1,190}\.[^\s@<>,;"']{2,}$/;

/// The addresses the user record holds. A key notice goes to ALL of the account's
/// addresses, never to one of them; the account's object keeps the full list
/// (verified, pending, on file, and a replaced address for 30 days:
/// `noticeSet` in src/email_ops.mjs) and callers add this to it.
export function noticeRecipients(user) {
  const out = new Set();
  const add = (value) => {
    if (typeof value === 'string' && ADDRESS.test(value.trim())) out.add(value.trim().toLowerCase());
  };
  add(user?.email);
  return [...out];
}

/// Send one notice to each recipient. Never throws: a notice that cannot be sent
/// must not undo the key change it reports; the result says what happened.
export async function sendNotice(env, recipients, rendered, { from = env.NOTICE_MAIL_FROM || 'identity@eustress.dev', kind = 'notice', accountId = null, fromName } = {}) {
  const results = [];
  if (!env.EMAIL) return recipients.map(() => ({ sent: false, reason: 'no_mail_binding' }));
  for (const to of recipients) {
    try {
      // A recipient's daily allowance (src/mail_limits.mjs): past it nothing is sent.
      if (!(await takeMailSlot(env, to, kind, accountId))) {
        console.error('account mail not sent: the recipient\'s daily allowance is used up');
        results.push({ sent: false, reason: 'daily_cap' });
        continue;
      }
      const raw = buildNoticeMime({ from, to, ...(fromName ? { fromName } : {}), ...rendered });
      const EmailMessage = env.__EmailMessage || (await import('cloudflare:email')).EmailMessage;
      await env.EMAIL.send(new EmailMessage(from, to, raw));
      results.push({ sent: true });
    } catch (e) {
      // A mail binding's error can quote the recipient: the log gets the kind of error and the message with every address masked.
      console.error('key notice not sent:', e?.name || 'Error', scrubAddresses(e?.message || e));
      results.push({ sent: false, reason: 'send_failed' });
    }
  }
  return results;
}

// -----------------------------------------------------------------------------
// Email notices: verification, and changing the address
// -----------------------------------------------------------------------------

/// `o***@example.com`: enough to recognise an address, not enough to read it
/// out of a notice that reached the wrong inbox.
export function maskAddress(address) {
  const [local = '', domain = ''] = String(address || '').split('@');
  return `${local.slice(0, 1)}***@${domain}`;
}

/// Text for a log with every address in it masked, whatever shape it is quoted in (bare, in quotes or angle brackets, any case,
/// with the `@` percent-encoded): a log is not where an address belongs.
export function scrubAddresses(value) {
  return String(value ?? '').replace(/\S*(?:@|%40)\S*/gi, (token) => maskAddress(token.replace(/%40/gi, '@')));
}

/// An email notice in the shell. `footer` is `full` for the verification email
/// (it carries the site's links) and `plain` (no link) for everything that
/// reports a change to the address or the key.
const inShell = (d, { subject, heading, label, preheader, body, text, footer = 'plain' }) => {
  const email = renderEmail({ title: heading, heading, label, preheader, body, text, footer, assetOrigin: d?.assetOrigin });
  return { subject, text: email.text, html: email.html };
};

/// `kind`: `email_verify` (the link to the address on file), `email_change_existing`
/// (to every address the account already had, with the cancel link),
/// `email_change_pending` (to the new address, no link), `email_changed`,
/// `email_change_cancelled`. Every value is escaped in the HTML, and the links
/// are built from the deployment's own site address, never from a request.
export function renderEmailNotice(kind, d) {
  if (!EMAIL_NOTICE_KINDS.includes(kind)) throw new Error(`unknown notice kind ${kind}`);
  if (kind === 'email_verify') {
    // The expiry in words, in UTC: "October 2, 2026 at 12:00 UTC".
    const expires = maybeTime(d.expiresAt, d);
    const works = expires ? `it works once and expires on ${expires}` : 'it works once and expires after a short time';
    const text = [
      `Confirm the email address for your Eustress account "${d.username}".`,
      '',
      `Open this link on the Eustress site (${works}):`,
      d.link,
      '',
      'This only confirms this address. It cannot change your key or your account. If you did not ask for it, ignore this message.',
    ].join('\n');
    const body = html`${paragraph(html`Confirm the email address for your Eustress account ${strong(d.username)}.`)}
${button(d.link, 'Confirm this address')}
${urlLine('If the button does not work, copy this address into your browser:', d.link)}
${paragraph(`The link works once and ${expires ? `expires on ${expires}` : 'expires after a short time'}.`)}
${note('This only confirms this address. It cannot change your key or your account. If you did not ask for it, ignore this message.')}`;
    return inShell(d, {
      subject: 'Confirm your Eustress email address',
      heading: 'Confirm your email address',
      label: 'Email Confirmation',
      preheader: 'Confirm this address for your Eustress account.',
      body,
      text,
      footer: 'full',
    });
  }
  if (kind === 'email_change_existing') {
    const text = [
      `Someone asked to change the email address on your Eustress account "${d.username}" to ${d.newMasked}.`,
      '',
      `It takes effect on ${showTime(d.effectiveAt, d)} unless it is cancelled. Until then nothing changes.`,
      '',
      'If this was you, you do not need to do anything.',
      'If it was NOT you, cancel it now. Cancelling also signs out every device on the account:',
      d.cancelLink,
      '',
      'Then replace your identity key in Settings, Security, because the request was signed with your current key. No email from Eustress asks you to replace or recover a key.',
    ].join('\n');
    const body = html`${paragraph(html`Someone asked to change the email address on your Eustress account ${strong(d.username)} to ${strong(d.newMasked)}.`)}
${paragraph(`It takes effect on ${showTime(d.effectiveAt, d)} unless it is cancelled. Until then nothing changes.`)}
${paragraph('If this was you, you do not need to do anything.')}
${paragraph(html`${strong('If it was not you,')} ${anchor(d.cancelLink, 'cancel it now')}. Cancelling also signs out every device on the account.`)}
${note('Then replace your identity key in Settings, Security, because the request was signed with your current key. No email from Eustress asks you to replace or recover a key.')}`;
    return inShell(d, {
      subject: 'A change of email address was requested on your Eustress account',
      heading: 'A change of email address was requested',
      label: 'Account Notice',
      preheader: 'Someone asked to change the email address on your Eustress account.',
      body,
      text,
    });
  }
  if (kind === 'email_change_pending') {
    const text = [
      `This address was named as the new email address for the Eustress account "${d.username}".`,
      '',
      `It takes effect on ${showTime(d.effectiveAt, d)}, and only if the account's owner confirms it then. You do not need to do anything.`,
      'If you do not know this account, ignore this message.',
    ].join('\n');
    const body = html`${paragraph(html`This address was named as the new email address for the Eustress account ${strong(d.username)}.`)}
${paragraph(`It takes effect on ${showTime(d.effectiveAt, d)}, and only if the account's owner confirms it then. You do not need to do anything. If you do not know this account, ignore this message.`)}`;
    return inShell(d, {
      subject: 'This address was named for a Eustress account',
      heading: 'This address was named for a Eustress account',
      label: 'Account Notice',
      preheader: 'This address was named as the new email address for a Eustress account.',
      body,
      text,
    });
  }
  if (kind === 'email_changed') {
    const text = [
      `The email address on your Eustress account "${d.username}" is now ${d.newMasked}.`,
      '',
      'If you did not do this, write to ' + SUPPORT + ' right away and say your username.',
      'The new address has to be confirmed again, and this message goes to your earlier address for 30 days.',
    ].join('\n');
    const body = html`${paragraph(html`The email address on your Eustress account ${strong(d.username)} is now ${strong(d.newMasked)}.`)}
${paragraph(html`${strong('If you did not do this,')} write to ${anchor('mailto:' + SUPPORT, SUPPORT)} right away and say your username. The new address has to be confirmed again, and this message goes to your earlier address for 30 days.`)}`;
    return inShell(d, {
      subject: 'Your Eustress email address was changed',
      heading: 'Your Eustress email address was changed',
      label: 'Account Notice',
      preheader: 'The email address on your Eustress account was changed.',
      body,
      text,
    });
  }
  if (kind === 'email_change_cancelled') {
    const text = [
      `The request to change the email address on your Eustress account "${d.username}" to ${d.cancelledMasked} was cancelled.`,
      d.sessionsEnded ? 'Every device on the account was signed out. Sign in again with your identity file. If you did not make the request, replace your key in Settings, Security.' : '',
    ].filter(Boolean).join('\n\n');
    const body = html`${paragraph(html`The request to change the email address on your Eustress account ${strong(d.username)} to ${strong(d.cancelledMasked)} was cancelled.`)}
${d.sessionsEnded ? paragraph('Every device on the account was signed out. Sign in again with your identity file. If you did not make the request, replace your key in Settings, Security.') : ''}`;
    return inShell(d, {
      subject: 'The email change on your Eustress account was cancelled',
      heading: 'The email change was cancelled',
      label: 'Account Notice',
      preheader: 'The request to change the email address on your Eustress account was cancelled.',
      body,
      text,
    });
  }
  throw new Error(`unknown notice kind ${kind}`);
}
