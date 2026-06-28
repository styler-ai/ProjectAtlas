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
    use super::{BROAD_SOURCE_EXTENSIONS, detect_language, detect_language_for_path};

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
}
