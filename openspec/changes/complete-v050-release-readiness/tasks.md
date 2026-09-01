## 1. Shared release contract

- [x] 1.1 Freeze the v0.5.0 proposal, design, capability specifications, non-goals, dependency order, CLI-versus-MCP policy, and RC-first publication contract.
- [x] 1.2 Classify database, path, parser, platform, performance, installer, compatibility, security, and release-proof implications before implementation ownership begins.
- [x] 1.3 Add the durable issue-specific v0.5.0 architecture views, render every Mermaid block with Mermaid CLI, and inspect both visual communication and semantic truth.
- [x] 1.4 Map ordered issue-owned task slices, prepare exact sanitized issue-body mirrors, reconcile the native release hierarchy/dependency graph, preserve candidate-local OpenSpec/Mermaid validation for planning PRs, and require an exact clean live-default-branch IssueOps readback before readiness, milestone assignment, native relationships, or implementation handoff.

## 2. Canonical project-root identity (#481)

- [x] 2.1 Design and land one concrete typed native project identity plus lossless/versioned SQLite encoding, key/comparison rules, constraints, legacy migration, transaction, rollback, recovery, EXPLAIN/query-plan, and real write/read coverage before adapters change.
- [x] 2.2 Route CLI, MCP, configuration, watcher, worktree, telemetry, graph, and persistence through that identity; make UTF-8 display terminal and forbid lossy identity, implicit missing-index initialization, and wrong-root mutation.
- [x] 2.3 Cover `/var` and `/private/var` equivalence, symlinks, unrelated roots, missing indexes, legacy metadata equivalence/refusal, non-UTF-8 native round-trip where supported, concurrent repair/open, injected failure, rollback, and unchanged purposes, telemetry, and current generation through unit, SQLite, CLI/MCP, watcher, worktree, and macOS tests.
- [x] 2.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 3. PHP language guidance (#339)

- [ ] 3.1 Freeze PHP as the v0.5 first guidance profile after #477 acceptance; derive its syntax, symbol, relation, provenance, fallback, and abstention claims from `LANGUAGE_CAPABILITIES`, generated language-support data, PHP fixtures, and representative PHP repositories.
- [ ] 3.2 Update only the version-matched ProjectAtlas plugin guidance and its generated support references to route PHP tasks through overview, folders, files, summary, outline, search, detailed graph evidence, and exact slice without adding a generic language-guidance framework.
- [ ] 3.3 Prove positive PHP navigation, malformed and mixed HTML/PHP fallback, unsupported/dynamic behavior, generated-source abstention, CLI/MCP parity, version match, and representative-repository task outcomes in the owning language compatibility and installed-skill tests.
- [ ] 3.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 4. Reverse-caller performance decision (#342)

- [ ] 4.1 Benchmark the current `load_import_relations_for_symbols -> import_alias_map -> called_by_map` path on small, high-symbol, high-import, duplicate-alias, and large representative SQLite databases; record wall/CPU/RSS/allocations, statements/rows/bytes, `EXPLAIN QUERY PLAN`, and output bytes, and freeze a material-improvement threshold before changing code.
- [ ] 4.2 Compare the current path with the smallest shared candidate, preserving exact symbol identity, Rust/TypeScript/Python aliases, ambiguity rejection, per-target fairness, deterministic ordering, truncation, freshness, cancellation, and all-or-error SQLite failure behavior.
- [ ] 4.3 Adopt only the measured winner at the shared service/database boundary or retain current code with reproducible no-change evidence; run the existing alias unit/E2E checks plus negative, stale, duplicate, large, corrupt-row, bounded-output, and representative-plan checks.
- [ ] 4.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 5. Graph-construction scale and parallelism (#358)

- [ ] 5.1 Profile current cold scan and incremental watch across symbol parsing, structural summaries, graph derivation, staging, SQLite replacement, and cleanup on representative small/medium/large and high-edge repositories; record wall/CPU/RSS/allocations, filesystem bytes, rows, plans, transaction/WAL/checkpoint behavior, persistent bytes, cancellation, and exact graph digest.
- [ ] 5.2 Define one process-level indexing budget passed through existing `SymbolBuildOptions` and downstream stages; compare sequential reuse/batching or one shared Rayon pool with the current separate-pool lifecycle, including concurrent repositories and same-root writer contention.
- [ ] 5.3 Adopt only a measured improvement or retain current behavior; preserve exact graph equivalence, generation atomicity, deterministic ordering, bounded queues/intermediates, cancellation, late-failure cleanup, and Windows/Linux/macOS behavior.
- [ ] 5.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 6. Filtered custom-harness timeout (#372)

- [x] 6.1 Confirm `.github/workflows/release.yml` `Filtered custom harness compatibility` is the reported unbounded owner and select the existing step-level `timeout-minutes` mechanism and value consistent with neighboring release verification steps.
- [x] 6.2 Add only that step timeout, preserving the exact Cargo command, output, exit status, and workflow artifacts; do not add a timeout wrapper or child-process framework.
- [x] 6.3 Extend the existing workflow-contract assertion to cover the step name, command, and timeout and run the locked YAML/parser, targeted Cargo test, and workflow/IssueOps checks.
- [x] 6.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 7. Entrypoint-aware dead-code profiles (#384)

- [ ] 7.1 Define the non-persistent typed `EntrypointProfile` request contract, exact file/symbol anchors, supported resolved relation families, profile/input/output bounds, graph-coverage prerequisites, cursor binding, and typed uncertainty; reuse existing graph storage and reject empty, stale, wrong-root, ambiguous, unsupported, or over-budget profiles before traversal.
- [ ] 7.2 Extend the existing bounded analysis service with node-simple reachability from every accepted entrypoint and classify reachable, evidence-backed unreachable candidate, and inconclusive; adapters only decode/serialize the shared typed contract and never claim deletion safety.
- [ ] 7.3 Cover reachable, unreachable, cyclic, disconnected, multi-entrypoint, duplicate/invalid anchor, dynamic uncertainty, incomplete relation coverage, stale generation/cursor, truncation, cancellation, wrong root, deterministic replay, CLI/MCP parity, and representative Rust/PHP repository tasks with exact source evidence.
- [ ] 7.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 8. npm native distribution (#388)

- [ ] 8.1 Define the npm package name/scope, supported npm and Node floor, registry-package and native-asset provenance, supported target tuple matrix, release-version and SHA-256 digest contract, install-time versus first-run materialization, package-manager-script-disabled behavior, proxy/offline behavior, process-safe cache locking and atomic activation, cache/update policy, and ownership across npm package, release assets, installers, and runtime. Add only repository artifacts justified by that boundary.
- [ ] 8.2 Package the smallest npm adapter that selects, stages, verifies, and atomically activates the matching existing native runtime, exposes an explicit materialization path when lifecycle scripts are disabled, and preserves CLI/MCP arguments, stdout, stderr, exit codes, signals, selected root/config/database, and machine formats.
- [ ] 8.3 Cover supported and unsupported tuples, missing or mismatched version/digest, disabled lifecycle scripts, proxy/offline/cache states, concurrent and interrupted materialization, stale cache repair, update, uninstall, and CLI/MCP smoke behavior on Windows, Linux, and macOS without launching or registering an unverified runtime.
- [ ] 8.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 9. Real host configuration consumption (#390)

- [ ] 9.1 Trace installer-generated Claude Code and OpenCode configuration ownership, native schemas, absolute runtime/database/config fields, version guards, restart messaging, isolated host roots, and collision-safe repair paths.
- [ ] 9.2 Make each installed host consume its generated configuration through the real host reader and establish a ProjectAtlas MCP session without weakening per-project routing or treating structural parsing or checked-in fallback state as proof.
- [ ] 9.3 Exercise installer-generated Claude Code and OpenCode configuration through the actual installed host readers in isolated homes/config roots on supported platforms. Cover valid launch and MCP initialize/session/source-evidence readback; missing host; invalid, stale, wrong-version, and repaired configuration; shared-registry default versus explicit project-root routing; and uninstall. Do not mutate unrelated host-global configuration, credentials, authentication state, or project data.
- [ ] 9.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 10. Released-main database baseline decision (#456)

- [ ] 10.1 Measure clean normal initialization and scan, database/startup/migration/freshness/update time, release/download/unpack bytes, private-copy activation, and intended-scale CPU/RSS/I/O/WAL/persistent bytes on exact representative revisions and supported platforms; freeze a net-benefit threshold.
- [ ] 10.2 Only if the threshold wins, design the private writable copy boundary and land the smallest database identity/schema/index/query/transaction/migration/integrity/rollback/fallback delta with real write/read, active-WAL, corrupt/truncated, wrong-revision, wrong-schema, and cancellation coverage before installer/service use.
- [ ] 10.3 Prove revision and digest identity, authored-state isolation, refresh to dirty/current source, update/repair/uninstall behavior, bounded release/write amplification, and safe full-init fallback, or retain normal database creation with reproducible no-change evidence.
- [ ] 10.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 11. Deterministic architecture-community analysis (#464)

- [x] 11.1 Freeze deterministic weighted label-propagation v1, admitted resolved local relation families/weights, stable node/label order and tie-breaks, iteration and resource ceilings, stable community IDs, coverage/truncation semantics, no-persistence decision, and planted-partition acceptance bounds against the current weak-component baseline.
- [x] 11.2 Replace only the existing optional `Community` projection inside the bounded service analysis path; reuse the current normalized graph/cursor/cancellation/output machinery and add no visualization, rewrite authority, persisted table, new crate, or algorithm framework.
- [x] 11.3 Cover planted cohesive groups, giant weak component, sparse/disconnected/singleton/cyclic/high-degree graphs, equal-score ties, invalid parameters, non-convergence, incomplete/stale/wrong-root evidence, cancellation/truncation, deterministic repeat, CLI/MCP parity, and representative-scale CPU/RSS/latency/output bounds.
- [x] 11.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 12. Bounded PDF and DOCX extraction (#465)

- [ ] 12.1 Freeze PDF and DOCX as the only v0.5 formats; pin and audit `pdf-extract` 0.12.0, `quick-xml` 0.42.0, and `zip` 0.6.6 plus their exact locked transitive trees for license, security, unsafe, panic, decompression, cancellation, and supported-format limits; admit only PDF content streams and DOCX `word/document.xml`; define PDF page/text-span and DOCX part/paragraph/run/text-span locators, magic/extension admission, provenance, completeness, compressed/input/expanded/output/time/memory/recursion/entry ceilings, typed failures, sparse-link policy, and the exact existing-or-new SQLite schema/index/query/transaction plan before adapters.
- [ ] 12.2 Implement one bounded in-process extraction boundary in `projectatlas-symbols` and runtime publication using the pinned crates and existing text/graph services; DOCX admits only the declared XML part, PDF executes no scripts or external references, and neither format invokes arbitrary programs, OCR, macros, networking, or embedded recursive parsers.
- [ ] 12.3 Cover valid multi-page PDF and DOCX, exact locator round-trip, malformed/truncated/encrypted/password PDF, ZIP slip/duplicate entry/compression bomb/oversized/recursive DOCX, unsupported/mismatched magic, non-UTF-8 metadata, cancellation/timeout, wrong root, sparse linkage, incremental replace/delete, rollback, CLI/MCP navigation, and Windows/Linux/macOS E2E with intended-scale CPU/RSS/I/O/database/output bounds.
- [ ] 12.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 13. Invalid graph identity admission (#476)

- [x] 13.1 Inventory every parser-derived package, symbol, parent/scope, relation, and resolution-key producer; keep strict graph constructors unchanged and route all producers through one concrete admission/classification result at the smallest shared source-graph boundary.
- [x] 13.2 Land any proven SQLite coverage/unresolved representation gap first, then publish every valid row plus bounded typed rejection reason, file/span/parser/field provenance, and generation identity without lossy rewriting or partial-current advertisement.
- [x] 13.3 Cover valid, empty, surrounding-whitespace, control-character, oversized, reserved-namespace, malformed-parser, duplicate, mixed-validity, multi-sibling-folder, incremental-watch, cancellation, fault, rollback, retry, CLI, and MCP behavior.
- [x] 13.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 14. Built-in PHP support (#477)

- [ ] 14.1 Pin `tree-sitter-php` 0.24.2 against the workspace Tree-sitter 0.26.9 contract and freeze PHP 8 registration, grammar dispatch, symbol kinds/parents/signatures, byte/line/column spans, namespace/import/include/call relation rules, parser provenance, mixed HTML/PHP handling, dynamic/unsupported fallback, cancellation, and file/node/output budgets without a provider framework.
- [ ] 14.2 Add PHP to the existing `LanguageCapability` and built-in Tree-sitter dispatch and implement the smallest PHP-specific node mapping inside `projectatlas-symbols`; centralize identifiers at the owning registry/parser boundary and preserve existing runtime/graph publication.
- [ ] 14.3 Cover functions, namespaces, classes/interfaces/traits/enums, methods/properties/constants, `use` aliases, include/require, malformed/recovery trees, mixed HTML/PHP, dynamic constructs, duplicate/ambiguous names, exact spans, large/canceled parses, incremental refresh, CLI/MCP navigation, representative Composer repositories, and Windows/Linux/macOS behavior.
- [ ] 14.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 15. Complete bounded document-graph publication (#480)

- [x] 15.1 Inspect existing keys, constraints, indexes, `EXPLAIN QUERY PLAN`, prepared statements, savepoints, and generation ownership; design only the smallest database-owner change needed to validate whole-publication invariants and process each work unit at or below `GraphLimits::MAX_ROWS`.
- [x] 15.2 Publish every valid unresolved and resolved document row through bounded prepared chunks inside one transaction/generation, advertising current only after complete success and retaining the previous generation after fault or cancellation.
- [x] 15.3 Cover below, at, one-above, and multiple-ceiling totals; duplicates and contradictions crossing chunk boundaries; mixed resolved/unresolved rows; symlink targets; staged and immediate callers; incremental refresh; fault between chunks; rollback; cancellation; retry; query plan; CPU, memory, I/O, and write-amplification bounds.
- [x] 15.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 16. Rust 1.98.0 deterministic upgrade (#482)

- [x] 16.1 Record Rust 1.93.1 as the historical reproduction baseline, update the sole repository toolchain declaration to exact stable Rust 1.98.0, and define the explicit stable-upgrade policy: evaluate each intended stable release in an issue/PR, pin a numeric version only after all gates pass, and never use floating stable in CI or release inputs.
- [x] 16.2 Route local developer validation, CI, optional-parser construction, packaging, installer-developer paths, and release workflows through the repository declaration; add one early preflight that reports expected and actual rustc, cargo, clippy, and rustfmt identity and fails before expensive or mutating jobs, with no duplicated version literals or host-global default change.
- [x] 16.3 Reconcile Rust 1.98.0-owned lock/build output and pass format, workspace/all-target/all-feature check, pedantic Clippy, unit/integration/doc/E2E, cargo-deny, feature/target, parser-pack, packaging, and installer smoke gates on Linux, Windows, macOS x64, and macOS Apple Silicon; cover missing Rustup, Homebrew/system Cargo precedence, explicit override, 1.93.1, and other mismatches.
- [x] 16.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 17. Truthful macOS Apple Silicon optional parser (#483)

- [x] 17.1 Consolidate containment, platform tuple, parser-pack, installer, lifecycle, supervisor, runtime/MCP capability, built-in fallback, feature gating, and tests behind one closed typed `PackPlatform`/capability authority with exhaustive matching and no provider framework.
- [x] 17.2 Define macOS Apple Silicon optional parsing as typed unavailable for v0.5.0; fail install, update, verify, selection, and worker startup before mutation, expose documented built-in-parser fallback, and make no unsupported containment or pack claim.
- [x] 17.3 Cover capability truth, pre-mutation refusal, built-in parsing/navigation, stale and wrong packs, supported-platform install/update/startup/parsing, resource limits, cancellation, crash, cleanup, and Linux/Windows/unsupported-tuple compatibility behavior.
- [x] 17.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 18. Native non-UTF-8 worktree identity (#484)

- [ ] 18.1 After #481, reuse its native identity and lossless/versioned SQLite codec for worktree root, Git common directory, and Git administrative directory; land the smallest key/constraint/migration/transaction/rollback/recovery delta with real write/read and duplicate-key proof before adapters change.
- [ ] 18.2 Remove premature UTF-8 conversion from registration, alias routing, duplicate detection, retirement, capacity, watcher, filesystem, Git process, CLI, and MCP boundaries; keep identity native and make UTF-8 display a terminal typed-unavailable conversion.
- [ ] 18.3 Cover invalid UTF-8 bytes independently in root/common/admin paths, valid Unicode, aliases, duplicate native registration, capacity, retirement, watcher and Git command invocation, UTF-8-only CLI/MCP output, migration, injected failure, rollback, retry, and supported Linux/macOS compatibility.
- [ ] 18.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 19. Clean macOS Apple Silicon installed lifecycle (#485)

- [ ] 19.1 After #481, #482, #483, #484, and #486, build one clean macOS Apple Silicon harness with isolated HOME/config/cache/project state, no pre-existing database, exact Rust 1.98.0 source validation, and installation of the exact packaged candidate path/version/digest rather than checkout or ambient binaries.
- [ ] 19.2 Exercise install, init/schema creation, scan, overview, exact files/summary/slice navigation, CLI/MCP session identity and host config, `/var`/`private-var` routing, worktree add/route/remove, watcher refresh, telemetry, symlinked documents, built-in parsing, and typed optional-parser unavailability.
- [ ] 19.3 Cover missing and unrelated roots without mutation, legacy alias recovery, non-UTF-8 worktree identity where the runner permits, stale/wrong parser pack, injected command/database failure, cancellation, retry, cleanup, and absence of residual process/config/database state; classify any failure at its owning subsystem without weakening assertions.
- [ ] 19.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 20. macOS all-features warning cleanliness (#486)

- [x] 20.1 On actual macOS x64 and arm64 with Rust 1.98.0, run the locked release-equivalent all-target/all-feature check and pedantic Clippy commands and record every warning name, file:line diagnostic, target/features, runtime reachability, and owning module; trace current diagnostics to #483's canonical capability authority or retain current behavior with reproducible no-change evidence when the matrix is clean.
- [x] 20.2 For reproduced warnings only, move impossible backend/lifecycle code behind the smallest owning target/feature module or item so supported configurations compile reachable code, shared built-in fallback remains present, and no crate/module-wide `allow(dead_code)`, duplicated platform matrix, or warning downgrade is added.
- [x] 20.3 Run warnings-as-errors for default, no-default, all-feature, all-target, and release-owned combinations on macOS x64/arm64 and supported Linux/Windows optional-parser tuples; cover typed unavailability, built-in fallback, supported worker startup, and Cargo check/Clippy compatibility.
- [x] 20.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 21. CLI E2E suite contract split (#487)

- [ ] 21.1 Produce and accept a complete test-to-domain move map for every `e2e.rs` test/helper/platform gate using `e2e_lifecycle.rs`, `e2e_delivery.rs`, `e2e_navigation.rs`, `e2e_worktrees.rs`, `e2e_maintenance.rs`, and existing separate suites; identify exact shared-support users and merge any proposed binary whose inventory proves no cohesive boundary rather than preserving symmetry.
- [ ] 21.2 Move one coherent domain at a time, extracting only multiply-owned process/repository/JSON/platform/package support, preserving durable test names and ignored/platform attributes, and keeping each intermediate integration binary runnable.
- [ ] 21.3 Compare pre/post test inventory and CI command selection and prove no test, assertion, ignored contract, platform gate, timeout, cleanup, process isolation, packaged-product path, or release selection was dropped or silently weakened; run each binary plus workspace/all-feature gates on affected platforms.
- [ ] 21.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 22. Production module responsibility decision (#488)

- [x] 22.1 Produce and accept a call/state/data/transaction/concurrency/error/test/hot-path move map for `mcp.rs`, `runtime.rs`, database `lib.rs`, and `repository_graph.rs`, including existing submodules, public re-exports, schema/SQL authority, cancellation, lock/transaction scope, dependency direction, and a no-change disposition for every rejected split.
- [x] 22.2 Apply only accepted cohesive moves into the fewest durable domain modules, one behavior boundary at a time, preserving seven crates, public API/wire/CLI behavior, SQL ownership, generation/transaction atomicity, cancellation, error chains, and platform behavior; retain current layout where no independent owner is proven.
- [x] 22.3 After each move run owning unit/integration tests and final API/serde compatibility, real SQLite plan/transaction/rollback/corruption checks, MCP/CLI smoke, concurrency/cancellation/fault tests, E2E/platform gates, and intended-scale compile/startup/CPU/RSS/I/O/database/output comparisons.
- [x] 22.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 23. Oversized benchmark artifact retention (#489)

- [x] 23.1 Confirm the 42,809,126-byte `docs/benchmarks/v0.4-agent-navigation-failed-binary-init-29a4863.jsonl` is reproducible from the retained preregistration/harness and define the smallest tracked benchmark-result size/allowlist rule after checking all current benchmark artifacts, compressed/release artifacts, Git LFS absence, `.gitignore`, and publication alternatives.
- [x] 23.2 Remove only that raw JSONL from the current tree without rewriting history; retain a compact sanitized failure summary with candidate/repository revision, harness/runtime identity, failure classification, digest/size, and reproduction command if those facts remain useful.
- [x] 23.3 Extend the narrow existing benchmark/repository policy check to reject an accidentally tracked oversized raw benchmark output while allowing normal fixtures, explicitly approved compact results, ignored local outputs, compressed/release assets, and tested false positives; wire it through the existing CI/release policy owner.
- [x] 23.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 24. Repeatable real-task agent evaluation (#490)

- [ ] 24.1 Gap-audit the existing agent-navigation preregistration, harness, tests, and v0.4 results; freeze v0.5 representative tasks covering PHP, documents, dead-code profiles, communities, and ordinary navigation, with exact repository/candidate/runtime/tool identity, equal-arm prompts, warm/cold state, success rubric, timeout, context/wrong-file accounting, privacy, output bounds, repeats, and uncertainty.
- [ ] 24.2 Reuse the existing harness unchanged when it satisfies the frozen contract; make only demonstrated minimal corrections, then run both arms in rotated order while retaining every success, failure, timeout, invalid trace, setup result, and self-audit rather than filtering unfavorable outcomes.
- [ ] 24.3 Validate repeatability and metric calculations, equal treatment of arms, modeled-versus-observed wording, bounded/sanitized artifacts, compact retained evidence, representative repository behavior, and no private paths/content/secrets; publish the v0.5 preregistration, bounded result, and evaluation, with an explicit no-product-code outcome when appropriate.
- [ ] 24.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 25. Complete cross-platform atlas CLI (#491)

- [ ] 25.1 Inventory the canonical executable, compatibility executable, installer-owned shim, PATH, package-manager, completion, help, and existing command-namespace collisions, explicitly including `health-check` and the administrative `health resolve` route; define one platform-neutral dispatch and lifecycle contract without introducing a second runtime or command framework.
- [ ] 25.2 Add one installer-managed collision-safe `atlas` forwarder to the exact verified runtime on Windows, macOS, and Linux, preserving `projectatlas` and the complete present/future argument vector without per-subcommand executables.
- [ ] 25.3 Harmonize the existing `health` namespace so `atlas health [report flags]` performs the read-only health report, `atlas health resolve ...` retains the existing administrative route, and `health-check` remains a compatibility alias. Cover zero-argument dispatch, help/completion, report-flag versus subcommand ambiguity, stdout/stderr bytes, JSON/TOON, exit status, child signals, Windows/Linux/macOS collisions, stale-shim repair, update, uninstall, and concurrent project isolation.
- [ ] 25.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 26. v0.5.0 release acceptance (#492)

- [ ] 26.1 Freeze one exact v0.5.0 candidate revision, the native hierarchy with #492 as parent of every other milestone issue, the declared direct-blocker graph, the complete supported CLI/MCP/host/public-route inventory, accepted compatibility dispositions, platform matrix, isolated-fixture policy, and holistic success criteria; require every child issue and required review to be closed successfully and every accepted issue's mapped OpenSpec task source plus architecture URL, heading, and Mermaid to resolve from exact published `main` before release acceptance.
- [ ] 26.2 Build the exact candidate with the Rust 1.98.0 workspace, database, IssueOps, OpenSpec, documentation, security/dependency, package, installer, asset, checksum/integrity, and release-policy gates; run the milestone gate from an exact clean checkout of the live default-branch revision, fail on candidate-only, stale, dirty, missing, or malformed issue evidence, abort on revision drift, reject partial accepted work, and preserve v0.4.5 as Latest.
- [ ] 26.3 On every supported Windows, Linux, macOS x64, and macOS arm64 tuple, install the exact candidate and reconcile the complete command/tool manifest by safely executing every CLI command and nested command plus every MCP tool, including unchanged routes, against isolated fixtures; cover root/worktree routing, freshness, JSON/TOON, stdout/stderr, exit/error schemas, source evidence, task status/cancellation, mutations, administration, legacy aliases, accepted removals, and database continuity.
- [ ] 26.4 Run one clean holistic installed-product E2E and a publication hard gate that starts from an exercised `v0.4.5` installation and project database, updates that same state to the exact candidate on every supported platform, preserves project identity, authored purposes, telemetry, worktree registrations, roots, current generation, and source evidence, and proves binary/npm/plugin/host convergence, init/scan/navigation, PHP/documents/graph/analysis, watcher/parser behavior, injected installer or schema-migration failure, atomic refusal, repair/retry, uninstall, concurrency, cancellation, and compatible rollback without destructive reinitialization or unrelated host/project mutation.
- [ ] 26.5 With explicit publication authorization, publish `v0.5.0-rc1` as a non-draft prerelease from the exact accepted revision, independently read back tag, metadata, assets, checksums, installers, runtime/plugin/skill/MCP/CLI/host identity and E2E results, preserve v0.4.5 as Latest, and return every confirmed defect to its sanitized owning IssueOps/OpenSpec issue before another complete candidate.
- [ ] 26.6 After an accepted release candidate and explicit promotion authorization, repeat the complete installed and hosted proof for stable v0.5.0, verify v0.5.0 becomes Latest and downstream pins agree, synchronize OpenSpec/issues/reviews/milestone state, and close #492 last.
- [ ] 26.7 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.
