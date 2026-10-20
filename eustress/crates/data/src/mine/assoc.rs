//! Association rules: frequent itemsets by Apriori, and the rules they imply.
//!
//! "Customers who buy diapers also buy beer" is a rule `{diaper} → {beer}`
//! with three numbers attached:
//!
//! - **support**: the share of all transactions containing both sides;
//! - **confidence**: of the transactions containing the left side, the share
//!   that also contain the right side;
//! - **lift**: confidence divided by how common the right side is anyway.
//!   Above 1 the sides occur together more than chance predicts; exactly 1
//!   means they are independent, however high the confidence looks.
//!
//! Lift is the number that guards against the obvious: a rule predicting an
//! item nearly everyone buys has high confidence and no information.

use std::collections::HashMap;

use serde::Serialize;

use super::cell_text;
use crate::{ColumnData, DataError, Frame, Result};

/// A set of items that occurs in at least `min_support` of the transactions.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Itemset {
    pub items: Vec<String>,
    pub count: usize,
    pub support: f64,
}

/// `antecedent → consequent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Rule {
    pub antecedent: Vec<String>,
    pub consequent: Vec<String>,
    /// Transactions containing both sides.
    pub count: usize,
    pub support: f64,
    pub confidence: f64,
    pub lift: f64,
}

/// Everything one mining pass found.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Mined {
    pub transactions: usize,
    /// Ordered by support, then size, then items.
    pub itemsets: Vec<Itemset>,
    /// Ordered by confidence, then lift, then support.
    pub rules: Vec<Rule>,
}

/// The Apriori algorithm: grow frequent itemsets one item at a time, pruning
/// any candidate with an infrequent subset (an infrequent set can have no
/// frequent superset, which is what makes the search tractable).
#[derive(Clone, Debug, PartialEq)]
pub struct Apriori {
    pub min_support: f64,
    pub min_confidence: f64,
    /// Largest itemset considered. Rule generation enumerates every split of an
    /// itemset, so this also bounds that cost.
    pub max_len: usize,
}

impl Default for Apriori {
    fn default() -> Self {
        Self { min_support: 0.1, min_confidence: 0.5, max_len: 4 }
    }
}

/// Whether sorted `small` is contained in sorted `big`.
fn is_subset(small: &[u32], big: &[u32]) -> bool {
    let mut it = big.iter();
    small.iter().all(|s| it.any(|b| b == s))
}

impl Apriori {
    pub fn mine<S: AsRef<str>>(&self, transactions: &[Vec<S>]) -> Result<Mined> {
        if !(self.min_support > 0.0 && self.min_support <= 1.0) {
            return Err(DataError::Schema("min_support must be in (0, 1]".into()));
        }
        if !(0.0..=1.0).contains(&self.min_confidence) {
            return Err(DataError::Schema("min_confidence must be in [0, 1]".into()));
        }
        if self.max_len == 0 || self.max_len > 16 {
            return Err(DataError::Schema("max_len must be between 1 and 16".into()));
        }
        let n = transactions.len();
        if n == 0 {
            return Err(DataError::Schema("no transactions to mine".into()));
        }

        // Intern items as sorted ids, so itemsets are sorted id vectors and
        // every ordering below is deterministic.
        let mut vocab: Vec<String> = transactions
            .iter()
            .flat_map(|t| t.iter().map(|s| s.as_ref().to_string()))
            .collect();
        vocab.sort();
        vocab.dedup();
        let id = |s: &str| vocab.binary_search_by(|v| v.as_str().cmp(s)).unwrap_or(0) as u32;
        let tx: Vec<Vec<u32>> = transactions
            .iter()
            .map(|t| {
                let mut v: Vec<u32> = t.iter().map(|s| id(s.as_ref())).collect();
                v.sort_unstable();
                v.dedup();
                v
            })
            .collect();

        // The smallest count that still meets min_support; the epsilon keeps a
        // support of exactly 0.6 on 5 transactions from rounding up to 4.
        let min_count = ((self.min_support * n as f64) - 1e-9).ceil().max(1.0) as usize;

        let mut freq: HashMap<Vec<u32>, usize> = HashMap::new();
        let mut single = vec![0usize; vocab.len()];
        for t in &tx {
            for &i in t {
                single[i as usize] += 1;
            }
        }
        let mut level: Vec<Vec<u32>> = (0..vocab.len() as u32)
            .filter(|&i| single[i as usize] >= min_count)
            .map(|i| vec![i])
            .collect();
        for s in &level {
            freq.insert(s.clone(), single[s[0] as usize]);
        }

        let mut k = 2;
        while !level.is_empty() && k <= self.max_len {
            let mut candidates: Vec<Vec<u32>> = Vec::new();
            for i in 0..level.len() {
                for j in i + 1..level.len() {
                    // `level` is sorted, so sets sharing a (k-2)-prefix are
                    // contiguous: once the prefix differs, no later set joins.
                    if level[i][..k - 2] != level[j][..k - 2] {
                        break;
                    }
                    let mut c = level[i].clone();
                    c.push(level[j][k - 2]);
                    let all_frequent = (0..c.len()).all(|skip| {
                        let sub: Vec<u32> =
                            c.iter().enumerate().filter(|&(p, _)| p != skip).map(|(_, &v)| v).collect();
                        freq.contains_key(&sub)
                    });
                    if all_frequent {
                        candidates.push(c);
                    }
                }
            }
            let mut counts = vec![0usize; candidates.len()];
            for t in tx.iter().filter(|t| t.len() >= k) {
                for (ci, c) in candidates.iter().enumerate() {
                    if is_subset(c, t) {
                        counts[ci] += 1;
                    }
                }
            }
            level = candidates
                .into_iter()
                .zip(counts)
                .filter(|&(_, c)| c >= min_count)
                .map(|(set, c)| {
                    freq.insert(set.clone(), c);
                    set
                })
                .collect();
            k += 1;
        }

        let names = |set: &[u32]| -> Vec<String> { set.iter().map(|&i| vocab[i as usize].clone()).collect() };
        let nf = n as f64;

        let mut rules = Vec::new();
        for (set, &count) in &freq {
            let m = set.len();
            if m < 2 {
                continue;
            }
            for mask in 1u32..(1u32 << m) - 1 {
                let (mut a, mut c) = (Vec::new(), Vec::new());
                for (p, &item) in set.iter().enumerate() {
                    if mask & (1 << p) != 0 { a.push(item) } else { c.push(item) }
                }
                // Both sides are subsets of a frequent set, hence frequent
                // themselves: the lookups cannot miss.
                let (Some(&a_count), Some(&c_count)) = (freq.get(&a), freq.get(&c)) else {
                    continue;
                };
                let confidence = count as f64 / a_count as f64;
                if confidence + 1e-12 < self.min_confidence {
                    continue;
                }
                rules.push(Rule {
                    antecedent: names(&a),
                    consequent: names(&c),
                    count,
                    support: count as f64 / nf,
                    confidence,
                    lift: confidence / (c_count as f64 / nf),
                });
            }
        }
        rules.sort_by(|p, q| {
            q.confidence
                .total_cmp(&p.confidence)
                .then(q.lift.total_cmp(&p.lift))
                .then(q.support.total_cmp(&p.support))
                .then(p.antecedent.cmp(&q.antecedent))
                .then(p.consequent.cmp(&q.consequent))
        });

        let mut itemsets: Vec<Itemset> = freq
            .iter()
            .map(|(set, &count)| Itemset { items: names(set), count, support: count as f64 / nf })
            .collect();
        itemsets.sort_by(|p, q| {
            q.count.cmp(&p.count).then(p.items.len().cmp(&q.items.len())).then(p.items.cmp(&q.items))
        });

        Ok(Mined { transactions: n, itemsets, rules })
    }
}

/// Transactions from a long-format table: one row per (transaction, item), the
/// shape an order-lines export arrives in. Transactions keep the order their
/// first row appears; rows missing either value are skipped.
pub fn transactions_from_frame(frame: &Frame, id_col: &str, item_col: &str) -> Result<Vec<Vec<String>>> {
    let ids = frame
        .column(id_col)
        .ok_or_else(|| DataError::Schema(format!("no column `{id_col}`")))?;
    let items = frame
        .column(item_col)
        .ok_or_else(|| DataError::Schema(format!("no column `{item_col}`")))?;
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<Vec<String>> = Vec::new();
    for r in 0..frame.n_rows() {
        let (Some(t), Some(item)) = (cell_text(ids, r), cell_text(items, r)) else {
            continue;
        };
        let slot = *index.entry(t).or_insert_with(|| {
            out.push(Vec::new());
            out.len() - 1
        });
        out[slot].push(item);
    }
    Ok(out)
}

/// Transactions from a wide table of flags: one row per transaction, one
/// column per item, the item present where its cell is true or a nonzero
/// number. The shape a one-hot export arrives in. A row with no flag set is
/// still a transaction, and counts toward every support.
pub fn transactions_from_flags(frame: &Frame, cols: &[&str]) -> Result<Vec<Vec<String>>> {
    if cols.is_empty() {
        return Err(DataError::Schema("no item columns selected".into()));
    }
    let mut data = Vec::with_capacity(cols.len());
    for &name in cols {
        let col = frame
            .column(name)
            .ok_or_else(|| DataError::Schema(format!("no column `{name}`")))?;
        if matches!(col, ColumnData::Str(_)) {
            return Err(DataError::Schema(format!(
                "item column `{name}` holds text; flags must be true/false or 0/1"
            )));
        }
        data.push(col);
    }
    Ok((0..frame.n_rows())
        .map(|r| {
            cols.iter()
                .zip(&data)
                .filter(|(_, col)| match col {
                    ColumnData::Bool(v) => v[r] == Some(true),
                    ColumnData::I64(v) => v[r].is_some_and(|x| x != 0),
                    ColumnData::F64(v) => v[r].is_some_and(|x| x != 0.0 && x.is_finite()),
                    ColumnData::Str(_) => false,
                })
                .map(|(name, _)| name.to_string())
                .collect()
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{frame_from_columns, ColumnDtype, ColumnSpec};

    /// The market-basket example of Tan, Steinbach and Kumar,
    /// "Introduction to Data Mining", chapter 6.
    fn baskets() -> Vec<Vec<&'static str>> {
        vec![
            vec!["bread", "milk"],
            vec!["bread", "diaper", "beer", "eggs"],
            vec!["milk", "diaper", "beer", "cola"],
            vec!["bread", "milk", "diaper", "beer"],
            vec!["bread", "milk", "diaper", "cola"],
        ]
    }

    fn support_of(m: &Mined, items: &[&str]) -> Option<f64> {
        let mut want: Vec<String> = items.iter().map(|s| s.to_string()).collect();
        want.sort();
        m.itemsets.iter().find(|s| s.items == want).map(|s| s.support)
    }

    fn rule<'a>(m: &'a Mined, a: &[&str], c: &[&str]) -> Option<&'a Rule> {
        m.rules.iter().find(|r| r.antecedent == a && r.consequent == c)
    }

    #[test]
    fn the_textbook_supports_come_out_exactly() {
        let m = Apriori { min_support: 0.6, min_confidence: 0.0, max_len: 3 }.mine(&baskets()).unwrap();
        for (items, want) in [
            (&["bread"][..], 0.8),
            (&["milk"][..], 0.8),
            (&["diaper"][..], 0.8),
            (&["beer"][..], 0.6),
            (&["beer", "diaper"][..], 0.6),
            (&["bread", "milk"][..], 0.6),
        ] {
            assert_eq!(support_of(&m, items), Some(want), "{items:?}");
        }
        assert_eq!(support_of(&m, &["cola"]), None, "0.4 is under the threshold");
        assert_eq!(m.itemsets.len(), 8, "4 singletons and 4 pairs clear 0.6: {:?}", m.itemsets);
    }

    #[test]
    fn the_textbook_rules_carry_the_right_confidence_and_lift() {
        let m = Apriori { min_support: 0.4, min_confidence: 0.6, max_len: 3 }.mine(&baskets()).unwrap();
        let r = rule(&m, &["beer"], &["diaper"]).expect("beer → diaper");
        assert!((r.confidence - 1.0).abs() < 1e-12);
        assert!((r.lift - 1.25).abs() < 1e-12);

        let r = rule(&m, &["diaper", "milk"], &["beer"]).expect("{diaper, milk} → beer");
        assert!((r.support - 0.4).abs() < 1e-12);
        assert!((r.confidence - 2.0 / 3.0).abs() < 1e-12);
        assert!((r.lift - 10.0 / 9.0).abs() < 1e-12);
    }

    #[test]
    fn rules_below_min_confidence_are_dropped() {
        let m = Apriori { min_support: 0.4, min_confidence: 0.9, max_len: 3 }.mine(&baskets()).unwrap();
        assert!(m.rules.iter().all(|r| r.confidence >= 0.9 - 1e-12));
        assert!(rule(&m, &["beer"], &["diaper"]).is_some(), "confidence 1.0 survives");
        assert!(rule(&m, &["diaper"], &["beer"]).is_none(), "confidence 0.75 does not");
    }

    #[test]
    fn independent_items_have_a_lift_of_one() {
        // a and b each appear in half the baskets, independently.
        let t = vec![vec!["a", "b"], vec!["a"], vec!["b"], vec!["c"]];
        let m = Apriori { min_support: 0.25, min_confidence: 0.0, max_len: 2 }.mine(&t).unwrap();
        let r = rule(&m, &["a"], &["b"]).unwrap();
        assert!((r.lift - 1.0).abs() < 1e-12, "{r:?}");
    }

    #[test]
    fn duplicate_items_in_a_basket_count_once() {
        let t = vec![vec!["a", "a", "a"], vec!["b"]];
        let m = Apriori { min_support: 0.5, min_confidence: 0.0, max_len: 2 }.mine(&t).unwrap();
        assert_eq!(support_of(&m, &["a"]), Some(0.5));
    }

    #[test]
    fn the_output_order_is_deterministic() {
        let spec = Apriori { min_support: 0.4, min_confidence: 0.0, max_len: 3 };
        assert_eq!(spec.mine(&baskets()).unwrap(), spec.mine(&baskets()).unwrap());
    }

    #[test]
    fn transactions_come_from_long_format_order_lines() {
        let f = frame_from_columns(vec![
            (
                ColumnSpec::new("order", ColumnDtype::I64),
                ColumnData::I64(vec![Some(7), Some(7), Some(9), None, Some(7)]),
            ),
            (
                ColumnSpec::new("sku", ColumnDtype::Str),
                ColumnData::Str(vec![
                    Some("A".into()),
                    Some("B".into()),
                    Some("A".into()),
                    Some("Z".into()),
                    Some("C".into()),
                ]),
            ),
        ])
        .unwrap();
        let t = transactions_from_frame(&f, "order", "sku").unwrap();
        assert_eq!(t, vec![vec!["A", "B", "C"], vec!["A"]]);
    }

    #[test]
    fn transactions_come_from_one_hot_flags() {
        let f = frame_from_columns(vec![
            (
                ColumnSpec::new("milk", ColumnDtype::Bool),
                ColumnData::Bool(vec![Some(true), Some(false), None]),
            ),
            (ColumnSpec::new("eggs", ColumnDtype::I64), ColumnData::I64(vec![Some(1), Some(2), Some(0)])),
        ])
        .unwrap();
        let t = transactions_from_flags(&f, &["milk", "eggs"]).unwrap();
        assert_eq!(t, vec![vec!["milk", "eggs"], vec!["eggs"], vec![]]);
    }
}
