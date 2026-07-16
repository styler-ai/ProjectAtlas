//! Focused unit coverage for language-registry validation, generation, and filesystem safety.

use super::*;
use serde_json::json;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fmt::Debug;
use std::process::Command;
use std::time::{Duration, Instant};

const LOCK: &[u8] = include_bytes!("../../../registry/language-registry.json");
const ACCEPTED: &[u8] =
    include_bytes!("../../../docs/benchmarks/projectatlas-v0.4-capability-registry.json");
const HISTORICAL: &[u8] =
    include_bytes!("../../../fixtures/languages/projectatlas-v0.3.26-runtime-contract.toon");

fn fixed_inputs() -> FixedInputBytes<'static> {
    FixedInputBytes {
        accepted_capability_registry: ACCEPTED,
        historical_runtime_contract: HISTORICAL,
    }
}

fn generated() -> Result<GeneratedArtifacts, LanguageRegistryError> {
    validate_and_generate(LOCK, &fixed_inputs())
}

fn decoded_contracts() -> Result<
    (
        LanguageRegistryLock,
        AcceptedTargetContract,
        HistoricalRuntimeContract,
    ),
    Box<dyn Error>,
> {
    let lock = serde_json::from_slice::<LanguageRegistryLock>(LOCK)?;
    let accepted_source = serde_json::from_slice::<AcceptedCapabilityRegistry>(ACCEPTED)?;
    let accepted = materialize_accepted_target(accepted_source)?;
    let historical_text = std::str::from_utf8(HISTORICAL)?;
    let historical_value = toon_format::decode_default::<serde_json::Value>(historical_text)?;
    let historical = serde_json::from_value::<HistoricalRuntimeContract>(historical_value)?;
    Ok((lock, accepted, historical))
}

fn validate_accepted_mutation(
    mutate: impl FnOnce(&mut serde_json::Value) -> Result<(), Box<dyn Error>>,
) -> Result<Result<GeneratedArtifacts, LanguageRegistryError>, Box<dyn Error>> {
    let mut accepted: serde_json::Value = serde_json::from_slice(ACCEPTED)?;
    mutate(&mut accepted)?;
    let accepted_bytes = serde_json::to_vec(&accepted)?;

    let mut lock: serde_json::Value = serde_json::from_slice(LOCK)?;
    lock["accepted_target"]["raw_sha256"] = json!(sha256_hex(&accepted_bytes));
    let lock_bytes = serde_json::to_vec(&lock)?;
    let fixed = FixedInputBytes {
        accepted_capability_registry: &accepted_bytes,
        historical_runtime_contract: HISTORICAL,
    };
    Ok(validate_and_generate(&lock_bytes, &fixed))
}

fn validate_historical_schema_mutation(
    schema_version: u32,
) -> Result<Result<GeneratedArtifacts, LanguageRegistryError>, Box<dyn Error>> {
    let historical = std::str::from_utf8(HISTORICAL)?.replacen(
        &format!("schema_version: {HISTORICAL_RUNTIME_CONTRACT_SCHEMA_VERSION}"),
        &format!("schema_version: {schema_version}"),
        1,
    );
    let historical_bytes = historical.as_bytes();
    let digest = sha256_hex(historical_bytes);

    let mut lock: serde_json::Value = serde_json::from_slice(LOCK)?;
    lock["historical_contract"]["raw_sha256"] = json!(digest);
    let evidence = lock["evidence"]
        .as_array_mut()
        .and_then(|rows| {
            rows.iter_mut()
                .find(|row| row["evidence_id"] == "evidence.historical-runtime-contract")
        })
        .ok_or_else(|| io::Error::other("historical runtime evidence row is absent"))?;
    evidence["digest_sha256"] = json!(digest);
    let lock_bytes = serde_json::to_vec(&lock)?;
    let fixed = FixedInputBytes {
        accepted_capability_registry: ACCEPTED,
        historical_runtime_contract: historical_bytes,
    };
    Ok(validate_and_generate(&lock_bytes, &fixed))
}

fn accepted_capability_mut<'a>(
    accepted: &'a mut serde_json::Value,
    capability_id: &str,
) -> Result<&'a mut serde_json::Value, Box<dyn Error>> {
    accepted["capabilities"]
        .as_array_mut()
        .and_then(|capabilities| {
            capabilities
                .iter_mut()
                .find(|capability| capability["capability_id"] == capability_id)
        })
        .ok_or_else(|| {
            io::Error::other(format!(
                "accepted capability fixture {capability_id:?} is absent"
            ))
            .into()
        })
}

fn require(condition: bool, message: impl Into<String>) -> Result<(), Box<dyn Error>> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message.into()).into())
    }
}

fn require_equal<T: Debug + PartialEq + ?Sized>(
    actual: &T,
    expected: &T,
    context: &str,
) -> Result<(), Box<dyn Error>> {
    if actual == expected {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{context}: expected {expected:?}, found {actual:?}"
        ))
        .into())
    }
}

fn require_validation_fragments(
    result: Result<(), LanguageRegistryError>,
    context: &str,
    fragments: &[&str],
) -> Result<(), Box<dyn Error>> {
    let message = match result {
        Err(LanguageRegistryError::Validation(message)) => message,
        Err(other) => {
            return Err(io::Error::other(format!(
                "{context} returned the wrong error class: {other}"
            ))
            .into());
        }
        Ok(()) => {
            return Err(io::Error::other(format!("{context} was accepted")).into());
        }
    };
    for fragment in fragments {
        require(
            message.contains(fragment),
            format!("{context} diagnostic {message:?} omits {fragment:?}"),
        )?;
    }
    Ok(())
}

fn json_rows<'a>(
    value: &'a serde_json::Value,
    field: &str,
) -> Result<&'a [serde_json::Value], Box<dyn Error>> {
    value
        .get(field)
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| io::Error::other(format!("generated JSON omits array {field:?}")).into())
}

fn json_row_ids(rows: &[serde_json::Value], field: &str) -> Result<Vec<String>, Box<dyn Error>> {
    rows.iter()
        .map(|row| {
            row.get(field)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| {
                    io::Error::other(format!("generated row omits string field {field:?}")).into()
                })
        })
        .collect()
}

fn seed_inputs(root: &Path) -> Result<(), Box<dyn Error>> {
    for (relative, bytes) in [
        (LOCK_PATH, LOCK),
        (ACCEPTED_TARGET_PATH, ACCEPTED),
        (HISTORICAL_CONTRACT_PATH, HISTORICAL),
    ] {
        let path = root.join(relative);
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("fixture input has no parent"))?;
        fs::create_dir_all(parent)?;
        fs::write(path, bytes)?;
    }
    for relative in [
        CORE_OUTPUT_PATH,
        SYMBOLS_OUTPUT_PATH,
        CLI_OUTPUT_PATH,
        EVIDENCE_OUTPUT_PATH,
        DOCUMENTATION_OUTPUT_PATH,
    ] {
        let parent = root
            .join(relative)
            .parent()
            .ok_or_else(|| io::Error::other("fixture output has no parent"))?
            .to_path_buf();
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

struct RepresentativeRegistryWitness {
    lock: LanguageRegistryLock,
    content_rule: DetectionRule,
    parser_abi: ParserAbi,
    asset_value: serde_json::Value,
    embedded_value: serde_json::Value,
    mode_overrides: AcceptedModeOverrides,
    parser_overrides: AcceptedParserOverrides,
}

fn representative_registry_witness(
    base: &LanguageRegistryLock,
) -> Result<RepresentativeRegistryWitness, Box<dyn Error>> {
    let content_rule = serde_json::from_value::<DetectionRule>(json!({
        "layer": "content",
        "id": "detect.content.javascript-module",
        "detector_id": "content.javascript-module",
        "detector_kind": "content-signature",
        "scanner_visible": true,
        "mode_id": "mode.javascript"
    }))?;
    let abi_value = json!({
        "abi_id": "abi.tree-sitter-wasm",
        "version": 15,
        "state": "pending-pack-verification"
    });
    let parser_abi = serde_json::from_value::<ParserAbi>(abi_value.clone())?;
    let asset_value = json!({
        "asset_id": "asset.javascript-wasm",
        "path": "registry/assets/javascript.wasm",
        "pack_id": "broad-language-pack",
        "source": "https://github.com/tree-sitter/tree-sitter-javascript",
        "version": "v0.25.0",
        "abi": abi_value,
        "digest_sha256": "1111111111111111111111111111111111111111111111111111111111111111",
        "license": "MIT",
        "patches": ["registry/patches/javascript-determinism.patch"]
    });
    let parser_asset = serde_json::from_value::<ParserAsset>(asset_value.clone())?;
    let query_pack = serde_json::from_value::<QueryPack>(json!({
        "query_pack_id": "queries.javascript-wasm",
        "path": "registry/queries/javascript-wasm.scm",
        "pack_id": "broad-language-pack",
        "digest_sha256": "2222222222222222222222222222222222222222222222222222222222222222"
    }))?;
    let embedded_value = json!({
        "adapter_id": "embedded.html-javascript",
        "host_mode_id": "mode.html",
        "embedded_mode_id": "mode.javascript",
        "pack_id": "broad-language-pack",
        "query_pack_id": "queries.javascript-wasm",
        "fixture_ids": ["fixture.historical-runtime-contract"]
    });
    let embedded_adapter = serde_json::from_value::<EmbeddedAdapter>(embedded_value.clone())?;
    let semantic_provider = serde_json::from_value::<SemanticProvider>(json!({
        "provider_id": "provider.javascript",
        "pack_id": "semantic-pack",
        "mode_ids": ["mode.javascript"],
        "fixture_ids": ["fixture.historical-runtime-contract"]
    }))?;
    let mode_overrides = serde_json::from_value::<AcceptedModeOverrides>(json!({
        "alias_of": "mode.javascript",
        "detection_rule_id": "detect.javascript-module",
        "fixture_ids": ["lang.javascript-module.valid"],
        "required_claims": ["detected", "parsed", "symbols"],
        "achieved_claims": ["detected"],
        "evidence_state": "pending",
        "advertisement": "blocked-until-achieved-manifest",
        "owner": "broad-language-pack-worker",
        "required_platforms": ["windows-x86_64"]
    }))?;
    let parser_overrides = serde_json::from_value::<AcceptedParserOverrides>(json!({
        "tree_sitter_abi": "15",
        "asset_id": "asset.javascript-wasm",
        "query_pack_id": "queries.javascript-wasm",
        "evidence_state": "pending-asset-fixture-and-platform-verification",
        "advertised": false,
        "owner": "broad-language-pack-worker",
        "required_platforms": ["linux-x86_64"]
    }))?;

    let mut lock = base.clone();
    lock.detection_rules.push(content_rule.clone());
    lock.assets.push(parser_asset);
    lock.query_packs.push(query_pack);
    lock.embedded_adapters.push(embedded_adapter);
    lock.semantic_providers.push(semantic_provider);

    Ok(RepresentativeRegistryWitness {
        lock,
        content_rule,
        parser_abi,
        asset_value,
        embedded_value,
        mode_overrides,
        parser_overrides,
    })
}

fn typecheck_generated_rust(artifacts: &GeneratedArtifacts) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| std::ffi::OsString::from("rustc"));
    for (index, artifact) in artifacts
        .entries()
        .into_iter()
        .filter(|artifact| {
            Path::new(artifact.path)
                .extension()
                .is_some_and(|ext| ext == "rs")
        })
        .enumerate()
    {
        let source_path = root.path().join(format!("generated_registry_{index}.rs"));
        fs::write(&source_path, artifact.bytes)?;
        let output = Command::new(&rustc)
            .arg("--edition=2024")
            .arg("--crate-type=lib")
            .arg("--emit=metadata")
            .arg("-Dwarnings")
            .arg("-Adead-code")
            .arg("--crate-name")
            .arg(format!("generated_registry_{index}"))
            .arg("--out-dir")
            .arg(root.path())
            .arg(&source_path)
            .output()?;
        require(
            output.status.success(),
            format!(
                "generated Rust {} failed standalone typechecking with {}:\n{}{}",
                artifact.path,
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        )?;
    }
    Ok(())
}

fn expected_detection_row(rule: &DetectionRule) -> String {
    let stage = match rule {
        DetectionRule::ExactFilename { .. } => "DetectionStage::ExactFilename",
        DetectionRule::CompoundExtension { .. } => "DetectionStage::CompoundExtension",
        DetectionRule::Extension { .. } => "DetectionStage::Extension",
        DetectionRule::Content {
            detector_kind: ContentDetectionKind::Shebang,
            ..
        } => "DetectionStage::Shebang",
        DetectionRule::Content {
            detector_kind: ContentDetectionKind::ContentSignature,
            ..
        } => "DetectionStage::ContentSignature",
        DetectionRule::Content {
            detector_kind: ContentDetectionKind::ProjectContext,
            ..
        } => "DetectionStage::ProjectContext",
    };
    let lookup_case = match rule.case_policy() {
        CasePolicy::Sensitive => "DetectionCase::Sensitive",
        CasePolicy::AsciiInsensitive => "DetectionCase::AsciiInsensitive",
    };
    let path_case = match rule.path_case_policy() {
        CasePolicy::Sensitive => "DetectionCase::Sensitive",
        CasePolicy::AsciiInsensitive => "DetectionCase::AsciiInsensitive",
    };
    format!(
        "    LanguageDetectionRule {{ id: {}, stage: {stage}, pattern: {}, lookup_case: {lookup_case}, path_case: {path_case}, scanner_visible: {}, mode: {} }},",
        rust_string(rule.id().as_str()),
        rust_string(rule.pattern()),
        rule.scanner_visible(),
        rust_string(rule.mode_id().as_str())
    )
}

fn expected_mode_row(mode: &CurrentLanguageMode) -> String {
    let alias_of = mode.alias_of.as_ref().map_or_else(
        || "None".to_string(),
        |alias| format!("Some({})", rust_string(alias.as_str())),
    );
    format!(
        "    LanguageMode {{ mode_id: {}, public_mode: {}, accepted_mode_id: {}, alias_of: {alias_of}, current_pack_id: {} }},",
        rust_string(mode.mode_id.as_str()),
        rust_string(mode.public_mode.as_str()),
        rust_string(mode.accepted_mode_id.as_str()),
        rust_string(mode.current_pack_id.as_str())
    )
}

const fn expected_built_in_parser(parser: BuiltInParserId) -> &'static str {
    match parser {
        BuiltInParserId::Rust => "BuiltInParser::Rust",
        BuiltInParserId::Python => "BuiltInParser::Python",
        BuiltInParserId::Javascript => "BuiltInParser::JavaScript",
        BuiltInParserId::Typescript => "BuiltInParser::TypeScript",
        BuiltInParserId::Tsx => "BuiltInParser::Tsx",
        BuiltInParserId::Java => "BuiltInParser::Java",
        BuiltInParserId::Kotlin => "BuiltInParser::Kotlin",
        BuiltInParserId::Csharp => "BuiltInParser::CSharp",
        BuiltInParserId::Go => "BuiltInParser::Go",
        BuiltInParserId::ObjectiveC => "BuiltInParser::ObjectiveC",
        BuiltInParserId::Zig => "BuiltInParser::Zig",
        BuiltInParserId::C => "BuiltInParser::C",
        BuiltInParserId::Cpp => "BuiltInParser::Cpp",
    }
}

const fn expected_augmenter(augmenter: AugmenterId) -> &'static str {
    match augmenter {
        AugmenterId::Kotlin => "SymbolAugmenter::Kotlin",
        AugmenterId::GradleKotlin => "SymbolAugmenter::GradleKotlin",
        AugmenterId::ObjectiveC => "SymbolAugmenter::ObjectiveC",
        AugmenterId::Zig => "SymbolAugmenter::Zig",
        AugmenterId::GradleGroovy => "SymbolAugmenter::GradleGroovy",
    }
}

fn expected_symbol_route_row(mode: &CurrentLanguageMode) -> String {
    let public_mode = rust_string(mode.public_mode.as_str());
    match &mode.symbols {
        SymbolPipeline::Skip => format!(
            "    LanguageSymbolRoute {{ public_mode: {public_mode}, route: SymbolRoute::Skip }},"
        ),
        SymbolPipeline::BuiltIn { parser, augmenters } => {
            let augmenters = augmenters
                .iter()
                .map(|augmenter| expected_augmenter(*augmenter))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "    LanguageSymbolRoute {{ public_mode: {public_mode}, route: SymbolRoute::BuiltIn {{ parser: {}, augmenters: &[{augmenters}] }} }},",
                expected_built_in_parser(*parser)
            )
        }
        SymbolPipeline::Manifest { adapter } => {
            let adapter = match adapter {
                ManifestAdapterId::CargoManifest => "ManifestAdapter::CargoManifest",
                ManifestAdapterId::CargoLock => "ManifestAdapter::CargoLock",
            };
            format!(
                "    LanguageSymbolRoute {{ public_mode: {public_mode}, route: SymbolRoute::Manifest({adapter}) }},"
            )
        }
        SymbolPipeline::Structural { adapter } => {
            let adapter = match adapter {
                SymbolAdapterId::Vue => "StructuralAdapter::Vue",
                SymbolAdapterId::Powershell => "StructuralAdapter::PowerShell",
            };
            format!(
                "    LanguageSymbolRoute {{ public_mode: {public_mode}, route: SymbolRoute::Structural({adapter}) }},"
            )
        }
        SymbolPipeline::Fallback { augmenters } => {
            let augmenters = augmenters
                .iter()
                .map(|augmenter| expected_augmenter(*augmenter))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "    LanguageSymbolRoute {{ public_mode: {public_mode}, route: SymbolRoute::Fallback {{ augmenters: &[{augmenters}] }} }},"
            )
        }
    }
}

fn expected_parser_component_row(component: &ParserComponent) -> String {
    let asset_id = component.asset_id.as_ref().map_or_else(
        || "None".to_string(),
        |asset| format!("Some({})", rust_string(asset.as_str())),
    );
    let query_pack_id = component.query_pack_id.as_ref().map_or_else(
        || "None".to_string(),
        |query| format!("Some({})", rust_string(query.as_str())),
    );
    let fixture_ids = component
        .fixture_ids
        .iter()
        .map(|fixture| rust_string(fixture.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    let evidence_ids = component
        .provenance_evidence_ids
        .iter()
        .map(|evidence| rust_string(evidence.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    let implementation = match component.implementation {
        ParserImplementation::CompiledTreeSitter => "ParserImplementation::CompiledTreeSitter",
    };
    let abi_state = match component.abi.state {
        AbiState::CurrentCompiledContract => "ParserAbiState::CurrentCompiledContract",
        AbiState::PendingPackVerification => "ParserAbiState::PendingPackVerification",
    };
    format!(
        "    ParserComponentContract {{ parser_id: {}, built_in_parser: {}, implementation: {implementation}, current_pack_id: {}, abi_id: {}, abi_version: {}, abi_state: {abi_state}, asset_id: {asset_id}, query_pack_id: {query_pack_id}, fixture_ids: &[{fixture_ids}], provenance_evidence_ids: &[{evidence_ids}] }},",
        rust_string(component.parser_id.as_str()),
        expected_built_in_parser(component.built_in_parser),
        rust_string(component.current_pack_id.as_str()),
        rust_string(component.abi.abi_id.as_str()),
        component.abi.version
    )
}

fn expected_embedded_adapter_row(adapter: &EmbeddedAdapter) -> String {
    let query_pack_id = adapter.query_pack_id.as_ref().map_or_else(
        || "None".to_string(),
        |query| format!("Some({})", rust_string(query.as_str())),
    );
    let fixture_ids = adapter
        .fixture_ids
        .iter()
        .map(|fixture| rust_string(fixture.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "    EmbeddedLanguageAdapter {{ adapter_id: {}, host_mode_id: {}, embedded_mode_id: {}, pack_id: {}, query_pack_id: {query_pack_id}, fixture_ids: &[{fixture_ids}] }},",
        rust_string(adapter.adapter_id.as_str()),
        rust_string(adapter.host_mode_id.as_str()),
        rust_string(adapter.embedded_mode_id.as_str()),
        rust_string(adapter.pack_id.as_str())
    )
}

fn expected_query_pack_row(query: &QueryPack) -> String {
    format!(
        "    ExtractionQueryPack {{ query_pack_id: {}, path: {}, pack_id: {}, digest_sha256: {} }},",
        rust_string(query.id.as_str()),
        rust_string(query.path.as_str()),
        rust_string(query.pack_id.as_str()),
        rust_string(query.digest_sha256.as_str())
    )
}

fn expected_semantic_provider_row(provider: &SemanticProvider) -> String {
    let mode_ids = provider
        .mode_ids
        .iter()
        .map(|mode| rust_string(mode.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    let fixture_ids = provider
        .fixture_ids
        .iter()
        .map(|fixture| rust_string(fixture.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "    SemanticProviderContract {{ provider_id: {}, pack_id: {}, mode_ids: &[{mode_ids}], fixture_ids: &[{fixture_ids}] }},",
        rust_string(provider.provider_id.as_str()),
        rust_string(provider.pack_id.as_str())
    )
}

#[derive(Debug, Eq, PartialEq)]
enum NormalizedRustToken {
    GroupStart(proc_macro2::Delimiter),
    GroupEnd(proc_macro2::Delimiter),
    Ident(String),
    Punct(char),
    Literal(String),
}

fn normalized_rust_tokens(source: &str) -> Result<Vec<NormalizedRustToken>, Box<dyn Error>> {
    fn append(stream: proc_macro2::TokenStream, output: &mut Vec<NormalizedRustToken>) {
        for token in stream {
            match token {
                proc_macro2::TokenTree::Group(group) => {
                    output.push(NormalizedRustToken::GroupStart(group.delimiter()));
                    append(group.stream(), output);
                    if matches!(
                        group.delimiter(),
                        proc_macro2::Delimiter::Brace | proc_macro2::Delimiter::Bracket
                    ) && matches!(output.last(), Some(NormalizedRustToken::Punct(',')))
                    {
                        output.pop();
                    }
                    output.push(NormalizedRustToken::GroupEnd(group.delimiter()));
                }
                proc_macro2::TokenTree::Ident(ident) => {
                    output.push(NormalizedRustToken::Ident(ident.to_string()));
                }
                proc_macro2::TokenTree::Punct(punct) => {
                    output.push(NormalizedRustToken::Punct(punct.as_char()));
                }
                proc_macro2::TokenTree::Literal(literal) => {
                    output.push(NormalizedRustToken::Literal(literal.to_string()));
                }
            }
        }
    }

    let stream = source.parse::<proc_macro2::TokenStream>()?;
    let mut output = Vec::new();
    append(stream, &mut output);
    Ok(output)
}

fn contains_rust_tokens(source: &[NormalizedRustToken], expected: &[NormalizedRustToken]) -> bool {
    if expected.is_empty() {
        return false;
    }
    if source
        .windows(expected.len())
        .any(|window| window == expected)
    {
        return true;
    }
    let Some((NormalizedRustToken::Punct(','), expected)) = expected.split_last() else {
        return false;
    };
    !expected.is_empty()
        && source
            .windows(expected.len())
            .any(|window| window == expected)
}

fn count_struct_initializers(source: &str, name: &str) -> Result<usize, Box<dyn Error>> {
    let tokens = normalized_rust_tokens(source)?;
    let occurrences = tokens
        .windows(2)
        .filter(|window| {
            matches!(
                window,
                [NormalizedRustToken::Ident(actual), NormalizedRustToken::GroupStart(proc_macro2::Delimiter::Brace)]
                    if actual == name
            )
        })
        .count();
    let definitions = tokens
        .windows(3)
        .filter(|window| {
            matches!(
                window,
                [NormalizedRustToken::Ident(keyword), NormalizedRustToken::Ident(actual), NormalizedRustToken::GroupStart(proc_macro2::Delimiter::Brace)]
                    if keyword == "struct" && actual == name
            )
        })
        .count();
    occurrences.checked_sub(definitions).ok_or_else(|| {
        io::Error::other(format!("generated Rust has invalid {name} ownership")).into()
    })
}

fn require_generated_row(source: &str, expected: &str, owner: &str) -> Result<(), Box<dyn Error>> {
    let source_tokens = normalized_rust_tokens(source)?;
    let expected_tokens = normalized_rust_tokens(expected)?;
    let (closest_start, closest) = source_tokens
        .windows(expected_tokens.len())
        .enumerate()
        .map(|(start, candidate)| {
            (
                start,
                candidate
                    .iter()
                    .zip(&expected_tokens)
                    .take_while(|(actual, expected)| actual == expected)
                    .count(),
            )
        })
        .max_by_key(|(_, matching)| *matching)
        .unwrap_or_default();
    let actual_difference = source_tokens.get(closest_start + closest);
    let expected_difference = expected_tokens.get(closest);
    require(
        contains_rust_tokens(&source_tokens, &expected_tokens),
        format!(
            "{owner} omitted or changed complete row {expected}; closest token prefix {closest}/{}; actual {actual_difference:?}; expected {expected_difference:?}",
            expected_tokens.len()
        ),
    )
}

fn require_generated_row_change(
    before_source: &str,
    after_source: &str,
    before_row: &str,
    after_row: &str,
    owner: &str,
) -> Result<(), Box<dyn Error>> {
    require(
        before_row != after_row,
        format!("{owner} mutation did not change its expected row"),
    )?;
    require_generated_row(before_source, before_row, owner)?;
    require_generated_row(after_source, after_row, owner)?;
    let after_tokens = normalized_rust_tokens(after_source)?;
    let before_tokens = normalized_rust_tokens(before_row)?;
    require(
        !contains_rust_tokens(&after_tokens, &before_tokens),
        format!("{owner} retained its pre-mutation row"),
    )
}

fn require_json_projection_change(
    before: &serde_json::Value,
    after: &serde_json::Value,
    pointer: &str,
    owner: &str,
) -> Result<(), Box<dyn Error>> {
    let before_projection = before.pointer(pointer).ok_or_else(|| {
        io::Error::other(format!(
            "{owner} pre-mutation projection {pointer} is missing"
        ))
    })?;
    let after_projection = after.pointer(pointer).ok_or_else(|| {
        io::Error::other(format!(
            "{owner} post-mutation projection {pointer} is missing"
        ))
    })?;
    require(
        before_projection != after_projection,
        format!("{owner} mutation did not change logical projection {pointer}"),
    )
}

fn expected_language_policy_row(mode: &CurrentLanguageMode) -> String {
    let parser_support = match mode.parser_support {
        ParserSupport::Native => "ParserSupport::Native",
        ParserSupport::Manifest => "ParserSupport::Manifest",
        ParserSupport::Structural => "ParserSupport::Structural",
        ParserSupport::Fallback => "ParserSupport::Fallback",
    };
    let summary_adapter = match mode.summary_adapter {
        SummaryAdapterId::None => "SummaryAdapter::None",
        SummaryAdapterId::Markdown => "SummaryAdapter::Markdown",
        SummaryAdapterId::Json => "SummaryAdapter::Json",
        SummaryAdapterId::Yaml => "SummaryAdapter::Yaml",
        SummaryAdapterId::Css => "SummaryAdapter::Css",
        SummaryAdapterId::Html => "SummaryAdapter::Html",
        SummaryAdapterId::Toon => "SummaryAdapter::Toon",
        SummaryAdapterId::ConfigText => "SummaryAdapter::ConfigText",
        SummaryAdapterId::Toml => "SummaryAdapter::Toml",
        SummaryAdapterId::Xml => "SummaryAdapter::Xml",
        SummaryAdapterId::Powershell => "SummaryAdapter::PowerShell",
    };
    let symbol_route = match mode.symbols {
        SymbolPipeline::Skip => "SymbolRouteKind::Skip",
        SymbolPipeline::BuiltIn { .. } => "SymbolRouteKind::BuiltIn",
        SymbolPipeline::Manifest { .. } => "SymbolRouteKind::Manifest",
        SymbolPipeline::Structural { .. } => "SymbolRouteKind::Structural",
        SymbolPipeline::Fallback { .. } => "SymbolRouteKind::Fallback",
    };
    format!(
        "    LanguagePolicy {{ public_mode: {}, parser_support: {parser_support}, summary_adapter: {summary_adapter}, symbol_route: {symbol_route} }},",
        rust_string(mode.public_mode.as_str())
    )
}

fn expected_registry_settings_row(
    lock: &LanguageRegistryLock,
    accepted: &AcceptedTargetContract,
) -> String {
    format!(
        "pub(crate) static LANGUAGE_REGISTRY_SETTINGS: LanguageRegistrySettings = LanguageRegistrySettings {{ registry_id: {}, accepted_registry_id: {}, accepted_set_sha256: {}, accepted_advertisement: AcceptedAdvertisement::BlockedUntilAchievedManifest, current_mode_count: {}, accepted_mode_count: {}, normalized_parser_capability_count: {}, parser_component_count: {}, parser_asset_count: {}, embedded_adapter_count: {}, query_pack_count: {}, semantic_provider_count: {} }};",
        rust_string(lock.registry_id.as_str()),
        rust_string(accepted.source.registry_id.as_str()),
        rust_string(accepted.source.accepted_set_digest.as_str()),
        lock.current_modes.len(),
        accepted.modes.len(),
        accepted.parsers.len(),
        lock.parser_components.len(),
        lock.assets.len(),
        lock.embedded_adapters.len(),
        lock.query_packs.len(),
        lock.semantic_providers.len()
    )
}

fn expected_capability_tiers_row(lock: &LanguageRegistryLock) -> String {
    let tiers = lock
        .capability_tiers
        .iter()
        .map(|tier| match tier {
            CapabilityTier::Detected => "CapabilityTier::Detected",
            CapabilityTier::Parsed => "CapabilityTier::Parsed",
            CapabilityTier::Symbols => "CapabilityTier::Symbols",
            CapabilityTier::Semantic => "CapabilityTier::Semantic",
            CapabilityTier::Benchmarked => "CapabilityTier::Benchmarked",
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("pub(crate) static LANGUAGE_CAPABILITY_TIERS: &[CapabilityTier] = &[{tiers}];")
}

fn expected_pack_settings_row(pack: &RegistryPack) -> String {
    let ownership = match pack.ownership {
        PackOwnership::DefaultCore => "PackOwnership::DefaultCore",
        PackOwnership::Optional => "PackOwnership::Optional",
    };
    let runtime = match pack.runtime {
        PackRuntime::InProcess => "PackRuntime::InProcess",
        PackRuntime::SupervisedWorker => "PackRuntime::SupervisedWorker",
    };
    format!(
        "    LanguagePackSettings {{ pack_id: {}, ownership: {ownership}, runtime: {runtime} }},",
        rust_string(pack.pack_id.as_str())
    )
}

fn expected_current_mode_value(mode: &CurrentLanguageMode) -> serde_json::Value {
    json!({
        "mode_id": mode.mode_id.as_str(),
        "pack_id": mode.current_pack_id.as_str(),
        "parser_support": mode.parser_support.contract_tag(),
        "summary_adapter": mode.summary_adapter.contract_tag(),
        "symbol_route": mode.symbols.contract_tag()
    })
}

fn expected_accepted_mode_values(
    lock: &LanguageRegistryLock,
    accepted: &AcceptedTargetContract,
) -> Vec<serde_json::Value> {
    let current = lock
        .current_modes
        .iter()
        .map(|mode| (mode.accepted_mode_id.as_str(), mode))
        .collect::<BTreeMap<_, _>>();
    let mut modes = accepted.modes.iter().collect::<Vec<_>>();
    modes.sort_by_key(|mode| mode.mode_id.as_str());
    modes
        .into_iter()
        .map(|mode| {
            json!({
                "accepted_mode_id": mode.mode_id.as_str(),
                "public_mode": mode.public_mode.as_str(),
                "parser_id": mode.parser_id.as_str(),
                "future_pack_id": mode.pack_id.as_str(),
                "owner": mode.owner,
                "accepted_delivery_target": mode.accepted_delivery_target,
                "alias_of": mode.alias_of.as_ref().map(ModeId::as_str),
                "detection_rule_id": mode.detection_rule_id.as_str(),
                "fixture_ids": mode.fixture_ids,
                "required_platforms": mode.required_platforms,
                "required_claims": mode.required_claims.iter().map(|tier| tier.contract_tag()).collect::<Vec<_>>(),
                "achieved_claims": mode.achieved_claims.iter().map(|tier| tier.contract_tag()).collect::<Vec<_>>(),
                "advertised": mode.advertisement.is_advertised(),
                "evidence_state": mode.evidence_state,
                "advertisement": mode.advertisement,
                "current": current.get(mode.mode_id.as_str()).map(|current| expected_current_mode_value(current))
            })
        })
        .collect()
}

fn expected_accepted_parser_values(accepted: &AcceptedTargetContract) -> Vec<serde_json::Value> {
    let mut parsers = accepted.parsers.iter().collect::<Vec<_>>();
    parsers.sort_by_key(|parser| parser.parser_id.as_str());
    parsers
        .into_iter()
        .map(|parser| {
            json!({
                "parser_id": parser.parser_id.as_str(),
                "kind": parser.kind.contract_tag(),
                "future_pack_id": parser.pack_id.as_str(),
                "owner": parser.owner,
                "tree_sitter_abi": parser.tree_sitter_abi.as_ref().map(ParserAbiVersion::as_str),
                "asset_id": parser.asset_id.as_str(),
                "query_pack_id": parser.query_pack_id.as_str(),
                "evidence_state": parser.evidence_state,
                "advertised": parser.advertised,
                "required_platforms": parser.required_platforms,
                "normalized_modes": parser.normalized_modes
            })
        })
        .collect()
}

fn expected_crosswalk_values(accepted: &AcceptedTargetContract) -> Vec<serde_json::Value> {
    let mut rows = accepted
        .source
        .accepted_language_crosswalk
        .entries
        .iter()
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| row.accepted_name_id.as_str());
    rows.into_iter()
        .map(|row| {
            json!({
                "accepted_name_id": row.accepted_name_id.as_str(),
                "standard_name": row.standard_name,
                "dialect": row.dialect,
                "mode_id": row.mode_id.as_str(),
                "mapping": row.mapping.contract_tag()
            })
        })
        .collect()
}

fn expected_conformance_mode_values(
    lock: &LanguageRegistryLock,
    accepted: &AcceptedTargetContract,
) -> Vec<serde_json::Value> {
    let registered = lock
        .fixtures
        .iter()
        .map(|fixture| fixture.fixture_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut modes = accepted.modes.iter().collect::<Vec<_>>();
    modes.sort_by_key(|mode| mode.mode_id.as_str());
    modes
        .into_iter()
        .map(|mode| {
            let registered_fixture_ids = mode
                .fixture_ids
                .iter()
                .map(String::as_str)
                .filter(|fixture| registered.contains(fixture))
                .collect::<Vec<_>>();
            let missing_fixture_ids = mode
                .fixture_ids
                .iter()
                .map(String::as_str)
                .filter(|fixture| !registered.contains(fixture))
                .collect::<Vec<_>>();
            json!({
                "mode_id": mode.mode_id.as_str(),
                "public_mode": mode.public_mode.as_str(),
                "parser_id": mode.parser_id.as_str(),
                "required_claims": mode.required_claims.iter().map(|tier| tier.contract_tag()).collect::<Vec<_>>(),
                "achieved_claims": mode.achieved_claims.iter().map(|tier| tier.contract_tag()).collect::<Vec<_>>(),
                "fixture_ids": mode.fixture_ids,
                "registered_fixture_ids": registered_fixture_ids,
                "missing_fixture_ids": missing_fixture_ids,
                "required_platforms": mode.required_platforms,
                "evidence_state": mode.evidence_state,
                "advertised": mode.advertisement.is_advertised()
            })
        })
        .collect()
}

fn expected_documentation_mode_values(
    lock: &LanguageRegistryLock,
    accepted: &AcceptedTargetContract,
) -> Vec<serde_json::Value> {
    let current = lock
        .current_modes
        .iter()
        .map(|mode| (mode.accepted_mode_id.as_str(), mode))
        .collect::<BTreeMap<_, _>>();
    let mut modes = accepted.modes.iter().collect::<Vec<_>>();
    modes.sort_by_key(|mode| mode.mode_id.as_str());
    modes
        .into_iter()
        .map(|mode| {
            json!({
                "mode_id": mode.mode_id.as_str(),
                "public_mode": mode.public_mode.as_str(),
                "parser_id": mode.parser_id.as_str(),
                "future_pack_id": mode.pack_id.as_str(),
                "required_claims": mode.required_claims.iter().map(|tier| tier.contract_tag()).collect::<Vec<_>>(),
                "achieved_claims": mode.achieved_claims.iter().map(|tier| tier.contract_tag()).collect::<Vec<_>>(),
                "evidence_state": mode.evidence_state,
                "advertisement": mode.advertisement,
                "advertised": mode.advertisement.is_advertised(),
                "current": current.get(mode.mode_id.as_str()).map(|current| expected_current_mode_value(current))
            })
        })
        .collect()
}

fn expected_sbom_component_values(
    lock: &LanguageRegistryLock,
) -> Result<Vec<serde_json::Value>, Box<dyn Error>> {
    lock.parser_components
        .iter()
        .map(|component| {
            let asset = component
                .asset_id
                .as_ref()
                .map(|asset_id| {
                    lock.assets
                        .iter()
                        .find(|asset| asset.asset_id == *asset_id)
                        .ok_or_else(|| io::Error::other("representative SBOM asset disappeared"))
                })
                .transpose()?;
            let query_pack = component
                .query_pack_id
                .as_ref()
                .map(|query_id| {
                    lock.query_packs
                        .iter()
                        .find(|query| query.id == *query_id)
                        .ok_or_else(|| io::Error::other("representative SBOM query disappeared"))
                })
                .transpose()?;
            let fixtures = component
                .fixture_ids
                .iter()
                .map(|fixture_id| {
                    lock.fixtures
                        .iter()
                        .find(|fixture| fixture.fixture_id == *fixture_id)
                        .ok_or_else(|| io::Error::other("representative SBOM fixture disappeared"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let provenance_evidence = component
                .provenance_evidence_ids
                .iter()
                .map(|evidence_id| {
                    lock.evidence
                        .iter()
                        .find(|evidence| evidence.evidence_id == *evidence_id)
                        .ok_or_else(|| io::Error::other("representative SBOM evidence disappeared"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!({
                "parser_id": component.parser_id.as_str(),
                "built_in_parser": component.built_in_parser,
                "implementation": component.implementation,
                "pack_id": component.current_pack_id.as_str(),
                "abi_id": component.abi.abi_id.as_str(),
                "abi_version": component.abi.version,
                "abi_state": component.abi.state,
                "asset": asset,
                "query_pack": query_pack,
                "fixtures": fixtures,
                "provenance_evidence": provenance_evidence
            }))
        })
        .collect()
}

fn representative_artifacts(
    base: &LanguageRegistryLock,
    accepted: &AcceptedTargetContract,
    historical: &HistoricalRuntimeContract,
) -> Result<(LanguageRegistryLock, GeneratedArtifacts), Box<dyn Error>> {
    let representative = representative_registry_witness(base)?.lock;
    validate_registry_lock(&representative, accepted)?;
    let source_bytes = serde_json::to_vec(&representative)?;
    let source_digest = sha256_hex(&source_bytes);
    let contract_digest = registry_contract_digest(&representative, accepted, historical);
    let artifacts = render_generated_artifacts(
        &representative,
        accepted,
        historical,
        &source_digest,
        &contract_digest,
    )?;
    Ok((representative, artifacts))
}

fn render_test_lock(
    lock: &LanguageRegistryLock,
    accepted: &AcceptedTargetContract,
    historical: &HistoricalRuntimeContract,
) -> Result<GeneratedArtifacts, Box<dyn Error>> {
    let source_bytes = serde_json::to_vec(lock)?;
    let source_digest = sha256_hex(&source_bytes);
    let contract_digest = registry_contract_digest(lock, accepted, historical);
    Ok(render_generated_artifacts(
        lock,
        accepted,
        historical,
        &source_digest,
        &contract_digest,
    )?)
}

fn verify_generated_rust_rows(
    lock: &LanguageRegistryLock,
    accepted: &AcceptedTargetContract,
    artifacts: &GeneratedArtifacts,
) -> Result<(), Box<dyn Error>> {
    let core = std::str::from_utf8(&artifacts.core)?;
    require_equal(
        &count_struct_initializers(core, "LanguageDetectionRule")?,
        &lock.detection_rules.len(),
        "generated detection row count",
    )?;
    require_equal(
        &count_struct_initializers(core, "LanguageMode")?,
        &lock.current_modes.len(),
        "generated current-mode row count",
    )?;
    for rule in &lock.detection_rules {
        require_generated_row(
            core,
            &expected_detection_row(rule),
            "core detection projection",
        )?;
    }
    require_generated_row(
        core,
        "pub(crate) enum DetectionStage {\n    ExactFilename,\n    CompoundExtension,\n    Extension,\n    Shebang,\n    ContentSignature,\n    ProjectContext,\n}",
        "generated detection-stage enum lost the accepted closed order",
    )?;
    require(
        !core.contains("enum DetectionLayer")
            && !core.contains("enum ContentDetectionKind")
            && !core.contains("content_kind: Option"),
        "generated detection metadata reintroduced split or optional stage state",
    )?;
    for mode in &lock.current_modes {
        require_generated_row(core, &expected_mode_row(mode), "core mode projection")?;
    }
    let mut scanner_extension_rows = String::new();
    for rule in &lock.detection_rules {
        if rule.scanner_visible()
            && matches!(
                rule,
                DetectionRule::CompoundExtension { .. } | DetectionRule::Extension { .. }
            )
        {
            writeln!(
                &mut scanner_extension_rows,
                "    {},",
                rust_string(rule.pattern())
            )?;
        }
    }
    require_generated_row(
        core,
        &format!(
            "pub(crate) static SCANNER_SOURCE_EXTENSIONS: &[&str] = &[\n{scanner_extension_rows}];"
        ),
        "core scanner-extension projection lost exact registry order",
    )?;
    let mut language_spec_rows = String::new();
    for mode in &lock.current_modes {
        writeln!(
            &mut language_spec_rows,
            "    LanguageSpec {{ language: {}, parser_support: {} }},",
            rust_string(mode.public_mode.as_str()),
            rust_parser_support(mode.parser_support)
        )?;
    }
    require_generated_row(
        core,
        &format!(
            "pub(crate) static CURRENT_LANGUAGE_SPECS: &[LanguageSpec] = &[\n{language_spec_rows}];"
        ),
        "core language-spec projection lost exact registry order",
    )?;

    let mut extension_lookup = String::from(
        "pub(crate) fn detect_extension(extension: &str) -> Option<&'static str> {\n    let extension = extension.to_ascii_lowercase();\n    match extension.as_str() {\n",
    );
    let mut normalized_extensions = BTreeSet::new();
    let mut extension_groups = Vec::<(String, Vec<String>)>::new();
    for rule in &lock.detection_rules {
        if !matches!(
            rule,
            DetectionRule::CompoundExtension { .. } | DetectionRule::Extension { .. }
        ) {
            continue;
        }
        let normalized = match rule.case_policy() {
            CasePolicy::Sensitive => rule.pattern().to_string(),
            CasePolicy::AsciiInsensitive => rule.pattern().to_ascii_lowercase(),
        };
        if !normalized_extensions.insert(normalized.clone()) {
            continue;
        }
        let public_mode = lock
            .current_modes
            .iter()
            .find(|mode| &mode.mode_id == rule.mode_id())
            .ok_or_else(|| io::Error::other("extension lookup mode disappeared"))?;
        let public_mode = public_mode.public_mode.as_str();
        if let Some((_, patterns)) = extension_groups
            .iter_mut()
            .find(|(mode, _)| mode == public_mode)
        {
            patterns.push(normalized);
        } else {
            extension_groups.push((public_mode.to_string(), vec![normalized]));
        }
    }
    for (public_mode, patterns) in extension_groups {
        let patterns = patterns
            .iter()
            .map(|pattern| rust_string(pattern))
            .collect::<Vec<_>>()
            .join(" | ");
        writeln!(
            &mut extension_lookup,
            "        {patterns} => Some({}),",
            rust_string(&public_mode)
        )?;
    }
    extension_lookup.push_str("        _ => None,\n    }\n}");
    require_generated_row(
        core,
        &extension_lookup,
        "core normalized extension lookup diverged from registry order or routing",
    )?;

    let mut filename_lookup = String::from(
        "pub(crate) fn detect_exact_filename(file_name: &str) -> Option<&'static str> {\n    match file_name {\n",
    );
    for rule in &lock.detection_rules {
        if !matches!(rule, DetectionRule::ExactFilename { .. }) {
            continue;
        }
        let public_mode = lock
            .current_modes
            .iter()
            .find(|mode| &mode.mode_id == rule.mode_id())
            .ok_or_else(|| io::Error::other("filename lookup mode disappeared"))?;
        writeln!(
            &mut filename_lookup,
            "        {} => Some({}),",
            rust_string(rule.pattern()),
            rust_string(public_mode.public_mode.as_str())
        )?;
    }
    filename_lookup.push_str("        _ => None,\n    }\n}");
    require_generated_row(
        core,
        &filename_lookup,
        "core exact-filename lookup diverged from registry order or routing",
    )?;

    let symbols = std::str::from_utf8(&artifacts.symbols)?;
    for (label, actual, expected) in [
        (
            "symbol routes",
            count_struct_initializers(symbols, "LanguageSymbolRoute")?,
            lock.current_modes.len(),
        ),
        (
            "parser components",
            count_struct_initializers(symbols, "ParserComponentContract")?,
            lock.parser_components.len(),
        ),
        (
            "embedded adapters",
            count_struct_initializers(symbols, "EmbeddedLanguageAdapter")?,
            lock.embedded_adapters.len(),
        ),
        (
            "query packs",
            count_struct_initializers(symbols, "ExtractionQueryPack")?,
            lock.query_packs.len(),
        ),
        (
            "semantic providers",
            count_struct_initializers(symbols, "SemanticProviderContract")?,
            lock.semantic_providers.len(),
        ),
    ] {
        require_equal(&actual, &expected, label)?;
    }
    for mode in &lock.current_modes {
        require_generated_row(
            symbols,
            &expected_symbol_route_row(mode),
            "symbols route projection",
        )?;
    }
    let mut specialized_language_rows = String::new();
    for mode in &lock.current_modes {
        if matches!(mode.symbols, SymbolPipeline::BuiltIn { .. }) {
            writeln!(
                &mut specialized_language_rows,
                "    {},",
                rust_string(mode.public_mode.as_str())
            )?;
        }
    }
    require_generated_row(
        symbols,
        &format!(
            "pub(crate) static SPECIALIZED_LANGUAGES: &[&str] = &[\n{specialized_language_rows}];"
        ),
        "specialized-language projection lost exact registry order",
    )?;
    let mut symbol_route_lookup_rows = String::new();
    for (index, mode) in lock.current_modes.iter().enumerate() {
        writeln!(
            &mut symbol_route_lookup_rows,
            "        {} => Some(&CURRENT_SYMBOL_ROUTES[{index}].route),",
            rust_string(mode.public_mode.as_str())
        )?;
    }
    require_generated_row(
        symbols,
        &format!(
            "pub(crate) fn symbol_route_for_public_mode(public_mode: &str) -> Option<&'static SymbolRoute> {{\n    match public_mode {{\n{symbol_route_lookup_rows}        _ => None,\n    }}\n}}"
        ),
        "generated public-mode route lookup diverged from CURRENT_SYMBOL_ROUTES",
    )?;
    require_generated_row(
        symbols,
        "pub(crate) fn built_in_parser_for_public_mode(public_mode: &str) -> Option<BuiltInParser> {\n    match symbol_route_for_public_mode(public_mode)? {\n        SymbolRoute::BuiltIn { parser, .. } => Some(*parser),\n        SymbolRoute::Skip\n        | SymbolRoute::Manifest(_)\n        | SymbolRoute::Structural(_)\n        | SymbolRoute::Fallback { .. } => None,\n    }\n}",
        "generated parser lookup no longer derives from the owning symbol route",
    )?;
    require(
        !symbols.contains("SymbolRouteKind")
            && !symbols.contains("SymbolProcessor")
            && !symbols.contains("processor: Option"),
        "generated symbols routing reintroduced split or optional route state",
    )?;
    for component in &lock.parser_components {
        require_generated_row(
            symbols,
            &expected_parser_component_row(component),
            "parser component projection",
        )?;
    }
    let mut parser_component_rows = String::new();
    for component in &lock.parser_components {
        writeln!(
            &mut parser_component_rows,
            "        {} => {},",
            expected_built_in_parser(component.built_in_parser),
            rust_string(component.parser_id.as_str())
        )?;
    }
    require_generated_row(
        symbols,
        &format!(
            "pub(crate) const fn parser_component_id(parser: BuiltInParser) -> &'static str {{\n    match parser {{\n{parser_component_rows}    }}\n}}"
        ),
        "generated parser-component identity lookup diverged from registry ownership",
    )?;
    for adapter in &lock.embedded_adapters {
        require_generated_row(
            symbols,
            &expected_embedded_adapter_row(adapter),
            "embedded adapter projection",
        )?;
    }
    for query in &lock.query_packs {
        require_generated_row(
            symbols,
            &expected_query_pack_row(query),
            "query-pack projection",
        )?;
    }
    for provider in &lock.semantic_providers {
        require_generated_row(
            symbols,
            &expected_semantic_provider_row(provider),
            "semantic-provider projection",
        )?;
    }

    let cli = std::str::from_utf8(&artifacts.cli)?;
    require_equal(
        &count_struct_initializers(cli, "LanguagePolicy")?,
        &lock.current_modes.len(),
        "generated CLI policy row count",
    )?;
    require_equal(
        &count_struct_initializers(cli, "LanguagePackSettings")?,
        &lock.packs.len(),
        "generated CLI pack-settings row count",
    )?;
    for mode in &lock.current_modes {
        require_generated_row(
            cli,
            &expected_language_policy_row(mode),
            "CLI language-policy projection",
        )?;
    }
    let mut language_policy_lookup_rows = String::new();
    for (index, mode) in lock.current_modes.iter().enumerate() {
        writeln!(
            &mut language_policy_lookup_rows,
            "        {} => Some(&CURRENT_LANGUAGE_POLICY[{index}]),",
            rust_string(mode.public_mode.as_str())
        )?;
    }
    require_generated_row(
        cli,
        &format!(
            "pub(crate) fn language_policy_for_public_mode(\n    public_mode: &str,\n) -> Option<&'static LanguagePolicy> {{\n    match public_mode {{\n{language_policy_lookup_rows}        _ => None,\n    }}\n}}"
        ),
        "generated public-mode policy lookup diverged from CURRENT_LANGUAGE_POLICY",
    )?;
    require_generated_row(
        cli,
        &expected_registry_settings_row(lock, accepted),
        "CLI registry settings projection",
    )?;
    require_generated_row(
        cli,
        &expected_capability_tiers_row(lock),
        "CLI capability-tier projection",
    )?;
    for pack in &lock.packs {
        require_generated_row(
            cli,
            &expected_pack_settings_row(pack),
            "CLI pack-settings projection",
        )?;
    }
    Ok(())
}

fn verify_generated_json(
    lock: &LanguageRegistryLock,
    accepted: &AcceptedTargetContract,
    historical: &HistoricalRuntimeContract,
    source_lock_sha256: &str,
    registry_contract_sha256: &str,
    artifacts: &GeneratedArtifacts,
) -> Result<(), Box<dyn Error>> {
    let expected_parity_modes = expected_accepted_mode_values(lock, accepted);
    let expected_crosswalk = expected_crosswalk_values(accepted);
    let expected_state = json!({
        "schema_version": 1,
        "format": "projectatlas.language-capability-state",
        "registry_id": lock.registry_id.as_str(),
        "source_lock_sha256": source_lock_sha256,
        "registry_contract_sha256": registry_contract_sha256,
        "current": {
            "detection_rules": &lock.detection_rules,
            "modes": &lock.current_modes,
            "parser_components": &lock.parser_components,
            "packs": &lock.packs
        },
        "accepted_target": {
            "registry_id": accepted.source.registry_id.as_str(),
            "accepted_set_sha256": accepted.source.accepted_set_digest.as_str(),
            "raw_sha256": lock.accepted_target.raw_sha256.as_str(),
            "modes": accepted.modes.len(),
            "normalized_parser_capabilities": accepted.parsers.len(),
            "crosswalk_entries": accepted.source.accepted_language_crosswalk.entries.len(),
            "advertisement": accepted.source.mode_defaults.advertisement
        },
        "historical_contract": {
            "release": historical.baseline_release.as_str(),
            "commit": historical.baseline_commit.as_str(),
            "raw_sha256": lock.historical_contract.raw_sha256.as_str(),
            "language_pipelines": historical.language_pipelines.len(),
            "augmenter_routes": historical.augmenter_routes.len()
        },
        "accepted_capability_parity": {
            "complete": false,
            "modes": &expected_parity_modes,
            "parsers": expected_accepted_parser_values(accepted),
            "language_crosswalk": &expected_crosswalk,
            "capabilities": &accepted.source.capabilities,
            "claim_types": &accepted.source.claim_types,
            "relation_traceability_contract": &accepted.source.relation_traceability_contract
        },
        "settings": {
            "registry_id": lock.registry_id.as_str(),
            "registry_contract_sha256": registry_contract_sha256,
            "accepted_registry_id": accepted.source.registry_id.as_str(),
            "accepted_set_sha256": accepted.source.accepted_set_digest.as_str(),
            "accepted_advertisement": accepted.source.mode_defaults.advertisement,
            "capability_tiers": &lock.capability_tiers,
            "packs": &lock.packs,
            "current_modes": lock.current_modes.len(),
            "accepted_modes": accepted.modes.len(),
            "accepted_parser_capabilities": accepted.parsers.len(),
            "parser_components": lock.parser_components.len(),
            "parser_assets": lock.assets.len(),
            "embedded_adapters": lock.embedded_adapters.len(),
            "query_packs": lock.query_packs.len(),
            "semantic_providers": lock.semantic_providers.len()
        },
        "conformance_inventory": {
            "modes": expected_conformance_mode_values(lock, accepted),
            "fixtures": &lock.fixtures,
            "query_packs": &lock.query_packs,
            "embedded_adapters": &lock.embedded_adapters,
            "semantic_providers": &lock.semantic_providers,
            "evidence": &lock.evidence
        },
        "sbom_inputs": {
            "parser_components": expected_sbom_component_values(lock)?,
            "parser_assets": &lock.assets,
            "query_packs": &lock.query_packs
        }
    });
    let state = serde_json::from_slice::<serde_json::Value>(&artifacts.evidence)?;
    require_equal(
        &state,
        &expected_state,
        "complete generated capability-state reconciliation",
    )?;

    let expected_documentation = json!({
        "schema_version": 1,
        "format": "projectatlas.language-capabilities",
        "registry_id": lock.registry_id.as_str(),
        "source_lock_sha256": source_lock_sha256,
        "registry_contract_sha256": registry_contract_sha256,
        "accepted_registry_id": accepted.source.registry_id.as_str(),
        "accepted_set_sha256": accepted.source.accepted_set_digest.as_str(),
        "parity_complete": false,
        "support": {
            "modes": expected_documentation_mode_values(lock, accepted),
            "language_crosswalk": expected_crosswalk
        }
    });
    let documentation = serde_json::from_slice::<serde_json::Value>(&artifacts.documentation)?;
    require_equal(
        &documentation,
        &expected_documentation,
        "complete generated documentation-support reconciliation",
    )
}

#[test]
fn task_arri_ut_arri_5_1() -> Result<(), Box<dyn Error>> {
    let (lock, accepted, historical) = decoded_contracts()?;
    require_equal(
        &lock.schema_version,
        &LANGUAGE_REGISTRY_SCHEMA_VERSION,
        "language registry schema version",
    )?;
    require_equal(
        &accepted.source.schema_version,
        &ACCEPTED_CAPABILITY_SCHEMA_VERSION,
        "accepted capability schema version",
    )?;
    require_equal(
        &historical.schema_version,
        &HISTORICAL_RUNTIME_CONTRACT_SCHEMA_VERSION,
        "historical runtime contract schema version",
    )?;
    require_equal(
        &REGISTRY_DIGEST_VERSION,
        &1_u64,
        "semantic digest encoding version",
    )?;
    require_equal(
        lock.capability_tiers.as_slice(),
        &[
            CapabilityTier::Detected,
            CapabilityTier::Parsed,
            CapabilityTier::Symbols,
            CapabilityTier::Semantic,
            CapabilityTier::Benchmarked,
        ],
        "closed capability tier order",
    )?;
    require_equal(
        accepted.source.detection_policy.precedence.as_slice(),
        &ACCEPTED_DETECTION_PRECEDENCE,
        "closed accepted detection precedence",
    )?;

    for (label, present) in [
        (
            "exact-filename",
            lock.detection_rules
                .iter()
                .any(|rule| matches!(rule, DetectionRule::ExactFilename { .. })),
        ),
        (
            "compound-extension",
            lock.detection_rules
                .iter()
                .any(|rule| matches!(rule, DetectionRule::CompoundExtension { .. })),
        ),
        (
            "extension",
            lock.detection_rules
                .iter()
                .any(|rule| matches!(rule, DetectionRule::Extension { .. })),
        ),
    ] {
        require(present, format!("real lock omits {label} detection rules"))?;
    }

    let RepresentativeRegistryWitness {
        lock: representative,
        content_rule,
        parser_abi,
        asset_value,
        embedded_value,
        mode_overrides,
        parser_overrides,
    } = representative_registry_witness(&lock)?;
    require(
        matches!(
            &content_rule,
            DetectionRule::Content {
                detector_kind: ContentDetectionKind::ContentSignature,
                ..
            }
        ),
        "content detection rule did not retain its closed detector kind",
    )?;

    require(
        !mode_overrides.is_empty()
            && mode_overrides.required_claims.as_deref()
                == Some(&[
                    CapabilityTier::Detected,
                    CapabilityTier::Parsed,
                    CapabilityTier::Symbols,
                ])
            && mode_overrides.evidence_state == Some(AcceptedEvidenceState::Pending)
            && mode_overrides.advertisement
                == Some(AcceptedModeAdvertisement::BlockedUntilAchievedManifest),
        "accepted mode tier and alias overrides were not typed",
    )?;
    require(
        !parser_overrides.is_empty()
            && parser_overrides
                .tree_sitter_abi
                .as_ref()
                .is_some_and(|version| version.as_str() == "15")
            && parser_overrides.asset_id.is_some()
            && parser_overrides.query_pack_id.is_some()
            && parser_overrides.evidence_state
                == Some(AcceptedParserEvidenceState::PendingAssetFixtureAndPlatformVerification),
        "accepted parser ABI, asset, and query overrides were not typed",
    )?;

    validate_registry_lock(&representative, &accepted)?;

    let baseline_digest = registry_contract_digest(&lock, &accepted, &historical);
    let representative_digest = registry_contract_digest(&representative, &accepted, &historical);
    require(
        baseline_digest != representative_digest,
        "representative 5.1 fields did not participate in the semantic digest",
    )?;
    require_equal(
        &representative_digest,
        &registry_contract_digest(&representative, &accepted, &historical),
        "semantic digest determinism",
    )?;

    let mut changed_content = representative.clone();
    if let Some(DetectionRule::Content { detector_id, .. }) =
        changed_content.detection_rules.last_mut()
    {
        detector_id.0.push_str("-variant");
    }
    require(
        registry_contract_digest(&changed_content, &accepted, &historical) != representative_digest,
        "content detector identity was absent from the semantic digest",
    )?;
    let mut changed_asset_source = representative.clone();
    if let Some(asset) = changed_asset_source.assets.last_mut() {
        asset.source.0.push_str("/mirror");
    }
    require(
        registry_contract_digest(&changed_asset_source, &accepted, &historical)
            != representative_digest,
        "asset source was absent from the semantic digest",
    )?;
    let mut changed_asset_version = representative.clone();
    if let Some(asset) = changed_asset_version.assets.last_mut() {
        asset.version.0.push_str("-reviewed");
    }
    require(
        registry_contract_digest(&changed_asset_version, &accepted, &historical)
            != representative_digest,
        "asset version was absent from the semantic digest",
    )?;
    let mut changed_asset_abi_version = representative.clone();
    if let Some(asset) = changed_asset_abi_version.assets.last_mut() {
        asset.abi.version += 1;
    }
    require(
        registry_contract_digest(&changed_asset_abi_version, &accepted, &historical)
            != representative_digest,
        "asset ABI version was absent from the semantic digest",
    )?;
    let mut changed_embedded = representative.clone();
    if let Some(adapter) = changed_embedded.embedded_adapters.last_mut() {
        std::mem::swap(&mut adapter.host_mode_id, &mut adapter.embedded_mode_id);
    }
    require(
        registry_contract_digest(&changed_embedded, &accepted, &historical)
            != representative_digest,
        "embedded adapter mapping was absent from the semantic digest",
    )?;
    let mut changed_tiers = representative.clone();
    changed_tiers.capability_tiers.swap(0, 1);
    require(
        registry_contract_digest(&changed_tiers, &accepted, &historical) != representative_digest,
        "capability tier order was absent from the semantic digest",
    )?;

    let mut asset_owner_mismatch = representative.clone();
    let asset_id = asset_owner_mismatch
        .assets
        .last()
        .ok_or_else(|| io::Error::other("representative parser asset is absent"))?
        .asset_id
        .clone();
    asset_owner_mismatch.parser_components[0].asset_id = Some(asset_id);
    require(
        validate_registry_lock(&asset_owner_mismatch, &accepted).is_err(),
        "cross-pack parser asset ownership was accepted",
    )?;
    let mut query_owner_mismatch = representative.clone();
    let query_id = query_owner_mismatch
        .query_packs
        .last()
        .ok_or_else(|| io::Error::other("representative query pack is absent"))?
        .id
        .clone();
    query_owner_mismatch.parser_components[0].query_pack_id = Some(query_id);
    require(
        validate_registry_lock(&query_owner_mismatch, &accepted).is_err(),
        "cross-pack extraction query ownership was accepted",
    )?;
    let mut provider_owner_mismatch = representative;
    provider_owner_mismatch.semantic_providers[0].pack_id =
        serde_json::from_value(json!("default-core"))?;
    require(
        validate_registry_lock(&provider_owner_mismatch, &accepted).is_err(),
        "semantic provider escaped the optional semantic-pack boundary",
    )?;

    let mut wrong_registry_version = lock.clone();
    wrong_registry_version.schema_version += 1;
    require(
        validate_registry_lock(&wrong_registry_version, &accepted).is_err(),
        "unsupported language-registry schema version was accepted",
    )?;
    let mut wrong_accepted_source = serde_json::from_slice::<AcceptedCapabilityRegistry>(ACCEPTED)?;
    wrong_accepted_source.schema_version += 1;
    let wrong_accepted = materialize_accepted_target(wrong_accepted_source)?;
    require(
        validate_accepted_target(&lock, &wrong_accepted).is_err(),
        "unsupported accepted-capability schema version was accepted",
    )?;

    for (label, invalid_license) in [
        ("whitespace-only", String::from("   ")),
        ("untrimmed", String::from(" MIT")),
        ("control-bearing", String::from("MI\0T")),
        ("oversized", "A".repeat(MAX_ID_BYTES + 1)),
    ] {
        let mut invalid_asset = asset_value.clone();
        invalid_asset["license"] = json!(invalid_license);
        require(
            serde_json::from_value::<ParserAsset>(invalid_asset).is_err(),
            format!("{label} parser asset license was accepted"),
        )?;
    }

    let accepted_value = serde_json::from_slice::<serde_json::Value>(ACCEPTED)?;
    for (label, mut invalid_defaults, field) in [
        (
            "accepted mode evidence",
            accepted_value["mode_defaults"].clone(),
            "evidence_state",
        ),
        (
            "accepted mode advertisement",
            accepted_value["mode_defaults"].clone(),
            "advertisement",
        ),
    ] {
        invalid_defaults[field] = json!("invalid-lifecycle-state");
        require(
            serde_json::from_value::<AcceptedModeDefaults>(invalid_defaults).is_err(),
            format!("invalid {label} lifecycle spelling was accepted"),
        )?;
    }
    let mut invalid_parser_defaults = accepted_value["parser_defaults"].clone();
    invalid_parser_defaults["evidence_state"] = json!("invalid-lifecycle-state");
    require(
        serde_json::from_value::<AcceptedParserDefaults>(invalid_parser_defaults).is_err(),
        "invalid accepted parser evidence lifecycle spelling was accepted",
    )?;
    for (label, invalid_override) in [
        (
            "accepted mode evidence override",
            serde_json::from_value::<AcceptedModeOverrides>(json!({
                "evidence_state": "invalid-lifecycle-state"
            })),
        ),
        (
            "accepted mode advertisement override",
            serde_json::from_value::<AcceptedModeOverrides>(json!({
                "advertisement": "invalid-lifecycle-state"
            })),
        ),
    ] {
        require(
            invalid_override.is_err(),
            format!("invalid {label} spelling was accepted"),
        )?;
    }
    require(
        serde_json::from_value::<AcceptedParserOverrides>(json!({
            "evidence_state": "invalid-lifecycle-state"
        }))
        .is_err(),
        "invalid accepted parser evidence override spelling was accepted",
    )?;

    let mut missing_source = asset_value;
    let missing_source_object = missing_source
        .as_object_mut()
        .ok_or_else(|| io::Error::other("parser asset witness is not an object"))?;
    missing_source_object.remove("source");
    require(
        serde_json::from_value::<ParserAsset>(missing_source).is_err(),
        "parser asset without source metadata was accepted",
    )?;
    let mut unknown_embedded = embedded_value;
    unknown_embedded["unexpected"] = json!(true);
    require(
        serde_json::from_value::<EmbeddedAdapter>(unknown_embedded).is_err(),
        "unknown embedded-adapter field was accepted",
    )?;
    require(
        reject_duplicate_json_keys(
            br#"{"asset":{"source":"one","source":"two"}}"#,
            "language registry duplicate witness",
        )
        .is_err()
            && reject_duplicate_json_keys(
                br#"{"mode":{"required_claims":[],"required_claims":[]}}"#,
                "accepted capability duplicate witness",
            )
            .is_err(),
        "nested duplicate keys were accepted",
    )?;

    require(
        lock.assets.is_empty()
            && lock.embedded_adapters.is_empty()
            && lock.query_packs.is_empty()
            && lock.semantic_providers.is_empty()
            && generated().is_ok(),
        "empty pending live inventories are no longer a valid lock state",
    )?;
    require(
        parser_abi.version == 15
            && parser_abi.abi_id.as_str() == "abi.tree-sitter-wasm"
            && parser_abi.state == AbiState::PendingPackVerification,
        "versioned parser ABI witness was not retained",
    )?;
    Ok(())
}

#[test]
fn duplicate_object_keys_are_rejected_before_typed_decode() {
    let source = String::from_utf8_lossy(LOCK);
    let duplicate = source.replacen('{', "{\"schema_version\":1,", 1);
    let result = validate_and_generate(duplicate.as_bytes(), &fixed_inputs());
    assert!(matches!(
        result,
        Err(LanguageRegistryError::JsonDecode { .. })
    ));
}

#[test]
fn unknown_nested_fields_are_rejected() -> Result<(), Box<dyn Error>> {
    let mut lock: serde_json::Value = serde_json::from_slice(LOCK)?;
    lock["packs"][0]["unexpected"] = json!(true);
    let bytes = serde_json::to_vec(&lock)?;
    let result = validate_and_generate(&bytes, &fixed_inputs());
    require(
        matches!(result, Err(LanguageRegistryError::JsonDecode { .. })),
        "unknown nested field was accepted",
    )?;
    Ok(())
}

#[test]
fn accepted_target_unknown_nested_fields_are_rejected() -> Result<(), Box<dyn Error>> {
    for (context, result) in [
        (
            "capability row",
            validate_accepted_mutation(|accepted| {
                accepted["capabilities"][0]["unexpected"] = json!(true);
                Ok(())
            })?,
        ),
        (
            "claim contract",
            validate_accepted_mutation(|accepted| {
                accepted["claim_types"]["detected"]["unexpected"] = json!(true);
                Ok(())
            })?,
        ),
        (
            "nested relation profile",
            validate_accepted_mutation(|accepted| {
                accepted["relation_traceability_contract"]["evidence_profiles"]["direct-syntax-v1"]
                    ["unexpected"] = json!(true);
                Ok(())
            })?,
        ),
    ] {
        require(
            matches!(result, Err(LanguageRegistryError::JsonDecode { .. })),
            format!("accepted {context} unknown field was accepted"),
        )?;
    }
    Ok(())
}

#[test]
fn accepted_target_capability_semantic_mutations_are_rejected() -> Result<(), Box<dyn Error>> {
    let duplicate = validate_accepted_mutation(|accepted| {
        accepted["capabilities"][1]["capability_id"] =
            accepted["capabilities"][0]["capability_id"].clone();
        Ok(())
    })?;
    require(
        matches!(duplicate, Err(LanguageRegistryError::Validation(_))),
        "duplicate accepted capability identifier was accepted",
    )?;

    for (context, result) in [
        (
            "unknown family",
            validate_accepted_mutation(|accepted| {
                accepted["capabilities"][0]["family"] = json!("unknown-family");
                Ok(())
            })?,
        ),
        (
            "unknown lifecycle",
            validate_accepted_mutation(|accepted| {
                accepted["capabilities"][0]["evidence_state"] = json!("ready");
                Ok(())
            })?,
        ),
    ] {
        require(
            matches!(result, Err(LanguageRegistryError::JsonDecode { .. })),
            format!("accepted capability {context} was accepted"),
        )?;
    }

    let broken_pack = validate_accepted_mutation(|accepted| {
        accepted["capabilities"][0]["pack_id"] = json!("missing-pack");
        Ok(())
    })?;
    require(
        broken_pack.is_err(),
        "accepted capability broken pack reference was accepted",
    )?;

    let empty_fixtures = validate_accepted_mutation(|accepted| {
        accepted["capabilities"][0]["fixture_ids"] = json!([]);
        Ok(())
    })?;
    require(
        matches!(empty_fixtures, Err(LanguageRegistryError::Validation(_))),
        "accepted capability empty fixture inventory was accepted",
    )?;
    Ok(())
}

#[test]
fn accepted_target_relation_traceability_mutations_are_rejected() -> Result<(), Box<dyn Error>> {
    let missing_traceability = validate_accepted_mutation(|accepted| {
        accepted_capability_mut(accepted, "relation.calls")?["traceability"] =
            serde_json::Value::Null;
        Ok(())
    })?;
    require(
        matches!(
            missing_traceability,
            Err(LanguageRegistryError::Validation(_))
        ),
        "accepted relation without traceability was accepted",
    )?;

    let unknown_profile = validate_accepted_mutation(|accepted| {
        accepted_capability_mut(accepted, "relation.calls")?["traceability"]["evidence_profile"] =
            json!("missing-profile");
        Ok(())
    })?;
    require(
        matches!(unknown_profile, Err(LanguageRegistryError::Validation(_))),
        "accepted relation with unknown traceability profile was accepted",
    )?;

    let enum_mismatch = validate_accepted_mutation(|accepted| {
        accepted_capability_mut(accepted, "relation.depends")?["traceability"]["typed_enum"] =
            json!("projectatlas_core::graph::GraphRelationKind::Imports");
        Ok(())
    })?;
    require(
        matches!(enum_mismatch, Err(LanguageRegistryError::Validation(_))),
        "accepted relation typed enum mismatch was accepted",
    )?;
    Ok(())
}

#[test]
fn accepted_target_relation_profile_mutations_are_rejected() -> Result<(), Box<dyn Error>> {
    let invalid_persistence = validate_accepted_mutation(|accepted| {
        accepted["relation_traceability_contract"]["persistence_profiles"]["slot-scoped-v1"]["tables"] =
            json!([]);
        Ok(())
    })?;
    require(
        matches!(
            invalid_persistence,
            Err(LanguageRegistryError::Validation(_))
        ),
        "invalid accepted relation persistence profile was accepted",
    )?;

    let invalid_accuracy = validate_accepted_mutation(|accepted| {
        accepted["relation_traceability_contract"]["accuracy_gates"]["symbols-family-v1"]["minimum_precision"] =
            json!(0.0);
        Ok(())
    })?;
    require(
        matches!(invalid_accuracy, Err(LanguageRegistryError::Validation(_))),
        "invalid accepted relation accuracy gate was accepted",
    )?;
    Ok(())
}

#[test]
fn external_contract_digests_and_identities_are_bound() {
    let mut accepted = ACCEPTED.to_vec();
    accepted.push(b'\n');
    let fixed = FixedInputBytes {
        accepted_capability_registry: &accepted,
        historical_runtime_contract: HISTORICAL,
    };
    let result = validate_and_generate(LOCK, &fixed);
    assert!(matches!(result, Err(LanguageRegistryError::Validation(_))));
}

#[test]
fn count_preserving_mode_remaps_are_rejected() -> Result<(), Box<dyn Error>> {
    let mut lock: serde_json::Value = serde_json::from_slice(LOCK)?;
    let replacement = lock["current_modes"][1]["accepted_mode_id"].clone();
    lock["current_modes"][0]["accepted_mode_id"] = replacement;
    let bytes = serde_json::to_vec(&lock)?;
    let result = validate_and_generate(&bytes, &fixed_inputs());
    require(
        matches!(result, Err(LanguageRegistryError::Validation(_))),
        "count-preserving accepted-mode remap was accepted",
    )?;
    Ok(())
}

#[test]
fn equivalent_case_folded_detection_aliases_are_preserved() -> Result<(), Box<dyn Error>> {
    let lock: LanguageRegistryLock = serde_json::from_slice(LOCK)?;
    let r_rows = lock
        .detection_rules
        .iter()
        .filter(|rule| rule.pattern().eq_ignore_ascii_case(".r"))
        .collect::<Vec<_>>();
    let [lowercase, uppercase] = r_rows.as_slice() else {
        return Err(io::Error::other("expected exactly two historical R aliases").into());
    };
    require_equal(&lowercase.pattern(), &".r", "lowercase R alias")?;
    require_equal(&uppercase.pattern(), &".R", "uppercase R alias")?;
    require_equal(&lowercase.mode_id(), &uppercase.mode_id(), "R alias mode")?;
    validate_detection_rules(
        &lock,
        &lock
            .current_modes
            .iter()
            .map(|mode| mode.mode_id.clone())
            .collect(),
    )?;
    Ok(())
}

#[test]
fn mixed_case_policy_detection_overlaps_are_rejected_in_both_orders() -> Result<(), Box<dyn Error>>
{
    for reverse in [false, true] {
        let mut lock: serde_json::Value = serde_json::from_slice(LOCK)?;
        let rules = lock["detection_rules"]
            .as_array_mut()
            .ok_or_else(|| io::Error::other("detection rules are not an array"))?;
        let uppercase = rules
            .iter_mut()
            .find(|rule| rule["extension"] == ".R")
            .ok_or_else(|| io::Error::other("uppercase R rule is absent"))?;
        uppercase["case"] = json!("sensitive");
        if reverse {
            rules.reverse();
        }
        let bytes = serde_json::to_vec(&lock)?;
        let result = validate_and_generate(&bytes, &fixed_inputs());
        require(
            matches!(result, Err(LanguageRegistryError::Validation(message)) if message.contains("ambiguously claim")),
            format!("mixed case-policy overlap was accepted with reverse={reverse}"),
        )?;
    }
    Ok(())
}

#[test]
fn compound_path_case_preserves_uppercase_d_ts_normalization() -> Result<(), Box<dyn Error>> {
    let lock: LanguageRegistryLock = serde_json::from_slice(LOCK)?;
    require_equal(
        &normalized_extension(&lock, "types.d.ts"),
        &".d.ts".to_string(),
        "lowercase compound extension",
    )?;
    require_equal(
        &normalized_extension(&lock, "nested/types.d.ts"),
        &".d.ts".to_string(),
        "slash compound extension",
    )?;
    require_equal(
        &normalized_extension(&lock, "nested\\types.d.ts"),
        &".d.ts".to_string(),
        "backslash compound extension",
    )?;
    require_equal(
        &normalized_extension(&lock, "types.D.TS"),
        &".ts".to_string(),
        "uppercase compound suffix remains ordinary extension",
    )?;
    Ok(())
}

#[test]
fn accepted_pack_assignments_do_not_change_current_routing() -> Result<(), Box<dyn Error>> {
    let lock: LanguageRegistryLock = serde_json::from_slice(LOCK)?;
    let accepted_source: AcceptedCapabilityRegistry = serde_json::from_slice(ACCEPTED)?;
    let accepted = materialize_accepted_target(accepted_source)?;
    for public_mode in ["vue", "powershell"] {
        let current = lock
            .current_modes
            .iter()
            .find(|mode| mode.public_mode.as_str() == public_mode)
            .ok_or_else(|| io::Error::other("current witness mode is absent"))?;
        let future = accepted
            .modes
            .iter()
            .find(|mode| mode.public_mode.as_str() == public_mode)
            .ok_or_else(|| io::Error::other("accepted witness mode is absent"))?;
        require_equal(
            &current.current_pack_id.as_str(),
            &"default-core",
            "current pack",
        )?;
        require_equal(
            &future.pack_id.as_str(),
            &"broad-language-pack",
            "future pack",
        )?;
    }
    Ok(())
}

const FORMATTER_TIMEOUT_MARKER_ENV: &str = "PROJECTATLAS_FORMATTER_TIMEOUT_MARKER";
const FORMATTER_DESCENDANT_READY_ENV: &str = "PROJECTATLAS_FORMATTER_DESCENDANT_READY";
const FORMATTER_TIMEOUT_FIXTURE: &str =
    "language_registry::tests::formatter_timeout_process_fixture";
const FORMATTER_DESCENDANT_FIXTURE: &str =
    "language_registry::tests::formatter_descendant_process_fixture";
const FORMATTER_OUTPUT_FIXTURE: &str = "language_registry::tests::formatter_output_process_fixture";
const FORMATTER_FAILURE_FIXTURE: &str =
    "language_registry::tests::formatter_failure_process_fixture";

fn formatter_fixture_command(test_name: &str) -> Result<ProcessCommand, Box<dyn Error>> {
    Ok(bounded_toolchain_command(env::current_exe()?).args([
        "--exact",
        test_name,
        "--ignored",
        "--nocapture",
    ]))
}

fn require_formatter_error(
    result: Result<ProcessResult<String>, LanguageRegistryError>,
    context: &str,
) -> Result<String, Box<dyn Error>> {
    match result {
        Err(LanguageRegistryError::FormatRust { detail, .. }) => Ok(detail),
        Err(error) => {
            Err(io::Error::other(format!("{context} returned the wrong error: {error}")).into())
        }
        Ok(_) => Err(io::Error::other(format!("{context} unexpectedly succeeded")).into()),
    }
}

fn verify_formatter_process_contract(
    formatter: &GeneratedRustFormatter,
) -> Result<(), Box<dyn Error>> {
    let policy = toml::from_str::<TestQualityPolicy>(TEST_QUALITY_POLICY)?;
    require(
        !policy.reference_toolchain.rust.is_empty(),
        "reference Rust toolchain is empty",
    )?;
    let input_path = PathBuf::from("formatter fixture with spaces").join("projection.rs");
    let command = formatter.command(&input_path);
    require_equal(
        command.program(),
        formatter.program.as_os_str(),
        "resolved pinned formatter program",
    )?;
    require_equal(
        command.arguments(),
        &[
            OsString::from("--edition"),
            OsString::from("2024"),
            OsString::from("--style-edition"),
            OsString::from("2024"),
            input_path.into_os_string(),
        ],
        "pinned formatter arguments",
    )?;
    require_equal(
        &command.configured_timeout(),
        &Some(RUST_FORMATTER_TIMEOUT),
        "pinned formatter timeout",
    )?;

    let missing = tempfile::tempdir()?
        .path()
        .join(format!("missing-rustfmt{}", std::env::consts::EXE_SUFFIX));
    let missing_command = bounded_toolchain_command(missing);
    let missing_detail = require_formatter_error(
        formatter.run("missing formatter fixture", &missing_command),
        "missing formatter fixture",
    )?;
    require(
        missing_detail.contains("failed to launch") || missing_detail.contains("not found"),
        format!("missing formatter diagnostic is not actionable: {missing_detail}"),
    )?;

    let marker_directory = tempfile::tempdir()?;
    let ready = marker_directory.path().join("descendant-ready");
    let marker = marker_directory.path().join("descendant-escaped");
    let timeout_command = formatter_fixture_command(FORMATTER_TIMEOUT_FIXTURE)?
        .env(FORMATTER_DESCENDANT_READY_ENV, &ready)
        .env(FORMATTER_TIMEOUT_MARKER_ENV, &marker)
        .timeout(Duration::from_secs(2));
    let started = Instant::now();
    let timeout_detail = require_formatter_error(
        formatter.run("formatter timeout fixture", &timeout_command),
        "formatter timeout fixture",
    )?;
    require(
        timeout_detail.contains("configured timeout"),
        format!("formatter timeout was not classified: {timeout_detail}"),
    )?;
    require(
        started.elapsed() < Duration::from_secs(10),
        "formatter timeout did not return within its bounded test envelope",
    )?;
    require(
        ready.is_file(),
        "formatter timeout fixture never started its descendant",
    )?;
    std::thread::sleep(Duration::from_millis(4_250));
    require(
        !marker.exists(),
        "formatter timeout allowed a descendant to survive and write its marker",
    )?;

    let output_command = formatter_fixture_command(FORMATTER_OUTPUT_FIXTURE)?;
    let output_detail = require_formatter_error(
        formatter.run("formatter output fixture", &output_command),
        "formatter output fixture",
    )?;
    require(
        output_detail.contains("retained bytes per stream"),
        format!("formatter output truncation was not classified: {output_detail}"),
    )?;

    let failure_command = formatter_fixture_command(FORMATTER_FAILURE_FIXTURE)?;
    let failure_detail = require_formatter_error(
        formatter.run("formatter failure fixture", &failure_command),
        "formatter failure fixture",
    )?;
    require(
        failure_detail.contains("exited with code"),
        format!("formatter nonzero exit was not classified: {failure_detail}"),
    )?;
    Ok(())
}

#[test]
#[ignore = "process fixture invoked by the ARRI 5.2 formatter supervision test"]
fn formatter_timeout_process_fixture() -> Result<(), Box<dyn Error>> {
    let marker = env::var_os(FORMATTER_TIMEOUT_MARKER_ENV)
        .ok_or_else(|| io::Error::other("formatter timeout marker is missing"))?;
    let child = Command::new(env::current_exe()?)
        .args([
            "--exact",
            FORMATTER_DESCENDANT_FIXTURE,
            "--ignored",
            "--nocapture",
        ])
        .env(FORMATTER_TIMEOUT_MARKER_ENV, marker)
        .spawn()?;
    drop(child);
    fs::write(
        env::var_os(FORMATTER_DESCENDANT_READY_ENV)
            .ok_or_else(|| io::Error::other("formatter descendant ready marker is missing"))?,
        b"ready",
    )?;
    std::thread::sleep(Duration::from_secs(10));
    Ok(())
}

#[test]
#[ignore = "descendant process fixture invoked by the ARRI 5.2 timeout fixture"]
fn formatter_descendant_process_fixture() -> Result<(), Box<dyn Error>> {
    let marker = env::var_os(FORMATTER_TIMEOUT_MARKER_ENV)
        .ok_or_else(|| io::Error::other("formatter descendant marker is missing"))?;
    std::thread::sleep(Duration::from_secs(4));
    fs::write(marker, b"escaped")?;
    Ok(())
}

#[test]
#[ignore = "process fixture invoked by the ARRI 5.2 capture-limit test"]
fn formatter_output_process_fixture() -> Result<(), Box<dyn Error>> {
    let bytes = vec![b'x'; RUST_FORMATTER_STREAM_LIMIT_BYTES + 1_024];
    let mut stdout = io::stdout().lock();
    stdout.write_all(&bytes)?;
    stdout.flush()?;
    Ok(())
}

#[test]
#[ignore = "process fixture invoked by the ARRI 5.2 status-diagnostic test"]
fn formatter_failure_process_fixture() -> Result<(), Box<dyn Error>> {
    Err(io::Error::other("formatter failure fixture sentinel").into())
}

#[test]
fn task_arri_ut_arri_5_2() -> Result<(), Box<dyn Error>> {
    let formatter = GeneratedRustFormatter::new()?;
    verify_formatter_process_contract(&formatter)?;
    let first = generated()?;
    let second = generated()?;
    let (lock, accepted, historical) = decoded_contracts()?;
    let mut paths = BTreeSet::new();
    for (left, right) in first.entries().into_iter().zip(second.entries()) {
        require_equal(&left.path, &right.path, "deterministic output owner")?;
        require_equal(&left.bytes, &right.bytes, "deterministic output bytes")?;
        require(!left.bytes.is_empty(), format!("{} is empty", left.path))?;
        require(
            paths.insert(left.path),
            format!("duplicate generated output path {}", left.path),
        )?;
        if Path::new(left.path)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
        {
            let source = std::str::from_utf8(left.bytes)?;
            let formatted = format_generated_rust("unit-test projection", source)?;
            require_equal(
                formatted.as_bytes(),
                left.bytes,
                "generated Rust formatter stability",
            )?;
            syn::parse_file(source).map_err(|source| {
                io::Error::other(format!("generated Rust {} is invalid: {source}", left.path))
            })?;
        }
    }
    require_equal(
        &first.entries().map(|entry| entry.path),
        &[
            CORE_OUTPUT_PATH,
            SYMBOLS_OUTPUT_PATH,
            CLI_OUTPUT_PATH,
            EVIDENCE_OUTPUT_PATH,
            DOCUMENTATION_OUTPUT_PATH,
        ],
        "fixed output ownership",
    )?;

    typecheck_generated_rust(&first)?;
    verify_generated_rust_rows(&lock, &accepted, &first)?;
    let (representative, representative_outputs) =
        representative_artifacts(&lock, &accepted, &historical)?;
    typecheck_generated_rust(&representative_outputs)?;
    verify_generated_rust_rows(&representative, &accepted, &representative_outputs)?;
    verify_generated_json(
        &lock,
        &accepted,
        &historical,
        &sha256_hex(LOCK),
        &registry_contract_digest(&lock, &accepted, &historical),
        &first,
    )?;
    let representative_bytes = serde_json::to_vec(&representative)?;
    verify_generated_json(
        &representative,
        &accepted,
        &historical,
        &sha256_hex(&representative_bytes),
        &registry_contract_digest(&representative, &accepted, &historical),
        &representative_outputs,
    )?;
    let representative_core = std::str::from_utf8(&representative_outputs.core)?;
    let representative_symbols = std::str::from_utf8(&representative_outputs.symbols)?;
    let representative_state =
        serde_json::from_slice::<serde_json::Value>(&representative_outputs.evidence)?;

    let mut content_mutation = representative.clone();
    let content_before = content_mutation
        .detection_rules
        .last()
        .ok_or_else(|| io::Error::other("representative content detector disappeared"))?;
    let content_before_row = expected_detection_row(content_before);
    let Some(DetectionRule::Content { detector_id, .. }) =
        content_mutation.detection_rules.last_mut()
    else {
        return Err(io::Error::other("representative content detector disappeared").into());
    };
    detector_id.0.push_str("-projection-change");
    let content_outputs = render_test_lock(&content_mutation, &accepted, &historical)?;
    let content_after = content_mutation
        .detection_rules
        .last()
        .ok_or_else(|| io::Error::other("mutated content detector disappeared"))?;
    require_generated_row_change(
        representative_core,
        std::str::from_utf8(&content_outputs.core)?,
        &content_before_row,
        &expected_detection_row(content_after),
        "content-detection Rust projection",
    )?;
    let content_state = serde_json::from_slice::<serde_json::Value>(&content_outputs.evidence)?;
    require_json_projection_change(
        &representative_state,
        &content_state,
        "/current/detection_rules",
        "content-detection evidence",
    )?;

    let mut asset_mutation = representative.clone();
    let asset = asset_mutation
        .assets
        .last_mut()
        .ok_or_else(|| io::Error::other("representative parser asset disappeared"))?;
    asset.source.0.push_str("/projection-change");
    let asset_outputs = render_test_lock(&asset_mutation, &accepted, &historical)?;
    let asset_state = serde_json::from_slice::<serde_json::Value>(&asset_outputs.evidence)?;
    require_json_projection_change(
        &representative_state,
        &asset_state,
        "/sbom_inputs/parser_assets",
        "parser-asset SBOM/provenance evidence",
    )?;

    let mut query_mutation = representative.clone();
    let query_before = query_mutation
        .query_packs
        .last()
        .ok_or_else(|| io::Error::other("representative query pack disappeared"))?;
    let query_before_row = expected_query_pack_row(query_before);
    let query = query_mutation
        .query_packs
        .last_mut()
        .ok_or_else(|| io::Error::other("representative query pack disappeared"))?;
    query.path.0.push_str(".projection-change");
    let query_outputs = render_test_lock(&query_mutation, &accepted, &historical)?;
    let query_after = query_mutation
        .query_packs
        .last()
        .ok_or_else(|| io::Error::other("mutated query pack disappeared"))?;
    require_generated_row_change(
        representative_symbols,
        std::str::from_utf8(&query_outputs.symbols)?,
        &query_before_row,
        &expected_query_pack_row(query_after),
        "query-pack Rust projection",
    )?;
    let query_state = serde_json::from_slice::<serde_json::Value>(&query_outputs.evidence)?;
    require_json_projection_change(
        &representative_state,
        &query_state,
        "/conformance_inventory/query_packs",
        "query-pack conformance evidence",
    )?;
    require_json_projection_change(
        &representative_state,
        &query_state,
        "/sbom_inputs/query_packs",
        "query-pack SBOM evidence",
    )?;

    let mut embedded_mutation = representative.clone();
    let embedded_before = embedded_mutation
        .embedded_adapters
        .last()
        .ok_or_else(|| io::Error::other("representative embedded adapter disappeared"))?;
    let embedded_before_row = expected_embedded_adapter_row(embedded_before);
    let adapter = embedded_mutation
        .embedded_adapters
        .last_mut()
        .ok_or_else(|| io::Error::other("representative embedded adapter disappeared"))?;
    std::mem::swap(&mut adapter.host_mode_id, &mut adapter.embedded_mode_id);
    let embedded_outputs = render_test_lock(&embedded_mutation, &accepted, &historical)?;
    let embedded_after = embedded_mutation
        .embedded_adapters
        .last()
        .ok_or_else(|| io::Error::other("mutated embedded adapter disappeared"))?;
    require_generated_row_change(
        representative_symbols,
        std::str::from_utf8(&embedded_outputs.symbols)?,
        &embedded_before_row,
        &expected_embedded_adapter_row(embedded_after),
        "embedded-adapter Rust projection",
    )?;
    let embedded_state = serde_json::from_slice::<serde_json::Value>(&embedded_outputs.evidence)?;
    require_json_projection_change(
        &representative_state,
        &embedded_state,
        "/conformance_inventory/embedded_adapters",
        "embedded-adapter conformance evidence",
    )?;

    let mut semantic_mutation = representative.clone();
    let semantic_before = semantic_mutation
        .semantic_providers
        .last()
        .ok_or_else(|| io::Error::other("representative semantic provider disappeared"))?;
    let semantic_before_row = expected_semantic_provider_row(semantic_before);
    let provider = semantic_mutation
        .semantic_providers
        .last_mut()
        .ok_or_else(|| io::Error::other("representative semantic provider disappeared"))?;
    provider
        .mode_ids
        .push(serde_json::from_value(json!("mode.typescript"))?);
    let semantic_outputs = render_test_lock(&semantic_mutation, &accepted, &historical)?;
    let semantic_after = semantic_mutation
        .semantic_providers
        .last()
        .ok_or_else(|| io::Error::other("mutated semantic provider disappeared"))?;
    require_generated_row_change(
        representative_symbols,
        std::str::from_utf8(&semantic_outputs.symbols)?,
        &semantic_before_row,
        &expected_semantic_provider_row(semantic_after),
        "semantic-provider Rust projection",
    )?;
    let semantic_state = serde_json::from_slice::<serde_json::Value>(&semantic_outputs.evidence)?;
    require_json_projection_change(
        &representative_state,
        &semantic_state,
        "/conformance_inventory/semantic_providers",
        "semantic-provider conformance evidence",
    )?;

    let mut provenance_mutation = representative;
    let provenance = provenance_mutation
        .evidence
        .last_mut()
        .ok_or_else(|| io::Error::other("representative provenance evidence disappeared"))?;
    provenance.path.0.push_str(".projection-change");
    let provenance_outputs = render_test_lock(&provenance_mutation, &accepted, &historical)?;
    let provenance_state =
        serde_json::from_slice::<serde_json::Value>(&provenance_outputs.evidence)?;
    require_json_projection_change(
        &representative_state,
        &provenance_state,
        "/conformance_inventory/evidence",
        "registry provenance conformance evidence",
    )?;
    require_json_projection_change(
        &representative_state,
        &provenance_state,
        "/sbom_inputs/parser_components",
        "component provenance SBOM evidence",
    )?;

    let state: serde_json::Value = serde_json::from_slice(&first.evidence)?;
    require_equal(
        &state["format"].as_str(),
        &Some("projectatlas.language-capability-state"),
        "capability-state format",
    )?;
    let parity = &state["accepted_capability_parity"];
    require_equal(
        &parity["complete"].as_bool(),
        &Some(false),
        "pending accepted parity",
    )?;
    let parity_modes = json_rows(parity, "modes")?;
    let parity_parsers = json_rows(parity, "parsers")?;
    let parity_crosswalk = json_rows(parity, "language_crosswalk")?;
    let parity_capabilities = json_rows(parity, "capabilities")?;
    let mut expected_mode_ids = accepted
        .modes
        .iter()
        .map(|mode| mode.mode_id.as_str().to_string())
        .collect::<Vec<_>>();
    expected_mode_ids.sort();
    let mut expected_parser_ids = accepted
        .parsers
        .iter()
        .map(|parser| parser.parser_id.as_str().to_string())
        .collect::<Vec<_>>();
    expected_parser_ids.sort();
    let mut expected_crosswalk_ids = accepted
        .source
        .accepted_language_crosswalk
        .entries
        .iter()
        .map(|row| row.accepted_name_id.as_str().to_string())
        .collect::<Vec<_>>();
    expected_crosswalk_ids.sort();
    let expected_capability_ids = accepted
        .source
        .capabilities
        .iter()
        .map(|row| row.capability_id.clone())
        .collect::<Vec<_>>();
    require_equal(
        &json_row_ids(parity_modes, "accepted_mode_id")?,
        &expected_mode_ids,
        "accepted parity mode identities",
    )?;
    require_equal(
        &json_row_ids(parity_parsers, "parser_id")?,
        &expected_parser_ids,
        "accepted parity parser identities",
    )?;
    require_equal(
        &json_row_ids(parity_crosswalk, "accepted_name_id")?,
        &expected_crosswalk_ids,
        "accepted parity crosswalk identities",
    )?;
    require_equal(
        &json_row_ids(parity_capabilities, "capability_id")?,
        &expected_capability_ids,
        "accepted general-capability identities",
    )?;
    require_equal(
        &parity["claim_types"].as_object().map(serde_json::Map::len),
        &Some(lock.capability_tiers.len()),
        "accepted claim-type inventory",
    )?;
    for mode in &accepted.modes {
        let row = parity_modes
            .iter()
            .find(|row| row["accepted_mode_id"] == mode.mode_id.as_str())
            .ok_or_else(|| io::Error::other("accepted parity mode row disappeared"))?;
        require_equal(
            &row["fixture_ids"].as_array().map(Vec::len),
            &Some(mode.fixture_ids.len()),
            "accepted fixture inventory",
        )?;
        require_equal(
            &row["required_platforms"].as_array().map(Vec::len),
            &Some(mode.required_platforms.len()),
            "accepted platform inventory",
        )?;
        require_equal(
            &row["evidence_state"].as_str(),
            &Some(mode.evidence_state.contract_tag()),
            "accepted mode evidence state",
        )?;
        require_equal(
            &row["advertisement"].as_str(),
            &Some(mode.advertisement.contract_tag()),
            "accepted mode advertisement state",
        )?;
    }

    let settings = &state["settings"];
    for (field, expected) in [
        ("current_modes", lock.current_modes.len()),
        ("accepted_modes", accepted.modes.len()),
        ("accepted_parser_capabilities", accepted.parsers.len()),
        ("parser_components", lock.parser_components.len()),
        ("parser_assets", lock.assets.len()),
        ("embedded_adapters", lock.embedded_adapters.len()),
        ("query_packs", lock.query_packs.len()),
        ("semantic_providers", lock.semantic_providers.len()),
    ] {
        require_equal(
            &settings[field].as_u64(),
            &Some(u64::try_from(expected)?),
            field,
        )?;
    }
    require_equal(
        &settings["accepted_set_sha256"].as_str(),
        &Some(accepted.source.accepted_set_digest.as_str()),
        "settings accepted-set digest",
    )?;
    require_equal(
        &settings["capability_tiers"].as_array().map(Vec::len),
        &Some(lock.capability_tiers.len()),
        "settings tier vocabulary",
    )?;

    let conformance = &state["conformance_inventory"];
    let conformance_modes = json_rows(conformance, "modes")?;
    require_equal(
        &json_row_ids(conformance_modes, "mode_id")?,
        &expected_mode_ids,
        "conformance mode identities",
    )?;
    require_equal(
        &json_rows(conformance, "fixtures")?.len(),
        &lock.fixtures.len(),
        "registered conformance fixtures",
    )?;
    require(
        conformance_modes.iter().all(|row| {
            row["advertised"] == false
                && row["fixture_ids"].as_array().is_some_and(|fixtures| {
                    fixtures.len()
                        == row["registered_fixture_ids"].as_array().map_or(0, Vec::len)
                            + row["missing_fixture_ids"].as_array().map_or(0, Vec::len)
                })
        }),
        "conformance inventory claimed support or lost fixture accounting",
    )?;

    let documentation: serde_json::Value = serde_json::from_slice(&first.documentation)?;
    require_equal(
        &documentation["format"].as_str(),
        &Some("projectatlas.language-capabilities"),
        "documentation support format",
    )?;
    require_equal(
        &documentation["parity_complete"].as_bool(),
        &Some(false),
        "documentation pending parity",
    )?;
    require_equal(
        &json_rows(&documentation["support"], "modes")?.len(),
        &accepted.modes.len(),
        "documentation support rows",
    )?;

    let sbom = &state["sbom_inputs"];
    let component_rows = json_rows(sbom, "parser_components")?;
    require_equal(
        &component_rows.len(),
        &lock.parser_components.len(),
        "component SBOM inputs",
    )?;
    for component in &lock.parser_components {
        let row = component_rows
            .iter()
            .find(|row| row["parser_id"] == component.parser_id.as_str())
            .ok_or_else(|| io::Error::other("component SBOM input disappeared"))?;
        require_equal(
            &row["abi_id"].as_str(),
            &Some(component.abi.abi_id.as_str()),
            "component SBOM ABI identity",
        )?;
        require_equal(
            &row["fixtures"].as_array().map(Vec::len),
            &Some(component.fixture_ids.len()),
            "component SBOM fixtures",
        )?;
        require_equal(
            &row["provenance_evidence"].as_array().map(Vec::len),
            &Some(component.provenance_evidence_ids.len()),
            "component SBOM provenance",
        )?;
    }
    require_equal(
        &json_rows(sbom, "parser_assets")?.len(),
        &lock.assets.len(),
        "parser-asset SBOM inputs",
    )?;

    let root = tempfile::tempdir()?;
    seed_inputs(root.path())?;
    write(root.path())?;
    check(root.path())?;
    for entry in first.entries() {
        let path = root.path().join(entry.path);
        fs::write(&path, b"deliberate-task-5.2-drift")?;
        let corrupted = fs::read(&path)?;
        let result = check(root.path());
        require(
            matches!(result, Err(LanguageRegistryError::Drift(rows)) if rows == vec![format!("{} differs", entry.path)]),
            format!("changed output drift was not exact for {}", entry.path),
        )?;
        require_equal(
            &fs::read(&path)?,
            &corrupted,
            "read-only drift check preserved changed bytes",
        )?;
        fs::write(&path, entry.bytes)?;
        fs::remove_file(&path)?;
        let result = check(root.path());
        require(
            matches!(result, Err(LanguageRegistryError::Drift(rows)) if rows == vec![format!("{} is missing", entry.path)]),
            format!("missing output drift was not exact for {}", entry.path),
        )?;
        require(
            matches!(fs::symlink_metadata(&path), Err(error) if error.kind() == io::ErrorKind::NotFound),
            format!("read-only drift check recreated {}", entry.path),
        )?;
        fs::write(path, entry.bytes)?;
    }
    check(root.path())?;
    Ok(())
}

#[test]
fn task_arri_ut_arri_5_3() -> Result<(), Box<dyn Error>> {
    let (lock, accepted, _) = decoded_contracts()?;
    validate_accepted_target(&lock, &accepted)?;
    validate_registry_lock(&lock, &accepted)?;

    let historical_schema = validate_historical_schema_mutation(2)?;
    require(
        matches!(
            &historical_schema,
            Err(LanguageRegistryError::Validation(message))
                if message.contains("historical runtime contract schema version must be 3, found 2")
        ),
        format!(
            "historical runtime schema drift had the wrong owner: {:?}",
            historical_schema.as_ref().err()
        ),
    )?;

    let unknown_stage = validate_accepted_mutation(|accepted| {
        accepted["detection_policy"]["precedence"][0] = json!("unknown-stage");
        Ok(())
    })?;
    require(
        matches!(
            &unknown_stage,
            Err(LanguageRegistryError::JsonDecode { label, source })
                if *label == "accepted capability registry"
                    && source.to_string().contains("unknown variant `unknown-stage`")
        ),
        format!(
            "unknown accepted detection stage had the wrong owner: {:?}",
            unknown_stage.as_ref().err()
        ),
    )?;
    let missing_stage = validate_accepted_mutation(|accepted| {
        accepted["detection_policy"]["precedence"]
            .as_array_mut()
            .ok_or_else(|| io::Error::other("accepted detection precedence is not an array"))?
            .pop();
        Ok(())
    })?;
    require(
        matches!(
            &missing_stage,
            Err(LanguageRegistryError::JsonDecode { label, source })
                if *label == "accepted capability registry"
                    && source.to_string().contains("invalid length 5")
                    && source.to_string().contains("array of length 6")
        ),
        format!(
            "incomplete accepted detection precedence had the wrong owner: {:?}",
            missing_stage.as_ref().err()
        ),
    )?;
    let duplicate_stage = validate_accepted_mutation(|accepted| {
        accepted["detection_policy"]["precedence"][5] = json!("content-signature");
        Ok(())
    })?;
    require(
        matches!(
            &duplicate_stage,
            Err(LanguageRegistryError::Validation(message))
                if message.contains("accepted target detection precedence")
        ),
        format!(
            "duplicate accepted detection stage had the wrong owner: {:?}",
            duplicate_stage.as_ref().err()
        ),
    )?;
    let swapped_stages = validate_accepted_mutation(|accepted| {
        accepted["detection_policy"]["precedence"]
            .as_array_mut()
            .ok_or_else(|| io::Error::other("accepted detection precedence is not an array"))?
            .swap(3, 4);
        Ok(())
    })?;
    require(
        matches!(
            &swapped_stages,
            Err(LanguageRegistryError::Validation(message))
                if message.contains("accepted target detection precedence")
        ),
        format!(
            "reordered accepted content-detection stages had the wrong owner: {:?}",
            swapped_stages.as_ref().err()
        ),
    )?;
    let appended_stage = validate_accepted_mutation(|accepted| {
        accepted["detection_policy"]["precedence"]
            .as_array_mut()
            .ok_or_else(|| io::Error::other("accepted detection precedence is not an array"))?
            .push(json!("project-context"));
        Ok(())
    })?;
    require(
        matches!(
            &appended_stage,
            Err(LanguageRegistryError::JsonDecode { label, source })
                if *label == "accepted capability registry"
                    && source.to_string().contains("trailing characters")
        ),
        format!(
            "appended accepted detection stage had the wrong owner: {:?}",
            appended_stage.as_ref().err()
        ),
    )?;

    for parser in &accepted.parsers {
        let suffix = parser
            .parser_id
            .as_str()
            .strip_prefix("parse.")
            .ok_or_else(|| io::Error::other("accepted parser lost its parse. prefix"))?;
        require_equal(
            parser.asset_id.as_str(),
            &format!("asset.{suffix}"),
            "materialized accepted parser asset identity",
        )?;
        require_equal(
            parser.query_pack_id.as_str(),
            &format!("queries.{suffix}"),
            "materialized accepted parser query-pack identity",
        )?;
    }

    let representative = representative_registry_witness(&lock)?.lock;
    validate_accepted_target(&representative, &accepted)?;
    validate_registry_lock(&representative, &accepted)?;

    let mut delivered_accepted = materialize_accepted_target(serde_json::from_slice(ACCEPTED)?)?;
    let delivered_parser = delivered_accepted
        .parsers
        .iter_mut()
        .find(|parser| parser.pack_id.as_str() == "broad-language-pack")
        .ok_or_else(|| io::Error::other("accepted broad-pack parser is absent"))?;
    delivered_parser.asset_id = AssetId::try_from("asset.javascript-wasm".to_string())?;
    delivered_parser.query_pack_id = QueryPackId::try_from("queries.javascript-wasm".to_string())?;
    delivered_parser.tree_sitter_abi = Some(serde_json::from_value(json!("15"))?);
    validate_accepted_target(&representative, &delivered_accepted)?;

    let mut duplicate_pack = lock.clone();
    let duplicate_pack_id = duplicate_pack
        .packs
        .first()
        .ok_or_else(|| io::Error::other("registry has no pack witness"))?
        .pack_id
        .clone();
    duplicate_pack.packs.push(
        duplicate_pack
            .packs
            .first()
            .ok_or_else(|| io::Error::other("registry pack witness disappeared"))?
            .clone(),
    );
    require_validation_fragments(
        validate_registry_lock(&duplicate_pack, &accepted),
        "duplicate pack identity",
        &["duplicate registry pack", duplicate_pack_id.as_str()],
    )?;

    let mut duplicate_fixture = lock.clone();
    let fixture = duplicate_fixture
        .fixtures
        .first()
        .ok_or_else(|| io::Error::other("registry has no fixture witness"))?
        .clone();
    let duplicate_fixture_id = fixture.fixture_id.clone();
    duplicate_fixture.fixtures.push(fixture);
    require_validation_fragments(
        validate_registry_lock(&duplicate_fixture, &accepted),
        "duplicate fixture identity",
        &[
            "duplicate fixture identifier",
            duplicate_fixture_id.as_str(),
        ],
    )?;

    for reverse in [false, true] {
        let mut ambiguous = lock.clone();
        let lowercase = ambiguous
            .detection_rules
            .iter()
            .find(|rule| rule.pattern() == ".r")
            .ok_or_else(|| io::Error::other("lowercase R detection rule is absent"))?;
        let lowercase_id = lowercase.id().clone();
        let lowercase_mode = lowercase.mode_id().clone();
        let uppercase = ambiguous
            .detection_rules
            .iter_mut()
            .find(|rule| rule.pattern() == ".R")
            .ok_or_else(|| io::Error::other("uppercase R detection rule is absent"))?;
        let uppercase_id = uppercase.id().clone();
        let rust_mode = ModeId::try_from("mode.rust".to_string())?;
        let DetectionRule::Extension { case, mode_id, .. } = uppercase else {
            return Err(io::Error::other("uppercase R rule is not an extension rule").into());
        };
        *case = CasePolicy::Sensitive;
        *mode_id = rust_mode.clone();
        if reverse {
            ambiguous.detection_rules.reverse();
        }
        let current_modes = ambiguous
            .current_modes
            .iter()
            .map(|mode| mode.mode_id.clone())
            .collect();
        require_validation_fragments(
            validate_detection_rules(&ambiguous, &current_modes),
            "ambiguous extension precedence",
            &[
                lowercase_id.as_str(),
                uppercase_id.as_str(),
                lowercase_mode.as_str(),
                rust_mode.as_str(),
                "extension field",
                ".r",
                ".R",
                "extension precedence",
            ],
        )?;
    }

    let mut phantom_reference = lock.clone();
    let phantom_mode = ModeId::try_from("mode.phantom".to_string())?;
    let phantom_rule = phantom_reference
        .detection_rules
        .first_mut()
        .ok_or_else(|| io::Error::other("registry has no detection-rule witness"))?;
    let phantom_rule_id = phantom_rule.id().clone();
    match phantom_rule {
        DetectionRule::ExactFilename { mode_id, .. }
        | DetectionRule::CompoundExtension { mode_id, .. }
        | DetectionRule::Extension { mode_id, .. }
        | DetectionRule::Content { mode_id, .. } => *mode_id = phantom_mode.clone(),
    }
    require_validation_fragments(
        validate_registry_lock(&phantom_reference, &accepted),
        "phantom detection mode",
        &[
            "detection rule",
            phantom_rule_id.as_str(),
            "missing current mode",
            phantom_mode.as_str(),
        ],
    )?;

    let mut unreachable_mode = lock.clone();
    let unreachable_mode_id = unreachable_mode
        .current_modes
        .first()
        .ok_or_else(|| io::Error::other("registry has no current-mode witness"))?
        .mode_id
        .clone();
    unreachable_mode
        .detection_rules
        .retain(|rule| rule.mode_id() != &unreachable_mode_id);
    require_validation_fragments(
        validate_registry_lock(&unreachable_mode, &accepted),
        "unreachable current mode",
        &[unreachable_mode_id.as_str(), "has no detection rule"],
    )?;

    let mut missing_asset = lock.clone();
    let parser = missing_asset
        .parser_components
        .first_mut()
        .ok_or_else(|| io::Error::other("registry has no parser-component witness"))?;
    let missing_asset_parser = parser.parser_id.clone();
    let missing_asset_id = AssetId::try_from("asset.missing".to_string())?;
    parser.asset_id = Some(missing_asset_id.clone());
    require_validation_fragments(
        validate_registry_lock(&missing_asset, &accepted),
        "missing live parser asset",
        &[
            missing_asset_parser.as_str(),
            "missing parser asset",
            missing_asset_id.as_str(),
        ],
    )?;

    let mut missing_accepted_asset =
        materialize_accepted_target(serde_json::from_slice(ACCEPTED)?)?;
    let accepted_parser = missing_accepted_asset
        .parsers
        .iter_mut()
        .find(|parser| parser.kind == AcceptedParserKind::TreeSitterOrVettedParser)
        .ok_or_else(|| io::Error::other("accepted target has no grammar parser witness"))?;
    let missing_accepted_parser_id = accepted_parser.parser_id.clone();
    accepted_parser.asset_id = missing_asset_id.clone();
    accepted_parser.tree_sitter_abi = Some(serde_json::from_value(json!("15"))?);
    require_validation_fragments(
        validate_accepted_target(&lock, &missing_accepted_asset),
        "missing accepted parser asset",
        &[
            missing_accepted_parser_id.as_str(),
            "tree-sitter ABI 15",
            missing_asset_id.as_str(),
            "missing",
        ],
    )?;

    let mut incompatible_abi = lock.clone();
    let parser = incompatible_abi
        .parser_components
        .first_mut()
        .ok_or_else(|| io::Error::other("registry has no parser ABI witness"))?;
    let incompatible_parser_id = parser.parser_id.clone();
    parser.abi.version = CURRENT_COMPILED_PARSER_ABI_VERSION + 1;
    let incompatible_version = parser.abi.version.to_string();
    let expected_version = CURRENT_COMPILED_PARSER_ABI_VERSION.to_string();
    require_validation_fragments(
        validate_registry_lock(&incompatible_abi, &accepted),
        "incompatible current parser ABI",
        &[
            incompatible_parser_id.as_str(),
            "incompatible ABI",
            CURRENT_COMPILED_PARSER_ABI_ID,
            incompatible_version.as_str(),
            expected_version.as_str(),
        ],
    )?;

    let mut incompatible_accepted = materialize_accepted_target(serde_json::from_slice(ACCEPTED)?)?;
    let accepted_parser = incompatible_accepted
        .parsers
        .iter_mut()
        .find(|parser| parser.pack_id.as_str() == "broad-language-pack")
        .ok_or_else(|| io::Error::other("accepted broad-pack parser is absent"))?;
    let incompatible_accepted_parser_id = accepted_parser.parser_id.clone();
    accepted_parser.asset_id = AssetId::try_from("asset.javascript-wasm".to_string())?;
    accepted_parser.tree_sitter_abi = Some(serde_json::from_value(json!("14"))?);
    require_validation_fragments(
        validate_accepted_target(&representative, &incompatible_accepted),
        "incompatible accepted parser ABI",
        &[
            incompatible_accepted_parser_id.as_str(),
            "tree-sitter ABI version 14",
            "asset.javascript-wasm",
            "abi.tree-sitter-wasm",
            "version 15",
        ],
    )?;

    let mut missing_fixture = lock.clone();
    let parser = missing_fixture
        .parser_components
        .first_mut()
        .ok_or_else(|| io::Error::other("registry has no parser fixture witness"))?;
    let missing_fixture_parser = parser.parser_id.clone();
    let missing_fixture_id = FixtureId::try_from("fixture.missing".to_string())?;
    parser.fixture_ids = vec![missing_fixture_id.clone()];
    require_validation_fragments(
        validate_registry_lock(&missing_fixture, &accepted),
        "missing parser fixture",
        &[
            missing_fixture_parser.as_str(),
            "missing fixture",
            missing_fixture_id.as_str(),
        ],
    )?;

    let mut missing_adapter_fixture = representative.clone();
    let adapter = missing_adapter_fixture
        .embedded_adapters
        .first_mut()
        .ok_or_else(|| io::Error::other("representative embedded adapter is absent"))?;
    let missing_fixture_adapter_id = adapter.adapter_id.clone();
    adapter.fixture_ids = vec![missing_fixture_id.clone()];
    require_validation_fragments(
        validate_registry_lock(&missing_adapter_fixture, &accepted),
        "missing embedded-adapter fixture",
        &[
            missing_fixture_adapter_id.as_str(),
            "missing fixture",
            missing_fixture_id.as_str(),
        ],
    )?;

    let mut missing_provider_fixture = representative.clone();
    let provider = missing_provider_fixture
        .semantic_providers
        .first_mut()
        .ok_or_else(|| io::Error::other("representative semantic provider is absent"))?;
    let missing_fixture_provider_id = provider.provider_id.clone();
    provider.fixture_ids = vec![missing_fixture_id.clone()];
    require_validation_fragments(
        validate_registry_lock(&missing_provider_fixture, &accepted),
        "missing semantic-provider fixture",
        &[
            missing_fixture_provider_id.as_str(),
            "missing fixture",
            missing_fixture_id.as_str(),
        ],
    )?;

    let mut empty_parser_fixtures = lock.clone();
    let parser = empty_parser_fixtures
        .parser_components
        .first_mut()
        .ok_or_else(|| io::Error::other("registry has no parser fixture witness"))?;
    let empty_parser_id = parser.parser_id.clone();
    parser.fixture_ids.clear();
    require_validation_fragments(
        validate_registry_lock(&empty_parser_fixtures, &accepted),
        "empty parser fixture inventory",
        &[empty_parser_id.as_str(), "has no fixtures"],
    )?;

    let mut empty_adapter_fixtures = representative.clone();
    let adapter = empty_adapter_fixtures
        .embedded_adapters
        .first_mut()
        .ok_or_else(|| io::Error::other("representative embedded adapter is absent"))?;
    let empty_adapter_id = adapter.adapter_id.clone();
    adapter.fixture_ids.clear();
    require_validation_fragments(
        validate_registry_lock(&empty_adapter_fixtures, &accepted),
        "empty embedded-adapter fixture inventory",
        &[empty_adapter_id.as_str(), "has no fixtures"],
    )?;

    let mut empty_provider_fixtures = representative;
    let provider = empty_provider_fixtures
        .semantic_providers
        .first_mut()
        .ok_or_else(|| io::Error::other("representative semantic provider is absent"))?;
    let empty_provider_id = provider.provider_id.clone();
    provider.fixture_ids.clear();
    require_validation_fragments(
        validate_registry_lock(&empty_provider_fixtures, &accepted),
        "empty semantic-provider fixture inventory",
        &[empty_provider_id.as_str(), "has no fixtures"],
    )?;

    let mut undeclared_pack = lock.clone();
    let parser = undeclared_pack
        .parser_components
        .first_mut()
        .ok_or_else(|| io::Error::other("registry has no parser ownership witness"))?;
    let undeclared_parser_id = parser.parser_id.clone();
    parser.current_pack_id = PackId("pack.missing".to_string());
    require_validation_fragments(
        validate_registry_lock(&undeclared_pack, &accepted),
        "undeclared parser pack ownership",
        &[
            undeclared_parser_id.as_str(),
            "undeclared pack",
            "pack.missing",
        ],
    )?;

    let mut wrong_mode_owner = materialize_accepted_target(serde_json::from_slice(ACCEPTED)?)?;
    let mode = wrong_mode_owner
        .modes
        .first_mut()
        .ok_or_else(|| io::Error::other("accepted target has no mode owner witness"))?;
    let wrong_owner_mode_id = mode.mode_id.clone();
    let owner_pack_id = mode.pack_id.clone();
    mode.owner = "undeclared-language-owner".to_string();
    require_validation_fragments(
        validate_accepted_target(&lock, &wrong_mode_owner),
        "accepted mode pack ownership",
        &[
            wrong_owner_mode_id.as_str(),
            "undeclared-language-owner",
            owner_pack_id.as_str(),
            "language owner",
        ],
    )?;

    let mut parser_ownership = materialize_accepted_target(serde_json::from_slice(ACCEPTED)?)?;
    let parser = parser_ownership
        .parsers
        .iter_mut()
        .find(|parser| parser.pack_id.as_str() == "default-core")
        .ok_or_else(|| io::Error::other("accepted target has no default-core parser witness"))?;
    let parser_ownership_id = parser.parser_id.clone();
    parser.pack_id = PackId("broad-language-pack".to_string());
    parser.owner = "broad-language-pack-worker".to_string();
    require_validation_fragments(
        validate_accepted_target(&lock, &parser_ownership),
        "accepted mode-to-parser pack ownership",
        &[
            parser_ownership_id.as_str(),
            "disagrees with its accepted modes",
        ],
    )?;

    let mut unsupported_tier = materialize_accepted_target(serde_json::from_slice(ACCEPTED)?)?;
    let mode = unsupported_tier
        .modes
        .first_mut()
        .ok_or_else(|| io::Error::other("accepted target has no tier witness"))?;
    let unsupported_tier_mode_id = mode.mode_id.clone();
    mode.required_claims = vec![CapabilityTier::Semantic];
    require_validation_fragments(
        validate_accepted_target(&lock, &unsupported_tier),
        "unsupported capability-tier claim",
        &[
            unsupported_tier_mode_id.as_str(),
            "required_claims",
            "supported ordered prefix",
        ],
    )?;

    let mut premature_achievement = materialize_accepted_target(serde_json::from_slice(ACCEPTED)?)?;
    let mode = premature_achievement
        .modes
        .first_mut()
        .ok_or_else(|| io::Error::other("accepted target has no achieved-tier witness"))?;
    let premature_mode_id = mode.mode_id.clone();
    mode.achieved_claims = vec![CapabilityTier::Detected];
    require_validation_fragments(
        validate_accepted_target(&lock, &premature_achievement),
        "premature accepted capability achievement",
        &[
            premature_mode_id.as_str(),
            "achieved_claims",
            "pending accepted target",
        ],
    )?;

    let mut aliased_delivery_mode = materialize_accepted_target(serde_json::from_slice(ACCEPTED)?)?;
    let alias_target = aliased_delivery_mode
        .modes
        .get(1)
        .ok_or_else(|| io::Error::other("accepted target has no alias target witness"))?
        .mode_id
        .clone();
    let mode = aliased_delivery_mode
        .modes
        .first_mut()
        .ok_or_else(|| io::Error::other("accepted target has no alias source witness"))?;
    let alias_source = mode.mode_id.clone();
    mode.alias_of = Some(alias_target.clone());
    require_validation_fragments(
        validate_accepted_target(&lock, &aliased_delivery_mode),
        "accepted canonical mode alias",
        &[
            alias_source.as_str(),
            "canonical delivery mode",
            alias_target.as_str(),
        ],
    )?;

    Ok(())
}

#[test]
fn registry_paths_reject_nonportable_and_colliding_names() -> Result<(), Box<dyn Error>> {
    require(
        RegistryPath::try_from("../escape".to_string()).is_err(),
        "parent traversal was accepted",
    )?;
    require(
        RegistryPath::try_from("fixtures/CON.txt".to_string()).is_err(),
        "Windows device path was accepted",
    )?;

    let mut lock: serde_json::Value = serde_json::from_slice(LOCK)?;
    lock["fixtures"][0]["path"] =
        json!("Fixtures/languages/projectatlas-v0.3.26-runtime-contract.toon");
    let bytes = serde_json::to_vec(&lock)?;
    let result = validate_and_generate(&bytes, &fixed_inputs());
    require(
        matches!(result, Err(LanguageRegistryError::Validation(message)) if message.contains("collide by ASCII case")),
        "case-colliding registry paths were accepted",
    )?;
    Ok(())
}

#[test]
fn filesystem_paths_reject_links_reparse_points_and_wrong_case() -> Result<(), Box<dyn Error>> {
    let wrong_case = tempfile::tempdir()?;
    fs::create_dir_all(wrong_case.path().join("Crates/projectatlas-core/src"))?;
    let workspace = RegistryWorkspace::new(wrong_case.path())?;
    let result = workspace.inspect_output(CORE_OUTPUT_PATH);
    require(
        matches!(result, Err(LanguageRegistryError::Validation(message)) if message.contains("wrong filesystem spelling")),
        "wrong-case filesystem spelling was accepted",
    )?;

    let linked = tempfile::tempdir()?;
    let parent = linked.path().join("crates/projectatlas-core/src");
    fs::create_dir_all(&parent)?;
    let target = linked.path().join("target.rs");
    fs::write(&target, b"target")?;
    let output = linked.path().join(CORE_OUTPUT_PATH);
    if let Err(source) = create_file_symlink(&target, &output) {
        #[cfg(windows)]
        {
            const ERROR_PRIVILEGE_NOT_HELD: i32 = 1_314;
            if source.raw_os_error() != Some(ERROR_PRIVILEGE_NOT_HELD) {
                return Err(source.into());
            }
            fs::create_dir(&output)?;
        }
        #[cfg(not(windows))]
        return Err(source.into());
    }
    let workspace = RegistryWorkspace::new(linked.path())?;
    let result = workspace.inspect_output(CORE_OUTPUT_PATH);
    require(
        matches!(result, Err(LanguageRegistryError::Validation(message)) if message.contains("link or reparse point") || message.contains("not a regular file")),
        "linked/reparse or non-regular output was accepted",
    )?;

    let linked_root_parent = tempfile::tempdir()?;
    let real_root = linked_root_parent.path().join("real-root");
    let linked_root = linked_root_parent.path().join("linked-root");
    fs::create_dir(&real_root)?;
    if let Err(source) = create_dir_symlink(&real_root, &linked_root) {
        #[cfg(windows)]
        {
            const ERROR_PRIVILEGE_NOT_HELD: i32 = 1_314;
            if source.raw_os_error() != Some(ERROR_PRIVILEGE_NOT_HELD) {
                return Err(source.into());
            }
        }
        #[cfg(not(windows))]
        return Err(source.into());
    } else {
        require(
            matches!(
                RegistryWorkspace::new(&linked_root),
                Err(LanguageRegistryError::Validation(message))
                    if message.contains("not a regular non-reparse directory")
            ),
            "linked/reparse registry root was accepted",
        )?;
    }
    Ok(())
}

#[cfg(windows)]
fn create_file_symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[cfg(unix)]
fn create_file_symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_dir_symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(unix)]
fn create_dir_symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[test]
fn check_reports_all_drift_without_mutating_the_workspace() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    seed_inputs(root.path())?;
    fs::write(root.path().join(CORE_OUTPUT_PATH), b"unchanged stale bytes")?;
    let before = fs::read(root.path().join(CORE_OUTPUT_PATH))?;
    let result = check(root.path());
    let Err(LanguageRegistryError::Drift(rows)) = result else {
        return Err(io::Error::other("check did not return structured drift").into());
    };
    require_equal(
        &rows.len(),
        &generated()?.entries().len(),
        "complete drift rows",
    )?;
    require_equal(
        &fs::read(root.path().join(CORE_OUTPUT_PATH))?,
        &before,
        "check preserved stale bytes",
    )?;
    require(
        matches!(
            fs::symlink_metadata(root.path().join(SYMBOLS_OUTPUT_PATH)),
            Err(error) if error.kind() == io::ErrorKind::NotFound
        ),
        "check created a missing output",
    )?;
    Ok(())
}

#[test]
fn write_validates_every_output_boundary_before_mutation() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    seed_inputs(root.path())?;
    fs::write(root.path().join(CORE_OUTPUT_PATH), b"preserve-before-write")?;
    fs::create_dir(root.path().join(EVIDENCE_OUTPUT_PATH))?;

    let result = write(root.path());
    require(
        matches!(result, Err(LanguageRegistryError::Validation(message)) if message.contains("not a regular file")),
        "invalid later output boundary did not fail closed",
    )?;
    require_equal(
        fs::read(root.path().join(CORE_OUTPUT_PATH))?.as_slice(),
        b"preserve-before-write",
        "earlier output before complete validation",
    )?;
    require(
        matches!(
            fs::symlink_metadata(root.path().join(SYMBOLS_OUTPUT_PATH)),
            Err(error) if error.kind() == io::ErrorKind::NotFound
        ),
        "write mutated an earlier missing output before complete validation",
    )?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn write_preserves_unix_output_mode_bits() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir()?;
    seed_inputs(root.path())?;
    let output = root.path().join(CORE_OUTPUT_PATH);
    fs::write(&output, b"stale")?;
    fs::set_permissions(&output, fs::Permissions::from_mode(0o640))?;

    write(root.path())?;
    let mode = fs::metadata(output)?.permissions().mode() & 0o777;
    require_equal(&mode, &0o640, "preserved Unix output mode")?;
    let new_mode = fs::metadata(root.path().join(SYMBOLS_OUTPUT_PATH))?
        .permissions()
        .mode()
        & 0o777;
    require_equal(&new_mode, &0o644, "new generated Unix output mode")?;
    Ok(())
}

#[test]
fn write_replaces_only_changed_fixed_outputs_after_complete_validation()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    seed_inputs(root.path())?;
    let expected = generated()?;
    let entries = expected.entries();
    fs::write(root.path().join(entries[0].path), entries[0].bytes)?;
    fs::write(root.path().join(entries[1].path), b"stale")?;

    write(root.path())?;
    for entry in expected.entries() {
        require_equal(
            fs::read(root.path().join(entry.path))?.as_slice(),
            entry.bytes,
            "written output bytes",
        )?;
    }
    check(root.path())?;
    Ok(())
}

#[test]
fn write_rolls_back_prior_replacements_after_a_late_failure() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    seed_inputs(root.path())?;
    let old = [
        b"old-core".as_slice(),
        b"",
        b"old-cli",
        b"old-evidence",
        b"old-documentation",
    ];
    for (index, relative) in [
        CORE_OUTPUT_PATH,
        SYMBOLS_OUTPUT_PATH,
        CLI_OUTPUT_PATH,
        EVIDENCE_OUTPUT_PATH,
        DOCUMENTATION_OUTPUT_PATH,
    ]
    .into_iter()
    .enumerate()
    {
        if index != 1 {
            fs::write(root.path().join(relative), old[index])?;
        }
    }
    let workspace = RegistryWorkspace::new(root.path())?;
    let mut prepared = Vec::new();
    for (index, relative) in [
        CORE_OUTPUT_PATH,
        SYMBOLS_OUTPUT_PATH,
        CLI_OUTPUT_PATH,
        EVIDENCE_OUTPUT_PATH,
        DOCUMENTATION_OUTPUT_PATH,
    ]
    .into_iter()
    .enumerate()
    {
        prepared.push(PreparedOutput::new(
            workspace.inspect_output(relative)?,
            format!("new-{index}").as_bytes(),
        )?);
    }
    let mut replacement = 0_usize;
    let inputs = workspace.read_inputs()?;
    let result = commit_prepared(&workspace, &inputs, prepared, |temporary, path| {
        replacement += 1;
        if replacement == 3 {
            Err(io::Error::other("injected late replacement failure"))
        } else {
            temporary
                .persist(path)
                .map(|_| ())
                .map_err(|error| error.error)
        }
    });
    require(
        matches!(result, Err(LanguageRegistryError::PersistOutput { ref source, ref rollback_failures, .. }) if source.to_string().contains("injected") && rollback_failures.is_empty()),
        "late failure was not reported with successful compensation",
    )?;
    require_equal(
        fs::read(root.path().join(CORE_OUTPUT_PATH))?.as_slice(),
        old[0],
        "existing first output rollback",
    )?;
    require(
        matches!(
            fs::symlink_metadata(root.path().join(SYMBOLS_OUTPUT_PATH)),
            Err(error) if error.kind() == io::ErrorKind::NotFound
        ),
        "previously absent output was not removed by rollback",
    )?;
    require_equal(
        fs::read(root.path().join(CLI_OUTPUT_PATH))?.as_slice(),
        old[2],
        "failed output remained unchanged",
    )?;
    require_equal(
        fs::read(root.path().join(EVIDENCE_OUTPUT_PATH))?.as_slice(),
        old[3],
        "unreached output remained unchanged",
    )?;
    Ok(())
}

#[test]
fn write_rolls_back_after_late_output_reinspection_failure() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    seed_inputs(root.path())?;
    fs::write(root.path().join(CORE_OUTPUT_PATH), b"old-core")?;

    let workspace = RegistryWorkspace::new(root.path())?;
    let mut prepared = Vec::new();
    for (index, relative) in [
        CORE_OUTPUT_PATH,
        SYMBOLS_OUTPUT_PATH,
        CLI_OUTPUT_PATH,
        EVIDENCE_OUTPUT_PATH,
        DOCUMENTATION_OUTPUT_PATH,
    ]
    .into_iter()
    .enumerate()
    {
        prepared.push(PreparedOutput::new(
            workspace.inspect_output(relative)?,
            format!("new-{index}").as_bytes(),
        )?);
    }

    let later_output = root.path().join(SYMBOLS_OUTPUT_PATH);
    let mut replacement = 0_usize;
    let inputs = workspace.read_inputs()?;
    let result = commit_prepared(&workspace, &inputs, prepared, |temporary, path| {
        replacement += 1;
        temporary
            .persist(path)
            .map(|_| ())
            .map_err(|error| error.error)?;
        if replacement == 1 {
            fs::create_dir(&later_output)?;
        }
        Ok(())
    });
    require(
        matches!(
            result,
            Err(LanguageRegistryError::PersistOutput {
                ref source,
                ref rollback_failures,
                ..
            }) if source.to_string().contains("not a regular file")
                && rollback_failures.is_empty()
        ),
        "late output reinspection failure did not report successful compensation",
    )?;
    require_equal(
        fs::read(root.path().join(CORE_OUTPUT_PATH))?.as_slice(),
        b"old-core",
        "earlier output after late reinspection failure",
    )?;
    Ok(())
}

#[test]
fn write_rolls_back_when_inputs_change_between_replacements() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    seed_inputs(root.path())?;
    fs::write(root.path().join(CORE_OUTPUT_PATH), b"old-core")?;

    let workspace = RegistryWorkspace::new(root.path())?;
    let inputs = workspace.read_inputs()?;
    let mut prepared = Vec::new();
    for (index, relative) in [
        CORE_OUTPUT_PATH,
        SYMBOLS_OUTPUT_PATH,
        CLI_OUTPUT_PATH,
        EVIDENCE_OUTPUT_PATH,
        DOCUMENTATION_OUTPUT_PATH,
    ]
    .into_iter()
    .enumerate()
    {
        prepared.push(PreparedOutput::new(
            workspace.inspect_output(relative)?,
            format!("new-{index}").as_bytes(),
        )?);
    }

    let lock_path = root.path().join(LOCK_PATH);
    let mut replacement = 0_usize;
    let result = commit_prepared(&workspace, &inputs, prepared, |temporary, path| {
        replacement += 1;
        temporary
            .persist(path)
            .map(|_| ())
            .map_err(|error| error.error)?;
        if replacement == 1 {
            fs::write(&lock_path, b"changed-after-validation")?;
        }
        Ok(())
    });
    require(
        matches!(
            result,
            Err(LanguageRegistryError::PersistOutput {
                ref source,
                ref rollback_failures,
                ..
            }) if source.to_string().contains("inputs changed after validation")
                && rollback_failures.is_empty()
        ),
        "input drift did not trigger successful compensation",
    )?;
    require_equal(
        fs::read(root.path().join(CORE_OUTPUT_PATH))?.as_slice(),
        b"old-core",
        "output after input-drift compensation",
    )?;
    Ok(())
}

#[test]
fn rollback_preserves_output_changed_after_this_write() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    seed_inputs(root.path())?;
    fs::write(root.path().join(CORE_OUTPUT_PATH), b"old-core")?;
    fs::write(root.path().join(SYMBOLS_OUTPUT_PATH), b"old-symbols")?;

    let workspace = RegistryWorkspace::new(root.path())?;
    let inputs = workspace.read_inputs()?;
    let mut prepared = Vec::new();
    for (index, relative) in [CORE_OUTPUT_PATH, SYMBOLS_OUTPUT_PATH]
        .into_iter()
        .enumerate()
    {
        prepared.push(PreparedOutput::new(
            workspace.inspect_output(relative)?,
            format!("new-{index}").as_bytes(),
        )?);
    }

    let core_output = root.path().join(CORE_OUTPUT_PATH);
    let mut replacement = 0_usize;
    let result = commit_prepared(&workspace, &inputs, prepared, |temporary, path| {
        replacement += 1;
        if replacement == 2 {
            fs::write(&core_output, b"concurrent-edit")?;
            Err(io::Error::other("injected later replacement failure"))
        } else {
            temporary
                .persist(path)
                .map(|_| ())
                .map_err(|error| error.error)
        }
    });
    require(
        matches!(
            result,
            Err(LanguageRegistryError::PersistOutput {
                ref source,
                ref rollback_failures,
                ..
            }) if source.to_string().contains("injected later replacement failure")
                && rollback_failures.iter().any(|failure| failure.contains("refusing compensating rollback"))
        ),
        "concurrent output change was not preserved and reported",
    )?;
    require_equal(
        fs::read(core_output)?.as_slice(),
        b"concurrent-edit",
        "concurrent output after refused compensation",
    )?;
    Ok(())
}

#[test]
fn write_reports_original_and_compensating_failures() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    seed_inputs(root.path())?;
    let old = [
        b"old-core".as_slice(),
        b"old-symbols",
        b"old-cli",
        b"old-evidence",
        b"old-documentation",
    ];
    let relatives = [
        CORE_OUTPUT_PATH,
        SYMBOLS_OUTPUT_PATH,
        CLI_OUTPUT_PATH,
        EVIDENCE_OUTPUT_PATH,
        DOCUMENTATION_OUTPUT_PATH,
    ];
    for (relative, bytes) in relatives.into_iter().zip(old) {
        fs::write(root.path().join(relative), bytes)?;
    }

    let workspace = RegistryWorkspace::new(root.path())?;
    let mut prepared = Vec::new();
    for (index, relative) in relatives.into_iter().enumerate() {
        prepared.push(PreparedOutput::new(
            workspace.inspect_output(relative)?,
            format!("new-{index}").as_bytes(),
        )?);
    }

    let mut replacement = 0_usize;
    let inputs = workspace.read_inputs()?;
    let result = commit_prepared(&workspace, &inputs, prepared, |temporary, path| {
        replacement += 1;
        match replacement {
            3 => Err(io::Error::other("injected original replacement failure")),
            4 => Err(io::Error::other(
                "injected compensating replacement failure",
            )),
            _ => temporary
                .persist(path)
                .map(|_| ())
                .map_err(|error| error.error),
        }
    });
    let Err(LanguageRegistryError::PersistOutput {
        source,
        rollback_failures,
        ..
    }) = result
    else {
        return Err(io::Error::other("combined replacement failure was not reported").into());
    };
    require(
        source
            .to_string()
            .contains("injected original replacement failure"),
        "original replacement failure was lost",
    )?;
    require(
        rollback_failures
            .iter()
            .any(|failure| failure.contains("injected compensating replacement failure")),
        "compensating replacement failure was lost",
    )?;
    require_equal(
        fs::read(root.path().join(CORE_OUTPUT_PATH))?.as_slice(),
        old[0],
        "successful reverse-order compensation",
    )?;
    Ok(())
}
