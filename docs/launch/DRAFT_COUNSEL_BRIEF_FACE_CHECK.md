# DRAFT: brief for counsel, a face check we run ourselves

**Status:** draft, 2026-10-01, written by the Website session at McKale's request. **Not sent.** McKale sends it himself.
Nothing here is legal advice; it states what we plan and asks what we may do. Companion design:
`docs/architecture/IDENTITY_KEY_ROTATION_AND_RECOVERY.md`.

---

**Company:** Eustress (eustress.dev), a simulation and creator platform, solo founder, pre-launch (about 16 users).
**Question in one line:** may we run our own live face check to let a member recover a lost account key, how must we get consent
and how long may we keep the footage?

## What we plan

1. **The product.** An account is a cryptographic key. If a member loses it, they recover the account by a **live capture of
   their face** compared with the portrait on the **government ID they registered with**, plus a check of the ID's printed
   details. An automatic decision is made within seconds; approval swaps in a new key. A denied person can appeal to a human.
2. **Who builds it.** We do, not a vendor. We capture the face in the member's browser (a short live video, with random
   on-screen prompts and colour flashes to tell a live person from a replay or a deepfake), send frames to our own analysis
   service, compare the face with the ID portrait using a face-recognition model, and run a fake-detection model. **No face
   template is stored**; the comparison is computed and discarded.
3. **What we keep.** The registration ID images (as KYC records, for the life of the account and a period after). For each
   recovery, a few still images and a short clip, with the decision record: **14 months** (McKale's decision), longer only
   under a legal hold for a dispute. The reason for keeping them: if an impersonator gets through, the real holder can dispute
   it and the footage is evidence, including for a criminal complaint.
4. **Who sees it.** Stored in a separate, encrypted evidence store; read only by an adjudication path that needs the founder's
   hardware key; every read is logged to a log we cannot alter. There is **one administrator**, so there is no separation of
   duties; the policy will say so.
5. **Training data.** To build and test the fake detector we will record **consenting volunteers** (staff and testers) doing
   genuine captures and **staged attacks** (a printed photo, a replay on a screen, a virtual camera, a real-time face swap built
   from a volunteer's own public video).
6. **Third parties.** The ID photo is read by a language model through **xAI's API** (name, date of birth, number). **If we ship the
   fake-spotter described in the design, a vision-language model (also xAI) reads a few LIVE capture frames of the member's face**;
   no face image goes to a vendor for the face matching itself. Payouts use Stripe Connect.
7. **Who is in scope.** Members of any country. We require a minimum age by jurisdiction at registration (not always 18); a
   payout needs an adult.

## Questions for counsel

**Biometric privacy laws**

1. **Illinois BIPA.** Does a face comparison against an ID portrait, with no template stored, still engage BIPA? What must the
   **public written policy** and the **written release** say, is an on-screen electronic signature enough (we understand the
   2024 amendment says so), and how do we word the **retention schedule** against the rule to destroy data when the purpose is
   satisfied or within three years of the person's last interaction, whichever is first? Is 14 months defensible, and how do we
   describe the legal hold?
2. **Texas (Business and Commerce Code 503.001).** Destruction no later than one year after the purpose for collecting
   expires. If the purpose is "verify identity **and investigate fraud**", can we define the fraud-investigation purpose as
   lasting the 14 months, or does the purpose expire when the decision is made?
3. **GDPR Article 9** for EU members: legal basis and **explicit consent**, a **data protection impact assessment**, purpose
   limitation, erasure on request against our evidence retention, and **Article 22** (an automated decision with significant
   effect, our human appeal). Transfers of EU members' ID images and face footage to the United States.
4. **Other states and regimes** (Washington, Colorado, California sensitive data, any with a private right of action) and
   anything specific to a **solo founder, pre-launch** operator.

**Consent and wording**

5. Please review the **consent screen** and the **public retention policy** before the first real capture. May we say "designed
   to detect fakes" and what must we not say ("deepfake-proof", "certified", "we never store your face")?
6. **Minors.** Our minimum age varies by country. May we run the face check for members under 18, and should we exclude them?

**Evidence and handling**

7. **Retention for fraud.** Can we keep footage of an **impostor** (who never consented as a member) for evidence, and on what
   notice? What do we do on a **subpoena, a warrant or a police request**, and what do we tell the person?
8. **One administrator.** Is a single administrator with access to biometric evidence a problem in itself, and what
   safeguards would counsel expect (access logging, hardware key, an outside custodian)?
9. **Breach.** Notification duties if the evidence store is compromised.

**Models and vendors**

10. **The face models.** Many strong face-recognition models are licensed for **non-commercial research only** or were trained
    on faces gathered without consent. **Ask specifically about models trained on datasets derived from MS-Celeb-1M.** We will use models whose weights are licensed for commercial use. What diligence do you
    want on **licence and training-data provenance**, and does provenance create exposure for us?
11. **xAI.** Does sending ID images **and live face frames** to xAI's API count as a disclosure to a processor that needs **consent
    and a data processing agreement**? What do we need to know about xAI's **retention and training terms** for such images? Does it
    matter that the ID image contains a face?
12. **Volunteers' data.** The consent form, retention and deletion path for the volunteers who help us train and test.

**Money (separate, shorter)**

13. **Discrimination and automated-decision rules.** Does a **false-reject or false-match gap between demographic groups** in an
    automatic denial create discrimination or disparate-impact exposure, and do state automated-decision rules or the **EU AI Act**
    (biometric verification) add anything beyond GDPR Article 22?
14. After an account takeover, who bears a fraudulent payout: us, Stripe or the member? (We are asking Stripe the factual
    questions separately: `docs/launch/DRAFT_STRIPE_QUESTION_ACCOUNT_TAKEOVER.md`.) What should our terms say?

## What we will do with the answers

The answers set the consent text, the retention schedule (and whether 14 months stands), the published policy, who may use the
check, and whether we must change the design before launch. The check does not launch until counsel has reviewed the consent
screen and the retention schedule.
