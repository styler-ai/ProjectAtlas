# External source-policy costs and release handoff

The derivation-25 policy repair adds 432,208 bytes (0.62%) to the optimized
default CLI relative to accepted main at derivation 24. Both retain semantic
contract 23. Exact binary identities, source-manifest identity, sample metrics
and fixture fingerprints are in [the compact result](external-policy-cost.json).

These are local affected-work measurements on Linux, Rust 1.98.0 and a Ryzen 7
5700U. They do not replace issue #708's combined performance gates or issue
#715's installed four-platform acceptance. No general speedup is claimed.

## Method and costs

Build both sources with `cargo build --locked -p projectatlas-cli --release`
using default features. Link the same small native Rust driver against each
build's filesystem/core libraries with `rustc -C opt-level=3`. In each fresh
process, use the public `RootScanPolicy` owner to measure 100 constructions,
five complete `scan_repo` traversals and 1,000 `excludes_path` calls on one
constructed policy. Use `std::hint::black_box`, time each group with `Instant`
and divide by its iteration count. Record process maximum RSS with GNU time.

Each fixture contains 512 numbered Rust files and `kept.rs`. A selected external
Git config points to `rules.ignore`, whose first rule is `excluded.rs`; its path
has no spaces so the cost comparison does not depend on the quoted-path defect.
The large-config variant adds 20,000 comment lines (about 480 KB); the many-rules
variant adds 10,000 absent-file rules. Neither variant changes the admitted
fixture inventory. System config is disabled and global/XDG selection is explicit.
Three trials alternate baseline/candidate order on the same saved fixture;
ordinary filesystem/page caches remain warm. Reported values are medians.
All measured ignore patterns are nonmatching for the authored source files;
these timings characterize policy overhead rather than positive-match scan cost.
Positive matching and exclusion correctness use the separate CLI/MCP regressions.

| Fixture | Construction, main → repair | Full scan, main → repair | Targeted check, main → repair | Process RSS KiB, main → repair |
| --- | ---: | ---: | ---: | ---: |
| Ordinary | 9.0 → 59.3 µs | 4.95 → 4.96 ms | 24.2 → 75.0 µs | 6,188 → 5,952 |
| Large config | 9.0 → 850.0 µs | 5.46 → 6.12 ms | 72.1 → 76.2 µs | 6,548 → 9,228 |
| 10,000 rules | 9.0 µs → 16.25 ms | 24.38 → 24.09 ms | 17.83 → 2.47 ms | 20,548 → 20,140 |

Construction now performs bounded canonical configuration parsing and compiles
external rules once. The old construction deferred work to later operations,
so construction alone is not an equivalent complete-work comparison. The repair
adds ordinary targeted-check overhead while reducing repeated large-rule work.
RSS covers the complete micro-process, including all three measured groups;
it does not attribute allocations to an individual operation. Three warm trials
do not establish cold-storage behavior, a scaling bound or a performance SLO.

## Complete CLI refresh costs

Final publication revalidates staged absent-document outcomes against freshly
selected policy. A pass-local native matcher amortizes hierarchical policy
loading across distinct targets, while the next pass and final revalidation
remain fresh. The CLI and filesystem helper were both rebuilt from the recorded
source manifest; all 36 component/CLI samples and four upgrade cases were rerun.

The controlled watcher retains full reconciliation under its original control.
Its native regression exercises a large single-file graph closure, compaction
and root replacement; these fresh-process timings do not isolate that recovery
branch. Capturing the database target adds one canonicalization per observed
request, including warm requests. Callback filtering performs no I/O. The
persistent MCP regressions separately prove document-only invalidation, typed
read-only guidance and explicit zero-reparse reconciliation. These CLI samples
do not isolate persistent MCP warm-request latency. Component costs use canonical
roots; alias and relative-root regressions do not establish separate timings.

An additional 18 fresh-root initialization samples compare accepted main with
the final artifact, followed by separately timed unchanged `watch --once` calls.
Each fixture adds `.gitignore` and a README link to one absent `missing.rs` target
beside the 513 Rust files; public relation queries confirm the `missing` reason.
All following unchanged refreshes report zero parsed sources. Source-file and
symbol counts match across each baseline/candidate fixture. Order, three trials,
cache state and RSS method match the component protocol.

| Fixture | Init ms, main → repair | Unchanged refresh ms, main → repair | Init RSS KiB, main → repair | Refresh RSS KiB, main → repair |
| --- | ---: | ---: | ---: | ---: |
| Ordinary | 312.3 → 309.3 | 48.3 → 49.6 | 32,936 → 30,608 | 21,924 → 20,500 |
| Large config | 304.6 → 313.2 | 49.1 → 53.8 | 32,972 → 33,748 | 21,568 → 23,468 |
| 10,000 rules | 389.1 → 393.5 | 72.5 → 96.3 | 44,996 → 42,004 | 32,860 → 33,032 |

The large-rule fresh-process refresh adds about 24 ms in this fixture. Fresh policy
validation is required even when source nodes do not change; retaining the old
compiled matcher allowed stale document reasons to be published. Carry the
measured increase into the combined performance gate rather than hiding it in
the faster targeted-matcher result.

## Cold document-policy verification

Eight Markdown files retain 1,024 distinct unresolved links across four 256-row
pages. Root `.ignore` contains 2,000 nonmatching rules (50,890 bytes). Three cold
CLI `summary guide0.md` reads per binary use distinct freshly initialized roots,
explicitly disabled external Git config and warm filesystem caches. The
predecessor is PR head 634cf78, not accepted main. Every read succeeds and leaves
the complete publication metadata unchanged; persisted unresolved counts are
verified through read-only SQLite queries.

The predecessor median is 4.37718 seconds; the repair median is 0.07073 seconds,
about 61.9 times faster in this fixture. Initialization is 10.21149 to 0.26473
seconds. These samples establish the repeated-policy-work repair, not a general
speedup, a thirty-minute denial experiment or release performance acceptance.
The unisolated desktop host and sequential predecessor/candidate groups limit
causal timing precision. Workspace check and rustdoc overlapped the final FS
component trial above; the cold-document samples completed before that overlap.

One resolution/probe pass admits at most 1,000,000 directory-context loads and
128 MiB of local hierarchical policy input. Reloads after child-cache eviction
are charged again; the warmed root/ancestor seed is shared without reparsing.
Child policy input/native/wrapper keys use soft 256-context / 4 MiB estimates,
with between-target eviction and one current ancestor chain under the cumulative
limits. These are input/key estimates, not exact RSS bounds. Staging and fresh
publication revalidation have separate pass quotas under the original operation
deadline. A failed pass requires a fresh matcher. Quota exhaustion preserves
existing typed PolicyUnavailable guidance and cannot restart a full refresh
through incremental or observed-MCP fallback. Trusted global rules remain
compiled once at policy discovery; external edits require fresh discovery.

## Dependency and boundary review

The repair uses canonical `gix-config` 0.62.2 with default features disabled and
the required `sha1` type feature, plus `ignore` 0.4.34 for hierarchical matching.
Both declare Rust 1.88; the project builds with its pinned Rust 1.98 toolchain.
This is dependency metadata review, not a claim of a complete Rust-1.88 build.
The direct parser licenses are MIT/Apache-2.0 and Unlicense/MIT respectively.
Locked metadata/fetch and current cargo-deny advisory, ban, license and registry
checks pass. Cargo manifests and lockfile remain the dependency authority.

Selected direct config reads are limited to 1 MiB per file and 4 MiB aggregate.
No Git subprocess, implicit include expansion or per-event global parser is
introduced. Existing watcher queue, registration, cancellation and publication
owners retain their bounds. The shared document inspector checks native prefixes
before target-specific policy reads; explicit operator config, database and
shared-worktree policy remain supported. That prerequisite protects both the
cached projection and the new uncached probe; #738 still owns its full acceptance.

## Upgrade and remaining release proof

The exact optimized candidate passes four native upgrade cases: published 0.5.0
and accepted-main derivation 24, each with an ordinary and linked Git worktree.
Each predecessor reproduces incorrect exclusion results; ordinary refresh repairs
them while preserving project identity and every approved purpose, including a
purpose on a newly excluded dormant file. A second refresh preserves the complete
derived/publication snapshot. Malformed config and an early zero deadline retain
last-valid state. A policy-only absent-target change converges with a full scan;
compatible rollback and re-upgrade preserve authored state and corrected results.
The early deadline does not claim mid-publication cancellation; owning controlled
tests separately cover cancellation and publication revalidation.

Actual CLI/MCP external-policy and source-root-replacement tests also pass against
this frozen optimized binary. Their causal controls establish the old failure
and, for replacement coverage, require later child-event delivery without an
unconditional MCP refresh masking a missing watcher. Native platform CI, complete
issue review and exact RC package verification remain required before closure
and release. Keep stable 0.5.0 Latest until separately authorized.
