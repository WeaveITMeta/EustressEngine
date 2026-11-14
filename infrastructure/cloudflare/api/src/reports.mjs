// =============================================================================
// Eustress API - Drafts of the offering circular and the Regulation A reports
// =============================================================================
// Drafts Form 1-K (the annual report), Form 1-SA (the semiannual report) and
// the data sections of the offering circular from what this Worker already
// keeps: the monthly books (books.mjs), the holder ledger (holder_ledger.mjs)
// and the offering (subscriptions.mjs). See docs/finance/REGULATION_A_TIER_2.md,
// Stage 6 and Stage 9.
//
// A DRAFT, NEVER A FILING. Nothing here files anything or sends anything to the
// SEC. Every draft says so at its top. The figures, the ownership table and the
// offering terms are filled from the records; every section that needs judgment
// (the business, the risks, management's discussion, the people) is left as a
// marked gap for a person to write. A person reads, completes and signs every
// document before it is filed. The financial figures are the company's own,
// unaudited: an annual report carries the auditor's statements in their place.
//
// Each draft also runs its checks: the period is over, every month's trial
// balance balances, the holder ledger's hash chain and balances verify, and each
// offering's unit counts match its subscriptions. A failed check is listed at the
// top of the draft and in `checks`; the draft is still made, so the problem can
// be seen in context, and it is marked NOT READY.
//
//   GET /api/admin/reports/draft?kind=1-K&year=YYYY[&format=html]          admin
//   GET /api/admin/reports/draft?kind=1-SA&year=YYYY[&format=html]         admin
//   GET /api/admin/reports/draft?kind=circular&year=YYYY[&offering_id=][&format=html]
//
// `year` is the fiscal year by the year it ends in. The fiscal year ends with
// the month in env FISCAL_YEAR_END_MONTH (1 to 12; December when unset). A 1-SA
// covers the first six months of the fiscal year. The circular's figures cover
// the fiscal year named.
//
// The HTML keeps to what EDGAR accepts in an HTML document: no scripts, no
// images, no links, no style sheets, and ASCII text only (anything else becomes
// a character reference or, outside Latin-1, a question mark, and is reported).
//
// Table of Contents:
// 1. Periods
// 2. Figures
// 3. The drafts
// 4. HTML for EDGAR
// 5. Gathering and the route
// =============================================================================

import { closeMonth, trialBalanceOf, ACCOUNTS } from './books.mjs';
import { holderLedgerCall } from './holder_ledger.mjs';

export const REPORT_KINDS = ['1-K', '1-SA', 'circular'];
/// Days after the period ends that a report is due (Rule 257(b)): the annual report in 120, the semiannual in 90.
export const DUE_DAYS = { '1-K': 120, '1-SA': 90 };
/// A holder of this share of the units or more is named in the ownership table.
export const NAMED_HOLDER_PERCENT = 10;

const YEAR = /^\d{4}$/;
const fail = (status, error) => ({ ok: false, status, error });

// -----------------------------------------------------------------------------
// 1. Periods
// -----------------------------------------------------------------------------

/// The fiscal year's last month, 1 to 12, from the environment. December when unset or not a month.
export function fiscalYearEndMonth(env) {
  const m = Number(env?.FISCAL_YEAR_END_MONTH);
  return Number.isInteger(m) && m >= 1 && m <= 12 ? m : 12;
}

const ym = (y, m) => `${y}-${String(m).padStart(2, '0')}`;

/// The months (YYYY-MM) a report covers, and its last day. `year` is the year the fiscal year ends in.
export function periodOf(kind, year, endMonth = 12) {
  // The fiscal year ending in `endMonth` of `year` starts the month after `endMonth` of the year before.
  const months = [];
  let y = endMonth === 12 ? year : year - 1;
  let m = endMonth === 12 ? 1 : endMonth + 1;
  const count = kind === '1-SA' ? 6 : 12;
  for (let i = 0; i < count; i++) {
    months.push(ym(y, m));
    m += 1;
    if (m === 13) { m = 1; y += 1; }
  }
  const [ly, lm] = months[months.length - 1].split('-').map(Number);
  const end = new Date(Date.UTC(ly, lm, 0)).toISOString().slice(0, 10);
  return { from: `${months[0]}-01`, to: end, months };
}

/// The day a report for a period ending on `to` is due, or null for a kind with no due date.
export function dueDate(kind, to) {
  const days = DUE_DAYS[kind];
  if (!days) return null;
  return new Date(Date.parse(`${to}T00:00:00Z`) + days * 86_400_000).toISOString().slice(0, 10);
}

// -----------------------------------------------------------------------------
// 2. Figures
// -----------------------------------------------------------------------------

const range = (lo, hi) => (a) => a.account >= lo && a.account <= hi;
/// An account's balance with the sign of its normal side: positive when it sits where it should.
const normal = (a) => {
  const side = ACCOUNTS[a.account]?.side || 'debit';
  const net = a.debit_cents - a.credit_cents;
  return side === 'debit' ? net : -net;
};

/// The period's figures from its journals: a statement of receipts and costs, and the movement of the three balance accounts.
/// All whole cents. Works from the combined trial balance, so it balances when the months do.
export function figuresOf(journals) {
  const tb = trialBalanceOf(journals.flat());
  const sum = (pred) => tb.accounts.filter(pred).reduce((a, r) => a + normal(r), 0);
  const lines = (pred) => tb.accounts.filter(pred).map((r) => ({ account: r.account, name: r.name, cents: normal(r) }));
  const receipts = lines(range(4000, 4099));
  const gross = sum(range(4000, 4099));
  const refunds = sum(range(4900, 4999));
  const fees = sum(range(5000, 5099));
  const treasury = sum(range(5200, 5299));
  const costs = lines(range(6000, 6999));
  const costTotal = sum(range(6000, 6999));
  const net = gross - refunds;
  const movement = (n) => { const a = tb.accounts.find((r) => r.account === n); return a ? a.debit_cents - a.credit_cents : 0; };
  return {
    receipts, receipts_cents: gross, refunds_cents: refunds, net_receipts_cents: net,
    fees_cents: fees, treasury_share_cents: treasury, costs, costs_cents: costTotal,
    result_cents: net - fees - treasury - costTotal,
    movement: {
      stripe_balance_cents: movement(1000),
      contributor_treasury_cents: -movement(2000),
      paid_outside_stripe_cents: -movement(2100),
    },
    balanced: tb.balanced,
  };
}

/// Holders named in the ownership table: everyone holding NAMED_HOLDER_PERCENT or more of the units, most first.
export function namedHolders(holders, issued) {
  if (!issued) return [];
  return holders
    .filter((h) => h.units * 100 >= issued * NAMED_HOLDER_PERCENT)
    .map((h) => ({ legal_name: h.legal_name, units: h.units, percent: Math.round((h.units * 10_000) / issued) / 100 }));
}

const usd = (cents) => {
  const neg = cents < 0;
  const s = (Math.abs(cents) / 100).toFixed(2).replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  return neg ? `($${s})` : `$${s}`;
};
const count = (n) => String(n).replace(/\B(?=(\d{3})+(?!\d))/g, ',');

// -----------------------------------------------------------------------------
// 3. The drafts
// -----------------------------------------------------------------------------

/// A section a person writes. The draft carries the instruction in its place.
const gap = (what) => ({ type: 'gap', text: what });
const para = (text) => ({ type: 'p', text });
const table = (head, rows) => ({ type: 'table', head, rows });

/// The checks every draft runs, as [{ name, ok, detail }].
export function checksOf(data) {
  const out = [];
  const add = (name, ok, detail = '') => out.push({ name, ok, detail });
  add('The period is over', data.period_over, data.period_over ? '' : `the period ends ${data.period.to}`);
  const unbalanced = data.months.filter((m) => !m.balanced).map((m) => m.month);
  add('Every month\'s trial balance balances', unbalanced.length === 0, unbalanced.join(', '));
  add('The holder ledger verifies', data.ledger_verify.sound === true, (data.ledger_verify.problems || []).slice(0, 5).join('; '));
  const unsound = data.offerings.filter((o) => !o.counts_sound).map((o) => o.offering_id);
  add('Each offering\'s units match its subscriptions', unsound.length === 0, unsound.join(', '));
  if (data.kind === 'circular') add('An offering is named', Boolean(data.offering), data.offering ? '' : 'no offering on file');
  return out;
}

function financialSections(data, audited) {
  const f = data.figures;
  const rows = [
    ...f.receipts.map((r) => [r.name, usd(r.cents)]),
    ['Total receipts', usd(f.receipts_cents)],
    ['Less refunds and disputes', usd(-f.refunds_cents)],
    ['Net receipts', usd(f.net_receipts_cents)],
    ['Payment fees', usd(-f.fees_cents)],
    ['Share of receipts held for contributors (the Treasury)', usd(-f.treasury_share_cents)],
    ...f.costs.map((c) => [c.name, usd(-c.cents)]),
    ['Result for the period', usd(f.result_cents)],
  ];
  return [
    para(`Statement of receipts and costs, ${data.period.from} to ${data.period.to}. Unaudited, prepared from the company's books on a cash basis. Amounts in US dollars.`),
    table(['', 'Amount'], rows),
    table(['Balance movement in the period', 'Amount'], [
      ['Stripe balance', usd(f.movement.stripe_balance_cents)],
      ['Held for contributors', usd(f.movement.contributor_treasury_cents)],
      ['Paid outside Stripe (bank or owner)', usd(f.movement.paid_outside_stripe_cents)],
    ]),
    para('Receipts are what was paid in the period. Whether prepaid Tickets, Delta and plans are revenue when paid or when used is the accountant\'s decision, and these figures do not make it.'),
    audited
      ? gap('Replace this section with the audited financial statements and the independent auditor\'s report for the fiscal year.')
      : gap('The accountant prepares the interim financial statements under US GAAP from these books and reviews them. Insert them here.'),
  ];
}

function ownershipSections(data) {
  const named = namedHolders(data.holders.holders, data.holders.issued_units);
  return [
    para(`Units issued at the end of the period: ${count(data.holders.issued_units)}. Holders of record: ${count(data.holders.counts.holders_of_record)}, of whom ${count(data.holders.counts.non_accredited)} are not accredited investors.`),
    named.length
      ? table([`Holder of ${NAMED_HOLDER_PERCENT}% or more`, 'Units', 'Percent'], named.map((h) => [h.legal_name, count(h.units), `${h.percent.toFixed(2)}%`]))
      : para(`No holder holds ${NAMED_HOLDER_PERCENT}% or more of the units.`),
    gap('Add each officer and manager, with the units they hold and any right to acquire units, and mark which holders above are officers or managers. The ledger reads holdings as of this draft, not as of the period\'s end: confirm the figures for the date the form asks for.'),
  ];
}

function offeringRows(o) {
  return [
    ['Offering', o.title],
    ['SEC file number', o.sec_file_number || 'not qualified'],
    ['Qualified on', o.qualified_on || 'not qualified'],
    ['Price per unit', usd(o.price_cents)],
    ['Units qualified', count(o.max_units)],
    ['Units sold', count(o.sold_units)],
    ['Gross proceeds', usd(o.raised_cents)],
    ['Sold by the company', count(o.sold_units)],
    ['Sold by selling holders', '0'],
    ['Status', o.status],
  ];
}

/// The document as sections, from gathered data. Pure: the same data makes the same draft.
export function draftOf(data) {
  const sections = [];
  const S = (title, body) => sections.push({ title, body });
  const company = data.company_name || '[COMPANY LEGAL NAME]';

  if (data.kind === '1-K') {
    S('Part I: Notification', [
      para(`Annual report of ${company} for the fiscal year ended ${data.period.to}, due ${data.due_on}.`),
      ...(data.offerings.filter((o) => o.sec_file_number).length
        ? data.offerings.filter((o) => o.sec_file_number).map((o) => table(['Summary of the offering', ''], offeringRows(o)))
        : [para('No offering has been qualified.')]),
      gap('Add the fees paid in connection with each offering (audit, legal, state filings), and the net proceeds to the company.'),
    ]);
    S('Item 1. Business', [gap('Describe the business and what changed during the year.')]);
    S('Item 2. Management\'s Discussion and Analysis', [
      gap('Discuss the results below, liquidity and capital resources, and the plan for the year ahead. Every figure quoted is taken from the statements.'),
    ]);
    S('Item 3. Directors and Officers', [gap('Each manager and officer: name, position, age, term, and experience in the past five years, and their compensation.')]);
    S('Item 4. Security Ownership of Management and Certain Securityholders', ownershipSections(data));
    S('Item 5. Interest of Management and Others in Certain Transactions', [gap('Every transaction with a manager, officer or 10% holder above the form\'s threshold, or "None".')]);
    S('Item 6. Other Information', [gap('Anything that should have been reported on Form 1-U during the last quarter and was not, or "None".')]);
    S('Item 7. Financial Statements', financialSections(data, true));
    S('Item 8. Exhibits', [gap('List the exhibits: the operating agreement and any amendment, material contracts, the auditor\'s consent.')]);
  } else if (data.kind === '1-SA') {
    S('Cover', [para(`Semiannual report of ${company} for the six months ended ${data.period.to}, due ${data.due_on}.`)]);
    S('Item 1. Management\'s Discussion and Analysis', [
      gap('Discuss the results below for the six months, against the same six months of the year before where there were operations, and liquidity and capital resources.'),
    ]);
    S('Item 2. Other Information', [gap('Anything that should have been reported on Form 1-U during the period and was not, or "None".')]);
    S('Item 3. Financial Statements', financialSections(data, false));
    S('Item 4. Exhibits', [gap('List the exhibits, or "None".')]);
  } else {
    const o = data.offering;
    S('Cover', [
      para(`Offering circular of ${company}.`),
      ...(o ? [table(['Terms of the offering', ''], offeringRows(o).filter(([k]) => !['Units sold', 'Gross proceeds', 'Sold by the company', 'Sold by selling holders', 'Status'].includes(k)))] : [gap('No offering is on file. Enter one under /api/admin/offerings first.')]),
      gap('Add the legends the form requires on the cover page, word for word as counsel confirms them.'),
    ]);
    S('Summary and Risk Factors', [gap('Summarize the offering, and set out the material risks of buying the units, specific to this company.')]);
    S('Dilution', [gap('Compare the price per unit with the net tangible book value per unit before and after the offering, and with what managers and early holders paid.')]);
    S('Plan of Distribution', o ? [
      para(`The company sells the units itself, through its website, with no underwriter and no commission. The smallest subscription is ${count(o.min_units)} units. The offering is open to residents of: ${o.states.join(', ') || 'no state yet'}. A subscriber who is not an accredited investor may invest at most 10% of the greater of annual income or net worth. Payment is by bank transfer; a subscription is accepted only once its full payment has arrived.`),
      gap('Counsel confirms this paragraph against the final terms, including whether any state requires a broker-dealer or escrow.'),
    ] : [gap('No offering is on file.')]);
    S('Use of Proceeds', [gap('Each named use of the proceeds and its amount, at full and at partial sale. The budget-to-actual report draws from this.')]);
    S('Description of Business', [gap('What the company does, its products, its customers, its competition, its people and its property.')]);
    S('Management\'s Discussion and Analysis', [gap('Results of operations for the periods shown below, liquidity and capital resources, and the plan of operations for the next twelve months.')]);
    S('Directors, Executive Officers and Significant Employees', [gap('Each manager and officer, their experience, and their compensation.')]);
    S('Security Ownership of Management and Certain Securityholders', ownershipSections(data));
    S('Interest of Management and Others in Certain Transactions', [gap('Every related-party transaction, or "None".')]);
    S('Securities Being Offered', [gap('The rights of the units: voting, distributions, transfer restrictions, and what the operating agreement says about them.')]);
    S('Financial Statements', financialSections(data, true));
  }

  const checks = checksOf(data);
  const ready = checks.every((c) => c.ok);
  const gaps = sections.reduce((a, s) => a + s.body.filter((b) => b.type === 'gap').length, 0);
  const title = data.kind === 'circular' ? 'Offering Circular' : `Form ${data.kind}`;
  return { title, kind: data.kind, period: data.period, due_on: data.due_on, ready, checks, gaps, sections };
}

// -----------------------------------------------------------------------------
// 4. HTML for EDGAR
// -----------------------------------------------------------------------------

/// Text made safe for an EDGAR HTML document: markup characters escaped, common typographic characters made ASCII, other
/// Latin-1 characters as numeric references, and anything beyond Latin-1 as a question mark. Returns the text and how many
/// characters were replaced with a question mark.
export function edgarText(s) {
  let lost = 0;
  const ascii = String(s)
    .replace(/[‘’]/g, "'").replace(/[“”]/g, '"').replace(/[–—]/g, '-').replace(/…/g, '...').replace(/ /g, ' ');
  let out = '';
  for (const ch of ascii) {
    const c = ch.codePointAt(0);
    if (ch === '&') out += '&amp;';
    else if (ch === '<') out += '&lt;';
    else if (ch === '>') out += '&gt;';
    else if (ch === '\n' || ch === '\t' || (c >= 0x20 && c < 0x7f)) out += ch;
    else if (c >= 0xa0 && c <= 0xff) out += `&#${c};`;
    else { out += '?'; lost += 1; }
  }
  return { text: out, lost };
}

/// The draft as one HTML document within what EDGAR accepts.
export function draftHtml(draft, { company_name } = {}) {
  let lost = 0;
  const t = (s) => { const r = edgarText(s); lost += r.lost; return r.text; };
  const out = [];
  out.push('<html>', '<head>', `<title>${t(`${draft.title} (draft)`)}</title>`, '</head>', '<body>');
  out.push(`<p><b>DRAFT. NOT FILED.</b> ${t('Prepared from the company\'s records. A person reads, completes and signs every section before filing. Sections in brackets are to be written.')}</p>`);
  if (!draft.ready) {
    out.push(`<p><b>NOT READY.</b> ${t('These checks failed:')}</p>`, '<ul>');
    for (const c of draft.checks.filter((x) => !x.ok)) out.push(`<li>${t(c.name)}${c.detail ? `: ${t(c.detail)}` : ''}</li>`);
    out.push('</ul>');
  }
  out.push(`<h1>${t(draft.title)}</h1>`);
  if (company_name) out.push(`<p>${t(company_name)}</p>`);
  for (const s of draft.sections) {
    out.push(`<h2>${t(s.title)}</h2>`);
    for (const b of s.body) {
      if (b.type === 'p') out.push(`<p>${t(b.text)}</p>`);
      else if (b.type === 'gap') out.push(`<p><b>[TO BE WRITTEN: ${t(b.text)}]</b></p>`);
      else if (b.type === 'table') {
        out.push('<table border="1" cellpadding="4" cellspacing="0">');
        out.push(`<tr>${b.head.map((h) => `<th>${t(h)}</th>`).join('')}</tr>`);
        for (const row of b.rows) out.push(`<tr>${row.map((c, i) => `<td${i > 0 ? ' align="right"' : ''}>${t(c)}</td>`).join('')}</tr>`);
        out.push('</table>');
      }
    }
  }
  out.push('</body>', '</html>');
  return { html: `${out.join('\n')}\n`, lost };
}

// -----------------------------------------------------------------------------
// 5. Gathering and the route
// -----------------------------------------------------------------------------

/// Read everything a draft needs. Reads only. `{ ok: false, status, error }` when a source cannot be read.
export async function gatherReportData(env, { kind, year, offering_id, now = new Date() }) {
  const period = periodOf(kind, year, fiscalYearEndMonth(env));
  const months = [];
  const journals = [];
  for (const month of period.months) {
    // A month that has not begun has nothing in it; it is not read.
    if (month > now.toISOString().slice(0, 7)) { months.push({ month, balanced: true, future: true }); continue; }
    const close = await closeMonth(env, month, { now });
    if (close.error) return fail(close.status || 503, `the books for ${month}: ${close.error}`);
    months.push({ month, balanced: close.trial_balance.balanced });
    journals.push(close.journal);
  }
  const [holders, verify, offerings] = await Promise.all([
    holderLedgerCall(env, 'holders'), holderLedgerCall(env, 'verify'), holderLedgerCall(env, 'offering_list'),
  ]);
  for (const [name, r] of [['holder ledger', holders], ['ledger check', verify], ['offerings', offerings]]) {
    if (r.ok === false) return fail(r.status || 503, `the ${name}: ${r.error}`);
  }
  let offering = null;
  if (kind === 'circular') {
    const live = ['testing', 'open', 'paused'];
    offering = offering_id
      ? offerings.offerings.find((o) => o.offering_id === offering_id) || null
      : offerings.offerings.find((o) => live.includes(o.status)) || offerings.offerings.find((o) => o.status === 'draft') || null;
    if (offering_id && !offering) return fail(404, 'No such offering');
  }
  return {
    ok: true,
    kind, period, months,
    period_over: period.to < now.toISOString().slice(0, 10),
    due_on: dueDate(kind, period.to),
    figures: figuresOf(journals),
    holders, ledger_verify: verify, offerings: offerings.offerings, offering,
    company_name: typeof env.COMPANY_LEGAL_NAME === 'string' && env.COMPANY_LEGAL_NAME.trim() ? env.COMPANY_LEGAL_NAME.trim() : null,
    generated_at: now.toISOString(),
  };
}

/// Handle /api/admin/reports/*; null for any other path. `deps`: { json, requireAdmin }.
export async function handleReportsRoute(request, url, env, cors, deps) {
  if (!url.pathname.startsWith('/api/admin/reports/')) return null;
  const { json } = deps;
  const adminId = await deps.requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);
  if (url.pathname !== '/api/admin/reports/draft' || request.method !== 'GET') return json({ error: 'Not found' }, 404, cors);
  const kind = url.searchParams.get('kind');
  if (!REPORT_KINDS.includes(kind)) return json({ error: `kind is one of ${REPORT_KINDS.join(', ')}` }, 400, cors);
  const yearText = url.searchParams.get('year') || '';
  const year = Number(yearText);
  if (!YEAR.test(yearText) || year < 2020 || year > 2100) return json({ error: 'year is the four-digit year the fiscal year ends in' }, 400, cors);
  const offering_id = url.searchParams.get('offering_id') || undefined;
  const data = await gatherReportData(env, { kind, year, offering_id });
  if (data.ok === false) return json({ error: data.error }, data.status, cors);
  const draft = draftOf(data);
  const { html, lost } = draftHtml(draft, { company_name: data.company_name });
  const headers = { ...cors, 'Cache-Control': 'no-store' };
  if (url.searchParams.get('format') === 'html') {
    const name = `eustress-${kind.toLowerCase()}-${year}-draft.htm`;
    return new Response(html, { status: 200, headers: { ...headers, 'Content-Type': 'text/html; charset=us-ascii', 'Content-Disposition': `attachment; filename="${name}"` } });
  }
  return json({ ...draft, figures: data.figures, generated_at: data.generated_at, characters_replaced: lost, html }, 200, headers);
}
