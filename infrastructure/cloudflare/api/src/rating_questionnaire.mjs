// =============================================================================
// The rating questionnaire: what the author says the Space contains
// =============================================================================
//
// Guardian Code v2.2 Part V. The author answers a short questionnaire at
// publish. Each answer maps to one of Eustress's own age bands and to the
// Code's content descriptors. The listing's rating is the highest of three: this
// questionnaire's band, Jev's `content_rating`, and the judge's `rating_floor`
// (moderation.mjs decideFinal). An answer the evidence disproves is a B9 wording
// fault; the change is to correct the answer. An answer that says the Space
// shows hard drugs names Code B11 (`articles`), and its label tells the author
// plainly that such a Space cannot be listed.
//
// The questions are written for Eustress in plain words. They are not, and are
// not labelled as, any outside rating board's.
//
// Pure: nothing here reads storage or the network.
//
//     POST /api/simulations/{id}/rating-questionnaire   (moderation.mjs, the author's session)
//       { answers: { violence: 'none', blood: 'none', ... } }
//     stored on the listing as `rating_questionnaire`:
//       { version, answers, band, descriptors, articles, answered_at }
// =============================================================================

/// Version 2 (Code v2.2) asks about alcohol and tobacco, and about drugs, as two questions. A version 1 record reads as
/// unanswered: its one "alcohol, tobacco or drugs" answer cannot say which band the Space is in.
export const QUESTIONNAIRE_VERSION = 2;

/// The age bands, lowest first (Code Part V). moderation.mjs RATINGS is this list.
export const BANDS = ['all_ages', 'ages_10', 'teen_13', 'mature_17', 'adult_18', 'adult_21'];

/// The band that showing the use of psychedelics puts a Space in. McKale's call to confirm (2026-10-08): 21+, with alcohol,
/// and with the 21+ rule where psilocybin is legal. One constant, read by this questionnaire and by Jev's rating floor
/// (moderation.mjs), so flipping it is one line here and the matching line of Code Part V.
export const PSYCHEDELIC_USE_RATING = 'adult_21';

/// The content descriptors, in the Code's own words (Part V).
export const DESCRIPTORS = [
  'fantasy violence', 'realistic violence', 'blood', 'language', 'sexual themes', 'nudity (clinical or artistic)', 'sexual content',
  'alcohol', 'tobacco', 'psychedelics', 'simulated gambling (no money)', 'in-Space purchases', 'players interact',
];

const option = (value, label, band, descriptors = [], articles = []) => ({ value, label, band, descriptors, articles });

/// Every question, in the order the publish form asks them. Each option names the band it puts the Space in and the descriptors it adds.
export const QUESTIONS = Object.freeze([
  {
    id: 'violence',
    text: 'Does anyone or anything in your Space get hurt?',
    options: [
      option('none', 'No. Nothing in the Space is hurt or in danger.', 'all_ages'),
      option('peril', 'Only mild danger: a cartoon fall, a near miss, a character in a scrape, and no one is shown hurt.', 'ages_10'),
      option('fantasy', 'Fantasy or cartoon fighting: creatures, robots or characters that are clearly not real are hit or defeated.', 'teen_13', ['fantasy violence']),
      option('realistic', 'Realistic people are hurt, or injuries are shown, including in medical, first-aid or crash training.', 'mature_17', ['realistic violence']),
    ],
  },
  {
    id: 'blood',
    text: 'Is blood or a wound shown?',
    options: [
      option('none', 'No.', 'all_ages'),
      option('mild', 'A little, and not in detail.', 'teen_13', ['blood']),
      option('detailed', 'Yes, in detail, including surgery or anatomy.', 'mature_17', ['blood']),
    ],
  },
  {
    id: 'language',
    text: 'What is the strongest language a visitor reads or hears?',
    options: [
      option('none', 'Nothing rude.', 'all_ages'),
      option('mild', 'Mild words, such as "darn" or "heck".', 'ages_10', ['language']),
      option('crude', 'Crude jokes or rude words now and then.', 'teen_13', ['language']),
      option('strong', 'Swearing or slurs.', 'mature_17', ['language']),
    ],
  },
  {
    id: 'sexual',
    text: 'Is there any romance or sexual material?',
    options: [
      option('none', 'No.', 'all_ages'),
      option('suggestive', 'Light flirting, a kiss, or a mild joke with a double meaning.', 'teen_13', ['sexual themes']),
      option('themes', 'Sexual subjects are talked about, but nothing sexual is shown.', 'mature_17', ['sexual themes']),
      option('explicit', 'Sexual activity is shown.', 'adult_18', ['sexual content']),
    ],
  },
  {
    id: 'nudity',
    text: 'Is anyone shown without clothes?',
    options: [
      option('none', 'No.', 'all_ages'),
      option('clinical', 'Yes, for anatomy, medicine or art, and not in a sexual way.', 'mature_17', ['nudity (clinical or artistic)']),
      option('sexual', 'Yes, in a sexual way.', 'adult_18', ['sexual content']),
    ],
  },
  {
    id: 'alcohol_tobacco',
    text: 'Do alcohol or tobacco appear in your Space?',
    options: [
      option('none', 'No.', 'all_ages'),
      option('alcohol', 'Alcohol: drinks are shown, served, sold or drunk.', 'adult_21', ['alcohol']),
      option('tobacco', 'Tobacco: cigarettes, cigars, vapes or other tobacco are shown, sold or smoked.', 'adult_21', ['tobacco']),
      option('both', 'Both alcohol and tobacco.', 'adult_21', ['alcohol', 'tobacco']),
    ],
  },
  {
    id: 'drugs',
    text: 'Do drugs appear in your Space?',
    options: [
      option('none', 'No.', 'all_ages'),
      option('education', 'Only to save lives: medical care, overdose first aid such as naloxone, or how addiction works, is treated or is prevented, taught accurately.', 'mature_17'),
      option('psychedelics', 'Psychedelics (psilocybin, LSD, DMT, mescaline, MDMA or ketamine) are shown being used.', PSYCHEDELIC_USE_RATING, ['psychedelics']),
      option('hard_drugs', 'Hard drugs (opioids outside medical care, cocaine or crack, methamphetamine, or cannabis) are shown being used, sold or made, or made to look good. A Space that shows this cannot be listed on Eustress (Code B11).', 'adult_21', [], ['B11']),
    ],
  },
  {
    id: 'gambling',
    text: 'Can a visitor bet or play a game of chance?',
    options: [
      option('none', 'No.', 'all_ages'),
      option('simulated', 'Yes, with pretend money or points only. No real money can go in or come out.', 'teen_13', ['simulated gambling (no money)']),
    ],
  },
  {
    id: 'purchases',
    text: 'Can a visitor buy anything inside the Space?',
    options: [
      option('none', 'No.', 'all_ages'),
      option('yes', 'Yes: the Space itself, a premium edition, an expansion, a cosmetic or a subscription, at a price shown in real money.', 'all_ages', ['in-Space purchases']),
    ],
  },
  {
    id: 'interaction',
    text: 'Can visitors talk to or play with each other?',
    options: [
      option('none', 'No. Each visitor is on their own.', 'all_ages'),
      option('yes', 'Yes: chat, voice, or playing together.', 'all_ages', ['players interact']),
    ],
  },
]);

const byId = new Map(QUESTIONS.map((q) => [q.id, q]));
export const QUESTION_IDS = QUESTIONS.map((q) => q.id);

/// `{ ok: true, answers }` with every question answered by one of its options, in QUESTIONS order, or `{ ok: false, error }`.
/// A partial set is refused: the questionnaire is answered whole or not at all, so "answered" can only mean every question.
export function checkAnswers(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return { ok: false, error: 'answers must be an object with one answer per question' };
  for (const key of Object.keys(value)) if (!byId.has(key)) return { ok: false, error: `there is no question "${String(key).slice(0, 40)}"` };
  const answers = {};
  for (const q of QUESTIONS) {
    const given = value[q.id];
    if (given === undefined) return { ok: false, error: `question "${q.id}" is not answered` };
    if (!q.options.some((o) => o.value === given)) return { ok: false, error: `"${String(given).slice(0, 40)}" is not an answer to "${q.id}"; the answers are ${q.options.map((o) => o.value).join(', ')}` };
    answers[q.id] = given;
  }
  return { ok: true, answers };
}

/// The band, the descriptors and the Code articles a complete set of answers gives: the highest band any answer names, and
/// every descriptor and article any answer adds, in the Code's order. Null when the answers are not a complete, valid set.
export function ratingFromAnswers(value) {
  const checked = checkAnswers(value);
  if (!checked.ok) return null;
  let band = 0;
  const found = new Set();
  const articles = new Set();
  for (const q of QUESTIONS) {
    const o = q.options.find((x) => x.value === checked.answers[q.id]);
    band = Math.max(band, BANDS.indexOf(o.band));
    for (const d of o.descriptors) found.add(d);
    for (const a of o.articles) articles.add(a);
  }
  return { band: BANDS[band], descriptors: DESCRIPTORS.filter((d) => found.has(d)), articles: [...articles].sort() };
}

/// The record the listing keeps. `now` is an ISO time.
export function questionnaireRecord(answers, now = new Date().toISOString()) {
  const rating = ratingFromAnswers(answers);
  if (!rating) return null;
  return { version: QUESTIONNAIRE_VERSION, answers: checkAnswers(answers).answers, band: rating.band, descriptors: rating.descriptors, articles: rating.articles, answered_at: now };
}

/// What review reads from a listing's stored questionnaire: `{ answered, band, descriptors, articles, answers }`. A record
/// that is missing, of another version, or no longer a complete valid set is unanswered (Code Part IV rule 7).
export function questionnaireOf(sim) {
  const unanswered = { answered: false, band: null, descriptors: [], articles: [], answers: null };
  const q = sim?.rating_questionnaire;
  if (!q || q.version !== QUESTIONNAIRE_VERSION) return unanswered;
  const rating = ratingFromAnswers(q.answers);
  if (!rating) return unanswered;
  return { answered: true, band: rating.band, descriptors: rating.descriptors, articles: rating.articles, answers: checkAnswers(q.answers).answers };
}

/// The questionnaire as the publish form and the API show it: each question with its options' values and words. The band an
/// option leads to is not shown, so an author answers what the Space contains rather than aiming at a band.
export function questionnaireForm() {
  return { version: QUESTIONNAIRE_VERSION, questions: QUESTIONS.map((q) => ({ id: q.id, text: q.text, options: q.options.map((o) => ({ value: o.value, label: o.label })) })) };
}
