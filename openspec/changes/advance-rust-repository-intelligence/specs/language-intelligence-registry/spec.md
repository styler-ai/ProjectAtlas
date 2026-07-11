## ADDED Requirements

### Requirement: Generated Language Registry
ProjectAtlas SHALL generate language detection, aliases, extensions, parser selection, query-pack selection, capability tiers, feature-pack ownership, accepted capability-set membership, and provenance from one version-controlled registry. The generated registry SHALL replace hand-maintained parallel language lists and SHALL reject duplicate identifiers, ambiguous extension precedence without an explicit rule, missing parser assets, incompatible tree-sitter ABI versions, capability-set entries without a unique canonical identity, and capability claims without required fixtures.

#### Scenario: Registry generation is deterministic
- **WHEN** the registry generator runs twice from the same lock manifest
- **THEN** it produces byte-identical Rust tables, capability metadata, and test inventory

#### Scenario: Conflicting language metadata is rejected
- **WHEN** two registry entries claim the same extension at the same precedence without an explicit disambiguation rule
- **THEN** generation fails with both language identifiers and the conflicting field

### Requirement: Honest Tiered Language Support
ProjectAtlas SHALL report language support separately as `detected`, `parsed`, `symbols`, `semantic`, and `benchmarked`. A language SHALL NOT be advertised at a higher tier until its required fixture, accuracy, error, and platform gates pass. CLI, MCP capability/settings output, documentation, and generated support matrices SHALL derive from the same registry state.

#### Scenario: Syntax support is not presented as semantic support
- **WHEN** a language has a working grammar but no project-wide resolver
- **THEN** capability output reports `parsed` or `symbols` and does not report `semantic`

#### Scenario: Unknown or unsupported syntax is explicit
- **WHEN** a detected file family has no grammar pack enabled
- **THEN** ProjectAtlas reports its fallback parser and coverage state instead of silently presenting fallback matches as grammar-backed symbols

### Requirement: Accepted Capability-Set Parity
The parity target SHALL be a reviewed, version-controlled accepted capability-set manifest with an immutable identity and digest. The initial accepted manifest SHALL contain at least 159 runnable language/file modes mapped to at least 157 verified parser capabilities after aliases and parser reuse are normalized. Each required entry SHALL identify its runnable mode, canonical parser capability, aliases or reuse, required support tier, parser/pack provenance, fixture inventory, and required release platforms. Generated counts MAY summarize this set, but the numeric floor, aliases, extra capabilities, or higher aggregate scores SHALL NOT compensate for one missing or failing required entry.

Parity SHALL be complete only when the achieved generated registry is a capability-preserving superset of the accepted set and every required entry passes its declared tier, fixture, accuracy, error-budget, provenance, and platform gates. Changing the accepted set SHALL require a reviewed manifest change with rationale and a new digest; a release process SHALL NOT silently shrink the set to make parity pass. Every runnable mode SHALL have at least one valid public fixture and explicit parse-error-byte accounting. Valid fixtures SHALL parse without a parser crash and with no unexplained error span; approved dialect recovery SHALL be recorded in the fixture expectation.

#### Scenario: Broad language gate passes
- **WHEN** the broad-pack conformance suite completes
- **THEN** every entry in the accepted capability set has loaded its pinned asset and passed its declared tier and fixture expectations on every required release platform, and the achieved manifest records the accepted-set identity/digest

#### Scenario: One grammar regresses
- **WHEN** an updated grammar crashes, violates its ABI lock, or exceeds its fixture error budget
- **THEN** the affected capability remains below its declared tier, accepted-set parity fails when the entry is required, and published counts/capabilities update without hiding the deficit

#### Scenario: Extra modes mask a required gap
- **WHEN** the achieved registry contains more total modes than the accepted set but omits or downgrades one required capability
- **THEN** parity fails and the extra modes do not offset the missing accepted entry

### Requirement: Normalized Symbol And Scope Extraction
Grammar-backed languages at the `symbols` tier SHALL emit normalized file, package/module, scope, declaration, import/export, reference, and source-span facts through typed Rust adapters. Language-specific adapters MAY add framework or DSL facts, but SHALL NOT invent resolved cross-file edges during syntax extraction or emit unbounded symbols from malformed input.

#### Scenario: A valid source file is extracted
- **WHEN** a symbols-tier file contains nested declarations, imports, and exports
- **THEN** its graph contains stable normalized facts with correct containment and source spans independent of language-specific tree node names

#### Scenario: Malformed input is bounded
- **WHEN** a file contains pathological or incomplete syntax
- **THEN** extraction respects configured byte, depth, symbol, error-span, time, and cancellation budgets and records partial coverage without crashing the ProjectAtlas process

### Requirement: Project-Wide Semantic Resolution
ProjectAtlas SHALL implement independently structured Rust semantic providers for every semantic family required by the accepted capability set before claiming parity complete. The initial accepted families SHALL include Go, C, C++, CUDA, PHP, Perl, Python, JavaScript, JSX, TypeScript, TSX, C#, Java, Kotlin, and Rust. Existing Objective-C, Zig, Vue, and PowerShell extraction SHALL not regress and MAY advance independently through the same tiers. Providers SHALL prefer canonical Rust parser/compiler/package metadata crates where available, keep ProjectAtlas policy narrow, use project-wide symbol/module/package registries and scoped candidates, and emit explicit resolved, ambiguous, unresolved, and external outcomes rather than first-name-match heuristics or a monolithic handwritten pseudo-language-server.

#### Scenario: Imported call resolves uniquely
- **WHEN** a call target has one valid visible definition under the language's module, scope, and type rules
- **THEN** ProjectAtlas creates a resolved evidence-bearing edge to that stable target

#### Scenario: Multiple candidates remain valid
- **WHEN** static evidence cannot distinguish two or more visible targets
- **THEN** ProjectAtlas records an ambiguous reference and candidates without fabricating a single resolved edge

#### Scenario: Dependency is outside the index
- **WHEN** a reference targets an external package or runtime symbol that is not indexed
- **THEN** ProjectAtlas records a typed external or unresolved target and does not count it as an internal-resolution false success

### Requirement: Compiler Metadata Inputs
Semantic providers for C, C++, CUDA, and other accepted compilation-unit languages SHALL accept bounded typed compiler metadata when present. Per-translation-unit inputs SHALL include the source identity, compilation-database entry identity, working directory, ordered include roots, defines and undefines, language standard/dialect, target mode, forced includes, and module settings needed by the accepted capability. ProjectAtlas SHALL parse compiler metadata and argument arrays as data without executing a compiler, shell, build system, response-file command, repository code, or package-manager hook. Paths SHALL pass repository-containment and explicit external-root policy; accepted external include roots SHALL remain typed external identities. Malformed, conflicting, unsupported, response-file-indirected, or out-of-policy metadata SHALL produce partial, ambiguous, or unavailable capability evidence rather than guessed resolution.

Raw define values and any argument classified as secret-bearing SHALL remain transient, SHALL NOT be persisted, logged, serialized to TOON/JSON, copied into snapshots/benchmarks/test artifacts, or included verbatim in diagnostics, and SHALL not be used to form a reversible public identity. Secret-bearing inputs SHALL use a fixed redacted marker and make any value-dependent semantic capability partial; normalized non-secret metadata MAY contribute only through the versioned opaque compiler-metadata digest required for invalidation. Canary validation SHALL scan the database, WAL/sidecars, logs, task output, snapshots, and retained test artifacts for forbidden raw values.

#### Scenario: Compilation database entry is valid
- **WHEN** a translation unit has bounded valid compiler metadata and contained or explicitly accepted external include roots
- **THEN** the provider receives normalized typed inputs whose identity participates in resolution provenance and incremental invalidation

#### Scenario: Compiler arguments require execution or unsafe indirection
- **WHEN** metadata requests shell expansion, response-file expansion, a build command, or repository code execution
- **THEN** ProjectAtlas does not execute it and records the affected provider capability as partial or unavailable with an exact reason

#### Scenario: Compiler define contains a secret canary
- **WHEN** compiler metadata includes a secret-bearing define or argument value
- **THEN** ProjectAtlas records only redacted partial-capability evidence and the canary is absent from every persisted, logged, serialized, snapshot, benchmark, and retained-test output

### Requirement: Per-Language Quality Gates
Each language advertised at `benchmarked` SHALL have public ground-truth fixtures covering definitions, scopes, imports/exports, calls/references, inheritance or equivalent type relationships, ambiguous names, unresolved externals, generated files where applicable, and malformed syntax. Every language advertised at `symbols` SHALL meet the accepted per-language core extraction gate, and every language/relation family advertised at `semantic` SHALL independently meet its accepted semantic accuracy gate on positive, ambiguous, unresolved/external, and adversarial negative fixtures. Core declaration and import/export extraction SHALL achieve at least 95% precision and 90% recall; project-wide semantic edges SHALL achieve at least 90% precision and 80% recall. The benchmark decision SHALL use the specified per-language/per-family confidence rule; no aggregate, micro-average, extra language, or stronger relation family SHALL mask a threshold failure.

#### Scenario: A language misses its recall threshold
- **WHEN** any language or relation family scores below a required per-language/per-family threshold or confidence rule
- **THEN** the release gate fails or that exact capability remains at a lower advertised tier with the deficit published

#### Scenario: Abstention prevents a false edge
- **WHEN** the resolver correctly records an ambiguous or unresolved outcome instead of choosing an unsupported target
- **THEN** the benchmark counts the abstention according to the declared ground truth rather than treating every missing edge as equal

### Requirement: Grammar Supply-Chain And Runtime Safety
Grammar packs SHALL be pinned by source/version and content digest, covered by dependency and license inventory, built reproducibly, and installed only through explicit ProjectAtlas commands or release artifacts. Normal indexing SHALL NOT fetch, compile, or execute repository-provided grammars, language servers, build scripts, package-manager hooks, or project code. ProjectAtlas-owned extraction and resolution logic SHALL remain Rust; generated parser artifacts SHALL stay isolated behind the grammar adapter boundary.

#### Scenario: An offline scan uses installed packs
- **WHEN** all configured grammar packs are installed and network access is unavailable
- **THEN** scan and query behavior remains fully functional without network attempts

#### Scenario: A grammar digest does not match
- **WHEN** an installed or downloaded grammar asset fails its pinned digest or ABI verification
- **THEN** ProjectAtlas refuses to load it, preserves the last valid active index, and reports a corrective action

### Requirement: Default-Core Resource And Link Containment
The release benchmark manifest SHALL define a `default-core` profile with every optional broad grammar, WASM host, model, ANN/vector, GPU, and download feature disabled and no optional pack installed. Against the pinned pre-change default-core release on the same release target, the profile's packaged artifact and installed bytes SHALL each be no more than 10% larger; cold process-start-to-ready p95 SHALL be no more than 10% slower and no more than 25 ms higher; and warm idle MCP peak RSS SHALL be no more than 10% higher and no more than 16 MiB larger. A limit miss SHALL block default inclusion rather than being averaged with pack-enabled results.

The default-core dependency and binary-link audit SHALL contain no optional WASM engine, model inference runtime, ANN/vector backend, GPU runtime, pack downloader/client, or optional grammar-pack native library. Optional pack executables and their transitive runtime libraries SHALL be separate artifacts loaded or invoked only after explicit pack enablement. Core scan, current built-in language support, deterministic lexical search, graph queries, and all existing administrative/navigation surfaces SHALL remain functional when every optional pack artifact is absent.

#### Scenario: Default core is packaged without packs
- **WHEN** the release artifact is built and tested under the `default-core` profile
- **THEN** size/startup/RSS budgets pass, heavy optional runtime symbols and dependencies are absent, and a no-network no-pack smoke performs normal scan and lexical/graph queries

#### Scenario: Optional runtime exceeds a core budget
- **WHEN** linking a broad parser, WASM, model, ANN, GPU, or download runtime would exceed a default-core budget or appear in its dependency/link manifest
- **THEN** that runtime remains in a separately installed pack and the default-core release is not linked to it

### Requirement: WASM And Native Pack Containment
A WASM grammar pack SHALL run only inside the supervised ProjectAtlas worker with a versioned minimal parser ABI. Pack modules SHALL receive bounded input bytes and explicit host callbacks only; they SHALL have no WASI filesystem, socket/network, process, environment, dynamic-library, wall-clock, or random capability. Each invocation SHALL enforce fuel or epoch interruption, wall deadline, linear-memory/table/stack/output limits, cancellation, and deterministic host inputs. A trap, malformed output, ABI mismatch, or budget breach SHALL fail only the affected task/capability and SHALL not terminate or mutate the long-lived MCP process or active index.

If a benchmark selects generated native parser artifacts for a capability, they SHALL execute in the same supervised child-process boundary and SHALL NOT be loaded as unverified in-process dynamic libraries. Each grammar-pack/host combination SHALL advertise only platforms on which its packaged real-process digest, ABI, containment, malformed-input, timeout, cancellation, and resource-limit smokes pass.

#### Scenario: WASM grammar attempts an undeclared capability
- **WHEN** a module imports filesystem, network, process, environment, clock, random, or another host function outside the parser ABI
- **THEN** pack verification or instantiation fails before source parsing and the active index remains unchanged

#### Scenario: Pack traps on one platform
- **WHEN** a packaged WASM or native parser traps, hangs, exceeds memory, or emits an oversized/malformed result
- **THEN** the supervisor terminates that work, reports partial/unavailable capability for the affected platform, and keeps core scan/query service responsive
