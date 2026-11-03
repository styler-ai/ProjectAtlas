//! Native identifier equality for grammar-owned lexical names, preserving display text.

use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};

/// Apply C# lexical equality without case folding or Unicode normalization.
pub(super) fn csharp(text: &str) -> Option<String> {
    let text = text.strip_prefix('@').unwrap_or(text);
    let mut characters = text.chars();
    let mut result = String::with_capacity(text.len());
    while let Some(character) = characters.next() {
        let character = universal_character(character, &mut characters)?;
        let category = character.general_category();
        let start = character == '_'
            || matches!(
                category,
                GeneralCategory::UppercaseLetter
                    | GeneralCategory::LowercaseLetter
                    | GeneralCategory::TitlecaseLetter
                    | GeneralCategory::ModifierLetter
                    | GeneralCategory::OtherLetter
                    | GeneralCategory::LetterNumber
            );
        if !start
            && (result.is_empty()
                || !matches!(
                    category,
                    GeneralCategory::DecimalNumber
                        | GeneralCategory::ConnectorPunctuation
                        | GeneralCategory::NonspacingMark
                        | GeneralCategory::SpacingMark
                        | GeneralCategory::Format
                ))
        {
            return None;
        }
        if category != GeneralCategory::Format {
            result.push(character);
        }
    }
    (!result.is_empty()).then_some(result)
}

/// Decode C++ universal character names without folding ordinary source names.
pub(super) fn cpp(text: &str) -> Option<String> {
    let mut characters = text.chars();
    let mut result = String::with_capacity(text.len());
    while let Some(character) = characters.next() {
        let decoded = universal_character(character, &mut characters)?;
        if character == '\\'
            && (decoded < '\u{a0}'
                || if result.is_empty() {
                    !unicode_ident::is_xid_start(decoded)
                } else {
                    !unicode_ident::is_xid_continue(decoded)
                })
        {
            return None;
        }
        result.push(decoded);
    }
    (!result.is_empty()).then_some(result)
}

/// Both native grammars admit fixed-width Unicode escapes inside identifiers.
fn universal_character(first: char, characters: &mut std::str::Chars<'_>) -> Option<char> {
    if first != '\\' {
        return Some(first);
    }
    let width = match characters.next()? {
        'u' => 4,
        'U' => 8,
        _ => return None,
    };
    let mut scalar = 0_u32;
    for _ in 0..width {
        scalar = scalar
            .checked_mul(16)?
            .checked_add(characters.next()?.to_digit(16)?)?;
    }
    char::from_u32(scalar)
}

#[cfg(test)]
mod tests {
    use super::{cpp, csharp as value};

    #[test]
    fn identifier_equality_preserves_exact_unicode_and_rejects_invalid_scalars() {
        for spelling in ["N", "@N", r"\u004e", r"\U0000004e", "N\u{200d}"] {
            assert_eq!(value(spelling).as_deref(), Some("N"), "{spelling}");
        }
        for spelling in [
            "",
            "@",
            "@@N",
            r"\uD800",
            r"\U00110000",
            r"\u004",
            r"\x4e",
            "1N",
            "\u{200d}N",
        ] {
            assert_eq!(value(spelling), None, "{spelling}");
        }
        assert_ne!(value("N"), value("n"));
        assert_ne!(value("é"), value("e\u{301}"));
        for spelling in ["α", r"\u03b1", r"\U000003b1"] {
            assert_eq!(cpp(spelling).as_deref(), Some("α"));
        }
        for spelling in [r"\uD800", r"\u0061", r"\UFFFFFFFF", r"\u0020"] {
            assert_eq!(cpp(spelling), None);
        }
    }
}
