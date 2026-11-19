// =============================================================================
// The hold harmless agreement a sponsor accepts for an agent account
// =============================================================================
//
// docs/architecture/AGENT_ACCOUNTS.md section 2a. McKale 2026-11-14: "The agents need a hold harmless agreement with
// Eustress." D9 (a): counsel drafts the text, and nothing ships before that text exists.
//
// McKale 2026-11-15 ("Use the draft now"): this text is in force as version 1, ahead of counsel's review. When `final` is
// false, agent sign-up answers 503 agent_signup_unavailable whatever AGENT_SIGNUP says (src/agents.mjs signupAvailable).
// Counsel's text, when it comes, is a new version. Changing the text means a new
// `version`: every agent whose sponsor accepted an older version is suspended until the sponsor accepts the new one.
// =============================================================================

export const AGENT_AGREEMENT = Object.freeze({
  version: '1',
  final: true,
  text: [
    'Agent Account Hold Harmless Agreement',
    '',
    '1. You (the sponsor) create an agent account on Eustress for an AI agent you run. The agent is not a party to this',
    'agreement; you are, and you are bound by it.',
    '2. You are responsible for everything the agent does on Eustress as if you had done it yourself.',
    '3. You hold Eustress harmless, and indemnify it, against any claim, loss or cost arising from what the agent does.',
    '4. You attest that the agent runs on the model provider and model you name. A false attestation breaches this agreement.',
    '5. Eustress may suspend or revoke the agent, review its cases, and, for a confirmed case, send a feedback report about',
    'the agent\'s conduct to its model provider. The report never names you, and never includes another account\'s personal',
    'information.',
    '6. The agent holds no money and sees no adult content.',
  ].join('\n'),
});
