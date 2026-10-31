//! Static ECMAScript module string values, without evaluating source.

use std::borrow::Cow;
use std::iter::Peekable;
use std::str::Chars;

/// Decode identifier `StringValue` without accepting string-only escape forms.
///
/// Identifier escapes denote individual Unicode scalar values. Unlike a string
/// literal, two surrogate escapes cannot combine into a valid identifier scalar.
pub(super) fn identifier_string_value(text: &str) -> Option<Cow<'_, str>> {
    if !text.contains('\\') {
        return Some(Cow::Borrowed(text));
    }
    let mut output = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        if characters.next()? != 'u' {
            return None;
        }
        output.push(char::from_u32(unicode_escape_value(&mut characters)?)?);
    }
    Some(Cow::Owned(output))
}

/// Decode one complete quoted module literal using strict string escape semantics.
///
/// Plain values borrow their source. Escaped values retain UTF-16 code units until
/// `String::from_utf16` validates surrogate pairs; lone surrogates cannot identify
/// the repository's lossless UTF-8 paths. Storage admission owns invalid identity
/// scalars such as NUL, which must not be silently discarded here.
pub(super) fn module_string_value(text: &str) -> Option<Cow<'_, str>> {
    let text = text.trim();
    let quote = text
        .chars()
        .next()
        .filter(|quote| matches!(quote, '\'' | '"'))?;
    let value = text.strip_prefix(quote)?.strip_suffix(quote)?;
    if !value.contains('\\') {
        return (!value.contains([quote, '\n', '\r'])).then_some(Cow::Borrowed(value));
    }

    let mut units = Vec::with_capacity(value.len());
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if character == quote || matches!(character, '\n' | '\r') {
            return None;
        }
        if character != '\\' {
            append_code_point(&mut units, u32::from(character))?;
            continue;
        }
        let escaped = characters.next()?;
        let code_point = match escaped {
            '\n' | '\u{2028}' | '\u{2029}' => continue,
            '\r' => {
                characters.next_if_eq(&'\n');
                continue;
            }
            'b' => 0x08,
            't' => 0x09,
            'n' => 0x0a,
            'v' => 0x0b,
            'f' => 0x0c,
            'r' => 0x0d,
            '0' if !characters.peek().is_some_and(char::is_ascii_digit) => 0,
            // Static imports are module code: legacy decimal/octal escapes are invalid.
            '0'..='9' => return None,
            'x' => fixed_hex_value(&mut characters, 2)?,
            'u' => unicode_escape_value(&mut characters)?,
            other => u32::from(other),
        };
        append_code_point(&mut units, code_point)?;
    }
    String::from_utf16(&units).ok().map(Cow::Owned)
}

/// Consume exactly the grammar's required number of hexadecimal digits.
fn fixed_hex_value(characters: &mut Peekable<Chars<'_>>, digits: usize) -> Option<u32> {
    (0..digits).try_fold(0_u32, |value, _| {
        value
            .checked_mul(16)?
            .checked_add(characters.next()?.to_digit(16)?)
    })
}

/// Decode fixed-width UTF-16 escapes or an explicit bounded Unicode code point.
fn unicode_escape_value(characters: &mut Peekable<Chars<'_>>) -> Option<u32> {
    if characters.next_if_eq(&'{').is_none() {
        return fixed_hex_value(characters, 4);
    }
    let mut value = 0_u32;
    let mut has_digit = false;
    loop {
        let character = characters.next()?;
        if character == '}' {
            return has_digit.then_some(value);
        }
        value = value
            .checked_mul(16)?
            .checked_add(character.to_digit(16)?)?;
        if value > 0x10_ffff {
            return None;
        }
        has_digit = true;
    }
}

/// Preserve UTF-16 code units until the complete string validates paired surrogates.
fn append_code_point(units: &mut Vec<u16>, code_point: u32) -> Option<()> {
    if let Ok(unit) = u16::try_from(code_point) {
        units.push(unit);
    } else {
        let character = char::from_u32(code_point)?;
        let mut encoded = [0_u16; 2];
        units.extend_from_slice(character.encode_utf16(&mut encoded));
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::module_string_value;
    use std::borrow::Cow;

    #[test]
    fn plain_literals_borrow_exact_module_spelling() {
        assert_eq!(
            module_string_value("'./a  b.js '"),
            Some(Cow::Borrowed("./a  b.js "))
        );
    }

    #[test]
    fn strict_invalid_or_non_utf8_literals_abstain() {
        for literal in [
            "'unterminated",
            "'embedded'quote'",
            "'one' trailing",
            "'raw\nline'",
            "'raw\rline'",
            "'raw\n\\u0061'",
            "'raw\r\\u0061'",
            r"'\1'",
            r"'\01'",
            r"'\8'",
            r"'\9'",
            r"'\xG0'",
            r"'\u00G0'",
            r"'\u{}'",
            r"'\u{110000}'",
            r"'\uD800'",
            r"'\uDC00'",
        ] {
            assert!(module_string_value(literal).is_none(), "accepted {literal}");
        }
    }

    #[test]
    fn decoded_controls_reach_identity_admission_without_becoming_raw_escape_paths() {
        assert_eq!(
            module_string_value(r"'./bad\x00name'"),
            Some(Cow::Owned("./bad\0name".into()))
        );
        assert_eq!(
            module_string_value(r"'./bad\0name'"),
            Some(Cow::Owned("./bad\0name".into()))
        );
    }

    #[test]
    fn unicode_separators_are_values_unless_they_follow_a_continuation_backslash() {
        for separator in ['\u{2028}', '\u{2029}'] {
            let plain = format!("'./raw{separator}.js'");
            let escaped = format!("'./\\u0061{separator}.js'");
            let continued = format!("'./de\\{separator}p.js'");
            assert_eq!(
                module_string_value(&plain).as_deref(),
                Some(format!("./raw{separator}.js").as_str())
            );
            assert_eq!(
                module_string_value(&escaped).as_deref(),
                Some(format!("./a{separator}.js").as_str())
            );
            assert_eq!(module_string_value(&continued).as_deref(), Some("./dep.js"));
        }
        assert_eq!(
            module_string_value(r"'\u2028'").as_deref(),
            Some("\u{2028}")
        );
        assert_eq!(
            module_string_value(r"'\u2029'").as_deref(),
            Some("\u{2029}")
        );
    }
}
