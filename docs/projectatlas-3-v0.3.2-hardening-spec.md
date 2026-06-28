# ProjectAtlas 3 v0.3.2 Architecture Hardening Specification

## Status

Accepted for implementation.

## Background

ProjectAtlas v0.3.1 is published and verified end to end. The remaining review
items are not runtime blockers, but they are worth closing before more parser
features accumulate:

- language-specific symbol extraction is concentrated in
  `crates/projectatlas-symbols/src/lib.rs`
- Windows extended-path normalization exists in both CLI runtime and DB metadata
  code
- cross-file `called_by` matching is intentionally conservative and should gain
  parser-specific import-alias resolution where it can stay deterministic

This specification turns those review notes into a release-grade v0.3.2
hardening pass.

## Goals

1. Split language-specific symbol extraction into maintainable strategy modules
   without changing the public `projectatlas-symbols` API.
2. Centralize native display/metadata path normalization in `projectatlas-core`
   so CLI and DB behavior cannot drift.
3. Improve parser-specific import/call resolution for deterministic cross-file
   `called_by` summaries while preserving the existing false-positive-averse
   policy.
4. Keep ProjectAtlas agent-first behavior stable: overview, folders, files,
   summary, symbols, search, slices, health, token telemetry, MCP, plugin, and
   release gates must remain green.

## Non-Goals

- Do not change CLI or MCP command names.
- Do not replace SQLite, `rmcp`, `toon-format`, `globset`, `ignore`, `notify`,
  `rayon`, or tree-sitter.
- Do not introduce Python or any non-Rust implementation path.
- Do not auto-approve generated purposes.
- Do not attach `called_by` rows when import/call resolution is ambiguous.

## Workstream A: Symbol Extractor Module Boundary

### Problem

`projectatlas-symbols/src/lib.rs` currently mixes public API, Cargo manifest
extraction, generic tree-sitter traversal, language augmenters, fallback parsing,
and tests. The file is functional, but continued language additions would make it
hard to reason about and would hide performance risks such as linear duplicate
lookups inside language-specific normalization.

### Required Design

Keep the public crate surface stable:

- `extract_symbol_graph`
- `has_specialized_parser`
- `specialized_languages`

Move implementation behind internal modules:

- `cargo_manifest`: Cargo.toml and Cargo.lock package/dependency extraction
- `tree_sitter`: generic tree-sitter dispatch and traversal
- `languages`: language-specific augmenters
  - `kotlin`
  - `objective_c`
  - `zig`
  - `c_family`
  - room for `rust`, `python`, `typescript`, and other future language-specific
    strategies
- `fallback`: regex/fallback extraction for unsupported file families

The split should be behavior-preserving except where this spec explicitly calls
for better deterministic import/call resolution.

### Phasing

Implement the module split in low-risk phases. The first implementation pass
should move the language augmentation layer out of `lib.rs` while keeping the
generic tree-sitter parser spine stable. The generic traversal, declaration
classification, import detection, call detection, parent detection, and node
name helpers are cross-language behavior; moving them is allowed only after the
language-specific split is green and tests show no broad parser drift.

### Objective-C Deduplication

The Objective-C duplicate normalization must move away from repeated linear
searches over the output vector. Use keyed indexes or maps for:

- class/interface identity by path and class name
- method identity by path, parent, selector name, and kind

Implementation entries with bodies should continue to win over interface
declarations when both exist.

### Acceptance Criteria

- Existing symbol extraction tests pass unchanged or with equivalent assertions.
- The Objective-C fixture still prefers implementation-body methods.
- C++ method declarations and inline methods remain indexed as class members.
- No public API break is introduced for the symbols crate.
- `projectatlas summary crates/projectatlas-symbols/src/lib.rs --limit 20`
  should become easier to read because the file no longer owns every language
  policy function.

## Workstream B: Shared Path Display Normalization

### Problem

The CLI and DB currently each normalize Windows extended path prefixes such as
`\\?\C:\...` and `\\?\UNC\...`. This is small duplication, but it sits on a
cross-platform boundary and should not drift.

### Required Design

Add a documented core helper in `projectatlas-core`, for example:

```rust
pub fn normalize_native_path_display(path: impl AsRef<Path>) -> String
pub fn normalize_native_path_display_str(path: &str) -> String
```

The helper must:

- replace host separators with `/`
- remove `\\?\` / `//?/` prefixes
- convert `\\?\UNC\server\share` to `//server/share`
- preserve ordinary POSIX paths
- preserve ordinary relative paths

Then update:

- DB project-root metadata persistence
- CLI settings and path diagnostics
- MCP config/runtime diagnostics if applicable

### Acceptance Criteria

- One canonical implementation exists in `projectatlas-core`.
- CLI and DB tests use the shared helper.
- Windows extended path cases are covered in core tests.
- ProjectAtlas settings continue to report slash-normalized paths.

## Workstream C: Parser-Specific Import/Call Resolution

### Problem

`called_by` currently matches exact function names and path-derived module
aliases. That is intentionally conservative, but it misses deterministic cases
where a parser has enough import information to resolve a call target.

### Required Design

Add a service-layer import-alias resolver that consumes persisted symbols and
relations. It should not require reparsing live source during summary requests.

Minimum deterministic cases for v0.3.2:

- Rust:
  - `use crate::foo::service; service::run();`
  - `use crate::foo::service as foo_service; foo_service::run();`
  - `use crate::foo::service::run as run_service; run_service();`
- TypeScript/JavaScript:
  - `import { run } from "./service"; run();`
  - `import { run as serviceRun } from "./service"; serviceRun();`
  - `import * as service from "./service"; service.run();`
- Python:
  - `from package.module import run; run()`
  - `from package.module import run as service_run; service_run()`
  - `import package.module as service; service.run()`

Only attach a `called_by` row when the import target resolves to exactly one
indexed file and one symbol candidate. If the resolver cannot prove uniqueness,
it must leave `called_by` empty rather than guessing.

The safest first implementation is to improve parser-emitted call targets rather
than loosening service matching. For Rust, collect simple `use crate::...`
bindings during symbol extraction and emit an additional qualified call relation
when a bare or alias call resolves unambiguously. The service resolver should
continue to reject suffix-only or duplicate-alias matches.

### Source of Truth

Use persisted index data:

- import/module symbols emitted during scan
- call relations emitted during scan
- indexed file paths and languages from SQLite

If current import symbols are too coarse for one target language, improve the
symbol extractor for that language and cover it with tests.

### Acceptance Criteria

- Existing ambiguity tests continue to pass.
- New service tests prove that ambiguous same-name imports do not attach
  `called_by`.
- New unit or E2E tests prove deterministic Rust, TypeScript, and Python
  import-alias `called_by` cases.
- File summary output remains bounded and does not load the whole repository
  into memory.

## Workstream D: Documentation, Memory, and Release Discipline

### Required Updates

- Update `docs/projectatlas-3-architecture.md` with the new module boundary and
  import-alias resolution design.
- Update `.codex/rules/memory/*` for current status and implementation notes.
- Regenerate `.projectatlas/projectatlas.toon`.
- Keep `AGENTS.md` and plugin skill guidance aligned only if the command
  workflow changes. No workflow change is expected.

### Verification Gates

Local gates before PR:

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo test --doc --workspace --all-features --locked
$env:RUSTDOCFLAGS="-D warnings"; cargo doc --workspace --no-deps --all-features --locked
cargo deny check
cargo hack check --workspace --each-feature --no-dev-deps
cargo hack check --workspace --all-targets --each-feature
projectatlas scan .
projectatlas lint --strict-folders --report-untracked
projectatlas parity report --profile repository-intelligence
projectatlas token --view tui
git diff --check
```

Hosted gates before calling the goal complete:

- PR CI green
- release workflow green if the workspace version is bumped and v0.3.2 is
  published
- final correctness reviewer subagent
- final architecture/crate-choice reviewer subagent

## Rollout Plan

1. Centralize path display normalization first because it is small and easy to
   verify.
2. Split the `projectatlas-symbols` language augmentation cluster into modules
   without changing generic parser behavior.
3. Replace Objective-C duplicate normalization with keyed lookup.
4. Add parser-specific import/call resolver tests, starting with Rust because it
   can feed the existing conservative resolver through better qualified
   relation targets.
5. Implement deterministic import-alias resolution behind service-layer helpers.
6. Regenerate atlas map and update Memory Bank.
7. Run strict local gates.
8. Run final subagent reviews and fix valid findings.
9. Open a PR. Publish v0.3.2 only if the implementation changes are intended as
   a public release rather than a main-only hardening change.

## Risks

- Moving parser code can create subtle line-range or signature drift. The
  existing symbol fixture tests must stay strict.
- Import-alias resolution can create false positives if it relies on suffix
  matching. It must require unique indexed targets.
- Over-modularizing the symbols crate can add ceremony. Keep module boundaries
  functional and language-oriented, not framework-heavy.
