# Staging test: replacing your key and confirming your email

**Status:** written 2026-10-01 for the run after the staging Worker (migration `v6-account-auth`) and the website build are
deployed. Staging only; production stays behind McKale's go. Nothing here has been run yet. Prepared by the Website session from the
current code.

What this proves, in order:

1. Sign-in still works, and the site knows the account's object.
2. The one-time prompt appears once.
3. The first email link goes to the address on file, and pressing its button confirms the address.
4. Replacing the key through the site: a new file is saved first, the old file stops signing in, and the other sign-ins end.
5. After a replacement, spending and payout changes are refused with the date.
6. A change of email address is signed with your file, announced with a cancel link, and the cancel link works.

## Before you start

| Needed | State |
|---|---|
| The staging Worker with `v6-account-auth`, then the website build | Deployed by the orchestrator, Worker first. |
| Your staging account and its identity file (`eustress-<name>.toml`) | Keep a copy of this file somewhere safe BEFORE you start; step 4 replaces it. |
| An email address on the staging account | Needed for steps 3 and 6. If the account has none, say so in your report; adding a first address takes the seven-day wait, so it cannot be tested in one sitting. |
| Mail from the staging Worker | If the binding is not set up, "Send me a link" answers "The email could not be sent". That is a finding, not a failure of the code under test. |

## Step 1: sign in

Open `https://staging.eustress-cbb.pages.dev/login`, choose your identity file, sign in. You should land signed in.

Check: `https://api-staging.eustress.dev/health` shows `integrations.account_auth.configured: true`.

## Step 2: the one-time prompt

Right after signing in, a dialog "Replace your identity key" appears. If your email is not confirmed it first asks you to confirm
it. Press **Later**. Reload the page: the dialog does not come back. (It is shown once per account.)

## Step 3: confirm your email

1. Settings, Security tab, **Email address**. It shows your address "(not verified)".
2. Press **Send me a link**. A message says where it went (`a***@example.com`).
3. Open the email. The link starts with `https://staging.eustress-cbb.pages.dev/verify-email?token=`. Open it.
4. The page does nothing until you press **Confirm this address**. Press it: "Your address is confirmed."
5. Back in Settings the address shows "(verified)" and a line that it counts for recovery from a date thirty days away.

Also check that the address bar of the confirm page no longer shows the token after it loads.

## Step 4: replace the key

1. Settings, Security tab, **Identity key**. Choose your current identity file, press **Check my file**.
2. Tick one API key or webhook if the list shows any (you can create a test API key first on the site); leave the box that emails the
   new file ticked.
3. Press **Replace my key**. Your browser saves `eustress-<name>.toml` BEFORE the replacement; check your Downloads folder.
4. The page says the key was replaced, lists what was revoked, and gives two dates (payouts paused, payout bank pinned).
5. An email arrives on each address the account has: the key was replaced, with short fingerprints and no link.
6. Sign out, then sign in with the NEW file: works. Sign in with the OLD file: "this identity key was replaced on DATE".
7. If you had a second browser signed in, it is signed out.

## Step 5: the freeze

With the account replaced in step 4:

1. Open `/bliss` and press **Connect Bank Account**. A message says payouts and spending are paused until a date (72 hours after the
   replacement, or 7 days if the account held value). After that date the same button says the payout account cannot be set up or
   changed until a date 30 days after the replacement.
2. Spending Bliss or Tickets on a purchase answers the same pause.
3. Publishing and everything else still works.

## Step 6: change the email address

1. Settings, Security, **Change my address**. Type a second address you control. Choose the NEW identity file. Press **Ask for the
   change**.
2. A banner appears across the site: a change of address is waiting, with the date seven days away.
3. The address on file receives a warning with a **cancel** link; the address you typed receives a notice with NO link.
4. Open the cancel link, press **Cancel the change**. You are signed out of every device. Sign in again with the new file.
5. The banner is gone and Settings shows no pending change.

You cannot test "Make it my address" in one sitting: it unlocks after seven days (the Worker's tests cover the clock).

## Step 7: the lost-file page (look only)

Signed out, open `/login` and press **Lost your identity file?** (under the file chooser). On `/replacement-key` type a username and
press **Make a new key**: a file downloads and the page shows only the public key and the words to send to support. Do NOT send it in
this test: the administrator route that uses it needs your staging account to be an administrator, which waits for your yes in chat.

## What to tell the Website session

For each step: worked, or what you saw (the words on the page, and any email that did not arrive). Do not paste identity files or
tokens.
