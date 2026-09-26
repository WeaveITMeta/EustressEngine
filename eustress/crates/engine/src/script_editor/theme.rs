//! # Script editor theme
//!
//! Maps each [`TokenClass`] to a colour taken from the live Studio palette
//! (the Slint `ThemeData`), so the editor follows the Studio theme. Services
//! choose classes; only this module chooses colours.

use super::language::TokenClass;

/// An 8-bit sRGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Parse `#rrggbb` (a trailing alpha pair is ignored).
    pub fn hex(s: &str) -> Option<Rgb> {
        let h = s.strip_prefix('#')?;
        if h.len() != 6 && h.len() != 8 {
            return None;
        }
        let at = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        Some(Rgb(at(0)?, at(2)?, at(4)?))
    }
}

/// The palette colours the editor draws with. The host fills it from the
/// active `ThemeData`; [`EditorPalette::DARK`] and [`EditorPalette::MODERN`]
/// are the two shipped palettes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorPalette {
    /// The code area's background (`viewport-background`). Token colours are
    /// kept readable against it.
    pub background: Rgb,
    pub text_primary: Rgb,
    pub text_secondary: Rgb,
    pub text_error: Rgb,
    pub accent_blue: Rgb,
    pub accent_cyan: Rgb,
    pub accent_green: Rgb,
    pub accent_orange: Rgb,
    pub accent_purple: Rgb,
    pub accent_yellow: Rgb,
}

impl EditorPalette {
    /// The shipped dark palette's values (`ui/slint/theme.slint`).
    pub const DARK: EditorPalette = EditorPalette {
        background: Rgb(0x0a, 0x0a, 0x0a),
        text_primary: Rgb(0xd4, 0xd4, 0xd4),
        text_secondary: Rgb(0x80, 0x80, 0x80),
        text_error: Rgb(0xf1, 0x4c, 0x4c),
        accent_blue: Rgb(0x00, 0x78, 0xd4),
        accent_cyan: Rgb(0x00, 0xbc, 0xd4),
        accent_green: Rgb(0x3c, 0xba, 0x54),
        accent_orange: Rgb(0xe8, 0x91, 0x2d),
        accent_purple: Rgb(0xb1, 0x80, 0xd7),
        accent_yellow: Rgb(0xf9, 0xc7, 0x4f),
    };

    /// The shipped Modern palette's values (`ui/slint/theme.slint`).
    pub const MODERN: EditorPalette = EditorPalette {
        background: Rgb(0x04, 0x07, 0x0d),
        text_primary: Rgb(0xe8, 0xec, 0xf3),
        text_secondary: Rgb(0x8b, 0x95, 0xa7),
        text_error: Rgb(0xff, 0x6b, 0x6b),
        accent_blue: Rgb(0x0a, 0x84, 0xff),
        accent_cyan: Rgb(0x22, 0xd3, 0xee),
        accent_green: Rgb(0x34, 0xd3, 0x99),
        accent_orange: Rgb(0xff, 0x9f, 0x45),
        accent_purple: Rgb(0xc0, 0x84, 0xfc),
        accent_yellow: Rgb(0xff, 0xd1, 0x66),
    };

    /// Selection and the caret line accent: Eustress cyan.
    pub fn selection(&self) -> Rgb {
        self.accent_cyan
    }
}

/// The minimum contrast for code text: WCAG AA for body text.
pub const MIN_CONTRAST: f32 = 4.5;

/// WCAG relative luminance of an sRGB colour.
pub fn luminance(c: Rgb) -> f32 {
    let lin = |v: u8| {
        let s = v as f32 / 255.0;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.0) + 0.7152 * lin(c.1) + 0.0722 * lin(c.2)
}

/// WCAG contrast ratio between two colours, from 1 to 21.
pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `fg`, blended toward white (on a dark background) or black (on a light
/// one) just far enough to reach [`MIN_CONTRAST`] against `bg`. A colour
/// that already reads is returned unchanged, so theme accents keep their hue.
pub fn readable(fg: Rgb, bg: Rgb) -> Rgb {
    if contrast(fg, bg) >= MIN_CONTRAST {
        return fg;
    }
    let target: u8 = if luminance(bg) < 0.18 { 255 } else { 0 };
    let mix = |c: u8, t: f32| (c as f32 + (target as f32 - c as f32) * t).round() as u8;
    for step in 1..=20 {
        let t = step as f32 / 20.0;
        let c = Rgb(mix(fg.0, t), mix(fg.1, t), mix(fg.2, t));
        if contrast(c, bg) >= MIN_CONTRAST {
            return c;
        }
    }
    Rgb(target, target, target)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenStyle {
    pub color: Rgb,
    pub bold: bool,
    pub italic: bool,
}

/// The style of one token class under `p`.
pub fn style_for(class: TokenClass, p: &EditorPalette) -> TokenStyle {
    use TokenClass::*;
    let (color, bold, italic) = match class {
        Keyword | Builtin => (p.accent_blue, false, false),
        ControlFlow | Interpolation => (p.accent_purple, false, false),
        Type => (p.accent_cyan, false, false),
        Function | Method => (p.accent_yellow, false, false),
        Constant => (p.accent_blue, true, false),
        Number => (p.accent_green, false, false),
        String => (p.accent_orange, false, false),
        StringEscape | Attribute => (p.accent_yellow, false, false),
        Comment => (p.text_secondary, false, true),
        DocComment => (p.accent_green, false, true),
        Punctuation => (p.text_secondary, false, false),
        Invalid => (p.text_error, false, false),
        Property | Variable | Parameter | Operator | Text => (p.text_primary, false, false),
    };
    TokenStyle { color: readable(color, p.background), bold, italic }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dark_palette_matches_its_hex_values() {
        assert_eq!(Rgb::hex("#00bcd4"), Some(EditorPalette::DARK.accent_cyan));
        assert_eq!(Rgb::hex("#0a84ff26"), Some(Rgb(0x0a, 0x84, 0xff)));
        assert_eq!(Rgb::hex("00bcd4"), None);
        assert_eq!(EditorPalette::DARK.selection(), Rgb(0x00, 0xbc, 0xd4));
    }

    const ALL: [TokenClass; 21] = [
        TokenClass::Keyword, TokenClass::ControlFlow, TokenClass::Type, TokenClass::Function,
        TokenClass::Method, TokenClass::Property, TokenClass::Variable, TokenClass::Parameter,
        TokenClass::Constant, TokenClass::Builtin, TokenClass::Number, TokenClass::String,
        TokenClass::StringEscape, TokenClass::Interpolation, TokenClass::Comment,
        TokenClass::DocComment, TokenClass::Operator, TokenClass::Punctuation,
        TokenClass::Attribute, TokenClass::Invalid, TokenClass::Text,
    ];

    #[test]
    fn every_token_class_reads_on_both_shipped_palettes() {
        for p in [EditorPalette::DARK, EditorPalette::MODERN] {
            for class in ALL {
                let c = style_for(class, &p).color;
                assert!(
                    contrast(c, p.background) >= MIN_CONTRAST,
                    "{class:?} {c:?} on {:?}: {:.2}",
                    p.background,
                    contrast(c, p.background),
                );
            }
        }
    }

    #[test]
    fn a_dim_accent_is_lifted_and_keeps_its_hue() {
        let p = EditorPalette::DARK;
        assert!(contrast(p.accent_blue, p.background) < MIN_CONTRAST, "the raw classic blue is too dim");
        let lifted = style_for(TokenClass::Keyword, &p).color;
        assert_ne!(lifted, p.accent_blue);
        assert!(lifted.2 > lifted.0, "still blue");
        assert_eq!(readable(p.text_primary, p.background), p.text_primary, "a readable colour is unchanged");
        assert!((contrast(Rgb(255, 255, 255), Rgb(0, 0, 0)) - 21.0).abs() < 0.01);
    }

    #[test]
    fn code_and_prose_classes_differ() {
        let p = EditorPalette::DARK;
        let kw = style_for(TokenClass::Keyword, &p).color;
        let text = style_for(TokenClass::Variable, &p).color;
        let comment = style_for(TokenClass::Comment, &p);
        assert_ne!(kw, text);
        assert!(comment.italic);
        assert_ne!(style_for(TokenClass::String, &p).color, text);
    }
}
