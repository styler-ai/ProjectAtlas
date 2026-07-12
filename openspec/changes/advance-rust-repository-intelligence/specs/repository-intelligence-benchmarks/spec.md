## ADDED Requirements

### Requirement: Pinned Reproducible Comparison Harness
ProjectAtlas SHALL provide a machine-readable comparison harness that pins ProjectAtlas and accepted comparison-baseline revisions, public repository commits and byte manifests, language/ignore/index/capability profiles, toolchains, hardware/OS/firmware metadata, commands, timeouts, cache and warmup policy, worker/concurrency levels, resource/query limits, repetitions, raw outputs, and result schemas. Comparative claims SHALL use the same repository bytes, accepted capability set, equivalent inclusion/coverage policy, and equivalent output limits. Differences that cannot be normalized SHALL be published and SHALL make the affected comparative cell ineligible rather than silently favoring either system.

Tool provenance SHALL name its exact boundary. The Phase 0 calibration launcher MAY bind Cargo and rustc executables plus the observed command/test-executable digest without pre-binding every linker, SDK, or generated test executable, but its manifest and retained evidence SHALL state that narrower scope and SHALL NOT support a complete-toolchain-provenance claim. A release comparison that requires complete toolchain provenance SHALL additionally bind those components or mark the affected result ineligible.

#### Scenario: Comparison run is reproduced
- **WHEN** a maintainer runs the recorded command on the declared environment
- **THEN** it resolves every pinned input and emits raw plus summarized results linked to exact commits and configuration

#### Scenario: Tool provenance is narrower than the full build chain
- **WHEN** the calibration harness pins Cargo and rustc but only observes the generated executable and inherits linker or SDK resolution inputs
- **THEN** the manifest and evidence bind the narrower provenance scope, reject arbitrary manifest-selected environment names before reading values, and prohibit complete-toolchain-provenance claims

#### Scenario: Evidence directory identity persistently changes
- **WHEN** a reserved evidence parent or journal directory has a different canonical target or operating-system identity at an explicit pre-publication or post-publication verification boundary
- **THEN** ProjectAtlas detects the persistent drift and marks the run ineligible; claim-eligible calibration additionally requires a trusted local workspace with no pre-existing reparse ancestry and no concurrent same-user namespace mutation, because path-based publication does not claim resistance to swap-and-restore races between verification points

#### Scenario: Baseline command fails
- **WHEN** either product exits, times out, corrupts output, or silently omits required coverage
- **THEN** the harness records the failure as a result and does not convert it to a successful zero metric

### Requirement: Benchmark Eligibility And Statistical Plan
Before measured runs, each benchmark family SHALL publish an immutable plan that defines its experimental unit, corpus and strata, paired/randomized execution order, required capabilities and coverage, configuration equivalence, primary and secondary metrics, direction, practical threshold or non-inferiority margin, alpha, confidence-interval method, multiplicity correction, cache/warmup protocol, minimum sample size/power, timeout, failure/exclusion rules, and allowed rerun policy. A measured run SHALL be eligible only when pinned inputs resolve, required capabilities are ready, correctness/coverage gates pass, commands complete within limits, outputs validate, no undeclared fallback or network access occurs, and the declared environment/cache state is proven. Failures and ineligible cells SHALL remain in raw and summary artifacts; a claim SHALL be blocked when required eligible sample counts are not met.

Performance cells SHALL use at least ten measured paired repetitions in randomized blocks after declared warmups. Cold process-start cells SHALL use at least thirty measured launches. Query-latency cells SHALL use at least one hundred warmup requests followed by at least one thousand measured requests for each query/concurrency cell. The harness SHALL use a monotonic high-resolution clock, retain every sample, and SHALL NOT select the fastest subset or stop early after a favorable result.

Unless a more conservative accepted method is predeclared, paired time/RSS ratios and geometric means SHALL use a 10,000-resample bias-corrected bootstrap clustered by repository/run; latency percentiles SHALL use a 10,000-resample hierarchical bootstrap over runs and requests; and accuracy/agent metrics SHALL use a paired 10,000-resample bootstrap clustered by unique fixture/task inside fixed pre-registered repository strata with frozen weights. Threshold decisions SHALL use one-sided 95% confidence bounds in the adverse direction. Claims spanning multiple required corpora, languages, relation families, or primary metrics SHALL control family-wise error with Holm correction. Repeated model runs or queries from one fixture/task SHALL NOT be counted as independent experimental units. A claim generalized beyond the pinned repositories SHALL require a power-justified number of independent repository clusters rather than treating three repositories as a population sample.

The analysis manifest SHALL pin its random-number algorithm, seed, and implementation version. Proportion gates SHALL pre-register Wilson, Newcombe, or exact intervals as appropriate. Zero denominators, one-class fixtures, bootstrap-degenerate samples, and language/relation cells below their declared minimum positive and negative ground-truth counts SHALL be ineligible and SHALL never be reported as 100% precision, recall, or correctness.

#### Scenario: Required benchmark cell is ineligible
- **WHEN** a run has a timeout, crash, incomplete coverage, wrong limits, undeclared fallback, missing raw artifact, or unproven cache state
- **THEN** it remains a recorded failure/ineligible cell, is not dropped or replaced opportunistically, and the affected claim remains blocked until the predeclared rerun policy is satisfied

#### Scenario: Confidence interval crosses a gate
- **WHEN** the point estimate passes but the corrected adverse confidence bound does not
- **THEN** the benchmark decision fails or remains inconclusive and no passing/superiority claim is published

### Requirement: Language And Graph Correctness Benchmarks
The benchmark corpus SHALL measure per-language and per-relation-family precision, recall, F1, abstention, parse-error coverage, stable identity, and source-span correctness from machine-readable ground truth. It SHALL include non-vacuous extraction fixtures, embedded languages, ambiguous names, external dependencies, malformed syntax, framework conventions, and cross-language service identities. Every advertised symbols/semantic/protocol family SHALL receive its own corrected threshold decision using the required adverse confidence bound; micro/macro aggregates, extra capabilities, or stronger families SHALL not offset a failing family. Manual PASS/PARTIAL grading and node counts MAY supplement but SHALL NOT replace these metrics.

#### Scenario: Aggregate score hides a weak language
- **WHEN** the overall score passes but one required language/relation falls below its threshold
- **THEN** the release gate fails or that specific capability remains at a lower advertised tier

### Requirement: Determinism And Incremental Correctness Benchmarks
The release harness SHALL index identical checkouts through clean full scans, mutation-driven incremental scans, worker counts 1 and a manifest-fixed N, and at least ten repeated runs for nondeterminism-sensitive enrichments. Canonical active entities, relations, lexical rows, stable IDs, coverage, and serialized ordering SHALL match within explicitly excluded slot/epoch/time fields. Optional vectors SHALL compare normalized model input bytes, model/tokenizer/preprocessing identity, eligible membership, and values by byte equality or a predeclared per-component tolerance. ANN topology/layout SHALL be excluded; seeded/tie behavior and declared Recall@K/top-K overlap SHALL remain required.

#### Scenario: Full and incremental results diverge
- **WHEN** canonical graph snapshots differ after reaching the same repository state
- **THEN** the integrity gate fails with the first entity/relation/coverage difference and no parity claim is published

### Requirement: Unicode And Cross-Platform Correctness
Release tests SHALL exercise real ProjectAtlas processes and supervised workers on Windows, macOS, and Linux using UTF-8 content, CJK, emoji, spaces, shell metacharacters, long supported paths, and NFC/NFD path variants where the filesystem permits them. String truncation, SQLite storage, source reads, subprocess handoff, TOON/JSON, and snapshots SHALL preserve valid text and root safety.

#### Scenario: Windows non-ASCII repository is indexed
- **WHEN** a repository root and source paths contain supported non-ASCII characters
- **THEN** discovery, parsing, persistence, worker communication, search, summary, slice, and graph queries complete without path recoding or crash

### Requirement: Performance Superiority Gate
On the pinned shared corpus and declared release hardware, only configurations that pass the accepted capability, correctness, coverage, limit, and eligibility gates SHALL enter performance comparison. Every baseline SHALL run in randomized paired blocks on the same eligible host and SHALL be reported separately rather than pooled. The upper corrected one-sided 95% confidence bound for the paired cold-index time ratio SHALL be at most 1.10 on every required corpus. The upper bound for the paired geometric-mean cold-index time ratio and peak-RSS ratio SHALL each be at most 0.80. The upper bound for equivalent structural-retrieval p95 latency ratio SHALL be below 1.00. Persistent bytes and every other named resource SHALL satisfy their independent gates below.

The 1 ms/50 ms warm SQLite/service and 50 ms/150 ms warm MCP values for simple indexed and bounded three-hop queries at the declared million-node scale SHALL be reference-host goals, not portable machine-independent constants. Before implementation measurements, Phase 0 SHALL pin the reference-host class, calibration workloads, raw envelope-construction rule, and a tolerance factor `tau` no greater than 1.25. Section 11 SHALL run the before/after calibration and absolute-goal decisions after the measured implementation stabilizes. A run is eligible for an absolute-goal decision only when runner image, toolchain, power policy, and before/after CPU, memory, and SQLite calibration results remain inside the frozen eligibility envelope. Measured latency SHALL NOT be divided by a calibration score. The adverse one-sided 95% bound SHALL be at most `tau` times the applicable reference goal. Uncalibrated hosted-runner timings are informational and cannot pass or fail the reference-host gate.

Any claimed CLI or cold-MCP path SHALL have a separately predeclared gate. A faster inner layer SHALL NOT substitute for an end-to-end claim. Unmet, ineligible, or inconclusive absolute-goal or paired-comparison targets SHALL block “faster” or “surpasses” release claims.

#### Scenario: Rust implementation is not faster
- **WHEN** the measured head-to-head results miss a superiority threshold
- **THEN** ProjectAtlas publishes the result, does not attribute unmeasured performance to Rust, and keeps optimization work open

#### Scenario: Faster result loses correctness
- **WHEN** a faster configuration omits required graph facts, reduces F1, or reports partial coverage as complete
- **THEN** that configuration is ineligible for the superiority gate

### Requirement: Persistent Bytes Per Retained Fact
Persistent derived bytes SHALL be measured after the declared publication, checkpoint, close, and sidecar-retention procedure and SHALL include the SQLite main database, retained WAL/SHM bytes, FTS indexes, vector/ANN indexes, and every derived sidecar needed to answer the benchmark. Source repositories, logs, temporary staging that has been successfully cleaned, and authored purpose/telemetry/settings bytes SHALL be reported separately and SHALL not be hidden in or removed from totals selectively.

The structural denominator SHALL be the count of unique true-positive canonical retained entities, logical relations, and evidence occurrences that pass ground-truth reconciliation; duplicate rows and false positives SHALL consume bytes but SHALL not increase the denominator. Semantic storage SHALL additionally report bytes per valid active vector and per labeled query-family improvement. Each corpus and geometric mean SHALL report total bytes, fact counts, bytes per retained structural fact, and pack-specific bytes. Persistent bytes per retained true fact SHALL pass a predeclared non-regression bound against both the prior ProjectAtlas release and accepted comparison baseline. A storage-superiority claim additionally requires its corrected adverse ratio bound to pass a predeclared practical margin below 1.00. A statistically supported correctness or agent-value improvement MAY justify shipping a larger index, but SHALL NOT support a claim that storage is superior; prose or raw node counts alone SHALL not satisfy either decision.

#### Scenario: Larger index contains duplicate or false facts
- **WHEN** storage grows because facts are duplicated or fail ground-truth precision
- **THEN** those rows increase the byte numerator without increasing retained-fact count and cannot justify the size regression

### Requirement: Incremental Performance And Write-Amplification Gate
Benchmarks SHALL measure no-change scans, one-file changes, dependency fan-out changes, dirty-worktree watch behavior, database bytes written, WAL growth, CPU, RSS, and time to publish. Physical derived-data write bytes SHALL count successful writes to the live database, WAL/SHM, staging database, SQLite temporary/sort files, FTS, vector/ANN data, and required sidecars, including overwritten same-size pages. Logical row/page changes and final file-size deltas SHALL be reported separately and SHALL not substitute for physical writes. Every accepted mutation class SHALL pass a predeclared write non-regression bound against both baselines; write-superiority claims require a separate practical ratio margin below 1.00. No-change watch cycles SHALL perform no graph rewrite, repeated unchanged dirty states SHALL be coalesced, and one-file changes SHALL write rows proportional to the affected dependency closure rather than rewrite the whole graph.

#### Scenario: Dirty worktree remains unchanged
- **WHEN** multiple watcher intervals observe the same content/Git state signature
- **THEN** ProjectAtlas performs no repeated index publication or unbounded disk writes

### Requirement: Retrieval And Agent-Efficiency Evaluation
Lexical/semantic retrieval SHALL report MRR, nDCG@10, Recall@10, precision at declared cutoffs, p50/p95 latency, index/update cost, RSS, bytes, and corrected confidence bounds for every advertised query family. A blinded paired agent-task suite SHALL compare answer correctness, unsupported assertions, context tokens, file reads, tool calls, and wall time using the same pinned model/version, system/user prompt, repository bytes, tool permissions, budgets, and task rubric. Assignment and answer presentation SHALL be randomized and blinded; judge model/version or human graders, adjudication, and inter-rater agreement SHALL be retained.

The paired suite SHALL also evaluate the agent experience through predeclared observable rubric fields: elapsed time and tool calls to first useful context, wrong or redundant tool selections, retries or backtracking caused by unclear, incomplete, or untrusted results, usefulness of recommended next actions, evidence traceability for the final answer, successful completion without a broad-read escape, and blinded task-end workflow preference. Correctness and unsupported-assertion gates remain mandatory and cannot be traded for preference. Every experience field SHALL receive an independent no-regression decision against the prior ProjectAtlas release; v0.4 SHALL remain open when agent-workflow reviewers do not conclude from retained transcripts and decisions that ProjectAtlas is the preferred first repository tool.

The suite SHALL contain at least thirty unique tasks across at least three repositories and three task families and SHALL satisfy a predeclared power analysis of at least 80% power for the five-percentage-point practical quality difference at corrected alpha 0.05. It SHALL run at least three independent model attempts per task when stochasticity exists, but statistical resampling SHALL cluster those attempts by unique task and repository.

Program completion SHALL require non-inferiority to both the prior ProjectAtlas release and the accepted comparison baseline: the corrected one-sided 95% lower confidence bound for paired answer-quality difference SHALL be greater than -2 percentage points, while the upper bound for paired context-token and tool-call differences SHALL be at most zero and unsupported assertions SHALL not regress. A “superior agent quality” claim SHALL additionally require a lower confidence bound above zero and a point improvement of at least five percentage points. When baseline quality exceeds 90%, ProjectAtlas MAY separately claim the 95% absolute-quality target only when its lower confidence bound is at least 95%; that absolute target SHALL NOT be called superiority unless the paired superiority rule also passes.

For every frozen normal atlas-first workflow family, including startup/orientation, locate, inspect/summary, and exact slice, the mandatory call sequence SHALL not grow. The corrected adverse bound for file reads and tool calls SHALL be no greater than zero versus the prior ProjectAtlas release, and the comparison baseline SHALL be reported separately. Default graph enrichment MAY add no more than 512 UTF-8 bytes and 128 conservative context tokens to any one enriched response, nor more than 1,536 bytes and 384 tokens across one frozen normal workflow; Phase 0 MAY tighten but SHALL NOT loosen those caps after results are observed. Every response SHALL also remain inside its existing absolute output cap. The blinded end-task gate above still requires overall context-token non-regression, so bounded additive context is accepted only when it avoids at least as much later context in real agent work. Workflow families SHALL be decided independently so one improvement cannot hide another workflow's regression. Explicitly requested architecture, impact, or trace analysis SHALL be reported outside the normal-workflow gate.

#### Scenario: Agent task comparison runs
- **WHEN** both systems answer the pinned task set under the same model and budgets
- **THEN** raw transcripts, judge rubric/results, tool calls, tokens, and confidence intervals are retained and the declared gate is computed mechanically

#### Scenario: Repeated attempts are treated as independent tasks
- **WHEN** multiple stochastic runs of one task would make a confidence interval appear narrower
- **THEN** validation fails unless the analysis clusters by unique task/repository and applies the predeclared paired method

### Requirement: Cache, Concurrency, Limits, And Timing Separation
Every benchmark cell SHALL declare exact file/input caps, worker count, query top-K/row/depth/visited-node/expanded-edge/time/memory limits, response byte limit, cancellation deadline, and concurrency. Compared systems SHALL receive equivalent semantic limits; a run that silently raises a limit, truncates differently, or uses an undeclared fallback is ineligible. Index tests SHALL run with worker counts 1 and `N`, where `N` is fixed in the environment manifest before measurement. Query tests on the release benchmark host SHALL run at concurrency 1, 4, and 16, plus a declared mixed workload with one publication task and eight concurrent readers; throughput, p50/p95, errors, cancellation, and RSS SHALL be reported for every cell.

Memory results SHALL cover the complete ProjectAtlas process tree and name the platform metric. The harness SHALL prefer aggregate PSS or an equivalent non-double-counted resident metric and SHALL also retain private or committed bytes where available. The manifest SHALL define sampling interval, warmup, observation window, child-process discovery, and how shared pages are treated. A ProjectAtlas-owned worker or optional pack SHALL NOT disappear from RSS accounting because it runs out of process.

Cold indexing SHALL start from a fresh immutable checkout, no prior index/database, no surviving process, and a documented OS page-cache reset or fresh ephemeral machine/VM. If cold-cache state cannot be proven, the cell SHALL be labeled cache-unknown and excluded from cold claims. Warm indexing/query states SHALL be created only by the exact predeclared warmup sequence and SHALL record whether process, graph snapshot, SQLite connection/prepared statements, SQLite page cache, and OS page cache are warm.

Timing layers SHALL be measured and named separately: warm SQLite timing runs from prepared-statement bind through final validated row decode on an already-open warmed connection; warm service timing runs from typed service entry through a fully materialized result with the declared graph/SQLite state and excludes transport serialization; warm MCP end-to-end timing runs on an initialized process from immediately before the client writes the request frame through receipt and validation of the complete response frame, including transport, deserialization, service, and serialization; cold MCP end-to-end timing includes process launch, initialization/handshake, client request, and validated complete response; CLI end-to-end timing includes process launch through validated output and exit. Harness setup, corpus copying, and result-file writing SHALL be outside operation timing but retained separately. No service/SQLite number SHALL be labeled as MCP or CLI latency.

#### Scenario: Warm service meets a target but MCP does not
- **WHEN** the service-layer p95 passes while serialization, transport, or adapter work causes warm MCP p95 to miss
- **THEN** only the service claim passes and the MCP/end-to-end claim remains blocked

#### Scenario: Concurrent reads overlap publication
- **WHEN** eight readers query while a bounded full or incremental publication runs
- **THEN** every response binds to one complete slot/epoch, error/timeout rates stay within the declared gate, and concurrency results are reported separately from single-client latency

### Requirement: Package, Supply-Chain, And Network Gates
Benchmarks and release gates SHALL record default-core and each optional-pack binary/artifact/installed size, install/remove time, separated cold/warm startup and idle/active process-tree RSS, grammar/model/runtime inventory, per-component SBOM entries, source revisions, digests, licenses, local patches, and dependency advisories. Installation results SHALL separately report transferred bytes, compressed artifact bytes, installed logical bytes, allocated filesystem bytes, and managed-artifact count. Install timing SHALL end only after a real initialized overview response validates the installed runtime. Each named resource dimension SHALL pass its own predeclared non-regression decision against both baselines; a dimension-specific superiority claim requires a practical corrected ratio margin below 1.00. Correctness or agent value MAY justify shipping a larger result but SHALL NOT make that resource dimension superior. The default-core profile SHALL enforce the size/startup/RSS budgets in the language registry specification and SHALL prove through dependency-tree, imported-symbol/dynamic-library, and packaged-file audits that optional WASM/model/ANN/GPU/download/heavy grammar runtimes are not linked or shipped. Pack-enabled costs SHALL be reported incrementally against that core. Integrity self-tests SHALL prove missing, modified, extra, absolute-path, stale-manifest, and zero-assets-checked states fail. Normal scan/query tests SHALL fail on unexpected network access.

#### Scenario: Integrity manifest verifies zero assets
- **WHEN** checksum paths are invalid, absolute, or all assets are missing
- **THEN** the gate fails rather than reporting success with a zero checked count

### Requirement: Public Results And Claim Discipline
Every advertised language count, semantic tier, parity statement, token reduction, query latency, indexing time, memory result, and package claim SHALL link to the generated capability manifest or a versioned benchmark artifact. Results SHALL include failures, partial coverage, excluded cases, confidence intervals where applicable, and methodology changes. README numbers SHALL be generated or validated against these artifacts.

#### Scenario: Documentation count drifts
- **WHEN** README or release text differs from the generated registry/benchmark result
- **THEN** documentation/release validation fails before publication

### Requirement: Plugin-Store Installation And Recovery Simplicity
On a clean supported host with only the supported agent harness and operating-system prerequisites, one official ProjectAtlas plugin-store installation action SHALL provision and verify the matching runtime, ProjectAtlas skill, and MCP registration. The user SHALL NOT need to download a binary manually, edit `PATH`, write MCP JSON/TOML, select a runtime version, or wire a database/config path. The first project workflow SHALL use ordinary `atlas_init` and atlas-first calls. Existing CLI installation SHALL remain compatible but SHALL not be required for the store path.

Before promising this path, Phase 0 SHALL evaluate the documented official store/host lifecycle surface and record any missing capability as a release blocker. After the managed lifecycle exists, the release gates SHALL prove the one-action path through real clean-host package/platform evidence. Install, update, repair, rollback, remove, and reinstall SHALL have one typed managed-lifecycle owner for plan, managed-artifact journal, apply, verification, compensating rollback, last-known-good recovery, and preservation rules for project-local databases, purposes, settings, and telemetry. Platform shell/PowerShell entrypoints SHALL remain thin bootstrap/download/launch adapters and SHALL not duplicate lifecycle policy. Cross-filesystem, plugin-manager, and MCP-registry work SHALL NOT be described as atomic or transactional unless the host provides that guarantee. Removal SHALL delete only managed runtime/plugin/config registrations unless the user separately confirms project-data deletion. Clean-host E2E SHALL run on every supported Windows, Linux, and macOS release target without pre-existing ProjectAtlas caches and SHALL assert real scan/query behavior rather than only process exit.

Installation comparison SHALL report explicit user actions, elapsed time, downloaded and installed bytes, first-success rate, repair/rollback success, manual configuration fields, diagnostic quality, and agent calls to the first useful overview. A simpler-installation claim SHALL require non-inferior reliability and a corrected superiority decision on at least one predeclared effort/time metric without regression in security, version correctness, rollback, or project isolation.

#### Scenario: Clean host installs from the plugin store
- **WHEN** a supported host invokes the one official ProjectAtlas store installation action and then opens an indexed or new repository
- **THEN** the matching verified runtime, skill, and MCP registration are ready, `atlas_init` and `atlas_overview` work without manual path/config wiring, and the transcript records actions, time, bytes, and versions

#### Scenario: Update fails after replacing a managed artifact
- **WHEN** digest, launch, registry, or smoke validation fails during update or repair
- **THEN** the prior verified runtime and registration are restored or the installation fails closed with an actionable recovery command, while project-local indexes and authored metadata remain intact

#### Scenario: Plugin is removed
- **WHEN** the user removes ProjectAtlas through the official store lifecycle
- **THEN** only managed runtime/plugin/registration artifacts are removed by default, project-local data is preserved, and reinstall can bind it after version/schema verification

#### Scenario: Installation simplicity is claimed
- **WHEN** the clean-host benchmark has worse reliability, requires hidden manual configuration, or lacks a corrected superiority result on the declared effort/time metric
- **THEN** the simpler-installation claim and v0.4 release gate remain blocked

### Requirement: Superiority And Simplicity Phase Gates
Every implementation phase SHALL publish a compact, machine-backed scoreboard containing delivered capability parity, correctness/coverage, full and incremental time, peak RSS, database/WAL writes, index/package bytes, query latency, agent answer quality, context tokens, file reads, tool calls, production code/dependency growth, and public CLI/MCP surface growth. A phase SHALL NOT close merely because features exist: it SHALL pass its applicable correctness and superiority thresholds, preserve the existing atlas-first workflow, and complete a KISS/DRY/ownership review. An optional capability that does not demonstrate measured agent value greater than its runtime, package, dependency, maintenance, and surface cost SHALL remain disabled, be deferred, or be removed.

#### Scenario: Phase adds features but regresses the agent workflow
- **WHEN** parity checks pass but normal agent tasks require more mandatory calls, more context, or a materially more complex public surface without compensating measured value
- **THEN** the phase remains open and the implementation is simplified, made automatic/optional, or rolled back

#### Scenario: Phase meets its exit gate
- **WHEN** required behavior, correctness, performance, resource, compatibility, and simplicity metrics pass with raw evidence
- **THEN** the one-page scoreboard records the result and identifies the smallest next independently releasable phase

### Requirement: Task-Level Verification Traceability
Every OpenSpec task SHALL have a machine-readable verification record linking task ID, requirement/scenario IDs, affected ownership boundary, changed trust boundaries and failure modes, risk level, risk rationale, and executable evidence. Each record SHALL contain at least one task-specific automated unit-test ID, exact bounded command, asserted behavior, commit SHA, and successful run URL or retained artifact identity. Runtime tasks SHALL use a focused owning-logic unit test; planning, documentation, benchmark-policy, generated-data, or GitHub-only tasks SHALL use a focused unit-level validator test for the promised schema, artifact, drift, reproduction, or policy behavior rather than an `N/A` placeholder. The additional required test layers SHALL be proportional to data-loss/security risk, public compatibility blast radius, concurrency/cancellation complexity, platform sensitivity, algorithmic novelty, and reversibility rather than imposed as placeholder ceremony.

Each implementation phase SHALL first pass its scoped local unit tests and every additional risk-required local gate. The phase checkpoint SHALL then run the applicable commit-bound GitHub Actions unit, integration, real E2E, packaged-smoke, and platform jobs before the phase is declared complete or implementation advances to the next phase. Issue #308 SHALL record the checkpoint commit SHA and successful hosted run URL. A failed, cancelled, skipped, stale, or timed-out required hosted job SHALL keep the phase open. This phase cadence SHALL NOT move repository-wide coverage saturation or the complete source-mutation campaign ahead of stabilized feature behavior.

The separately mapped Rust test-quality change SHALL be a final v0.4 release prerequisite and SHALL provide fail-closed `cargo nextest`, `cargo llvm-cov`, and `cargo mutants` evidence with reviewed machine-readable exclusions, thresholds, shards, timeouts, and rerun rules. It SHALL NOT be a prerequisite to begin or continue repository-intelligence architecture, refactoring, or feature implementation. Every stable behavior SHALL still receive its focused owning-logic unit test and every risk-required integration, CLI/MCP E2E, packaged, platform, property, fuzz, or benchmark layer when implemented. Repository-wide legacy/refactored saturation, final adjusted-coverage closure, the complete source-mutation campaign, and final evidence SHALL run only after architecture, migrations, public contracts, and feature behavior stabilize. Repository mutation sequences and fault-mutation fixtures SHALL NOT count as source mutation-testing evidence. Missing tools, timed-out required shards, untested mutants, unexplained exclusions, and threshold failures SHALL remain failures. Doctests SHALL continue through `cargo test --doc` because nextest does not execute them.

High/critical-risk schema/migration/publication, root/path/filesystem, worker/WASM/native-pack, optional model/vector, federation, network/security, concurrency/cancellation, and public CLI/MCP serialization changes SHALL include focused unit or property tests, cross-boundary integration/fault-injection tests, real CLI/MCP end-to-end compatibility/error tests, and packaged smoke tests on every affected platform. Medium-risk bounded service behavior SHALL include the mandatory task-specific unit test plus affected integration or adapter evidence. Low-risk internal refactors, generated data, documentation, benchmark, or planning tasks MAY omit irrelevant non-unit layers when a reviewed machine-readable rationale identifies the focused deterministic unit-level compile, schema, drift, golden, reproduction, or artifact validator used instead. No task SHALL be marked complete while its task-specific unit test, successful commit-bound run, or other risk-required evidence is absent, skipped, flaky, timing out, stale, inapplicable without rationale, or asserting only process exit instead of the promised behavior.

The release compatibility E2E SHALL execute the actually installed/packaged v0.4 binary over real CLI processes and real stdio MCP transport against project-local SQLite databases populated by indexing real repositories. It SHALL exercise all 41 preserved CLI paths, all 40 current MCP tools, and every added command, tool, mode, or function across successful workflows, failures, hard limits, cancellation, project routing and isolation, freshness, automatic graph behavior, and TOON/JSON compatibility. The smaller release smoke matrix SHALL run from a cold clean state on Windows, Linux, macOS arm64, and macOS x86-64, cover critical workflows and representative failures, use explicit timeouts, and assert returned semantics rather than only exit status. Unit tests, mocks, in-process service calls, or prebuilt fixture databases SHALL NOT substitute for either real-product gate.

GitHub IssueOps SHALL expose or link every task's unit-test ID and successful run evidence, keep local OpenSpec state and mapped GitHub checklist state synchronized, and reject a checked task when evidence or state drifts. One issue SHALL remain the authoritative program checklist; bounded phase execution issues MAY carry detailed evidence when GitHub body limits require them, but a machine-readable ownership map SHALL make every task belong to exactly one execution ledger and SHALL reject duplicates or omissions. A pull request SHALL not be proposed for review while any task in its declared scope is unchecked, lacks current passing evidence, or disagrees between local and GitHub state.

#### Scenario: Runtime task is proposed for completion
- **WHEN** its risk record identifies a high/critical trust boundary but lacks an applicable unit/property, integration/fault, end-to-end, or affected-platform smoke result
- **THEN** task/checklist validation fails and the task remains unchecked

#### Scenario: Non-runtime task has deterministic evidence
- **WHEN** a spec, generated manifest, benchmark, migration plan, documentation, or review task has applicable schema/drift/artifact/reproduction validation and reviewed not-applicable test layers
- **THEN** the verification gate requires and accepts a focused unit-level validator test plus its successful commit-bound run without requiring meaningless runtime integration or packaged tests

#### Scenario: Checked task lacks a visible successful unit-test run
- **WHEN** a local or GitHub checklist marks a task complete but IssueOps cannot resolve its task-specific unit-test ID and successful run for the checked commit
- **THEN** task, PR-review, and release validation fail and the task must remain unchecked

#### Scenario: Local phase gates pass without hosted evidence
- **WHEN** all scoped local checks pass but the commit-bound GitHub Actions phase checkpoint is absent, failed, cancelled, skipped, stale, or timed out
- **THEN** the phase remains open and issue #308 cannot record it as complete until the successful hosted run URL and commit SHA are visible

#### Scenario: Mock coverage substitutes for the installed-product matrix
- **WHEN** compatibility or smoke evidence uses a unit-test binary, mock transport, in-memory store, prebuilt fixture database, or only process exit instead of the installed v0.4 executable, real stdio, project-local SQLite, real indexing, and semantic assertions
- **THEN** the E2E/smoke gate fails even when unit and integration tests pass

#### Scenario: Phase issues split the evidence ledger
- **WHEN** GitHub body limits require task evidence to be rendered across bounded phase issues
- **THEN** the authoritative ownership map proves every OpenSpec task appears in the umbrella checklist, belongs to exactly one phase evidence ledger, and has no duplicate or missing state

#### Scenario: Low-risk task copies the critical test matrix
- **WHEN** a reversible internal change adds unrelated E2E/platform tests only to fill every column while omitting the focused owning assertion
- **THEN** verification review rejects the placeholder evidence and requires the smallest test that proves the actual risk and behavior

#### Scenario: Stable feature behavior lands before repository-wide saturation
- **WHEN** a repository-intelligence slice has a focused owning-logic unit test and every additional risk-required boundary or public-workflow test but the final repository-wide coverage and mutation campaign has not started
- **THEN** implementation may continue with the task unchecked until final evidence closure, and the mapped quality change does not block the next feature slice

#### Scenario: Repository-wide saturation starts before architecture stabilizes
- **WHEN** planned ownership, schema, service, or public-contract refactoring would invalidate broad legacy/refactored coverage or full-mutation evidence
- **THEN** that evidence is ineligible for final v0.4 closure and the repository-wide campaign waits for the stabilized implementation commit

#### Scenario: Test is vacuous or flaky
- **WHEN** a linked test checks only exit status, depends on unpinned external state, flakes across repeated runs, or exceeds its explicit timeout
- **THEN** the task remains incomplete until the test asserts the required semantic result deterministically

### Requirement: Rust Architecture Quality Gate
The final implementation SHALL use a documented acyclic crate dependency graph with one clear owner for each domain contract, extraction stage, storage operation, service/query, adapter, and CLI/MCP surface. Domain and protocol data SHALL use typed structs, enums, newtypes, and validated constructors; closed variants SHALL prefer enums, and open variation SHALL use small object-safe traits or generics only when at least two real implementations, a required test boundary, or measured zero-cost reuse justifies them. Hot paths SHALL prefer static dispatch and compact representations unless benchmarks justify dynamic dispatch. GoF concepts SHALL be adapted through Rust composition and ownership, such as Strategy providers, Adapter parser/model boundaries, Command query/task structs, State enums/typestate, Repository/store facades, and validated Builders, without inheritance, global singletons, service locators, pattern-for-pattern's-sake abstractions, or speculative factories.

Every new or changed active file, module, crate, type or trait, method/function, constant/static, durable variable, command, serialized contract or schema, fixture, and test SHALL use a durable name for its concrete responsibility. Phase, codename, migration-order, temporary, predecessor, and vague catch-all owner names, including `common`, `admin`, `manager`, `helper`, `utils`, `phaseN`, and `scaffold`, SHALL block acceptance unless the architecture evidence records an exact external protocol/algorithm, frozen compatibility contract, versioned release/evidence history, or genuine domain-operation/lifecycle exception. The gate SHALL evaluate ownership and responsibility, not mechanically reject substrings or short local bindings.

Phase exit and v0.4 completion SHALL remove every inaccurate initiative or provisional identity associated with completed behavior, including scaffold/placeholder names and `bootstrap` or `partial` used only to mean unfinished work, or replace it with the final responsibility-named implementation and its behavioral test or validator. A cosmetic rename, exit-only assertion, or provisional test under a final-looking name SHALL NOT count as replacement. Genuine bootstrap operations and typed partial-result or lifecycle states SHALL remain allowed.

The workspace SHALL remain on supported stable Rust with Edition 2024/resolver 3 or the then-current reviewed stable successor, keep `unsafe_code = "forbid"` for every ProjectAtlas-owned crate without an exception path, and pass format, locked all-target/all-feature check, strict Clippy with warnings denied, tests/doctests, and rustdoc with warnings denied. Existing `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`, stdout/stderr, must-use, error-doc, private-doc, large-allocation/type, needless-allocation/clone, and performance lint policies SHALL not be weakened for this program. New dependencies and crates SHALL require a documented ownership/value reason, canonical maintenance/security review, feature/default review, and measured package/compile/runtime cost. When a platform capability requires native or unsafe internals, ProjectAtlas SHALL prefer a maintained safe wrapper crate and keep that dependency behind the smallest supervised boundary.

A generated architecture scorecard SHALL report production and generated source separately, crate/dependency counts and cycles, ProjectAtlas-owned unsafe lines, transitive unsafe/native/FFI/dynamic-library inventory, relevant vulnerability/advisory state, containment strength by platform, duplicated schema/protocol owners, custom storage/query/protocol infrastructure, public tool/config/install steps, warnings, and unresolved independent-review findings. ProjectAtlas SHALL claim a cleaner or more modern implementation only when all hard architecture/safety/quality gates pass, no unfavorable metric is omitted, every added dependency/crate/public surface or production-code increase has a traced capability and measured value, and independent Rust, storage, security, performance, KISS, and agent-workflow reviewers have resolved every blocking finding. Rust safety claims SHALL be limited to the proven ProjectAtlas-owned and dependency boundaries; generated or third-party native code SHALL NOT be described as memory-safe merely because its host is Rust.

#### Scenario: Architecture review finds pattern ceremony
- **WHEN** a new trait, generic, factory, builder, crate, or dynamic-dispatch layer has only one speculative implementation and no measured or test-boundary need
- **THEN** the architecture gate fails until it is simplified or the concrete justification is recorded and tested

#### Scenario: Active owner has a vague or phase-shaped name
- **WHEN** a new or changed active owner is named for a phase, codename, migration sequence, temporary or predecessor state, or vague catch-all responsibility and no allowed documented exception applies
- **THEN** architecture acceptance remains blocked until the owner is renamed for its concrete responsibility and every affected contract and test uses that durable ownership

#### Scenario: Completed behavior retains provisional evidence
- **WHEN** completed behavior is still represented by an inaccurate initiative/provisional implementation, test, fixture, validator, or evidence identity, including `bootstrap` or `partial` used only to mean unfinished work rather than a real domain operation or result state
- **THEN** phase exit and v0.4 completion remain blocked until that provisional artifact is removed or replaced by the final behavioral test or validator with substantive assertions

#### Scenario: Hot path uses dynamic or heap-heavy design
- **WHEN** extraction, merge, persistence, ranking, or traversal adds dynamic dispatch, boxing, cloning, JSON conversion, or allocation in a measured hot loop
- **THEN** the change must demonstrate a correctness/ownership need and pass allocation/latency/RSS comparison or be redesigned

#### Scenario: Final Rust gate runs
- **WHEN** any workspace target, feature combination, doctest, rustdoc, or strict Clippy invocation emits a warning/error or requires weakening a listed lint
- **THEN** the release remains blocked until the code is corrected without a broad allowance

#### Scenario: Crate dependency graph is inspected
- **WHEN** the final workspace metadata and architecture ownership map are validated
- **THEN** dependencies are acyclic, core domain crates do not depend on CLI/adapters, optional packs remain at outer boundaries, and every public contract has one documented owner

#### Scenario: Cleaner architecture claim is requested
- **WHEN** any hard gate fails, a scorecard metric is hidden, an abstraction lacks an owner/value justification, or an independent blocking finding remains open
- **THEN** the cleaner/more-modern claim and v0.4 completion remain blocked even when functional tests pass
