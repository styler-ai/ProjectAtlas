//! Validate pinned optional-parser evidence and render its accepted manifest and corpus.

use projectatlas_core::language::{
    OPTIONAL_GRAMMAR_CATALOG, OPTIONAL_GRAMMAR_CATALOG_RELEASE_REVISION,
    OPTIONAL_GRAMMAR_CATALOG_VERSION,
};
use projectatlas_core::optional_parser_pack::{
    AcceptedGrammar, GrammarAbiExport, GrammarExportSymbol, GrammarFixture, GrammarFixtureOrigin,
    GrammarFixtures, GrammarLibraryStem, GrammarLicense, GrammarSourceProvenance,
    OPTIONAL_GRAMMAR_CATALOG_CRATE_PATH_IN_VCS, OPTIONAL_GRAMMAR_CATALOG_CRATE_REVISION,
    OPTIONAL_GRAMMAR_CATALOG_CRATE_SHA256, OPTIONAL_GRAMMAR_CATALOG_RELEASE_TAG,
    OPTIONAL_GRAMMAR_CATALOG_SOURCE_BUNDLE_SHA256, OPTIONAL_PARSER_PACK_MAXIMUM_ABI,
    OPTIONAL_PARSER_PACK_MINIMUM_ABI, OPTIONAL_PARSER_PACK_PROJECTATLAS_VERSION,
    OPTIONAL_PARSER_PACK_TREE_SITTER_VERSION, OptionalParserCargoArchive,
    OptionalParserNativeRelease, OptionalParserPackManifest, OptionalParserPackRuntime,
    OptionalParserPackSource, PackPlatform, ParserPackConsumer, Sha256Digest, SourceRevision,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::error::Error;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

/// Accepted source-evidence schema.
const SOURCE_SCHEMA_VERSION: u32 = 2;
/// Generated fixture-corpus schema.
const CORPUS_SCHEMA_VERSION: u32 = 1;
/// Canonical compile-input tree digest algorithm.
const COMPILE_INPUT_DIGEST_ALGORITHM: &str = "sha256-file-tree-v1";
/// Authoritative pinned source-evidence path.
const SOURCE_RELATIVE_PATH: &str =
    "packaging/parser-pack/sources/tree-sitter-language-pack-1.13.2.json";
/// SHA-256 sidecar for the pinned source evidence.
const SOURCE_SIDECAR_RELATIVE_PATH: &str =
    "packaging/parser-pack/sources/tree-sitter-language-pack-1.13.2.json.sha256";
/// Pinned native release-asset source authority.
const PLATFORM_BUNDLES_RELATIVE_PATH: &str =
    "packaging/parser-pack/sources/tree-sitter-language-pack-1.13.2-platform-bundles.json";
/// SHA-256 sidecar for the pinned native release-asset authority.
const PLATFORM_BUNDLES_SIDECAR_RELATIVE_PATH: &str =
    "packaging/parser-pack/sources/tree-sitter-language-pack-1.13.2-platform-bundles.json.sha256";
/// Exact upstream release-asset URL prefix.
const RELEASE_ASSET_URL_PREFIX: &str =
    "https://github.com/xberg-io/tree-sitter-language-pack/releases/download/v1.13.2/";
/// Largest accepted upstream native bundle or metadata asset.
const MAX_RELEASE_ASSET_BYTES: u64 = 64 * 1024 * 1024;
/// Generated logical accepted-capability manifest path.
const MANIFEST_RELATIVE_PATH: &str = "packaging/parser-pack/accepted-capabilities.json";
/// SHA-256 sidecar for the generated logical manifest.
const MANIFEST_SIDECAR_RELATIVE_PATH: &str =
    "packaging/parser-pack/accepted-capabilities.json.sha256";
/// Generated fixture corpus retained for packaged parser verification.
const CORPUS_RELATIVE_PATH: &str = "fixtures/languages/optional-parser-pack-corpus.json";

/// Fallible generator result error.
type DynError = Box<dyn Error>;

/// Pinned source-evidence document.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceSnapshot {
    /// Evidence schema version.
    schema_version: u32,
    /// Exact source crate name.
    source_package: String,
    /// Exact source crate version.
    source_version: String,
    /// Exact published Cargo archive identity.
    cargo_archive: CargoArchiveInput,
    /// Exact native release identity.
    native_release: NativeReleaseInput,
    /// Fully evidenced grammar rows.
    rows: Vec<SourceRow>,
}

/// Exact published Cargo archive input identity.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CargoArchiveInput {
    /// Full embedded VCS revision.
    vcs_revision: String,
    /// Monorepo-relative crate path.
    path_in_vcs: String,
    /// Published archive SHA-256.
    sha256: String,
}

/// Exact upstream release input identity.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeReleaseInput {
    /// Version tag owning the assets.
    tag: String,
    /// Full tag revision.
    revision: String,
    /// Parser-source bundle SHA-256.
    source_bundle_sha256: String,
}

/// Pinned release-asset intake document.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlatformBundleSnapshot {
    /// Evidence schema version.
    schema_version: u32,
    /// Exact source crate name.
    source_package: String,
    /// Exact source crate version.
    source_version: String,
    /// Exact published Cargo archive identity.
    cargo_archive: CargoArchiveInput,
    /// Exact native release identity.
    native_release: NativeReleaseInput,
    /// Exact upstream parser inventory asset.
    upstream_release_manifest: ReleaseAsset,
    /// Complete required native bundle set.
    platforms: Vec<PlatformBundleAsset>,
}

/// One exact upstream release asset.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseAsset {
    /// Immutable version-qualified HTTPS URL.
    url: String,
    /// SHA-256 of the exact downloaded bytes.
    sha256: String,
    /// Exact downloaded byte length.
    byte_length: u64,
}

/// One exact native bundle bound to a required target.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlatformBundleAsset {
    /// Canonical Rust release target.
    platform: PackPlatform,
    /// Upstream platform identity used in its metadata.
    upstream_platform: String,
    /// Immutable release asset.
    #[serde(flatten)]
    asset: ReleaseAsset,
}

/// One grammar evidence row.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRow {
    /// Canonical language-registry identity.
    language_id: String,
    /// Pinned grammar source and compile inputs.
    source: GrammarSourceInput,
    /// Intake license classification retained for validation.
    license_label: String,
    /// Exact applicable source license records.
    licenses: Vec<LicenseInput>,
    /// Exported Tree-sitter language ABI.
    abi: u32,
    /// Exported Tree-sitter language function.
    export_symbol: String,
    /// Platform-neutral dynamic-library stem.
    library_stem: String,
    /// Natural positive and negative evidence.
    fixtures: FixturePairInput,
}

/// Pinned grammar repository subtree and compile-input identity.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrammarSourceInput {
    /// HTTPS source repository.
    repository: String,
    /// Full source revision.
    revision: String,
    /// Optional repository-relative grammar subtree.
    subdirectory: Option<String>,
    /// Compile-input digest algorithm.
    compile_input_digest_algorithm: String,
    /// Compile-input file-tree SHA-256.
    compile_input_digest: String,
    /// Number of files represented by the digest.
    compile_files: usize,
}

/// Exact license text and source evidence.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LicenseInput {
    /// Repository-relative license path.
    source_path: String,
    /// Exact Git blob identity.
    source_blob: String,
    /// Exact license-text byte length.
    byte_length: usize,
    /// Exact license-text SHA-256.
    sha256: String,
    /// Exact applicable license text.
    text: String,
}

/// Positive and negative fixtures for one grammar.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FixturePairInput {
    /// Natural source expected to parse.
    positive: PositiveFixtureInput,
    /// Distinct source expected to produce an error.
    negative: NegativeFixtureInput,
}

/// Natural positive fixture and expected tree when known upstream.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PositiveFixtureInput {
    /// Evidence-origin classification.
    origin: GrammarFixtureOrigin,
    /// Upstream repository-relative source path.
    source_path: String,
    /// Upstream corpus case name.
    case_name: String,
    /// Exact source text.
    source: String,
    /// Exact source-text SHA-256.
    source_sha256: String,
    /// Expected Tree-sitter S-expression when retained upstream.
    expected_tree: Option<String>,
    /// Expected-tree SHA-256 when a tree is present.
    expected_tree_sha256: Option<String>,
}

/// Natural negative fixture expected to produce a parser error.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NegativeFixtureInput {
    /// Evidence-origin classification.
    origin: GrammarFixtureOrigin,
    /// Upstream repository-relative source path.
    source_path: String,
    /// Upstream corpus case name.
    case_name: String,
    /// Exact source text.
    source: String,
    /// Exact source-text SHA-256.
    source_sha256: String,
    /// Required parser-error expectation.
    expected_error: bool,
}

/// Generated corpus used by packaged parser validation.
#[derive(Debug, Serialize)]
struct OptionalParserCorpus {
    /// Corpus schema version.
    schema_version: u32,
    /// SHA-256 of the complete pinned source-evidence document.
    source_manifest_sha256: String,
    /// Sorted grammar fixture rows.
    rows: Vec<CorpusRow>,
}

/// One language's retained fixture evidence.
#[derive(Debug, Serialize)]
struct CorpusRow {
    /// Canonical language-registry identity.
    language_id: String,
    /// Exact fixture metadata, source, and expectations.
    fixtures: FixturePairInput,
}

/// Deterministic in-memory generator output.
struct GeneratedArtifacts {
    /// Pretty JSON logical manifest bytes.
    manifest: Vec<u8>,
    /// SHA-256 of the exact manifest bytes.
    manifest_sha256: String,
    /// Exact generated SHA-256 sidecar bytes for the logical manifest.
    manifest_sidecar: Vec<u8>,
    /// Pretty JSON fixture-corpus bytes.
    corpus: Vec<u8>,
    /// Number of accepted grammar rows.
    grammar_count: usize,
    /// Number of provenance-preserving license records.
    license_count: usize,
}

fn main() -> Result<(), DynError> {
    let root = workspace_root();
    let generated = generate(&root)?;
    fs::write(root.join(MANIFEST_RELATIVE_PATH), &generated.manifest)?;
    fs::write(
        root.join(MANIFEST_SIDECAR_RELATIVE_PATH),
        &generated.manifest_sidecar,
    )?;
    fs::write(root.join(CORPUS_RELATIVE_PATH), &generated.corpus)?;
    writeln!(
        io::stdout().lock(),
        "rendered {} grammars, {} license records; accepted manifest SHA-256 {}",
        generated.grammar_count,
        generated.license_count,
        generated.manifest_sha256
    )?;
    Ok(())
}

/// Resolve the repository root containing the core crate.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Validate the pinned evidence and render both generated artifacts.
fn generate(root: &Path) -> Result<GeneratedArtifacts, DynError> {
    validate_platform_bundle_snapshot(root)?;
    let source_path = root.join(SOURCE_RELATIVE_PATH);
    let source = fs::read(&source_path)?;
    let source_sha256 = sha256_hex(&source);
    verify_source_sidecar(
        &source_sha256,
        &fs::read_to_string(root.join(SOURCE_SIDECAR_RELATIVE_PATH))?,
        source_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid_data("source snapshot has no UTF-8 filename"))?,
    )?;

    let mut snapshot: SourceSnapshot = serde_json::from_slice(&source)?;
    validate_snapshot_header(&snapshot)?;
    snapshot
        .rows
        .sort_unstable_by(|left, right| left.language_id.cmp(&right.language_id));
    if snapshot
        .rows
        .windows(2)
        .any(|rows| rows[0].language_id == rows[1].language_id)
    {
        return Err(invalid_data("source snapshot contains duplicate language IDs").into());
    }

    let grammar_count = snapshot.rows.len();
    let mut licenses = Vec::new();
    let mut grammars = Vec::with_capacity(grammar_count);
    let mut corpus_rows = Vec::with_capacity(grammar_count);

    for row in snapshot.rows {
        let language_id = row.language_id;
        validate_nonempty("language_id", &language_id)?;
        validate_nonempty("license_label", &row.license_label)?;
        validate_fixture_pair(&language_id, &row.fixtures)?;

        if row.source.compile_input_digest_algorithm != COMPILE_INPUT_DIGEST_ALGORITHM {
            return Err(invalid_data(format!(
                "{language_id}: unsupported compile-input digest algorithm {:?}",
                row.source.compile_input_digest_algorithm
            ))
            .into());
        }
        if row.source.compile_files == 0 {
            return Err(
                invalid_data(format!("{language_id}: compile-input inventory is empty")).into(),
            );
        }

        let source_revision = SourceRevision::new(row.source.revision.clone())?;
        let subdirectory = row.source.subdirectory.unwrap_or_else(|| ".".to_string());
        let source_provenance = GrammarSourceProvenance {
            repository_url: row.source.repository.clone(),
            revision: source_revision.clone(),
            subdirectory: subdirectory.clone(),
            compile_input_sha256: Sha256Digest::new(row.source.compile_input_digest)?,
        };

        if row.licenses.is_empty() {
            return Err(invalid_data(format!("{language_id}: no applicable license text")).into());
        }
        let mut license_record_ids = Vec::with_capacity(row.licenses.len());
        for license in row.licenses {
            validate_license_input(&language_id, &subdirectory, &license)?;
            let id = license_record_id(
                &language_id,
                &row.source.repository,
                source_revision.as_str(),
                &license.source_path,
            );
            license_record_ids.push(id.clone());
            licenses.push(GrammarLicense::new(
                id,
                row.source.repository.clone(),
                license.source_path,
                source_revision.clone(),
                license.text,
                None,
            ));
        }
        license_record_ids.sort_unstable();

        let fixtures = GrammarFixtures {
            positive: GrammarFixture::new(
                row.fixtures.positive.origin,
                row.fixtures.positive.source_path.clone(),
                row.fixtures.positive.case_name.clone(),
                row.fixtures.positive.source.clone(),
            ),
            negative: GrammarFixture::new(
                row.fixtures.negative.origin,
                row.fixtures.negative.source_path.clone(),
                row.fixtures.negative.case_name.clone(),
                row.fixtures.negative.source.clone(),
            ),
        };
        grammars.push(AcceptedGrammar::new(
            language_id.clone(),
            source_provenance,
            license_record_ids,
            GrammarAbiExport {
                minimum_abi: OPTIONAL_PARSER_PACK_MINIMUM_ABI,
                maximum_abi: OPTIONAL_PARSER_PACK_MAXIMUM_ABI,
                expected_abi: row.abi,
                export_symbol: GrammarExportSymbol::new(row.export_symbol)?,
                library_stem: GrammarLibraryStem::new(row.library_stem)?,
            },
            fixtures,
        ));
        corpus_rows.push(CorpusRow {
            language_id,
            fixtures: row.fixtures,
        });
    }

    licenses.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    let manifest = OptionalParserPackManifest::new(
        OptionalParserPackSource {
            package: snapshot.source_package,
            version: snapshot.source_version,
            cargo_archive: OptionalParserCargoArchive {
                sha256: Sha256Digest::new(snapshot.cargo_archive.sha256)?,
                vcs_revision: SourceRevision::new(snapshot.cargo_archive.vcs_revision)?,
                path_in_vcs: snapshot.cargo_archive.path_in_vcs,
            },
            native_release: OptionalParserNativeRelease {
                tag: snapshot.native_release.tag,
                revision: SourceRevision::new(snapshot.native_release.revision)?,
                source_bundle_sha256: Sha256Digest::new(
                    snapshot.native_release.source_bundle_sha256,
                )?,
            },
        },
        OptionalParserPackRuntime {
            consumer: ParserPackConsumer::ProjectAtlasParserWorker,
            projectatlas_version: OPTIONAL_PARSER_PACK_PROJECTATLAS_VERSION.to_string(),
            tree_sitter_version: OPTIONAL_PARSER_PACK_TREE_SITTER_VERSION.to_string(),
            minimum_abi: OPTIONAL_PARSER_PACK_MINIMUM_ABI,
            maximum_abi: OPTIONAL_PARSER_PACK_MAXIMUM_ABI,
        },
        licenses,
        grammars,
    )?;
    manifest.validate()?;

    let manifest_bytes = pretty_json(&manifest)?;
    OptionalParserPackManifest::from_json(&manifest_bytes)?;
    let license_count = manifest.licenses().len();
    let manifest_sha256 = sha256_hex(&manifest_bytes);
    let manifest_sidecar = format!("{manifest_sha256}  accepted-capabilities.json\n").into_bytes();
    let corpus = pretty_json(&OptionalParserCorpus {
        schema_version: CORPUS_SCHEMA_VERSION,
        source_manifest_sha256: source_sha256,
        rows: corpus_rows,
    })?;

    Ok(GeneratedArtifacts {
        manifest: manifest_bytes,
        manifest_sha256,
        manifest_sidecar,
        corpus,
        grammar_count,
        license_count,
    })
}

/// Validate the independent native release-asset authority and its sidecar.
fn validate_platform_bundle_snapshot(root: &Path) -> Result<(), DynError> {
    let path = root.join(PLATFORM_BUNDLES_RELATIVE_PATH);
    let bytes = fs::read(&path)?;
    let sha256 = sha256_hex(&bytes);
    verify_source_sidecar(
        &sha256,
        &fs::read_to_string(root.join(PLATFORM_BUNDLES_SIDECAR_RELATIVE_PATH))?,
        path.file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid_data("platform bundle snapshot has no UTF-8 filename"))?,
    )?;
    let snapshot: PlatformBundleSnapshot = serde_json::from_slice(&bytes)?;
    let expected = [
        (
            "source_package",
            snapshot.source_package.as_str(),
            OPTIONAL_GRAMMAR_CATALOG,
        ),
        (
            "source_version",
            snapshot.source_version.as_str(),
            OPTIONAL_GRAMMAR_CATALOG_VERSION,
        ),
        (
            "cargo_archive.vcs_revision",
            snapshot.cargo_archive.vcs_revision.as_str(),
            OPTIONAL_GRAMMAR_CATALOG_CRATE_REVISION,
        ),
        (
            "cargo_archive.path_in_vcs",
            snapshot.cargo_archive.path_in_vcs.as_str(),
            OPTIONAL_GRAMMAR_CATALOG_CRATE_PATH_IN_VCS,
        ),
        (
            "cargo_archive.sha256",
            snapshot.cargo_archive.sha256.as_str(),
            OPTIONAL_GRAMMAR_CATALOG_CRATE_SHA256,
        ),
        (
            "native_release.tag",
            snapshot.native_release.tag.as_str(),
            OPTIONAL_GRAMMAR_CATALOG_RELEASE_TAG,
        ),
        (
            "native_release.revision",
            snapshot.native_release.revision.as_str(),
            OPTIONAL_GRAMMAR_CATALOG_RELEASE_REVISION,
        ),
        (
            "native_release.source_bundle_sha256",
            snapshot.native_release.source_bundle_sha256.as_str(),
            OPTIONAL_GRAMMAR_CATALOG_SOURCE_BUNDLE_SHA256,
        ),
    ];
    if snapshot.schema_version != SOURCE_SCHEMA_VERSION {
        return Err(invalid_data(format!(
            "platform bundle schema {} does not match {SOURCE_SCHEMA_VERSION}",
            snapshot.schema_version
        ))
        .into());
    }
    for (field, actual, required) in expected {
        if actual != required {
            return Err(invalid_data(format!(
                "platform bundle {field} {actual:?} does not match {required:?}"
            ))
            .into());
        }
    }
    validate_release_asset(
        &snapshot.upstream_release_manifest,
        "parsers.json",
        "upstream release manifest",
    )?;
    if snapshot.platforms.len() != PackPlatform::ALL.len() {
        return Err(invalid_data("platform bundle set is incomplete").into());
    }
    for (actual, required) in snapshot.platforms.iter().zip(PackPlatform::ALL) {
        if actual.platform != *required {
            return Err(invalid_data("platform bundles are not complete canonical order").into());
        }
        let (upstream_platform, asset_name) = required_platform_asset(*required);
        if actual.upstream_platform != upstream_platform {
            return Err(invalid_data(format!(
                "{} upstream platform {:?} does not match {:?}",
                required.as_str(),
                actual.upstream_platform,
                upstream_platform
            ))
            .into());
        }
        validate_release_asset(&actual.asset, asset_name, required.as_str())?;
    }
    Ok(())
}

/// Return the exact upstream platform and asset identities for one required target.
fn required_platform_asset(platform: PackPlatform) -> (&'static str, &'static str) {
    match platform {
        PackPlatform::LinuxX86_64 => ("linux-x86_64", "parsers-linux-x86_64.tar.zst"),
        PackPlatform::WindowsX86_64 => ("windows-x86_64", "parsers-windows-x86_64.tar.zst"),
    }
}

/// Validate one exact immutable release asset without fetching it.
fn validate_release_asset(
    asset: &ReleaseAsset,
    expected_name: &str,
    owner: &str,
) -> Result<(), DynError> {
    let expected_url = format!("{RELEASE_ASSET_URL_PREFIX}{expected_name}");
    if asset.url != expected_url {
        return Err(invalid_data(format!("{owner} release URL changed")).into());
    }
    Sha256Digest::new(asset.sha256.clone())?;
    if asset.byte_length == 0 || asset.byte_length > MAX_RELEASE_ASSET_BYTES {
        return Err(
            invalid_data(format!("{owner} byte length is outside the intake bound")).into(),
        );
    }
    Ok(())
}

/// Bind the evidence header to current selected source authority.
fn validate_snapshot_header(snapshot: &SourceSnapshot) -> Result<(), DynError> {
    let expected = [
        (
            "source_package",
            snapshot.source_package.as_str(),
            OPTIONAL_GRAMMAR_CATALOG,
        ),
        (
            "source_version",
            snapshot.source_version.as_str(),
            OPTIONAL_GRAMMAR_CATALOG_VERSION,
        ),
        (
            "cargo_archive.vcs_revision",
            snapshot.cargo_archive.vcs_revision.as_str(),
            OPTIONAL_GRAMMAR_CATALOG_CRATE_REVISION,
        ),
        (
            "cargo_archive.path_in_vcs",
            snapshot.cargo_archive.path_in_vcs.as_str(),
            OPTIONAL_GRAMMAR_CATALOG_CRATE_PATH_IN_VCS,
        ),
        (
            "cargo_archive.sha256",
            snapshot.cargo_archive.sha256.as_str(),
            OPTIONAL_GRAMMAR_CATALOG_CRATE_SHA256,
        ),
        (
            "native_release.tag",
            snapshot.native_release.tag.as_str(),
            OPTIONAL_GRAMMAR_CATALOG_RELEASE_TAG,
        ),
        (
            "native_release.revision",
            snapshot.native_release.revision.as_str(),
            OPTIONAL_GRAMMAR_CATALOG_RELEASE_REVISION,
        ),
        (
            "native_release.source_bundle_sha256",
            snapshot.native_release.source_bundle_sha256.as_str(),
            OPTIONAL_GRAMMAR_CATALOG_SOURCE_BUNDLE_SHA256,
        ),
    ];
    if snapshot.schema_version != SOURCE_SCHEMA_VERSION {
        return Err(invalid_data(format!(
            "source schema {} does not match {SOURCE_SCHEMA_VERSION}",
            snapshot.schema_version
        ))
        .into());
    }
    for (field, actual, required) in expected {
        if actual != required {
            return Err(invalid_data(format!(
                "source {field} {actual:?} does not match {required:?}"
            ))
            .into());
        }
    }
    Ok(())
}

/// Validate one exact license record and its subtree applicability.
fn validate_license_input(
    language_id: &str,
    subdirectory: &str,
    license: &LicenseInput,
) -> Result<(), DynError> {
    validate_nonempty("license.source_path", &license.source_path)?;
    validate_lower_hex("license.source_blob", &license.source_blob, 40)?;
    if license.byte_length != license.text.len() {
        return Err(invalid_data(format!(
            "{language_id}: license {} byte length changed",
            license.source_path
        ))
        .into());
    }
    verify_sha256(
        &format!("{language_id}: license {}", license.source_path),
        license.text.as_bytes(),
        &license.sha256,
    )?;
    if !license_path_applies(subdirectory, &license.source_path) {
        return Err(invalid_data(format!(
            "{language_id}: license {} is outside grammar subtree {subdirectory}",
            license.source_path
        ))
        .into());
    }
    Ok(())
}

/// Return whether a license is at repository root, the grammar root, or its ancestor.
fn license_path_applies(subdirectory: &str, source_path: &str) -> bool {
    if subdirectory == "." {
        return true;
    }
    let Some((parent, _)) = source_path.rsplit_once('/') else {
        return true;
    };
    parent == subdirectory
        || subdirectory
            .strip_prefix(parent)
            .is_some_and(|remaining| remaining.starts_with('/'))
}

/// Validate one fixture pair and every retained source or tree digest.
fn validate_fixture_pair(language_id: &str, fixtures: &FixturePairInput) -> Result<(), DynError> {
    validate_fixture_metadata(
        language_id,
        "positive",
        &fixtures.positive.source_path,
        &fixtures.positive.case_name,
    )?;
    verify_sha256(
        &format!("{language_id}: positive source"),
        fixtures.positive.source.as_bytes(),
        &fixtures.positive.source_sha256,
    )?;
    match (
        fixtures.positive.expected_tree.as_deref(),
        fixtures.positive.expected_tree_sha256.as_deref(),
    ) {
        (Some(tree), Some(digest)) => verify_sha256(
            &format!("{language_id}: positive expected tree"),
            tree.as_bytes(),
            digest,
        )?,
        (None, None) => {}
        _ => {
            return Err(invalid_data(format!(
                "{language_id}: positive expected tree and digest must appear together"
            ))
            .into());
        }
    }

    validate_fixture_metadata(
        language_id,
        "negative",
        &fixtures.negative.source_path,
        &fixtures.negative.case_name,
    )?;
    verify_sha256(
        &format!("{language_id}: negative source"),
        fixtures.negative.source.as_bytes(),
        &fixtures.negative.source_sha256,
    )?;
    if !fixtures.negative.expected_error {
        return Err(invalid_data(format!(
            "{language_id}: negative fixture does not require a parse error"
        ))
        .into());
    }
    Ok(())
}

/// Validate retained fixture-origin fields.
fn validate_fixture_metadata(
    language_id: &str,
    role: &str,
    source_path: &str,
    case_name: &str,
) -> Result<(), DynError> {
    for (field, value) in [("source_path", source_path), ("case_name", case_name)] {
        validate_nonempty(&format!("{language_id}.{role}.{field}"), value)?;
    }
    Ok(())
}

/// Require non-empty, unpadded, control-free source metadata.
fn validate_nonempty(field: &str, value: &str) -> Result<(), DynError> {
    if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
        return Err(invalid_data(format!("{field} must be non-empty, unpadded text")).into());
    }
    Ok(())
}

/// Require canonical lowercase hexadecimal text of one exact length.
fn validate_lower_hex(field: &str, value: &str, length: usize) -> Result<(), DynError> {
    if value.len() != length
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid_data(format!(
            "{field} must be {length} lowercase hexadecimal characters"
        ))
        .into());
    }
    Ok(())
}

/// Verify the pinned source snapshot against its exact SHA-256 sidecar.
fn verify_source_sidecar(actual: &str, sidecar: &str, expected_name: &str) -> Result<(), DynError> {
    let mut fields = sidecar.split_whitespace();
    let expected_digest = fields
        .next()
        .ok_or_else(|| invalid_data("source SHA-256 sidecar is empty"))?;
    let name = fields
        .next()
        .ok_or_else(|| invalid_data("source SHA-256 sidecar has no filename"))?;
    if fields.next().is_some() || name != expected_name || expected_digest != actual {
        return Err(
            invalid_data("source SHA-256 sidecar does not match the exact snapshot").into(),
        );
    }
    Ok(())
}

/// Verify exact bytes against a canonical SHA-256 value.
fn verify_sha256(owner: &str, bytes: &[u8], expected: &str) -> Result<(), DynError> {
    validate_lower_hex("sha256", expected, 64)?;
    let actual = sha256_hex(bytes);
    if actual != expected {
        return Err(invalid_data(format!("{owner} SHA-256 changed")).into());
    }
    Ok(())
}

/// Derive a language-specific license identity without losing source-path provenance.
fn license_record_id(language_id: &str, repository: &str, revision: &str, path: &str) -> String {
    let identity = format!("{language_id}\0{repository}\0{revision}\0{path}");
    format!("{language_id}.license.{}", sha256_hex(identity.as_bytes()))
}

/// Render the lowercase SHA-256 of exact bytes.
fn sha256_hex(bytes: &[u8]) -> String {
    const LOWER_HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut rendered = String::with_capacity(digest.len() * 2);
    for byte in digest {
        rendered.push(char::from(LOWER_HEX[usize::from(byte >> 4)]));
        rendered.push(char::from(LOWER_HEX[usize::from(byte & 0x0f)]));
    }
    rendered
}

/// Serialize deterministic pretty JSON with a final newline.
fn pretty_json(value: &impl Serialize) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Construct one invalid-evidence error.
fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authoritative_snapshot_generates_deterministically() -> Result<(), DynError> {
        let root = workspace_root();
        let first = generate(&root)?;
        let second = generate(&root)?;
        if first.manifest != second.manifest
            || first.corpus != second.corpus
            || first.manifest_sha256 != second.manifest_sha256
            || first.manifest_sidecar != second.manifest_sidecar
        {
            return Err(invalid_data("successive generations produced different bytes").into());
        }
        for (relative_path, generated) in [
            (MANIFEST_RELATIVE_PATH, first.manifest.as_slice()),
            (
                MANIFEST_SIDECAR_RELATIVE_PATH,
                first.manifest_sidecar.as_slice(),
            ),
            (CORPUS_RELATIVE_PATH, first.corpus.as_slice()),
        ] {
            let committed = fs::read(root.join(relative_path))?;
            if committed != generated {
                return Err(invalid_data(format!(
                    "generated artifact {relative_path} differs from the committed bytes"
                ))
                .into());
            }
        }
        let parsed = OptionalParserPackManifest::from_json(&first.manifest)?;
        if parsed.grammars().len() != first.grammar_count {
            return Err(invalid_data("rendered manifest grammar count changed").into());
        }
        Ok(())
    }

    #[test]
    fn sibling_license_path_is_rejected() -> Result<(), DynError> {
        let accepted = [
            license_path_applies("grammars/rust", "LICENSE"),
            license_path_applies("grammars/rust", "grammars/LICENSE"),
            license_path_applies("grammars/rust", "grammars/rust/LICENSE"),
        ];
        if accepted.contains(&false)
            || license_path_applies("grammars/rust", "grammars/python/LICENSE")
        {
            return Err(invalid_data("license subtree applicability changed").into());
        }
        Ok(())
    }
}
