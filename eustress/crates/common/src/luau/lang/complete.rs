//! Completion for Luau, from the catalog: what fits after `game:`,
//! `part.`, `Enum.KeyCode.`, inside `GetService("`, or at a bare name.
//!
//! What an expression is comes from walking it: `game:GetService("Players")`
//! is the Players service, `.LocalPlayer` a Player, `:GetMouse()` its mouse.
//! Locals get a type from what they were set to, from an annotation, from the
//! list a `for` loop walks, or from the signal a handler is connected to.

use super::lexer::tokenize_line;
use super::{Token, TokenClass};
use crate::luau::catalog::{self, Kind};

/// What an expression is, as far as completion needs to know.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ty {
    /// An instance of this class.
    Instance(String),
    /// A value that is not an instance: `Vector3`, `PlayerMouse`, ...
    Object(&'static str),
    /// A signal, with the parameters its handlers receive.
    Signal(&'static str),
    /// A global table: `math`, `Vector3`, `task`, ...
    Library(&'static str),
    /// `Enum`.
    Enums,
    /// `Enum.KeyCode`.
    EnumType(String),
    /// `Enum.KeyCode.W`.
    EnumItem(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Dot,
    Colon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Method,
    Property,
    Event,
    Callback,
    Function,
    Field,
    Constant,
    Library,
    Class,
    Service,
    EnumType,
    EnumItem,
    Keyword,
    Variable,
    Child,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub kind: ItemKind,
    /// A signature or a type: `(name: string, recursive: boolean?) -> Instance?`.
    pub detail: String,
    pub doc: &'static str,
}

/// Where the caret is.
#[derive(Clone, Debug, Default)]
pub struct Context<'a> {
    /// The identifier characters just before the caret.
    pub prefix: &'a str,
    /// The expression before a `.` or `:`, as written.
    pub receiver: Option<&'a str>,
    pub access: Option<Access>,
    /// Inside a string argument: the callee as written (`game:GetService`,
    /// `Instance.new`) and the argument's index.
    pub in_string: Option<(&'a str, u32)>,
    /// `Script`, `LocalScript` or `ModuleScript`.
    pub script_class: Option<&'a str>,
    /// The locals in scope, most recent last, with what they are when known.
    pub locals: &'a [(String, Option<Ty>)],
}

pub const KEYWORDS: &[&str] = &[
    "and", "break", "continue", "do", "else", "elseif", "end", "export", "false", "for", "function", "if", "in",
    "local", "nil", "not", "or", "repeat", "return", "then", "true", "type", "until", "while",
];

// ── Types from catalog text ──────────────────────────────────────────────────

fn is_object(name: &str) -> Option<&'static str> {
    catalog::OBJECTS.iter().map(|(n, _)| *n).find(|n| *n == name)
}

/// A catalog type (`Instance?`, `ArgClass`, `{Player}`, `Enum.KeyCode`,
/// `(CFrame, Vector3)`) as a [`Ty`]. `self_class` stands for `Self`,
/// `arg_class` for `ArgClass`.
pub fn ty_from(text: &str, self_class: Option<&str>, arg_class: Option<&str>) -> Option<Ty> {
    let t = text.trim().trim_end_matches('?');
    // A tuple: its first value.
    let t = t.strip_prefix('(').map_or(t, |inner| inner.split(',').next().unwrap_or("").trim_end_matches(')').trim());
    if t.is_empty() || t.starts_with('{') || t.starts_with("...") {
        return None;
    }
    if let Some(e) = t.strip_prefix("Enum.") {
        return Some(Ty::EnumItem(e.to_string()));
    }
    match t {
        "Self" => self_class.map(|c| Ty::Instance(c.to_string())),
        "ArgClass" => Some(Ty::Instance(arg_class.unwrap_or("Instance").to_string())),
        "RBXScriptSignal" => Some(Ty::Signal("...: any")),
        "number" | "string" | "boolean" | "any" | "nil" | "never" | "thread" | "iterator" => None,
        _ if t.starts_with(|c: char| c.is_ascii_uppercase()) && t.chars().all(|c| c.is_ascii_alphanumeric()) => {
            Some(is_object(t).map_or_else(|| Ty::Instance(t.to_string()), Ty::Object))
        }
        _ => None,
    }
}

/// The element type of a list type such as `{Player}`.
fn element_of(text: &str) -> Option<&str> {
    let t = text.trim().trim_end_matches('?');
    let inner = t.strip_prefix('{')?.strip_suffix('}')?.trim();
    (!inner.starts_with('[')).then_some(inner)
}

// ── Walking an expression ────────────────────────────────────────────────────

/// What the walk has so far.
#[derive(Clone, Debug)]
enum Step {
    Ty(Ty),
    /// A list, by its element type's text.
    List(String),
    /// A function not yet called: what calling it returns.
    Callable { returns: &'static str, self_class: Option<String> },
    Unknown,
}

/// The code words of `src`, each with the byte where it starts. Comments are
/// left out, and a string literal is one word, quotes and all: the lexer
/// hands it over in pieces (the opening quote, the text, each escape), and a
/// call's class argument (`GetService("Players")`) must be read whole.
fn words_at(src: &str) -> Vec<(TokenClass, String, usize)> {
    let (tokens, _) = tokenize_line(src, super::lexer::initial_state());
    let mut out: Vec<(TokenClass, String, usize)> = Vec::new();
    let mut end = usize::MAX;
    for t in tokens.iter().filter(|t: &&Token| !matches!(t.class, TokenClass::Comment | TokenClass::DocComment)) {
        let (start, stop) = (t.start as usize, (t.start + t.len) as usize);
        let text = &src[start..stop];
        let piece = matches!(t.class, TokenClass::String | TokenClass::StringEscape);
        match out.last_mut() {
            Some((TokenClass::String, word, _)) if piece && start == end => word.push_str(text),
            _ => out.push((if piece { TokenClass::String } else { t.class }, text.to_string(), start)),
        }
        end = stop;
    }
    out
}

fn words(src: &str) -> Vec<(TokenClass, String)> {
    words_at(src).into_iter().map(|(class, word, _)| (class, word)).collect()
}

/// The string literal's contents, when `w` is one (`"Players"`).
fn string_value(w: &str) -> Option<&str> {
    let q = w.chars().next()?;
    (matches!(q, '"' | '\'' | '`') && w.len() >= 2 && w.ends_with(q)).then(|| &w[1..w.len() - 1])
}

/// The class a string argument names, when the callee takes a class name.
fn names_a_class(callee: &str) -> bool {
    matches!(
        callee,
        "GetService" | "FindService" | "FindFirstChildOfClass" | "FindFirstChildWhichIsA" | "FindFirstAncestorOfClass"
            | "FindFirstAncestorWhichIsA" | "new"
    )
}

/// What `expr` is. `local` answers names the file declared.
pub fn resolve(expr: &str, local: &dyn Fn(&str) -> Option<Option<Ty>>, script_class: Option<&str>) -> Option<Ty> {
    let w = words(expr);
    if w.is_empty() {
        return None;
    }
    let mut i = 0;
    // The start: a local, a global, `Enum`, or a library table.
    let first = w[0].1.as_str();
    let mut step = match local(first) {
        Some(Some(ty)) => Step::Ty(ty),
        Some(None) => Step::Unknown,
        None => match first {
            "game" | "Game" => Step::Ty(Ty::Instance("DataModel".into())),
            "workspace" | "Workspace" => Step::Ty(Ty::Instance("Workspace".into())),
            "script" => Step::Ty(Ty::Instance(script_class.unwrap_or("LuaSourceContainer").into())),
            "Enum" => Step::Ty(Ty::Enums),
            name => match catalog::LIBRARIES.iter().find(|(n, _)| *n == name) {
                Some((n, _)) => Step::Ty(Ty::Library(n)),
                None => Step::Unknown,
            },
        },
    };
    i += 1;
    // The last member name, for a call's class argument.
    let mut last_name = String::new();
    while i < w.len() {
        let t = w[i].1.as_str();
        match t {
            "." | ":" => {
                let Some(name) = w.get(i + 1).map(|x| x.1.clone()) else { return None };
                step = member_step(&step, &name, t == ":");
                last_name = name;
                i += 2;
            }
            "(" => {
                // Skip to the matching `)`, keeping the first argument when it
                // is a string.
                let mut depth = 0;
                let mut first_arg: Option<String> = None;
                let mut j = i;
                while j < w.len() {
                    match w[j].1.as_str() {
                        "(" | "{" | "[" => depth += 1,
                        ")" | "}" | "]" => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        s if j == i + 1 => first_arg = string_value(s).map(str::to_string),
                        _ => {}
                    }
                    j += 1;
                }
                let arg = first_arg.filter(|_| names_a_class(&last_name));
                step = match step {
                    Step::Callable { returns, self_class } => {
                        if let Some(elem) = element_of(returns) {
                            Step::List(elem.to_string())
                        } else {
                            ty_from(returns, self_class.as_deref(), arg.as_deref()).map_or(Step::Unknown, Step::Ty)
                        }
                    }
                    _ => Step::Unknown,
                };
                i = j + 1;
            }
            "[" => {
                let mut depth = 0;
                let mut j = i;
                while j < w.len() {
                    match w[j].1.as_str() {
                        "[" | "(" | "{" => depth += 1,
                        "]" | ")" | "}" => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                step = match step {
                    Step::List(elem) => ty_from(&elem, None, None).map_or(Step::Unknown, Step::Ty),
                    _ => Step::Unknown,
                };
                i = j + 1;
            }
            // A string call (`require "x"`) or anything else ends what can
            // be known.
            _ => return None,
        }
    }
    match step {
        Step::Ty(ty) => Some(ty),
        _ => None,
    }
}

/// `class`'s member `name`. An instance of a class nothing here knows (a
/// child such as `workspace.Pad`, or what `FindFirstChild` returns) may be of
/// any class, so there a member any class has counts.
fn member_on(class: &str, name: &str) -> Option<&'static catalog::Member> {
    catalog::member(class, name).or_else(|| {
        (class == "Instance").then(|| catalog::MEMBERS.iter().find(|m| m.name == name && !m.deprecated)).flatten()
    })
}

fn member_step(step: &Step, name: &str, colon: bool) -> Step {
    let Step::Ty(ty) = step else { return Step::Unknown };
    match ty {
        Ty::Instance(class) => {
            if colon {
                return match member_on(class, name) {
                    Some(m) if matches!(m.kind, Kind::Method | Kind::ServiceMethod) => {
                        Step::Callable { returns: m.returns, self_class: Some(class.clone()) }
                    }
                    _ => Step::Unknown,
                };
            }
            if let Some(m) = member_on(class, name) {
                return match m.kind {
                    Kind::Event | Kind::ServiceEvent => Step::Ty(Ty::Signal(m.params)),
                    Kind::Method | Kind::ServiceMethod => {
                        Step::Callable { returns: m.returns, self_class: Some(class.clone()) }
                    }
                    Kind::Callback => Step::Unknown,
                };
            }
            if let Some(p) = catalog::properties_of(class).into_iter().find(|p| p.name == name) {
                return ty_from(&p.ty, Some(class), None).map_or(Step::Unknown, Step::Ty);
            }
            if let Some((_, _, child)) = catalog::SESSION_CHILDREN.iter().find(|(c, n, _)| c == class && *n == name) {
                return Step::Ty(Ty::Instance((*child).into()));
            }
            // `game.Players`: a service by name.
            if class == "DataModel" && catalog::services().contains(&name) {
                return Step::Ty(Ty::Instance(name.into()));
            }
            // Any other name is a child, of a class nothing here knows.
            Step::Ty(Ty::Instance("Instance".into()))
        }
        Ty::Object(o) | Ty::Library(o) => {
            let fields = if matches!(ty, Ty::Object(_)) { catalog::object_fields(o) } else { catalog::library(o) };
            match fields.iter().find(|f| f.name == name) {
                Some(f) if f.kind == Kind::Method => Step::Callable { returns: f.ty, self_class: None },
                Some(f) if f.kind == Kind::Event => Step::Ty(Ty::Signal(f.params)),
                Some(f) => ty_from(f.ty, None, None).map_or(Step::Unknown, Step::Ty),
                None => Step::Unknown,
            }
        }
        Ty::Signal(_) if colon => match name {
            "Connect" | "Once" => Step::Callable { returns: "RBXScriptConnection", self_class: None },
            _ => Step::Unknown,
        },
        Ty::Enums if !colon => Step::Ty(Ty::EnumType(name.into())),
        Ty::EnumType(t) if !colon => Step::Ty(Ty::EnumItem(t.clone())),
        _ => Step::Unknown,
    }
}

// ── Locals ───────────────────────────────────────────────────────────────────

/// The locals `source` declares, in order, with what each is when that can
/// be read off its line: `local x = <expr>`, `local x: T`, an annotated
/// parameter, `for _, v in ipairs(<list>)`, and a handler's parameters in
/// `<signal>:Connect(function(a, b)`.
pub fn locals(source: &str, script_class: Option<&str>) -> Vec<(String, Option<Ty>)> {
    let mut out: Vec<(String, Option<Ty>)> = Vec::new();
    let mut state = super::lexer::initial_state();
    for line in source.lines() {
        let (tokens, end) = tokenize_line(line, state);
        state = end;
        let w: Vec<(TokenClass, &str)> = tokens
            .iter()
            .filter(|t| !matches!(t.class, TokenClass::Comment | TokenClass::DocComment))
            .map(|t| (t.class, &line[t.start as usize..(t.start + t.len) as usize]))
            .collect();
        let lookup = |out: &Vec<(String, Option<Ty>)>, name: &str| -> Option<Option<Ty>> {
            out.iter().rev().find(|(n, _)| n == name).map(|(_, t)| t.clone())
        };
        let text_from = |k: usize| -> String {
            match w.get(k) {
                Some(_) => {
                    let start = tokens
                        .iter()
                        .filter(|t| !matches!(t.class, TokenClass::Comment | TokenClass::DocComment))
                        .nth(k)
                        .map_or(0, |t| t.start as usize);
                    let end = tokens
                        .iter()
                        .rev()
                        .find(|t| !matches!(t.class, TokenClass::Comment | TokenClass::DocComment))
                        .map_or(line.len(), |t| (t.start + t.len) as usize);
                    line[start..end].to_string()
                }
                None => String::new(),
            }
        };
        let mut k = 0;
        while k < w.len() {
            let (class, word) = w[k];
            // `local name`, `local name: T`, `local name = expr`,
            // `local a, b = ...` (the names only).
            if class == TokenClass::Keyword && word == "local" && w.get(k + 1).is_some_and(|x| x.1 != "function") {
                let mut names: Vec<(String, Option<Ty>)> = Vec::new();
                let mut j = k + 1;
                while let Some((TokenClass::Variable, name)) = w.get(j) {
                    let mut ty = None;
                    if w.get(j + 1).is_some_and(|x| x.1 == ":") {
                        ty = w.get(j + 2).and_then(|x| ty_from(x.1, None, None));
                        j += 2;
                        // Skip the rest of the annotation.
                        while w.get(j + 1).is_some_and(|x| x.0 == TokenClass::Type || matches!(x.1, "?" | "." | "<" | ">")) {
                            j += 1;
                        }
                    }
                    names.push((name.to_string(), ty));
                    j += 1;
                    if w.get(j).is_some_and(|x| x.1 == ",") {
                        j += 1;
                    } else {
                        break;
                    }
                }
                if names.len() == 1 && names[0].1.is_none() && w.get(j).is_some_and(|x| x.1 == "=") {
                    let expr = text_from(j + 1);
                    let known = |n: &str| lookup(&out, n);
                    names[0].1 = resolve(&expr, &known, script_class);
                }
                out.extend(names);
                k = j + 1;
                continue;
            }
            // Parameters: `function f(a: T, b)` and `function(a, b)`.
            if class == TokenClass::Parameter {
                let ty = if w.get(k + 1).is_some_and(|x| x.1 == ":") {
                    w.get(k + 2).and_then(|x| ty_from(x.1, None, None))
                } else {
                    None
                };
                out.push((word.to_string(), ty));
                k += 1;
                continue;
            }
            // `for a, b in ipairs(list)` / `pairs(list)`: b is an element.
            if class == TokenClass::ControlFlow && word == "for" {
                let mut vars: Vec<String> = Vec::new();
                let mut j = k + 1;
                while let Some((TokenClass::Variable, name)) = w.get(j) {
                    vars.push(name.to_string());
                    j += 1;
                    if w.get(j).is_some_and(|x| x.1 == ",") {
                        j += 1;
                    } else {
                        break;
                    }
                }
                let mut elem = None;
                if w.get(j).is_some_and(|x| x.1 == "in") && w.get(j + 1).is_some_and(|x| matches!(x.1, "ipairs" | "pairs")) {
                    let rest = text_from(j + 2);
                    // `(list) do`: the list is inside the first parentheses.
                    if let Some(inner) = rest.strip_prefix('(').and_then(|r| r.rsplit_once(") do").map(|(a, _)| a)) {
                        let known = |n: &str| lookup(&out, n);
                        elem = list_element(inner, &known, script_class);
                    }
                }
                for (idx, v) in vars.into_iter().enumerate() {
                    out.push((v, if idx == 1 { elem.clone() } else { None }));
                }
                k = j;
                continue;
            }
            // `<signal>:Connect(function(a, b)`: a and b are what it fires with.
            if word == "function" && k >= 2 && w[k - 1].1 == "(" && matches!(w[k - 2].1, "Connect" | "Once") {
                let expr: String = {
                    // The signal: everything before `:Connect`.
                    let colon = k - 3;
                    let start = tokens.iter().filter(|t| !matches!(t.class, TokenClass::Comment | TokenClass::DocComment)).next().map_or(0, |t| t.start as usize);
                    let end = tokens
                        .iter()
                        .filter(|t| !matches!(t.class, TokenClass::Comment | TokenClass::DocComment))
                        .nth(colon)
                        .map_or(0, |t| t.start as usize);
                    signal_expr(&line[start..end])
                };
                let known = |n: &str| lookup(&out, n);
                let params = match resolve(&expr, &known, script_class) {
                    Some(Ty::Signal(p)) => p,
                    _ => "",
                };
                let types: Vec<Option<Ty>> = params
                    .split(',')
                    .map(|p| p.split_once(':').and_then(|(_, t)| ty_from(t, None, None)))
                    .collect();
                let mut j = k + 2;
                let mut idx = 0;
                while let Some((TokenClass::Parameter, name)) = w.get(j) {
                    out.push((name.to_string(), types.get(idx).cloned().flatten()));
                    idx += 1;
                    j += 1;
                    if w.get(j).is_some_and(|x| x.1 == ",") {
                        j += 1;
                    } else {
                        break;
                    }
                }
                k = j;
                continue;
            }
            k += 1;
        }
    }
    out
}

/// The signal expression at the end of `before` (what precedes
/// `:Connect`): from the last statement boundary on.
fn signal_expr(before: &str) -> String {
    let trimmed = before.trim_end();
    let start = trimmed.rfind(|c: char| c == '=' || c == ';' || c == ' ').map_or(0, |i| i + 1);
    trimmed[start..].to_string()
}

fn list_element(expr: &str, local: &dyn Fn(&str) -> Option<Option<Ty>>, script_class: Option<&str>) -> Option<Ty> {
    // The list is a call whose catalog return is `{T}`: resolve up to the
    // last call, then read its element type.
    let w = words_at(expr);
    let colon = w.iter().rposition(|x| x.1 == ":" || x.1 == ".")?;
    let receiver = expr[..w[colon].2].to_string();
    let name = &w.get(colon + 1)?.1;
    let owner = if receiver.is_empty() { None } else { resolve(&receiver, local, script_class) };
    let returns = match owner? {
        Ty::Instance(class) => catalog::member(&class, name)?.returns,
        Ty::Object(o) => catalog::object_fields(o).iter().find(|f| f.name == name)?.ty,
        Ty::Library(l) => catalog::library(l).iter().find(|f| f.name == name)?.ty,
        _ => return None,
    };
    ty_from(element_of(returns)?, None, None)
}

// ── Completion ───────────────────────────────────────────────────────────────

fn signature(params: &str, returns: &str) -> String {
    if returns.is_empty() {
        format!("({params})")
    } else {
        format!("({params}) -> {returns}")
    }
}

fn member_items(ty: &Ty, access: Access) -> Vec<Item> {
    let mut out = Vec::new();
    match ty {
        Ty::Instance(class) => {
            for m in catalog::members_of(class) {
                let (want, kind) = match m.kind {
                    Kind::Method | Kind::ServiceMethod => (Access::Colon, ItemKind::Method),
                    Kind::Event | Kind::ServiceEvent => (Access::Dot, ItemKind::Event),
                    Kind::Callback => (Access::Dot, ItemKind::Callback),
                };
                if want == access {
                    let detail = match m.kind {
                        Kind::Event | Kind::ServiceEvent => format!("RBXScriptSignal ({})", m.params),
                        _ => signature(m.params, m.returns),
                    };
                    out.push(Item { label: m.name.into(), kind, detail, doc: m.doc });
                }
            }
            if access == Access::Dot {
                for p in catalog::properties_of(class) {
                    out.push(Item { label: p.name.into(), kind: ItemKind::Property, detail: p.ty.clone(), doc: "" });
                }
                for (c, name, child) in catalog::SESSION_CHILDREN {
                    if c == class {
                        out.push(Item { label: (*name).into(), kind: ItemKind::Child, detail: (*child).into(), doc: "" });
                    }
                }
                if class == "DataModel" {
                    for s in catalog::services() {
                        out.push(Item { label: s.into(), kind: ItemKind::Service, detail: s.into(), doc: "" });
                    }
                }
            }
        }
        Ty::Object(o) | Ty::Library(o) => {
            let fields = if matches!(ty, Ty::Object(_)) { catalog::object_fields(o) } else { catalog::library(o) };
            for f in fields {
                let (want, kind) = match f.kind {
                    Kind::Method if matches!(ty, Ty::Object(_)) => (Access::Colon, ItemKind::Method),
                    Kind::Method => (Access::Dot, ItemKind::Function),
                    Kind::Event => (Access::Dot, ItemKind::Event),
                    _ if matches!(ty, Ty::Library(_)) => (Access::Dot, ItemKind::Constant),
                    _ => (Access::Dot, ItemKind::Field),
                };
                if want == access {
                    let detail = if f.kind == Kind::Method { signature(f.params, f.ty) } else { f.ty.to_string() };
                    out.push(Item { label: f.name.into(), kind, detail, doc: f.doc });
                }
            }
        }
        Ty::Signal(params) if access == Access::Colon => {
            let handler = format!("fn: ({params}) -> ()");
            for (name, doc) in [
                ("Connect", "Calls `fn` each time the signal fires."),
                ("Once", "Calls `fn` the next time only."),
            ] {
                out.push(Item {
                    label: name.into(),
                    kind: ItemKind::Method,
                    detail: format!("({handler}) -> RBXScriptConnection"),
                    doc,
                });
            }
            out.push(Item {
                label: "Wait".into(),
                kind: ItemKind::Method,
                detail: format!("() -> ({params})"),
                doc: "Yields until the signal fires.",
            });
        }
        Ty::Enums if access == Access::Dot => {
            for (name, _) in catalog::ENUMS {
                out.push(Item { label: (*name).into(), kind: ItemKind::EnumType, detail: format!("Enum.{name}"), doc: "" });
            }
        }
        Ty::EnumType(t) if access == Access::Dot => {
            for item in catalog::enum_items(t) {
                out.push(Item {
                    label: (*item).into(),
                    kind: ItemKind::EnumItem,
                    detail: format!("Enum.{t}.{item}"),
                    doc: "",
                });
            }
        }
        Ty::EnumItem(t) if access == Access::Dot => {
            for (name, detail) in [("Name", "string"), ("Value", "number"), ("EnumType", "string")] {
                out.push(Item {
                    label: name.into(),
                    kind: ItemKind::Field,
                    detail: detail.into(),
                    doc: if name == "EnumType" { "The enum this item belongs to." } else { "" },
                });
                let _ = t;
            }
        }
        _ => {}
    }
    out
}

/// The completions at the caret, filtered by `prefix` (case-insensitive,
/// prefix matches first).
pub fn complete(cx: &Context) -> Vec<Item> {
    let known = |n: &str| cx.locals.iter().rev().find(|(name, _)| name == n).map(|(_, t)| t.clone());
    let mut items: Vec<Item> = Vec::new();

    if let Some((callee, arg)) = cx.in_string {
        if arg == 0 {
            let method = callee.rsplit(|c| c == ':' || c == '.').next().unwrap_or(callee);
            let receiver = callee.len() > method.len() && callee.len() > method.len() + 1;
            match method {
                "GetService" | "FindService" => {
                    items.extend(catalog::services().into_iter().map(|s| Item {
                        label: s.into(),
                        kind: ItemKind::Service,
                        detail: s.into(),
                        doc: "",
                    }))
                }
                "new" if callee.ends_with("Instance.new") => items.extend(catalog::CREATABLE.iter().map(|c| Item {
                    label: (*c).into(),
                    kind: ItemKind::Class,
                    detail: (*c).into(),
                    doc: "",
                })),
                "IsA" | "FindFirstChildOfClass" | "FindFirstChildWhichIsA" | "FindFirstAncestorOfClass"
                | "FindFirstAncestorWhichIsA" => {
                    let mut classes: Vec<&str> = catalog::CREATABLE.to_vec();
                    classes.extend(["BasePart", "GuiObject", "GuiButton", "PVInstance", "LuaSourceContainer", "ValueBase"]);
                    classes.sort_unstable();
                    items.extend(classes.into_iter().map(|c| Item {
                        label: c.into(),
                        kind: ItemKind::Class,
                        detail: c.into(),
                        doc: "",
                    }));
                }
                "GetPropertyChangedSignal" if receiver => {
                    let recv = &callee[..callee.len() - method.len() - 1];
                    if let Some(Ty::Instance(class)) = resolve(recv, &known, cx.script_class) {
                        items.extend(catalog::properties_of(&class).into_iter().map(|p| Item {
                            label: p.name.into(),
                            kind: ItemKind::Property,
                            detail: p.ty,
                            doc: "",
                        }));
                    }
                }
                _ => {}
            }
        }
    } else if let (Some(receiver), Some(access)) = (cx.receiver, cx.access) {
        if let Some(ty) = resolve(receiver, &known, cx.script_class) {
            items = member_items(&ty, access);
        }
    } else {
        let mut seen = std::collections::HashSet::new();
        for (name, ty) in cx.locals.iter().rev() {
            if seen.insert(name.clone()) {
                let detail = match ty {
                    Some(Ty::Instance(c)) => c.clone(),
                    Some(Ty::Object(o)) | Some(Ty::Library(o)) => (*o).into(),
                    Some(Ty::Signal(_)) => "RBXScriptSignal".into(),
                    _ => String::new(),
                };
                items.push(Item { label: name.clone(), kind: ItemKind::Variable, detail, doc: "" });
            }
        }
        for g in catalog::GLOBALS {
            if seen.insert(g.name.to_string()) {
                let (kind, detail) = if g.kind == Kind::Method {
                    (ItemKind::Function, signature(g.params, g.ty))
                } else {
                    (ItemKind::Variable, g.ty.to_string())
                };
                items.push(Item { label: g.name.into(), kind, detail, doc: g.doc });
            }
        }
        for (name, _) in catalog::LIBRARIES {
            if seen.insert(name.to_string()) {
                items.push(Item { label: (*name).into(), kind: ItemKind::Library, detail: (*name).into(), doc: "" });
            }
        }
        for k in KEYWORDS {
            if seen.insert(k.to_string()) {
                items.push(Item { label: (*k).into(), kind: ItemKind::Keyword, detail: String::new(), doc: "" });
            }
        }
    }

    // Prefix matches first. After a receiver or in a string, alphabetical;
    // at a bare name, locals (most recent first), then globals and
    // libraries, then keywords, since the editor shows only the first few.
    let p = cx.prefix.to_ascii_lowercase();
    items.retain(|i| i.label.to_ascii_lowercase().contains(&p));
    let bare = cx.in_string.is_none() && cx.receiver.is_none();
    let rank = |i: &Item| match i.kind {
        ItemKind::Variable if cx.locals.iter().any(|(n, _)| *n == i.label) => 0,
        ItemKind::Keyword => 2,
        _ => 1,
    };
    if bare {
        items.sort_by_key(|i| (!i.label.to_ascii_lowercase().starts_with(&p), rank(i)));
    } else {
        items.sort_by_key(|i| (!i.label.to_ascii_lowercase().starts_with(&p), i.label.to_ascii_lowercase()));
    }
    items.dedup_by(|a, b| a.label == b.label && a.kind == b.kind);
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    fn at(source: &str, receiver: &str, access: Access) -> Vec<Item> {
        let locals = locals(source, Some("LocalScript"));
        complete(&Context {
            receiver: Some(receiver),
            access: Some(access),
            script_class: Some("LocalScript"),
            locals: &locals,
            ..Default::default()
        })
    }

    #[test]
    fn a_local_players_methods_after_a_colon() {
        let items = at("", "game:GetService(\"Players\").LocalPlayer", Access::Colon);
        let names = labels(&items);
        for want in ["GetMouse", "LoadCharacter", "Kick", "IsKeyDown", "FindFirstChild", "Destroy", "WaitForChild"] {
            assert!(names.contains(&want), "{want} missing: {names:?}");
        }
        for never in ["FireServer", "TakeDamage", "Raycast", "CharacterAdded", "Name", "isA"] {
            assert!(!names.contains(&never), "{never} offered: {names:?}");
        }
        let mouse = items.iter().find(|i| i.label == "GetMouse").unwrap();
        assert_eq!(mouse.detail, "() -> PlayerMouse");
    }

    #[test]
    fn properties_events_and_children_after_a_dot() {
        let names_of = |r: &str| labels(&at("", r, Access::Dot)).into_iter().map(String::from).collect::<Vec<_>>();
        let player = names_of("game.Players.LocalPlayer");
        for want in ["Character", "UserId", "CharacterAdded", "PlayerGui", "Name", "Parent"] {
            assert!(player.iter().any(|n| n == want), "{want} missing: {player:?}");
        }
        assert!(!player.iter().any(|n| n == "GetMouse"), "methods belong after a colon");

        let part = names_of("Instance.new(\"Part\")");
        for want in ["Anchored", "Position", "Touched", "Material", "CFrame"] {
            assert!(part.iter().any(|n| n == want), "{want} missing: {part:?}");
        }
        let game = names_of("game");
        assert!(game.iter().any(|n| n == "ReplicatedStorage"), "services after `game.`");
    }

    #[test]
    fn locals_carry_their_type() {
        let src = "local Players = game:GetService(\"Players\")\n\
                   local player = Players.LocalPlayer\n\
                   local mouse = player:GetMouse()\n\
                   local hum: Humanoid = nil\n";
        let names = labels(&at(src, "mouse", Access::Dot)).join(",");
        assert!(names.contains("Hit") && names.contains("Button1Down"), "{names}");
        let names = labels(&at(src, "hum", Access::Colon)).join(",");
        assert!(names.contains("TakeDamage") && names.contains("LoadAnimation"), "{names}");
        let names = labels(&at(src, "Players", Access::Colon)).join(",");
        assert!(names.contains("GetPlayers"), "{names}");
    }

    #[test]
    fn handler_and_loop_variables_carry_their_type() {
        let src = "local Players = game:GetService(\"Players\")\n\
                   Players.PlayerAdded:Connect(function(player)\n\
                   end)\n\
                   for _, p in ipairs(Players:GetPlayers()) do\n\
                   end\n\
                   workspace.Pad.Touched:Connect(function(hit)\n";
        let l = locals(src, None);
        let ty = |name: &str| l.iter().rev().find(|(n, _)| n == name).and_then(|(_, t)| t.clone());
        assert_eq!(ty("player"), Some(Ty::Instance("Player".into())));
        assert_eq!(ty("p"), Some(Ty::Instance("Player".into())));
        assert_eq!(ty("hit"), Some(Ty::Instance("BasePart".into())));
    }

    #[test]
    fn signals_enums_values_and_libraries() {
        let conn = labels(&at("", "workspace.ChildAdded", Access::Colon)).join(",");
        assert_eq!(conn, "Connect,Once,Wait");
        let keys = at("", "Enum.KeyCode", Access::Dot);
        let kc = labels(&keys);
        assert!(kc.contains(&"W") && kc.contains(&"LeftShift"));
        let v = labels(&at("", "Vector3.new(1, 2, 3)", Access::Dot)).join(",");
        assert!(v.contains("Magnitude") && !v.contains("Dot"), "{v}");
        let v = labels(&at("", "Vector3.new(1, 2, 3)", Access::Colon)).join(",");
        assert!(v.contains("Dot") && v.contains("Lerp"), "{v}");
        let m = labels(&at("", "math", Access::Dot)).join(",");
        assert!(m.contains("floor") && m.contains("pi"), "{m}");
        let t = labels(&at("", "task", Access::Dot)).join(",");
        assert!(t.contains("wait") && t.contains("spawn"), "{t}");
    }

    #[test]
    fn strings_that_name_a_service_or_class() {
        let s = complete(&Context { in_string: Some(("game:GetService", 0)), prefix: "Rep", ..Default::default() });
        assert_eq!(labels(&s)[..2], ["ReplicatedFirst", "ReplicatedStorage"]);
        let c = complete(&Context { in_string: Some(("Instance.new", 0)), prefix: "Part", ..Default::default() });
        assert_eq!(labels(&c)[0], "Part");
    }

    #[test]
    fn bare_names_offer_locals_globals_and_keywords() {
        let l = locals("local cash = 0\n", None);
        let items = complete(&Context { prefix: "ca", locals: &l, ..Default::default() });
        assert_eq!(labels(&items)[0], "cash");
        let items = complete(&Context { prefix: "wor", ..Default::default() });
        assert_eq!(labels(&items)[0], "workspace");
        let items = complete(&Context { prefix: "fun", ..Default::default() });
        assert_eq!(labels(&items)[0], "function");
        // With no prefix the locals lead, most recent first.
        let l = locals("local a = 1\nlocal zeta = 2\n", None);
        let items = complete(&Context { locals: &l, ..Default::default() });
        assert_eq!(labels(&items)[..2], ["zeta", "a"]);
        assert_eq!(items.last().map(|i| i.kind), Some(ItemKind::Keyword));
    }

    #[test]
    fn a_string_argument_is_read_whole() {
        // The lexer hands a string over in pieces; the reader joins them, an
        // escape included, and keeps where each word starts.
        let w = words_at("game:GetService(\"Players\") .. \"a\\\"b\"");
        let texts: Vec<&str> = w.iter().map(|x| x.1.as_str()).collect();
        assert_eq!(texts, ["game", ":", "GetService", "(", "\"Players\"", ")", "..", "\"a\\\"b\""]);
        assert_eq!(w[4].2, 16, "the string starts at its quote");
        assert_eq!(string_value(&w[4].1), Some("Players"));

        let known = |_: &str| None;
        assert_eq!(resolve("Instance.new(\"RemoteFunction\")", &known, None), Some(Ty::Instance("RemoteFunction".into())));
        assert_eq!(resolve("game:GetService('Players')", &known, None), Some(Ty::Instance("Players".into())));
    }

    #[test]
    fn a_list_after_a_string_argument_gives_its_element() {
        let l = locals("for _, p in ipairs(game:GetService(\"Players\"):GetPlayers()) do\nend\n", None);
        let p = l.iter().find(|(n, _)| n == "p").and_then(|(_, t)| t.clone());
        assert_eq!(p, Some(Ty::Instance("Player".into())));
    }
}
