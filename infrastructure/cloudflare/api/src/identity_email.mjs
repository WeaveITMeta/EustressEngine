// =============================================================================
// The registration email
// =============================================================================
//
// One canonical place to assemble the registration / identity-backup email, so
// /api/auth/register's auto-send and /api/identity/email-backup share the same
// output. Emits HTML and plain-text bodies and an attachment descriptor for the
// multipart/mixed wrapper (`buildMimeWithAttachment` in index.js).
//
// The header, footer, palette and plain-text twin come from the shared shell in
// email_shell.mjs, the same one the account emails use (src/account_notices.mjs),
// so the two cannot drift apart. This file is the body only.
//
// Layout choices (2026-04-25):
//   * Header has the gear above the "EUSTRESS ENGINE" wordmark, both centred,
//     matching the website footer's brand block.
//   * No inline TOML preview block; the body points the user to the attached
//     `.toml` file instead.
//   * Subject and copy are framed as "Registration", not "Backup".
// =============================================================================

import { renderEmail, html, code, anchor } from './email_shell.mjs';

export function buildIdentityEmail({ username, toml_content, host }) {
  const safeName = (username || 'Creator').replace(/[<>"']/g, '');
  const filename = `eustress-${safeName}.toml`;

  // The `/assets/eustress-gear.png` route is bound to both `api.eustress.dev/*`
  // and the workers.dev URL, so either resolves. A caller-supplied `host` (the
  // origin the Worker was reached on) wins, keeping image origin == request
  // origin during dev; the shell falls back to the production hostname.
  const body = html`
              <p style="color:#f0f6fc;font-size:15px;margin:0 0 16px;line-height:1.5;">Welcome, <strong>${safeName}</strong>.</p>
              <p style="color:#8b949e;font-size:14px;margin:0 0 24px;line-height:1.6;">
                Your Eustress account is registered. Your identity file
                ${code(filename)}
                is attached to this email. It contains your Ed25519 private key, the cryptographic root of the account. Treat it like a password-manager vault key: store it securely, back it up off-device, and never share it.
              </p>

              <!-- Centred call-to-action pointing at the attachment dock.
                   The user downloads the real .toml file from this email
                   instead of copy-pasting from a code block. -->
              <table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" bgcolor="#161b22" style="margin:0 0 26px;background-color:#161b22;">
                <tr>
                  <td align="center" bgcolor="#1a2a3a" style="background-color:#1a2a3a;background-image:linear-gradient(135deg,#1a2a3a 0%,#1e2a40 100%);border:1px solid #2a3a4a;border-radius:10px;padding:24px 20px;">
                    <div style="color:#8ab4f8;font-size:11px;font-weight:700;letter-spacing:0.12em;text-transform:uppercase;margin:0 0 8px;">Your Identity File</div>
                    <div style="color:#f0f6fc;font-size:15px;font-family:'SF Mono',Consolas,Monaco,monospace;margin:0 0 4px;">${filename}</div>
                    <div style="color:#8b949e;font-size:12px;line-height:1.5;">Download from the attachment dock.</div>
                  </td>
                </tr>
              </table>

              <div style="background-color:#0d1117;color:#c9d1d9;border:1px solid #30363d;border-radius:8px;padding:18px 20px;margin:0 0 22px;">
                <div style="color:#c9d1d9;font-size:13px;font-weight:600;letter-spacing:0.02em;margin:0 0 10px;">How to use your registration</div>
                <ol style="color:#8b949e;font-size:13px;margin:0;padding-left:20px;line-height:1.8;">
                  <li style="color:#8b949e;">Save the attached ${code(filename)} on the device(s) you sign in from.</li>
                  <li style="color:#8b949e;">Sign in at ${anchor('https://eustress.dev/login', 'eustress.dev/login')}</li>
                  <li style="color:#8b949e;">Or load it in EustressEngine via the Sign In dialog.</li>
                </ol>
              </div>

              <p style="color:#f85149;font-size:12px;margin:0 0 6px;text-align:center;font-weight:500;line-height:1.5;">
                &#9888; Never share your <code style="background-color:#2d2026;padding:1px 5px;border-radius:3px;color:#ff7b72;font-family:'SF Mono',Consolas,Monaco,monospace;font-size:11px;">private_key</code> with anyone.
              </p>
              <p style="color:#6e7681;font-size:11px;margin:0 0 8px;text-align:center;line-height:1.5;">
                Eustress staff will never ask for it. If you receive a request claiming otherwise, it is fraudulent.
              </p>`;

  const text = [
    `Welcome, ${safeName}.`,
    '',
    `Your Eustress account is registered. Your identity file (${filename}) is attached to this email.`,
    'It contains your Ed25519 private key; store it securely, back it up off-device, and never share it.',
    '',
    `Download the attached ${filename} from this email to use it.`,
    '',
    'How to use your registration:',
    `  1. Save the attached ${filename} on the device(s) you sign in from.`,
    '  2. Sign in at https://eustress.dev/login',
    '  3. Or load it in EustressEngine via the Sign In dialog.',
    '',
    'WARNING: Never share your private_key with anyone. Eustress staff will never ask for it.',
    'If you receive a request claiming otherwise, it is fraudulent.',
  ].join('\n');

  const email = renderEmail({
    title: 'Eustress Identity Registration',
    label: 'Identity Registration',
    preheader: 'Your Eustress account is registered. Your identity file is attached.',
    body,
    text,
    footer: 'full',
    assetOrigin: host,
  });

  return {
    subject: `Your Eustress Identity Registration: ${safeName}`,
    html: email.html,
    text: email.text,
    attachment: {
      filename,
      mime: 'application/toml',
      content: toml_content,
    },
  };
}
