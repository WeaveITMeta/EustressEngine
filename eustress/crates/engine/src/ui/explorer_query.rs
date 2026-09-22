//! The Explorer search box's query language.
//!
//! A query is a list of terms separated by spaces, all of which must match,
//! with `or` splitting alternatives. Besides plain text (matched against the
//! node's name and the text of any TextLabel beneath it), a term can be:
//!
//! - `is:Part` for an exact class, or a family such as `is:BasePart`,
//!   `is:GuiObject`, `is:Script`, `is:Light`;
//! - `tag:Checkpoint` for a CollectionService tag;
//! - `Locked = true`, `Material = Neon`, `Transparency = 0.5`, `Name = Wall`
//!   for a property (`==` also works, spaces around the operator are fine);
//! - `-term` to require a term NOT to match;
//! - `"two words"` to keep a phrase together.
//!
//! `is:Part Anchored = false or tag:Door` reads as (Part and unanchored) or
//! tagged Door. Matching is case-insensitive throughout.

use eustress_common::classes::BasePart;

#[derive(Debug, Clone, PartialEq)]
enum Term {
    Text(String),
    Class(String),
    Tag(String),
    Prop { key: String, value: String },
}

/// One `or` alternative: every term must match.
#[derive(Debug, Clone, PartialEq, Default)]
struct Clause {
    terms: Vec<(bool, Term)>,
}

/// A parsed query. Empty matches everything.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExplorerQuery {
    clauses: Vec<Clause>,
}

/// What the matcher may ask about a row. The label text is fetched lazily
/// through a closure because walking a subtree for TextLabels is the one
/// expensive part and only plain-text terms need it.
pub struct NodeFacts<'a> {
    pub name: &'a str,
    pub class_name: &'a str,
    pub tags: &'a [String],
    pub base_part: Option<&'a BasePart>,
}

impl ExplorerQuery {
    pub fn parse(input: &str) -> Self {
        let normalised = collapse_operator_spaces(input);
        let mut clauses = vec![Clause::default()];
        for token in tokenize(&normalised) {
            if token.eq_ignore_ascii_case("or") {
                clauses.push(Clause::default());
                continue;
            }
            let (negated, body) = match token.strip_prefix('-') {
                Some(rest) if !rest.is_empty() => (true, rest),
                _ => (false, token.as_str()),
            };
            let term = if let Some(class) = strip_prefix_ci(body, "is:") {
                Term::Class(class.to_lowercase())
            } else if let Some(tag) = strip_prefix_ci(body, "tag:") {
                Term::Tag(tag.to_lowercase())
            } else if let Some((key, value)) = body
                .split_once("==")
                .or_else(|| body.split_once('='))
                .filter(|(k, _)| !k.trim().is_empty())
            {
                Term::Prop {
                    key: key.trim().to_lowercase(),
                    value: value.trim().trim_matches('"').to_lowercase(),
                }
            } else {
                Term::Text(body.to_lowercase())
            };
            let empty = matches!(&term, Term::Text(t) | Term::Class(t) | Term::Tag(t) if t.is_empty());
            if !empty {
                clauses.last_mut().expect("one clause always exists").terms.push((negated, term));
            }
        }
        clauses.retain(|c| !c.terms.is_empty());
        Self { clauses }
    }

    pub fn is_empty(&self) -> bool {
        self.clauses.is_empty()
    }

    /// True when any clause has every term matching. `label_text` is called
    /// at most once, and only if a plain-text term misses the name.
    pub fn matches(&self, facts: &NodeFacts, label_text: &mut dyn FnMut() -> Option<String>) -> bool {
        if self.clauses.is_empty() {
            return true;
        }
        let mut label: Option<Option<String>> = None;
        self.clauses.iter().any(|clause| {
            clause.terms.iter().all(|(negated, term)| {
                let hit = match term {
                    Term::Text(needle) => {
                        text_matches(facts.name, needle) || {
                            let text = label.get_or_insert_with(|| label_text());
                            text.as_deref().map(|t| text_matches(t, needle)).unwrap_or(false)
                        }
                    }
                    Term::Class(want) => class_matches(facts.class_name, want),
                    Term::Tag(want) => facts.tags.iter().any(|t| t.eq_ignore_ascii_case(want)),
                    Term::Prop { key, value } => prop_matches(facts, key, value),
                };
                hit != *negated
            })
        })
    }
}

/// `Locked = true` and `Locked == true` tokenise as one term.
fn collapse_operator_spaces(input: &str) -> String {
    let mut out = input.trim().to_string();
    for (from, to) in [(" == ", "=="), (" = ", "="), ("== ", "=="), ("= ", "="), (" ==", "=="), (" =", "=")] {
        while out.contains(from) {
            out = out.replace(from, to);
        }
    }
    out
}

/// Split on whitespace, keeping double-quoted runs together and dropping the
/// quotes: `Name="Big Wall"` and `"big wall"` are each one token.
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for c in input.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        s.get(prefix.len()..)
    } else {
        None
    }
}

/// Case-insensitive substring, then a second try with whitespace collapsed on
/// both sides so "the ledger" finds a node authored as "TheLedger".
fn text_matches(haystack: &str, needle: &str) -> bool {
    let h = haystack.to_lowercase();
    if h.contains(needle) {
        return true;
    }
    let needle_tight: String = needle.chars().filter(|c| !c.is_whitespace()).collect();
    if needle_tight.is_empty() {
        return false;
    }
    let h_tight: String = h.chars().filter(|c| !c.is_whitespace()).collect();
    h_tight.contains(&needle_tight)
}

/// Class families a query may name instead of one exact class.
fn family_members(family: &str) -> &'static [&'static str] {
    match family {
        "basepart" => &[
            "part", "meshpart", "unionoperation", "wedgepart", "cornerwedgepart", "trusspart",
            "seat", "vehicleseat", "spawnlocation", "cadpart",
        ],
        "pvinstance" => &[
            "part", "meshpart", "unionoperation", "wedgepart", "cornerwedgepart", "trusspart",
            "seat", "vehicleseat", "spawnlocation", "cadpart", "model",
        ],
        "guiobject" => &[
            "frame", "textlabel", "textbutton", "textbox", "imagelabel", "imagebutton",
            "scrollingframe", "videoframe", "viewportframe", "webframe", "documentframe",
        ],
        "gui" | "layercollector" => &["screengui", "billboardgui", "surfacegui"],
        "script" | "luasourcecontainer" => &["soulscript", "script", "localscript", "modulescript"],
        "light" => &["pointlight", "spotlight", "surfacelight", "directionallight"],
        "constraint" => &[
            "weldconstraint", "hingeconstraint", "ropeconstraint", "rodconstraint",
            "springconstraint", "ballsocketconstraint", "prismaticconstraint",
            "cylindricalconstraint", "universalconstraint", "motor6d",
        ],
        _ => &[],
    }
}

fn class_matches(class_name: &str, want: &str) -> bool {
    let class = class_name.to_lowercase();
    class == want || family_members(want).contains(&class.as_str())
}

fn prop_matches(facts: &NodeFacts, key: &str, value: &str) -> bool {
    let bool_eq = |actual: bool| matches!((value, actual), ("true", true) | ("false", false) | ("1", true) | ("0", false) | ("yes", true) | ("no", false));
    let float_eq = |actual: f32| value.parse::<f32>().map(|v| (v - actual).abs() < 1e-3).unwrap_or(false);
    match key {
        "name" => text_matches(facts.name, value),
        "classname" | "class" => facts.class_name.eq_ignore_ascii_case(value),
        _ => {
            let Some(bp) = facts.base_part else { return false };
            match key {
                "locked" => bool_eq(bp.locked),
                "anchored" => bool_eq(bp.anchored),
                "cancollide" => bool_eq(bp.can_collide),
                "castshadow" => bool_eq(bp.cast_shadow),
                "transparency" => float_eq(bp.transparency),
                "reflectance" => float_eq(bp.reflectance),
                "material" => format!("{:?}", bp.material).to_lowercase() == value,
                _ => false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts<'a>(name: &'a str, class: &'a str, tags: &'a [String], bp: Option<&'a BasePart>) -> NodeFacts<'a> {
        NodeFacts { name, class_name: class, tags, base_part: bp }
    }

    fn no_label() -> impl FnMut() -> Option<String> {
        || None
    }

    #[test]
    fn plain_text_matches_name_case_insensitively() {
        let q = ExplorerQuery::parse("ledger");
        assert!(q.matches(&facts("TheLedger", "Part", &[], None), &mut no_label()));
        assert!(!q.matches(&facts("Wall", "Part", &[], None), &mut no_label()));
    }

    #[test]
    fn plain_text_falls_back_to_label_text_once() {
        let q = ExplorerQuery::parse("open sign");
        let mut calls = 0;
        let mut label = || {
            calls += 1;
            Some("OPEN SIGN".to_string())
        };
        assert!(q.matches(&facts("Board", "Part", &[], None), &mut label));
        assert_eq!(calls, 1);
    }

    #[test]
    fn is_matches_exact_class_and_families() {
        let q = ExplorerQuery::parse("is:BasePart");
        assert!(q.matches(&facts("x", "MeshPart", &[], None), &mut no_label()));
        assert!(!q.matches(&facts("x", "Model", &[], None), &mut no_label()));
        let exact = ExplorerQuery::parse("is:model");
        assert!(exact.matches(&facts("x", "Model", &[], None), &mut no_label()));
    }

    #[test]
    fn tag_and_negation() {
        let tags = vec!["Door".to_string()];
        assert!(ExplorerQuery::parse("tag:door").matches(&facts("x", "Part", &tags, None), &mut no_label()));
        assert!(!ExplorerQuery::parse("-tag:door").matches(&facts("x", "Part", &tags, None), &mut no_label()));
    }

    #[test]
    fn property_terms_read_the_base_part() {
        let mut bp = BasePart::default();
        bp.locked = true;
        bp.anchored = false;
        bp.transparency = 0.5;
        let f = facts("x", "Part", &[], Some(&bp));
        assert!(ExplorerQuery::parse("Locked = true").matches(&f, &mut no_label()));
        assert!(ExplorerQuery::parse("locked==true anchored=false").matches(&f, &mut no_label()));
        assert!(ExplorerQuery::parse("Transparency = 0.5").matches(&f, &mut no_label()));
        assert!(!ExplorerQuery::parse("Anchored = true").matches(&f, &mut no_label()));
        // A property term on a row with no BasePart never matches.
        assert!(!ExplorerQuery::parse("Locked = true").matches(&facts("x", "Model", &[], None), &mut no_label()));
    }

    #[test]
    fn or_splits_alternatives_and_quotes_keep_phrases() {
        let q = ExplorerQuery::parse("is:Model or \"big wall\"");
        assert!(q.matches(&facts("Big Wall", "Part", &[], None), &mut no_label()));
        assert!(q.matches(&facts("anything", "Model", &[], None), &mut no_label()));
        assert!(!q.matches(&facts("small wall", "Part", &[], None), &mut no_label()));
    }

    #[test]
    fn empty_query_matches_everything() {
        assert!(ExplorerQuery::parse("   ").is_empty());
        assert!(ExplorerQuery::parse("").matches(&facts("x", "Part", &[], None), &mut no_label()));
    }
}
