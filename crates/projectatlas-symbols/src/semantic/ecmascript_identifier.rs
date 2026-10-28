//! Canonical ECMAScript identifier values and module binding early errors.

use std::borrow::Cow;

use oxc_syntax::identifier::is_identifier_name;
use oxc_syntax::keyword::is_reserved_keyword;

use super::ecmascript_literal::identifier_string_value;

/// Decode an `IdentifierName`, which may name a reserved exported property.
pub(super) fn identifier_name_value(text: &str) -> Option<Cow<'_, str>> {
    identifier_string_value(text).filter(|value| is_identifier_name(value))
}

/// Validate a local `BindingIdentifier` in an ECMAScript module's strict code.
pub(super) fn binding_identifier_value(text: &str) -> Option<Cow<'_, str>> {
    identifier_name_value(text).filter(|value| {
        !is_reserved_keyword(value) && !matches!(value.as_ref(), "eval" | "arguments")
    })
}

#[cfg(test)]
mod tests {
    use super::{binding_identifier_value, identifier_name_value};

    #[test]
    fn module_binding_names_obey_canonical_characters_and_early_errors() {
        for name in [
            "class",
            "await",
            "yield",
            "let",
            "interface",
            "eval",
            "arguments",
            "foo.bar",
            "0start",
            "🙂",
            "\u{301}start",
            r"cl\u0061ss",
            r"\uD835\uDC9C",
            r"\x61",
            r"\u{110000}",
        ] {
            assert!(binding_identifier_value(name).is_none(), "{name}");
        }
        for (spelling, expected) in [
            ("undefined", "undefined"),
            ("async", "async"),
            ("type", "type"),
            ("$worker", "$worker"),
            ("café", "café"),
            ("a\u{200c}", "a\u{200c}"),
            (r"l\u0061unch", "launch"),
            (r"\u{1d49c}", "𝒜"),
        ] {
            assert_eq!(
                binding_identifier_value(spelling).as_deref(),
                Some(expected)
            );
        }
        assert_eq!(identifier_name_value("class").as_deref(), Some("class"));
        assert_eq!(identifier_name_value("default").as_deref(), Some("default"));
    }
}
