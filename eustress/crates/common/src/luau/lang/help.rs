//! Signature help, hover and go-to-definition for Luau, from the catalog
//! and the locals the file declares.

use super::complete::{locals, resolve, Ty};
use super::lexer::{initial_state, tokenize_line};
use super::TokenClass;
use crate::luau::catalog::{self, Kind};

/// A token with its byte range in the whole source.
#[derive(Clone, Copy, Debug)]
struct At {
    start: usize,
    end: usize,
    class: TokenClass,
}

fn tokens(source: &str) -> Vec<At> {
    let mut out = Vec::new();
    let mut state = initial_state();
    let mut base = 0;
    for line in source.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let (toks, end) = tokenize_line(body, state);
        state = end;
        for t in toks {
            if !matches!(t.class, TokenClass::Comment | TokenClass::DocComment) {
                out.push(At { start: base + t.start as usize, end: base + (t.start + t.len) as usize, class: t.class });
            }
        }
        base += line.len();
    }
    out
}

/// Where a member access chain ending at token `last` begins: back over
/// names, `.`, `:`, and bracketed calls and indexes.
fn chain_start(source: &str, toks: &[At], last: usize) -> usize {
    let text = |i: usize| &source[toks[i].start..toks[i].end];
    let mut i = last;
    loop {
        if i == 0 {
            return 0;
        }
        let prev = text(i - 1);
        match prev {
            "." | ":" => {
                i -= 1;
                if i == 0 {
                    return 0;
                }
                // Before the dot: a name, or a closing bracket to skip over.
                let before = text(i - 1);
                if before == ")" || before == "]" {
                    let mut depth = 0;
                    let mut j = i - 1;
                    loop {
                        match text(j) {
                            ")" | "]" | "}" => depth += 1,
                            "(" | "[" | "{" => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        if j == 0 {
                            return 0;
                        }
                        j -= 1;
                    }
                    // `j` is the opening bracket; the callee's name is before it.
                    if j == 0 {
                        return 0;
                    }
                    i = j - 1;
                } else {
                    i -= 1;
                }
            }
            _ => return i,
        }
    }
}

/// The member-access expression that ends at byte `end`, as written, calls
/// and indexes included: before the `:` in
/// `game:GetService("Players").LocalPlayer:` it is
/// `game:GetService("Players").LocalPlayer`. `None` when what ends there is
/// not a name or a closing bracket.
pub fn expression_before(source: &str, end: usize) -> Option<String> {
    let before = &source[..end.min(source.len())];
    let toks = tokens(before);
    let last = toks.len().checked_sub(1)?;
    if toks[last].end != before.trim_end().len() {
        return None;
    }
    let text = |i: usize| &before[toks[i].start..toks[i].end];
    // Trailing calls and indexes (`GetPlayers()[1]`): back over each to
    // its opening bracket, then to the name before them.
    let mut name = last;
    while matches!(text(name), ")" | "]") {
        let mut depth = 0;
        let mut j = name;
        loop {
            match text(j) {
                ")" | "]" | "}" => depth += 1,
                "(" | "[" | "{" => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j = j.checked_sub(1)?;
        }
        name = j.checked_sub(1)?;
    }
    if !matches!(
        toks[name].class,
        TokenClass::Variable | TokenClass::Builtin | TokenClass::Property | TokenClass::Method | TokenClass::Function
            | TokenClass::Constant | TokenClass::Parameter
    ) {
        return None;
    }
    let start = chain_start(before, &toks, name);
    Some(before[toks[start].start..toks[last].end].to_string())
}

/// A call's signature, split into parameters, with the one at the caret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    /// `FindFirstChild(name: string, recursive: boolean?)`.
    pub label: String,
    /// Each parameter's byte range within `label`.
    pub params: Vec<(u32, u32)>,
    pub active: u32,
    pub doc: &'static str,
}

/// Parameters split at top-level commas.
fn split_params(params: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    for (i, c) in params.char_indices() {
        match c {
            '(' | '{' | '[' | '<' => depth += 1,
            ')' | '}' | ']' | '>' => depth -= 1,
            ',' if depth == 0 => {
                out.push((start, i));
                start = i + 1;
            }
            _ => {}
        }
    }
    if !params.trim().is_empty() {
        out.push((start, params.len()));
    }
    out.into_iter()
        .map(|(s, e)| {
            let lead = params[s..e].len() - params[s..e].trim_start().len();
            let trail = params[s..e].len() - params[s..e].trim_end().len();
            (s + lead, e - trail)
        })
        .collect()
}

/// The parameters and doc of what `callee` (a name, `a.b` or `a:b`) calls.
fn callee_signature(callee: &str, source_before: &str, script_class: Option<&str>) -> Option<(String, String, &'static str)> {
    let l = locals(source_before, script_class);
    let known = |n: &str| l.iter().rev().find(|(name, _)| name == n).map(|(_, t)| t.clone());
    let split = callee.rfind([':', '.']);
    let (receiver, name) = match split {
        Some(i) => (Some(&callee[..i]), &callee[i + 1..]),
        None => (None, callee),
    };
    match receiver {
        None => catalog::GLOBALS.iter().find(|g| g.name == name && g.kind == Kind::Method).map(|g| (name.into(), g.params.into(), g.doc)),
        Some(recv) => match resolve(recv, &known, script_class)? {
            Ty::Instance(class) => catalog::member(&class, name)
                .filter(|m| matches!(m.kind, Kind::Method | Kind::ServiceMethod))
                .map(|m| (name.into(), m.params.into(), m.doc)),
            Ty::Object(o) => catalog::object_fields(o).iter().find(|f| f.name == name && f.kind == Kind::Method).map(|f| (name.into(), f.params.into(), f.doc)),
            Ty::Library(lib) => catalog::library(lib).iter().find(|f| f.name == name && f.kind == Kind::Method).map(|f| (name.into(), f.params.into(), f.doc)),
            Ty::Signal(p) => match name {
                "Connect" | "Once" => Some((name.into(), format!("fn: ({p}) -> ()"), "Calls `fn` when the signal fires.")),
                _ => None,
            },
            _ => None,
        },
    }
}

/// The signature of the call the caret at `offset` sits in.
pub fn signature_at(source: &str, offset: usize, script_class: Option<&str>) -> Option<Signature> {
    let before = &source[..offset.min(source.len())];
    let toks = tokens(before);
    // The innermost unclosed `(`, and the commas after it at its own depth.
    let (mut depth, mut commas) = (0i32, 0u32);
    let mut open = None;
    for i in (0..toks.len()).rev() {
        match &before[toks[i].start..toks[i].end] {
            ")" | "]" | "}" => depth += 1,
            "[" | "{" => depth -= 1,
            "(" if depth == 0 => {
                open = Some(i);
                break;
            }
            "(" => depth -= 1,
            "," if depth == 0 => commas += 1,
            _ => {}
        }
        if depth < 0 {
            // Inside a table or an index, not a call's arguments.
            return None;
        }
    }
    let open = open?;
    if open == 0 || !matches!(toks[open - 1].class, TokenClass::Function | TokenClass::Method | TokenClass::Builtin | TokenClass::Variable) {
        return None;
    }
    let start = chain_start(before, &toks, open - 1);
    let callee = &before[toks[start].start..toks[open - 1].end];
    let (name, params, doc) = callee_signature(callee, &before[..toks[start].start], script_class)?;
    let label = format!("{name}({params})");
    let base = name.len() + 1;
    let ranges: Vec<(u32, u32)> = split_params(&params).into_iter().map(|(s, e)| ((base + s) as u32, (base + e) as u32)).collect();
    // A trailing `...` takes every argument past it.
    let active = if ranges.is_empty() { 0 } else { commas.min(ranges.len() as u32 - 1) };
    Some(Signature { label, params: ranges, active, doc })
}

/// What the name at `offset` is: markdown, and the byte range it covers.
pub fn hover_at(source: &str, offset: usize, script_class: Option<&str>) -> Option<(String, (u32, u32))> {
    let toks = tokens(source);
    let i = toks.iter().position(|t| t.start <= offset && offset < t.end.max(t.start + 1))?;
    let t = toks[i];
    let word = &source[t.start..t.end];
    let range = (t.start as u32, t.end as u32);
    let before = &source[..t.start];
    let l = locals(before, script_class);
    let known = |n: &str| l.iter().rev().find(|(name, _)| name == n).map(|(_, ty)| ty.clone());
    let access = if i > 0 { Some(&source[toks[i - 1].start..toks[i - 1].end]) } else { None };

    if matches!(access, Some("." | ":")) && i >= 2 {
        let start = chain_start(source, &toks, i - 2);
        let recv = &source[toks[start].start..toks[i - 2].end];
        let md = match resolve(recv, &known, script_class)? {
            Ty::Instance(class) => {
                if let Some(m) = catalog::member(&class, word) {
                    match m.kind {
                        Kind::Event | Kind::ServiceEvent => format!("**{class}.{word}**: RBXScriptSignal ({})\n\n{}", m.params, m.doc),
                        _ => format!("**{class}:{word}**({}){}\n\n{}", m.params, ret(m.returns), m.doc),
                    }
                } else if let Some(p) = catalog::properties_of(&class).into_iter().find(|p| p.name == word) {
                    format!("**{class}.{word}**: {}", p.ty)
                } else {
                    return None;
                }
            }
            Ty::Object(o) | Ty::Library(o) => {
                let fields = if catalog::object_fields(o).is_empty() { catalog::library(o) } else { catalog::object_fields(o) };
                let f = fields.iter().find(|f| f.name == word)?;
                match f.kind {
                    Kind::Method => format!("**{o}.{word}**({}){}\n\n{}", f.params, ret(f.ty), f.doc),
                    _ => format!("**{o}.{word}**: {}\n\n{}", f.ty, f.doc),
                }
            }
            Ty::EnumType(e) => format!("**Enum.{e}.{word}**"),
            Ty::Enums => format!("**Enum.{word}**"),
            _ => return None,
        };
        return Some((md, range));
    }
    // A local, a global, or a library table.
    if let Some(ty) = known(word) {
        let what = match ty {
            Some(Ty::Instance(c)) => c,
            Some(Ty::Object(o)) | Some(Ty::Library(o)) => o.to_string(),
            Some(Ty::Signal(p)) => format!("RBXScriptSignal ({p})"),
            Some(Ty::EnumItem(e)) => format!("Enum.{e}"),
            _ => "unknown".into(),
        };
        return Some((format!("local **{word}**: {what}"), range));
    }
    if let Some(g) = catalog::GLOBALS.iter().find(|g| g.name == word) {
        let md = if g.kind == Kind::Method {
            format!("**{word}**({}){}\n\n{}", g.params, ret(g.ty), g.doc)
        } else {
            format!("**{word}**: {}\n\n{}", g.ty, g.doc)
        };
        return Some((md, range));
    }
    if catalog::LIBRARIES.iter().any(|(n, _)| *n == word) {
        return Some((format!("**{word}**: library"), range));
    }
    None
}

fn ret(returns: &str) -> String {
    if returns.is_empty() {
        String::new()
    } else {
        format!(" -> {returns}")
    }
}

/// Every named function the file declares, as written (`pay`, `Tycoon:Buy`),
/// with the byte range of its last name.
pub fn functions(source: &str) -> Vec<(String, usize, usize)> {
    let toks = tokens(source);
    let mut out = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if &source[t.start..t.end] != "function" || t.class != TokenClass::Keyword {
            continue;
        }
        // The name runs from the next token up to the parameter list.
        let mut j = i + 1;
        while j < toks.len() && &source[toks[j].start..toks[j].end] != "(" && &source[toks[j].start..toks[j].end] != "<" {
            j += 1;
        }
        if j > i + 1 && j <= toks.len() {
            let (first, last) = (toks[i + 1], toks[j - 1]);
            if matches!(last.class, TokenClass::Function | TokenClass::Method | TokenClass::Property) {
                out.push((source[first.start..last.end].to_string(), last.start, last.end));
            }
        }
    }
    out
}

/// Where the local, parameter or function named at `offset` was declared: its
/// name's byte range.
pub fn definition_at(source: &str, offset: usize) -> Option<(usize, usize)> {
    let toks = tokens(source);
    let i = toks.iter().position(|t| t.start <= offset && offset < t.end.max(t.start + 1))?;
    let word = &source[toks[i].start..toks[i].end];
    // A member (`self:Buy()`, `Tycoon.new`): a function this file declares
    // under that last name (`function Tycoon:Buy`).
    if i > 0 && matches!(&source[toks[i - 1].start..toks[i - 1].end], "." | ":") {
        return functions(source)
            .into_iter()
            .find(|(name, _, _)| name.rsplit(['.', ':']).next() == Some(word) && name.contains(['.', ':']))
            .map(|(_, s, e)| (s, e));
    }
    // The nearest declaration before the use: `local name`, a parameter, a
    // `for` variable, or `function name`.
    (0..i).rev().find_map(|j| {
        let t = toks[j];
        if &source[t.start..t.end] != word {
            return None;
        }
        let prev = if j > 0 { &source[toks[j - 1].start..toks[j - 1].end] } else { "" };
        let declares = t.class == TokenClass::Parameter
            || matches!(prev, "local" | "function" | "for")
            || (prev == "," && (0..j).rev().take(8).any(|k| matches!(&source[toks[k].start..toks[k].end], "local" | "for")));
        declares.then_some((t.start, t.end))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_help_follows_the_argument() {
        let src = "local part = workspace:FindFirstChild(\"Door\", ";
        let s = signature_at(src, src.len(), None).expect("inside FindFirstChild");
        assert_eq!(s.label, "FindFirstChild(name: string, recursive: boolean?)");
        assert_eq!(s.active, 1);
        assert_eq!(&s.label[s.params[1].0 as usize..s.params[1].1 as usize], "recursive: boolean?");

        let src = "local Players = game:GetService(\"Players\")\nPlayers.PlayerAdded:Connect(";
        let s = signature_at(src, src.len(), None).expect("inside Connect");
        assert_eq!(s.label, "Connect(fn: (player: Player) -> ())");

        let src = "local v = Vector3.new(1, ";
        assert_eq!(signature_at(src, src.len(), None).unwrap().active, 1);
        let src = "print(math.max(1, 2), ";
        assert_eq!(signature_at(src, src.len(), None).unwrap().label, "print(...: any)");
        assert!(signature_at("local t = { a = 1, ", 19, None).is_none(), "a table is not a call");
    }

    #[test]
    fn hover_names_what_is_under_the_caret() {
        let src = "local Players = game:GetService(\"Players\")\nlocal p = Players.LocalPlayer\np:Kick(\"bye\")";
        let at = src.rfind("Kick").unwrap() + 1;
        let (md, range) = hover_at(src, at, None).unwrap();
        assert!(md.starts_with("**Player:Kick**(message: string?)"), "{md}");
        assert_eq!(&src[range.0 as usize..range.1 as usize], "Kick");
        let (md, _) = hover_at(src, src.find("p:Kick").unwrap(), None).unwrap();
        assert_eq!(md, "local **p**: Player");
        let (md, _) = hover_at(src, src.find("LocalPlayer").unwrap(), None).unwrap();
        assert_eq!(md, "**Players.LocalPlayer**: Player");
    }

    #[test]
    fn the_expression_before_an_access_keeps_its_calls() {
        let src = "local p = game:GetService(\"Players\").LocalPlayer:";
        assert_eq!(expression_before(src, src.len() - 1).as_deref(), Some("game:GetService(\"Players\").LocalPlayer"));
        let src = "for _, x in Players:GetPlayers()[1].";
        assert_eq!(expression_before(src, src.len() - 1).as_deref(), Some("Players:GetPlayers()[1]"));
        let src = "local m = player:GetMouse().";
        assert_eq!(expression_before(src, src.len() - 1).as_deref(), Some("player:GetMouse()"));
        assert_eq!(expression_before("print(1 + ", 9), None, "an operator is not an expression's end");
    }

    #[test]
    fn functions_are_listed_as_written() {
        let src = "local function pay(a)\nend\nfunction Tycoon:Buy(item)\nend\nfunction M.new<T>(x: T)\nend\nlocal f = function() end";
        let names: Vec<String> = functions(src).into_iter().map(|(n, _, _)| n).collect();
        assert_eq!(names, ["pay", "Tycoon:Buy", "M.new"]);
    }

    #[test]
    fn definition_finds_the_declaration() {
        let src = "local cash = 0\nlocal function pay(amount)\n  cash += amount\nend\npay(5)";
        let use_at = src.rfind("cash").unwrap();
        assert_eq!(definition_at(src, use_at), Some((6, 10)));
        let amount = src.rfind("amount").unwrap();
        let (s, e) = definition_at(src, amount).unwrap();
        assert_eq!(&src[s..e], "amount");
        assert!(s < amount);
        let (s, _) = definition_at(src, src.rfind("pay").unwrap()).unwrap();
        assert_eq!(s, src.find("pay").unwrap());

        // A method call finds the method the file declares.
        let src = "function Tycoon:Buy(item)\nend\nfunction Tycoon.new()\n  local self = {}\n  self:Buy(1)\nend";
        let (s, e) = definition_at(src, src.rfind("Buy").unwrap()).unwrap();
        assert_eq!((s, &src[s..e]), (src.find("Buy").unwrap(), "Buy"));
        let (s, e) = definition_at(src, src.rfind("self").unwrap()).unwrap();
        assert_eq!((s, &src[s..e]), (src.find("self").unwrap(), "self"), "the receiver finds its local");
    }
}
