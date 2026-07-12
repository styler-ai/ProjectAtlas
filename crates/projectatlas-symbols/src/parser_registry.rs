//! Closed registry for built-in tree-sitter parsers.

use tree_sitter::Language;

/// `ProjectAtlas` language identifiers backed by built-in tree-sitter parsers.
const SPECIALIZED_LANGUAGES: &[&str] = &[
    "rust",
    "rust-build-script",
    "python",
    "javascript",
    "typescript",
    "tsx",
    "java",
    "kotlin",
    "csharp",
    "go",
    "objective-c",
    "zig",
    "c",
    "cpp",
    "h",
    "hpp",
];

/// Return whether the language has a specialized tree-sitter parser.
#[must_use]
pub fn has_specialized_parser(language: &str) -> bool {
    parser_language(language).is_some()
}

/// Return all specialized parser language identifiers.
#[must_use]
pub fn specialized_languages() -> &'static [&'static str] {
    SPECIALIZED_LANGUAGES
}

/// Return the built-in tree-sitter parser for a `ProjectAtlas` language identifier.
pub(crate) fn parser_language(language: &str) -> Option<Language> {
    match language {
        "rust" | "rust-build-script" => Some(tree_sitter_rust::LANGUAGE.into()),
        "python" => Some(tree_sitter_python::LANGUAGE.into()),
        "javascript" => Some(tree_sitter_javascript::LANGUAGE.into()),
        "typescript" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        "tsx" => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        "java" => Some(tree_sitter_java::LANGUAGE.into()),
        "kotlin" => Some(tree_sitter_kotlin_ng::LANGUAGE.into()),
        "csharp" => Some(tree_sitter_c_sharp::LANGUAGE.into()),
        "go" => Some(tree_sitter_go::LANGUAGE.into()),
        "objective-c" => Some(tree_sitter_objc::LANGUAGE.into()),
        "zig" => Some(tree_sitter_zig::LANGUAGE.into()),
        "c" | "h" => Some(tree_sitter_c::LANGUAGE.into()),
        "cpp" | "hpp" => Some(tree_sitter_cpp::LANGUAGE.into()),
        _ => None,
    }
}
