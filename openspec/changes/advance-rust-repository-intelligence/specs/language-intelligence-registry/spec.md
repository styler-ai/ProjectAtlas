## ADDED Requirements

### Requirement: Generated Honest Capability Registry

One versioned Rust registry SHALL generate language identifiers, aliases, detection precedence, parser ownership, optional pack ownership, fixtures, support tiers, settings, and documentation inputs. The full 0.3.26 accepted language surface and behavior SHALL be its compatibility floor. Support SHALL be reported independently as detected, parsed, symbols, semantic, and benchmarked. Counts, aliases, reused grammars, or extensions SHALL NOT hide a missing required capability, inflate a stronger tier, or retain a ghost row with no live detector or provider. The default agent-facing settings projection SHALL remain content-free and bounded: it SHALL expose registry and accepted-set versions and digests, derived per-axis counts, and pinned optional-catalog identity without embedding the complete per-language matrix. The generated support document SHALL expose that complete matrix from the same authority.

#### Scenario: Parser exists without semantic resolution
- **WHEN** a language parses but lacks validated project-wide resolution
- **THEN** settings and documentation report only its achieved tier

#### Scenario: Registry entries conflict
- **WHEN** two entries claim the same detection precedence without an explicit rule
- **THEN** generation fails with both owning entries

#### Scenario: Agent inspects ordinary settings
- **WHEN** the accepted language catalog contains hundreds of rows
- **THEN** settings return the compact bounded registry identity and derived counts while the generated support document remains the complete capability inventory

### Requirement: Complete Support Is A Reviewed Capability Profile

One versioned complete-support profile schema SHALL define fixed profile-kind-specific ProjectAtlas navigation contracts. `Complete` SHALL mean conformance to that fixed ProjectAtlas navigation contract; it SHALL NOT mean compiler, build-system, runtime, or whole-language completeness. Every row SHALL declare exactly one closed semantic kind—`language`, `dialect`, `domain_format`, or `framework_projection`—independently from its presentation categories and tags. A framework projection SHALL bind its exact host language or dialect and SHALL NOT increase language or parser totals. A dialect that shares an extension with another dialect SHALL require explicit project, path, configuration, or bounded content evidence; an extension alone SHALL NOT establish that dialect.

For each semantic kind the schema SHALL define mandatory evidence slots for deterministic detection and dialect evidence; grammar parsing plus malformed/partial behavior; symbol or domain-fact extraction; at least one non-empty accepted relation family when relations apply; exact source occurrences; resolved, ambiguous, unresolved, and external outcomes when resolution applies; real SQLite publication, reopen, and incremental convergence; owning unit and integration fixtures; representative-repository measurements; and bounded agent-navigation evaluation. A mandatory slot MAY be `not_applicable` only with a typed reason admitted by the profile-kind schema and independently reviewed; an individual row SHALL NOT omit or weaken a mandatory slot. Evidence references SHALL be machine-checkable and bound to the applicable registry, parser, provider, relation, publication, and navigation contract identities. A row SHALL enter the accepted complete-support inventory only after every mandatory slot passes and an independent reviewer confirms the registry, implementation, generated documentation, and evidence agree.

Parsed, structural, fallback, or candidate rows SHALL remain at their achieved runtime tiers and SHALL NOT contribute to complete-support totals. Planned and unavailable ecosystem assessments SHALL live only in a documentation catalog projection derived from the same declarative authority; they SHALL NOT create runtime registry or accepted-capability ghost rows and SHALL contribute to no detected, parsed, symbol, semantic, benchmarked, or complete-support total.

The public language-and-ecosystem document SHALL extend the existing generated `docs/language-support.md` authority and group rows by stable user-facing categories such as backend, frontend/web, systems, mobile, data/scientific, enterprise/legacy modernization, database/query, infrastructure/cloud, build/config/template, and testing frameworks. Language, dialect, domain-format, and framework-aware counts SHALL remain separate. A modernization tag MAY identify source families where trustworthy dependency and exact-evidence navigation is especially valuable, but SHALL NOT claim automatic translation or a target-language conversion capability. The document SHALL explain the detection-to-navigation pipeline and the benefits of honest tiers, contained optional parsing, provenance/license binding, exact source evidence, typed resolution, incremental freshness, no repository-code execution, and absent-pack core independence.

GitHub Pages SHALL publish an HTML projection derived from the same catalog identity and SHALL test its landing-page link and agreement with the checked-in Markdown authority. The canonical Mermaid source SHALL remain in GitHub-rendered Markdown; the Pages projection SHALL embed a reviewed rendered SVG or link directly to that canonical view. The public projection SHALL link to the owning system/component, crate ownership, database authority, graph physical model, bounded graph read, MCP read, and transactional publication views. Every changed Mermaid view SHALL be rendered and visually plus semantically reviewed. Repository and GitHub Pages navigation SHALL link to these generated projections rather than maintain another language list.

Maintained license-compatible Tree-sitter grammars and their generated parser, node-type metadata, and trustworthy standard query assets SHALL be reused before ProjectAtlas creates grammar or extraction machinery. ProjectAtlas MAY add bounded query definitions for missing facts and Rust provider logic for language-specific scope, dialect, module, package, and cross-file resolution. A new or forked grammar SHALL be a documented last resort with its own provenance, maintenance, compatibility, and fixture decision. Grammar availability alone SHALL NOT promote symbols, relations, semantics, benchmarks, or complete support.

#### Scenario: Public page groups support claims
- **WHEN** the generated support page is rendered
- **THEN** its Markdown and Pages projections share one catalog identity, group rows by stable category, keep language/dialect/domain/framework counts distinct, link every complete claim to its achieved fixed profile, explain weaker tiers without presenting them as complete, and expose the reviewed Mermaid architecture path plus its durable architecture links

#### Scenario: Grammar parses but semantic evidence is incomplete
- **WHEN** an admitted Tree-sitter grammar passes positive and malformed parsing but lacks one declared fact, relation, persistence, incremental, representative, or agent-navigation proof
- **THEN** the row remains at its achieved lower tiers and is excluded from the complete-support inventory

#### Scenario: Mandatory evidence does not apply
- **WHEN** one profile kind cannot meaningfully produce a mandatory evidence family
- **THEN** its row uses only a schema-admitted typed `not_applicable` reason that survives independent review instead of omitting or weakening the evidence slot

#### Scenario: Planned family has no runtime owner
- **WHEN** the ecosystem catalog assesses a planned or unavailable family without a live detector, parser, fact provider, or semantic provider
- **THEN** it appears only in the documentation projection, creates no runtime capability row, and contributes to no capability or complete-support total

#### Scenario: Framework or dialect shares syntax
- **WHEN** a framework projection uses a supported host language or a dialect shares a common extension
- **THEN** the framework binds the exact host profile and the dialect supplies independent bounded evidence without increasing the host language or parser count

#### Scenario: Existing grammar assets cover the requirement
- **WHEN** a maintained compatible grammar and trustworthy query assets provide the required syntax facts
- **THEN** ProjectAtlas pins and validates those assets instead of reimplementing the grammar, while retaining Rust ownership of ProjectAtlas-specific semantic resolution

### Requirement: Existing Parser Behavior Remains Compatible

Generated selection SHALL preserve current exact-filename spelling and case, compound-extension behavior, case-insensitive extension behavior, parser selection, parse metadata, structural summary, fallback visibility, and empty-native-parse behavior before new modes are enabled. The typed precedence SHALL be explicit override, exact filename, compound extension, extension, then bounded content/dialect classification, with a testable reason for the selected rule. Built-in parsers remain closed compile-time choices. Fallback parsing SHALL be identified honestly and SHALL NOT be presented as grammar-backed symbol support.

For every accepted embedded-language capability, extraction SHALL be bounded and SHALL reconcile embedded byte, line, and source spans back to the host file. The registry SHALL identify the host/embedded pairing and its natural fixtures; malformed or truncated embedded content SHALL return honest partial coverage rather than host-relative fabricated spans.

#### Scenario: Existing fixture is rescanned
- **WHEN** registry-driven selection replaces hand-maintained selection
- **THEN** every current fixture selects the same effective built-in parser and compatible output

#### Scenario: Component host contains an embedded language
- **WHEN** a supported template or component file contains an accepted embedded-language region
- **THEN** definitions and relationships use exact reusable host-file spans and bounded partial coverage is reported when reconciliation is incomplete

### Requirement: Accepted Language Capability Cannot Shrink Silently

The versioned registry SHALL contain an explicit accepted capability-set manifest. Each accepted row SHALL declare required membership and tier, a natural positive fixture, a non-vacuous negative fixture, provenance and license inputs, and accepted optional-pack target applicability. A negative fixture MAY be an exact upstream error case or a deterministic incomplete editor-state form derived from exact natural upstream source; the manifest SHALL distinguish those origins and retain the source path, case, content digest, and transformation classification. Every accepted negative SHALL produce its declared error or non-match through the selected grammar on every accepted optional-pack target before the row is advertised as grammar-backed support. Generated runtime tables, settings, validation, and documentation SHALL derive from those rows, including their counts. Removing or weakening an accepted row SHALL require an explicit compatibility decision and capability-set version change; validation SHALL fail when generated output omits or understates a still-accepted row. Product Rust and tests SHALL NOT duplicate mutable accepted membership or totals as literals.

#### Scenario: A generator drops an accepted language row
- **WHEN** generated parser tables, settings, fixtures, or documentation omit a still-accepted capability or advertise a lower tier
- **THEN** validation fails with the owning capability row instead of accepting a smaller advertised set

#### Scenario: Incomplete editor state is used as a negative fixture
- **WHEN** a negative case is deterministically derived by truncating exact natural upstream source
- **THEN** its upstream source identity and transformation remain visible, and the row is not advertised until the selected grammar proves the declared negative outcome on every accepted optional-pack target

### Requirement: Advertised Semantic Families Are Independently Validated

Project-wide resolution SHALL use small language-owned providers over normalized registries and candidates. Resolved, ambiguous, unresolved, and external outcomes SHALL remain distinct. Every advertised language/relation family SHALL have non-vacuous positive, negative, malformed, duplicate-name, and ambiguity fixtures; aggregate success SHALL NOT mask one failing family.

#### Scenario: Static source cannot choose one target
- **WHEN** several scoped candidates remain valid
- **THEN** the provider abstains with bounded candidates rather than choosing the first name match

### Requirement: Compiler Metadata Is Bounded Non-Executable Input

Translation-unit providers MAY consume typed working-directory, include-root, dialect/target, forced-include, and opaque define identity data when required for correct resolution. ProjectAtlas SHALL parse that metadata as data, SHALL NOT execute compilers, shells, response-file commands, builds, or repository code, and SHALL NOT persist, log, serialize, snapshot, benchmark, or return secret-bearing raw define values.

#### Scenario: Metadata requires execution or contains a secret
- **WHEN** a compiler entry requires command execution, unsafe indirection, an out-of-scope path, or a secret-bearing raw value
- **THEN** ProjectAtlas returns a typed partial/unsupported outcome, uses only safe opaque invalidation identity, and emits no secret value

### Requirement: Optional Parser Breadth Does Not Burden Core

Broad parser capability SHALL be installed, verified, enabled, updated, rolled back, disabled, and removed explicitly through one optional-pack lifecycle. The implementation SHALL NOT add a generic multi-pack framework; it MAY split the pack into multiple artifacts only after measured package, installation, or platform limits require that split, and every resulting artifact SHALL bind its exact capability membership independently. The accepted v0.4.0 optional-pack artifact targets SHALL be Linux x86-64 and Windows x86-64. ProjectAtlas and the complete 0.3.26 built-in parser surface SHALL remain supported on macOS, but optional-pack activation there SHALL return typed `unsupported_containment` before worker launch or source transfer and SHALL NOT advertise optional grammar-backed breadth. Normal core scan/query SHALL not download, compile, link, initialize, or execute an absent or unsupported optional pack, and pack absence or unsupported containment SHALL not change the accepted 0.3.26 built-in surface or impose optional-grammar binary, startup, or resident-memory cost. Pack work SHALL be bounded and isolated from the long-lived service.

The v0.4 pack SHALL preserve the published Cargo archive identity separately from the release/tag and native-asset identity. Each accepted optional-pack target artifact SHALL carry the same accepted logical manifest and fixture corpus, exactly the accepted grammar libraries, individual payload digests, bounded size/path/file measurements, and a closed native dependency/export audit. Immutable artifact construction evidence and fresh-runner loading/fixture evidence SHALL remain separate and SHALL bind the exact archive they prove. The support claim SHALL require an aggregate proof containing exactly the accepted Linux and Windows target set and the same successful accepted grammar identities. Construction after the bounded dependency/asset acquisition stage SHALL run under physical egress denial as well as Cargo/dependency offline controls.

Catalog recognition SHALL remain separate from default scan admission. An absent or disabled pack SHALL NOT silently add optional-catalog extensions, including data-like or secret-bearing file families, to the default-core scan policy. An enabled pack MAY add only manifest-accepted rows that passed every pack gate to the effective scan policy.

#### Scenario: No optional parser pack is installed
- **WHEN** normal ProjectAtlas scan and navigation run
- **THEN** existing built-in language support remains fully functional without network or pack-runtime cost

#### Scenario: Optional pack is requested on macOS
- **WHEN** a user verifies, installs, enables, or invokes the optional parser pack on macOS
- **THEN** ProjectAtlas returns typed `unsupported_containment` before worker launch or source transfer, advertises no optional grammar-backed breadth, and keeps all built-in parsing available

#### Scenario: One platform realization differs
- **WHEN** a platform artifact or fresh-runner receipt has a different accepted manifest, corpus, language set, dependency policy, digest, or failed fixture
- **THEN** the aggregate pack proof fails and no optional grammar-backed breadth is advertised

### Requirement: Installed Parser Packs Are Contained And Non-Executable

Every installed parser pack SHALL bind pinned provenance, digest, license, ABI/runtime compatibility, and accepted capability rows. Normal pack use SHALL be offline and SHALL run through a supervised out-of-process boundary whose filesystem allowlist contains only immutable pack manifests/libraries plus exact unavoidable loader/runtime state, whose environment and inherited handles are cleared then allowlisted, and whose repository source arrives only as bounded raw bytes without a repository path. On Linux the exact allowed system runtime DSOs SHALL be eager artifact-audited worker mappings before `main`, after which the worker SHALL allow filesystem reads only beneath the immutable pack root. On Windows an empty artifact-scoped AppContainer profile MAY exist as unavoidable bounded sandbox state, but it SHALL expose no repository/user data and SHALL be removed through the optional-pack lifecycle. The contained worker SHALL NOT execute child processes, repository code, shell commands, builds, compilers, DNS, direct sockets, or other network requests.

Bounded Rust SHALL own the closed protocol, pre-allocation frame/input/output/stderr byte limits, one global broker-owned grammar-affined worker session, deterministic grammar/path grouping, one strict pre-READY `SessionOpen` containing only protocol version plus fresh supervisor entropy, READY validation against that exact session plus the independently observed artifact and admitted containment kind, request/progress/completion/failure replay rejection across sessions, session-bound monotonically sequenced progress, an absolute deadline that progress cannot extend, no-progress detection, aggregate task/memory/output budgets, existing task-owned cancellation/admission, exactly-once termination, pipe draining, reap/join verification, and typed pre-publication failure. The operating-system adapter SHALL own process admission before grammar loading, environment/handle inheritance, filesystem/process/exec/network containment, resource controls, process-group/job kill, and the orphan-prevention primitive. On Windows Rust SHALL own one artifact-bound x86-64 PE32+ containment broker as its direct child; construction and release verification SHALL bind its explicit Windows .NET Framework CLR v4 contract, ordinary PE-loader imports, reflected managed P/Invoke module set, method count, and normalized method digest to the exact immutable broker bytes. The broker SHALL own exactly one suspended LPAC parser-worker grandchild and no-breakaway Job Object, pass only the supervisor-created stdin/stdout/stderr endpoints through an exact `HANDLE_LIST`, configure zero capabilities, child-process denial, active-process limit one, per-process/job committed-memory ceilings, and kill-on-close, verify admission, resume, and then emit one fixed bounded adapter-local admission record. Rust SHALL validate that record before sending `SessionOpen`, SHALL interpret parser protocol bytes directly rather than through the broker, and SHALL reap the broker; the broker SHALL report success only after exact worker/job wait and cleanup. On Linux the supervisor SHALL launch only the trusted ProjectAtlas worker without grammar or source input, with exact protocol pipes and one thread; that worker SHALL install hard resource/address-space limits and `no_new_privs`, hard-require fully enforced Landlock ABI v3 with read-only pack-root access, then install seccomp process/exec/socket denial before reading `SessionOpen`. The supervisor SHALL send no grammar identity or source bytes until READY validates. Delegated cgroup v2 MAY strengthen Linux accounting when available but SHALL NOT be a hidden ordinary-user installation prerequisite. Missing required primitives SHALL fail closed. macOS SHALL return typed `unsupported_containment` before worker launch or source transfer while default-core parsing remains available.

The concrete process supervisor SHALL be owned by `projectatlas-cli` and reused by both runtime staging and the CLI-owned fresh-artifact verifier. The release verifier SHALL NOT keep a separate direct-worker or independently implemented protocol path. `projectatlas-core` SHALL remain the owner of artifact/protocol types, fixed framing, limits, identities, and validation rather than process launch or I/O-thread orchestration.

Source parse and fact graph provenance SHALL remain independently persisted and reconstructed. A grammar-backed source parse SHALL NOT relabel conservative fallback symbols, relationships, normalized confidence, or fact coverage as grammar-native after commit, reopen, reload, or incremental reprojection. Pack crash, forged/stalled progress, timeout, invalid output, flood, resource breach, cancellation, containment failure, or cleanup failure SHALL leave background task/status/cancel/read operations responsive and the active structural generation unchanged.

#### Scenario: Optional parser exceeds its boundary
- **WHEN** a pack crashes, hangs, exceeds a resource limit, requests a forbidden capability, or emits invalid output
- **THEN** the pack operation fails with bounded diagnostics while normal built-in navigation and the active generation remain available

#### Scenario: Broad catalog is larger than the accepted pack
- **WHEN** an upstream grammar catalog contains rows that lack a ProjectAtlas fixture, exact subtree license, accepted-platform proof, compatible ABI/export, or clean-machine loading evidence
- **THEN** those rows remain input inventory only and do not contribute to any advertised parsed-language count

#### Scenario: Built-in and optional grammar overlap
- **WHEN** an installed pack contains a grammar for a language already owned by the 0.3.26 built-in surface
- **THEN** the built-in parser remains authoritative and the overlapping pack entry does not add a capability row

#### Scenario: Worker receives source
- **WHEN** ProjectAtlas requests optional parsing
- **THEN** a grammar-affined child receives only bounded raw source bytes plus typed limits and manifest identities, and it cannot receive a repository path, command, compiler, build, environment block, URL, or network capability

#### Scenario: Optional grammar succeeds before language-specific fact extraction exists
- **WHEN** the contained worker proves a grammar-backed structural parse but the language has only the conservative fallback symbol or relationship extractor
- **THEN** source parse metadata records the grammar-backed result while each emitted fact retains fallback provenance and no stronger symbol or semantic support claim is made

#### Scenario: Platform memory controls differ
- **WHEN** ProjectAtlas enforces the configured worker memory ceiling on an accepted optional-pack platform
- **THEN** Windows uses its Job Object committed-memory controls, Linux uses hard address-space/resource limits plus bounded resident-memory observation and immediate process-group termination, every platform proves worker cleanup, and any sampled kill path reports its polling interval and hosted-measured maximum overshoot without claiming a stronger kernel-hard aggregate RSS primitive

#### Scenario: Worker admission cannot establish the selected containment boundary
- **WHEN** required environment/handle isolation, filesystem/process/exec/network denial, resource control, or READY authentication cannot be installed before grammar loading
- **THEN** optional parsing fails closed before source bytes are sent, no child is allowed to run outside the boundary, and that platform does not contribute to the advertised pack support claim
