// =============================================================================
// A daily allowance of account mail per recipient
// =============================================================================
//
// The address on an account is not verified until its owner says so, so an
// attacker's own account can name a stranger's address and ask for mail to it:
// verification links, change warnings, key notices. The number of accounts an
// attacker can make is no limit on what lands in one stranger's inbox, so the
// RECIPIENT is what is counted. Mail past the allowance is not sent, and the
// overflow is counted where an admin can see it (mailcap-overflow:{date}).
// Read then write: a parallel burst can overrun the number by a few; the
// platform limiter in front of the routes bounds that.
// =============================================================================

/// Per recipient per day, by what caused the mail.
///
///   verify   a verification link: an attacker's own account can name a stranger's
///            address as its own and ask for one.
///   pending  the notice sent to the address a change names: the requester chooses
///            that address, and a stranger's inbox is one they can pick.
///   notice   mail to an address the account already holds: a key replacement, the
///            warning of an address change, a cancellation. This is the mail a
///            victim most needs, so it does not share a bucket with the two above,
///            and its bucket belongs to the ACCOUNT as well as the address: an
///            address is not exclusive to one account (any number of accounts can
///            name a victim's address as theirs), so a bucket per address would let
///            other accounts spend it and the real notice would never be sent. A
///            counter per address is kept, for an admin to see a flood; it never
///            suppresses anything. Each account is bounded already (3 key
///            replacements and 5 address changes a day).
export const MAIL_PER_DAY = { verify: 5, pending: 5, notice: 30 };

async function sha256Hex(text) {
  const bytes = new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text)));
  return [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('');
}

/// Spend one send of `kind` ('verify', 'pending' or 'notice') to `address`; false when the
/// day's allowance is used up. A 'notice' is counted for `accountId` and the address; one
/// sent with no account named gets the smallest allowance, like an unknown kind.
export async function takeMailSlot(env, address, kind, accountId = null) {
  if (!env.USERS) return true;
  const hash = (await sha256Hex(String(address).toLowerCase())).slice(0, 32);
  const day = new Date().toISOString().slice(0, 10);
  let scope = kind;
  let cap = MAIL_PER_DAY[kind] ?? MAIL_PER_DAY.verify;
  if (kind === 'notice') {
    if (typeof accountId === 'string' && accountId) {
      scope = `notice:${accountId}`;
      // Counted for an admin to see a flood on one address; never a reason to refuse.
      const floodKey = `mailcap-flood:${hash}:${day}`;
      const flood = parseInt((await env.USERS.get(floodKey)) || '0', 10) || 0;
      await env.USERS.put(floodKey, String(flood + 1), { expirationTtl: 2 * 86400 });
    } else {
      cap = MAIL_PER_DAY.verify;
    }
  }
  const key = `mailcap:acct-${scope}:${hash}:${day}`;
  const used = parseInt((await env.USERS.get(key)) || '0', 10) || 0;
  if (used >= cap) {
    const overflowKey = `mailcap-overflow:${day}`;
    const n = parseInt((await env.USERS.get(overflowKey)) || '0', 10) || 0;
    await env.USERS.put(overflowKey, String(n + 1), { expirationTtl: 30 * 86400 });
    return false;
  }
  await env.USERS.put(key, String(used + 1), { expirationTtl: 2 * 86400 });
  return true;
}
