//! Typed Rust paths and module ownership retained from the existing native AST.

use crate::SourceByteSpan;
use std::collections::HashSet;
use std::sync::Arc;
use tree_sitter::Node;

/// Explicit anchor of a grammar-owned Rust path.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum SourceRustAnchor {
    /// The selected crate target's root module.
    Crate,
    /// The source's actual containing module.
    SelfModule,
    /// A bounded number of containing module parents.
    Super(usize),
    /// An explicit external-prelude root, without guessed crate ownership.
    Absolute,
    /// A bare path whose crate or module ownership needs independent evidence.
    Unqualified,
}

/// A complete source path before deciding whether its final segment is an item.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct SourceRustPath {
    /// Grammar-owned root role, independent from path length.
    pub anchor: SourceRustAnchor,
    /// Ordered identifiers after the explicit anchor.
    pub segments: Vec<String>,
}

impl SourceRustPath {
    /// Retain components from the bounded canonical path built by the AST walker.
    pub(crate) fn from_ast_path(path: &str) -> Option<Self> {
        let absolute = path.starts_with("::");
        let mut components = path
            .strip_prefix("::")
            .unwrap_or(path)
            .split("::")
            .peekable();
        let anchor = if absolute {
            SourceRustAnchor::Absolute
        } else {
            match components.peek().copied() {
                Some("crate") => {
                    components.next();
                    SourceRustAnchor::Crate
                }
                Some("self") => {
                    components.next();
                    SourceRustAnchor::SelfModule
                }
                Some("super") => {
                    let mut parents = 0;
                    while components.peek() == Some(&"super") {
                        components.next();
                        parents += 1;
                    }
                    SourceRustAnchor::Super(parents)
                }
                _ => SourceRustAnchor::Unqualified,
            }
        };
        let segments = components.map(ToString::to_string).collect::<Vec<_>>();
        if segments.iter().any(String::is_empty) {
            return None;
        }
        Some(Self { anchor, segments })
    }

    /// Count owned path identifiers without serializing source facts.
    pub(crate) fn retained_bytes(&self) -> u64 {
        self.segments.iter().fold(
            (self.segments.capacity() * std::mem::size_of::<String>()) as u64,
            |bytes, part| bytes.saturating_add(part.capacity() as u64),
        )
    }
}

/// Meaning of one Rust use-tree leaf, before value/module lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum SourceRustUseRole {
    /// An identifier path may import a value, a type, a module, or several namespaces.
    Item,
    /// Grouped self syntax explicitly imports the prefix's module namespace.
    SelfNamespace,
    /// A wildcard retains dependencies without enumerating guessed export bindings.
    Wildcard,
}

/// One explicit use binding, paired with its existing import occurrence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceRustUse {
    /// Complete AST-owned path, without an inferred module/item split.
    pub path: SourceRustPath,
    /// Source leaf role, including grouped self and wildcards.
    pub role: SourceRustUseRole,
    /// Actual local alias, absent for a wildcard or unsupported binding name.
    pub local: Option<String>,
    /// Inline module path inside the owning source file.
    pub module: Arc<[String]>,
    /// Visibility syntax explicitly makes this a re-export as well as an import.
    pub reexport: bool,
    /// Attributes or malformed ownership prevent this occurrence from proving a target.
    pub supported: bool,
}

impl SourceRustUse {
    /// Count owned target and alias text; shared module paths are charged separately.
    pub(crate) fn retained_bytes(&self) -> u64 {
        self.path
            .retained_bytes()
            .saturating_add(self.local.as_ref().map_or(0, String::capacity) as u64)
    }
}

/// Source-proven placement of one Rust module declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceRustModule {
    /// Inline containing module path inside this source file.
    pub parent: Arc<[String]>,
    /// Exact grammar identifier declared by mod syntax.
    pub name: String,
    /// Original module declaration extent.
    pub span: SourceByteSpan,
    /// Inline body extent; absence means an external module file is required.
    pub body: Option<SourceByteSpan>,
    /// Conditional or custom-path ownership is deliberately not guessed.
    pub supported: bool,
    /// Visibility checked independently from callable declaration lookup.
    pub visibility: SourceRustVisibility,
}

/// Visibility forms that can be established without compiler name resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum SourceRustVisibility {
    /// Visible in the containing module and its descendants.
    Private,
    /// Visible throughout this exact crate target.
    Crate,
    /// Explicit unrestricted public visibility.
    Public,
    /// Restricted or malformed visibility needs additional unsupported proof.
    Unsupported,
}

/// One static call path whose source namespace was established lexically.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceRustCall {
    /// Complete static callee path, without a guessed module/item split.
    pub path: SourceRustPath,
    /// Actual inline module owner of the binding's source reference.
    pub module: Arc<[String]>,
    /// Distinguish a declared module binding from an explicit anchored path.
    pub origin: SourceRustCallOrigin,
}

/// Native syntax evidence that establishes a static Rust call path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceRustCallOrigin {
    /// Lexical lookup selected an actual mod declaration.
    DeclaredModule,
    /// The callee explicitly used crate, self or super qualification.
    AnchoredPath,
    /// Mixed module-level namespaces need source-target classification before lookup.
    ModuleLookup,
}

impl SourceRustCall {
    /// Retain the existing direct canonical lookup without discarding inline owners.
    pub(crate) fn reference(&self) -> Option<crate::ImportReference> {
        if self.origin == SourceRustCallOrigin::ModuleLookup {
            return None;
        }
        let (member, segments) = self.path.segments.split_last()?;
        let (anchor, inline) = match self.path.anchor {
            SourceRustAnchor::Crate => ("crate".to_string(), &[][..]),
            SourceRustAnchor::SelfModule => ("self".to_string(), self.module.as_ref()),
            SourceRustAnchor::Super(parents) if parents <= self.module.len() => (
                "self".to_string(),
                &self.module[..self.module.len() - parents],
            ),
            SourceRustAnchor::Super(parents) => (
                (0..parents - self.module.len())
                    .map(|_| "super")
                    .collect::<Vec<_>>()
                    .join("::"),
                &[][..],
            ),
            SourceRustAnchor::Absolute | SourceRustAnchor::Unqualified => return None,
        };
        let mut module = anchor.clone();
        for segment in inline.iter().chain(segments) {
            module.push_str("::");
            module.push_str(segment);
        }
        Some(crate::ImportReference::new(
            crate::ImportSyntax::Rust,
            &module,
            Some(member),
            anchor.split("::").next()?,
        ))
    }
}

/// Source-owned meaning of a Rust module's value slot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceRustBinding {
    /// Actual callable declaration ordinal in the accompanying graph.
    Declaration(usize),
    /// A type namespace owner, which never proves a direct callable value.
    TypeDeclaration(usize),
    /// A type namespace owner survived while its exact declaration proof did not.
    UnresolvedType,
    /// A lost import may occupy either namespace and must remain a competing blocker.
    UnresolvedImport,
    /// A source use binding may forward a value or a namespace.
    Use(Arc<SourceRustUse>),
    /// The source name exists but cannot prove a callable target.
    Unresolved,
}

/// One Rust module value binding, including non-callable blockers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceRustDeclaration {
    /// Actual inline module owner, independent from display parent strings.
    pub module: Arc<[String]>,
    /// Identifier declared in this module's value namespace.
    pub name: String,
    /// Visibility supplied by this declaration's syntax.
    pub visibility: SourceRustVisibility,
    /// Actual source-owned callable or forwarding evidence.
    pub binding: SourceRustBinding,
}

/// One inherent method linked to its owning type by actual impl syntax.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceRustAssociated {
    /// Inline module containing the impl and its local type owner.
    pub module: Arc<[String]>,
    /// Actual local type identifier, excluding inferred type flow.
    pub owner: String,
    /// Actual method identifier.
    pub name: String,
    /// Admitted declaration ordinal; a lost ordinal remains a competing blocker.
    pub declaration: Option<usize>,
    /// Method visibility from source syntax.
    pub visibility: SourceRustVisibility,
    /// False for conditional, transforming or malformed method/impl syntax.
    pub supported: bool,
}

/// One lexical call whose winning scope contains only competing import bindings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceRustCallImports {
    /// Exact raw call relation ordinal.
    pub relation: usize,
    /// Shared (import evidence, reference) ordinals; every candidate must survive remapping.
    pub bindings: Arc<[(usize, usize)]>,
    /// Actual same-scope value declarations competing with those imports.
    pub declarations: Arc<[usize]>,
    /// Complete static member suffix in the call's requested namespace.
    pub member: Option<String>,
}

/// Rust ownership evidence from the same parsed source as lexical calls.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceRustFacts {
    /// Inline and external module declarations in source order.
    pub modules: Vec<SourceRustModule>,
    /// Module value slots, preserving duplicates and unresolved competitors.
    pub declarations: Vec<SourceRustDeclaration>,
    /// Same-source inherent methods with actual local type ownership.
    pub associated: Vec<SourceRustAssociated>,
    /// Scope-owned import competitors, including function/block-local imports.
    pub candidate_calls: Vec<SourceRustCallImports>,
    /// False when ownership or cardinality bounds discarded competing evidence.
    pub complete: bool,
}

/// Cargo target category, retained independently from source filenames.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum SourceCargoTargetKind {
    /// The package's library target.
    Library,
    /// An executable target.
    Binary,
    /// An example target.
    Example,
    /// An integration test target.
    Test,
    /// A benchmark target.
    Benchmark,
    /// The package's build script.
    BuildScript,
}

impl SourceCargoTargetKind {
    /// Return the conventional directory for independently named automatic targets.
    #[must_use]
    pub const fn automatic_directory(self) -> Option<&'static str> {
        match self {
            Self::Binary => Some("src/bin"),
            Self::Example => Some("examples"),
            Self::Test => Some("tests"),
            Self::Benchmark => Some("benches"),
            Self::Library | Self::BuildScript => None,
        }
    }

    /// Return a fixed conventional root whose absence can be tracked explicitly.
    #[must_use]
    pub const fn conventional_root(self) -> Option<&'static str> {
        match self {
            Self::Library => Some("src/lib.rs"),
            Self::Binary => Some("src/main.rs"),
            Self::BuildScript => Some("build.rs"),
            Self::Example | Self::Test | Self::Benchmark => None,
        }
    }
}

/// Canonical invalidation cause for one Cargo target-root directory inventory.
///
/// This is a dependency key, not proof of a call target or crate membership.
///
/// # Errors
///
/// Returns a graph contract error for invalid package, manifest or directory identity.
pub fn cargo_target_inventory_key(
    project: projectatlas_core::graph::ProjectInstanceId,
    package: &str,
    manifest: &str,
    directory: &str,
) -> Result<
    projectatlas_core::graph::CanonicalResolutionKey,
    projectatlas_core::graph::GraphContractError,
> {
    cargo_directory_key(
        project,
        package,
        manifest,
        directory,
        projectatlas_core::graph::GraphRelationKind::from_legacy(
            projectatlas_core::symbols::RelationKind::Imports,
        ),
    )
}

/// Canonical invalidation cause emitted only by admitted explicit sibling roots.
///
/// Its configuration role is distinct from automatic inventory, so an unlisted
/// helper cannot invalidate explicit roots when automatic discovery is disabled.
///
/// # Errors
///
/// Returns a graph contract error for invalid package, manifest or directory identity.
pub fn cargo_root_cohort_key(
    project: projectatlas_core::graph::ProjectInstanceId,
    package: &str,
    manifest: &str,
    directory: &str,
) -> Result<
    projectatlas_core::graph::CanonicalResolutionKey,
    projectatlas_core::graph::GraphContractError,
> {
    cargo_directory_key(
        project,
        package,
        manifest,
        directory,
        projectatlas_core::graph::GraphRelationKind::Extended(
            projectatlas_core::graph::ExtendedRelationKind::Configures,
        ),
    )
}

/// Share text admission while preserving distinct automatic/configured root roles.
fn cargo_directory_key(
    project: projectatlas_core::graph::ProjectInstanceId,
    package: &str,
    manifest: &str,
    directory: &str,
    kind: projectatlas_core::graph::GraphRelationKind,
) -> Result<
    projectatlas_core::graph::CanonicalResolutionKey,
    projectatlas_core::graph::GraphContractError,
> {
    use projectatlas_core::graph::{
        CanonicalResolutionKey, GraphContractError, GraphIdentityText, ResolutionKeyDomain,
    };
    use projectatlas_core::language::SemanticProviderOwner;
    let owner = SemanticProviderOwner::Rust;
    let provider = GraphIdentityText::new(owner.as_str())?;
    let language = GraphIdentityText::new(owner.resolution_family().ok_or(
        GraphContractError::InvalidResolution {
            reason: "Rust has no registered resolution family",
        },
    )?)?;
    let package = GraphIdentityText::new(package)?;
    let manifest = GraphIdentityText::new(manifest)?;
    let directory = GraphIdentityText::new(directory)?;
    Ok(CanonicalResolutionKey::new(
        project,
        ResolutionKeyDomain::Module,
        &provider,
        &language,
        Some(&package),
        Some(&manifest),
        Some(kind),
        &directory,
    ))
}

/// Return the inventory directory for a flat potential automatic Cargo root.
///
/// This classifies source placement, not actual target membership or discovery policy.
///
/// ```
/// assert_eq!(projectatlas_symbols::cargo_automatic_target_directory("src/bin/app.rs", ""), Some("src/bin"));
/// assert_eq!(projectatlas_symbols::cargo_automatic_target_directory("src/bin/app/main.rs", ""), None);
/// ```
#[must_use]
pub fn cargo_automatic_target_directory<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    let relative = if root.is_empty() {
        path
    } else {
        path.strip_prefix(root)?.strip_prefix('/')?
    };
    for kind in [
        SourceCargoTargetKind::Binary,
        SourceCargoTargetKind::Example,
        SourceCargoTargetKind::Test,
        SourceCargoTargetKind::Benchmark,
    ] {
        let Some(directory) = kind.automatic_directory() else {
            continue;
        };
        if let Some(file) = relative
            .strip_prefix(directory)
            .and_then(|suffix| suffix.strip_prefix('/'))
            && !file.contains('/')
            && file
                .strip_suffix(".rs")
                .is_some_and(|stem| !stem.is_empty())
        {
            return path.rsplit_once('/').map(|(parent, _)| parent);
        }
    }
    None
}

/// One manifest-owned crate root candidate, validated against admitted inventory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceCargoTarget {
    /// Actual Cargo target category.
    pub kind: SourceCargoTargetKind,
    /// Manifest-relative source path; no source-root basename inference is needed.
    pub path: String,
    /// Explicit target name used to avoid restoring an overridden inferred root.
    pub name: Option<String>,
}

/// A conventional target category admitted by the manifest's discovery policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceCargoAutomaticTarget {
    /// Category whose conventional source layout may supply roots.
    pub kind: SourceCargoTargetKind,
    /// False when an inherited edition leaves the discovery default unproved.
    pub certain: bool,
}

/// Target configuration captured at the existing Cargo TOML parse boundary.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceCargoTargets {
    /// Explicit package edition, or the Cargo 2015 default; inherited/unknown stays absent.
    pub edition: Option<u16>,
    /// Explicit or manifest-default source root candidates.
    pub targets: Vec<SourceCargoTarget>,
    /// Categories whose conventional directory discovery is enabled.
    pub automatic: Vec<SourceCargoAutomaticTarget>,
    /// False when malformed or excessive target settings prevent exact ownership.
    pub complete: bool,
}

/// Maximum inline ownership depth retained by a source binding proof.
const MAX_SOURCE_MODULE_DEPTH: usize = 64;

/// Find actual inline module ancestors, independent from graph display parents.
pub(crate) fn module_owner(node: Node<'_>, content: &str) -> Option<Arc<[String]>> {
    let mut parts = Vec::new();
    let mut parent = node.parent();
    let mut visited = 0;
    while let Some(current) = parent {
        visited += 1;
        if visited > MAX_SOURCE_MODULE_DEPTH * 4 {
            return None;
        }
        if current.kind() == "mod_item" {
            if !is_module_item(current) {
                return None;
            }
            if parts.len() >= MAX_SOURCE_MODULE_DEPTH {
                return None;
            }
            let name = current.child_by_field_name("name")?;
            parts.push(name.utf8_text(content.as_bytes()).ok()?.to_string());
        }
        parent = current.parent();
    }
    parts.reverse();
    Some(parts.into())
}

/// Charge one shared immutable module path once within its owning projection.
pub(crate) fn module_retained_bytes(
    module: &Arc<[String]>,
    seen: &mut HashSet<*const [String]>,
) -> u64 {
    if !seen.insert(Arc::as_ptr(module)) {
        return 0;
    }
    module.iter().fold(
        (2 * std::mem::size_of::<usize>() + std::mem::size_of_val(module.as_ref())) as u64,
        |bytes, part| bytes.saturating_add(part.capacity() as u64),
    )
}

/// Charge a use target shared by its import occurrence and module declaration slot.
pub(crate) fn use_retained_bytes(
    binding: &Arc<SourceRustUse>,
    uses: &mut HashSet<*const SourceRustUse>,
    modules: &mut HashSet<*const [String]>,
) -> u64 {
    if !uses.insert(Arc::as_ptr(binding)) {
        return 0;
    }
    (2 * std::mem::size_of::<usize>() + std::mem::size_of::<SourceRustUse>()) as u64
        + binding.retained_bytes()
        + module_retained_bytes(&binding.module, modules)
}

impl SourceRustFacts {
    /// Account actual vector capacities, payloads and shared source module/use owners.
    pub(crate) fn retained_bytes(
        &self,
        uses: &mut HashSet<*const SourceRustUse>,
        modules: &mut HashSet<*const [String]>,
    ) -> u64 {
        let mut bytes = (self.modules.capacity() * std::mem::size_of::<SourceRustModule>()
            + self.declarations.capacity() * std::mem::size_of::<SourceRustDeclaration>()
            + self.associated.capacity() * std::mem::size_of::<SourceRustAssociated>()
            + self.candidate_calls.capacity() * std::mem::size_of::<SourceRustCallImports>())
            as u64;
        for module in &self.modules {
            bytes = bytes
                .saturating_add(module.name.capacity() as u64)
                .saturating_add(module_retained_bytes(&module.parent, modules));
        }
        for declaration in &self.declarations {
            bytes = bytes
                .saturating_add(declaration.name.capacity() as u64)
                .saturating_add(module_retained_bytes(&declaration.module, modules));
            if let SourceRustBinding::Use(binding) = &declaration.binding {
                bytes = bytes.saturating_add(use_retained_bytes(binding, uses, modules));
            }
        }
        for method in &self.associated {
            bytes = bytes
                .saturating_add(method.owner.capacity() as u64)
                .saturating_add(method.name.capacity() as u64)
                .saturating_add(module_retained_bytes(&method.module, modules));
        }
        let mut candidates = HashSet::new();
        let mut declarations = HashSet::new();
        for call in &self.candidate_calls {
            bytes = bytes.saturating_add(call.member.as_ref().map_or(0, String::capacity) as u64);
            if declarations.insert(Arc::as_ptr(&call.declarations)) {
                bytes = bytes.saturating_add(
                    (std::mem::size_of_val(call.declarations.as_ref())
                        + 2 * std::mem::size_of::<usize>()) as u64,
                );
            }
            if candidates.insert(Arc::as_ptr(&call.bindings)) {
                bytes = bytes.saturating_add(
                    (std::mem::size_of_val(call.bindings.as_ref())
                        + 2 * std::mem::size_of::<usize>()) as u64,
                );
            }
        }
        bytes
    }
}

impl SourceCargoTargets {
    /// Account manifest target arrays and their retained path/name strings.
    pub(crate) fn retained_bytes(&self) -> u64 {
        self.targets.iter().fold(
            (self.targets.capacity() * std::mem::size_of::<SourceCargoTarget>()
                + self.automatic.capacity() * std::mem::size_of::<SourceCargoAutomaticTarget>())
                as u64,
            |bytes, target| {
                bytes
                    .saturating_add(target.path.capacity() as u64)
                    .saturating_add(target.name.as_ref().map_or(0, String::capacity) as u64)
            },
        )
    }
}

/// Return whether an item belongs to a file or inline module declaration list.
pub(crate) fn is_module_item(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        parent.kind() == "source_file"
            || parent.kind() == "declaration_list"
                && parent
                    .parent()
                    .is_some_and(|owner| owner.kind() == "mod_item")
    })
}

/// Refuse attributes that may transform an item or select a different source owner.
pub(crate) fn has_unsupported_attributes(node: Node<'_>, content: &str) -> bool {
    let mut sibling = node.prev_named_sibling();
    while let Some(previous) = sibling {
        match previous.kind() {
            "attribute_item" | "inner_attribute_item" => {
                let mut cursor = previous.walk();
                let attribute = previous
                    .named_children(&mut cursor)
                    .find(|child| child.kind() == "attribute");
                let name = attribute
                    .and_then(|attribute| attribute.named_child(0))
                    .and_then(|path| crate::rust_use_source::rust_path(path, content, 0));
                if !name.as_deref().is_some_and(|name| {
                    matches!(
                        name,
                        "allow"
                            | "warn"
                            | "deny"
                            | "forbid"
                            | "expect"
                            | "doc"
                            | "inline"
                            | "cold"
                            | "must_use"
                            | "deprecated"
                            | "track_caller"
                            | "repr"
                            | "non_exhaustive"
                            | "rustfmt::skip"
                    )
                }) {
                    return true;
                }
                sibling = previous.prev_named_sibling();
            }
            "line_comment" | "block_comment" => sibling = previous.prev_named_sibling(),
            _ => break,
        }
    }
    false
}

/// A tuple constructor is no more visible than its least visible field.
pub(crate) fn constructor_visibility(node: Node<'_>) -> SourceRustVisibility {
    let declared = visibility(node);
    let Some(body) = node.child_by_field_name("body") else {
        return declared;
    };
    if body.kind() != "ordered_field_declaration_list" {
        return declared;
    }
    let mut result = declared;
    let mut field = SourceRustVisibility::Private;
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            field = visibility_modifier(child);
        } else if child.kind() != "attribute_item" {
            result = match (result, field) {
                (SourceRustVisibility::Unsupported, _) | (_, SourceRustVisibility::Unsupported) => {
                    SourceRustVisibility::Unsupported
                }
                (SourceRustVisibility::Private, _) | (_, SourceRustVisibility::Private) => {
                    SourceRustVisibility::Private
                }
                (SourceRustVisibility::Crate, _) | (_, SourceRustVisibility::Crate) => {
                    SourceRustVisibility::Crate
                }
                _ => SourceRustVisibility::Public,
            };
            field = SourceRustVisibility::Private;
        }
    }
    result
}

/// Read visibility tokens from the same native tree without parsing a signature.
pub(crate) fn visibility(node: Node<'_>) -> SourceRustVisibility {
    let mut cursor = node.walk();
    let Some(visibility) = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "visibility_modifier")
    else {
        return SourceRustVisibility::Private;
    };
    visibility_modifier(visibility)
}

/// Interpret one grammar-owned visibility modifier.
fn visibility_modifier(visibility: Node<'_>) -> SourceRustVisibility {
    if visibility.child_count() == 1 {
        SourceRustVisibility::Public
    } else {
        let mut cursor = visibility.walk();
        if visibility
            .named_children(&mut cursor)
            .any(|child| child.kind() == "crate")
        {
            SourceRustVisibility::Crate
        } else {
            SourceRustVisibility::Unsupported
        }
    }
}

/// Capture target settings from the already parsed Cargo manifest value.
pub(crate) fn cargo_targets<E>(
    root: &toml::map::Map<String, toml::Value>,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<SourceCargoTargets, E> {
    use SourceCargoTargetKind as Kind;
    let mut facts = SourceCargoTargets {
        complete: true,
        ..SourceCargoTargets::default()
    };
    let Some(package) = root.get("package").and_then(toml::Value::as_table) else {
        return Ok(facts);
    };
    facts.edition = match package.get("edition") {
        None => Some(2015),
        Some(toml::Value::String(edition)) => match edition.as_str() {
            "2015" => Some(2015),
            "2018" => Some(2018),
            "2021" => Some(2021),
            "2024" => Some(2024),
            _ => None,
        },
        Some(_) => None,
    };
    let manual = ["lib", "bin", "example", "test", "bench"]
        .iter()
        .any(|key| root.contains_key(*key));
    let default_auto = match package.get("edition") {
        None => Some(!manual),
        Some(toml::Value::String(edition)) if edition == "2015" => Some(!manual),
        Some(toml::Value::String(edition))
            if matches!(edition.as_str(), "2018" | "2021" | "2024") =>
        {
            Some(true)
        }
        Some(toml::Value::Table(edition))
            if edition.get("workspace").and_then(toml::Value::as_bool) == Some(true) =>
        {
            (!manual).then_some(true)
        }
        Some(_) => {
            facts.complete = false;
            None
        }
    };
    for (key, flag, kind, directory) in [
        ("lib", "autolib", Kind::Library, "src"),
        ("bin", "autobins", Kind::Binary, "src/bin"),
        ("example", "autoexamples", Kind::Example, "examples"),
        ("test", "autotests", Kind::Test, "tests"),
        ("bench", "autobenches", Kind::Benchmark, "benches"),
    ] {
        check()?;
        let auto = match package.get(flag) {
            None => default_auto,
            Some(value) => {
                let enabled = value.as_bool();
                if enabled.is_none() {
                    facts.complete = false;
                }
                enabled
            }
        };
        if kind == Kind::Library {
            if let Some(target) = root.get(key) {
                if let Some(target) = target.as_table() {
                    let path = target
                        .get("path")
                        .map_or(Some("src/lib.rs"), toml::Value::as_str);
                    if let Some(path) = path.filter(|path| !path.is_empty()) {
                        facts.targets.push(SourceCargoTarget {
                            kind,
                            path: path.to_string(),
                            name: None,
                        });
                    } else {
                        facts.complete = false;
                    }
                } else {
                    facts.complete = false;
                }
            } else if auto != Some(false) {
                facts.automatic.push(SourceCargoAutomaticTarget {
                    kind,
                    certain: auto == Some(true),
                });
            }
            continue;
        }
        if auto != Some(false) {
            facts.automatic.push(SourceCargoAutomaticTarget {
                kind,
                certain: auto == Some(true),
            });
        }
        let Some(targets) = root.get(key) else {
            continue;
        };
        let Some(targets) = targets.as_array() else {
            facts.complete = false;
            continue;
        };
        for target in targets {
            check()?;
            if facts.targets.len() >= crate::MAX_RELATIONS_PER_FILE {
                facts.complete = false;
                facts.targets.clear();
                return Ok(facts);
            }
            let Some(target) = target.as_table() else {
                facts.complete = false;
                continue;
            };
            let Some(name) = target
                .get("name")
                .and_then(toml::Value::as_str)
                .filter(|name| {
                    !name.is_empty() && !name.contains(['/', '\\']) && *name != "." && *name != ".."
                })
            else {
                facts.complete = false;
                continue;
            };
            if let Some(path) = target.get("path") {
                if let Some(path) = path.as_str().filter(|path| !path.is_empty()) {
                    facts.targets.push(SourceCargoTarget {
                        kind,
                        path: path.to_string(),
                        name: Some(name.to_string()),
                    });
                } else {
                    facts.complete = false;
                }
            } else {
                // These are candidates, not root proof. Inventory and module
                // membership decide which default path actually owns the source.
                for path in [
                    format!("{directory}/{name}.rs"),
                    format!("{directory}/{name}/main.rs"),
                ] {
                    facts.targets.push(SourceCargoTarget {
                        kind,
                        path,
                        name: Some(name.to_string()),
                    });
                }
                if kind == Kind::Binary
                    && package.get("name").and_then(toml::Value::as_str) == Some(name)
                {
                    facts.targets.push(SourceCargoTarget {
                        kind,
                        path: "src/main.rs".to_string(),
                        name: Some(name.to_string()),
                    });
                }
            }
        }
    }
    match package.get("build") {
        Some(toml::Value::Boolean(false)) => {}
        Some(toml::Value::String(path)) if !path.is_empty() => {
            facts.targets.push(SourceCargoTarget {
                kind: Kind::BuildScript,
                path: path.clone(),
                name: None,
            });
        }
        None | Some(toml::Value::Boolean(true)) => facts.targets.push(SourceCargoTarget {
            kind: Kind::BuildScript,
            path: "build.rs".to_string(),
            name: None,
        }),
        Some(_) => facts.complete = false,
    }
    if facts.targets.len() > crate::MAX_RELATIONS_PER_FILE {
        facts.complete = false;
        facts.targets.clear();
    }
    check()?;
    Ok(facts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SourceCallBinding, extract_symbol_graph_with_semantics_controlled};
    use projectatlas_core::{IndexCancellation, IndexWorkControl};
    /// Fail a fixture before using its value, keeping assertion tests infallible.
    macro_rules! fixture_ok {
        ($value:expr) => {{
            let value = $value;
            assert!(value.is_ok(), "{value:?}");
            let Ok(value) = value else {
                return;
            };
            value
        }};
    }

    #[test]
    fn rust_attributes_preserve_inert_items_and_refuse_conditional_bindings() {
        let source = "#[allow(dead_code)] pub mod valid {#[inline] pub fn work(){}}\n#[cfg(any())] use std::mem::drop as omitted; #[cfg(any())] use crate::valid::work as hidden; fn caller(){omitted(1); hidden();}\npub struct Public(pub u8); pub struct Private(u8); pub struct Mixed(pub u8, pub(crate) u8);";
        let result = fixture_ok!(extract_symbol_graph_with_semantics_controlled(
            "src/lib.rs",
            Some("rust"),
            source,
            &IndexWorkControl::new(IndexCancellation::new(), None)
        ));
        let facts = result.semantics();
        let rust = fixture_ok!(facts.rust().ok_or("missing Rust facts"));
        assert!(
            rust.modules
                .iter()
                .any(|module| module.name == "valid" && module.supported)
        );
        assert!(
            rust.declarations
                .iter()
                .any(|declaration| declaration.name == "work"
                    && matches!(declaration.binding, SourceRustBinding::Declaration(_)))
        );
        for (name, visibility) in [
            ("Public", SourceRustVisibility::Public),
            ("Private", SourceRustVisibility::Private),
            ("Mixed", SourceRustVisibility::Crate),
        ] {
            assert!(
                rust.declarations
                    .iter()
                    .any(|declaration| declaration.name == name
                        && declaration.visibility == visibility),
                "{name}"
            );
        }
        assert!(facts.imports().iter().all(|import| {
            import
                .rust
                .as_ref()
                .is_some_and(|binding| !binding.supported)
                && import.rust_toolchain.is_none()
        }));
        assert!(facts.calls().iter().all(|call| !matches!(
            call.binding,
            SourceCallBinding::Imported { .. } | SourceCallBinding::LocalDeclaration(_)
        )));
    }

    #[test]
    fn rust_partial_imports_cannot_narrow_a_retained_alias() {
        let oversized = "nested::".repeat(crate::MAX_SNIPPET_CHARS);
        let source = format!(
            "pub mod first {{pub fn run(){{}}}} use crate::{{first::run as launch, {oversized}run as launch}}; fn caller(){{launch();}}"
        );
        let result = fixture_ok!(extract_symbol_graph_with_semantics_controlled(
            "src/lib.rs",
            Some("rust"),
            &source,
            &IndexWorkControl::new(IndexCancellation::new(), None)
        ));
        let facts = result.semantics();
        assert!(facts.bindings_incomplete);
        assert!(facts.rust().is_some_and(|rust| !rust.complete));
        assert!(facts.imports().iter().any(|import| {
            import
                .references
                .iter()
                .any(|reference| reference.local() == "launch")
        }));
        assert!(!facts.calls().is_empty());
        assert!(
            facts
                .calls()
                .iter()
                .all(|call| matches!(call.binding, SourceCallBinding::Unresolved))
        );
        assert!(facts.matches_graph(result.graph()));
    }

    #[test]
    fn rust_module_boundaries_do_not_inherit_bare_outer_bindings_or_local_module_addresses() {
        let source = "fn run(){} mod global {pub fn run(){}} mod inner {pub fn inherited(){run();} use crate::run as launch; pub fn explicit(){launch();}} fn outer(){mod global {pub fn wrong(){self::run();}}}";
        let result = fixture_ok!(extract_symbol_graph_with_semantics_controlled(
            "src/lib.rs",
            Some("rust"),
            source,
            &IndexWorkControl::new(IndexCancellation::new(), None)
        ));
        let graph = result.graph();
        let facts = result.semantics();
        for name in ["inherited", "wrong"] {
            let ordinal = fixture_ok!(
                graph
                    .symbols
                    .iter()
                    .position(|symbol| symbol.name == name)
                    .ok_or("missing declaration")
            );
            assert!(
                facts.calls().iter().any(|call| call.owner == Some(ordinal)
                    && matches!(call.binding, SourceCallBinding::Unresolved)),
                "{name}"
            );
        }
        let explicit = fixture_ok!(
            graph
                .symbols
                .iter()
                .position(|symbol| symbol.name == "explicit")
                .ok_or("missing explicit caller")
        );
        assert!(facts.calls().iter().any(|call| call.owner == Some(explicit)
            && matches!(call.binding, SourceCallBinding::Imported { .. })));
        assert!(facts.rust().is_some_and(|rust| rust.complete));
    }

    #[test]
    fn automatic_target_inventory_keys_track_only_the_owning_target_directory() {
        let project = fixture_ok!(projectatlas_core::graph::ProjectInstanceId::from_bytes(
            [39; 16]
        ));
        let key = fixture_ok!(cargo_target_inventory_key(
            project,
            "member",
            "crates/member/Cargo.toml",
            "crates/member/tests"
        ));
        for (path, expected) in [
            ("crates/member/tests/new.rs", true),
            ("crates/member/tests/helper/support.rs", false),
            ("crates/membership/tests/new.rs", false),
            ("crates/member/src/service.rs", false),
        ] {
            let graph = crate::extract_symbol_graph(path, Some("rust"), "fn main() {}");
            let projection = fixture_ok!(crate::derive_resolution_keys_with_context(
                project,
                Some("member"),
                &graph,
                crate::ResolutionProjectionContext::default()
                    .with_rust_package("crates/member/Cargo.toml", "crates/member"),
            ));
            assert_eq!(projection.source_keys().contains(&key), expected, "{path}");
        }
    }

    #[test]
    fn toolchain_import_proof_follows_source_roots_and_binding_shadowing() {
        for (source, expected, import_proved) in [
            ("mod std {} fn caller(){std::fs::read(\"x\");}", None, false),
            (
                "mod fake {} use crate::fake as std; fn caller(){std::fs::read(\"x\");}",
                None,
                false,
            ),
            (
                "use std; fn caller(){std::fs::read(\"x\");}",
                Some("std::fs::read"),
                true,
            ),
            (
                "use std::mem::drop as dispose; fn caller(){dispose(1);}",
                Some("std::mem::drop"),
                true,
            ),
            (
                "use core::mem as std; fn caller(){std::drop(1);}",
                Some("core::mem::drop"),
                true,
            ),
            (
                "use std::mem::drop as dispose; fn caller(dispose:fn(u8)){dispose(1);}",
                None,
                true,
            ),
            (
                "mod std {} use std::mem::drop as dispose; fn caller(){dispose(1);}",
                None,
                false,
            ),
        ] {
            let result = fixture_ok!(extract_symbol_graph_with_semantics_controlled(
                "src/lib.rs",
                Some("rust"),
                source,
                &IndexWorkControl::new(IndexCancellation::new(), None)
            ));
            let facts = result.semantics();
            let call = fixture_ok!(facts.calls().last().ok_or("missing call"));
            let actual = match &call.binding {
                SourceCallBinding::Imported { rust_toolchain, .. } => {
                    rust_toolchain.as_ref().map(|reference| {
                        reference.imported().map_or_else(
                            || reference.module().to_string(),
                            |name| format!("{}::{name}", reference.module()),
                        )
                    })
                }
                _ => None,
            };
            assert_eq!(actual.as_deref(), expected, "{source}");
            assert_eq!(
                facts
                    .imports()
                    .iter()
                    .any(|import| import.rust_toolchain.is_some()),
                import_proved,
                "{source}"
            );
        }
    }

    #[test]
    fn use_paths_retain_leaf_roles_before_module_or_item_classification() {
        let source = "pub fn run() {} mod run {pub fn child() {}}\nuse crate::run as flat; use crate::{run as grouped}; use crate::run::{self as namespace};";
        let result = fixture_ok!(extract_symbol_graph_with_semantics_controlled(
            "src/lib.rs",
            Some("rust"),
            source,
            &IndexWorkControl::new(IndexCancellation::new(), None),
        ));
        let uses = result
            .semantics()
            .imports()
            .iter()
            .filter_map(|import| import.rust.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(uses.len(), 3);
        for binding in &uses {
            assert_eq!(binding.path.anchor, SourceRustAnchor::Crate);
            assert_eq!(binding.path.segments, ["run"]);
        }
        assert_eq!(uses[0].local.as_deref(), Some("flat"));
        assert_eq!(uses[0].role, SourceRustUseRole::Item);
        assert_eq!(uses[1].role, SourceRustUseRole::Item);
        assert_eq!(uses[2].role, SourceRustUseRole::SelfNamespace);
        assert_eq!(
            SourceRustPath::from_ast_path("::external::item"),
            Some(SourceRustPath {
                anchor: SourceRustAnchor::Absolute,
                segments: vec!["external".into(), "item".into()]
            })
        );
        assert!(SourceRustPath::from_ast_path("crate::::item").is_none());
    }

    #[test]
    fn module_calls_and_reexports_retain_actual_source_owners() {
        let source = "pub fn run() {} mod worker; mod inner {pub use crate::run as launch; pub fn caller() {launch(); crate::run();}} fn outer() {worker::run();}";
        let result = fixture_ok!(extract_symbol_graph_with_semantics_controlled(
            "src/lib.rs",
            Some("rust"),
            source,
            &IndexWorkControl::new(IndexCancellation::new(), None),
        ));
        let facts = result.semantics();
        let rust = fixture_ok!(facts.rust().ok_or("Rust ownership was not captured"));
        assert!(rust.complete);
        assert_eq!(rust.modules.len(), 2);
        assert!(rust.modules.iter().any(|module| module.name == "worker"
            && module.body.is_none()
            && module.parent.is_empty()
            && module.supported));
        assert!(rust.modules.iter().any(|module| module.name == "inner"
            && module.body.is_some()
            && module.parent.is_empty()));
        let imported = fixture_ok!(facts.imports()[0].rust.as_ref().ok_or("use path missing"));
        assert_eq!(imported.module.as_ref(), ["inner"]);
        assert!(imported.reexport);
        let exported = fixture_ok!(
            rust.declarations
                .iter()
                .find(|declaration| declaration.name == "launch")
                .ok_or("reexport slot missing")
        );
        assert!(matches!(&exported.binding, SourceRustBinding::Use(_)));
        let SourceRustBinding::Use(exported) = &exported.binding else {
            return;
        };
        assert!(Arc::ptr_eq(imported, exported));
        assert!(facts.calls().iter().any(|call| matches!(&call.binding, SourceCallBinding::RustPath(path) if path.path.anchor == SourceRustAnchor::Crate && path.module.as_ref() == ["inner"])));
        assert!(facts.calls().iter().any(|call| matches!(&call.binding, SourceCallBinding::RustPath(path) if path.path.anchor == SourceRustAnchor::SelfModule && path.module.is_empty() && path.path.segments == ["worker", "run"])));
        assert!(facts.matches_graph(result.graph()));
    }

    #[test]
    fn cargo_target_paths_come_from_the_existing_manifest_parse() {
        let source = "[package]\nname='fixture'\nversion='0.1.0'\nedition='2024'\nautobins=false\n[lib]\npath='src/api.rs'\n[[bin]]\nname='tool'\npath='program.rs'\n";
        let result = fixture_ok!(extract_symbol_graph_with_semantics_controlled(
            "Cargo.toml",
            Some("cargo-manifest"),
            source,
            &IndexWorkControl::new(IndexCancellation::new(), None),
        ));
        let targets = fixture_ok!(
            result
                .semantics()
                .cargo()
                .ok_or("Cargo target settings missing")
        );
        assert!(targets.complete);
        assert!(
            targets
                .targets
                .iter()
                .any(|target| target.kind == SourceCargoTargetKind::Library
                    && target.path == "src/api.rs")
        );
        assert!(
            targets
                .targets
                .iter()
                .any(|target| target.kind == SourceCargoTargetKind::Binary
                    && target.path == "program.rs")
        );
        assert!(!targets.automatic.iter().any(|target| matches!(
            target.kind,
            SourceCargoTargetKind::Library | SourceCargoTargetKind::Binary
        )));
        assert!(
            !targets
                .targets
                .iter()
                .any(|target| target.path == "src/lib.rs")
        );
        assert!(result.semantics().matches_graph(result.graph()));
    }
}
