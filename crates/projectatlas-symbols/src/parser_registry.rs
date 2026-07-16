//! Closed registry compatibility surface for built-in tree-sitter parsers.

use crate::language_parser_registry::{
    BuiltInParser, SPECIALIZED_LANGUAGES, built_in_parser_for_public_mode,
};
use tree_sitter::Language;

/// Return whether the language has a specialized tree-sitter parser.
#[must_use]
pub fn has_specialized_parser(language: &str) -> bool {
    built_in_parser_for_public_mode(language).is_some()
}

/// Return all specialized parser language identifiers.
#[must_use]
pub fn specialized_languages() -> &'static [&'static str] {
    SPECIALIZED_LANGUAGES
}

/// Adapt a generated closed parser identity to its compiled tree-sitter grammar.
pub(crate) fn parser_language(parser: BuiltInParser) -> Language {
    match parser {
        BuiltInParser::Rust => tree_sitter_rust::LANGUAGE.into(),
        BuiltInParser::Python => tree_sitter_python::LANGUAGE.into(),
        BuiltInParser::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        BuiltInParser::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        BuiltInParser::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        BuiltInParser::Java => tree_sitter_java::LANGUAGE.into(),
        BuiltInParser::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
        BuiltInParser::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
        BuiltInParser::Go => tree_sitter_go::LANGUAGE.into(),
        BuiltInParser::ObjectiveC => tree_sitter_objc::LANGUAGE.into(),
        BuiltInParser::Zig => tree_sitter_zig::LANGUAGE.into(),
        BuiltInParser::C => tree_sitter_c::LANGUAGE.into(),
        BuiltInParser::Cpp => tree_sitter_cpp::LANGUAGE.into(),
    }
}
