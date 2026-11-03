//! Canonical ECMAScript identifier values and module binding early errors.

use std::borrow::Cow;

use oxc_syntax::identifier::is_identifier_name;
use oxc_syntax::keyword::is_reserved_keyword;

use super::ecmascript_literal::identifier_string_value;

/// Source-owned grammar parameters, independent of a binding's lookup owner.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct EcmascriptNameContext {
    /// A module is always strict; Script strictness follows its source boundary.
    pub(crate) mode: EcmascriptSourceMode,
    /// The current async/static-initializer grammar reserves await.
    pub(crate) await_keyword: bool,
    /// The current generator grammar reserves yield, independently of strictness.
    pub(crate) yield_keyword: bool,
    /// Class field/static initializers prohibit lexical arguments references.
    pub(crate) arguments_forbidden: bool,
}

/// Source mode prevents an impossible non-strict module context.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum EcmascriptSourceMode {
    /// Script code without an inherited strict directive or class boundary.
    #[default]
    SloppyScript,
    /// Script code under an actual directive or class boundary.
    StrictScript,
    /// Module code reserves await throughout and is always strict.
    Module,
}

impl EcmascriptNameContext {
    /// Establish an explicit module goal without interpreting runtime loaders.
    pub(crate) const fn module() -> Self {
        Self {
            mode: EcmascriptSourceMode::Module,
            await_keyword: true,
            yield_keyword: false,
            arguments_forbidden: false,
        }
    }

    /// Whether the source's explicit goal reserves await throughout.
    pub(crate) const fn is_module(self) -> bool {
        matches!(self.mode, EcmascriptSourceMode::Module)
    }

    /// Whether binding and identifier-reference strict early errors apply.
    pub(crate) const fn is_strict(self) -> bool {
        !matches!(self.mode, EcmascriptSourceMode::SloppyScript)
    }

    /// Add an inherited directive/class restriction without weakening a module.
    pub(crate) const fn with_strict(mut self, required: bool) -> Self {
        if required && matches!(self.mode, EcmascriptSourceMode::SloppyScript) {
            self.mode = EcmascriptSourceMode::StrictScript;
        }
        self
    }

    /// Enter a callable body/formal grammar while retaining its source goal.
    pub(crate) const fn callable(self, asynchronous: bool, generator: bool, arrow: bool) -> Self {
        Self {
            await_keyword: self.is_module() || asynchronous,
            yield_keyword: generator,
            arguments_forbidden: arrow && self.arguments_forbidden,
            ..self
        }
    }
}

/// Syntax roles have different early errors even in the same strict source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EcmascriptNameRole {
    /// A function, variable, parameter or other ordinary binding leaf.
    Binding,
    /// A let/const binding additionally forbids the name let in non-strict code.
    LexicalBinding,
    /// An expression may reference eval/arguments without declaring either name.
    Reference,
}

/// Decode an `IdentifierName`, which may name a reserved exported property.
pub(super) fn identifier_name_value(text: &str) -> Option<Cow<'_, str>> {
    identifier_string_value(text).filter(|value| is_identifier_name(value))
}

/// Validate a local `BindingIdentifier` in an ECMAScript module's strict code.
pub(super) fn binding_identifier_value(text: &str) -> Option<Cow<'_, str>> {
    contextual_identifier_value(
        text,
        EcmascriptNameContext::module(),
        EcmascriptNameRole::Binding,
    )
}

/// Validate one cooked grammar-owned name without conflating bindings and reads.
pub(super) fn contextual_identifier_value(
    text: &str,
    context: EcmascriptNameContext,
    role: EcmascriptNameRole,
) -> Option<Cow<'_, str>> {
    identifier_name_value(text).filter(|value| match value.as_ref() {
        "eval" => role == EcmascriptNameRole::Reference || !context.is_strict(),
        "arguments" => {
            if role == EcmascriptNameRole::Reference {
                !context.arguments_forbidden
            } else {
                !context.is_strict()
            }
        }
        "await" => !context.is_module() && !context.await_keyword,
        "yield" => !context.is_strict() && !context.yield_keyword,
        "let" => !context.is_strict() && role != EcmascriptNameRole::LexicalBinding,
        "implements" | "interface" | "package" | "private" | "protected" | "public" | "static" => {
            !context.is_strict()
        }
        _ => !is_reserved_keyword(value),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        EcmascriptNameContext, EcmascriptNameRole, binding_identifier_value,
        contextual_identifier_value, identifier_name_value,
    };

    #[test]
    fn source_context_separates_binding_and_reference_early_errors() {
        let script = EcmascriptNameContext::default();
        let strict = script.with_strict(true);
        for name in [
            "eval",
            "arguments",
            "await",
            "yield",
            "let",
            "static",
            "interface",
        ] {
            assert!(
                contextual_identifier_value(name, script, EcmascriptNameRole::Binding).is_some()
            );
        }
        for name in ["eval", "arguments"] {
            assert!(
                contextual_identifier_value(name, strict, EcmascriptNameRole::Binding).is_none()
            );
            assert!(
                contextual_identifier_value(name, strict, EcmascriptNameRole::Reference).is_some()
            );
        }
        assert!(
            contextual_identifier_value("await", strict, EcmascriptNameRole::Binding).is_some()
        );
        assert!(
            contextual_identifier_value(
                "await",
                EcmascriptNameContext::module(),
                EcmascriptNameRole::Reference
            )
            .is_none()
        );
        assert!(
            contextual_identifier_value(
                "await",
                script.callable(true, false, false),
                EcmascriptNameRole::Binding
            )
            .is_none()
        );
        assert!(
            contextual_identifier_value(
                "yield",
                script.callable(false, true, false),
                EcmascriptNameRole::Binding
            )
            .is_none()
        );
        assert!(
            contextual_identifier_value("let", script, EcmascriptNameRole::LexicalBinding)
                .is_none()
        );
        let field = EcmascriptNameContext {
            arguments_forbidden: true,
            ..strict
        };
        assert!(
            contextual_identifier_value(
                "arguments",
                field.callable(false, false, true),
                EcmascriptNameRole::Reference
            )
            .is_none()
        );
        assert!(
            contextual_identifier_value(
                "arguments",
                field.callable(false, false, false),
                EcmascriptNameRole::Reference
            )
            .is_some()
        );
    }

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
