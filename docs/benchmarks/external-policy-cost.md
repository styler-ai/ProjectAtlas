# External source-policy costs and release handoff

The derivation-26 policy repair adds 538,912 bytes (0.78%) to the optimized
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
source manifest. The rebuilt filesystem helper is byte-identical to the prior
measured helper, so its 18 component samples are reused with their original
manifest and overlap limitation. All 18 CLI cost samples and six upgrade cases
were rerun against the current frozen CLI without overlapping builds.

The controlled watcher retains full reconciliation under its original control.
Its native regression exercises a large single-file graph closure, compaction
and root replacement; these fresh-process timings do not isolate that recovery
branch. Capturing the database target adds one canonicalization per observed
request, including warm requests. Callback filtering performs no I/O. The
policy-event scope index resolves unrelated source events with constant native
namespace work; directory events resolve only affected selected bindings once
across overlapping event paths. The 1,024-input resource regression checks this
bound without claiming a warm-latency result. The persistent MCP regressions
separately prove document-only invalidation, typed
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
| Ordinary | 312.8 → 310.1 | 48.3 → 49.9 | 32,092 → 31,276 | 20,660 → 20,384 |
| Large config | 305.6 → 314.5 | 49.2 → 53.3 | 31,924 → 33,312 | 20,984 → 23,628 |
| 10,000 rules | 392.9 → 394.8 | 69.6 → 97.3 | 44,412 → 41,700 | 32,212 → 32,984 |

The large-rule fresh-process refresh adds about 28 ms in this fixture. Fresh policy
validation is required even when source nodes do not change; retaining the old
compiled matcher allowed stale document reasons to be published. Carry the
measured increase into the combined performance gate rather than hiding it in
the faster targeted-matcher result.

## Cold document-policy verification

Eight Markdown files retain 1,024 distinct unresolved links across four 256-row
pages. Root `.ignore` contains 2,000 nonmatching rules (50,890 bytes). Three fresh
CLI `summary guide0.md` processes per binary read the same freshly initialized
root; each binary has a distinct root. The runs use
explicitly disabled external Git config and warm filesystem caches. The
predecessor is PR head 634cf78, not accepted main. Every read succeeds and leaves
the complete publication metadata unchanged; persisted unresolved counts are
verified through read-only SQLite queries.

The predecessor median is 4.37718 seconds; the current repair median is 0.06877
seconds, about 63.6 times faster in this fixture. Initialization is
10.21149 to 0.27031 seconds. These samples establish the repeated-policy-work repair, not a general
speedup, a thirty-minute denial experiment or release performance acceptance.
The unisolated desktop host and separately measured predecessor/candidate groups
limit causal timing precision. The predecessor samples retain their verified
unchanged binary identity; the current candidate samples ran without overlapping
builds. Workspace check and rustdoc overlapped the final historical FS component
trial above; that limitation still applies to the reused component samples.

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

## Native document-prefix admission

A second fixture uses four Markdown files with 512 distinct missing links beneath
64 existing ordinary directory components. The preceding optimized artifact
repeated full canonicalization for every component. The repair checks the selected
root freshly, inspects each component without following its leaf, and
canonicalizes symlink/reparse components immediately. It refuses the first escape,
including outside-then-reentry paths. There is no cross-target allow-cache.

| Operation | Preceding artifact → derivation-25 prefix repair |
| --- | ---: |
| Initialization | 5.320 → 0.735 s |
| Cold CLI summary | 2.600 → 0.322 s |
| Selected filesystem syscalls in separate tracing | 1,302,683 → 73,883 |
| readlink calls within that subset | 1,300,153 → 5,305 |

These are one quiet initialization/read sample per exact binary on distinct,
identical-input roots, with warm filesystem caches. Both retain all 512 missing
relations and leave publication unchanged. Separate `strace -f -c` runs count only
`readlink`, `readlinkat`, `newfstatat`, `statx` and `openat`; their instrumented
timings are not compared with the quiet timings. This is affected-work evidence,
not a general speedup or persistent-MCP latency claim. Exact identities and sample
data are in the compact result.

The 512-link timing and syscall samples above are retained historical measurements
of the identified derivation-25 artifact. Its CLI bytes differ from the current
candidate; no equivalence or fresh syscall measurement is claimed.

The reported-scale check uses 1,000 links beneath 200 existing components.
Two documents with 500 links each keep extraction complete under the existing
intermediate-byte limit; a single 1,000-link document at that depth correctly
retained only 614 links with explicit partial coverage and is excluded from the
complete comparison. On the complete fixture, initialization is
191.48 → 7.13 s and a cold summary is 95.71 → 3.43 s.
Both retain all 1,000 missing relations with complete document coverage and leave
publication unchanged. These are one-sample, quiet-process, warm-cache local
measurements on distinct identical-input roots, subject to the same host/SLO
limitations above.

The owning 128-target/32-component regression fails before the repair with 4,352
canonicalizations and passes afterward with at most 384. A separate counter
checks metadata plus canonical operations against the actual ledger (at most
8,704 in that test). Replacing an expensive canonicalization with cheap metadata
can increase the count of high-level operations while reducing kernel work.

One resolution/verification index admits at most 4,000,000 target-specific native
prefix/leaf/ancestor metadata and canonical probes, charged before execution.
The quota spans SQL pages, cache hits retain cancellation and terminal-refusal
checks, and a failed pass cannot restart through automatic refresh. A flat missing
target uses four probes; deeper targets spend the same allowance on extra checks.
A probe is an accounting unit, not a constant-time syscall. Fixed root capture and
native ignore-engine work retain their separate existing controls. No graph
schema, public filesystem/core error enum, command or MCP wire field changes.

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

The exact optimized candidate passes six native upgrade cases. Four cover published 0.5.0
and accepted-main derivation 24, each with an ordinary and linked Git worktree.
Each predecessor reproduces incorrect exclusion results; ordinary refresh repairs
them while preserving project identity and every approved purpose, including a
purpose on a newly excluded dormant file. A second refresh preserves the complete
derived/publication snapshot. Malformed config and an early zero deadline retain
last-valid state. A policy-only absent-target change converges with a full scan;
compatible rollback and re-upgrade preserve authored state and corrected results.
The early deadline does not claim mid-publication cancellation; owning controlled
tests separately cover cancellation and publication revalidation.

Two further ordinary/linked-worktree cases start at derivation 25 and prove the
patched Markdown heading changes from `dog.png` to `![[cat.png|dog.png]]`,
with semantic contract 23 retained. They preserve approved purposes, identity and
ordinary/wiki document references, verify no-op and early-deadline state, and
prove compatible rollback and re-upgrade. The locked pulldown-cmark 0.13.6 fixes
reachable RUSTSEC-2026-0433 without disabling supported wikilinks.

Actual CLI/MCP external-policy and source-root-replacement tests also pass against
this frozen optimized binary. Their causal controls establish the old failure
and, for replacement coverage, use a Unicode root and require later child/policy
event delivery without an
unconditional MCP refresh masking a missing watcher. Native platform CI, complete
issue review and exact RC package verification remain required before closure
and release. Keep stable 0.5.0 Latest until separately authorized.
