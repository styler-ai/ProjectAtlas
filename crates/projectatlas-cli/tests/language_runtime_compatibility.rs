//! Freeze the accepted 0.3.26 language runtime semantics against current typed owners.

use projectatlas_core::language::{
    LANGUAGE_CAPABILITIES, LanguageCapability, LanguageParserSupport, StructuralSummaryOwner,
    SymbolParserOwner, detect_language_for_path, normalized_language_extension,
};
use projectatlas_symbols::{extract_symbol_graph, has_specialized_parser};
use serde::Deserialize;
use std::error::Error;
use std::path::Path;

const RELEASE: &str = "v0.3.26";
const CORRECTION: &str = "intentional-correction";
const SUPPLIED_LANGUAGE_MODES: [&str; 8] = [
    "absent",
    "cargo-manifest",
    "cargo-lock",
    "vue",
    "powershell",
    "built-in",
    "fallback",
    "unknown",
];

#[derive(Debug, Deserialize)]
struct RuntimeSemanticsFixture {
    schema_version: u32,
    baseline_release: String,
    adapter_precedence_disposition: String,
    adapter_precedence_rationale: String,
    extension_normalization_disposition: String,
    extension_normalization_rationale: String,
    language_pipelines: Vec<LanguagePipeline>,
    semantic_witnesses: Vec<SemanticWitness>,
    specialized_parsers: Vec<SpecializedParser>,
    adapter_precedence: Vec<AdapterPrecedence>,
    negative_detection: Vec<DetectionCase>,
    extension_normalization: Vec<NormalizationCase>,
    cargo_routing_corrections: Vec<CargoCorrection>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct LanguagePipeline {
    language: String,
    support: String,
    summary_adapter: String,
    symbol_adapter: String,
}

#[derive(Debug, Deserialize)]
struct SemanticWitness {
    language: String,
    path: String,
    base_adapter: String,
    source: String,
    symbol_kind: String,
    symbol_name: String,
    symbol_detail: String,
    symbol_parent: String,
}

#[derive(Debug, Deserialize)]
struct SpecializedParser {
    language: String,
    package: String,
    source: String,
    symbol_kind: String,
    symbol_name: String,
}

#[derive(Debug, Deserialize)]
struct AdapterPrecedence {
    path_class: String,
    path: String,
    absent: String,
    cargo_manifest: String,
    cargo_lock: String,
    vue: String,
    powershell: String,
    built_in: String,
    fallback: String,
    unknown: String,
}

impl AdapterPrecedence {
    fn expectation(&self, mode: &str) -> &str {
        match mode {
            "absent" => &self.absent,
            "cargo-manifest" => &self.cargo_manifest,
            "cargo-lock" => &self.cargo_lock,
            "vue" => &self.vue,
            "powershell" => &self.powershell,
            "built-in" => &self.built_in,
            "fallback" => &self.fallback,
            "unknown" => &self.unknown,
            _ => unreachable!("closed supplied-language mode"),
        }
    }
}

#[derive(Debug, Deserialize)]
struct DetectionCase {
    path: String,
    extension: String,
    language: String,
}

#[derive(Debug, Deserialize)]
struct NormalizationCase {
    path: String,
    extension: String,
}

#[derive(Debug, Deserialize)]
struct CargoCorrection {
    case_id: String,
    path: String,
    supplied_language: String,
    historical_symbol_candidate: bool,
    accepted_symbol_candidate: bool,
    historical_parser_kind: String,
    accepted_parser_kind: String,
    disposition: String,
    rationale: String,
}

fn core_pipeline(capability: &LanguageCapability) -> LanguagePipeline {
    LanguagePipeline {
        language: capability.id.to_string(),
        support: match capability.parser_support {
            LanguageParserSupport::Native => "native",
            LanguageParserSupport::Manifest => "manifest",
            LanguageParserSupport::Structural => "structural",
            LanguageParserSupport::Fallback => "fallback",
        }
        .to_string(),
        summary_adapter: match capability.structural_summary {
            Some(StructuralSummaryOwner::Markdown) => "markdown",
            Some(StructuralSummaryOwner::Json) => "json",
            Some(StructuralSummaryOwner::Yaml) => "yaml",
            Some(StructuralSummaryOwner::Toml) => "toml",
            Some(StructuralSummaryOwner::Xml) => "xml",
            Some(StructuralSummaryOwner::Css) => "css",
            Some(StructuralSummaryOwner::Html) => "html",
            Some(StructuralSummaryOwner::Toon) => "toon",
            Some(StructuralSummaryOwner::PowerShell) => "powershell",
            Some(StructuralSummaryOwner::ConfigText) => "config-text",
            None => "none",
        }
        .to_string(),
        symbol_adapter: match capability.symbol_parser {
            SymbolParserOwner::TreeSitter(_) => "tree-sitter",
            SymbolParserOwner::CargoManifest => "manifest",
            SymbolParserOwner::Vue => "vue-structural",
            SymbolParserOwner::PowerShell => "powershell-structural",
            SymbolParserOwner::Markdown => "markdown-structural",
            SymbolParserOwner::Fallback => "fallback",
            SymbolParserOwner::Unavailable => "none",
        }
        .to_string(),
    }
}

fn supplied_language(mode: &str) -> Option<&str> {
    match mode {
        "absent" => None,
        "cargo-manifest" => Some("cargo-manifest"),
        "cargo-lock" => Some("cargo-lock"),
        "vue" => Some("vue"),
        "powershell" => Some("powershell"),
        "built-in" => Some("rust"),
        "fallback" => Some("ruby"),
        "unknown" => Some("unknown-language"),
        _ => unreachable!("closed supplied-language mode"),
    }
}

fn source_for_adapter(adapter: &str) -> &'static str {
    match adapter {
        "cargo-manifest" => "[package]\nname = \"atlas\"\n",
        "cargo-lock" => "[[package]]\nname = \"atlas\"\nversion = \"1.0.0\"\n",
        "vue" => "const count = ref(0)\n",
        "powershell" => "function Get-Atlas { return 1 }\n",
        "built-in" => "fn run() {}\n",
        "fallback" => "function fallbackOnly() {}\n",
        _ => unreachable!("closed adapter expectation"),
    }
}

fn assert_adapter(path: &str, language: Option<&str>, expected: &str) {
    let graph = extract_symbol_graph(path, language, source_for_adapter(expected));
    let matches = match expected {
        "cargo-manifest" => {
            graph.parser.to_string() == "manifest"
                && graph
                    .symbols
                    .iter()
                    .any(|symbol| symbol.kind.to_string() == "package")
        }
        "cargo-lock" => {
            graph.parser.to_string() == "manifest"
                && graph
                    .symbols
                    .iter()
                    .any(|symbol| symbol.kind.to_string() == "dependency")
        }
        "vue" | "powershell" => graph.parser.to_string() == "structural",
        "built-in" => graph.parser.to_string() == "tree-sitter",
        "fallback" => graph.parser.to_string() == "fallback",
        _ => false,
    };
    assert!(
        matches,
        "adapter mismatch for path {path:?}, language {language:?}, expected {expected:?}: {graph:?}"
    );
}

#[test]
fn frozen_release_language_runtime_semantics_match_current_projection() -> Result<(), Box<dyn Error>>
{
    let fixture: RuntimeSemanticsFixture = toon_format::decode_default(include_str!(
        "../../../fixtures/languages/v0.3.26-runtime-semantics.toon"
    ))?;
    assert_runtime_semantics(&fixture);
    Ok(())
}

fn assert_runtime_semantics(fixture: &RuntimeSemanticsFixture) {
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.baseline_release, RELEASE);
    assert_eq!(fixture.adapter_precedence_disposition, CORRECTION);
    assert!(!fixture.adapter_precedence_rationale.is_empty());
    assert_eq!(fixture.extension_normalization_disposition, CORRECTION);
    assert!(!fixture.extension_normalization_rationale.is_empty());

    let pipelines = LANGUAGE_CAPABILITIES
        .iter()
        .filter(|capability| capability.optional_pack.is_none())
        .map(core_pipeline)
        .collect::<Vec<_>>();
    let mut historical_pipelines = pipelines.clone();
    if let Some(markdown) = historical_pipelines
        .iter_mut()
        .find(|pipeline| pipeline.language == "markdown")
    {
        markdown.symbol_adapter = "none".to_string();
    }
    assert_eq!(historical_pipelines, fixture.language_pipelines);
    assert!(pipelines.iter().any(|pipeline| {
        pipeline.language == "markdown" && pipeline.symbol_adapter == "markdown-structural"
    }));
    let markdown = extract_symbol_graph("docs/guide.md", Some("markdown"), "# Guide\n");
    assert!(
        markdown
            .symbols
            .iter()
            .any(|symbol| { symbol.kind.to_string() == "heading" && symbol.name == "Guide" })
    );

    for row in &fixture.semantic_witnesses {
        let capability = projectatlas_core::language::language_capability(&row.language);
        assert!(
            capability.is_some(),
            "missing augmenter language {:?}",
            row.language
        );
        let Some(capability) = capability else {
            return;
        };
        assert_eq!(core_pipeline(capability).symbol_adapter, row.base_adapter);
        let graph = extract_symbol_graph(&row.path, Some(&row.language), &row.source);
        assert_eq!(graph.parser.to_string(), row.base_adapter);
        let witnesses = graph
            .symbols
            .iter()
            .filter(|symbol| {
                symbol.kind.to_string() == row.symbol_kind
                    && symbol.name == row.symbol_name
                    && (row.symbol_detail.is_empty()
                        || symbol.detail.as_deref() == Some(row.symbol_detail.as_str()))
                    && (row.symbol_parent.is_empty()
                        || symbol.parent.as_deref() == Some(row.symbol_parent.as_str()))
            })
            .count();
        assert_eq!(
            witnesses, 1,
            "language-specific semantics for {:?} did not produce one normalized witness: {:?}",
            row.language, graph.symbols
        );
    }

    for row in &fixture.specialized_parsers {
        let capability = projectatlas_core::language::language_capability(&row.language);
        assert!(
            capability.is_some(),
            "missing specialized language {:?}",
            row.language
        );
        let Some(capability) = capability else {
            return;
        };
        assert!(
            matches!(capability.symbol_parser, SymbolParserOwner::TreeSitter(_)),
            "specialized language {:?} has no grammar",
            row.language
        );
        let SymbolParserOwner::TreeSitter(grammar) = capability.symbol_parser else {
            return;
        };
        assert_eq!(grammar.package(), row.package);
        assert!(has_specialized_parser(&row.language));
        let path = format!("fixture.{}", row.language);
        let graph = extract_symbol_graph(&path, Some(&row.language), &row.source);
        assert_eq!(graph.parser.to_string(), "tree-sitter");
        assert!(
            graph.symbols.iter().any(|symbol| {
                symbol.kind.to_string() == row.symbol_kind && symbol.name == row.symbol_name
            }),
            "specialized package {:?} did not produce its witness: {:?}",
            row.package,
            graph.symbols
        );
    }

    for row in &fixture.adapter_precedence {
        assert!(!row.path_class.is_empty());
        for mode in SUPPLIED_LANGUAGE_MODES {
            assert_adapter(&row.path, supplied_language(mode), row.expectation(mode));
        }
    }

    for row in &fixture.negative_detection {
        let extension = (!row.extension.is_empty()).then_some(row.extension.as_str());
        assert_eq!(
            detect_language_for_path(&row.path, extension).as_deref(),
            (!row.language.is_empty()).then_some(row.language.as_str()),
            "negative detector case {:?}",
            row.path
        );
    }
    for row in &fixture.extension_normalization {
        assert_eq!(
            normalized_language_extension(Path::new(&row.path)).as_deref(),
            (!row.extension.is_empty()).then_some(row.extension.as_str()),
            "extension normalization case {:?}",
            row.path
        );
    }
    for row in &fixture.cargo_routing_corrections {
        assert_eq!(row.disposition, CORRECTION, "{}", row.case_id);
        assert!(!row.rationale.is_empty(), "{}", row.case_id);
        assert_ne!(
            (row.historical_symbol_candidate, &row.historical_parser_kind),
            (row.accepted_symbol_candidate, &row.accepted_parser_kind),
            "{} is not an explicit correction",
            row.case_id
        );
        let language =
            (!row.supplied_language.is_empty()).then_some(row.supplied_language.as_str());
        let graph = extract_symbol_graph(
            &row.path,
            language,
            "[[package]]\nname = \"near-miss\"\nversion = \"1.0.0\"\n",
        );
        assert_eq!(
            graph.parser.to_string(),
            row.accepted_parser_kind,
            "{}",
            row.case_id
        );
        assert_eq!(
            !graph.symbols.is_empty(),
            row.accepted_symbol_candidate,
            "{}",
            row.case_id
        );
    }
}
