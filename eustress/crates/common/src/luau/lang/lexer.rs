//! Luau, one line at a time.
//!
//! A line is lexed from the state the previous line ended in, and hands its
//! own end state to the next. What can span lines lives in that state: an
//! open long string or block comment with its `=` level, a quoted string
//! continued by `\` or `\z` at the end of a line, and the stack of backtick
//! strings whose `{...}` expressions are still open.
//!
//! Classes are semantic and decided from the line alone: a name after `.` is
//! a property (or a function when it is called), after `:` a method, in a
//! type annotation a type, in a parameter list a parameter.

use super::{LineState, Token, TokenClass};

/// Globals the Play VM's environment defines, and the standard library.
pub const BUILTINS: &[&str] = &[
    "game", "Game", "workspace", "Workspace", "script", "plugin", "shared", "_G", "self",
    "Enum", "Instance", "Vector3", "Vector2", "CFrame", "Color3", "UDim", "UDim2", "BrickColor", "Ray",
    "RaycastParams", "Region3", "TweenInfo", "NumberRange", "NumberSequence", "NumberSequenceKeypoint",
    "ColorSequence", "ColorSequenceKeypoint", "Random", "Rect", "PhysicalProperties", "OverlapParams",
    "task", "math", "string", "table", "coroutine", "os", "debug", "utf8", "bit32", "buffer", "vector",
    "print", "warn", "error", "assert", "pcall", "xpcall", "type", "typeof", "tostring", "tonumber",
    "pairs", "ipairs", "next", "select", "unpack", "require", "setmetatable", "getmetatable", "rawget",
    "rawset", "rawequal", "rawlen", "newproxy", "gcinfo", "tick", "time", "elapsedTime", "wait", "delay",
    "spawn", "settings",
];

const CONTROL_FLOW: &[&str] = &[
    "if", "then", "else", "elseif", "for", "while", "repeat", "until", "do", "return", "break",
];
const KEYWORDS: &[&str] = &["local", "function", "end", "in", "and", "or", "not"];

// ── Line state ───────────────────────────────────────────────────────────────
//
// bits 0..4   mode (below)
// bits 4..12  the long bracket's `=` level
// bits 12..15 how many interpolation expressions are open (0..=7)
// bits 16..   6 bits each: the brace depth inside each open expression

const CODE: u64 = 0;
const LONG_STRING: u64 = 1;
const LONG_COMMENT: u64 = 2;
const QUOTED: u64 = 3; // continued by `\` or `\z`; the quote is in bits 4..12
const BACKTICK: u64 = 4; // backtick text, continued by `\` or `\z`

const MAX_FRAMES: usize = 7;

#[derive(Clone, Debug, PartialEq, Eq)]
struct State {
    mode: u64,
    /// The long bracket level, or the quote byte for [`QUOTED`].
    level: u8,
    /// Brace depth inside each open interpolation expression, outermost first.
    frames: Vec<u8>,
}

impl State {
    fn decode(s: LineState) -> Self {
        let n = ((s >> 12) & 0x7) as usize;
        let frames = (0..n).map(|i| ((s >> (16 + 6 * i)) & 0x3f) as u8).collect();
        Self { mode: s & 0xf, level: ((s >> 4) & 0xff) as u8, frames }
    }

    fn encode(&self) -> LineState {
        let n = self.frames.len().min(MAX_FRAMES);
        let mut s = self.mode | (self.level as u64) << 4 | (n as u64) << 12;
        for (i, d) in self.frames.iter().take(n).enumerate() {
            s |= ((*d).min(0x3f) as u64) << (16 + 6 * i);
        }
        s
    }
}

pub fn initial_state() -> LineState {
    0
}

// ── Raw tokens ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Raw {
    Name,
    Number,
    Str,
    Escape,
    Interp,
    Comment,
    DocComment,
    Op,
    Punct,
    Attribute,
    Invalid,
}

struct Lexed<'a> {
    line: &'a str,
    raws: Vec<(usize, usize, Raw)>,
}

impl<'a> Lexed<'a> {
    fn text(&self, i: usize) -> &'a str {
        let (s, e, _) = self.raws[i];
        &self.line[s..e]
    }
}

/// The long bracket at `i` (`[`, some `=`, `[`): its level and length.
fn long_bracket(b: &[u8], i: usize) -> Option<(u8, usize)> {
    if b.get(i) != Some(&b'[') {
        return None;
    }
    let mut j = i + 1;
    while b.get(j) == Some(&b'=') {
        j += 1;
    }
    (b.get(j) == Some(&b'[')).then(|| ((j - i - 1).min(255) as u8, j + 1 - i))
}

/// Where the long bracket of `level` closes at or after `i`: the index just
/// past its `]=*]`.
fn long_close(b: &[u8], mut i: usize, level: u8) -> Option<usize> {
    while i < b.len() {
        if b[i] == b']' {
            let mut j = i + 1;
            while b.get(j) == Some(&b'=') {
                j += 1;
            }
            if b.get(j) == Some(&b']') && j - i - 1 == level as usize {
                return Some(j + 1);
            }
        }
        i += 1;
    }
    None
}

/// An escape starting at the backslash at `i`: its length, or `None` when
/// the backslash ends the line (the string continues on the next).
fn escape_len(b: &[u8], i: usize) -> Option<usize> {
    let c = *b.get(i + 1)?;
    Some(match c {
        b'x' => 2 + b[i + 2..].iter().take(2).take_while(|c| c.is_ascii_hexdigit()).count(),
        b'u' if b.get(i + 2) == Some(&b'{') => {
            let body = b[i + 3..].iter().take_while(|c| c.is_ascii_hexdigit()).count();
            3 + body + usize::from(b.get(i + 3 + body) == Some(&b'}'))
        }
        b'0'..=b'9' => 1 + b[i + 1..].iter().take(3).take_while(|c| c.is_ascii_digit()).count(),
        _ => 2,
    })
}

const OPS: &[&str] = &[
    "...", "..=", "//=", "->", "::", "==", "~=", "<=", ">=", "+=", "-=", "*=", "/=", "%=", "^=", "//", "..", "+", "-",
    "*", "/", "%", "^", "#", "<", ">", "=", "?", "|", "&",
];

fn lex<'a>(line: &'a str, state: &mut State) -> Lexed<'a> {
    let b = line.as_bytes();
    let mut raws: Vec<(usize, usize, Raw)> = Vec::new();
    let mut i = 0;
    let push = |raws: &mut Vec<(usize, usize, Raw)>, s: usize, e: usize, k: Raw| {
        if e > s {
            raws.push((s, e, k));
        }
    };

    while i < b.len() {
        match state.mode {
            LONG_STRING | LONG_COMMENT => {
                let kind = if state.mode == LONG_STRING { Raw::Str } else { Raw::Comment };
                match long_close(b, i, state.level) {
                    Some(end) => {
                        push(&mut raws, i, end, kind);
                        i = end;
                        state.mode = CODE;
                        state.level = 0;
                    }
                    None => {
                        push(&mut raws, i, b.len(), kind);
                        i = b.len();
                    }
                }
            }
            QUOTED | BACKTICK => {
                let quote = if state.mode == QUOTED { state.level } else { b'`' };
                // Where the current run of plain string text began.
                let mut seg = i;
                // A `\z` with only whitespace after it so far: it skips the
                // line break too, so the string goes on.
                let mut pending_z = false;
                let mut left_string = false;
                while i < b.len() {
                    let c = b[i];
                    if c == b'\\' {
                        push(&mut raws, seg, i, Raw::Str);
                        match escape_len(b, i) {
                            Some(n) => {
                                let n = n.min(b.len() - i);
                                pending_z = &b[i..i + n] == b"\\z";
                                push(&mut raws, i, i + n, Raw::Escape);
                                i += n;
                                seg = i;
                                continue;
                            }
                            None => {
                                // `\` at the end of the line: the string goes on.
                                push(&mut raws, i, i + 1, Raw::Escape);
                                return Lexed { line, raws };
                            }
                        }
                    }
                    if !c.is_ascii_whitespace() {
                        pending_z = false;
                    }
                    if c == quote {
                        push(&mut raws, seg, i + 1, Raw::Str);
                        i += 1;
                        left_string = true;
                        break;
                    }
                    if quote == b'`' && c == b'{' {
                        push(&mut raws, seg, i, Raw::Str);
                        push(&mut raws, i, i + 1, Raw::Interp);
                        i += 1;
                        state.frames.push(0);
                        left_string = true;
                        break;
                    }
                    i += 1;
                }
                if left_string {
                    state.mode = CODE;
                    state.level = 0;
                } else {
                    push(&mut raws, seg, b.len(), Raw::Str);
                    i = b.len();
                    if !pending_z {
                        // Unterminated, with nothing to carry it on: the
                        // parser reports it, and the next line starts in code.
                        state.mode = CODE;
                        state.level = 0;
                    }
                }
            }
            _ => {
                let c = b[i];
                if c.is_ascii_whitespace() {
                    i += 1;
                    continue;
                }
                // Comments.
                if line[i..].starts_with("--") {
                    if let Some((level, n)) = long_bracket(b, i + 2) {
                        state.mode = LONG_COMMENT;
                        state.level = level;
                        match long_close(b, i + 2 + n, level) {
                            Some(end) => {
                                push(&mut raws, i, end, Raw::Comment);
                                i = end;
                                state.mode = CODE;
                                state.level = 0;
                            }
                            None => {
                                push(&mut raws, i, b.len(), Raw::Comment);
                                i = b.len();
                            }
                        }
                        continue;
                    }
                    let doc = line[i..].starts_with("---") && !line[i..].starts_with("----");
                    push(&mut raws, i, b.len(), if doc { Raw::DocComment } else { Raw::Comment });
                    i = b.len();
                    continue;
                }
                // Long strings.
                if let Some((level, n)) = long_bracket(b, i) {
                    match long_close(b, i + n, level) {
                        Some(end) => {
                            push(&mut raws, i, end, Raw::Str);
                            i = end;
                        }
                        None => {
                            push(&mut raws, i, b.len(), Raw::Str);
                            state.mode = LONG_STRING;
                            state.level = level;
                            i = b.len();
                        }
                    }
                    continue;
                }
                // Quoted and backtick strings: open, then the string loop runs.
                if c == b'"' || c == b'\'' || c == b'`' {
                    push(&mut raws, i, i + 1, Raw::Str);
                    i += 1;
                    if c == b'`' {
                        state.mode = BACKTICK;
                    } else {
                        state.mode = QUOTED;
                        state.level = c;
                    }
                    continue;
                }
                // Closing an interpolation expression.
                if c == b'}' {
                    if let Some(depth) = state.frames.last_mut() {
                        if *depth == 0 {
                            state.frames.pop();
                            push(&mut raws, i, i + 1, Raw::Interp);
                            i += 1;
                            state.mode = BACKTICK;
                            continue;
                        }
                        *depth -= 1;
                    }
                    push(&mut raws, i, i + 1, Raw::Punct);
                    i += 1;
                    continue;
                }
                if c == b'{' {
                    if let Some(depth) = state.frames.last_mut() {
                        *depth = depth.saturating_add(1);
                    }
                    push(&mut raws, i, i + 1, Raw::Punct);
                    i += 1;
                    continue;
                }
                // Numbers.
                if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
                    let start = i;
                    if c == b'0' && matches!(b.get(i + 1), Some(b'x' | b'X' | b'b' | b'B')) {
                        i += 2;
                        while i < b.len() && (b[i].is_ascii_hexdigit() || b[i] == b'_') {
                            i += 1;
                        }
                    } else {
                        while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'_') {
                            i += 1;
                        }
                        if b.get(i) == Some(&b'.') && b.get(i + 1) != Some(&b'.') {
                            i += 1;
                            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'_') {
                                i += 1;
                            }
                        }
                        if matches!(b.get(i), Some(b'e' | b'E')) {
                            let mut j = i + 1;
                            if matches!(b.get(j), Some(b'+' | b'-')) {
                                j += 1;
                            }
                            if b.get(j).is_some_and(u8::is_ascii_digit) {
                                i = j;
                                while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'_') {
                                    i += 1;
                                }
                            }
                        }
                    }
                    push(&mut raws, start, i, Raw::Number);
                    continue;
                }
                // Names.
                if c.is_ascii_alphabetic() || c == b'_' {
                    let start = i;
                    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                        i += 1;
                    }
                    push(&mut raws, start, i, Raw::Name);
                    continue;
                }
                // Attributes: `@native`.
                if c == b'@' {
                    let start = i;
                    i += 1;
                    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                        i += 1;
                    }
                    push(&mut raws, start, i, Raw::Attribute);
                    continue;
                }
                if let Some(op) = OPS.iter().find(|op| line[i..].starts_with(**op)) {
                    push(&mut raws, i, i + op.len(), Raw::Op);
                    i += op.len();
                    continue;
                }
                if matches!(c, b'(' | b')' | b'[' | b']' | b';' | b',' | b'.' | b':') {
                    push(&mut raws, i, i + 1, Raw::Punct);
                    i += 1;
                    continue;
                }
                // Anything else, a whole character at a time.
                let n = line[i..].chars().next().map_or(1, char::len_utf8);
                push(&mut raws, i, i + n, Raw::Invalid);
                i += n;
            }
        }
    }
    Lexed { line, raws }
}

// ── Classes ──────────────────────────────────────────────────────────────────

/// Whether a call follows: `(`, a string, or a table constructor.
fn called(lx: &Lexed, next: Option<usize>) -> bool {
    next.is_some_and(|n| {
        let t = lx.text(n);
        matches!(lx.raws[n].2, Raw::Str) || t == "(" || t == "{"
    })
}

/// A type annotation's reach: from `:`, `->`, `::` or a type statement's `=`
/// to the first token that ends it outside its own brackets.
struct TypeCtx {
    depth: i32,
}

pub fn tokenize_line(line: &str, state: LineState) -> (Vec<Token>, LineState) {
    let mut st = State::decode(state);
    let lx = lex(line, &mut st);
    let n = lx.raws.len();
    let mut out: Vec<Token> = Vec::with_capacity(n);

    let mut ty: Option<TypeCtx> = None;
    // Inside a function's parameter list: its paren depth.
    let mut params: Option<i32> = None;
    // After `function`, until its `(`.
    let mut awaiting_params = false;
    // After `local` (not `local function`), until `=` or `in`.
    let mut declaring = false;
    // Just past the `)` that closed a parameter list.
    let mut closed_params = false;
    // Inside `<...>` right after a function or type name.
    let mut generics: Option<i32> = None;
    // Inside an `Enum.X.Y` path.
    let mut enum_path = false;

    let sig = |k: Raw| !matches!(k, Raw::Comment | Raw::DocComment);
    let next_sig = |i: usize| (i + 1..n).find(|&j| sig(lx.raws[j].2));
    let mut prev: Option<usize> = None;

    for i in 0..n {
        let (s, e, raw) = lx.raws[i];
        let text = &line[s..e];
        let prev_text = prev.map(|p| lx.text(p));
        let class = match raw {
            Raw::Number => TokenClass::Number,
            Raw::Str => TokenClass::String,
            Raw::Escape => TokenClass::StringEscape,
            Raw::Interp => TokenClass::Interpolation,
            Raw::Comment => TokenClass::Comment,
            Raw::DocComment => TokenClass::DocComment,
            Raw::Attribute => TokenClass::Attribute,
            Raw::Invalid => TokenClass::Invalid,
            Raw::Op | Raw::Punct => {
                let mut cls = if raw == Raw::Op { TokenClass::Operator } else { TokenClass::Punctuation };
                // Type annotations open and close around punctuation.
                if let Some(t) = ty.as_mut() {
                    match text {
                        "(" | "{" | "[" | "<" => t.depth += 1,
                        ")" | "}" | "]" | ">" if t.depth > 0 => t.depth -= 1,
                        "?" | "|" | "&" | "->" | "..." | ":" | "." => {}
                        "," if t.depth > 0 => {}
                        _ => ty = None,
                    }
                }
                if let Some(g) = generics.as_mut() {
                    match text {
                        "<" => *g += 1,
                        ">" => {
                            *g -= 1;
                            if *g == 0 {
                                generics = None;
                            }
                        }
                        _ => {}
                    }
                }
                match text {
                    "(" if awaiting_params => {
                        awaiting_params = false;
                        params = Some(0);
                        ty = None;
                    }
                    "(" => {
                        if let Some(p) = params.as_mut() {
                            *p += 1;
                        }
                    }
                    ")" => match params {
                        Some(0) => {
                            params = None;
                            ty = None;
                            closed_params = true;
                            prev = Some(i);
                            out.push(Token { start: s as u32, len: text.len() as u32, class: cls });
                            continue;
                        }
                        Some(p) => params = Some(p - 1),
                        None => {}
                    },
                    "," if params == Some(0) && ty.as_ref().map_or(true, |t| t.depth == 0) => ty = None,
                    // Generic parameters: after a type statement's name, or a
                    // function's name (`function M.new<T>(`).
                    "<" if prev.is_some_and(|p| {
                        let k = out.last().map(|t| t.class);
                        lx.raws[p].2 == Raw::Name
                            && (awaiting_params || matches!(k, Some(TokenClass::Function | TokenClass::Type)))
                    }) && ty.is_none() =>
                    {
                        generics = Some(1);
                        cls = TokenClass::Punctuation;
                    }
                    ":" if ty.is_none() => {
                        // An annotation after a declared name, a parameter or a
                        // parameter list; otherwise a method access.
                        let annotates = closed_params
                            || (params == Some(0))
                            || (declaring && prev.is_some_and(|p| lx.raws[p].2 == Raw::Name));
                        if annotates {
                            ty = Some(TypeCtx { depth: 0 });
                        }
                    }
                    "->" | "::" if ty.is_none() => ty = Some(TypeCtx { depth: 0 }),
                    "=" => {
                        declaring = false;
                        // `type Name<T> = ...`: the rest is a type.
                        let stmt_type = (0..i).any(|j| lx.raws[j].2 == Raw::Name && lx.text(j) == "type" && is_type_stmt(&lx, j));
                        if stmt_type && ty.is_none() {
                            ty = Some(TypeCtx { depth: 0 });
                        }
                    }
                    _ => {}
                }
                closed_params = false;
                cls
            }
            Raw::Name => {
                let next = next_sig(i);
                let next_text = next.map(|j| lx.text(j));
                let after_dot = prev_text == Some(".");
                let after_colon = prev_text == Some(":") && ty.is_none();
                closed_params = false;
                if ty.is_some() {
                    if matches!(text, "typeof") {
                        TokenClass::Builtin
                    } else if matches!(text, "nil" | "true" | "false") {
                        TokenClass::Constant
                    } else if next_text == Some(":") && ty.as_ref().is_some_and(|t| t.depth > 0) {
                        TokenClass::Property
                    } else {
                        TokenClass::Type
                    }
                } else if generics.is_some() {
                    TokenClass::Type
                } else if after_dot {
                    if enum_path {
                        TokenClass::Constant
                    } else if called(&lx, next) {
                        TokenClass::Function
                    } else {
                        TokenClass::Property
                    }
                } else if after_colon {
                    TokenClass::Method
                } else if matches!(text, "true" | "false" | "nil") {
                    TokenClass::Constant
                } else if CONTROL_FLOW.contains(&text) {
                    declaring = false;
                    TokenClass::ControlFlow
                } else if text == "continue" && !continues_as_name(next_text) {
                    TokenClass::ControlFlow
                } else if (text == "type" && is_type_stmt(&lx, i)) || (text == "export" && next_text == Some("type")) {
                    TokenClass::Keyword
                } else if KEYWORDS.contains(&text) {
                    match text {
                        "function" => {
                            awaiting_params = true;
                            declaring = false;
                        }
                        "local" => declaring = next_text != Some("function"),
                        "in" => declaring = false,
                        _ => {}
                    }
                    TokenClass::Keyword
                } else if prev_text == Some("type") && prev.is_some_and(|p| is_type_stmt(&lx, p)) {
                    TokenClass::Type
                } else if params.is_some() {
                    TokenClass::Parameter
                } else if awaiting_params {
                    // The function's name, or the table it is stored in.
                    if matches!(next_text, Some("." | ":")) {
                        TokenClass::Variable
                    } else {
                        TokenClass::Function
                    }
                } else if BUILTINS.contains(&text) {
                    TokenClass::Builtin
                } else if declaring {
                    TokenClass::Variable
                } else if called(&lx, next) {
                    TokenClass::Function
                } else {
                    TokenClass::Variable
                }
            }
        };
        // `Enum`, then `.Name` after `.Name`: anything else ends the path.
        if sig(raw) {
            enum_path = match raw {
                Raw::Name if prev_text == Some(".") => enum_path,
                Raw::Name => text == "Enum" && class == TokenClass::Builtin,
                Raw::Punct => enum_path && text == ".",
                _ => false,
            };
        }
        if sig(raw) {
            prev = Some(i);
        }
        out.push(Token { start: s as u32, len: (e - s) as u32, class });
    }
    (out, st.encode())
}

/// `type` starting a type statement: `type Name` or `export type Name`, at
/// the start of the statement.
fn is_type_stmt(lx: &Lexed, i: usize) -> bool {
    let next_is_name = lx.raws.get(i + 1).is_some_and(|r| r.2 == Raw::Name);
    let at_start = i == 0 || (i == 1 && lx.text(0) == "export") || lx.text(i - 1) == ";";
    next_is_name && at_start
}

/// `continue` used as a name (`continue = 1`, `continue()`), which Luau allows.
fn continues_as_name(next: Option<&str>) -> bool {
    matches!(next, Some("(" | "." | ":" | "[" | "=" | "," | "{"))
}

// ── Editing conventions ─────────────────────────────────────────────────────

const OPENERS: &[&str] = &["then", "do", "repeat", "function", "else", "{", "(", "["];
const CLOSERS: &[&str] = &["end", "until", "elseif", "}", ")", "]"];

fn code_words(line: &str) -> Vec<String> {
    let (tokens, _) = tokenize_line(line, initial_state());
    tokens
        .iter()
        .filter(|t| {
            !matches!(
                t.class,
                TokenClass::Comment | TokenClass::DocComment | TokenClass::String | TokenClass::StringEscape
            )
        })
        .filter(|t| {
            matches!(t.class, TokenClass::Keyword | TokenClass::ControlFlow | TokenClass::Punctuation)
        })
        .map(|t| line[t.start as usize..(t.start + t.len) as usize].to_string())
        .collect()
}

/// The line opens a block the next line sits inside.
pub fn indent_after(line: &str) -> bool {
    let words = code_words(line);
    let lead = words.iter().take_while(|w| CLOSERS.contains(&w.as_str()) || w.as_str() == "else").count();
    if words.first().is_some_and(|w| w == "else") {
        return true;
    }
    let net: i32 = words[lead..]
        .iter()
        .map(|w| {
            if OPENERS.contains(&w.as_str()) {
                1
            } else if CLOSERS.contains(&w.as_str()) {
                -1
            } else {
                0
            }
        })
        .sum();
    net > 0
}

/// The line closes a block, so it sits one level out.
pub fn dedent_line(line: &str) -> bool {
    code_words(line).first().is_some_and(|w| matches!(w.as_str(), "end" | "else" | "elseif" | "until" | "}" | ")" | "]"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes(line: &str) -> Vec<(String, TokenClass)> {
        let (tokens, _) = tokenize_line(line, initial_state());
        tokens.iter().map(|t| (line[t.start as usize..(t.start + t.len) as usize].to_string(), t.class)).collect()
    }

    fn class_of(line: &str, word: &str) -> TokenClass {
        classes(line)
            .into_iter()
            .find(|(t, _)| t == word)
            .unwrap_or_else(|| panic!("{word} not in {line}"))
            .1
    }

    #[test]
    fn names_take_their_role_from_where_they_stand() {
        let line = "local me = game:GetService(\"Players\").LocalPlayer";
        assert_eq!(class_of(line, "local"), TokenClass::Keyword);
        assert_eq!(class_of(line, "me"), TokenClass::Variable);
        assert_eq!(class_of(line, "game"), TokenClass::Builtin);
        assert_eq!(class_of(line, "GetService"), TokenClass::Method);
        assert_eq!(class_of(line, "LocalPlayer"), TokenClass::Property);
        assert_eq!(class_of("local n = math.floor(x)", "floor"), TokenClass::Function);
        assert_eq!(class_of("if x then return end", "then"), TokenClass::ControlFlow);
        assert_eq!(class_of("x = nil", "nil"), TokenClass::Constant);
        assert_eq!(class_of("local k = Enum.KeyCode.W", "KeyCode"), TokenClass::Constant);
        assert_eq!(class_of("local k = Enum.KeyCode.W", "W"), TokenClass::Constant);
        assert_eq!(class_of("@native function f() end", "@native"), TokenClass::Attribute);
    }

    #[test]
    fn functions_parameters_and_types() {
        let line = "local function damage(h: Humanoid, amount: number?): boolean";
        assert_eq!(class_of(line, "damage"), TokenClass::Function);
        assert_eq!(class_of(line, "h"), TokenClass::Parameter);
        assert_eq!(class_of(line, "Humanoid"), TokenClass::Type);
        assert_eq!(class_of(line, "amount"), TokenClass::Parameter);
        assert_eq!(class_of(line, "number"), TokenClass::Type);
        assert_eq!(class_of(line, "boolean"), TokenClass::Type);

        let line = "local hits: {BasePart} = {}";
        assert_eq!(class_of(line, "hits"), TokenClass::Variable);
        assert_eq!(class_of(line, "BasePart"), TokenClass::Type);

        let line = "function Tycoon:Buy(item)";
        assert_eq!(class_of(line, "Tycoon"), TokenClass::Variable);
        assert_eq!(class_of(line, "Buy"), TokenClass::Method);
        assert_eq!(class_of(line, "item"), TokenClass::Parameter);

        let line = "export type Pad<T> = { price: number, owner: T? }";
        assert_eq!(class_of(line, "Pad"), TokenClass::Type);
        assert_eq!(class_of(line, "price"), TokenClass::Property);
        assert_eq!(class_of(line, "owner"), TokenClass::Property);
        assert_eq!(class_of(line, "type"), TokenClass::Keyword);

        let line = "function M.new<T>(x: T): T";
        assert_eq!(class_of(line, "new"), TokenClass::Property);
        assert!(classes(line).iter().filter(|(t, _)| t == "T").all(|(_, k)| *k == TokenClass::Type), "{:?}", classes(line));
        assert_eq!(class_of(line, "x"), TokenClass::Parameter);

        assert_eq!(class_of("local t = type(x)", "type"), TokenClass::Builtin);
        assert_eq!(class_of("local n = (x :: number) + 1", "number"), TokenClass::Type);
    }

    #[test]
    fn continue_is_a_statement_unless_used_as_a_name() {
        assert_eq!(class_of("if skip then continue end", "continue"), TokenClass::ControlFlow);
        assert_eq!(class_of("local continue = 1", "continue"), TokenClass::Variable);
    }

    #[test]
    fn compound_assignment_and_floor_division_are_one_operator() {
        let c = classes("cash += 5 // 2");
        assert!(c.contains(&("+=".into(), TokenClass::Operator)));
        assert!(c.contains(&("//".into(), TokenClass::Operator)));
        let c = classes("s ..= \"!\"");
        assert!(c.contains(&("..=".into(), TokenClass::Operator)));
    }

    #[test]
    fn strings_escapes_and_interpolation() {
        let c = classes(r#"print("a\n\x41\u{263A}b")"#);
        let escapes: Vec<_> = c.iter().filter(|(_, k)| *k == TokenClass::StringEscape).map(|(t, _)| t.as_str()).collect();
        assert_eq!(escapes, [r"\n", r"\x41", r"\u{263A}"]);

        let line = "print(`Cash: {fmt(cash, `{unit}`)}!`)";
        let c = classes(line);
        let interps = c.iter().filter(|(_, k)| *k == TokenClass::Interpolation).count();
        assert_eq!(interps, 4, "two expressions, each opened and closed: {c:?}");
        assert_eq!(class_of(line, "fmt"), TokenClass::Function);
        assert_eq!(class_of(line, "unit"), TokenClass::Variable);
        let (_, end) = tokenize_line(line, initial_state());
        assert_eq!(end, initial_state(), "everything closed on the line");

        // A table inside an interpolation does not close it.
        let line = "print(`{#({1, 2})} items`)";
        let (_, end) = tokenize_line(line, initial_state());
        assert_eq!(end, initial_state());
        assert!(classes(line).contains(&(" items`".into(), TokenClass::String)));
    }

    #[test]
    fn what_spans_lines_carries_in_the_state() {
        // A long string.
        let (_, s) = tokenize_line("local s = [==[ first", initial_state());
        assert_ne!(s, initial_state());
        let (t, s2) = tokenize_line("still ]] inside ]==] after", s);
        assert_eq!(t[0].class, TokenClass::String);
        assert_eq!(s2, initial_state());
        assert_eq!(t.last().map(|t| t.class), Some(TokenClass::Variable), "code resumes after the close");

        // A block comment.
        let (_, s) = tokenize_line("x = 1 --[[ a", initial_state());
        let (t, s2) = tokenize_line("b ]] y = 2", s);
        assert_eq!(t[0].class, TokenClass::Comment);
        assert_eq!(s2, initial_state());

        // An interpolation whose expression spans lines.
        let (_, s) = tokenize_line("print(`a {f(", initial_state());
        assert_ne!(s, initial_state());
        let (t, s2) = tokenize_line("  x)} b`)", s);
        assert!(t.iter().any(|t| t.class == TokenClass::Interpolation));
        assert_eq!(s2, initial_state());

        // A quoted string carried on by `\z`.
        let (_, s) = tokenize_line(r#"local s = "a \z"#, initial_state());
        assert_ne!(s, initial_state());
        let (t, s2) = tokenize_line(r#"   b" .. c"#, s);
        assert_eq!(t[0].class, TokenClass::String);
        assert_eq!(s2, initial_state());
    }

    #[test]
    fn states_round_trip() {
        for st in [
            State { mode: CODE, level: 0, frames: vec![] },
            State { mode: LONG_STRING, level: 3, frames: vec![] },
            State { mode: QUOTED, level: b'"', frames: vec![0, 2] },
            State { mode: BACKTICK, level: 0, frames: vec![1, 0, 5] },
        ] {
            assert_eq!(State::decode(st.encode()), st);
        }
    }

    #[test]
    fn comments_and_doc_comments() {
        assert_eq!(classes("--- Buys a pad")[0].1, TokenClass::DocComment);
        assert_eq!(classes("-- note")[0].1, TokenClass::Comment);
        assert_eq!(classes("x = 1 -- note").last().unwrap().1, TokenClass::Comment);
    }

    /// The editor's contract: byte offsets within the line, in order, never
    /// overlapping, each on a character boundary.
    #[test]
    fn tokens_are_ordered_and_never_overlap() {
        let lines = [
            "print(\"héllo → wörld\", `ünï {cödé} ✓`) -- ç'est fini",
            "local s = [==[ ∑ ]==] .. 'a\\u{1F600}b' ¤ x",
            "local t = { a = `{ {1, 2} }`, [\"k\"] = 0x1F }",
            "@native function f<T>(x: T?, ...: number): (T, ...number) return x end",
        ];
        for line in lines {
            let (tokens, _) = tokenize_line(line, initial_state());
            let mut at = 0u32;
            for t in &tokens {
                assert!(t.start >= at, "overlap at {t:?} in {line}");
                assert!(t.len > 0, "empty token {t:?} in {line}");
                let (s, e) = (t.start as usize, (t.start + t.len) as usize);
                assert!(line.is_char_boundary(s) && line.is_char_boundary(e), "{t:?} splits a character in {line}");
                at = t.start + t.len;
            }
            assert!(at as usize <= line.len());
        }
    }

    #[test]
    fn numbers() {
        for n in ["42", "3.14", ".5", "1e10", "2.5E-3", "0xFF", "0b1010", "1_000_000"] {
            assert_eq!(classes(n), vec![(n.to_string(), TokenClass::Number)], "{n}");
        }
    }

    #[test]
    fn indentation() {
        for line in ["if x then", "for i = 1, 3 do", "while x do", "function f()", "local f = function(a)", "repeat",
            "local t = {", "else", "elseif y then", "button.Activated:Connect(function()"]
        {
            assert!(indent_after(line), "{line}");
        }
        for line in ["if a then b() end", "end", "end)", "print(1)", "local s = \"then do {\"", "-- if x then"] {
            assert!(!indent_after(line), "{line}");
        }
        for line in ["end", "  end)", "else", "elseif x then", "until done", "}"] {
            assert!(dedent_line(line), "{line}");
        }
        for line in ["endpoint = 1", "print(1)", "local e = 1"] {
            assert!(!dedent_line(line), "{line}");
        }
    }
}
