//! Purpose: Detect file language families from extensions.

/// Broad source extensions supported by the `ProjectAtlas` scanner.
pub const BROAD_SOURCE_EXTENSIONS: &[&str] = &[
    ".py",
    ".pyw",
    ".js",
    ".jsx",
    ".ts",
    ".tsx",
    ".mjs",
    ".cjs",
    ".d.ts",
    ".java",
    ".c",
    ".cpp",
    ".h",
    ".hpp",
    ".cxx",
    ".cc",
    ".hxx",
    ".hh",
    ".cs",
    ".go",
    ".m",
    ".mm",
    ".rb",
    ".php",
    ".swift",
    ".kt",
    ".kts",
    ".rs",
    ".scala",
    ".sh",
    ".bash",
    ".zsh",
    ".ps1",
    ".psm1",
    ".psd1",
    ".bat",
    ".cmd",
    ".r",
    ".R",
    ".pl",
    ".pm",
    ".lua",
    ".dart",
    ".hs",
    ".ml",
    ".mli",
    ".fs",
    ".fsx",
    ".clj",
    ".cljs",
    ".vim",
    ".zig",
    ".zon",
    ".html",
    ".htm",
    ".css",
    ".scss",
    ".sass",
    ".less",
    ".stylus",
    ".styl",
    ".md",
    ".mdx",
    ".json",
    ".jsonc",
    ".xml",
    ".yml",
    ".yaml",
    ".toml",
    ".toon",
    ".txt",
    ".ini",
    ".cfg",
    ".conf",
    ".vue",
    ".svelte",
    ".astro",
    ".jsp",
    ".jspx",
    ".jspf",
    ".tag",
    ".tagx",
    ".gsp",
    ".properties",
    ".gradle",
    ".groovy",
    ".proto",
    ".hbs",
    ".handlebars",
    ".ejs",
    ".pug",
    ".ftl",
    ".mustache",
    ".liquid",
    ".erb",
    ".sql",
    ".ddl",
    ".dml",
    ".mysql",
    ".postgresql",
    ".psql",
    ".sqlite",
    ".mssql",
    ".oracle",
    ".ora",
    ".db2",
    ".proc",
    ".procedure",
    ".func",
    ".function",
    ".view",
    ".trigger",
    ".index",
    ".migration",
    ".seed",
    ".fixture",
    ".schema",
    ".cql",
    ".cypher",
    ".sparql",
    ".gql",
    ".liquibase",
    ".flyway",
];

/// Parser coverage level available for a detected language family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanguageParserSupport {
    /// A native Tree-sitter adapter backs symbol extraction.
    Native,
    /// A manifest-specific parser backs package/dependency extraction.
    Manifest,
    /// A deterministic structural summarizer backs agent-facing summaries.
    Structural,
    /// A conservative fallback parser is the current coverage boundary.
    Fallback,
}

/// Static parser coverage metadata for one detected language family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LanguageSpec {
    /// Detected language or file-family identifier.
    pub language: &'static str,
    /// Parser coverage level.
    pub parser_support: LanguageParserSupport,
}

/// Supported detected language families and their parser coverage level.
pub const LANGUAGE_SPECS: &[LanguageSpec] = &[
    LanguageSpec {
        language: "rust",
        parser_support: LanguageParserSupport::Native,
    },
    LanguageSpec {
        language: "rust-build-script",
        parser_support: LanguageParserSupport::Native,
    },
    LanguageSpec {
        language: "python",
        parser_support: LanguageParserSupport::Native,
    },
    LanguageSpec {
        language: "javascript",
        parser_support: LanguageParserSupport::Native,
    },
    LanguageSpec {
        language: "typescript",
        parser_support: LanguageParserSupport::Native,
    },
    LanguageSpec {
        language: "tsx",
        parser_support: LanguageParserSupport::Native,
    },
    LanguageSpec {
        language: "java",
        parser_support: LanguageParserSupport::Native,
    },
    LanguageSpec {
        language: "kotlin",
        parser_support: LanguageParserSupport::Native,
    },
    LanguageSpec {
        language: "csharp",
        parser_support: LanguageParserSupport::Native,
    },
    LanguageSpec {
        language: "go",
        parser_support: LanguageParserSupport::Native,
    },
    LanguageSpec {
        language: "objective-c",
        parser_support: LanguageParserSupport::Native,
    },
    LanguageSpec {
        language: "zig",
        parser_support: LanguageParserSupport::Native,
    },
    LanguageSpec {
        language: "c",
        parser_support: LanguageParserSupport::Native,
    },
    LanguageSpec {
        language: "cpp",
        parser_support: LanguageParserSupport::Native,
    },
    LanguageSpec {
        language: "h",
        parser_support: LanguageParserSupport::Native,
    },
    LanguageSpec {
        language: "hpp",
        parser_support: LanguageParserSupport::Native,
    },
    LanguageSpec {
        language: "cargo-manifest",
        parser_support: LanguageParserSupport::Manifest,
    },
    LanguageSpec {
        language: "cargo-lock",
        parser_support: LanguageParserSupport::Manifest,
    },
    LanguageSpec {
        language: "vue",
        parser_support: LanguageParserSupport::Structural,
    },
    LanguageSpec {
        language: "markdown",
        parser_support: LanguageParserSupport::Structural,
    },
    LanguageSpec {
        language: "json",
        parser_support: LanguageParserSupport::Structural,
    },
    LanguageSpec {
        language: "yaml",
        parser_support: LanguageParserSupport::Structural,
    },
    LanguageSpec {
        language: "css",
        parser_support: LanguageParserSupport::Structural,
    },
    LanguageSpec {
        language: "html",
        parser_support: LanguageParserSupport::Structural,
    },
    LanguageSpec {
        language: "toon",
        parser_support: LanguageParserSupport::Structural,
    },
    LanguageSpec {
        language: "dockerfile",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "makefile",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "text",
        parser_support: LanguageParserSupport::Structural,
    },
    LanguageSpec {
        language: "toml",
        parser_support: LanguageParserSupport::Structural,
    },
    LanguageSpec {
        language: "xml",
        parser_support: LanguageParserSupport::Structural,
    },
    LanguageSpec {
        language: "svelte",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "astro",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "jsp",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "jsp-tag",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "gsp",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "groovy",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "protobuf",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "handlebars",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "ejs",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "pug",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "freemarker",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "mustache",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "liquid",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "erb",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "sql",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "graphql",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "config",
        parser_support: LanguageParserSupport::Structural,
    },
    LanguageSpec {
        language: "ruby",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "php",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "swift",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "scala",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "shell",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "powershell",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "batch",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "r",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "perl",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "lua",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "dart",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "haskell",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "ocaml",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "fsharp",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "clojure",
        parser_support: LanguageParserSupport::Fallback,
    },
    LanguageSpec {
        language: "vim",
        parser_support: LanguageParserSupport::Fallback,
    },
];

/// Return parser coverage metadata for a detected language family.
#[must_use]
pub fn language_spec(language: &str) -> Option<&'static LanguageSpec> {
    LANGUAGE_SPECS.iter().find(|spec| spec.language == language)
}

/// Detect a language or file family from an extension.
#[must_use]
pub fn detect_language(extension: Option<&str>) -> Option<String> {
    let extension = extension?.to_ascii_lowercase();
    let language = match extension.as_str() {
        ".py" | ".pyw" => "python",
        ".js" | ".jsx" | ".mjs" | ".cjs" => "javascript",
        ".ts" | ".d.ts" => "typescript",
        ".tsx" => "tsx",
        ".rs" => "rust",
        ".go" => "go",
        ".java" => "java",
        ".kt" | ".kts" => "kotlin",
        ".cs" => "csharp",
        ".m" | ".mm" => "objective-c",
        ".zig" | ".zon" => "zig",
        ".html" | ".htm" => "html",
        ".css" | ".scss" | ".sass" | ".less" | ".styl" | ".stylus" => "css",
        ".md" | ".mdx" => "markdown",
        ".json" | ".jsonc" => "json",
        ".xml" => "xml",
        ".yml" | ".yaml" => "yaml",
        ".vue" => "vue",
        ".svelte" => "svelte",
        ".astro" => "astro",
        ".jsp" | ".jspx" | ".jspf" => "jsp",
        ".tag" | ".tagx" => "jsp-tag",
        ".gsp" => "gsp",
        ".gradle" | ".groovy" => "groovy",
        ".proto" => "protobuf",
        ".hbs" | ".handlebars" => "handlebars",
        ".ejs" => "ejs",
        ".pug" => "pug",
        ".ftl" => "freemarker",
        ".mustache" => "mustache",
        ".liquid" => "liquid",
        ".erb" => "erb",
        ".sql" | ".ddl" | ".dml" | ".mysql" | ".postgresql" | ".psql" | ".sqlite" | ".mssql"
        | ".oracle" | ".ora" | ".db2" | ".proc" | ".procedure" | ".func" | ".function"
        | ".view" | ".trigger" | ".index" | ".migration" | ".seed" | ".fixture" | ".schema"
        | ".cql" | ".cypher" | ".sparql" | ".liquibase" | ".flyway" => "sql",
        ".gql" => "graphql",
        ".toml" => "toml",
        ".toon" => "toon",
        ".txt" => "text",
        ".ini" | ".cfg" | ".conf" | ".properties" | ".env" | ".gitignore" | ".dockerignore"
        | ".editorconfig" => "config",
        ".c" => "c",
        ".cpp" | ".cxx" | ".cc" => "cpp",
        ".h" => "h",
        ".hpp" | ".hxx" | ".hh" => "hpp",
        ".rb" => "ruby",
        ".php" => "php",
        ".swift" => "swift",
        ".scala" => "scala",
        ".sh" | ".bash" | ".zsh" => "shell",
        ".ps1" | ".psm1" | ".psd1" => "powershell",
        ".bat" | ".cmd" => "batch",
        ".r" => "r",
        ".pl" | ".pm" => "perl",
        ".lua" => "lua",
        ".dart" => "dart",
        ".hs" => "haskell",
        ".ml" | ".mli" => "ocaml",
        ".fs" | ".fsx" => "fsharp",
        ".clj" | ".cljs" => "clojure",
        ".vim" => "vim",
        _ => return None,
    };
    Some(language.to_string())
}

/// Detect a language or file family from a path plus extension.
#[must_use]
pub fn detect_language_for_path(path: &str, extension: Option<&str>) -> Option<String> {
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let language = match file_name {
        "Cargo.toml" => Some("cargo-manifest"),
        "Cargo.lock" => Some("cargo-lock"),
        "build.rs" => Some("rust-build-script"),
        "Dockerfile" => Some("dockerfile"),
        "Makefile" => Some("makefile"),
        _ => None,
    };
    language
        .map(ToString::to_string)
        .or_else(|| detect_language(extension))
}

#[cfg(test)]
mod tests {
    use super::{
        BROAD_SOURCE_EXTENSIONS, LanguageParserSupport, detect_language, detect_language_for_path,
        language_spec,
    };
    use std::collections::HashSet;
    use std::error::Error;
    use std::io;

    #[test]
    fn detects_every_broad_source_extension() {
        for extension in BROAD_SOURCE_EXTENSIONS {
            assert!(
                detect_language(Some(extension)).is_some(),
                "missing broad source extension support for {extension}"
            );
        }
    }

    #[test]
    fn detects_representative_broad_source_extensions() {
        assert_eq!(
            detect_language(Some(".d.ts")).as_deref(),
            Some("typescript")
        );
        assert_eq!(detect_language(Some(".pyw")).as_deref(), Some("python"));
        assert_eq!(detect_language(Some(".kts")).as_deref(), Some("kotlin"));
        assert_eq!(
            detect_language(Some(".psm1")).as_deref(),
            Some("powershell")
        );
        assert_eq!(detect_language(Some(".zon")).as_deref(), Some("zig"));
        assert_eq!(detect_language(Some(".proto")).as_deref(), Some("protobuf"));
        assert_eq!(detect_language(Some(".R")).as_deref(), Some("r"));
        assert_eq!(detect_language(Some(".ini")).as_deref(), Some("config"));
        assert_eq!(detect_language(Some(".liquibase")).as_deref(), Some("sql"));
        assert_eq!(detect_language(Some(".toon")).as_deref(), Some("toon"));
    }

    #[test]
    fn detects_cargo_files_from_filename() {
        assert_eq!(
            detect_language_for_path("Cargo.toml", Some(".toml")).as_deref(),
            Some("cargo-manifest")
        );
        assert_eq!(
            detect_language_for_path("crates/demo/build.rs", Some(".rs")).as_deref(),
            Some("rust-build-script")
        );
    }

    #[test]
    fn every_detected_language_has_parser_coverage_metadata() -> Result<(), Box<dyn Error>> {
        let mut languages = HashSet::new();
        for extension in BROAD_SOURCE_EXTENSIONS {
            let Some(language) = detect_language(Some(extension)) else {
                return Err(
                    io::Error::other(format!("missing detected language for {extension}")).into(),
                );
            };
            languages.insert(language);
        }
        for (path, extension) in [
            ("Cargo.toml", Some(".toml")),
            ("Cargo.lock", None),
            ("build.rs", Some(".rs")),
            ("Dockerfile", None),
            ("Makefile", None),
        ] {
            let Some(language) = detect_language_for_path(path, extension) else {
                return Err(
                    io::Error::other(format!("missing detected language for {path}")).into(),
                );
            };
            languages.insert(language);
        }
        for language in languages {
            if language_spec(&language).is_none() {
                return Err(io::Error::other(format!(
                    "missing parser coverage metadata for {language}"
                ))
                .into());
            }
        }
        Ok(())
    }

    #[test]
    fn representative_parser_coverage_is_explicit() {
        assert_eq!(
            language_spec("rust").map(|spec| spec.parser_support),
            Some(LanguageParserSupport::Native)
        );
        assert_eq!(
            language_spec("cargo-manifest").map(|spec| spec.parser_support),
            Some(LanguageParserSupport::Manifest)
        );
        assert_eq!(
            language_spec("vue").map(|spec| spec.parser_support),
            Some(LanguageParserSupport::Structural)
        );
        assert_eq!(
            language_spec("toml").map(|spec| spec.parser_support),
            Some(LanguageParserSupport::Structural)
        );
        assert_eq!(
            language_spec("config").map(|spec| spec.parser_support),
            Some(LanguageParserSupport::Structural)
        );
        assert_eq!(
            language_spec("text").map(|spec| spec.parser_support),
            Some(LanguageParserSupport::Structural)
        );
        assert_eq!(
            language_spec("xml").map(|spec| spec.parser_support),
            Some(LanguageParserSupport::Structural)
        );
        assert_eq!(
            language_spec("ruby").map(|spec| spec.parser_support),
            Some(LanguageParserSupport::Fallback)
        );
    }
}
