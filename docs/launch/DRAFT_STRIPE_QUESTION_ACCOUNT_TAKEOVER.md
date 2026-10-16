# DRAFT: question for Stripe, losses and bank matching after an account takeover

**Status:** draft, 2026-10-01, written by the Website session at McKale's request. **Not sent.** McKale sends it himself.
Each question is a question, not a claim about how Stripe works.

---

**To:** Stripe (the Connect account team or support, whichever McKale uses)
**From:** Eustress (eustress.dev), a Connect platform
**Subject:** Who bears losses after an account takeover, and how strictly is the bank holder matched to the verified person?

## What we do today

- We create **Connect Custom accounts** (`type=custom`), `business_type=individual`, with the `card_payments` and `transfers`
  capabilities requested. We accept the Stripe terms of service on the person's behalf and record the date and IP address.
- **Cash-out needs a verified adult and an active paid plan** (Creator Pro or Bundle), a cost barrier any impersonator would also have to
  clear.
- Before we create the account, the person has passed our own identity check (a government ID photo read by a model, with an age
  gate). We prefill the account's date of birth from that document and upload the ID images to Stripe for identity verification.
- We send the person to a Stripe-hosted onboarding link (`account_links`, type `account_onboarding`, collecting
  `eventually_due`) to give the bank details and the tax number.
- Our members earn a platform currency that we convert and pay out to their connected account on a schedule. Accounts are
  controlled by a cryptographic key, not a password; a member who loses the key recovers the account through a new live
  identity check. We are building that check ourselves, and **it can be fooled by a skilled impersonator**. We want to
  understand what happens at the money step if it is.

## The scenario

An impersonator takes over a member's Eustress account (a stolen key, or a successful fake recovery) and tries to get that
member's money out.

## Questions

1. **Losses.** For Custom accounts created by our platform, who bears **negative balances, disputes and fraud losses** by
   default, Stripe or us? Which settings (for example the account's controller or loss-liability settings) change that, and can
   we move the liability toward Stripe for individuals?
2. **Changing the bank account.** Once an account exists, can the person (or someone who has taken over the person's Eustress
   account) **replace the external bank account** by opening a new hosted onboarding or update link? If yes, what does Stripe do
   to check a new bank account (name match, micro-deposits, a waiting period, a payout hold)? Can we **remove the person's
   ability to change the bank account through a hosted link** and make every change go through our own API call, after our own
   verification?
3. **Bank holder against the verified person.** How strictly does Stripe match the **name on the bank account** to the person
   whose identity it verified? Does it differ by country? Can we require an exact match, or is it advisory?
4. **Holding a first or changed payout.** Does Stripe offer a way to **delay or hold the first payout** to a new or changed
   bank account (payout schedule delay, manual payouts, a review hold)? Can we apply it only for the first payout after a
   change that we choose?
5. **Reversals.** After a payout has left Stripe for a bank, what can be reversed (a payout, a transfer reversal on the
   connected account's balance)? **Our payouts run from a daily scheduled transfer we control: can a transfer to the connected
   account be cancelled before it settles?** Who carries the loss if the connected account has no balance to reverse against?
6. **Signals.** Does Stripe expose a signal when a connected account's details change in a risky way (a new bank account, a
   new device or address on the hosted flow) that we can use to hold our own payouts?
7. **Recommended setup.** For a platform whose members are individuals and whose identity step can occasionally be fooled,
   what does Stripe recommend (account type, payout settings, verification requirements) to keep the loss on the person who
   caused it?

8. **Stripe's own checks at the money moment.** Does Stripe's verification of a connected individual include a **selfie or liveness
   step**, and can we **require it before the first payout** (using Stripe's checked step at the money moment, without building our own)?
9. **Whose acceptance counts.** We accept the Stripe terms of service on the person's behalf using the IP address of the session that
   asked for onboarding. If that session was an impersonator's, **whose acceptance counts**, and what follows for the account?

## What we will do with the answers

The answers decide whether we hold the first payout after a recovery for 24 hours, whether we stop members changing the bank
account through the hosted link, and what we tell members about payouts.
