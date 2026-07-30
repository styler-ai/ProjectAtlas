//! Validated repository-configured ECMAScript module scopes.

use crate::resolution_keys::strip_known_source_extension;
use std::error::Error;
use std::fmt;

/// Maximum compiler configuration files admitted to one resolution snapshot.
pub const MAX_CONFIGURED_MODULE_CONFIGS: usize = 1_024;
/// Maximum path mappings admitted across one resolution snapshot.
pub const MAX_CONFIGURED_MODULE_MAPPINGS: usize = 4_096;
/// Maximum target substitutions admitted for one path mapping.
pub const MAX_CONFIGURED_MODULE_TARGETS: usize = 64;
/// Maximum bytes admitted for one normalized pattern or repository path.
pub const MAX_CONFIGURED_MODULE_IDENTITY_BYTES: usize = 1_024;

/// ECMAScript compiler configuration family.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum EcmaScriptConfigKind {
    /// `tsconfig.json`.
    TypeScript,
    /// `jsconfig.json`.
    JavaScript,
}

/// One validated compiler `paths` pattern and its repository-normalized targets.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EcmaScriptPathMapping {
    /// Module specifier pattern from the compiler configuration.
    pattern: String,
    /// Repository-normalized target substitutions.
    targets: Vec<String>,
}

impl EcmaScriptPathMapping {
    /// Construct one bounded mapping.
    ///
    /// # Errors
    ///
    /// Returns an error when the pattern or normalized targets are unsafe or
    /// exceed the per-mapping target bound.
    pub fn new(
        pattern: impl Into<String>,
        targets: Vec<String>,
    ) -> Result<Self, ConfiguredModuleError> {
        let pattern = pattern.into();
        validate_pattern("module pattern", &pattern)?;
        if targets.is_empty() || targets.len() > MAX_CONFIGURED_MODULE_TARGETS {
            return Err(ConfiguredModuleError::TargetCount {
                requested: targets.len(),
            });
        }
        let mut targets = targets;
        for target in &targets {
            validate_repository_pattern("module target", target)?;
        }
        targets.sort();
        targets.dedup();
        Ok(Self { pattern, targets })
    }
}

/// One repository-contained `tsconfig.json` or `jsconfig.json` scope.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EcmaScriptModuleConfig {
    /// Repository-relative compiler configuration path.
    config_path: String,
    /// Repository-relative directory that owns the configuration scope.
    directory: String,
    /// Compiler configuration family used for same-scope precedence.
    kind: EcmaScriptConfigKind,
    /// Repository-normalized `baseUrl`, when configured.
    base_url: Option<String>,
    /// Validated and deterministically ordered path mappings.
    mappings: Vec<EcmaScriptPathMapping>,
}

impl EcmaScriptModuleConfig {
    /// Construct one validated direct compiler configuration.
    ///
    /// `base_url` and every mapping target must already be normalized relative
    /// to the repository root by the filesystem-owning caller.
    ///
    /// # Errors
    ///
    /// Returns an error when any path is absolute, traversing, malformed, or
    /// exceeds the configured identity bounds.
    pub fn new(
        config_path: impl Into<String>,
        kind: EcmaScriptConfigKind,
        base_url: Option<String>,
        mut mappings: Vec<EcmaScriptPathMapping>,
    ) -> Result<Self, ConfiguredModuleError> {
        let config_path = config_path.into();
        validate_repository_path("configuration path", &config_path)?;
        let expected_name = match kind {
            EcmaScriptConfigKind::TypeScript => "tsconfig.json",
            EcmaScriptConfigKind::JavaScript => "jsconfig.json",
        };
        if config_path.rsplit('/').next() != Some(expected_name) {
            return Err(ConfiguredModuleError::InvalidIdentity {
                field: "configuration path",
                value: config_path,
            });
        }
        if let Some(base_url) = base_url.as_deref() {
            validate_repository_path_allow_root("baseUrl", base_url)?;
        }
        mappings.sort();
        mappings.dedup();
        let directory = config_path
            .rsplit_once('/')
            .map_or_else(String::new, |(directory, _)| directory.to_string());
        Ok(Self {
            config_path,
            directory,
            kind,
            base_url,
            mappings,
        })
    }
}

/// Deterministic compiler configuration snapshot shared by graph projections.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConfiguredModuleResolution {
    /// Validated configurations ordered for deterministic selection.
    configs: Vec<EcmaScriptModuleConfig>,
}

impl ConfiguredModuleResolution {
    /// Construct one sorted, bounded configuration snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error when configuration or aggregate mapping counts exceed
    /// their owning bounds.
    pub fn new(mut configs: Vec<EcmaScriptModuleConfig>) -> Result<Self, ConfiguredModuleError> {
        if configs.len() > MAX_CONFIGURED_MODULE_CONFIGS {
            return Err(ConfiguredModuleError::ConfigCount {
                requested: configs.len(),
            });
        }
        let mapping_count = configs
            .iter()
            .map(|config| config.mappings.len())
            .try_fold(0_usize, usize::checked_add)
            .unwrap_or(usize::MAX);
        if mapping_count > MAX_CONFIGURED_MODULE_MAPPINGS {
            return Err(ConfiguredModuleError::MappingCount {
                requested: mapping_count,
            });
        }
        configs.sort();
        configs.dedup();
        Ok(Self { configs })
    }

    /// Resolve configured candidate scopes for one non-relative import.
    pub(crate) fn scopes_for_import(&self, caller_path: &str, module_spec: &str) -> Vec<String> {
        if module_spec.starts_with("./") || module_spec.starts_with("../") || module_spec.is_empty()
        {
            return Vec::new();
        }
        let Some(config) = self.applicable_config(caller_path) else {
            return Vec::new();
        };
        let mut matched = config
            .mappings
            .iter()
            .filter_map(|mapping| {
                match_module_pattern(&mapping.pattern, module_spec)
                    .map(|capture| (mapping, capture))
            })
            .collect::<Vec<_>>();
        if !matched.is_empty() {
            let best_specificity = matched
                .iter()
                .map(|(mapping, _)| pattern_specificity(&mapping.pattern))
                .max()
                .unwrap_or_default();
            matched
                .retain(|(mapping, _)| pattern_specificity(&mapping.pattern) == best_specificity);
            let mut scopes = matched
                .into_iter()
                .flat_map(|(mapping, capture)| {
                    mapping.targets.iter().map(move |target| match capture {
                        ModulePatternMatch::Exact => target.clone(),
                        ModulePatternMatch::Wildcard(capture) => target.replacen('*', capture, 1),
                    })
                })
                .map(|scope| strip_known_source_extension(&scope))
                .collect::<Vec<_>>();
            scopes.sort();
            scopes.dedup();
            return scopes;
        }
        let Some(base_url) = config.base_url.as_deref() else {
            return Vec::new();
        };
        let scope = if base_url.is_empty() {
            module_spec.to_string()
        } else {
            format!("{base_url}/{module_spec}")
        };
        vec![strip_known_source_extension(&scope)]
    }

    /// Select the nearest containing config, preferring the source-family kind
    /// only when both kinds exist at that same scope.
    fn applicable_config(&self, caller_path: &str) -> Option<&EcmaScriptModuleConfig> {
        let preferred = preferred_config_kind(caller_path);
        self.configs
            .iter()
            .filter(|config| directory_contains(&config.directory, caller_path))
            .max_by(|left, right| {
                directory_depth(&left.directory)
                    .cmp(&directory_depth(&right.directory))
                    .then_with(|| {
                        (left.kind == preferred)
                            .cmp(&(right.kind == preferred))
                            .then_with(|| right.config_path.cmp(&left.config_path))
                    })
            })
    }
}

/// Typed validation failure for configured module resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfiguredModuleError {
    /// One identity was not a bounded normalized repository value.
    InvalidIdentity {
        /// Owning configuration field.
        field: &'static str,
        /// Rejected value.
        value: String,
    },
    /// Too many configuration files were supplied.
    ConfigCount {
        /// Requested count.
        requested: usize,
    },
    /// Too many aggregate path mappings were supplied.
    MappingCount {
        /// Requested count.
        requested: usize,
    },
    /// One mapping supplied too many or no targets.
    TargetCount {
        /// Requested count.
        requested: usize,
    },
}

impl fmt::Display for ConfiguredModuleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentity { field, value } => {
                write!(formatter, "invalid configured module {field}: {value:?}")
            }
            Self::ConfigCount { requested } => write!(
                formatter,
                "configured module snapshot contains {requested} configs; maximum is {MAX_CONFIGURED_MODULE_CONFIGS}"
            ),
            Self::MappingCount { requested } => write!(
                formatter,
                "configured module snapshot contains {requested} mappings; maximum is {MAX_CONFIGURED_MODULE_MAPPINGS}"
            ),
            Self::TargetCount { requested } => write!(
                formatter,
                "configured module mapping contains {requested} targets; admitted range is 1..={MAX_CONFIGURED_MODULE_TARGETS}"
            ),
        }
    }
}

impl Error for ConfiguredModuleError {}

/// Match result that preserves whether wildcard substitution is required.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ModulePatternMatch<'a> {
    /// The complete module specifier matched a non-wildcard pattern.
    Exact,
    /// A wildcard matched this bounded portion of the module specifier.
    Wildcard(
        /// Source text captured by the pattern wildcard.
        &'a str,
    ),
}

/// Choose the compiler configuration family preferred by the caller extension.
fn preferred_config_kind(path: &str) -> EcmaScriptConfigKind {
    if matches!(path.rsplit('.').next(), Some("js" | "jsx" | "mjs" | "cjs")) {
        EcmaScriptConfigKind::JavaScript
    } else {
        EcmaScriptConfigKind::TypeScript
    }
}

/// Return whether a configuration directory contains the caller path.
fn directory_contains(directory: &str, path: &str) -> bool {
    directory.is_empty()
        || path
            .strip_prefix(directory)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Count normalized path components for nearest-config selection.
fn directory_depth(directory: &str) -> usize {
    directory
        .split('/')
        .filter(|component| !component.is_empty())
        .count()
}

/// Rank a path pattern by the literal prefix before its optional wildcard.
fn pattern_specificity(pattern: &str) -> usize {
    pattern.find('*').unwrap_or(pattern.len())
}

/// Match one compiler path pattern against an imported module specifier.
fn match_module_pattern<'a>(pattern: &str, module_spec: &'a str) -> Option<ModulePatternMatch<'a>> {
    let Some((prefix, suffix)) = pattern.split_once('*') else {
        return (pattern == module_spec).then_some(ModulePatternMatch::Exact);
    };
    module_spec
        .strip_prefix(prefix)?
        .strip_suffix(suffix)
        .map(ModulePatternMatch::Wildcard)
}

/// Validate one bounded compiler pattern with at most one wildcard.
fn validate_pattern(field: &'static str, value: &str) -> Result<(), ConfiguredModuleError> {
    validate_compact(field, value)?;
    if value.matches('*').count() > 1 {
        return Err(ConfiguredModuleError::InvalidIdentity {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

/// Validate one compiler pattern that must stay within repository scope.
fn validate_repository_pattern(
    field: &'static str,
    value: &str,
) -> Result<(), ConfiguredModuleError> {
    validate_pattern(field, value)?;
    validate_repository_components(field, value)
}

/// Validate one non-root repository-relative path.
fn validate_repository_path(field: &'static str, value: &str) -> Result<(), ConfiguredModuleError> {
    validate_compact(field, value)?;
    if value.contains('*') {
        return Err(ConfiguredModuleError::InvalidIdentity {
            field,
            value: value.to_string(),
        });
    }
    validate_repository_components(field, value)
}

/// Validate one repository-relative path while admitting the root identity.
fn validate_repository_path_allow_root(
    field: &'static str,
    value: &str,
) -> Result<(), ConfiguredModuleError> {
    if value.is_empty() {
        return Ok(());
    }
    validate_repository_path(field, value)
}

/// Validate common size, separator, and control-character constraints.
fn validate_compact(field: &'static str, value: &str) -> Result<(), ConfiguredModuleError> {
    if value.is_empty()
        || value.len() > MAX_CONFIGURED_MODULE_IDENTITY_BYTES
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err(ConfiguredModuleError::InvalidIdentity {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

/// Reject absolute, drive-qualified, empty, current, and parent components.
fn validate_repository_components(
    field: &'static str,
    value: &str,
) -> Result<(), ConfiguredModuleError> {
    if value.starts_with('/')
        || value.contains(':')
        || value
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(ConfiguredModuleError::InvalidIdentity {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ConfiguredModuleResolution, EcmaScriptConfigKind, EcmaScriptModuleConfig,
        EcmaScriptPathMapping,
    };
    use std::error::Error;

    #[test]
    fn nearest_config_and_most_specific_pattern_win() -> Result<(), Box<dyn Error>> {
        let root = EcmaScriptModuleConfig::new(
            "tsconfig.json",
            EcmaScriptConfigKind::TypeScript,
            Some("src".to_string()),
            vec![EcmaScriptPathMapping::new(
                "@/*",
                vec!["src/*".to_string()],
            )?],
        )?;
        let nested = EcmaScriptModuleConfig::new(
            "packages/app/tsconfig.json",
            EcmaScriptConfigKind::TypeScript,
            Some("packages/app/src".to_string()),
            vec![
                EcmaScriptPathMapping::new("@/*", vec!["packages/app/src/*".to_string()])?,
                EcmaScriptPathMapping::new(
                    "@/models/*",
                    vec!["packages/shared/models/*".to_string()],
                )?,
            ],
        )?;
        let configured = ConfiguredModuleResolution::new(vec![root, nested])?;
        if configured.scopes_for_import("packages/app/src/page.ts", "@/models/user")
            != vec!["packages/shared/models/user"]
        {
            return Err(std::io::Error::other("nearest config or specific pattern lost").into());
        }
        if configured.scopes_for_import("src/page.ts", "@/controller") != vec!["src/controller"] {
            return Err(std::io::Error::other("root alias did not resolve").into());
        }
        Ok(())
    }

    #[test]
    fn source_family_preference_and_index_targets_are_deterministic() -> Result<(), Box<dyn Error>>
    {
        let ts = EcmaScriptModuleConfig::new(
            "tsconfig.json",
            EcmaScriptConfigKind::TypeScript,
            None,
            vec![EcmaScriptPathMapping::new(
                "@/*",
                vec!["src/typed/*/index.ts".to_string()],
            )?],
        )?;
        let js = EcmaScriptModuleConfig::new(
            "jsconfig.json",
            EcmaScriptConfigKind::JavaScript,
            None,
            vec![EcmaScriptPathMapping::new(
                "@/*",
                vec!["src/script/*/index.js".to_string()],
            )?],
        )?;
        let configured = ConfiguredModuleResolution::new(vec![ts, js])?;
        if configured.scopes_for_import("src/page.tsx", "@/tools") != vec!["src/typed/tools/index"]
        {
            return Err(std::io::Error::other("TypeScript config preference lost").into());
        }
        if configured.scopes_for_import("src/page.jsx", "@/tools") != vec!["src/script/tools/index"]
        {
            return Err(std::io::Error::other("JavaScript config preference lost").into());
        }
        Ok(())
    }

    #[test]
    fn unsafe_or_excessive_targets_are_rejected() {
        assert!(EcmaScriptPathMapping::new("@/*", vec!["../outside/*".to_string()]).is_err());
        assert!(
            EcmaScriptPathMapping::new("@/*", (0..65).map(|i| format!("src/{i}/*")).collect())
                .is_err()
        );
    }
}
