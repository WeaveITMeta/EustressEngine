// =============================================================================
// Eustress Web - Tags
// =============================================================================
// A Space has no genre. Its author describes it with tags (#zombies #tycoon #coop), and when the author gives none the
// review (the Jev chat, Claude as judge) chooses them. The Worker keeps the rule in one place:
// infrastructure/cloudflare/api/src/tags.mjs. This file mirrors it for the pages that need it: the Gallery's address, the
// tag editor in a Space's settings, and the chips on a card.
//
//   at most 8 tags; each 2 to 24 characters of a to z and 0 to 9, with single hyphens between parts
//   (^[a-z0-9]+(?:-[a-z0-9]+)*$); Unicode NFKC, trimmed, lower-cased, one leading # dropped.
//
// NFKC without a normalization library. This crate carries no Unicode tables, so `fold` lists every character whose NFKC
// form is made only of letters, digits, a hyphen, a # or a space (full-width letters, circled and mathematical letters,
// super and subscript digits, ligatures, the Kelvin sign, the odd spaces...). A character not listed either stays itself
// under NFKC or turns into something a tag may not hold, and either way it is refused, as the Worker refuses it. The
// tables below come from Node 25 (Unicode 16.0): `for each code point, String.fromCodePoint(c).normalize('NFKC')`. They were
// checked against tags.mjs's normalizeTag over every code point, alone and between letters. The Worker has the last word
// on every request, so this file only has to agree with it, never to be stricter.
//
// Table of Contents:
// 1. The rule
// 2. One tag
// 3. A list of tags, and the editor's moves
// 4. The Gallery's address
// 5. Who sees what
// 6. The tables
// =============================================================================

// -----------------------------------------------------------------------------
// 1. The rule
// -----------------------------------------------------------------------------

/// The most tags a Space holds.
pub const MAX_TAGS: usize = 8;
/// The shortest and the longest tag, in characters.
pub const TAG_MIN: usize = 2;
pub const TAG_MAX: usize = 24;

/// A request that names more entries than this is refused without reading them (tags.mjs MAX_ENTRIES_READ).
const MAX_ENTRIES_READ: usize = 256;

/// The most of a refused tag a message repeats.
const ECHO_MAX: usize = 40;

/// What a refusal ends with.
pub const TAG_RULES: &str = "A tag is 2 to 24 characters: lowercase letters a to z and digits, with single hyphens between them, and an optional leading #. A Space holds at most 8 tags.";

/// The sentence under the tag editor in a Space's settings.
pub const TAGS_HELP: &str = "Leave them empty and the review chooses tags for you. Changing tags sends your Space back to review.";

/// The label on tags the review chose, where the author sees their own Space.
pub const SUGGESTED_BY_REVIEW: &str = "Suggested by review";

// -----------------------------------------------------------------------------
// 2. One tag
// -----------------------------------------------------------------------------

/// The characters the Worker's JavaScript `trim()` removes: white space and line terminators, and the byte order mark.
/// (Not Rust's `is_whitespace`, which also takes U+0085.)
fn is_trimmed(c: char) -> bool {
    matches!(c, '\u{9}'..='\u{d}' | '\u{20}' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

/// Unicode NFKC for the characters that matter to a tag (see the header).
fn fold(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        if c.is_ascii() {
            out.push(c);
            continue;
        }
        let cp = c as u32;
        let run = FOLD_RUNS.binary_search_by(|&(start, end, _)| {
            if end < cp {
                std::cmp::Ordering::Less
            } else if start > cp {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        if let Ok(i) = run {
            let (start, _, first) = FOLD_RUNS[i];
            out.push((first + (cp - start) as u8) as char);
        } else if let Ok(i) = FOLD_MULTI.binary_search_by_key(&cp, |&(from, _)| from) {
            out.push_str(FOLD_MULTI[i].1);
        } else {
            out.push(c);
        }
    }
    out
}

/// One tag as a Space stores it: NFKC, trimmed, lower-cased, one leading `#` removed. `None` when what is left is not a
/// valid tag (tags.mjs normalizeTag).
pub fn normalize_tag(raw: &str) -> Option<String> {
    let folded = fold(raw);
    let lower = folded.trim_matches(is_trimmed).to_lowercase();
    let tag = lower.strip_prefix('#').unwrap_or(&lower);
    let bytes = tag.as_bytes();
    let count = bytes.len();
    if !(TAG_MIN..=TAG_MAX).contains(&count) {
        return None;
    }
    // ^[a-z0-9]+(?:-[a-z0-9]+)*$ : letters and digits, a hyphen only between two of them.
    let word = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    let shaped = bytes.iter().all(|b| word(b) || *b == b'-') && word(&bytes[0]) && word(&bytes[count - 1]) && !tag.contains("--");
    shaped.then(|| tag.to_string())
}

/// At most `ECHO_MAX` characters of `raw`, with anything that could hide or reorder text shown as a `\uXXXX` escape, and
/// an ellipsis when there was more (tags.mjs echo).
fn echo(raw: &str) -> String {
    let unsafe_char = |c: char| {
        let code = c as u32;
        matches!(c, '"' | '\'' | '<' | '>' | '&' | '\\' | '`')
            || code <= 0x1f
            || (0x7f..=0x9f).contains(&code)
            || (0x200b..=0x200f).contains(&code)
            || (0x2028..=0x202e).contains(&code)
            || (0x2060..=0x2069).contains(&code)
            || code == 0xfeff
    };
    let mut shown = String::new();
    for (i, c) in raw.chars().enumerate() {
        if i == ECHO_MAX {
            shown.push('\u{2026}');
            break;
        }
        if unsafe_char(c) {
            shown.push_str(&format!("\\u{:04x}", c as u32));
        } else {
            shown.push(c);
        }
    }
    shown
}

// -----------------------------------------------------------------------------
// 3. A list of tags, and the editor's moves
// -----------------------------------------------------------------------------

/// A list the Worker would accept (tags.mjs checkTags): every entry normalized, duplicates dropped, order kept, at most 8.
/// Nothing is dropped to make a list fit: the first bad tag refuses the whole list. An empty list is valid (it clears the
/// tags, and the review chooses them).
pub fn check_tags(values: &[String]) -> Result<Vec<String>, String> {
    if values.len() > MAX_ENTRIES_READ {
        return Err(format!("A Space holds at most {MAX_TAGS} tags, and this names {} entries. {TAG_RULES}", values.len()));
    }
    let mut tags: Vec<String> = Vec::new();
    for raw in values {
        let Some(tag) = normalize_tag(raw) else {
            return Err(format!("\"{}\" is not a valid tag. {TAG_RULES}", echo(raw)));
        };
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    if tags.len() > MAX_TAGS {
        return Err(format!("A Space holds at most {MAX_TAGS} tags, and this names {}. Remove one to add another. {TAG_RULES}", tags.len()));
    }
    Ok(tags)
}

/// What the editor does when a person adds: the text typed (one tag, or several split by spaces and commas) joins the
/// tags already there. All or nothing, like the Worker: a bad tag or a ninth tag leaves the list as it was and says why.
pub fn add_tags(current: &[String], typed: &str) -> Result<Vec<String>, String> {
    let pieces: Vec<&str> = typed.split(|c: char| c == ',' || is_trimmed(c)).filter(|p| !p.is_empty()).collect();
    if pieces.is_empty() {
        return Err("Type a tag, then press Add.".to_string());
    }
    let mut all: Vec<String> = current.to_vec();
    all.extend(pieces.into_iter().map(str::to_string));
    check_tags(&all)
}

/// The tags without `tag`.
pub fn remove_tag(current: &[String], tag: &str) -> Vec<String> {
    current.iter().filter(|t| t.as_str() != tag).cloned().collect()
}

/// Whether two lists hold the same tags in any order (tags.mjs sameTagSet): reordering is not a change.
pub fn same_set(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().all(|t| b.contains(t))
}

/// Whether a free-text search matches any of a Space's tags: part of a tag, with or without the leading `#` (tags.mjs
/// queryMatchesTag).
pub fn query_matches_tag(query: &str, tags: &[String]) -> bool {
    let folded = fold(query).to_lowercase();
    let needle = folded.strip_prefix('#').unwrap_or(&folded);
    !needle.is_empty() && tags.iter().any(|t| t.contains(needle))
}

// -----------------------------------------------------------------------------
// 4. The Gallery's address
// -----------------------------------------------------------------------------

/// The tag an old `?genre=` link names: the genre's words joined by hyphens, as a tag. "All" named no genre.
/// Old links read `?genre=Horror`, `?genre=Sci-Fi` and `?genre=Town%20and%20City`.
pub fn legacy_genre_tag(genre: &str) -> Option<String> {
    let words = genre.split_whitespace().collect::<Vec<_>>().join("-");
    if words.eq_ignore_ascii_case("all") {
        return None;
    }
    normalize_tag(&words)
}

/// The tags the Gallery filters by, from the address: every `tag=` value that is a valid tag (anything else is ignored, so a
/// hand-typed address cannot put markup or a long string on the page), then an old `genre=` as a tag. Duplicates are dropped,
/// the order is kept, and no more than 8 are used.
pub fn tags_in_address(tag_values: &[String], genre: Option<&str>) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for tag in tag_values.iter().filter_map(|v| normalize_tag(v)).chain(genre.and_then(legacy_genre_tag)) {
        if !tags.contains(&tag) && tags.len() < MAX_TAGS {
            tags.push(tag);
        }
    }
    tags
}

/// `tag=a&tag=b`: one `tag=` for each, which the Worker reads as "every one of these" (AND).
pub fn tag_query(tags: &[String]) -> String {
    tags.iter().map(|t| format!("tag={}", urlencoding::encode(t))).collect::<Vec<_>>().join("&")
}

/// The Gallery's address for these tags: `/gallery`, or `/gallery?tag=a&tag=b`.
pub fn gallery_href(tags: &[String]) -> String {
    if tags.is_empty() {
        "/gallery".to_string()
    } else {
        format!("/gallery?{}", tag_query(tags))
    }
}

/// The tags after a chip is pressed: a tag that is on comes off, and one that is off is added (up to 8).
pub fn toggled(active: &[String], tag: &str) -> Vec<String> {
    if active.iter().any(|t| t == tag) {
        remove_tag(active, tag)
    } else if active.len() < MAX_TAGS {
        let mut next = active.to_vec();
        next.push(tag.to_string());
        next
    } else {
        active.to_vec()
    }
}

// -----------------------------------------------------------------------------
// 5. Who sees what
// -----------------------------------------------------------------------------

/// "Suggested by review", when the review chose a Space's tags (`tags_source` is `"judge"`) and the reader is its author.
/// Everyone else sees the tags and no label; a listing whose author chose them, or that has no tags yet, carries none.
pub fn suggested_label(tags: &[String], tags_source: Option<&str>, reader_is_author: bool) -> Option<&'static str> {
    (reader_is_author && tags_source == Some("judge") && !tags.is_empty()).then_some(SUGGESTED_BY_REVIEW)
}

/// The review's sentence about the tags it chose ("Tags chosen for your Space: a, b, c"), ready to show; nothing when the Worker
/// sent none or only blanks.
pub fn tags_chosen_line(chosen: Option<&str>) -> Option<String> {
    chosen.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

// -----------------------------------------------------------------------------
// 6. The tables
// -----------------------------------------------------------------------------

/// Runs of characters that fold to one ASCII character each: (first code point, last code point, the ASCII byte the first
/// becomes; each next one is the next byte). Sorted.
const FOLD_RUNS: &[(u32, u32, u8)] = &[
    (0xa0, 0xa0, b' '), (0xaa, 0xaa, b'a'), (0xb2, 0xb3, b'2'), (0xb9, 0xb9, b'1'),
    (0xba, 0xba, b'o'), (0x17f, 0x17f, b's'), (0x2b0, 0x2b0, b'h'), (0x2b2, 0x2b2, b'j'),
    (0x2b3, 0x2b3, b'r'), (0x2b7, 0x2b7, b'w'), (0x2b8, 0x2b8, b'y'), (0x2e1, 0x2e1, b'l'),
    (0x2e2, 0x2e2, b's'), (0x2e3, 0x2e3, b'x'), (0x1d2c, 0x1d2c, b'A'), (0x1d2e, 0x1d2e, b'B'),
    (0x1d30, 0x1d31, b'D'), (0x1d33, 0x1d3a, b'G'), (0x1d3c, 0x1d3c, b'O'), (0x1d3e, 0x1d3e, b'P'),
    (0x1d3f, 0x1d3f, b'R'), (0x1d40, 0x1d41, b'T'), (0x1d42, 0x1d42, b'W'), (0x1d43, 0x1d43, b'a'),
    (0x1d47, 0x1d47, b'b'), (0x1d48, 0x1d49, b'd'), (0x1d4d, 0x1d4d, b'g'), (0x1d4f, 0x1d4f, b'k'),
    (0x1d50, 0x1d50, b'm'), (0x1d52, 0x1d52, b'o'), (0x1d56, 0x1d56, b'p'), (0x1d57, 0x1d58, b't'),
    (0x1d5b, 0x1d5b, b'v'), (0x1d62, 0x1d62, b'i'), (0x1d63, 0x1d63, b'r'), (0x1d64, 0x1d65, b'u'),
    (0x1d9c, 0x1d9c, b'c'), (0x1da0, 0x1da0, b'f'), (0x1dbb, 0x1dbb, b'z'), (0x2000, 0x2000, b' '),
    (0x2001, 0x2001, b' '), (0x2002, 0x2002, b' '), (0x2003, 0x2003, b' '), (0x2004, 0x2004, b' '),
    (0x2005, 0x2005, b' '), (0x2006, 0x2006, b' '), (0x2007, 0x2007, b' '), (0x2008, 0x2008, b' '),
    (0x2009, 0x2009, b' '), (0x200a, 0x200a, b' '), (0x202f, 0x202f, b' '), (0x205f, 0x205f, b' '),
    (0x2070, 0x2070, b'0'), (0x2071, 0x2071, b'i'), (0x2074, 0x2079, b'4'), (0x207f, 0x207f, b'n'),
    (0x2080, 0x2089, b'0'), (0x2090, 0x2090, b'a'), (0x2091, 0x2091, b'e'), (0x2092, 0x2092, b'o'),
    (0x2093, 0x2093, b'x'), (0x2095, 0x2095, b'h'), (0x2096, 0x2099, b'k'), (0x209a, 0x209a, b'p'),
    (0x209b, 0x209c, b's'), (0x2102, 0x2102, b'C'), (0x210a, 0x210a, b'g'), (0x210b, 0x210b, b'H'),
    (0x210c, 0x210c, b'H'), (0x210d, 0x210d, b'H'), (0x210e, 0x210e, b'h'), (0x2110, 0x2110, b'I'),
    (0x2111, 0x2111, b'I'), (0x2112, 0x2112, b'L'), (0x2113, 0x2113, b'l'), (0x2115, 0x2115, b'N'),
    (0x2119, 0x211b, b'P'), (0x211c, 0x211c, b'R'), (0x211d, 0x211d, b'R'), (0x2124, 0x2124, b'Z'),
    (0x2128, 0x2128, b'Z'), (0x212a, 0x212a, b'K'), (0x212c, 0x212d, b'B'), (0x212f, 0x212f, b'e'),
    (0x2130, 0x2131, b'E'), (0x2133, 0x2133, b'M'), (0x2134, 0x2134, b'o'), (0x2139, 0x2139, b'i'),
    (0x2145, 0x2145, b'D'), (0x2146, 0x2147, b'd'), (0x2148, 0x2149, b'i'), (0x2160, 0x2160, b'I'),
    (0x2164, 0x2164, b'V'), (0x2169, 0x2169, b'X'), (0x216c, 0x216c, b'L'), (0x216d, 0x216e, b'C'),
    (0x216f, 0x216f, b'M'), (0x2170, 0x2170, b'i'), (0x2174, 0x2174, b'v'), (0x2179, 0x2179, b'x'),
    (0x217c, 0x217c, b'l'), (0x217d, 0x217e, b'c'), (0x217f, 0x217f, b'm'), (0x2460, 0x2468, b'1'),
    (0x24b6, 0x24cf, b'A'), (0x24d0, 0x24e9, b'a'), (0x24ea, 0x24ea, b'0'), (0x2c7c, 0x2c7c, b'j'),
    (0x2c7d, 0x2c7d, b'V'), (0x3000, 0x3000, b' '), (0xa7f2, 0xa7f2, b'C'), (0xa7f3, 0xa7f3, b'F'),
    (0xa7f4, 0xa7f4, b'Q'), (0xfe5f, 0xfe5f, b'#'), (0xfe63, 0xfe63, b'-'), (0xff03, 0xff03, b'#'),
    (0xff0d, 0xff0d, b'-'), (0xff10, 0xff19, b'0'), (0xff21, 0xff3a, b'A'), (0xff41, 0xff5a, b'a'),
    (0x107a5, 0x107a5, b'q'), (0x1ccd6, 0x1ccef, b'A'), (0x1ccf0, 0x1ccf9, b'0'), (0x1d400, 0x1d419, b'A'),
    (0x1d41a, 0x1d433, b'a'), (0x1d434, 0x1d44d, b'A'), (0x1d44e, 0x1d454, b'a'), (0x1d456, 0x1d467, b'i'),
    (0x1d468, 0x1d481, b'A'), (0x1d482, 0x1d49b, b'a'), (0x1d49c, 0x1d49c, b'A'), (0x1d49e, 0x1d49f, b'C'),
    (0x1d4a2, 0x1d4a2, b'G'), (0x1d4a5, 0x1d4a6, b'J'), (0x1d4a9, 0x1d4ac, b'N'), (0x1d4ae, 0x1d4b5, b'S'),
    (0x1d4b6, 0x1d4b9, b'a'), (0x1d4bb, 0x1d4bb, b'f'), (0x1d4bd, 0x1d4c3, b'h'), (0x1d4c5, 0x1d4cf, b'p'),
    (0x1d4d0, 0x1d4e9, b'A'), (0x1d4ea, 0x1d503, b'a'), (0x1d504, 0x1d505, b'A'), (0x1d507, 0x1d50a, b'D'),
    (0x1d50d, 0x1d514, b'J'), (0x1d516, 0x1d51c, b'S'), (0x1d51e, 0x1d537, b'a'), (0x1d538, 0x1d539, b'A'),
    (0x1d53b, 0x1d53e, b'D'), (0x1d540, 0x1d544, b'I'), (0x1d546, 0x1d546, b'O'), (0x1d54a, 0x1d550, b'S'),
    (0x1d552, 0x1d56b, b'a'), (0x1d56c, 0x1d585, b'A'), (0x1d586, 0x1d59f, b'a'), (0x1d5a0, 0x1d5b9, b'A'),
    (0x1d5ba, 0x1d5d3, b'a'), (0x1d5d4, 0x1d5ed, b'A'), (0x1d5ee, 0x1d607, b'a'), (0x1d608, 0x1d621, b'A'),
    (0x1d622, 0x1d63b, b'a'), (0x1d63c, 0x1d655, b'A'), (0x1d656, 0x1d66f, b'a'), (0x1d670, 0x1d689, b'A'),
    (0x1d68a, 0x1d6a3, b'a'), (0x1d7ce, 0x1d7d7, b'0'), (0x1d7d8, 0x1d7e1, b'0'), (0x1d7e2, 0x1d7eb, b'0'),
    (0x1d7ec, 0x1d7f5, b'0'), (0x1d7f6, 0x1d7ff, b'0'), (0x1f12b, 0x1f12b, b'C'), (0x1f12c, 0x1f12c, b'R'),
    (0x1f130, 0x1f149, b'A'), (0x1fbf0, 0x1fbf9, b'0'),
];

const FOLD_MULTI: &[(u32, &str)] = &[
    (0x132, "IJ"), (0x133, "ij"), (0x1c7, "LJ"), (0x1c8, "Lj"), (0x1c9, "lj"), (0x1ca, "NJ"),
    (0x1cb, "Nj"), (0x1cc, "nj"), (0x1f1, "DZ"), (0x1f2, "Dz"), (0x1f3, "dz"), (0x20a8, "Rs"),
    (0x2116, "No"), (0x2120, "SM"), (0x2121, "TEL"), (0x2122, "TM"), (0x213b, "FAX"), (0x2161, "II"),
    (0x2162, "III"), (0x2163, "IV"), (0x2165, "VI"), (0x2166, "VII"), (0x2167, "VIII"), (0x2168, "IX"),
    (0x216a, "XI"), (0x216b, "XII"), (0x2171, "ii"), (0x2172, "iii"), (0x2173, "iv"), (0x2175, "vi"),
    (0x2176, "vii"), (0x2177, "viii"), (0x2178, "ix"), (0x217a, "xi"), (0x217b, "xii"), (0x2469, "10"),
    (0x246a, "11"), (0x246b, "12"), (0x246c, "13"), (0x246d, "14"), (0x246e, "15"), (0x246f, "16"),
    (0x2470, "17"), (0x2471, "18"), (0x2472, "19"), (0x2473, "20"), (0x3250, "PTE"), (0x3251, "21"),
    (0x3252, "22"), (0x3253, "23"), (0x3254, "24"), (0x3255, "25"), (0x3256, "26"), (0x3257, "27"),
    (0x3258, "28"), (0x3259, "29"), (0x325a, "30"), (0x325b, "31"), (0x325c, "32"), (0x325d, "33"),
    (0x325e, "34"), (0x325f, "35"), (0x32b1, "36"), (0x32b2, "37"), (0x32b3, "38"), (0x32b4, "39"),
    (0x32b5, "40"), (0x32b6, "41"), (0x32b7, "42"), (0x32b8, "43"), (0x32b9, "44"), (0x32ba, "45"),
    (0x32bb, "46"), (0x32bc, "47"), (0x32bd, "48"), (0x32be, "49"), (0x32bf, "50"), (0x32cc, "Hg"),
    (0x32cd, "erg"), (0x32ce, "eV"), (0x32cf, "LTD"), (0x3371, "hPa"), (0x3372, "da"), (0x3373, "AU"),
    (0x3374, "bar"), (0x3375, "oV"), (0x3376, "pc"), (0x3377, "dm"), (0x3378, "dm2"), (0x3379, "dm3"),
    (0x337a, "IU"), (0x3380, "pA"), (0x3381, "nA"), (0x3383, "mA"), (0x3384, "kA"), (0x3385, "KB"),
    (0x3386, "MB"), (0x3387, "GB"), (0x3388, "cal"), (0x3389, "kcal"), (0x338a, "pF"), (0x338b, "nF"),
    (0x338e, "mg"), (0x338f, "kg"), (0x3390, "Hz"), (0x3391, "kHz"), (0x3392, "MHz"), (0x3393, "GHz"),
    (0x3394, "THz"), (0x3396, "ml"), (0x3397, "dl"), (0x3398, "kl"), (0x3399, "fm"), (0x339a, "nm"),
    (0x339c, "mm"), (0x339d, "cm"), (0x339e, "km"), (0x339f, "mm2"), (0x33a0, "cm2"), (0x33a1, "m2"),
    (0x33a2, "km2"), (0x33a3, "mm3"), (0x33a4, "cm3"), (0x33a5, "m3"), (0x33a6, "km3"), (0x33a9, "Pa"),
    (0x33aa, "kPa"), (0x33ab, "MPa"), (0x33ac, "GPa"), (0x33ad, "rad"), (0x33b0, "ps"), (0x33b1, "ns"),
    (0x33b3, "ms"), (0x33b4, "pV"), (0x33b5, "nV"), (0x33b7, "mV"), (0x33b8, "kV"), (0x33b9, "MV"),
    (0x33ba, "pW"), (0x33bb, "nW"), (0x33bd, "mW"), (0x33be, "kW"), (0x33bf, "MW"), (0x33c3, "Bq"),
    (0x33c4, "cc"), (0x33c5, "cd"), (0x33c8, "dB"), (0x33c9, "Gy"), (0x33ca, "ha"), (0x33cb, "HP"),
    (0x33cc, "in"), (0x33cd, "KK"), (0x33ce, "KM"), (0x33cf, "kt"), (0x33d0, "lm"), (0x33d1, "ln"),
    (0x33d2, "log"), (0x33d3, "lx"), (0x33d4, "mb"), (0x33d5, "mil"), (0x33d6, "mol"), (0x33d7, "PH"),
    (0x33d9, "PPM"), (0x33da, "PR"), (0x33db, "sr"), (0x33dc, "Sv"), (0x33dd, "Wb"), (0x33ff, "gal"),
    (0xfb00, "ff"), (0xfb01, "fi"), (0xfb02, "fl"), (0xfb03, "ffi"), (0xfb04, "ffl"), (0xfb05, "st"),
    (0xfb06, "st"), (0x1f12d, "CD"), (0x1f12e, "WZ"), (0x1f14a, "HV"), (0x1f14b, "MV"), (0x1f14c, "SD"),
    (0x1f14d, "SS"), (0x1f14e, "PPV"), (0x1f14f, "WC"), (0x1f16a, "MC"), (0x1f16b, "MD"), (0x1f16c, "MR"),
    (0x1f190, "DJ"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// A string of the code points of `s` moved to the full-width block (U+FF01 to U+FF5E), as tags.mjs's test does.
    fn full_width(s: &str) -> String {
        s.chars().map(|c| char::from_u32(c as u32 + 0xfee0).unwrap()).collect()
    }

    // These follow infrastructure/cloudflare/api/tests/listing_tags.test.mjs, case for case.

    #[test]
    fn the_limits_are_8_tags_of_2_to_24_characters() {
        assert_eq!((MAX_TAGS, TAG_MIN, TAG_MAX), (8, 2, 24));
        assert!(TAG_RULES.contains("2 to 24 characters") && TAG_RULES.contains("at most 8 tags"));
    }

    #[test]
    fn a_tag_is_trimmed_lower_cased_and_loses_one_leading_hash() {
        for (raw, want) in [("zombies", "zombies"), ("Zombies", "zombies"), ("ZOMBIES", "zombies"), ("#zombies", "zombies"), ("#Zombies", "zombies"), ("  #Tycoon  ", "tycoon"), ("\t\n#coop \n", "coop")] {
            assert_eq!(normalize_tag(raw).as_deref(), Some(want), "{raw:?}");
        }
        assert_eq!(normalize_tag("\u{a0}coop\u{a0}").as_deref(), Some("coop"), "a no-break space is white space");
        assert_eq!(normalize_tag("\u{3000}coop").as_deref(), Some("coop"), "so is an ideographic space");
        assert_eq!(normalize_tag("\u{feff}coop").as_deref(), Some("coop"), "the byte order mark is trimmed, as JavaScript trims it");
        assert_eq!(normalize_tag("\u{85}coop"), None, "U+0085 is not trimmed by JavaScript, so it is refused");
    }

    #[test]
    fn only_one_leading_hash_is_removed_and_nothing_else_is_stripped() {
        for bad in ["##zombies", "# zombies", "zombies#", "zom#bies", "#", "  #  ", "", "   "] {
            assert_eq!(normalize_tag(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn nfkc_folds_full_width_letters_digits_hyphens_and_hash_to_ascii() {
        assert_eq!(normalize_tag(&full_width("zombies")).as_deref(), Some("zombies"));
        assert_eq!(normalize_tag(&full_width("Zombies")).as_deref(), Some("zombies"));
        assert_eq!(normalize_tag(&full_width("#coop")).as_deref(), Some("coop"), "a full-width # is a #");
        assert_eq!(normalize_tag(&full_width("co-op")).as_deref(), Some("co-op"), "a full-width hyphen is a hyphen");
        assert_eq!(normalize_tag(&full_width("3d2")).as_deref(), Some("3d2"));
        assert_eq!(normalize_tag("\u{212a}ids").as_deref(), Some("kids"), "the Kelvin sign folds to k");
        // A few more that NFKC turns into ASCII: ligature, circled letter, mathematical bold, superscript digit, roman numeral.
        assert_eq!(normalize_tag("\u{fb01}sh").as_deref(), Some("fish"));
        assert_eq!(normalize_tag("\u{24d0}b").as_deref(), Some("ab"));
        assert_eq!(normalize_tag("\u{1d41a}\u{1d41b}").as_deref(), Some("ab"));
        assert_eq!(normalize_tag("x\u{b2}").as_deref(), Some("x2"));
        assert_eq!(normalize_tag("\u{2163}").as_deref(), Some("iv"));
    }

    #[test]
    fn hyphens_go_between_letters_or_digits_only() {
        for ok in ["co-op", "a-b", "co-op-2", "sci-fi-horror", "1-2-3", "#co-op"] {
            assert!(normalize_tag(ok).is_some(), "{ok}");
        }
        for bad in ["-coop", "coop-", "-coop-", "co--op", "co---op", "--", "-", "a--b", "#-coop", "coop-#"] {
            assert_eq!(normalize_tag(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_tag_is_2_to_24_characters_after_the_hash_is_removed() {
        assert_eq!(normalize_tag("a"), None, "1 character");
        assert_eq!(normalize_tag("#a"), None, "1 character behind a #");
        assert_eq!(normalize_tag("ab").as_deref(), Some("ab"));
        assert_eq!(normalize_tag("#ab").as_deref(), Some("ab"));
        assert_eq!(normalize_tag(&"a".repeat(24)), Some("a".repeat(24)), "24 characters");
        assert_eq!(normalize_tag(&format!("#{}", "a".repeat(24))), Some("a".repeat(24)), "the # does not count");
        assert_eq!(normalize_tag(&"a".repeat(25)), None, "25 characters");
        assert_eq!(normalize_tag(&format!("#{}", "a".repeat(25))), None);
        assert!(normalize_tag("abcdefghijk-abcdefghijk").is_some(), "hyphens count as characters: 23");
        assert_eq!(normalize_tag("abcdefghijkl-abcdefghijkl"), None, "25 with the hyphen");
    }

    #[test]
    fn digits_are_fine_alone_and_mixed_in() {
        for ok in ["2026", "3d", "1v1", "top10", "007", "0-0", "24h"] {
            assert_eq!(normalize_tag(ok).as_deref(), Some(ok));
        }
    }

    #[test]
    fn letters_outside_a_to_z_emoji_underscores_spaces_and_punctuation_are_refused() {
        for bad in [
            "caf\u{e9}", "\u{430}bc", "stra\u{df}e", "\u{130}stanbul", "\u{1f680}", "rocket\u{1f680}", "\u{1f680}\u{1f680}", "sci_fi", "_scifi", "sci fi", "sci\tfi", "sci\nfi", "sci.fi",
            "sci/fi", "sci+fi", "c++", "c#", "a@b", "what?", "hi!", "it's", "say\"hi\"", "<b>", "<script>", "a,b", "a;b", "a:b", "a(b)", "a*b", "a%20b", "a\u{0}b", "\u{200b}coop",
            "co\u{200b}op",
        ] {
            assert_eq!(normalize_tag(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_list_is_normalized_deduplicated_and_kept_in_the_order_it_was_sent() {
        let sent = list(&["Zombies", "#zombies", "ZOMBIES", "tycoon", "zombies", " #Coop ", &full_width("tycoon")]);
        assert_eq!(check_tags(&sent), Ok(list(&["zombies", "tycoon", "coop"])));
        assert_eq!(check_tags(&list(&["b2", "a1", "c3"])), Ok(list(&["b2", "a1", "c3"])), "the author's order, not alphabetical");
        assert_eq!(check_tags(&[]), Ok(Vec::new()), "an empty list is valid: the review chooses");
    }

    #[test]
    fn at_most_8_tags_8_pass_and_9_are_refused_with_the_limit_in_the_message() {
        let nine = list(&["t1", "t2", "t3", "t4", "t5", "t6", "t7", "t8", "t9"]);
        assert_eq!(check_tags(&nine[..8]).map(|t| t.len()), Ok(8));
        let err = check_tags(&nine).unwrap_err();
        assert!(err.contains("at most 8 tags") && err.contains("names 9"), "{err}");
        // The limit counts tags after duplicates are dropped.
        let mut with_dups = nine[..8].to_vec();
        with_dups.extend(list(&["T1", "#t2", "t3"]));
        assert!(check_tags(&with_dups).is_ok(), "duplicates do not count");
        let mut nine_and_dups = nine.clone();
        nine_and_dups.extend(list(&["dup", "dup"]));
        assert!(check_tags(&nine_and_dups).is_err());
    }

    #[test]
    fn a_list_far_past_the_limit_is_refused_without_reading_it() {
        let huge = vec!["ab".to_string(); 300];
        let err = check_tags(&huge).unwrap_err();
        assert!(err.contains("at most 8 tags") && err.contains("300 entries"), "{err}");
    }

    #[test]
    fn one_bad_tag_refuses_the_whole_list_and_the_message_names_the_first_bad_one() {
        let err = check_tags(&list(&["good", "b@d", "w^rse", "fine"])).unwrap_err();
        assert!(err.starts_with("\"b@d\" is not a valid tag."), "{err}");
        assert!(!err.contains("w^rse"), "only the first is named");
        assert!(err.ends_with(TAG_RULES));
        let many = list(&["a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "a9", "b@d"]);
        assert!(check_tags(&many).unwrap_err().starts_with("\"b@d\""), "the bad tag wins over the count");
    }

    #[test]
    fn a_refused_tag_is_repeated_at_most_40_characters_and_never_raw() {
        let long = check_tags(&["x".repeat(10_000)]).unwrap_err();
        assert!(long.contains(&format!("\"{}\u{2026}\"", "x".repeat(40))), "the first 40 and an ellipsis");
        assert!(!long.contains(&"x".repeat(41)));
        assert!(long.len() < 400);
        assert!(check_tags(&[format!("{}!", "x".repeat(40))]).unwrap_err().contains(&format!("\"{}\u{2026}\"", "x".repeat(40))), "41 characters");
        assert!(check_tags(&[format!("{}!", "y".repeat(39))]).unwrap_err().contains(&format!("\"{}!\"", "y".repeat(39))), "40 characters: all, no ellipsis");
        let script = check_tags(&list(&["<script>alert(\"x\")</script>"])).unwrap_err();
        assert!(script.contains("\\u003cscript\\u003ealert(\\u0022x\\u0022)"), "{script}");
        assert!(!script.contains('<') && !script.contains('>'));
        assert!(check_tags(&list(&["a\"b"])).unwrap_err().contains("\"a\\u0022b\""), "a double quote cannot close the quotes around the tag");
        assert!(check_tags(&list(&["a\u{202e}b"])).unwrap_err().contains("a\\u202eb"), "a direction override cannot reorder the message");
        assert!(check_tags(&list(&["a\u{200b}b"])).unwrap_err().contains("a\\u200bb"));
        assert!(check_tags(&list(&["rocket\u{1f680}"])).unwrap_err().contains("\"rocket\u{1f680}\""), "emoji stay readable");
        assert!(check_tags(&["\u{1f680}".repeat(60)]).unwrap_err().contains(&format!("\"{}\u{2026}\"", "\u{1f680}".repeat(40))), "counted in characters");
    }

    #[test]
    fn the_same_tags_in_another_order_are_the_same_set() {
        assert!(same_set(&list(&["a1", "b2"]), &list(&["b2", "a1"])));
        assert!(!same_set(&list(&["a1"]), &list(&["a1", "b2"])));
        assert!(!same_set(&list(&["a1", "b2"]), &list(&["a1", "c3"])));
        assert!(same_set(&[], &[]));
        assert!(!same_set(&[], &list(&["a1"])));
    }

    // The editor's moves.

    #[test]
    fn adding_takes_one_tag_or_several_split_by_spaces_and_commas_and_keeps_what_was_there() {
        let now = list(&["zombies"]);
        assert_eq!(add_tags(&now, "Co-Op"), Ok(list(&["zombies", "co-op"])));
        assert_eq!(add_tags(&now, "#tycoon, coop  2026"), Ok(list(&["zombies", "tycoon", "coop", "2026"])));
        assert_eq!(add_tags(&now, "ZOMBIES"), Ok(now.clone()), "a tag already there changes nothing");
        assert_eq!(add_tags(&[], "ab"), Ok(list(&["ab"])));
    }

    #[test]
    fn adding_nothing_or_a_bad_tag_leaves_the_list_and_says_why() {
        let now = list(&["zombies"]);
        assert_eq!(add_tags(&now, ""), Err("Type a tag, then press Add.".to_string()));
        assert_eq!(add_tags(&now, " , ,  "), Err("Type a tag, then press Add.".to_string()));
        let err = add_tags(&now, "coop sci_fi tycoon").unwrap_err();
        assert!(err.starts_with("\"sci_fi\" is not a valid tag."), "{err}");
        assert!(add_tags(&now, "a").is_err(), "one character is too short");
        assert!(add_tags(&now, &"a".repeat(25)).is_err(), "25 characters is too long");
    }

    #[test]
    fn the_editor_holds_8_tags_and_the_ninth_is_refused_with_the_way_out() {
        let eight = list(&["t1", "t2", "t3", "t4", "t5", "t6", "t7", "t8"]);
        let err = add_tags(&eight, "t9").unwrap_err();
        assert!(err.contains("at most 8 tags") && err.contains("Remove one to add another"), "{err}");
        assert_eq!(add_tags(&eight, "t8"), Ok(eight.clone()), "a repeat of one of the eight is not a ninth");
        let seven = list(&["t1", "t2", "t3", "t4", "t5", "t6", "t7"]);
        assert_eq!(add_tags(&seven, "t8").map(|t| t.len()), Ok(8));
        assert!(add_tags(&seven, "t8 t9").is_err(), "all or nothing: two more would be nine");
        assert_eq!(remove_tag(&eight, "t3").len(), 7);
        assert_eq!(add_tags(&remove_tag(&eight, "t3"), "t9").map(|t| t.len()), Ok(8));
    }

    #[test]
    fn removing_takes_out_only_that_tag() {
        assert_eq!(remove_tag(&list(&["a1", "b2", "c3"]), "b2"), list(&["a1", "c3"]));
        assert_eq!(remove_tag(&list(&["a1"]), "zz"), list(&["a1"]));
        assert!(remove_tag(&list(&["a1"]), "a1").is_empty(), "no tags is allowed: the review chooses");
    }

    #[test]
    fn a_search_matches_part_of_a_tag_with_or_without_the_hash() {
        let tags = list(&["zombies", "co-op"]);
        assert!(query_matches_tag("zomb", &tags));
        assert!(query_matches_tag("#zomb", &tags));
        assert!(query_matches_tag("CO-O", &tags));
        assert!(query_matches_tag("op", &tags));
        assert!(!query_matches_tag("", &tags));
        assert!(!query_matches_tag("#", &tags));
        assert!(!query_matches_tag("racing", &tags));
        assert!(!query_matches_tag("zomb", &[]));
    }

    // The Gallery's address.

    #[test]
    fn the_filter_is_one_tag_equals_per_tag_and_repeats() {
        assert_eq!(tag_query(&list(&["horror"])), "tag=horror");
        assert_eq!(tag_query(&list(&["horror", "co-op", "2026"])), "tag=horror&tag=co-op&tag=2026");
        assert_eq!(tag_query(&[]), "");
        assert_eq!(gallery_href(&[]), "/gallery");
        assert_eq!(gallery_href(&list(&["horror"])), "/gallery?tag=horror");
        assert_eq!(gallery_href(&list(&["horror", "co-op"])), "/gallery?tag=horror&tag=co-op");
        assert!(!gallery_href(&list(&["a b&c"])).contains(' '), "a value is encoded");
        assert!(!gallery_href(&list(&["a b&c"])).contains("&c"));
        assert!(!tag_query(&list(&["horror"])).contains("genre"));
    }

    #[test]
    fn an_old_genre_link_becomes_a_tag() {
        assert_eq!(legacy_genre_tag("Horror").as_deref(), Some("horror"));
        assert_eq!(legacy_genre_tag("Sci-Fi").as_deref(), Some("sci-fi"));
        assert_eq!(legacy_genre_tag("Town and City").as_deref(), Some("town-and-city"));
        assert_eq!(legacy_genre_tag("FPS").as_deref(), Some("fps"));
        assert_eq!(legacy_genre_tag("All"), None, "All named no genre");
        assert_eq!(legacy_genre_tag("all"), None);
        assert_eq!(legacy_genre_tag(""), None);
        assert_eq!(legacy_genre_tag("<script>"), None);
        assert_eq!(tags_in_address(&[], Some("Horror")), list(&["horror"]));
        assert_eq!(gallery_href(&tags_in_address(&[], Some("Horror"))), "/gallery?tag=horror");
    }

    #[test]
    fn the_address_gives_every_valid_tag_once_in_order_and_no_more_than_eight() {
        assert_eq!(tags_in_address(&list(&["Horror", "#co-op", "horror"]), None), list(&["horror", "co-op"]));
        assert_eq!(tags_in_address(&list(&["horror"]), Some("Horror")), list(&["horror"]), "an old genre that is also a tag is one tag");
        assert_eq!(tags_in_address(&list(&["horror"]), Some("Racing")), list(&["horror", "racing"]));
        assert_eq!(tags_in_address(&list(&["<script>", "two words", "a", "", "ok"]), None), list(&["ok"]), "anything else is ignored");
        let ten: Vec<String> = (1..=10).map(|i| format!("t{i}")).collect();
        assert_eq!(tags_in_address(&ten, None).len(), 8);
        assert!(tags_in_address(&[], None).is_empty());
    }

    #[test]
    fn a_chip_turns_its_tag_on_or_off() {
        assert_eq!(toggled(&[], "horror"), list(&["horror"]));
        assert_eq!(toggled(&list(&["horror"]), "co-op"), list(&["horror", "co-op"]), "a second tag narrows it");
        assert_eq!(toggled(&list(&["horror", "co-op"]), "horror"), list(&["co-op"]));
        assert!(toggled(&list(&["horror"]), "horror").is_empty());
        let eight: Vec<String> = (1..=8).map(|i| format!("t{i}")).collect();
        assert_eq!(toggled(&eight, "t9"), eight, "no ninth");
    }

    // Who sees what.

    #[test]
    fn suggested_by_review_shows_only_to_the_author_and_only_for_tags_the_judge_chose() {
        let tags = list(&["racing"]);
        assert_eq!(suggested_label(&tags, Some("judge"), true), Some("Suggested by review"));
        assert_eq!(suggested_label(&tags, Some("judge"), false), None, "not the author");
        assert_eq!(suggested_label(&tags, Some("author"), true), None, "the author chose them");
        assert_eq!(suggested_label(&tags, None, true), None, "absent means the author");
        assert_eq!(suggested_label(&tags, Some("something new"), true), None);
        assert_eq!(suggested_label(&[], Some("judge"), true), None, "nothing to label");
    }

    #[test]
    fn the_reviews_sentence_about_its_tags_shows_as_sent_and_a_blank_one_not_at_all() {
        assert_eq!(tags_chosen_line(Some("Tags chosen for your Space: harbour, tides")).as_deref(), Some("Tags chosen for your Space: harbour, tides"));
        assert_eq!(tags_chosen_line(Some("  x  ")).as_deref(), Some("x"));
        assert_eq!(tags_chosen_line(Some("   ")), None);
        assert_eq!(tags_chosen_line(Some("")), None);
        assert_eq!(tags_chosen_line(None), None);
    }

    #[test]
    fn the_words_here_use_no_dashes_of_the_long_kind() {
        for s in [TAG_RULES, TAGS_HELP, SUGGESTED_BY_REVIEW] {
            assert!(!s.contains('\u{2014}') && !s.contains('\u{2013}'), "{s}");
        }
        assert_eq!(TAGS_HELP, "Leave them empty and the review chooses tags for you. Changing tags sends your Space back to review.");
    }

    #[test]
    fn the_fold_tables_are_sorted_and_do_not_overlap() {
        for pair in FOLD_RUNS.windows(2) {
            assert!(pair[0].1 < pair[1].0, "runs {:x?} and {:x?}", pair[0], pair[1]);
        }
        for run in FOLD_RUNS {
            assert!(run.0 <= run.1 && run.0 >= 0x80);
        }
        for pair in FOLD_MULTI.windows(2) {
            assert!(pair[0].0 < pair[1].0);
        }
        for m in FOLD_MULTI {
            assert!(!FOLD_RUNS.iter().any(|r| (r.0..=r.1).contains(&m.0)), "{:x} is in both tables", m.0);
        }
    }
}
