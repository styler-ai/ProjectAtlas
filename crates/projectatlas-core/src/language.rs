//! Language detection and public parser-coverage compatibility surface.

pub use crate::language_detection_registry::{LanguageParserSupport, LanguageSpec};

/// Ordered scanner-visible source extensions.
pub const BROAD_SOURCE_EXTENSIONS: &[&str] =
    crate::language_detection_registry::SCANNER_SOURCE_EXTENSIONS;

/// Ordered public language-family parser coverage.
pub const LANGUAGE_SPECS: &[LanguageSpec] =
    crate::language_detection_registry::CURRENT_LANGUAGE_SPECS;

/// Return parser coverage metadata for a detected language family.
#[must_use]
pub fn language_spec(language: &str) -> Option<&'static LanguageSpec> {
    LANGUAGE_SPECS.iter().find(|spec| spec.language == language)
}

/// Detect a language or file family from an extension.
#[must_use]
pub fn detect_language(extension: Option<&str>) -> Option<String> {
    crate::language_detection_registry::detect_extension(extension?).map(ToString::to_string)
}

/// Detect a language or file family from a path plus extension.
#[must_use]
pub fn detect_language_for_path(path: &str, extension: Option<&str>) -> Option<String> {
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    crate::language_detection_registry::detect_exact_filename(file_name)
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
