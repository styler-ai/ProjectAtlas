//! Bounded repository compiler-configuration loading for semantic graph projection.

use super::{
    CliError, IndexWorkControl, IndexWorkFailure, IndexWorkResource, IndexWorkStage, Node,
    NodeKind, SourceReadFailure, read_source_bytes_controlled, repo_path_to_native,
    source_changed_during_derivation,
};
use jsonc_parser::{ParseOptions as JsoncParseOptions, parse_to_serde_value};
use projectatlas_symbols::{
    ConfiguredModuleResolution, EcmaScriptConfigKind, EcmaScriptModuleConfig,
    EcmaScriptPathMapping, MAX_CONFIGURED_MODULE_CONFIGS, MAX_CONFIGURED_MODULE_MAPPINGS,
    MAX_CONFIGURED_MODULE_TARGETS,
};
use serde_json::Value;
use std::path::Path;

/// Maximum complete bytes admitted from one compiler configuration.
const MAX_MODULE_CONFIG_FILE_BYTES: u64 = 1024 * 1024;
/// Maximum aggregate compiler-configuration bytes retained by one graph stage.
const MAX_MODULE_CONFIG_TOTAL_BYTES: u64 = 16 * 1024 * 1024;
/// Work interval between cancellation/deadline checks while decoding mappings.
const MODULE_CONFIG_CONTROL_INTERVAL: usize = 64;

/// Load one deterministic compiler-configuration snapshot from indexed nodes.
pub(super) fn load_configured_module_resolution(
    root: &Path,
    nodes: &[Node],
    control: &IndexWorkControl,
) -> Result<ConfiguredModuleResolution, CliError> {
    let mut config_nodes = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::File)
        .filter_map(|node| config_kind(&node.path).map(|kind| (node, kind)))
        .collect::<Vec<_>>();
    config_nodes.sort_by(|(left, _), (right, _)| left.path.cmp(&right.path));
    if config_nodes.len() > MAX_CONFIGURED_MODULE_CONFIGS {
        return Err(module_config_resource_limit(
            IndexWorkResource::Entries,
            MAX_CONFIGURED_MODULE_CONFIGS,
            config_nodes.len(),
        ));
    }

    let mut total_bytes = 0_u64;
    let mut total_mappings = 0_u64;
    let mut configs = Vec::with_capacity(config_nodes.len());
    for (index, (node, kind)) in config_nodes.into_iter().enumerate() {
        check_config_work(control, index)?;
        let size = node.size_bytes.unwrap_or_default();
        if size > MAX_MODULE_CONFIG_FILE_BYTES {
            return Err(module_config_resource_limit(
                IndexWorkResource::SourceBytes,
                MAX_MODULE_CONFIG_FILE_BYTES,
                size,
            ));
        }
        claim_module_config_total(
            &mut total_bytes,
            size,
            MAX_MODULE_CONFIG_TOTAL_BYTES,
            IndexWorkResource::SourceBytes,
        )?;
        let native_path = root.join(repo_path_to_native(&node.path));
        let bytes = match read_source_bytes_controlled(
            &native_path,
            MAX_MODULE_CONFIG_FILE_BYTES,
            IndexWorkStage::SymbolParsing,
            control,
        ) {
            Ok(bytes) => bytes,
            Err(SourceReadFailure::Io(source)) => {
                return Err(CliError::Io {
                    path: native_path,
                    source,
                });
            }
            Err(SourceReadFailure::IndexWork(failure)) => return Err(failure.into()),
            Err(SourceReadFailure::LimitExceeded { observed }) => {
                return Err(module_config_resource_limit(
                    IndexWorkResource::SourceBytes,
                    MAX_MODULE_CONFIG_FILE_BYTES,
                    observed,
                ));
            }
        };
        let current_hash = blake3::hash(&bytes).to_hex().to_string();
        if node.content_hash.as_deref() != Some(current_hash.as_str()) {
            return Err(source_changed_during_derivation(root, &node.path));
        }
        let content = String::from_utf8(bytes).map_err(|_utf8_error| {
            CliError::InvalidInput(format!(
                "compiler configuration '{}' is not valid UTF-8",
                node.path
            ))
        })?;
        let value: Value =
            parse_to_serde_value(&content, &JsoncParseOptions::default()).map_err(|error| {
                CliError::InvalidInput(format!(
                    "failed to parse compiler configuration '{}': {error}",
                    node.path
                ))
            })?;
        let config = decode_config(node.path.as_str(), kind, &value, control)?;
        claim_module_config_total(
            &mut total_mappings,
            u64::try_from(config.1).unwrap_or(u64::MAX),
            u64::try_from(MAX_CONFIGURED_MODULE_MAPPINGS).unwrap_or(u64::MAX),
            IndexWorkResource::RelationRows,
        )?;
        configs.push(config.0);
    }
    ConfiguredModuleResolution::new(configs)
        .map_err(|error| CliError::InvalidInput(error.to_string()))
}

/// Decode direct `compilerOptions.baseUrl` and `compilerOptions.paths`.
fn decode_config(
    config_path: &str,
    kind: EcmaScriptConfigKind,
    value: &Value,
    control: &IndexWorkControl,
) -> Result<(EcmaScriptModuleConfig, usize), CliError> {
    let root = value.as_object().ok_or_else(|| {
        CliError::InvalidInput(format!(
            "compiler configuration '{config_path}' must contain a JSON object"
        ))
    })?;
    let Some(compiler_options) = root.get("compilerOptions") else {
        return EcmaScriptModuleConfig::new(config_path, kind, None, Vec::new())
            .map(|config| (config, 0))
            .map_err(|error| CliError::InvalidInput(error.to_string()));
    };
    let compiler_options = compiler_options.as_object().ok_or_else(|| {
        CliError::InvalidInput(format!(
            "compiler configuration '{config_path}' field 'compilerOptions' must be an object"
        ))
    })?;
    let config_directory = config_path
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    let base_url = compiler_options
        .get("baseUrl")
        .map(|value| {
            value.as_str().ok_or_else(|| {
                CliError::InvalidInput(format!(
                    "compiler configuration '{config_path}' field 'baseUrl' must be a string"
                ))
            })
        })
        .transpose()?
        .map(|base_url| normalize_repository_target(config_directory, base_url, false))
        .transpose()?;
    let mapping_base = base_url.as_deref().unwrap_or(config_directory);
    let mut mappings = Vec::new();
    if let Some(paths) = compiler_options.get("paths") {
        let paths = paths.as_object().ok_or_else(|| {
            CliError::InvalidInput(format!(
                "compiler configuration '{config_path}' field 'paths' must be an object"
            ))
        })?;
        for (index, (pattern, targets)) in paths.iter().enumerate() {
            check_config_work(control, index)?;
            let targets = targets.as_array().ok_or_else(|| {
                CliError::InvalidInput(format!(
                    "compiler configuration '{config_path}' path mapping '{pattern}' must be an array"
                ))
            })?;
            if targets.is_empty() || targets.len() > MAX_CONFIGURED_MODULE_TARGETS {
                return Err(module_config_resource_limit(
                    IndexWorkResource::RelationRows,
                    MAX_CONFIGURED_MODULE_TARGETS,
                    targets.len(),
                ));
            }
            let targets = targets
                .iter()
                .map(|target| {
                    let target = target.as_str().ok_or_else(|| {
                        CliError::InvalidInput(format!(
                            "compiler configuration '{config_path}' path mapping '{pattern}' contains a non-string target"
                        ))
                    })?;
                    normalize_repository_target(mapping_base, target, true)
                })
                .collect::<Result<Vec<_>, _>>()?;
            mappings.push(
                EcmaScriptPathMapping::new(pattern, targets)
                    .map_err(|error| CliError::InvalidInput(error.to_string()))?,
            );
        }
    }
    let mapping_count = mappings.len();
    EcmaScriptModuleConfig::new(config_path, kind, base_url, mappings)
        .map(|config| (config, mapping_count))
        .map_err(|error| CliError::InvalidInput(error.to_string()))
}

/// Normalize one config-relative target into a repository-contained lexical path.
fn normalize_repository_target(
    base: &str,
    target: &str,
    allow_wildcard: bool,
) -> Result<String, CliError> {
    let target = target.replace('\\', "/");
    if target.starts_with('/') || target.contains(':') || target.contains('\0') {
        return Err(CliError::InvalidInput(format!(
            "configured module target {target:?} is absolute or invalid"
        )));
    }
    if !allow_wildcard && target.contains('*') || target.matches('*').count() > 1 {
        return Err(CliError::InvalidInput(format!(
            "configured module target {target:?} contains an invalid wildcard"
        )));
    }
    let mut components = base
        .split('/')
        .filter(|component| !component.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    for component in target.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(CliError::InvalidInput(format!(
                        "configured module target {target:?} escapes the repository"
                    )));
                }
            }
            component => components.push(component.to_string()),
        }
    }
    Ok(components.join("/"))
}

/// Classify exact compiler-configuration basenames.
fn config_kind(path: &str) -> Option<EcmaScriptConfigKind> {
    match path.rsplit('/').next() {
        Some("tsconfig.json") => Some(EcmaScriptConfigKind::TypeScript),
        Some("jsconfig.json") => Some(EcmaScriptConfigKind::JavaScript),
        _ => None,
    }
}

/// Observe cancellation and deadline ownership at a bounded work interval.
fn check_config_work(control: &IndexWorkControl, index: usize) -> Result<(), CliError> {
    if index.is_multiple_of(MODULE_CONFIG_CONTROL_INTERVAL) {
        control.check(IndexWorkStage::SymbolParsing)?;
    }
    Ok(())
}

/// Admit aggregate compiler-configuration work without saturating past its bound.
fn claim_module_config_total(
    total: &mut u64,
    added: u64,
    limit: u64,
    resource: IndexWorkResource,
) -> Result<(), CliError> {
    let observed = total.saturating_add(added);
    if observed > limit {
        return Err(module_config_resource_limit(resource, limit, observed));
    }
    *total = observed;
    Ok(())
}

/// Translate configured-module bounds into the runtime's typed limit failure.
fn module_config_resource_limit(
    resource: IndexWorkResource,
    limit: impl TryInto<u64>,
    observed: impl TryInto<u64>,
) -> CliError {
    IndexWorkFailure::resource_limit(
        IndexWorkStage::SymbolParsing,
        resource,
        limit.try_into().unwrap_or(u64::MAX),
        observed.try_into().unwrap_or(u64::MAX),
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::{
        CliError, MAX_MODULE_CONFIG_FILE_BYTES, MAX_MODULE_CONFIG_TOTAL_BYTES,
        claim_module_config_total, decode_config, load_configured_module_resolution,
        normalize_repository_target,
    };
    use projectatlas_core::{
        IndexCancellation, IndexWorkControl, IndexWorkFailure, IndexWorkResource, IndexWorkStage,
        Node, NodeKind,
    };
    use projectatlas_symbols::{
        EcmaScriptConfigKind, MAX_CONFIGURED_MODULE_CONFIGS, MAX_CONFIGURED_MODULE_MAPPINGS,
        MAX_CONFIGURED_MODULE_TARGETS,
    };
    use serde_json::json;
    use std::error::Error;
    use std::fs;
    use std::path::Path;
    use std::time::Instant;

    fn config_node(path: &str, content: &[u8]) -> Node {
        Node {
            path: path.to_string(),
            kind: NodeKind::File,
            parent_path: Path::new(path)
                .parent()
                .and_then(Path::to_str)
                .filter(|parent| !parent.is_empty())
                .map(ToString::to_string),
            extension: Some(".json".to_string()),
            language: Some("json".to_string()),
            size_bytes: Some(u64::try_from(content.len()).unwrap_or(u64::MAX)),
            mtime_ns: Some(1),
            content_hash: Some(blake3::hash(content).to_hex().to_string()),
        }
    }

    #[test]
    fn direct_json_config_is_normalized_without_leaving_the_repository()
    -> Result<(), Box<dyn Error>> {
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        let (config, mappings) = decode_config(
            "packages/app/tsconfig.json",
            EcmaScriptConfigKind::TypeScript,
            &json!({
                "compilerOptions": {
                    "baseUrl": "src",
                    "paths": {
                        "@/*": ["*"],
                        "@shared/*": ["../../shared/*"]
                    }
                }
            }),
            &control,
        )?;
        if mappings != 2 {
            return Err(std::io::Error::other("expected both configured path mappings").into());
        }
        let _configured = projectatlas_symbols::ConfiguredModuleResolution::new(vec![config])?;
        Ok(())
    }

    #[test]
    fn lexical_target_normalization_rejects_root_escape() {
        let contained = normalize_repository_target("packages/app", "../shared/*", true);
        assert!(contained.is_ok());
        assert_eq!(contained.ok().as_deref(), Some("packages/shared/*"));
        assert_eq!(
            normalize_repository_target("src", "shared folder/*", true)
                .ok()
                .as_deref(),
            Some("src/shared folder/*")
        );
        assert!(normalize_repository_target("", "../outside/*", true).is_err());
        assert!(normalize_repository_target("src", "C:/outside/*", true).is_err());
    }

    #[test]
    fn config_decode_observes_typed_cancellation_before_mapping_work() {
        let control = IndexWorkControl::new(IndexCancellation::new(), None);
        control.cancel();
        let result = decode_config(
            "tsconfig.json",
            EcmaScriptConfigKind::TypeScript,
            &json!({
                "compilerOptions": {
                    "paths": {
                        "@/*": ["src/*"]
                    }
                }
            }),
            &control,
        );
        assert!(matches!(
            result,
            Err(CliError::IndexWork(IndexWorkFailure::Cancelled {
                stage: IndexWorkStage::SymbolParsing
            }))
        ));
    }

    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn configured_module_loader_enforces_deadline_currentness_and_bounds()
    -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let content = br#"{"compilerOptions":{"paths":{"@/*":["src/*"]}}}"#;
        fs::write(temp.path().join("tsconfig.json"), content)?;
        let node = config_node("tsconfig.json", content);

        let expired = IndexWorkControl::with_deadline(IndexCancellation::new(), Instant::now());
        assert!(matches!(
            load_configured_module_resolution(temp.path(), std::slice::from_ref(&node), &expired),
            Err(CliError::IndexWork(IndexWorkFailure::DeadlineExceeded {
                stage: IndexWorkStage::SymbolParsing
            }))
        ));

        let mut changed = node.clone();
        changed.content_hash = Some(blake3::hash(b"other").to_hex().to_string());
        assert!(
            load_configured_module_resolution(
                temp.path(),
                &[changed],
                &IndexWorkControl::new(IndexCancellation::new(), None),
            )
            .is_err()
        );

        let mut oversized = node;
        oversized.size_bytes = Some(MAX_MODULE_CONFIG_FILE_BYTES + 1);
        assert!(matches!(
            load_configured_module_resolution(
                temp.path(),
                &[oversized],
                &IndexWorkControl::new(IndexCancellation::new(), None),
            ),
            Err(CliError::IndexWork(
                IndexWorkFailure::ResourceLimitExceeded {
                    resource: IndexWorkResource::SourceBytes,
                    ..
                }
            ))
        ));

        let excessive_configs = (0..=MAX_CONFIGURED_MODULE_CONFIGS)
            .map(|index| config_node(&format!("config-{index}/tsconfig.json"), content))
            .collect::<Vec<_>>();
        assert!(matches!(
            load_configured_module_resolution(
                temp.path(),
                &excessive_configs,
                &IndexWorkControl::new(IndexCancellation::new(), None),
            ),
            Err(CliError::IndexWork(
                IndexWorkFailure::ResourceLimitExceeded {
                    resource: IndexWorkResource::Entries,
                    ..
                }
            ))
        ));

        let mut total = MAX_MODULE_CONFIG_TOTAL_BYTES;
        assert!(matches!(
            claim_module_config_total(
                &mut total,
                1,
                MAX_MODULE_CONFIG_TOTAL_BYTES,
                IndexWorkResource::SourceBytes,
            ),
            Err(CliError::IndexWork(
                IndexWorkFailure::ResourceLimitExceeded {
                    resource: IndexWorkResource::SourceBytes,
                    ..
                }
            ))
        ));
        let mut mappings = u64::try_from(MAX_CONFIGURED_MODULE_MAPPINGS).unwrap_or(u64::MAX);
        assert!(matches!(
            claim_module_config_total(
                &mut mappings,
                1,
                u64::try_from(MAX_CONFIGURED_MODULE_MAPPINGS).unwrap_or(u64::MAX),
                IndexWorkResource::RelationRows,
            ),
            Err(CliError::IndexWork(
                IndexWorkFailure::ResourceLimitExceeded {
                    resource: IndexWorkResource::RelationRows,
                    ..
                }
            ))
        ));

        let excessive_targets = (0..=MAX_CONFIGURED_MODULE_TARGETS)
            .map(|index| format!("src/{index}/*"))
            .collect::<Vec<_>>();
        assert!(matches!(
            decode_config(
                "tsconfig.json",
                EcmaScriptConfigKind::TypeScript,
                &json!({"compilerOptions":{"paths":{"@/*": excessive_targets}}}),
                &IndexWorkControl::new(IndexCancellation::new(), None),
            ),
            Err(CliError::IndexWork(
                IndexWorkFailure::ResourceLimitExceeded {
                    resource: IndexWorkResource::RelationRows,
                    ..
                }
            ))
        ));

        Ok(())
    }
}
