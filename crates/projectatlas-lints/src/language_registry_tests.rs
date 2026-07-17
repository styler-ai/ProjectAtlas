//! Focused unit coverage for language-registry validation, generation, and filesystem safety.

use super::*;
use serde_json::json;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fmt::Debug;
use std::process::Command;
use std::time::{Duration, Instant};
use syn::visit::Visit;

const LOCK: &[u8] = include_bytes!("../../../registry/language-registry.json");
const ACCEPTED: &[u8] =
    include_bytes!("../../../docs/benchmarks/projectatlas-v0.4-capability-registry.json");
const HISTORICAL: &[u8] =
    include_bytes!("../../../fixtures/languages/projectatlas-v0.3.26-runtime-contract.toon");
const PARSER_PACK_TRUST: &[u8] = include_bytes!("../../../registry/parser-pack-trust.json");
const REPOSITORY_INTELLIGENCE_CONTRACTS: &[u8] = include_bytes!(
    "../../../docs/benchmarks/projectatlas-v0.4-repository-intelligence-contracts.json"
);
const PARSER_REGISTRY: &str = include_str!("../../projectatlas-symbols/src/parser_registry.rs");
const SYMBOL_RUNTIME: &str = include_str!("../../projectatlas-symbols/src/lib.rs");

fn owned_inputs() -> Result<OwnedInputBytes, LanguageRegistryError> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    RegistryWorkspace::new(&root)?.read_inputs()
}

fn generated() -> Result<GeneratedArtifacts, LanguageRegistryError> {
    let inputs = owned_inputs()?;
    validate_and_generate(LOCK, &inputs.fixed())
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

fn decoded_parser_pack_contract() -> Result<(ParserPackTrustManifest, u64), Box<dyn Error>> {
    let trust = decode_parser_pack_trust(PARSER_PACK_TRUST)?;
    let budgets =
        serde_json::from_slice::<ParserPackBudgetDocument>(REPOSITORY_INTELLIGENCE_CONTRACTS)?;
    let installed_byte_limit = budgets
        .budgets
        .optional_pack_contract
        .accepted_pack_budgets
        .iter()
        .find(|budget| budget.pack_id.as_str() == BROAD_LANGUAGE_PACK_ID)
        .map(|budget| budget.limits.installed_bytes)
        .ok_or_else(|| io::Error::other("broad parser-pack budget is absent"))?;
    Ok((trust, installed_byte_limit))
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
    let baseline = owned_inputs()?;
    let fixed = FixedInputBytes {
        accepted_capability_registry: &accepted_bytes,
        historical_runtime_contract: HISTORICAL,
        parser_pack_trust: PARSER_PACK_TRUST,
        repository_intelligence_contracts: REPOSITORY_INTELLIGENCE_CONTRACTS,
        parser_pack_payloads: &baseline.parser_pack_payloads,
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
    let baseline = owned_inputs()?;
    let fixed = FixedInputBytes {
        accepted_capability_registry: ACCEPTED,
        historical_runtime_contract: historical_bytes,
        parser_pack_trust: PARSER_PACK_TRUST,
        repository_intelligence_contracts: REPOSITORY_INTELLIGENCE_CONTRACTS,
        parser_pack_payloads: &baseline.parser_pack_payloads,
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

fn accepted_pre_parse_transform_mut(
    accepted: &mut serde_json::Value,
) -> Result<&mut serde_json::Value, Box<dyn Error>> {
    accepted["modes"]
        .as_array_mut()
        .and_then(|modes| {
            modes
                .iter_mut()
                .find(|mode| mode["mode_id"] == OBJECTSCRIPT_EXPORT_XML_MODE_ID)
        })
        .and_then(|mode| mode.get_mut("pre_parse_transform"))
        .ok_or_else(|| io::Error::other("ObjectScript export transform fixture is absent").into())
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

fn require_duplicate_metadata_rejected<T>(
    bytes: &[u8],
    duplicate_member: &str,
    label: &'static str,
    context: &str,
) -> Result<(), Box<dyn Error>>
where
    for<'de> T: Deserialize<'de>,
{
    let duplicate = std::str::from_utf8(bytes)?.replacen('{', &format!("{{{duplicate_member}"), 1);
    require(
        matches!(
            decode_parser_pack_metadata::<T>(duplicate.as_bytes(), label),
            Err(LanguageRegistryError::JsonDecode { .. })
        ),
        format!("duplicate {context} keys were accepted"),
    )
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
        (PARSER_PACK_TRUST_PATH, PARSER_PACK_TRUST),
        (
            REPOSITORY_INTELLIGENCE_CONTRACTS_PATH,
            REPOSITORY_INTELLIGENCE_CONTRACTS,
        ),
    ] {
        let path = root.join(relative);
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("fixture input has no parent"))?;
        fs::create_dir_all(parent)?;
        fs::write(path, bytes)?;
    }
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for relative in [
        "fixtures/parser-packs/tree-sitter-wasm-grammar-pack/manifest.json",
        "fixtures/parser-packs/tree-sitter-wasm-grammar-pack/parsers/javascript.wasm",
        "fixtures/parser-packs/tree-sitter-wasm-grammar-pack/wasm-validation.json",
        "fixtures/parser-packs/tree-sitter-wasm-grammar-pack/wasm-probe.mjs",
        "fixtures/parser-packs/tree-sitter-wasm-grammar-pack/provenance.json",
        "fixtures/parser-packs/tree-sitter-wasm-grammar-pack/LICENSE",
        "fixtures/parser-packs/tree-sitter-wasm-grammar-pack/advisories.json",
        "fixtures/parser-packs/tree-sitter-wasm-grammar-pack/sbom.spdx.json",
    ] {
        let source = source_root.join(relative);
        let destination = root.join(relative);
        let parent = destination
            .parent()
            .ok_or_else(|| io::Error::other("parser-pack fixture has no parent"))?;
        fs::create_dir_all(parent)?;
        fs::copy(source, destination)?;
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
    let mut lock = base.clone();
    if !lock
        .detection_rules
        .iter()
        .any(|rule| matches!(rule, DetectionRule::Content { .. }))
    {
        for (detector_id, rule_id) in [
            (
                BuiltInContentDetector::ShebangPython,
                "detect.content.shebang-python",
            ),
            (
                BuiltInContentDetector::ShebangShell,
                "detect.content.shebang-shell",
            ),
            (
                BuiltInContentDetector::ShebangJavascript,
                "detect.content.shebang-javascript",
            ),
            (
                BuiltInContentDetector::ShebangRuby,
                "detect.content.shebang-ruby",
            ),
            (
                BuiltInContentDetector::ShebangPerl,
                "detect.content.shebang-perl",
            ),
            (
                BuiltInContentDetector::SignaturePhp,
                "detect.content.signature-php",
            ),
            (
                BuiltInContentDetector::SignatureXml,
                "detect.content.signature-xml",
            ),
            (
                BuiltInContentDetector::ContextDockerBuild,
                "detect.content.context-docker-build",
            ),
        ] {
            lock.detection_rules.push(DetectionRule::Content {
                id: DetectionRuleId::try_from(rule_id.to_string())?,
                detector_id,
                detector_kind: detector_id.detection_kind(),
                scanner_visible: false,
                mode_id: ModeId::try_from(detector_id.mode_id().to_string())?,
            });
        }
    }
    if lock.semantic_modes.is_empty() {
        for mode in SEMANTIC_MODES {
            lock.semantic_modes.push(SemanticModeRule {
                mode,
                base_mode_id: ModeId::try_from(mode.base_mode_id().to_string())?,
            });
        }
    }
    let content_rule = lock
        .detection_rules
        .iter()
        .find(|rule| {
            matches!(
                rule,
                DetectionRule::Content {
                    detector_id: BuiltInContentDetector::SignaturePhp,
                    ..
                }
            )
        })
        .cloned()
        .ok_or_else(|| io::Error::other("PHP content-detector witness is absent"))?;
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
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
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
        let command = bounded_generated_rust_command(&rustc)
            .arg("--edition=2024")
            .arg("--crate-type=lib")
            .arg("--emit=metadata")
            .arg("-Dwarnings")
            .arg("-Adead-code")
            .arg("--crate-name")
            .arg(format!("generated_registry_{index}"))
            .arg("--out-dir")
            .arg(root.path())
            .arg(&source_path);
        let output = run_bounded_generated_rust_process(
            &runtime,
            "generated registry standalone typecheck",
            &command,
        )?;
        require(
            output.stdout().is_empty() && output.stderr().is_empty(),
            format!(
                "successful standalone typecheck for generated Rust {} emitted output: stdout={:?}; stderr={:?}",
                artifact.path,
                output.stdout(),
                output.stderr()
            ),
        )?;
    }
    Ok(())
}

fn execute_generated_registry(source: &str) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let source_path = root.path().join("generated_registry_case_policy_probe.rs");
    let executable_path = root.path().join(format!(
        "generated_registry_case_policy_probe{}",
        env::consts::EXE_SUFFIX
    ));
    fs::write(&source_path, source)?;
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let compilation = bounded_generated_rust_command(&rustc)
        .arg("--edition=2024")
        .arg("--crate-type=bin")
        .arg("-Dwarnings")
        .arg("-Adead-code")
        .arg("--crate-name")
        .arg("generated_registry_case_policy_probe")
        .arg("-o")
        .arg(&executable_path)
        .arg(&source_path);
    let compilation = run_bounded_generated_rust_process(
        &runtime,
        "generated registry case-policy compilation",
        &compilation,
    )?;
    require(
        compilation.stdout().is_empty() && compilation.stderr().is_empty(),
        format!(
            "successful generated registry compilation emitted output: stdout={:?}; stderr={:?}",
            compilation.stdout(),
            compilation.stderr()
        ),
    )?;
    let execution = bounded_generated_rust_command(&executable_path);
    let execution = run_bounded_generated_rust_process(
        &runtime,
        "generated registry case-policy execution",
        &execution,
    )?;
    require(
        execution.stdout().is_empty() && execution.stderr().is_empty(),
        format!(
            "successful generated registry execution emitted output: stdout={:?}; stderr={:?}",
            execution.stdout(),
            execution.stderr()
        ),
    )?;
    Ok(())
}

fn path_is_exact(path: &syn::Path, expected: &[&str]) -> bool {
    path.leading_colon.is_none()
        && path.segments.len() == expected.len()
        && path
            .segments
            .iter()
            .zip(expected)
            .all(|(segment, expected)| segment.ident == *expected && segment.arguments.is_empty())
}

fn typed_value_argument_is_exact(argument: &syn::FnArg, name: &str, ty: &[&str]) -> bool {
    matches!(
        argument,
        syn::FnArg::Typed(argument)
            if matches!(
                argument.pat.as_ref(),
                syn::Pat::Ident(binding)
                    if binding.ident == name
                        && binding.by_ref.is_none()
                        && binding.mutability.is_none()
                        && binding.subpat.is_none()
            ) && matches!(
                argument.ty.as_ref(),
                syn::Type::Path(path)
                    if path.qself.is_none() && path_is_exact(&path.path, ty)
            )
    )
}

#[derive(Default)]
struct ClosedParserRuntimeVisitor {
    parser_language_bindings: usize,
    parser_language_installs: usize,
}

impl<'ast> Visit<'ast> for ClosedParserRuntimeVisitor {
    fn visit_local(&mut self, local: &'ast syn::Local) {
        let binds_compiled_language = matches!(
            &local.pat,
            syn::Pat::Ident(binding)
                if binding.ident == "parser_language"
                    && binding.by_ref.is_none()
                    && binding.mutability.is_none()
                    && binding.subpat.is_none()
        ) && local.init.as_ref().is_some_and(|init| {
            matches!(
                init.expr.as_ref(),
                syn::Expr::Call(call)
                    if matches!(
                        call.func.as_ref(),
                        syn::Expr::Path(path)
                            if path.qself.is_none()
                                && path_is_exact(
                                    &path.path,
                                    &["parser_registry", "parser_language"],
                                )
                    ) && call.args.len() == 1
                        && call.args.first().is_some_and(|argument| {
                            matches!(
                                argument,
                                syn::Expr::Path(path)
                                    if path.qself.is_none()
                                        && path_is_exact(&path.path, &["parser"])
                            )
                        })
            )
        });
        if binds_compiled_language {
            self.parser_language_bindings += 1;
        }
        syn::visit::visit_local(self, local);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let installs_compiled_language = call.method == "set_language"
            && call.args.len() == 1
            && matches!(
                call.receiver.as_ref(),
                syn::Expr::Path(path)
                    if path.qself.is_none() && path_is_exact(&path.path, &["parser"])
            )
            && call.args.first().is_some_and(|argument| {
                matches!(
                    argument,
                    syn::Expr::Reference(reference)
                        if reference.mutability.is_none()
                            && matches!(
                                reference.expr.as_ref(),
                                syn::Expr::Path(path)
                                    if path.qself.is_none()
                                        && path_is_exact(&path.path, &["parser_language"])
                            )
                )
            });
        if installs_compiled_language {
            self.parser_language_installs += 1;
        }
        syn::visit::visit_expr_method_call(self, call);
    }
}

fn expected_detection_row(
    lock: &LanguageRegistryLock,
    rule: &DetectionRule,
) -> Result<String, Box<dyn Error>> {
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
    let content_detector = match rule {
        DetectionRule::Content { detector_id, .. } => format!(
            "Some(LanguageContentDetector::{})",
            detector_id.rust_variant()
        ),
        DetectionRule::ExactFilename { .. }
        | DetectionRule::CompoundExtension { .. }
        | DetectionRule::Extension { .. } => "None".to_string(),
    };
    let public_mode = lock
        .current_modes
        .iter()
        .find(|mode| &mode.mode_id == rule.mode_id())
        .ok_or_else(|| io::Error::other("detection-rule current mode disappeared"))?;
    Ok(format!(
        "    LanguageDetectionRule {{ id: {}, stage: {stage}, pattern: {}, lookup_case: {lookup_case}, path_case: {path_case}, content_detector: {content_detector}, scanner_visible: {}, language: {} }},",
        rust_string(rule.id().as_str()),
        rust_string(rule.pattern()),
        rule.scanner_visible(),
        rust_string(public_mode.public_mode.as_str())
    ))
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
        "pub(crate) static LANGUAGE_REGISTRY_SETTINGS: LanguageRegistrySettings = LanguageRegistrySettings {{ registry_id: {}, accepted_registry_id: {}, accepted_set_sha256: {}, accepted_advertisement: AcceptedAdvertisement::BlockedUntilAchievedManifest, current_mode_count: {}, accepted_mode_count: {}, accepted_pre_parse_transform_count: {}, normalized_parser_capability_count: {}, parser_component_count: {}, parser_asset_count: {}, embedded_adapter_count: {}, query_pack_count: {}, semantic_provider_count: {} }};",
        rust_string(lock.registry_id.as_str()),
        rust_string(accepted.source.registry_id.as_str()),
        rust_string(accepted.source.accepted_set_digest.as_str()),
        lock.current_modes.len(),
        accepted.modes.len(),
        accepted
            .modes
            .iter()
            .filter(|mode| mode.pre_parse_transform.is_some())
            .count(),
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
                "pre_parse_transform": mode.pre_parse_transform,
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
                "pre_parse_transform": mode.pre_parse_transform,
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
                "pre_parse_transform": mode.pre_parse_transform,
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
    let (parser_pack_trust, parser_pack_installed_byte_limit) = decoded_parser_pack_contract()?;
    let contract_digest = registry_contract_digest(
        &representative,
        accepted,
        historical,
        &parser_pack_trust,
        parser_pack_installed_byte_limit,
    );
    let artifacts = render_generated_artifacts(
        &representative,
        accepted,
        historical,
        &parser_pack_trust,
        parser_pack_installed_byte_limit,
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
    let (parser_pack_trust, parser_pack_installed_byte_limit) = decoded_parser_pack_contract()?;
    let contract_digest = registry_contract_digest(
        lock,
        accepted,
        historical,
        &parser_pack_trust,
        parser_pack_installed_byte_limit,
    );
    Ok(render_generated_artifacts(
        lock,
        accepted,
        historical,
        &parser_pack_trust,
        parser_pack_installed_byte_limit,
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
            &expected_detection_row(lock, rule)?,
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
    for rule in &lock.detection_rules {
        let DetectionRule::Content {
            detector_id,
            mode_id,
            ..
        } = rule
        else {
            continue;
        };
        let public_mode = lock
            .current_modes
            .iter()
            .find(|mode| &mode.mode_id == mode_id)
            .ok_or_else(|| io::Error::other("content-detector current mode disappeared"))?;
        require_generated_row(
            core,
            &format!(
                "        LanguageContentDetector::{} => {},",
                detector_id.rust_variant(),
                rust_string(public_mode.public_mode.as_str())
            ),
            "generated content-detector routing",
        )?;
    }
    let mut semantic_variants_by_base = BTreeMap::<&str, Vec<&str>>::new();
    for semantic_mode in &lock.semantic_modes {
        let base_mode = lock
            .current_modes
            .iter()
            .find(|mode| mode.mode_id == semantic_mode.base_mode_id)
            .ok_or_else(|| io::Error::other("semantic-mode base language disappeared"))?;
        semantic_variants_by_base
            .entry(base_mode.public_mode.as_str())
            .or_default()
            .push(semantic_mode.mode.rust_variant());
    }
    for (base_language, semantic_variants) in semantic_variants_by_base {
        require_generated_row(
            core,
            &format!(
                "            {} => base_language == {},",
                semantic_variants
                    .iter()
                    .map(|variant| format!("Self::{variant}"))
                    .collect::<Vec<_>>()
                    .join(" | "),
                rust_string(base_language)
            ),
            "generated semantic-mode compatibility",
        )?;
    }
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
    parser_pack_trust: &ParserPackTrustManifest,
    parser_pack_installed_byte_limit: u64,
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
            "semantic_modes": &lock.semantic_modes,
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
            "pre_parse_transforms": accepted.modes.iter().filter(|mode| mode.pre_parse_transform.is_some()).count(),
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
        "parser_pack_trust": {
            "format": "projectatlas.parser-pack-trust",
            "path": lock.parser_pack_trust.path.as_str(),
            "raw_sha256": lock.parser_pack_trust.raw_sha256.as_str(),
            "installed_byte_limit": parser_pack_installed_byte_limit,
            "selection_state": "blocked",
            "selected_candidate": null,
            "achieved_manifest": null,
            "candidates": &parser_pack_trust.candidates
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
            "accepted_pre_parse_transforms": accepted.modes.iter().filter(|mode| mode.pre_parse_transform.is_some()).count(),
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
        "parser_pack_trust": {
            "format": "projectatlas.parser-pack-trust",
            "path": lock.parser_pack_trust.path.as_str(),
            "raw_sha256": lock.parser_pack_trust.raw_sha256.as_str(),
            "installed_byte_limit": parser_pack_installed_byte_limit,
            "selection_state": "blocked",
            "selected_candidate": null,
            "achieved_manifest": null,
            "candidates": &parser_pack_trust.candidates
        },
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
fn language_registry_contract_is_closed_and_semantically_bound() -> Result<(), Box<dyn Error>> {
    let (lock, accepted, historical) = decoded_contracts()?;
    let (parser_pack_trust, parser_pack_installed_byte_limit) = decoded_parser_pack_contract()?;
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
        &parser_pack_trust.schema_version,
        &PARSER_PACK_TRUST_SCHEMA_VERSION,
        "parser-pack trust schema version",
    )?;
    require_equal(
        &lock.parser_pack_trust.path.as_str(),
        &PARSER_PACK_TRUST_PATH,
        "parser-pack trust lock path",
    )?;
    let parser_pack_trust_raw_sha256 = sha256_hex(PARSER_PACK_TRUST);
    require_equal(
        lock.parser_pack_trust.raw_sha256.as_str(),
        parser_pack_trust_raw_sha256.as_str(),
        "parser-pack trust raw digest",
    )?;
    let [candidate] = parser_pack_trust.candidates.as_slice() else {
        return Err(io::Error::other("expected one parser-pack trust candidate").into());
    };
    let trusted_installed_bytes = candidate
        .inventory
        .iter()
        .map(|file| file.bytes)
        .sum::<u64>();
    require(
        candidate.eligibility == ParserPackCandidateEligibility::EvaluationOnlyUnselected
            && !candidate.advertised
            && candidate.pack_id.as_str() == BROAD_LANGUAGE_PACK_ID
            && candidate.pack_abi.abi_id.as_str() == PROJECTATLAS_PACK_ABI_ID
            && candidate.pack_abi.version == PROJECTATLAS_PACK_ABI_VERSION
            && candidate.grammar_abi.abi_id.as_str() == TREE_SITTER_WASM_ABI_ID
            && candidate.grammar_abi.version == TREE_SITTER_WASM_ABI_VERSION
            && candidate.grammar_abi.state == ParserPackAbiState::PendingPackVerification
            && candidate.packaged_platform.as_str() == TREE_SITTER_WASM_PLATFORM
            && candidate.installed_bytes == trusted_installed_bytes
            && candidate.installed_bytes < parser_pack_installed_byte_limit,
        "parser-pack candidate lost its evaluation-only parser ABI or isolated budget binding",
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

    let baseline_digest = registry_contract_digest(
        &lock,
        &accepted,
        &historical,
        &parser_pack_trust,
        parser_pack_installed_byte_limit,
    );
    let representative_digest = registry_contract_digest(
        &representative,
        &accepted,
        &historical,
        &parser_pack_trust,
        parser_pack_installed_byte_limit,
    );
    require(
        baseline_digest != representative_digest,
        "representative 5.1 fields did not participate in the semantic digest",
    )?;
    require_equal(
        &representative_digest,
        &registry_contract_digest(
            &representative,
            &accepted,
            &historical,
            &parser_pack_trust,
            parser_pack_installed_byte_limit,
        ),
        "semantic digest determinism",
    )?;
    let mut changed_parser_pack_trust = parser_pack_trust.clone();
    changed_parser_pack_trust.candidates[0].installed_bytes += 1;
    require(
        registry_contract_digest(
            &representative,
            &accepted,
            &historical,
            &changed_parser_pack_trust,
            parser_pack_installed_byte_limit,
        ) != representative_digest,
        "parser-pack trust was absent from the semantic registry digest",
    )?;

    let mut changed_content = representative.clone();
    let changed_content_rule = changed_content
        .detection_rules
        .iter_mut()
        .find(|rule| rule.id() == content_rule.id())
        .ok_or_else(|| io::Error::other("content-detector digest witness disappeared"))?;
    let DetectionRule::Content { detector_id, .. } = changed_content_rule else {
        return Err(io::Error::other("content-detector digest witness changed kind").into());
    };
    *detector_id = BuiltInContentDetector::SignatureXml;
    require(
        registry_contract_digest(
            &changed_content,
            &accepted,
            &historical,
            &parser_pack_trust,
            parser_pack_installed_byte_limit,
        ) != representative_digest,
        "content detector identity was absent from the semantic digest",
    )?;
    let mut changed_semantic_mode = representative.clone();
    changed_semantic_mode
        .semantic_modes
        .first_mut()
        .ok_or_else(|| io::Error::other("semantic-mode digest witness disappeared"))?
        .base_mode_id = ModeId::try_from("mode.json".to_string())?;
    require(
        registry_contract_digest(
            &changed_semantic_mode,
            &accepted,
            &historical,
            &parser_pack_trust,
            parser_pack_installed_byte_limit,
        ) != representative_digest,
        "semantic-mode compatibility was absent from the semantic digest",
    )?;
    let mut changed_asset_source = representative.clone();
    if let Some(asset) = changed_asset_source.assets.last_mut() {
        asset.source.0.push_str("/mirror");
    }
    require(
        registry_contract_digest(
            &changed_asset_source,
            &accepted,
            &historical,
            &parser_pack_trust,
            parser_pack_installed_byte_limit,
        ) != representative_digest,
        "asset source was absent from the semantic digest",
    )?;
    let mut changed_asset_version = representative.clone();
    if let Some(asset) = changed_asset_version.assets.last_mut() {
        asset.version.0.push_str("-reviewed");
    }
    require(
        registry_contract_digest(
            &changed_asset_version,
            &accepted,
            &historical,
            &parser_pack_trust,
            parser_pack_installed_byte_limit,
        ) != representative_digest,
        "asset version was absent from the semantic digest",
    )?;
    let mut changed_asset_abi_version = representative.clone();
    if let Some(asset) = changed_asset_abi_version.assets.last_mut() {
        asset.abi.version += 1;
    }
    require(
        registry_contract_digest(
            &changed_asset_abi_version,
            &accepted,
            &historical,
            &parser_pack_trust,
            parser_pack_installed_byte_limit,
        ) != representative_digest,
        "asset ABI version was absent from the semantic digest",
    )?;
    let mut changed_embedded = representative.clone();
    if let Some(adapter) = changed_embedded.embedded_adapters.last_mut() {
        std::mem::swap(&mut adapter.host_mode_id, &mut adapter.embedded_mode_id);
    }
    require(
        registry_contract_digest(
            &changed_embedded,
            &accepted,
            &historical,
            &parser_pack_trust,
            parser_pack_installed_byte_limit,
        ) != representative_digest,
        "embedded adapter mapping was absent from the semantic digest",
    )?;
    let mut changed_tiers = representative.clone();
    changed_tiers.capability_tiers.swap(0, 1);
    require(
        registry_contract_digest(
            &changed_tiers,
            &accepted,
            &historical,
            &parser_pack_trust,
            parser_pack_installed_byte_limit,
        ) != representative_digest,
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
fn duplicate_object_keys_are_rejected_before_typed_decode() -> Result<(), Box<dyn Error>> {
    let source = String::from_utf8_lossy(LOCK);
    let duplicate = source.replacen('{', "{\"schema_version\":1,", 1);
    let inputs = owned_inputs()?;
    let result = validate_and_generate(duplicate.as_bytes(), &inputs.fixed());
    require(
        matches!(result, Err(LanguageRegistryError::JsonDecode { .. })),
        "duplicate registry object keys were accepted",
    )?;
    Ok(())
}

#[test]
fn unknown_nested_fields_are_rejected() -> Result<(), Box<dyn Error>> {
    let mut lock: serde_json::Value = serde_json::from_slice(LOCK)?;
    lock["packs"][0]["unexpected"] = json!(true);
    let bytes = serde_json::to_vec(&lock)?;
    let inputs = owned_inputs()?;
    let result = validate_and_generate(&bytes, &inputs.fixed());
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
fn accepted_pre_parse_transform_is_fail_closed_and_digest_bound() -> Result<(), Box<dyn Error>> {
    let missing = validate_accepted_mutation(|accepted| {
        let mode = accepted["modes"]
            .as_array_mut()
            .and_then(|modes| {
                modes
                    .iter_mut()
                    .find(|mode| mode["mode_id"] == OBJECTSCRIPT_EXPORT_XML_MODE_ID)
            })
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| io::Error::other("ObjectScript export mode fixture is absent"))?;
        mode.remove("pre_parse_transform");
        Ok(())
    })?;
    require(
        matches!(missing, Err(LanguageRegistryError::Validation(_))),
        "missing ObjectScript export transform was accepted",
    )?;

    let extra = validate_accepted_mutation(|accepted| {
        let transform = accepted_pre_parse_transform_mut(accepted)?.clone();
        let routine = accepted["modes"]
            .as_array_mut()
            .and_then(|modes| {
                modes
                    .iter_mut()
                    .find(|mode| mode["mode_id"] == "mode.objectscript-routine")
            })
            .ok_or_else(|| io::Error::other("ObjectScript routine fixture is absent"))?;
        routine["pre_parse_transform"] = transform;
        Ok(())
    })?;
    require(
        matches!(extra, Err(LanguageRegistryError::Validation(_))),
        "an extra ObjectScript transform owner was accepted",
    )?;

    for (pointer, replacement) in [
        (
            "/transform_id",
            json!("transform.objectscript-export-xml-to-udl-v2"),
        ),
        ("/version", json!(2)),
        ("/behavior", json!("unknown-behavior")),
        ("/deterministic", json!(false)),
        ("/target_mode_id", json!("mode.objectscript-udl")),
        ("/target_parser_id", json!("parse.objectscript-routine")),
        ("/detection_ownership", json!("after-transform")),
        ("/detection_rule_id", json!("detect.objectscript-udl")),
        ("/limits/max_input_bytes", json!(1)),
        ("/limits/max_derived_output_bytes", json!(1)),
        ("/limits/max_records", json!(1)),
        ("/limits/max_nesting_depth", json!(1)),
        ("/limits/max_diagnostics", json!(1)),
        ("/limits/deadline_ms", json!(1)),
        ("/cancellation/enabled", json!(false)),
        ("/cancellation/poll_interval_ms", json!(1)),
        ("/cancellation/grace_period_ms", json!(1)),
        ("/source_mapping/original_file_identity", json!(false)),
        ("/source_mapping/per_record_provenance", json!(false)),
        ("/source_mapping/every_derived_fact", json!(false)),
        ("/source_mapping/every_diagnostic", json!(false)),
        ("/security/dtd", json!("allowed")),
        ("/security/entity_expansion", json!("allowed")),
        ("/security/external_resources", json!("allowed")),
        ("/security/schema_fetch", json!("allowed")),
        ("/security/execution", json!("allowed")),
        (
            "/failure_policy/empty_input",
            json!("partial-or-unavailable"),
        ),
        ("/failure_policy/malformed_input", json!("unavailable")),
        (
            "/failure_policy/oversized_input",
            json!("partial-or-unavailable"),
        ),
        (
            "/failure_policy/deeply_nested_input",
            json!("partial-or-unavailable"),
        ),
        (
            "/failure_policy/multi_record_input",
            json!("parse-first-only"),
        ),
        ("/failure_policy/unrelated_parser_fallback", json!(true)),
        ("/failure_policy/guessed_symbols_after_failure", json!(true)),
        (
            "/failure_policy/coverage_after_failure",
            json!("unavailable"),
        ),
    ] {
        let result = validate_accepted_mutation(|accepted| {
            let field = accepted_pre_parse_transform_mut(accepted)?
                .pointer_mut(pointer)
                .ok_or_else(|| {
                    io::Error::other(format!("transform pointer {pointer} is absent"))
                })?;
            *field = replacement;
            Ok(())
        })?;
        require(
            result.is_err(),
            format!("execution-significant transform field {pointer} was accepted after drift"),
        )?;
    }

    let (lock, mut accepted, historical) = decoded_contracts()?;
    let (parser_pack_trust, parser_pack_installed_byte_limit) = decoded_parser_pack_contract()?;
    let accepted_digest = accepted_set_digest(&accepted)?;
    let composite_digest = registry_contract_digest(
        &lock,
        &accepted,
        &historical,
        &parser_pack_trust,
        parser_pack_installed_byte_limit,
    );
    let transform = accepted
        .modes
        .iter_mut()
        .find(|mode| mode.mode_id.as_str() == OBJECTSCRIPT_EXPORT_XML_MODE_ID)
        .and_then(|mode| mode.pre_parse_transform.as_mut())
        .ok_or_else(|| io::Error::other("materialized ObjectScript export transform is absent"))?;
    transform.failure_policy.empty_input = AcceptedTransformFailureCoverage::PartialOrUnavailable;
    require(
        accepted_set_digest(&accepted)? != accepted_digest
            && registry_contract_digest(
                &lock,
                &accepted,
                &historical,
                &parser_pack_trust,
                parser_pack_installed_byte_limit,
            ) != composite_digest,
        "transform values were absent from accepted or composite semantic identity",
    )
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
fn external_contract_digests_and_identities_are_bound() -> Result<(), Box<dyn Error>> {
    let mut accepted = ACCEPTED.to_vec();
    accepted.push(b'\n');
    let baseline = owned_inputs()?;
    let fixed = FixedInputBytes {
        accepted_capability_registry: &accepted,
        historical_runtime_contract: HISTORICAL,
        parser_pack_trust: PARSER_PACK_TRUST,
        repository_intelligence_contracts: REPOSITORY_INTELLIGENCE_CONTRACTS,
        parser_pack_payloads: &baseline.parser_pack_payloads,
    };
    let result = validate_and_generate(LOCK, &fixed);
    require(
        matches!(result, Err(LanguageRegistryError::Validation(_))),
        "external contract digest drift was accepted",
    )?;
    Ok(())
}

#[test]
fn count_preserving_mode_remaps_are_rejected() -> Result<(), Box<dyn Error>> {
    let mut lock: serde_json::Value = serde_json::from_slice(LOCK)?;
    let replacement = lock["current_modes"][1]["accepted_mode_id"].clone();
    lock["current_modes"][0]["accepted_mode_id"] = replacement;
    let bytes = serde_json::to_vec(&lock)?;
    let inputs = owned_inputs()?;
    let result = validate_and_generate(&bytes, &inputs.fixed());
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
        let inputs = owned_inputs()?;
        let result = validate_and_generate(&bytes, &inputs.fixed());
        require(
            matches!(result, Err(LanguageRegistryError::Validation(message)) if message.contains("ambiguously claim")),
            format!("mixed case-policy overlap was accepted with reverse={reverse}"),
        )?;
    }
    Ok(())
}

#[test]
fn compound_path_case_overlaps_are_rejected_in_both_orders() -> Result<(), Box<dyn Error>> {
    for reverse in [false, true] {
        let mut lock: serde_json::Value = serde_json::from_slice(LOCK)?;
        let rules = lock["detection_rules"]
            .as_array_mut()
            .ok_or_else(|| io::Error::other("detection rules are not an array"))?;
        let compound_index = rules
            .iter()
            .position(|rule| rule["extension"] == ".d.ts")
            .ok_or_else(|| io::Error::other("D.TS compound rule is absent"))?;
        rules[compound_index]["case"] = json!("sensitive");
        rules[compound_index]["path_suffix_case"] = json!("ascii-insensitive");
        let mut conflicting = rules[compound_index].clone();
        conflicting["id"] = json!("detect.extension.d-ts-path-case-probe");
        conflicting["extension"] = json!(".D.TS");
        rules.push(conflicting);
        if reverse {
            rules.reverse();
        }
        let bytes = serde_json::to_vec(&lock)?;
        let inputs = owned_inputs()?;
        let result = validate_and_generate(&bytes, &inputs.fixed());
        require(
            matches!(result, Err(LanguageRegistryError::Validation(message)) if message.contains("ambiguously claim")),
            format!("compound path-case overlap was accepted with reverse={reverse}"),
        )?;
    }
    Ok(())
}

#[test]
fn generated_matchers_execute_case_policies() -> Result<(), Box<dyn Error>> {
    let (mut lock, accepted, _) = decoded_contracts()?;
    let mut sensitive_extension_mode = None;
    let mut insensitive_filename_mode = None;
    let mut compound_mode = None;
    for rule in &mut lock.detection_rules {
        match rule {
            DetectionRule::Extension {
                extension,
                case,
                mode_id,
                ..
            } if extension == ".zon" => {
                *case = CasePolicy::Sensitive;
                sensitive_extension_mode = Some(mode_id.as_str().to_string());
            }
            DetectionRule::ExactFilename {
                file_name,
                case,
                mode_id,
                ..
            } if file_name == "Makefile" => {
                *case = CasePolicy::AsciiInsensitive;
                insensitive_filename_mode = Some(mode_id.as_str().to_string());
            }
            DetectionRule::CompoundExtension {
                extension, mode_id, ..
            } if extension == ".d.ts" => {
                compound_mode = Some(mode_id.as_str().to_string());
            }
            DetectionRule::ExactFilename { .. }
            | DetectionRule::CompoundExtension { .. }
            | DetectionRule::Extension { .. }
            | DetectionRule::Content { .. } => {}
        }
    }
    let sensitive_extension_mode =
        sensitive_extension_mode.ok_or_else(|| io::Error::other("ZON extension rule is absent"))?;
    let insensitive_filename_mode = insensitive_filename_mode
        .ok_or_else(|| io::Error::other("Makefile exact-filename rule is absent"))?;
    let compound_mode =
        compound_mode.ok_or_else(|| io::Error::other("D.TS compound rule is absent"))?;
    validate_registry_lock(&lock, &accepted)?;

    let public_mode = |mode_id: &str| {
        lock.current_modes
            .iter()
            .find(|mode| mode.mode_id.as_str() == mode_id)
            .map(|mode| mode.public_mode.as_str())
            .ok_or_else(|| io::Error::other(format!("current mode {mode_id} is absent")))
    };
    let sensitive_extension_mode = public_mode(&sensitive_extension_mode)?;
    let insensitive_filename_mode = public_mode(&insensitive_filename_mode)?;
    let compound_mode = public_mode(&compound_mode)?;
    let rendered = render_core_registry(&lock, "source-lock", "registry-contract")?;
    let executable = format!(
        "{rendered}\n\
         fn main() {{\n\
             assert_eq!(detect_extension(\".zon\"), Some({}));\n\
             assert_eq!(detect_extension(\".ZON\"), None);\n\
             assert_eq!(detect_extension(\".projectatlas-case-policy-probe\"), None);\n\
             assert_eq!(detect_exact_filename(\"Makefile\"), Some({}));\n\
             assert_eq!(detect_exact_filename(\"makefile\"), Some({}));\n\
             assert_eq!(detect_exact_filename(\"ProjectAtlasCasePolicyProbe\"), None);\n\
             assert_eq!(detect_compound_extension(\"types.d.ts\", Some(\".projectatlas-case-policy-probe\")), Some({}));\n\
             assert_eq!(detect_compound_extension(\"types.D.TS\", None), None);\n\
             assert_eq!(detect_compound_extension(\"types.D.TS\", Some(\".D.TS\")), Some({}));\n\
             assert_eq!(detect_compound_extension(\"types.D.TS\", Some(\".projectatlas-case-policy-probe\")), None);\n\
         }}\n",
        rust_string(sensitive_extension_mode),
        rust_string(insensitive_filename_mode),
        rust_string(insensitive_filename_mode),
        rust_string(compound_mode),
        rust_string(compound_mode),
    );
    execute_generated_registry(&executable)
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
fn representative_content_detector_bindings_are_closed() -> Result<(), Box<dyn Error>> {
    let (base, accepted, _) = decoded_contracts()?;
    validate_registry_lock(&base, &accepted)?;
    require(
        !base
            .detection_rules
            .iter()
            .any(|rule| matches!(rule, DetectionRule::Content { .. }))
            && base.semantic_modes.is_empty(),
        "current registry unexpectedly delivers representative content or semantic routing",
    )?;
    let lock = representative_registry_witness(&base)?.lock;
    validate_registry_lock(&lock, &accepted)?;

    let content_rules = lock
        .detection_rules
        .iter()
        .filter_map(|rule| match rule {
            DetectionRule::Content {
                detector_id,
                detector_kind,
                mode_id,
                ..
            } => Some((*detector_id, *detector_kind, mode_id)),
            DetectionRule::ExactFilename { .. }
            | DetectionRule::CompoundExtension { .. }
            | DetectionRule::Extension { .. } => None,
        })
        .collect::<Vec<_>>();
    require_equal(
        &content_rules.len(),
        &BUILT_IN_CONTENT_DETECTORS.len(),
        "closed content-detector inventory size",
    )?;
    for (detector, kind, mode_id) in content_rules {
        require_equal(
            &kind,
            &detector.detection_kind(),
            "content-detector precedence stage",
        )?;
        require_equal(
            &mode_id.as_str(),
            &detector.mode_id(),
            "content-detector selected mode",
        )?;
    }
    require_equal(
        &lock.semantic_modes.len(),
        &SEMANTIC_MODES.len(),
        "closed semantic-mode inventory size",
    )?;
    for semantic_mode in &lock.semantic_modes {
        require_equal(
            &semantic_mode.base_mode_id.as_str(),
            &semantic_mode.mode.base_mode_id(),
            "semantic-mode compatible base",
        )?;
    }

    let unknown_detector = serde_json::from_value::<DetectionRule>(json!({
        "layer": "content",
        "id": "detect.content.unknown",
        "detector_id": "content.unknown",
        "detector_kind": "content-signature",
        "scanner_visible": false,
        "mode_id": "mode.xml"
    }));
    require(
        matches!(
            &unknown_detector,
            Err(source)
                if source.to_string().contains("unknown variant `content.unknown`")
        ),
        format!(
            "unknown content detector did not fail typed decoding: {:?}",
            unknown_detector.as_ref().err()
        ),
    )?;

    let mut wrong_kind = lock.clone();
    let wrong_kind_rule = wrong_kind
        .detection_rules
        .iter_mut()
        .find(|rule| {
            matches!(
                rule,
                DetectionRule::Content {
                    detector_id: BuiltInContentDetector::SignaturePhp,
                    ..
                }
            )
        })
        .ok_or_else(|| io::Error::other("PHP detector-kind witness is absent"))?;
    let DetectionRule::Content { detector_kind, .. } = wrong_kind_rule else {
        return Err(io::Error::other("PHP detector-kind witness changed kind").into());
    };
    *detector_kind = ContentDetectionKind::Shebang;
    require_validation_fragments(
        validate_registry_lock(&wrong_kind, &accepted),
        "content-detector precedence stage",
        &[
            "content.signature.php",
            "requires content-signature",
            "declared as shebang",
        ],
    )?;

    let mut wrong_mode = lock.clone();
    let wrong_mode_rule = wrong_mode
        .detection_rules
        .iter_mut()
        .find(|rule| {
            matches!(
                rule,
                DetectionRule::Content {
                    detector_id: BuiltInContentDetector::SignaturePhp,
                    ..
                }
            )
        })
        .ok_or_else(|| io::Error::other("PHP detector-mode witness is absent"))?;
    let DetectionRule::Content { mode_id, .. } = wrong_mode_rule else {
        return Err(io::Error::other("PHP detector-mode witness changed kind").into());
    };
    *mode_id = ModeId::try_from("mode.xml".to_string())?;
    require_validation_fragments(
        validate_registry_lock(&wrong_mode, &accepted),
        "content-detector selected mode",
        &[
            "content.signature.php",
            "requires mode mode.php",
            "declared for mode.xml",
        ],
    )?;

    let mut missing_detector = lock.clone();
    missing_detector.detection_rules.retain(|rule| {
        !matches!(
            rule,
            DetectionRule::Content {
                detector_id: BuiltInContentDetector::SignaturePhp,
                ..
            }
        )
    });
    require_validation_fragments(
        validate_registry_lock(&missing_detector, &accepted),
        "missing content detector",
        &[
            "built-in content-detector inventory mismatch",
            "SignaturePhp",
        ],
    )?;

    let unknown_semantic_mode = serde_json::from_value::<SemanticModeRule>(json!({
        "mode": "helm",
        "base_mode_id": "mode.yaml"
    }));
    require(
        matches!(
            &unknown_semantic_mode,
            Err(source) if source.to_string().contains("unknown variant `helm`")
        ),
        format!(
            "unknown semantic mode did not fail typed decoding: {:?}",
            unknown_semantic_mode.as_ref().err()
        ),
    )?;

    let mut wrong_semantic_base = lock.clone();
    wrong_semantic_base
        .semantic_modes
        .first_mut()
        .ok_or_else(|| io::Error::other("semantic-mode base witness is absent"))?
        .base_mode_id = ModeId::try_from("mode.json".to_string())?;
    require_validation_fragments(
        validate_registry_lock(&wrong_semantic_base, &accepted),
        "semantic-mode compatible base",
        &[
            "semantic mode kubernetes",
            "requires base mode mode.yaml",
            "declared for mode.json",
        ],
    )?;

    let mut missing_semantic_mode = lock.clone();
    let _removed = missing_semantic_mode.semantic_modes.pop();
    require_validation_fragments(
        validate_registry_lock(&missing_semantic_mode, &accepted),
        "missing semantic mode",
        &["semantic-mode inventory mismatch", "Kustomize"],
    )?;

    let mut duplicate_semantic_mode = lock.clone();
    let duplicate = duplicate_semantic_mode
        .semantic_modes
        .first()
        .ok_or_else(|| io::Error::other("semantic-mode duplicate witness is absent"))?
        .clone();
    duplicate_semantic_mode.semantic_modes.push(duplicate);
    require_validation_fragments(
        validate_registry_lock(&duplicate_semantic_mode, &accepted),
        "duplicate semantic mode",
        &["duplicate semantic mode kubernetes"],
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
const FORMATTER_TIMEOUT_SENTINEL: &str = "formatter timeout fixture sentinel";
const FORMATTER_FAILURE_SENTINEL: &str = "formatter failure fixture sentinel";

fn formatter_fixture_command(test_name: &str) -> Result<ProcessCommand, Box<dyn Error>> {
    Ok(bounded_generated_rust_command(env::current_exe()?).args([
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
        Err(LanguageRegistryError::GeneratedRustProcess { detail, .. }) => Ok(detail),
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
        &Some(GENERATED_RUST_PROCESS_TIMEOUT),
        "pinned formatter timeout",
    )?;
    require_equal(
        &GENERATED_RUST_PROCESS_TIMEOUT,
        &Duration::from_secs(30),
        "literal generated Rust process timeout contract",
    )?;
    require_equal(
        &GENERATED_RUST_PROCESS_STREAM_LIMIT_BYTES,
        &65_536,
        "literal generated Rust stream-retention contract",
    )?;

    let missing = tempfile::tempdir()?
        .path()
        .join(format!("missing-rustfmt{}", std::env::consts::EXE_SUFFIX));
    let missing_command = bounded_generated_rust_command(missing);
    let missing_detail = require_formatter_error(
        formatter.run("missing formatter fixture", &missing_command),
        "missing formatter fixture",
    )?;
    require(
        missing_detail.contains("missing-rustfmt") && missing_detail.contains("not found"),
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
        timeout_detail.contains("configured timeout")
            && timeout_detail.contains(FORMATTER_TIMEOUT_SENTINEL),
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
        output_detail.contains("65536 retained bytes per stream"),
        format!("formatter output truncation was not classified: {output_detail}"),
    )?;

    let failure_command = formatter_fixture_command(FORMATTER_FAILURE_FIXTURE)?;
    let failure_detail = require_formatter_error(
        formatter.run("formatter failure fixture", &failure_command),
        "formatter failure fixture",
    )?;
    require(
        failure_detail.contains("exited with code 101")
            && failure_detail.contains(FORMATTER_FAILURE_SENTINEL),
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
    let mut stderr = io::stderr().lock();
    stderr.write_all(FORMATTER_TIMEOUT_SENTINEL.as_bytes())?;
    stderr.flush()?;
    drop(stderr);
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
    let bytes = vec![b'x'; GENERATED_RUST_PROCESS_STREAM_LIMIT_BYTES + 1_024];
    let mut stdout = io::stdout().lock();
    stdout.write_all(&bytes)?;
    stdout.flush()?;
    Ok(())
}

#[test]
#[ignore = "process fixture invoked by the ARRI 5.2 status-diagnostic test"]
fn formatter_failure_process_fixture() -> Result<(), Box<dyn Error>> {
    Err(io::Error::other(FORMATTER_FAILURE_SENTINEL).into())
}

#[test]
fn generated_language_registry_outputs_are_deterministic_and_complete() -> Result<(), Box<dyn Error>>
{
    require_equal(
        rust_u64_literal(999).as_str(),
        "999",
        "ungrouped generated Rust boundary literal",
    )?;
    require_equal(
        rust_u64_literal(1_000).as_str(),
        "1_000",
        "grouped generated Rust boundary literal",
    )?;
    require_equal(
        rust_u64_literal(1_234_567).as_str(),
        "1_234_567",
        "multiply grouped generated Rust literal",
    )?;
    let formatter = GeneratedRustFormatter::new()?;
    verify_formatter_process_contract(&formatter)?;
    let first = generated()?;
    let second = generated()?;
    let (lock, accepted, historical) = decoded_contracts()?;
    let (parser_pack_trust, parser_pack_installed_byte_limit) = decoded_parser_pack_contract()?;
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
    let [candidate] = parser_pack_trust.candidates.as_slice() else {
        return Err(io::Error::other("expected one generated parser-pack candidate").into());
    };
    let cli = std::str::from_utf8(&first.cli)?;
    require(
        cli.contains("#[cfg(test)]")
            && cli.contains("EVALUATION_PARSER_PACK_TRUST")
            && cli.contains(candidate.candidate_id.as_str())
            && cli.contains("advertised: false")
            && cli.contains("pack_abi_version: 1")
            && cli.contains("grammar_abi_version: 15"),
        "generated CLI projection omitted the test-only unselected parser-pack trust record",
    )?;
    let (representative, representative_outputs) =
        representative_artifacts(&lock, &accepted, &historical)?;
    typecheck_generated_rust(&representative_outputs)?;
    verify_generated_rust_rows(&representative, &accepted, &representative_outputs)?;
    verify_generated_json(
        &lock,
        &accepted,
        &historical,
        &parser_pack_trust,
        parser_pack_installed_byte_limit,
        &sha256_hex(LOCK),
        &registry_contract_digest(
            &lock,
            &accepted,
            &historical,
            &parser_pack_trust,
            parser_pack_installed_byte_limit,
        ),
        &first,
    )?;
    let representative_bytes = serde_json::to_vec(&representative)?;
    verify_generated_json(
        &representative,
        &accepted,
        &historical,
        &parser_pack_trust,
        parser_pack_installed_byte_limit,
        &sha256_hex(&representative_bytes),
        &registry_contract_digest(
            &representative,
            &accepted,
            &historical,
            &parser_pack_trust,
            parser_pack_installed_byte_limit,
        ),
        &representative_outputs,
    )?;
    let representative_core = std::str::from_utf8(&representative_outputs.core)?;
    let representative_symbols = std::str::from_utf8(&representative_outputs.symbols)?;
    let representative_state =
        serde_json::from_slice::<serde_json::Value>(&representative_outputs.evidence)?;

    let mut content_mutation = representative.clone();
    let content_before = content_mutation
        .detection_rules
        .iter()
        .find(|rule| {
            matches!(
                rule,
                DetectionRule::Content {
                    detector_id: BuiltInContentDetector::SignaturePhp,
                    ..
                }
            )
        })
        .ok_or_else(|| io::Error::other("representative content detector disappeared"))?;
    let content_before_row = expected_detection_row(&content_mutation, content_before)?;
    let Some(DetectionRule::Content { mode_id, .. }) =
        content_mutation.detection_rules.iter_mut().find(|rule| {
            matches!(
                rule,
                DetectionRule::Content {
                    detector_id: BuiltInContentDetector::SignaturePhp,
                    ..
                }
            )
        })
    else {
        return Err(io::Error::other("representative content detector disappeared").into());
    };
    *mode_id = ModeId::try_from("mode.xml".to_string())?;
    let content_outputs = render_test_lock(&content_mutation, &accepted, &historical)?;
    let content_after = content_mutation
        .detection_rules
        .iter()
        .find(|rule| {
            matches!(
                rule,
                DetectionRule::Content {
                    detector_id: BuiltInContentDetector::SignaturePhp,
                    ..
                }
            )
        })
        .ok_or_else(|| io::Error::other("mutated content detector disappeared"))?;
    require_generated_row_change(
        representative_core,
        std::str::from_utf8(&content_outputs.core)?,
        &content_before_row,
        &expected_detection_row(&content_mutation, content_after)?,
        "content-detection Rust projection",
    )?;
    let content_state = serde_json::from_slice::<serde_json::Value>(&content_outputs.evidence)?;
    require_json_projection_change(
        &representative_state,
        &content_state,
        "/current/detection_rules",
        "content-detection evidence",
    )?;

    let mut semantic_mutation = representative.clone();
    semantic_mutation
        .semantic_modes
        .first_mut()
        .ok_or_else(|| io::Error::other("semantic-mode projection witness disappeared"))?
        .base_mode_id = ModeId::try_from("mode.json".to_string())?;
    let semantic_outputs = render_test_lock(&semantic_mutation, &accepted, &historical)?;
    let semantic_state = serde_json::from_slice::<serde_json::Value>(&semantic_outputs.evidence)?;
    require_json_projection_change(
        &representative_state,
        &semantic_state,
        "/current/semantic_modes",
        "semantic-mode evidence",
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
    let trust_state = &state["parser_pack_trust"];
    require(
        trust_state["selection_state"] == "blocked"
            && trust_state["selected_candidate"].is_null()
            && trust_state["achieved_manifest"].is_null()
            && trust_state["installed_byte_limit"] == parser_pack_installed_byte_limit
            && trust_state["candidates"][0]["candidate_id"] == candidate.candidate_id.as_str()
            && trust_state["candidates"][0]["advertised"] == false
            && trust_state["candidates"][0]["pack_abi"]["version"] == 1
            && trust_state["candidates"][0]["grammar_abi"]["version"] == 15,
        "generated capability state promoted or omitted parser-pack evaluation trust",
    )?;
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
        (
            "accepted_pre_parse_transforms",
            accepted
                .modes
                .iter()
                .filter(|mode| mode.pre_parse_transform.is_some())
                .count(),
        ),
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
    require(
        documentation["parser_pack_trust"]["selection_state"] == "blocked"
            && documentation["parser_pack_trust"]["selected_candidate"].is_null()
            && documentation["parser_pack_trust"]["achieved_manifest"].is_null()
            && documentation["parser_pack_trust"]["candidates"][0]["candidate_id"]
                == candidate.candidate_id.as_str()
            && documentation["parser_pack_trust"]["candidates"][0]["advertised"] == false,
        "generated documentation omitted the unselected parser-pack trust projection",
    )?;
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
fn language_registry_validation_rejects_invalid_or_drifting_contracts() -> Result<(), Box<dyn Error>>
{
    let (lock, accepted, _) = decoded_contracts()?;
    validate_accepted_target(&lock, &accepted)?;
    validate_registry_lock(&lock, &accepted)?;

    let baseline = owned_inputs()?;
    let mut wrong_trust_digest: serde_json::Value = serde_json::from_slice(LOCK)?;
    wrong_trust_digest["parser_pack_trust"]["raw_sha256"] = json!("0".repeat(64));
    let wrong_trust_digest = serde_json::to_vec(&wrong_trust_digest)?;
    require(
        matches!(
            validate_and_generate(&wrong_trust_digest, &baseline.fixed()),
            Err(LanguageRegistryError::Validation(message))
                if message.contains("parser-pack trust manifest raw SHA-256")
        ),
        "parser-pack trust raw digest was not fail-closed",
    )?;

    let mut unknown_trust_field: serde_json::Value = serde_json::from_slice(PARSER_PACK_TRUST)?;
    unknown_trust_field["candidates"][0]["unexpected"] = json!(true);
    let unknown_trust_bytes = serde_json::to_vec(&unknown_trust_field)?;
    let mut trust_bound_lock: serde_json::Value = serde_json::from_slice(LOCK)?;
    trust_bound_lock["parser_pack_trust"]["raw_sha256"] = json!(sha256_hex(&unknown_trust_bytes));
    let trust_bound_lock = serde_json::to_vec(&trust_bound_lock)?;
    let unknown_trust_fixed = FixedInputBytes {
        accepted_capability_registry: ACCEPTED,
        historical_runtime_contract: HISTORICAL,
        parser_pack_trust: &unknown_trust_bytes,
        repository_intelligence_contracts: REPOSITORY_INTELLIGENCE_CONTRACTS,
        parser_pack_payloads: &baseline.parser_pack_payloads,
    };
    require(
        matches!(
            validate_and_generate(&trust_bound_lock, &unknown_trust_fixed),
            Err(LanguageRegistryError::JsonDecode { label, .. })
                if label == "parser-pack trust manifest"
        ),
        "unknown parser-pack trust fields were accepted",
    )?;

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

fn verify_parser_pack_capture_boundaries(
    candidate: &ParserPackCandidateTrust,
    installed_byte_limit: u64,
) -> Result<(), Box<dyn Error>> {
    let mut entry_limit = CandidateCaptureState::new();
    entry_limit.entries = MAX_PARSER_PACK_DIRECTORY_ENTRIES;
    require(
        matches!(
            entry_limit.record_entry(),
            Err(LanguageRegistryError::Validation(message))
                if message.contains("total entries")
        ),
        "the 8,193rd candidate entry was accepted",
    )?;

    let mut directory_limit = CandidateCaptureState::new();
    directory_limit.directories = (0..MAX_PARSER_PACK_DIRECTORIES)
        .map(|index| RegistryPath::try_from(format!("directory-{index}")))
        .collect::<Result<BTreeSet<_>, _>>()?;
    require(
        matches!(
            directory_limit.record_directory(RegistryPath::try_from(
                "directory-overflow".to_string()
            )?),
            Err(LanguageRegistryError::Validation(message))
                if message.contains("directories")
        ),
        "the 4,097th candidate directory was accepted",
    )?;

    let file_witness = CandidateFileSnapshot {
        bytes: 1,
        sha256: Sha256Digest("1".repeat(64)),
        metadata_bytes: None,
    };
    let mut file_limit = CandidateCaptureState::new();
    file_limit.files = (0..MAX_PARSER_PACK_FILES)
        .map(|index| {
            Ok((
                RegistryPath::try_from(format!("file-{index}"))?,
                file_witness.clone(),
            ))
        })
        .collect::<Result<BTreeMap<_, _>, LanguageRegistryError>>()?;
    require(
        matches!(
            file_limit.reserve_file(),
            Err(LanguageRegistryError::Validation(message)) if message.contains("files")
        ),
        "the 4,097th candidate file was accepted",
    )?;

    let diagnostic = Path::new("candidate-boundary");
    require(
        validate_candidate_depth(diagnostic, MAX_PARSER_PACK_DEPTH).is_ok(),
        "candidate depth 64 was rejected",
    )?;
    require(
        matches!(
            validate_candidate_depth(diagnostic, MAX_PARSER_PACK_DEPTH + 1),
            Err(LanguageRegistryError::Validation(message))
                if message.contains("maximum depth")
        ),
        "candidate depth 65 was accepted",
    )?;

    let identity = CapabilityPathIdentity {
        device: 7,
        inode: 11,
    };
    let replacement = CapabilityPathIdentity {
        device: 7,
        inode: 12,
    };
    let initial_names = vec!["manifest.json".to_string()];
    require(
        require_candidate_directory_stable(
            diagnostic,
            &initial_names,
            &initial_names,
            identity,
            identity,
        )
        .is_ok(),
        "stable candidate directory was rejected",
    )?;
    require(
        matches!(
            require_candidate_directory_stable(
                diagnostic,
                &initial_names,
                &["provenance.json".to_string()],
                identity,
                identity,
            ),
            Err(LanguageRegistryError::Validation(message)) if message.contains("changed")
        ),
        "candidate directory entry-set drift was accepted",
    )?;
    require(
        matches!(
            require_candidate_directory_stable(
                diagnostic,
                &initial_names,
                &initial_names,
                identity,
                replacement,
            ),
            Err(LanguageRegistryError::Validation(message)) if message.contains("changed")
        ),
        "candidate directory replacement was accepted",
    )?;

    let mut stream = CandidateCaptureState::new();
    let streamed = record_candidate_stream_bytes(diagnostic, 0, 4, 5, 5, 5, &mut stream)?;
    require_equal(&streamed, &4, "bounded candidate file stream")?;
    require(
        matches!(
            record_candidate_stream_bytes(
                diagnostic,
                streamed,
                2,
                5,
                5,
                5,
                &mut stream,
            ),
            Err(LanguageRegistryError::Validation(message))
                if message.contains("streaming byte ceiling")
        ),
        "growing candidate stream crossed its declared size",
    )?;
    let mut cumulative = CandidateCaptureState::new();
    cumulative.captured_bytes = 5;
    require(
        matches!(
            record_candidate_stream_bytes(
                diagnostic,
                0,
                1,
                1,
                1,
                5,
                &mut cumulative,
            ),
            Err(LanguageRegistryError::Validation(message))
                if message.contains("streaming byte ceiling")
        ),
        "candidate cumulative-byte ceiling was crossed",
    )?;
    require(
        matches!(
            require_candidate_file_stable(diagnostic, 4, 5, identity, identity, identity),
            Err(LanguageRegistryError::Validation(message)) if message.contains("changed")
        ),
        "short candidate stream was accepted",
    )?;
    require(
        matches!(
            require_candidate_file_stable(diagnostic, 5, 5, identity, identity, replacement),
            Err(LanguageRegistryError::Validation(message)) if message.contains("changed")
        ),
        "candidate file replacement was accepted",
    )?;

    let stale_trust_root = tempfile::tempdir()?;
    seed_inputs(stale_trust_root.path())?;
    fs::remove_dir_all(
        stale_trust_root
            .path()
            .join(candidate.payload_root.as_str()),
    )?;
    let mut stale_trust = PARSER_PACK_TRUST.to_vec();
    stale_trust.push(b'\n');
    fs::write(
        stale_trust_root.path().join(PARSER_PACK_TRUST_PATH),
        stale_trust,
    )?;
    require(
        matches!(
            RegistryWorkspace::new(stale_trust_root.path())?.read_inputs(),
            Err(LanguageRegistryError::Validation(message))
                if message.contains("parser-pack trust manifest raw SHA-256")
        ),
        "stale trust digest did not fail before trust-directed traversal",
    )?;

    let closed_budget_root = tempfile::tempdir()?;
    seed_inputs(closed_budget_root.path())?;
    fs::remove_dir_all(
        closed_budget_root
            .path()
            .join(candidate.payload_root.as_str()),
    )?;
    let mut closed_budget: serde_json::Value =
        serde_json::from_slice(REPOSITORY_INTELLIGENCE_CONTRACTS)?;
    closed_budget["budgets"]["optional_pack_contract"]["accepted_pack_budgets"][0]["limits"]["unexpected_limit"] =
        json!(1);
    fs::write(
        closed_budget_root
            .path()
            .join(REPOSITORY_INTELLIGENCE_CONTRACTS_PATH),
        serde_json::to_vec(&closed_budget)?,
    )?;
    require(
        matches!(
            RegistryWorkspace::new(closed_budget_root.path())?.read_inputs(),
            Err(LanguageRegistryError::JsonDecode { label, .. })
                if label == "repository-intelligence contracts"
        ),
        "unknown nested optional-pack budget field did not fail before traversal",
    )?;

    let ceiling_root = tempfile::tempdir()?;
    seed_inputs(ceiling_root.path())?;
    fs::remove_dir_all(ceiling_root.path().join(candidate.payload_root.as_str()))?;
    let mut lowered_budget: serde_json::Value =
        serde_json::from_slice(REPOSITORY_INTELLIGENCE_CONTRACTS)?;
    let broad_budget = lowered_budget["budgets"]["optional_pack_contract"]["accepted_pack_budgets"]
        .as_array_mut()
        .and_then(|budgets| {
            budgets
                .iter_mut()
                .find(|budget| budget["pack_id"] == BROAD_LANGUAGE_PACK_ID)
        })
        .ok_or_else(|| io::Error::other("broad parser-pack budget row is absent"))?;
    broad_budget["limits"]["installed_bytes"] = json!(candidate.installed_bytes - 1);
    fs::write(
        ceiling_root
            .path()
            .join(REPOSITORY_INTELLIGENCE_CONTRACTS_PATH),
        serde_json::to_vec(&lowered_budget)?,
    )?;
    require(
        matches!(
            RegistryWorkspace::new(ceiling_root.path())?.read_inputs(),
            Err(LanguageRegistryError::Validation(message))
                if message.contains("resource contract")
        ),
        "authoritative installed-byte ceiling did not fail before traversal",
    )?;

    let declared_size_root = tempfile::tempdir()?;
    seed_inputs(declared_size_root.path())?;
    let declared_workspace = RegistryWorkspace::new(declared_size_root.path())?;
    let mut wrong_declared_size = candidate.clone();
    wrong_declared_size.inventory[0].bytes += 1;
    wrong_declared_size.installed_bytes += 1;
    require(
        matches!(
            declared_workspace.capture_candidate_payload(
                &wrong_declared_size,
                installed_byte_limit,
            ),
            Err(LanguageRegistryError::Validation(message))
                if message.contains("declared length")
        ),
        "candidate declared-size mismatch was streamed",
    )?;

    let wrong_case_root = tempfile::tempdir()?;
    seed_inputs(wrong_case_root.path())?;
    let lower = wrong_case_root.path().join("fixtures/parser-packs");
    let transition = wrong_case_root
        .path()
        .join("fixtures/parser-packs-case-transition");
    let wrong_case = wrong_case_root.path().join("fixtures/Parser-Packs");
    fs::rename(&lower, &transition)?;
    fs::rename(&transition, &wrong_case)?;
    require(
        matches!(
            RegistryWorkspace::new(wrong_case_root.path())?
                .capture_candidate_payload(candidate, installed_byte_limit),
            Err(LanguageRegistryError::Validation(message))
                if message.contains("wrong filesystem spelling")
        ),
        "wrong-case candidate-root component was accepted",
    )?;

    let linked_directory_root = tempfile::tempdir()?;
    seed_inputs(linked_directory_root.path())?;
    let parser_directory = linked_directory_root
        .path()
        .join(candidate.payload_root.as_str())
        .join("parsers");
    let parser_target = linked_directory_root.path().join("parser-directory-target");
    fs::rename(&parser_directory, &parser_target)?;
    if create_dir_symlink(&parser_target, &parser_directory).is_ok() {
        require(
            matches!(
                RegistryWorkspace::new(linked_directory_root.path())?
                    .capture_candidate_payload(candidate, installed_byte_limit),
                Err(LanguageRegistryError::Validation(message))
                    if message.contains("link or reparse point")
                        || message.contains("non-link directory")
            ),
            "linked/reparse candidate directory was accepted",
        )?;
    }
    Ok(())
}

fn verify_parser_pack_metadata_and_evidence_mutations(
    candidate: &ParserPackCandidateTrust,
    snapshot: &CandidatePayloadSnapshot,
    inventory: &BTreeMap<RegistryPath, &ParserPackTrustedFile>,
    parser_file: (&RegistryPath, &ParserPackTrustedFile),
) -> Result<(), Box<dyn Error>> {
    let manifest_bytes = candidate_metadata_bytes(snapshot, &candidate.manifest_path, "manifest")?;
    let provenance_bytes =
        candidate_metadata_bytes(snapshot, &candidate.provenance.record_path, "provenance")?;
    let advisory_bytes =
        candidate_metadata_bytes(snapshot, &candidate.advisory_record_path, "advisory record")?;
    let wasm_validation_bytes = candidate_metadata_bytes(
        snapshot,
        &candidate.wasm_validation_record_path,
        "WASM validation record",
    )?;
    let sbom_bytes = candidate_metadata_bytes(snapshot, &candidate.sbom_record_path, "SBOM")?;

    require_duplicate_metadata_rejected::<ParserPackReleaseManifest>(
        manifest_bytes,
        "\"schema_version\":1,",
        "parser-pack release manifest",
        "release manifest",
    )?;
    require_duplicate_metadata_rejected::<ParserPackProvenanceRecord>(
        provenance_bytes,
        "\"schema_version\":1,",
        "parser-pack provenance record",
        "provenance record",
    )?;
    require_duplicate_metadata_rejected::<ParserPackAdvisoryRecord>(
        advisory_bytes,
        "\"schema_version\":1,",
        "parser-pack advisory record",
        "advisory record",
    )?;
    require_duplicate_metadata_rejected::<ParserPackWasmValidationRecord>(
        wasm_validation_bytes,
        "\"schema_version\":1,",
        "parser-pack WASM validation record",
        "WASM validation record",
    )?;
    require_duplicate_metadata_rejected::<ParserPackSpdxDocument>(
        sbom_bytes,
        "\"spdxVersion\":\"SPDX-2.3\",",
        "parser-pack SPDX record",
        "SPDX record",
    )?;

    let wasm_record = || {
        decode_parser_pack_metadata::<ParserPackWasmValidationRecord>(
            wasm_validation_bytes,
            "parser-pack WASM validation record",
        )
    };
    let mut wrong_magic = wasm_record()?;
    wrong_magic.module.magic_hex = "0161736d".to_string();
    require(
        matches!(
            validate_candidate_wasm(
                candidate,
                snapshot,
                inventory,
                parser_file,
                &wrong_magic,
            ),
            Err(LanguageRegistryError::Validation(message))
                if message.contains("WASM validation identity")
        ),
        "wrong recorded WASM magic was accepted",
    )?;

    let mut wrong_module_version = wasm_record()?;
    wrong_module_version.module.module_version += 1;
    require(
        validate_candidate_wasm(
            candidate,
            snapshot,
            inventory,
            parser_file,
            &wrong_module_version,
        )
        .is_err(),
        "wrong recorded WASM module version was accepted",
    )?;

    let mut wrong_required_export = wasm_record()?;
    wrong_required_export.module.required_function_export = "tree_sitter_wrong".to_string();
    require(
        validate_candidate_wasm(
            candidate,
            snapshot,
            inventory,
            parser_file,
            &wrong_required_export,
        )
        .is_err(),
        "wrong required WASM function export was accepted",
    )?;

    let mut wrong_grammar_abi = wasm_record()?;
    wrong_grammar_abi.module.grammar_abi_version += 1;
    require(
        validate_candidate_wasm(
            candidate,
            snapshot,
            inventory,
            parser_file,
            &wrong_grammar_abi,
        )
        .is_err(),
        "wrong WASM grammar ABI was accepted",
    )?;

    let mut wrong_script_path = wasm_record()?;
    wrong_script_path.probe.script_path = RegistryPath::try_from("wrong-probe.mjs".to_string())?;
    require(
        validate_candidate_wasm(
            candidate,
            snapshot,
            inventory,
            parser_file,
            &wrong_script_path,
        )
        .is_err(),
        "wrong WASM probe-script path was accepted",
    )?;

    let mut wrong_script_digest = wasm_record()?;
    wrong_script_digest.probe.script_sha256 = Sha256Digest("0".repeat(64));
    require(
        validate_candidate_wasm(
            candidate,
            snapshot,
            inventory,
            parser_file,
            &wrong_script_digest,
        )
        .is_err(),
        "wrong WASM probe-script digest was accepted",
    )?;

    let mut wrong_probe_digest = wasm_record()?;
    wrong_probe_digest.probe.raw_result_sha256 = Sha256Digest("0".repeat(64));
    require(
        validate_candidate_wasm(
            candidate,
            snapshot,
            inventory,
            parser_file,
            &wrong_probe_digest,
        )
        .is_err(),
        "wrong raw WASM probe-result digest was accepted",
    )?;

    for (field, value, context) in [
        ("abi", json!(candidate.grammar_abi.version - 1), "ABI"),
        ("exports", json!(["tree_sitter_wrong"]), "export"),
    ] {
        let mut wrong_probe_result = wasm_record()?;
        let mut raw: serde_json::Value =
            serde_json::from_str(&wrong_probe_result.probe.raw_result)?;
        raw[field] = value;
        wrong_probe_result.probe.raw_result = serde_json::to_string(&raw)?;
        wrong_probe_result.probe.raw_result_sha256 =
            Sha256Digest(sha256_hex(wrong_probe_result.probe.raw_result.as_bytes()));
        require(
            validate_candidate_wasm(
                candidate,
                snapshot,
                inventory,
                parser_file,
                &wrong_probe_result,
            )
            .is_err(),
            format!("wrong raw WASM probe-result {context} was accepted"),
        )?;
    }

    let mut duplicate_probe_result = wasm_record()?;
    duplicate_probe_result.probe.raw_result =
        duplicate_probe_result
            .probe
            .raw_result
            .replacen('{', "{\"abi\":15,", 1);
    duplicate_probe_result.probe.raw_result_sha256 = Sha256Digest(sha256_hex(
        duplicate_probe_result.probe.raw_result.as_bytes(),
    ));
    require(
        matches!(
            validate_candidate_wasm(
                candidate,
                snapshot,
                inventory,
                parser_file,
                &duplicate_probe_result,
            ),
            Err(LanguageRegistryError::JsonDecode { .. })
        ),
        "duplicate keys in the retained raw WASM probe result were accepted",
    )?;

    for (offset, context) in [(0_usize, "magic"), (4_usize, "module version")] {
        let mut changed_snapshot = snapshot.clone();
        let module = changed_snapshot
            .files
            .get_mut(parser_file.0)
            .and_then(|file| file.metadata_bytes.as_mut())
            .ok_or_else(|| io::Error::other("retained WASM module is absent"))?;
        module[offset] ^= 1;
        require(
            validate_candidate_wasm(
                candidate,
                &changed_snapshot,
                inventory,
                parser_file,
                &wasm_record()?,
            )
            .is_err(),
            format!("mutated WASM {context} was accepted"),
        )?;
    }

    let mut missing_export_snapshot = snapshot.clone();
    let module = missing_export_snapshot
        .files
        .get_mut(parser_file.0)
        .and_then(|file| file.metadata_bytes.as_mut())
        .ok_or_else(|| io::Error::other("retained WASM module is absent"))?;
    let required_export = TREE_SITTER_WASM_REQUIRED_EXPORT.as_bytes();
    let export_offset = module
        .windows(required_export.len())
        .position(|window| window == required_export)
        .ok_or_else(|| io::Error::other("required WASM export bytes are absent"))?;
    module[export_offset] = b'x';
    require(
        validate_candidate_wasm(
            candidate,
            &missing_export_snapshot,
            inventory,
            parser_file,
            &wasm_record()?,
        )
        .is_err(),
        "WASM module without the required function export was accepted",
    )?;

    let provenance_record = || {
        decode_parser_pack_metadata::<ParserPackProvenanceRecord>(
            provenance_bytes,
            "parser-pack provenance record",
        )
    };
    let mut wrong_run_command = provenance_record()?;
    wrong_run_command.local_output_runs[0].command = "tree-sitter build --wasm".to_string();
    require(
        validate_candidate_provenance(candidate, &wrong_run_command, parser_file).is_err(),
        "wrong local reproduction command was accepted",
    )?;
    let mut wrong_run_directory = provenance_record()?;
    wrong_run_directory.local_output_runs[1].working_directory =
        RegistryPath::try_from("build-c".to_string())?;
    wrong_run_directory.local_output_runs[1].command =
        "tree-sitter build --wasm --output ../build-c.wasm .".to_string();
    require(
        validate_candidate_provenance(candidate, &wrong_run_directory, parser_file).is_err(),
        "wrong local reproduction directory was accepted",
    )?;
    let mut wrong_run_output = provenance_record()?;
    wrong_run_output.local_output_runs[1].sha256 = Sha256Digest("0".repeat(64));
    require(
        validate_candidate_provenance(candidate, &wrong_run_output, parser_file).is_err(),
        "wrong local reproduction output was accepted",
    )?;

    let invalid_evidence_state = std::str::from_utf8(provenance_bytes)?.replacen(
        "\"hosted_reproduction\": \"pending\"",
        "\"hosted_reproduction\": \"complete\"",
        1,
    );
    require(
        matches!(
            decode_parser_pack_metadata::<ParserPackProvenanceRecord>(
                invalid_evidence_state.as_bytes(),
                "parser-pack provenance record",
            ),
            Err(LanguageRegistryError::JsonDecode { .. })
        ),
        "unsupported provenance evidence state was accepted",
    )?;

    let clean_provenance = provenance_record()?;
    let advisory_record = || {
        decode_parser_pack_metadata::<ParserPackAdvisoryRecord>(
            advisory_bytes,
            "parser-pack advisory record",
        )
    };
    let mut wrong_advisory_input = advisory_record()?;
    wrong_advisory_input.inputs[0].bytes += 1;
    require(
        validate_candidate_advisory(candidate, &clean_provenance, &wrong_advisory_input).is_err(),
        "wrong advisory input identity was accepted",
    )?;
    let mut advisory_value: serde_json::Value = serde_json::from_slice(advisory_bytes)?;
    let duplicate_input = advisory_value["inputs"]
        .as_array()
        .and_then(|inputs| inputs.first())
        .cloned()
        .ok_or_else(|| io::Error::other("advisory input witness is absent"))?;
    advisory_value["inputs"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("advisory inputs are not an array"))?
        .push(duplicate_input);
    let duplicate_input_record =
        serde_json::from_value::<ParserPackAdvisoryRecord>(advisory_value)?;
    require(
        validate_candidate_advisory(candidate, &clean_provenance, &duplicate_input_record).is_err(),
        "extra advisory input row was accepted",
    )?;
    let mut duplicate_advisory_raw = advisory_record()?;
    duplicate_advisory_raw.raw_result =
        duplicate_advisory_raw
            .raw_result
            .replacen('{', "{\"warnings\":{},", 1);
    duplicate_advisory_raw.raw_result_bytes = duplicate_advisory_raw.raw_result.len() as u64;
    duplicate_advisory_raw.raw_result_sha256 =
        Sha256Digest(sha256_hex(duplicate_advisory_raw.raw_result.as_bytes()));
    require(
        matches!(
            validate_candidate_advisory(candidate, &clean_provenance, &duplicate_advisory_raw,),
            Err(LanguageRegistryError::JsonDecode { .. })
        ),
        "duplicate keys in the retained raw advisory result were accepted",
    )?;

    let clean_advisory = advisory_record()?;
    let validate_spdx_value = |value: serde_json::Value| -> Result<_, Box<dyn Error>> {
        let document = serde_json::from_value::<ParserPackSpdxDocument>(value)?;
        Ok(validate_candidate_spdx(
            candidate,
            inventory,
            &clean_provenance,
            &clean_advisory,
            &document,
        ))
    };
    let mut extra_relationship: serde_json::Value = serde_json::from_slice(sbom_bytes)?;
    extra_relationship["relationships"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("SPDX relationships are not an array"))?
        .push(json!({
            "spdxElementId": "SPDXRef-Package-tree-sitter-javascript-source",
            "relationshipType": "OTHER",
            "relatedSpdxElement": "SPDXRef-Package-ProjectAtlas-parser-pack"
        }));
    require(
        matches!(
            validate_spdx_value(extra_relationship)?,
            Err(LanguageRegistryError::Validation(message))
                if message.contains("relationship graph is not exact")
        ),
        "extra SPDX relationship row was accepted",
    )?;
    let mut duplicate_relationship: serde_json::Value = serde_json::from_slice(sbom_bytes)?;
    let repeated = duplicate_relationship["relationships"]
        .as_array()
        .and_then(|relationships| relationships.first())
        .cloned()
        .ok_or_else(|| io::Error::other("SPDX relationship witness is absent"))?;
    duplicate_relationship["relationships"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("SPDX relationships are not an array"))?
        .push(repeated);
    require(
        validate_spdx_value(duplicate_relationship)?.is_err(),
        "duplicate SPDX relationship row was accepted",
    )?;
    let mut missing_relationship: serde_json::Value = serde_json::from_slice(sbom_bytes)?;
    missing_relationship["relationships"]
        .as_array_mut()
        .and_then(Vec::pop)
        .ok_or_else(|| io::Error::other("SPDX relationship witness is absent"))?;
    require(
        validate_spdx_value(missing_relationship)?.is_err(),
        "missing SPDX relationship row was accepted",
    )?;
    for package_index in 0..2 {
        let mut verification_code: serde_json::Value = serde_json::from_slice(sbom_bytes)?;
        verification_code["packages"][package_index]["packageVerificationCode"] = json!({
            "packageVerificationCodeValue": "0".repeat(40)
        });
        require(
            validate_spdx_value(verification_code)?.is_err(),
            format!("SPDX package {package_index} verification code was accepted"),
        )?;
    }
    Ok(())
}

#[test]
fn parser_pack_trust_inventory_is_exact_and_fail_closed() -> Result<(), Box<dyn Error>> {
    let lock = serde_json::from_slice::<LanguageRegistryLock>(LOCK)?;
    let trust = decode_parser_pack_trust(PARSER_PACK_TRUST)?;
    let inputs = owned_inputs()?;
    let installed_byte_limit = validate_parser_pack_trust(
        &lock,
        &trust,
        &inputs.parser_pack_payloads,
        REPOSITORY_INTELLIGENCE_CONTRACTS,
    )?;
    let [candidate] = trust.candidates.as_slice() else {
        return Err(io::Error::other("expected one parser-pack trust candidate").into());
    };
    let snapshot = inputs
        .parser_pack_payloads
        .iter()
        .find(|payload| payload.candidate_id == candidate.candidate_id)
        .ok_or_else(|| io::Error::other("candidate payload snapshot is absent"))?;
    let inventory = candidate
        .inventory
        .iter()
        .map(|file| (file.path.clone(), file))
        .collect::<BTreeMap<_, _>>();
    let parser_file = inventory
        .iter()
        .find(|(_, file)| file.role == ParserPackTrustedFileRole::ParserModule)
        .map(|(path, file)| (path, *file))
        .ok_or_else(|| io::Error::other("parser module inventory row is absent"))?;

    verify_parser_pack_capture_boundaries(candidate, installed_byte_limit)?;
    verify_parser_pack_metadata_and_evidence_mutations(
        candidate,
        snapshot,
        &inventory,
        parser_file,
    )?;

    let mut installed_overflow = trust.clone();
    installed_overflow.candidates[0].installed_bytes = installed_byte_limit + 1;
    require(
        matches!(
            validate_parser_pack_trust(
                &lock,
                &installed_overflow,
                &inputs.parser_pack_payloads,
                REPOSITORY_INTELLIGENCE_CONTRACTS,
            ),
            Err(LanguageRegistryError::Validation(_))
        ),
        "parser-pack installed-byte ceiling was not enforced",
    )?;

    let mut wrong_inventory_digest = trust.clone();
    wrong_inventory_digest.candidates[0].inventory[0].sha256 =
        serde_json::from_value(json!("0".repeat(64)))?;
    require(
        matches!(
            validate_parser_pack_trust(
                &lock,
                &wrong_inventory_digest,
                &inputs.parser_pack_payloads,
                REPOSITORY_INTELLIGENCE_CONTRACTS,
            ),
            Err(LanguageRegistryError::Validation(message))
                if message.contains("trusted size or digest")
        ),
        "parser-pack inventory digest drift was accepted",
    )?;

    let mut wrong_probe_script_bytes = trust.clone();
    let probe_script = wrong_probe_script_bytes.candidates[0]
        .inventory
        .iter_mut()
        .find(|file| file.role == ParserPackTrustedFileRole::WasmProbeScript)
        .ok_or_else(|| io::Error::other("WASM probe-script trust row is absent"))?;
    probe_script.bytes += 1;
    wrong_probe_script_bytes.candidates[0].installed_bytes += 1;
    require(
        matches!(
            validate_parser_pack_trust(
                &lock,
                &wrong_probe_script_bytes,
                &inputs.parser_pack_payloads,
                REPOSITORY_INTELLIGENCE_CONTRACTS,
            ),
            Err(LanguageRegistryError::Validation(_))
        ),
        "wrong WASM probe-script byte count was accepted",
    )?;

    let mut borrowing_budget: serde_json::Value =
        serde_json::from_slice(REPOSITORY_INTELLIGENCE_CONTRACTS)?;
    let broad_budget =
        borrowing_budget["budgets"]["optional_pack_contract"]["accepted_pack_budgets"]
            .as_array_mut()
            .and_then(|budgets| {
                budgets
                    .iter_mut()
                    .find(|budget| budget["pack_id"] == BROAD_LANGUAGE_PACK_ID)
            })
            .ok_or_else(|| io::Error::other("broad parser-pack budget row is absent"))?;
    broad_budget["may_borrow_default_core_allowance"] = json!(true);
    let borrowing_budget = serde_json::to_vec(&borrowing_budget)?;
    require(
        matches!(
            validate_parser_pack_trust(
                &lock,
                &trust,
                &inputs.parser_pack_payloads,
                &borrowing_budget,
            ),
            Err(LanguageRegistryError::Validation(_))
        ),
        "parser pack was allowed to borrow the default-core allowance",
    )?;

    let provenance_bytes =
        candidate_metadata_bytes(snapshot, &candidate.provenance.record_path, "provenance")?;
    let mut provenance = decode_parser_pack_metadata::<ParserPackProvenanceRecord>(
        provenance_bytes,
        "parser-pack provenance record",
    )?;
    provenance.local_output_runs[1].bytes += 1;
    require(
        matches!(
            validate_candidate_provenance(candidate, &provenance, parser_file),
            Err(LanguageRegistryError::Validation(message))
                if message.contains("byte-identical clean builds")
        ),
        "non-reproducible parser-pack build evidence was accepted",
    )?;

    let advisory_bytes =
        candidate_metadata_bytes(snapshot, &candidate.advisory_record_path, "advisory record")?;
    let mut advisory = decode_parser_pack_metadata::<ParserPackAdvisoryRecord>(
        advisory_bytes,
        "parser-pack advisory record",
    )?;
    advisory.raw_result = advisory
        .raw_result
        .replacen("\"found\":false", "\"found\":true", 1);
    advisory.raw_result_bytes = advisory.raw_result.len() as u64;
    advisory.raw_result_sha256 =
        serde_json::from_value(json!(sha256_hex(advisory.raw_result.as_bytes())))?;
    let provenance = decode_parser_pack_metadata::<ParserPackProvenanceRecord>(
        provenance_bytes,
        "parser-pack provenance record",
    )?;
    require(
        matches!(
            validate_candidate_advisory(candidate, &provenance, &advisory),
            Err(LanguageRegistryError::Validation(_))
        ),
        "non-clean parser-pack advisory record was accepted",
    )?;

    let sbom_bytes =
        candidate_metadata_bytes(snapshot, &candidate.sbom_record_path, "SBOM record")?;
    let mut sbom = decode_parser_pack_metadata::<ParserPackSpdxDocument>(
        sbom_bytes,
        "parser-pack SPDX record",
    )?;
    let sha256 = sbom.files[0]
        .checksums
        .iter_mut()
        .find(|checksum| checksum.algorithm == "SHA256")
        .ok_or_else(|| io::Error::other("SPDX file SHA-256 is absent"))?;
    sha256.checksum_value = "0".repeat(64);
    let clean_advisory = decode_parser_pack_metadata::<ParserPackAdvisoryRecord>(
        advisory_bytes,
        "parser-pack advisory record",
    )?;
    require(
        matches!(
            validate_candidate_spdx(candidate, &inventory, &provenance, &clean_advisory, &sbom,),
            Err(LanguageRegistryError::Validation(_))
        ),
        "parser-pack SPDX file digest drift was accepted",
    )?;

    let duplicate_key =
        String::from_utf8_lossy(PARSER_PACK_TRUST).replacen('{', "{\"schema_version\":1,", 1);
    require(
        matches!(
            decode_parser_pack_trust(duplicate_key.as_bytes()),
            Err(LanguageRegistryError::JsonDecode { .. })
        ),
        "duplicate parser-pack trust keys were accepted",
    )?;

    let extra_file_root = tempfile::tempdir()?;
    seed_inputs(extra_file_root.path())?;
    fs::write(
        extra_file_root
            .path()
            .join("fixtures/parser-packs/tree-sitter-wasm-grammar-pack/extra.bin"),
        b"undeclared",
    )?;
    let extra_inputs = RegistryWorkspace::new(extra_file_root.path())?.read_inputs();
    require(
        matches!(
            extra_inputs,
            Err(LanguageRegistryError::Validation(message))
                if message.contains("undeclared file")
        ),
        "undeclared parser-pack payload file was not rejected during capture",
    )?;

    let hardlink_root = tempfile::tempdir()?;
    seed_inputs(hardlink_root.path())?;
    let parser_path = hardlink_root
        .path()
        .join("fixtures/parser-packs/tree-sitter-wasm-grammar-pack/parsers/javascript.wasm");
    fs::hard_link(
        &parser_path,
        hardlink_root.path().join("outside-parser-hardlink.wasm"),
    )?;
    require(
        matches!(
            RegistryWorkspace::new(hardlink_root.path())?.read_inputs(),
            Err(LanguageRegistryError::Validation(message))
                if message.contains("hardlink") || message.contains("single-link")
        ),
        "hard-linked parser-pack payload file was accepted",
    )?;

    let symlink_root = tempfile::tempdir()?;
    seed_inputs(symlink_root.path())?;
    let license_path = symlink_root
        .path()
        .join("fixtures/parser-packs/tree-sitter-wasm-grammar-pack/LICENSE");
    let license_target = symlink_root.path().join("license-target");
    fs::rename(&license_path, &license_target)?;
    if create_file_symlink(&license_target, &license_path).is_ok() {
        require(
            matches!(
                RegistryWorkspace::new(symlink_root.path())?.read_inputs(),
                Err(LanguageRegistryError::Validation(message))
                    if message.contains("link or reparse point")
            ),
            "linked/reparse parser-pack payload file was accepted",
        )?;
    }
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
    let inputs = owned_inputs()?;
    let result = validate_and_generate(&bytes, &inputs.fixed());
    require(
        matches!(result, Err(LanguageRegistryError::Validation(message)) if message.contains("collide by ASCII case")),
        "case-colliding registry paths were accepted",
    )?;
    Ok(())
}

#[test]
fn built_in_parser_selection_remains_closed_and_pack_isolated() -> Result<(), Box<dyn Error>> {
    let (lock, accepted, _) = decoded_contracts()?;
    validate_accepted_target(&lock, &accepted)?;
    validate_registry_lock(&lock, &accepted)?;

    let pack_contracts = lock
        .packs
        .iter()
        .map(|pack| (pack.pack_id.as_str(), (pack.ownership, pack.runtime)))
        .collect::<BTreeMap<_, _>>();
    let components_by_parser = lock
        .parser_components
        .iter()
        .map(|component| (component.built_in_parser, component))
        .collect::<BTreeMap<_, _>>();
    require_equal(
        &components_by_parser.len(),
        &lock.parser_components.len(),
        "unique built-in parser component inventory",
    )?;
    for component in &lock.parser_components {
        let expected_component_id = format!(
            "parser.builtin.{}",
            component.built_in_parser.contract_tag()
        );
        require_equal(
            component.parser_id.as_str(),
            expected_component_id.as_str(),
            "built-in parser component identity",
        )?;
        require_equal(
            &component.implementation,
            &ParserImplementation::CompiledTreeSitter,
            "built-in parser implementation",
        )?;
        require_equal(
            &component.abi.state,
            &AbiState::CurrentCompiledContract,
            "built-in parser ABI state",
        )?;
        require_equal(
            component.abi.abi_id.as_str(),
            CURRENT_COMPILED_PARSER_ABI_ID,
            "built-in parser ABI identity",
        )?;
        require_equal(
            &component.abi.version,
            &CURRENT_COMPILED_PARSER_ABI_VERSION,
            "built-in parser ABI version",
        )?;
        require_equal(
            component.current_pack_id.as_str(),
            DEFAULT_CORE_PACK_ID,
            "built-in parser current pack",
        )?;
        require_equal(
            &pack_contracts
                .get(component.current_pack_id.as_str())
                .copied(),
            &Some((PackOwnership::DefaultCore, PackRuntime::InProcess)),
            "built-in parser pack runtime boundary",
        )?;
    }

    let accepted_modes = accepted
        .modes
        .iter()
        .map(|mode| (&mode.mode_id, mode))
        .collect::<BTreeMap<_, _>>();
    let accepted_parsers = accepted
        .parsers
        .iter()
        .map(|parser| (&parser.parser_id, parser))
        .collect::<BTreeMap<_, _>>();
    let mut routed_parsers = BTreeSet::new();
    let mut current_accepted_parser_ids = BTreeSet::new();
    for mode in &lock.current_modes {
        let SymbolPipeline::BuiltIn { parser, .. } = &mode.symbols else {
            continue;
        };
        let component = components_by_parser.get(parser).ok_or_else(|| {
            io::Error::other(format!(
                "current built-in route {} has no parser component",
                mode.mode_id.as_str()
            ))
        })?;
        routed_parsers.insert(*parser);
        let accepted_mode = accepted_modes.get(&mode.accepted_mode_id).ok_or_else(|| {
            io::Error::other(format!(
                "current built-in route {} has no accepted mode",
                mode.mode_id.as_str()
            ))
        })?;
        let expected_accepted_parser_id = AcceptedParserId::try_from(format!(
            "parse.{}",
            component.built_in_parser.contract_tag()
        ))?;
        require_equal(
            accepted_mode.pack_id.as_str(),
            DEFAULT_CORE_PACK_ID,
            "current built-in accepted-mode pack",
        )?;
        require_equal(
            &accepted_mode.parser_id,
            &expected_accepted_parser_id,
            "current built-in accepted-mode parser",
        )?;
        let accepted_parser = accepted_parsers
            .get(&accepted_mode.parser_id)
            .ok_or_else(|| io::Error::other("current built-in accepted parser disappeared"))?;
        require_equal(
            accepted_parser.pack_id.as_str(),
            DEFAULT_CORE_PACK_ID,
            "current built-in accepted-parser pack",
        )?;
        require_equal(
            &accepted_parser.parser_id,
            &expected_accepted_parser_id,
            "current built-in accepted-parser identity",
        )?;
        current_accepted_parser_ids.insert(expected_accepted_parser_id);
    }
    require_equal(
        &routed_parsers,
        &components_by_parser
            .keys()
            .copied()
            .collect::<BTreeSet<_>>(),
        "closed built-in route and parser-component inventories",
    )?;

    let broad_parsers = accepted
        .parsers
        .iter()
        .filter(|parser| parser.pack_id.as_str() == BROAD_LANGUAGE_PACK_ID)
        .collect::<Vec<_>>();
    let broad_pack_owner = accepted
        .source
        .packs
        .iter()
        .find(|pack| pack.pack_id.as_str() == BROAD_LANGUAGE_PACK_ID)
        .and_then(|pack| pack.language_owner.as_deref())
        .ok_or_else(|| io::Error::other("accepted broad-language pack owner is absent"))?;
    require(
        !broad_parsers.is_empty(),
        "accepted broad-language parser inventory is empty",
    )?;
    let broad_parser_ids = broad_parsers
        .iter()
        .map(|parser| parser.parser_id.clone())
        .collect::<BTreeSet<_>>();
    for parser in &broad_parsers {
        require_equal(
            parser.owner.as_str(),
            broad_pack_owner,
            "accepted broad parser owner",
        )?;
        require_equal(
            &parser.evidence_state,
            &AcceptedParserEvidenceState::PendingAssetFixtureAndPlatformVerification,
            "accepted broad parser evidence state",
        )?;
        require(!parser.advertised, "accepted broad parser is advertised")?;
    }
    require(
        current_accepted_parser_ids.is_disjoint(&broad_parser_ids),
        "current built-in and broad accepted parser inventories overlap",
    )?;

    let artifacts = generated()?;
    let symbols = std::str::from_utf8(&artifacts.symbols)?;
    require_equal(
        &count_struct_initializers(symbols, "ParserComponentContract")?,
        &components_by_parser.len(),
        "generated current parser-component inventory",
    )?;
    require_equal(
        &count_struct_initializers(symbols, "LanguageSymbolRoute")?,
        &lock.current_modes.len(),
        "generated current symbol-route inventory",
    )?;
    for component in &lock.parser_components {
        require_generated_row(
            symbols,
            &expected_parser_component_row(component),
            "generated current parser component",
        )?;
    }
    for mode in &lock.current_modes {
        require_generated_row(
            symbols,
            &expected_symbol_route_row(mode),
            "generated current symbol route",
        )?;
    }
    for parser_id in &broad_parser_ids {
        require(
            !symbols.contains(&format!("parser_id: {}", rust_string(parser_id.as_str()))),
            format!(
                "accepted broad parser {} leaked into the generated current parser projection",
                parser_id.as_str()
            ),
        )?;
    }

    let generated_syntax = syn::parse_file(symbols)?;
    let built_in_parser = generated_syntax
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Enum(item) if item.ident == "BuiltInParser" => Some(item),
            _ => None,
        })
        .ok_or_else(|| io::Error::other("generated BuiltInParser is not an enum"))?;
    let expected_variants = lock
        .parser_components
        .iter()
        .map(|component| {
            expected_built_in_parser(component.built_in_parser)
                .rsplit("::")
                .next()
                .ok_or_else(|| io::Error::other("built-in parser variant is malformed"))
                .map(str::to_string)
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let generated_variants = built_in_parser
        .variants
        .iter()
        .map(|variant| variant.ident.to_string())
        .collect::<BTreeSet<_>>();
    require_equal(
        &generated_variants,
        &expected_variants,
        "generated closed BuiltInParser variants",
    )?;
    require(
        generated_syntax
            .items
            .iter()
            .any(|item| matches!(item, syn::Item::Enum(item) if item.ident == "SymbolRoute")),
        "generated SymbolRoute is not a closed enum",
    )?;

    let parser_syntax = syn::parse_file(PARSER_REGISTRY)?;
    require(
        parser_syntax
            .items
            .iter()
            .all(|item| !matches!(item, syn::Item::Trait(_) | syn::Item::Static(_))),
        "parser adapter introduced a trait or static parser registry",
    )?;
    let parser_language = parser_syntax
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(function) if function.sig.ident == "parser_language" => Some(function),
            _ => None,
        })
        .ok_or_else(|| io::Error::other("parser_language adapter is absent"))?;
    require_equal(
        &parser_language.sig.inputs.len(),
        &1_usize,
        "parser_language argument count",
    )?;
    require(
        parser_language.sig.inputs.first().is_some_and(|argument| {
            typed_value_argument_is_exact(argument, "parser", &["BuiltInParser"])
        }),
        "parser_language does not accept the generated closed BuiltInParser",
    )?;
    require(
        matches!(
            &parser_language.sig.output,
            syn::ReturnType::Type(_, output)
                if matches!(output.as_ref(), syn::Type::Path(path) if path_is_exact(&path.path, &["Language"]))
        ),
        "parser_language does not return tree_sitter::Language through its imported type",
    )?;
    let [syn::Stmt::Expr(syn::Expr::Match(parser_match), None)] =
        parser_language.block.stmts.as_slice()
    else {
        return Err(io::Error::other(
            "parser_language is not one direct exhaustive match expression",
        )
        .into());
    };
    require(
        matches!(
            parser_match.expr.as_ref(),
            syn::Expr::Path(path) if path_is_exact(&path.path, &["parser"])
        ),
        "parser_language does not match its closed parser argument directly",
    )?;
    let mut matched_variants = BTreeSet::new();
    for arm in &parser_match.arms {
        require(arm.guard.is_none(), "parser_language match arm has a guard")?;
        let syn::Pat::Path(pattern) = &arm.pat else {
            return Err(io::Error::other(
                "parser_language contains a wildcard, compound, or indirect parser pattern",
            )
            .into());
        };
        let pattern_segments = pattern
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>();
        let [owner, variant] = pattern_segments.as_slice() else {
            return Err(
                io::Error::other("parser_language arm is not BuiltInParser::Variant").into(),
            );
        };
        require_equal(
            owner.as_str(),
            "BuiltInParser",
            "parser_language pattern owner",
        )?;
        require(
            matched_variants.insert(variant.clone()),
            format!("parser_language repeats BuiltInParser::{variant}"),
        )?;
        let syn::Expr::MethodCall(into) = arm.body.as_ref() else {
            return Err(io::Error::other(
                "parser_language arm does not map a compiled grammar constant directly",
            )
            .into());
        };
        require(
            into.method == "into" && into.args.is_empty(),
            "parser_language arm does not call only Language::into on its grammar constant",
        )?;
        let syn::Expr::Path(grammar) = into.receiver.as_ref() else {
            return Err(io::Error::other(
                "parser_language arm uses a factory, lookup, registration call, or indirect value",
            )
            .into());
        };
        let grammar_segments = grammar
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>();
        require(
            grammar.qself.is_none()
                && grammar_segments.len() >= 2
                && grammar_segments
                    .first()
                    .is_some_and(|owner| owner.starts_with("tree_sitter_"))
                && grammar_segments
                    .last()
                    .is_some_and(|constant| constant.starts_with("LANGUAGE")),
            format!(
                "parser_language arm for BuiltInParser::{variant} does not map directly to a tree_sitter_*::LANGUAGE constant"
            ),
        )?;
    }
    require_equal(
        &matched_variants,
        &generated_variants,
        "direct compiled parser match coverage",
    )?;

    let runtime_syntax = syn::parse_file(SYMBOL_RUNTIME)?;
    let tree_sitter_runtime = runtime_syntax
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(function) if function.sig.ident == "extract_tree_sitter_graph" => {
                Some(function)
            }
            _ => None,
        })
        .ok_or_else(|| io::Error::other("tree-sitter runtime caller is absent"))?;
    require(
        tree_sitter_runtime
            .sig
            .inputs
            .iter()
            .any(|argument| typed_value_argument_is_exact(argument, "parser", &["BuiltInParser"])),
        "tree-sitter runtime caller does not accept the closed BuiltInParser",
    )?;
    let mut runtime_visitor = ClosedParserRuntimeVisitor::default();
    runtime_visitor.visit_block(&tree_sitter_runtime.block);
    require_equal(
        &runtime_visitor.parser_language_bindings,
        &1_usize,
        "closed parser runtime adapter binding count",
    )?;
    require_equal(
        &runtime_visitor.parser_language_installs,
        &1_usize,
        "closed parser runtime grammar installation count",
    )?;

    for pack_id in [BROAD_LANGUAGE_PACK_ID, SEMANTIC_PACK_ID] {
        let mut relabeled = lock.clone();
        let component = relabeled
            .parser_components
            .first_mut()
            .ok_or_else(|| io::Error::other("registry has no current parser component"))?;
        let parser_id = component.parser_id.clone();
        component.current_pack_id = PackId(pack_id.to_string());
        require_validation_fragments(
            validate_registry_lock(&relabeled, &accepted),
            "relabeled current parser component",
            &[
                parser_id.as_str(),
                "not a closed default-core in-process parser choice",
            ],
        )?;
    }

    let mut relocated = materialize_accepted_target(serde_json::from_slice(ACCEPTED)?)?;
    let current_mode = lock
        .current_modes
        .iter()
        .find(|mode| matches!(mode.symbols, SymbolPipeline::BuiltIn { .. }))
        .ok_or_else(|| io::Error::other("registry has no built-in route witness"))?;
    let accepted_mode = relocated
        .modes
        .iter()
        .find(|mode| mode.mode_id == current_mode.accepted_mode_id)
        .ok_or_else(|| io::Error::other("built-in accepted mode witness is absent"))?;
    let relocated_parser_id = accepted_mode.parser_id.clone();
    let relocated_mode_id = accepted_mode.mode_id.clone();
    for mode in relocated
        .modes
        .iter_mut()
        .filter(|mode| mode.parser_id == relocated_parser_id)
    {
        mode.pack_id = PackId(BROAD_LANGUAGE_PACK_ID.to_string());
        mode.owner = broad_pack_owner.to_string();
    }
    let relocated_parser = relocated
        .parsers
        .iter_mut()
        .find(|parser| parser.parser_id == relocated_parser_id)
        .ok_or_else(|| io::Error::other("built-in accepted parser witness is absent"))?;
    relocated_parser.pack_id = PackId(BROAD_LANGUAGE_PACK_ID.to_string());
    relocated_parser.owner = broad_pack_owner.to_string();
    relocated_parser.evidence_state =
        AcceptedParserEvidenceState::PendingAssetFixtureAndPlatformVerification;
    validate_accepted_target(&lock, &relocated)?;
    require_validation_fragments(
        validate_registry_lock(&lock, &relocated),
        "coordinated accepted built-in parser relocation",
        &[
            relocated_mode_id.as_str(),
            DEFAULT_CORE_PACK_ID,
            relocated_parser_id.as_str(),
        ],
    )?;

    for (pack_id, process) in [
        (BROAD_LANGUAGE_PACK_ID, "projectatlas"),
        (DEFAULT_CORE_PACK_ID, "supervised-worker"),
        (SEMANTIC_PACK_ID, "projectatlas"),
    ] {
        let result = validate_accepted_mutation(|source| {
            let pack = source["packs"]
                .as_array_mut()
                .and_then(|packs| packs.iter_mut().find(|pack| pack["pack_id"] == pack_id))
                .ok_or_else(|| io::Error::other(format!("accepted pack {pack_id} is absent")))?;
            pack["process"] = json!(process);
            Ok(())
        })?
        .map(|_| ());
        require_validation_fragments(
            result,
            "accepted pack process mutation",
            &[pack_id, "required process boundary"],
        )?;
    }

    let mut broad_in_process = lock.clone();
    let broad_pack = broad_in_process
        .packs
        .iter_mut()
        .find(|pack| pack.pack_id.as_str() == BROAD_LANGUAGE_PACK_ID)
        .ok_or_else(|| io::Error::other("broad lock pack is absent"))?;
    broad_pack.runtime = PackRuntime::InProcess;
    require_validation_fragments(
        validate_registry_lock(&broad_in_process, &accepted),
        "in-process broad lock pack",
        &[BROAD_LANGUAGE_PACK_ID, "required runtime boundary"],
    )?;

    let mut runtime_selected: serde_json::Value = serde_json::from_slice(LOCK)?;
    runtime_selected["parser_components"][0]["implementation"] = json!("runtime-selected");
    let runtime_selected_bytes = serde_json::to_vec(&runtime_selected)?;
    let inputs = owned_inputs()?;
    let runtime_selected_result = validate_and_generate(&runtime_selected_bytes, &inputs.fixed());
    require(
        matches!(
            &runtime_selected_result,
            Err(LanguageRegistryError::JsonDecode { label, source })
                if *label == "language registry lock"
                    && source.to_string().contains("unknown variant `runtime-selected`")
        ),
        format!(
            "runtime-selected parser implementation did not fail typed decoding: {:?}",
            runtime_selected_result.as_ref().err()
        ),
    )?;

    let broad_suffix = broad_parsers
        .first()
        .and_then(|parser| parser.parser_id.as_str().strip_prefix("parse."))
        .ok_or_else(|| io::Error::other("broad parser suffix is absent"))?;
    require(
        !lock
            .parser_components
            .iter()
            .any(|component| component.built_in_parser.contract_tag() == broad_suffix),
        "broad parser witness unexpectedly belongs to the built-in parser set",
    )?;
    let mut broad_route: serde_json::Value = serde_json::from_slice(LOCK)?;
    let built_in_route = broad_route["current_modes"]
        .as_array_mut()
        .and_then(|modes| {
            modes
                .iter_mut()
                .find(|mode| mode["symbols"]["kind"] == "built-in")
        })
        .ok_or_else(|| io::Error::other("raw lock has no built-in route witness"))?;
    built_in_route["symbols"]["parser"] = json!(broad_suffix);
    let broad_route_bytes = serde_json::to_vec(&broad_route)?;
    let broad_route_result = validate_and_generate(&broad_route_bytes, &inputs.fixed());
    require(
        matches!(
            &broad_route_result,
            Err(LanguageRegistryError::JsonDecode { label, source })
                if *label == "language registry lock"
                    && source.to_string().contains("unknown variant")
                    && source.to_string().contains(broad_suffix)
        ),
        format!(
            "broad parser entered a typed current built-in route: {:?}",
            broad_route_result.as_ref().err()
        ),
    )?;

    let mut broad_pending = materialize_accepted_target(serde_json::from_slice(ACCEPTED)?)?;
    let parser = broad_pending
        .parsers
        .iter_mut()
        .find(|parser| parser.pack_id.as_str() == BROAD_LANGUAGE_PACK_ID)
        .ok_or_else(|| io::Error::other("accepted broad parser is absent"))?;
    let broad_pending_id = parser.parser_id.clone();
    parser.evidence_state = AcceptedParserEvidenceState::Pending;
    require_validation_fragments(
        validate_accepted_target(&lock, &broad_pending),
        "broad parser lifecycle downgrade",
        &[
            broad_pending_id.as_str(),
            "pending asset, fixture, and platform verification",
        ],
    )?;

    let mut broad_advertised = materialize_accepted_target(serde_json::from_slice(ACCEPTED)?)?;
    let parser = broad_advertised
        .parsers
        .iter_mut()
        .find(|parser| parser.pack_id.as_str() == BROAD_LANGUAGE_PACK_ID)
        .ok_or_else(|| io::Error::other("accepted broad parser is absent"))?;
    let broad_advertised_id = parser.parser_id.clone();
    parser.advertised = true;
    require_validation_fragments(
        validate_accepted_target(&lock, &broad_advertised),
        "premature broad parser advertisement",
        &[broad_advertised_id.as_str(), "unadvertised"],
    )?;

    let mut mismatched_component = lock.clone();
    let component = mismatched_component
        .parser_components
        .first_mut()
        .ok_or_else(|| io::Error::other("registry has no parser component"))?;
    let built_in = component.built_in_parser;
    component.parser_id = ParserId::try_from("parser.builtin.mismatch".to_string())?;
    require_validation_fragments(
        validate_registry_lock(&mismatched_component, &accepted),
        "mismatched built-in parser component identity",
        &[
            "parser.builtin.mismatch",
            built_in.contract_tag(),
            "does not match built-in parser identity",
        ],
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
            return Ok(());
        }
        #[cfg(not(windows))]
        return Err(source.into());
    }
    require(
        matches!(
            RegistryWorkspace::new(&linked_root),
            Err(LanguageRegistryError::Validation(message))
                if message.contains("not a regular non-reparse directory")
        ),
        "linked/reparse registry root was accepted",
    )?;
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
            fs::write(&lock_path, [LOCK, b"\n".as_slice()].concat())?;
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
        format!("input drift did not trigger successful compensation: {result:?}"),
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
