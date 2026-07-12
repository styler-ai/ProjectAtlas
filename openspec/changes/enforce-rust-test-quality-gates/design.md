## Context

ProjectAtlas already treats `cargo fmt`, workspace check, Clippy with `-D warnings`, plain workspace tests, stable doctests, rustdoc, custom source lints, and ProjectAtlas scan/lint checks as release-blocking work. The same broad sequence appears in `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `.githooks/pre-push`, and `docs/workflow.md`, with E2E assertions in `projectatlas-cli` protecting workflow policy. The current suite does not independently prove nextest execution, source coverage, or source mutation strength, and it does not retain machine-readable evidence that a release can bind to one commit.

The pinned local all-feature observation used to start this change contains 286 runnable non-doctests and zero ignored tests across nine suites; 24,130 of 27,495 covered lines (87.76%, 3,365 missed), 34,041 of 40,094 covered regions (84.90%), and 2,045 of 2,369 covered functions (86.32%). An earlier `cargo-mutants` audit with the native default call skip contained 4,911 candidates: 2,189 in `projectatlas-cli`, 951 in `projectatlas-db`, 587 in `projectatlas-service`, 570 in `projectatlas-symbols`, 441 in `projectatlas-core`, 97 in `projectatlas-fs`, and 76 in `projectatlas-lints`. Disabling that hidden default produces the current unfiltered 4,931-row observation: 2,205 CLI, 955 database, and the same counts for the other crates, a source/config-explained drift of 20. These local observations are neither hosted platform floors nor claims of complete testing or mutation strength. The audited tools are `cargo-nextest` 0.9.140, `cargo-llvm-cov` 0.8.7, and `cargo-mutants` 27.1.0.

This change affects developer tooling and hosted verification, not ProjectAtlas runtime behavior or MCP/CLI contracts. It must work on every supported release operating system, fail closed when evidence is absent or stale, keep `unsafe_code = "forbid"` and the existing strict Rust gates, and remain practical enough for pull requests. Full mutation testing is therefore a scheduled, manually dispatchable, and release-bound sharded operation rather than a serial step on every edit. Its repository-wide implementation and evidence campaign follows stabilization of the repository-intelligence architecture and features so broad test work is not invalidated by planned refactoring; focused tests for each new stable behavior remain mandatory when that behavior lands.

## Goals / Non-Goals

**Goals:**

- Give nextest, stable doctests, LLVM source coverage, and source mutation testing separate blocking conclusions and separate retained evidence.
- Pin tool versions and declare explicit command, test, mutant, shard, and job timeouts so a missing tool, hung command, empty selection, or incomplete run cannot pass.
- Establish per-platform line, region, and function floors from measured evidence and raise them monotonically toward near-complete coverage of applicable ProjectAtlas-owned Rust.
- Enforce the agreed v0.4 hard targets of 98% raw/100% adjusted lines, 95% raw/98% adjusted regions, 98% raw/100% adjusted functions, and 90% raw/95% adjusted viable mutation kills.
- Require narrowly scoped, reviewed, expiring coverage and mutation exceptions instead of hiding files, crates, or generated outcomes.
- Require zero missed viable mutants, zero timed-out mutants, and complete disposition for Rust changed by a pull request.
- Reconcile a deterministic 16-shard full mutation run against one master inventory and ratchet the measured viable kill rate without fabricating an initial score.
- Keep local, CI, release, documentation, hook, and workflow-policy E2E contracts synchronized.
- Saturate legacy and refactored repository code, close the agreed adjusted-coverage targets, and run the complete mutation/evidence campaign only against the stabilized repository-intelligence architecture.

**Non-Goals:**

- Claiming that the current suite has 100% coverage, near-100% mutation strength, or no possible bugs.
- Requiring all platforms to share one percentage or treating the measured local snapshot as an unmeasured Linux or macOS baseline.
- Adding Tarpaulin, unstable LLVM doctest instrumentation, a runtime dependency, a new production crate, or a reusable abstraction hierarchy.
- Replacing existing repository mutation/fault fixtures with source mutation testing, or counting those fixtures as `cargo-mutants` evidence.
- Making retries, ignored failures, blanket exclusions, or manually edited result summaries a route to green.
- Delaying focused tests for new stable behavior until the repository-wide coverage campaign.

## Decisions

### 1. Use each native tool for one explicit quality claim

The non-doctest gate will run pinned `cargo-nextest` with a repository-owned `.config/nextest.toml`. Doctests will remain a separate stable `cargo test --doc --workspace --all-features --locked` gate because nextest does not execute doctests and `cargo-llvm-cov` doctest support is unstable. Coverage will use pinned `cargo-llvm-cov` plus the matching `llvm-tools-preview` component and export LLVM JSON and human-readable reports. Source mutation will use pinned `cargo-mutants`, its conventional `.cargo/mutants.toml`, native diff filtering, deterministic ordering, and native `--shard N/16` selection.

All four commands remain independently blocking. Coverage may execute instrumented tests through the supported LLVM coverage runner, but its conclusion is coverage evidence, not a substitute for the uninstrumented nextest conclusion. Tarpaulin is excluded because it would duplicate the LLVM source-coverage claim without filling a missing quality dimension.

Alternative considered: keep `cargo test` as the only test gate and infer coverage or mutation strength from its exit code. Rejected because one successful command cannot prove the other dimensions and nextest does not cover doctests.

### 2. Compose native configuration with one small repository policy

Native runner behavior stays in `.config/nextest.toml` and a base `.cargo/mutants.toml`. The base mutants config owns deterministic execution/timeouts and contains no reviewed quality-policy exclusions because cargo-mutants applies them before `--list --json`. A single checked-in test-quality policy file will own only cross-tool facts: exact tool versions, supported platform identities, applicable source scope, integer coverage counts and ratios, monotonic floors and targets, exception records, timeout ceilings, expected inventory metadata, mutation floor and agreed target, and evidence freshness/binding rules. Each exception must identify the exact path and range or stable mutant selector, category, rationale, owner, tracking issue, approval date, and expiry date or release. Blanket crate and workspace exclusions are invalid.

The policy will record both raw totals and adjusted applicable totals. Raw results remain visible; an exception changes only the declared applicable denominator and never deletes source rows from retained reports. Ratchets compare covered and total counts before deriving percentages, avoiding floating-point rounding as the source of pass/fail decisions.

Alternative considered: embed all policy in YAML expressions and shell fragments. Rejected because three operating systems, three coverage dimensions, exception expiry, and 16-shard reconciliation need one parseable contract to prevent drift.

### 3. Extend the existing lint binary only for typed aggregation

The implementation will first use the native tools' exit codes and schemas. Where native commands cannot validate cross-platform coverage policy, exception lifecycle, evidence identity, or mutation-shard reconciliation, `projectatlas-lints` will gain the smallest necessary test-quality validation subcommands using concrete deserialized structs and closed enums. It will not run tests, mutate source, own CI orchestration, or become a general task framework. It will consume policy plus native JSON/result files and emit a deterministic summary and nonzero status.

This is a closed repository policy domain, so concrete types and exhaustive `match` are preferred. No trait, generic framework, builder, actor, shared mutable state, or new crate is justified. Typed errors retain file, field, platform, shard, and source evidence. Existing workspace `serde`, `serde_json`, and `toml` crates may be reused if native parsing is required.

Alternative considered: add a dedicated quality-gate crate. Rejected because the existing cargo-adjacent lint binary already owns repository policy checks, and a new production boundary would add packaging and maintenance without runtime value.

### 4. Bind every result to its exact inputs

Each retained evidence manifest will identify the canonical `styler-ai/ProjectAtlas` repository, commit SHA, target OS and architecture, Rust and LLVM versions, pinned tool version, `Cargo.lock` digest, policy digest, relevant configuration digests, command/profile, source scope, start/completion timestamps, timeout settings, raw artifact digests, and final typed result/status. A validator will reject wrong-repository, wrong-commit, wrong-platform, wrong-tool, stale-policy, partial, truncated, manually inconsistent, or missing evidence.

GitHub Actions will upload JUnit, LLVM JSON/LCOV or HTML summaries, native mutation output, master/shard inventories, normalized manifests, and concise job summaries with `if: always()` so failures remain diagnosable. Uploading failure evidence does not change the blocking conclusion.

Alternative considered: rely only on GitHub check names. Rejected because a green check alone cannot prove which source scope, tool version, policy, or inventory it evaluated.

### 5. Keep coverage ratchets platform-specific and monotonic

Coverage runs independently on each supported release OS and records separate line, region, and function counts/floors. The pinned local 24,130/27,495-line, 34,041/40,094-region, and 2,045/2,369-function observation may initialize only its identified Windows host after trusted retention makes it eligible. Linux and macOS floors must come from retained runs on those platforms before their blocking gates can be declared established; placeholders or copied percentages fail validation.

No pull request may lower a floor, reduce applicable source scope, silently increase exclusions, or relabel uncovered source as non-applicable. A floor may rise only to a value demonstrated by retained passing evidence for the same platform and policy scope. The agreed hard v0.4 targets are 98% raw/100% adjusted lines, 95% raw/98% adjusted regions, 98% raw/100% adjusted functions, and 90% raw/95% adjusted viable mutation kills. An unmet target remains visible with a tracking issue and keeps CI, review, and release blocked; an exception may define an honest adjusted applicability denominator but cannot waive the agreed numeric target.

Alternative considered: pool platform results into one workspace percentage. Rejected because conditional compilation and platform adapters produce genuinely different executable source sets.

### 6. Treat pull-request and full mutation runs as different bounded gates

For a pull request, the workflow resolves and records the trusted merge base, asks `cargo-mutants` for candidates in the Rust diff, and verifies the selected set against changed applicable production lines. The gate requires a successful unmutated baseline, complete candidate disposition, zero viable `missed` outcomes, zero `timeout` outcomes, no untested candidates, and valid non-expired exclusions. An empty candidate set passes only when the retained diff/scope evidence proves there was no eligible mutant; tool or diff failure is not empty success.

For the full gate, pinned cargo-mutants first creates an unfiltered raw master inventory using an explicit no-policy-exclusion configuration. The validator applies only exact current policy exceptions to derive a filtered execution inventory and a disjoint excluded inventory, retaining both identities and the generated execution config/filter arguments. Native deterministic `N/16` shards run over the filtered execution inventory against the same commit, policy, tool, and base/execution configuration identities. Aggregation requires `executed candidates union exact policy-excluded candidates == raw master`, with an empty intersection and every raw candidate disposed exactly once. It fails for a missing shard, duplicate or omitted candidate, foreign candidate, inconsistent metadata, command timeout, incomplete result, or unexplained exclusion. The raw and adjusted viable kill rates are both reported; exclusions and unviable candidates never improve the raw metric. The initial full score is set only from a complete retained 16-shard run, then becomes a monotonic floor, while the separately agreed v0.4 target remains a hard CI/release condition.

Alternative considered: require the complete current mutation inventory serially on every pull request. Rejected because it would make normal feedback impractical without improving changed-code enforcement; the complete run remains mandatory for scheduled/manual verification and release evidence.

### 7. Make timeouts and reruns fail closed

Every quality job and long-running step has an explicit GitHub timeout. Nextest has per-test slow/timeout policy; mutation has build/test command ceilings plus outer shard/job ceilings. The normalized manifest distinguishes a test failure, baseline failure, mutant timeout, job timeout, cancellation, missing tool, no tests, no mutants, incomplete output, and infrastructure failure. No required status is converted into success, and a retry cannot erase the failed attempt; an intentional rerun produces a new evidence set whose identity and reason remain visible.

Alternative considered: use GitHub's default six-hour job timeout and rerun flaky jobs until green. Rejected because it hides hangs and destroys the evidence needed to improve deterministic tests.

### 8. Reuse existing workflow and policy-test ownership

CI will expose independent job names for nextest, doctests, per-platform coverage, and changed-source mutation. A dedicated full-mutation workflow may provide the scheduled/manual/release-callable 16-shard matrix because that lifecycle differs from ordinary PR CI. Release verification will require evidence bound to the exact release commit before packaging proceeds. `.githooks/pre-push` and `docs/workflow.md` will expose the bounded local equivalents and make the expensive full run explicit rather than surprising every push.

Existing `projectatlas-cli` workflow-policy E2E tests will assert pinned versions, independent blocking jobs, explicit timeouts, stable doctests, artifact upload-on-failure, platform coverage entries, mutation shard count/reconciliation, release dependencies, hook commands, and documentation parity. Existing format, workspace check, strict Clippy, source lint, dependency, rustdoc, scan, and purpose/lint gates remain required with no warnings.

Alternative considered: introduce a new orchestration service or custom CI DSL. Rejected because GitHub job composition, native Cargo tools, one policy file, and the existing repository-policy test surface cover the requirement.

### 9. Make task completion evidence-bearing and unambiguous

Every OpenSpec checkbox will carry at least one stable, task-specific unit-test identifier. A machine-readable task-evidence ledger will bind each identifier to the exact successful command/assertion, tested implementation commit, normalized digest of every task-owned covered input, platform where relevant, timestamp, and retained local or hosted run. After that run passes, one metadata-only closure commit may change only the task checkbox, evidence pointer/ledger metadata, and mapped GitHub state. The validator recomputes the covered-input digest and accepts the closure commit only when no covered implementation, test, policy, configuration, documentation, or generated artifact changed; any covered-input change requires a new run. A task may change to complete only after its identifiers have successful current evidence and the authoritative GitHub checkbox state matches `tasks.md`. Check-in and PR policy will fail missing identifiers, missing or stale passing runs, duplicate identifiers, orphan evidence, premature completion, covered-input drift, or local/GitHub drift.

For production logic, the identifier names the focused Rust unit test. For workflow/configuration composition, it names a small parser or policy assertion over the artifact. For planning, documentation, and benchmark-policy work, it names an automated unit-level validator assertion over required structure and claims; no task invents a production-code test where no production logic exists. Integration, E2E, smoke, coverage, mutation, or benchmark evidence may be required in addition to this unit-level record but cannot replace it.

`tasks.md` remains the local task definition. The mapped primary GitHub issue remains the checklist index and authority. If the exact mirrored checklist plus issue specification would exceed GitHub's 65,536-character body limit, deterministic phase issues may own disjoint checklist ranges; the primary issue must map every range, and each local task must have exactly one authoritative remote checkbox. Duplicating a checkbox as authoritative in two issues is invalid.

When the primary checklist fits, IssueOps keeps its body authoritative and renders one idempotent managed evidence comment per top-level OpenSpec section. Each comment uses a non-checkbox table with task ID, unit-test ID, escaped assertion, bounded argument-vector command, tested commit, covered-input digest, derived Actions run/artifact link, and validated status. The renderer updates only an exact versioned marker and never trusts or executes a command, URL, success string, or Markdown supplied through issue text or a pull-request artifact. Commands in the versioned plan are argument arrays, not shell strings.

Pull-request validation resolves the issues explicitly linked by the PR and the disjoint task ranges those issues own. It requires every task declared in that PR scope to be checked with current evidence and rejects an unlinked change, an ambiguous multi-issue scope, reordered/duplicate/extra remote tasks, or a PR that changes artifacts outside its declared task ownership. It does not require unrelated v0.4 issues in the same milestone to be complete. Release validation retains the stronger full-milestone rule and requires every authoritative issue/range and its evidence to be complete before packaging.

Read-only PR CI runs untrusted branch tests with only `contents`, `issues`, `pull-requests`, and `actions` read access. A separate `workflow_run` renderer checks out trusted default-branch code, validates the source repository/event/head SHA/run attempt/job/artifact/digests/conclusion, and receives only the narrow `issues: write` permission needed to update managed comments. It never executes PR code under write credentials, does not use `pull_request_target`, and ignores fork writeback unless a maintainer-authorized trusted run satisfies the same checks. The required check blocks ready-for-review policy and merge; it does not claim GitHub can prevent a person from clicking the review button or automatically rewrite draft state without broader permission.

Alternative considered: accept a passing aggregate CI run as evidence for every task. Rejected because it cannot prove which assertion covers a specific task and allows unrelated green tests to close unverified work.

### 10. Sequence repository-wide saturation after product stabilization

The repository-intelligence change owns architecture, migrations, public contracts, functionality, and the focused tests required by each stable behavior. This change begins its repository-wide implementation only after those boundaries stabilize and no planned broad refactor would invalidate a saturation pass. The existing format, check, strict Clippy, tests, doctests, rustdoc, source lints, and behavior-specific tests continue to run during feature work.

On the stabilized implementation commit, this change first audits legacy and refactored ProjectAtlas-owned Rust, adds the smallest meaningful unit/integration/E2E cases needed for uncovered behavior, and then runs final nextest/doctest, platform coverage, and source-mutation evidence. The complete 16-shard campaign and final task-evidence reconciliation bind to that stabilized source and test scope. This change remains a hard v0.4 release prerequisite; it is not a prerequisite to start repository-intelligence implementation.

Alternative considered: saturate the pre-refactor repository before repository-intelligence work. Rejected because ownership splits, migrations, and service changes would discard or rewrite a material portion of that test effort. This sequencing does not permit untested feature work because focused tests stay coupled to each stable behavior.

## Risks / Trade-offs

- [Coverage differs across toolchain or operating-system updates] -> Keep per-platform count-based baselines bound to exact versions and require retained evidence before raising a floor or accepting a new baseline.
- [A nominal 100% target encourages meaningless tests or broad exclusions] -> Report raw and adjusted metrics, require mutation evidence, forbid blanket exclusions, and keep every gap tied to an expiring reviewed record.
- [Full mutation consumes substantial hosted time] -> Use exactly 16 deterministic native shards, explicit ceilings, scheduled/manual execution, and changed-diff enforcement for ordinary pull requests.
- [Mutant identities drift after source or tool changes] -> Regenerate one master inventory for the exact commit/tool/config and reject shard results that do not reconcile to it.
- [Conditional code is absent from one runner] -> Require distinct platform evidence and do not let another platform's percentage satisfy the missing gate.
- [A custom validator grows into a framework] -> Keep it inside `projectatlas-lints`, accept only policy/native evidence inputs, use concrete types, and delete code that native tools can enforce directly.
- [Artifact upload can mask a failed command] -> Run uploads with `if: always()` while preserving the original blocking step and final job conclusion.
- [Pre-push becomes too slow to use] -> Keep nextest, doctests, and policy validation local by default; document coverage and changed-mutation commands explicitly, and reserve the full 16-shard run for intentional verification.
- [Task evidence becomes checkbox bureaucracy] -> Use one stable identifier and one compact generated ledger row per task, reuse existing tests when they assert the exact task contract, and add phase issues only when the GitHub body limit requires them.

## Migration Plan

1. Complete and stabilize the repository-intelligence architecture, migrations, public contracts, functionality, and their focused risk-based tests; resolve planned broad refactors before taking an eligible quality-closure baseline.
2. Check in native nextest/mutants configuration and the typed policy schema with exact audited tool pins, retaining the earlier local observation as historical pre-stabilization provenance and capturing the stabilized source/test scope separately.
3. Add validator unit and fixture tests for parsing, monotonic comparison, expiry, evidence binding, empty selections, native result classification, and 16-shard reconciliation before wiring workflows to it.
4. Add separate nextest, doctest, per-platform coverage, and changed-mutation jobs with bounded commands and always-uploaded evidence; capture real platform baselines against the stabilized commit rather than copying historical local values.
5. Audit legacy/refactored code and add the smallest meaningful focused unit, integration, and E2E tests required to close the agreed adjusted-coverage targets without broad exclusions or vacuous assertions.
6. Run the complete deterministic 16-shard mutation campaign only after source and test saturation stabilizes, retain all raw outputs, reconcile the exact current raw master, explain audited inventory drift, and set the first viable kill-rate floor only from passing evidence.
7. Add task-specific test identifiers, the task-evidence ledger validator, and IssueOps/check-in/PR checks before marking any implementation task complete; mirror authoritative checkbox state locally and on GitHub.
8. Update release prerequisites, pre-push commands, workflow documentation, and E2E policy assertions in the same change so no supported entry point describes a weaker contract.
9. Enable required branch checks only after the corresponding evidence-producing workflow is present and green. Rollback may disable a newly required branch-check name while preserving artifacts for diagnosis; it must not lower committed floors, delete failures, or silently expand exclusions.

## Resolved Agreements

- The hard v0.4 targets are 98% raw/100% adjusted line coverage, 95% raw/98% adjusted region coverage, 98% raw/100% adjusted function coverage, and 90% raw/95% adjusted viable mutation kills. Historical floors remain separate monotonic measurements and cannot weaken these targets.
- Required evidence is retained for 90 days, which exceeds the declared 30-day release-decision window. Expired or unavailable artifacts are ineligible even when an old manifest says they passed.
