# #358 graph-construction resource measurement

## Decision

The existing separate-pool indexing lifecycle is retained. The candidate that
reused one Rayon pool for parsing, graph-identity admission, and structural
summaries was removed after comparison because it did not produce a material
full-envelope win. No schema or dependency boundary changed by the historical
candidate decision. The exact-head remediation now adds one production
writer-availability probe before expensive staging; it does not change the
publication schema or staged projection ownership.

The frozen remediation threshold was a reduction of at least 10% in both cold
scan wall time and CPU time for every representative shape, with no more than a
5% regression in any measured resource envelope and no correctness, generation,
cancellation, contention, or cleanup regression. A faster single incremental
rebuild was not sufficient to offset a cold-scan regression.

## Current optimized resource profile

The current profile uses normal telemetry and an optimized Windows build from
source `52bc7b12a8946cc07ad237b298198328280c7964`. Its SHA-256 is
`7734bc465af75157e2ddaeb4c2e32677349285193ccadda8a04a5c63dad132e8`
and its size is 58,554,368 bytes. The preregistration binds that build witness
and the committed harness inputs; the harness rejects mismatched runtime bytes
before measurement. The enclosing checkout revision is recorded separately
and is not treated as compiled-source provenance.

The reproducible commands and unchanged diagnostic thresholds are in
[`v050-358-current-profile-preregistration.json`](v050-358-current-profile-preregistration.json).
The bounded operands, every diagnostic check, graph rows/digests, query plans,
storage peaks, telemetry, and lifecycle results are in
[`v050-358-current-profile-results.json`](v050-358-current-profile-results.json).
These are single-run current observations, not a speedup comparison against
the historical debug builds below.
Their harness inputs remain frozen at source revision
`4342b874e1eae28684fd6dae1b3cb08065148d66`. The later validation that rejects
`--small-variant` or `--caller-files` with `--only all` does not change these
focused measurement paths. Full publication runs must include every small
fixture variant and the preregistered medium cardinality.
Publication identity also requires a nonempty preregistered candidate version;
both the requested compatibility version and the reported runtime version must
match it. An omitted version uses that lock, while an explicit empty version
is rejected. These admission checks do not change the frozen resource profile
or the separate graph digest recomputation below.

The frozen harness omitted relation canonical identities and the resolution-key
associations of exports and dependencies from graph digests.
The results retain those originally measured `graph_digest` values and add
a separate `graph_digest_recomputation` from the unchanged retained databases.
That read-only correction includes relation identities and their typed nested
project fields, including each export/dependency resolution binding. Nested
entity ownership must agree with its relation. This does not rerun or relock
any resource observation. Bounded runs reject nonpositive caller counts; only
an omitted count selects the preregistered default.

| Shape | Cold wall / CPU (s) | Peak RSS / private commit (bytes) | Cold read / write (bytes) | Unchanged watch (s) | Narrow watch (s) | Rebuild (s) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Small clean, 7 files | 0.390582 / 0.125000 | 14,426,112 / 4,190,208 | 675,849 / 3,661,768 | 0.229212 | N/A | N/A |
| Medium, 1,024 callers | 2.339334 / 2.000000 | 40,603,648 / 31,326,208 | 102,072,286 / 450,857,800 | 0.288656 | 1.134229 | 2.199476 |
| Large/high-edge, 4,096 callers | 10.467258 / 10.984375 | 130,793,472 / 126,627,840 | 900,325,430 / 2,185,769,564 | 0.289354 | 1.424903 | 11.839484 |

CPU and I/O use terminal Windows Job accounting for the owned process tree.
RSS and private committed memory use 20 ms samples. Private commit is an
allocation footprint, not allocator event counts. The scan envelope includes
parsing, summaries, graph derivation/admission, staging, SQLite publication,
and cleanup; it does not expose exact individual stage durations. Reported
parser-worker ceilings are 6, 16, and 16. All three cases preserve bounded
output, worker/thread limits, stable query publication, and clean SQLite
quick-check. Each records 26 telemetry rows after the measured MCP queries;
scan and settings alone legitimately record zero.

Final database sizes are 667,648, 19,136,512, and 72,572,928 bytes. Each final
observation has zero WAL and staging bytes, with a 32,768-byte SHM file still
visible at the harness observation boundary. All three representative query
plans use their owning indexes without temporary B-trees. The profile records
WAL/full synchronization and 4,096-byte pages; these read-connection PRAGMAs
are observations, not a new runtime configuration authority.

The small profile passes every diagnostic. Medium retains one failed historical
I/O cell: rebuild reads 147,115,884 bytes against 134,217,728. Large retains
seven failed historical cells: full scan read/write, narrow refresh reads,
expanded-guidance reads, rebuild read/write, and rebuild write amplification.
Their exact operands remain in the results JSON. These unchanged v0.4 caps
were copied as diagnostics, including for the larger synthetic fixture; no
cap was raised and neither failed profile is presented as an all-green run.
The no-adoption decision does not depend on passing those caps.

The same locked runtime and normal environment pass all four lifecycle phases:

| Phase | Observed result |
| --- | --- |
| Two repositories | Eight configured workers per process within a 16-worker host budget; two reported parser workers in aggregate for the narrow edits; conservative summed peak RSS 90,312,704 bytes and private commit 71,716,864 bytes; isolated publication and same-root concurrency pass |
| Held writer | Typed refusal in 0.218629 s, generation and rows unchanged; retry advances exactly one generation |
| Active MCP cancellation | Terminal `canceled` in 0.048108 s, published data preserved, writer released before closing the server, no survivors |
| Forced termination | Quiescent in 0.033103 s, no survivors; production reopen succeeds, `quick_check` is `ok`, recovery checkpoint reports busy/log/checkpointed frames all zero, final WAL/staging bytes zero |

The lifecycle transcript uses the existing harness functions sequentially after
the three commands in the preregistration. After validating the runtime witness
and measurement input hashes, use `measurement_environment('enabled')` and the
preregistered `thresholds.all`: `concurrent_isolation` with 1,024 callers in a
fresh work root; `publication_contention` on its `concurrent-b` root;
`cooperative_cancellation_reopen` on the completed large fixture; then
`forced_termination_quiescence` using that fixture and the lifecycle work root.
Each function receives the locked runtime, environment, owning timeout/budget,
and required version `0.4.5`. Preserve each result before proceeding. Recovery
checkpoint results do not claim a trace of normal checkpoint frame timing.

## Historical measurement boundary

The existing `docs/benchmarks/harness/system_scale.py` is the measurement
owner. It now accepts `--required-version` and `--caller-files`, so the reviewed
v0.4.5 runtime and both generated corpus sizes are selected by the harness
itself; no command-line shim is required. The exact invocations, runtime
identities, source revisions, corpus inputs, environment lock, threshold, and
path placeholders are recorded in
[`v050-358-resource-measurement-input.json`](v050-358-resource-measurement-input.json).
The bounded machine-readable operands are retained in
[`v050-358-resource-measurement-results.json`](v050-358-resource-measurement-results.json);
the `shapes`, `concurrency`, `contention`, and `cancellation` objects are enough
to recompute the table, digest equality, threshold, and failed cells without
local paths or process IDs. The deterministic existing corpus generator
supplied the synthetic 1,024-caller high-degree and synthetic 4,096-caller
high-edge shapes; the committed small clean fixture supplied the small shape.
These are generated fixtures, not an external repository or a claim about the
preregistered huge corpus.

The retained raw baseline runs record runtime SHA-256
`e86a47dfb4468f79261b98a9a70e1ba98c1650f7f3412852cce12458aa3f82f9`
(76,443,648 bytes); their compiled source binding is unavailable and that binary
is no longer retained. The earlier input artifact's different baseline digest
and source claim are historical declarations, not verified provenance. The
shared-pool candidate binary remains retained and matches its raw-run digest.
The results artifact explicitly corrects this discrepancy. This comparison is
an unadopted historical observation, not a verified exact-source speedup or
release performance claim; current measurements have a separate build witness.

The sampler records terminal process-tree CPU and I/O bytes, sampled peak RSS
and threads, SQLite rows/profile/storage, and persistent database/WAL/SHM/stage
bytes. The harness also records representative read-only `EXPLAIN QUERY PLAN`
details and confirms owning-index use without a temporary B-tree. Allocator
events, transaction duration, checkpoint frame timing, and Linux/macOS
measurements are explicitly typed **unavailable** in the results artifact; no
unavailable field is used to justify adoption. The existing external-corpus
path was also attempted against the locked VS Code commit, but its full scan
failed before a valid measurement result (see below). All successful runs used
the same host and telemetry-disabled measurement environment. Both compared
binaries were local debug builds; the measurements are a comparative no-change
decision, not a release performance claim.

The graph digest is emitted by the existing harness as a canonical JSON digest
of logical graph entities, relations, occurrences, coverage, resolution keys,
exports, dependencies, and identity rejections. Project-instance witnesses,
storage-generated row/key identifiers, and rejection generation numbers are
excluded or joined to canonical identities, allowing independent fixture
databases to compare graph content rather than allocation order.

## Historical baseline-versus-shared-pool comparison

The following matrix is the preregistered baseline-versus-candidate comparison
that rejected the shared-pool candidate. It is separate from the exact-head
contention-only replay below; the replay is not a replacement for this matrix.

Values are baseline/candidate pairs. CPU is process-tree CPU seconds, RSS is
peak bytes, and I/O is terminal process-tree transfer bytes. Every database
ended with zero WAL and zero graph-stage directories.

| Shape | Cold wall (s) | Cold CPU (s) | Cold RSS (bytes) | Cold read (bytes) | Cold write (bytes) | Persistent DB (bytes) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Small clean (7 files) | 0.671189 / 0.490724 | 0.187500 / 0.203125 | 27,377,664 / 26,402,816 | 671,219 / 671,426 | 3,620,736 / 3,637,136 | 651,264 / 651,264 |
| Synthetic high-degree (1,028 files; 1,024 callers) | 4.309553 / 5.474020 | 4.062500 / 4.875000 | 59,396,096 / 59,535,360 | 101,830,670 / 103,485,574 | 451,197,848 / 450,574,864 | 19,140,608 / 19,156,992 |
| Synthetic high-edge (4,100 files; 4,096 callers) | 19.481111 / 34.076379 | 18.953125 / 27.625000 | 147,451,904 / 144,781,312 | 901,150,599 / 902,420,659 | 2,182,955,980 / 2,185,186,056 | 72,482,816 / 72,519,680 |

| Shape | Unchanged watch wall (s) | Unchanged watch CPU (s) | Narrow watch wall (s) | Narrow watch CPU (s) | Explicit rebuild wall (s) | Explicit rebuild CPU (s) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Small clean | 0.329231 / 0.300670 | 0.171875 / 0.156250 | not run | not run | not run | not run |
| Synthetic high-degree | 0.372607 / 0.720197 | 0.406250 / 0.390625 | 1.230014 / 1.462383 | 0.359375 / 0.453125 | 4.421013 / 7.415060 | 4.859375 / 6.390625 |
| Synthetic high-edge | 0.527223 / 0.969778 | 0.984375 / 1.234375 | 1.698872 / 1.972959 | 1.562500 / 1.609375 | 21.011708 / 26.531929 | 23.515625 / 28.750000 |

The candidate therefore regressed the synthetic high-degree cold scan by 27.02%
wall and 20.00% CPU, and the synthetic high-edge cold scan by 74.92% wall and
45.75% CPU. The small wall reduction was paired with higher CPU. The candidate
failed the all-shape threshold and was not adopted.

## Concurrency, cancellation, and writer behavior

The existing concurrent-isolation and historical contention harness paths were
run for both binaries with two roots and eight configured workers per process
from a 16-worker host budget. Both binaries preserved cross-root/same-root
correctness, worker-budget, and concurrent RSS limits; the values below are
conservative sums of per-process peaks. Both runs missed the existing medium
rebuild read-transfer cap (149,323,652 baseline; 148,916,441 candidate); the
failed cell is retained and is part of the no-adoption decision.

| Scenario | Baseline | Candidate |
| --- | --- | --- |
| Two-root peak RSS / CPU / read / write | 117,329,920 / 10.562500 / 291,914,812 / 848,033,472 | 118,550,528 / 8.312500 / 291,353,444 / 846,995,952 |
| Same-root peak RSS / CPU / read / write | 119,140,352 / 7.781250 / 147,366,318 / 424,354,216 | 116,932,608 / 5.343750 / 148,050,446 / 425,600,736 |
| Two-root and same-root publication result | passed; own-root identity and generation preserved | passed; own-root identity and generation preserved |
| Writer-lock failure / retry | 1.263722s, `database is locked`; generation unchanged, retry +1; threshold cell failed | 1.264615s, `database is locked`; generation unchanged, retry +1; threshold cell failed |
| Cooperative MCP cancellation | 0.104863s; terminal `canceled`, generation unchanged, writer released, no survivors | 0.114796s; terminal `canceled`, generation unchanged, writer released, no survivors |

The existing delivery E2E cancellation helper additionally reopens the canceled
database and compares its bounded logical SQLite snapshot. Separately, the
`scan_and_watch_preserve_atomic_publication_across_roots`
test exercises the real CLI scan/watch, timeout rollback, MCP watch, two-root
watch, same-root writer lock, and retry in one
bounded fixture. The historical writer-lock rows above include process/setup
overhead and failed the frozen one-second cell in both binaries.

## Exact-head contention-only remediation replay

After the production writer-availability probe was added, the exact-head
runtime was replayed only for same-root contention. This bounded replay is a
causal refusal check, not a complete eligible resource matrix and does not
check OpenSpec tasks 5.1–5.4.

| Runtime scope | Blocked wall (s) | Error | Complete generation/snapshot unchanged | Retry generation | Staging residue | Result |
| --- | ---: | --- | --- | ---: | --- | --- |
| Exact-head corrected runtime, same-root writer contention | 0.799257 | `database is locked` | yes | +1 | none observed | pass |

The replay retained the historical explicit-rebuild read-transfer failure and
does not supply allocator events, a successful external huge-repository
measurement, Linux/macOS runs, transaction duration, or WAL checkpoint-frame
timing. Those fields remain explicitly unavailable below. The replay identity
is the exact source head `32f0297b487a74dc07da50f4a37f842125452204` and the
local `projectatlas 0.4.5` runtime digest is
`9755f0c7b14e0b2de7b92dbc8d275d7b45bd0cf87cf9d8caa0cb42465c61734d`.

## External huge-corpus attempt

The existing `prepare_huge` path fetched and verified the preregistered
external input `https://github.com/microsoft/vscode.git` at commit
`1b6a188127eeaf9194f945eb6eb89a657e93c54c` (tag `1.130.0`), with
`GIT_LFS_SKIP_SMUDGE=1`, a detached clean checkout, 16,646 tracked files, and
235,792,515 tracked bytes. The exact-head `--only huge` command is retained in
the results artifact with the runtime revision and digest. The scan then
failed closed on the real repository's malformed compiler configuration at
`extensions/copilot/test/simulation/fixtures/tests/simple-ts-proj-with-test-file-1/tsconfig.json`;
there is no external huge-corpus timing, resource, or digest claim. This remains
negative fail-closed evidence. The successful 4,096-caller synthetic fixture
supplies the required large/high-edge profile; no malformed configuration is
ignored or special-cased to turn this extra corpus green.

## Graph equivalence and SQLite profile

These hashes are a current read-only recomputation of the six retained SQLite
databases including canonical relation identities, export/dependency resolution
bindings, and validated nested ownership, normalizing only typed project fields. The historical emitted hashes remain
separate in the results JSON. All three pairs and the three current profile
databases remain equal, including their record counts. Authored hexadecimal
text and unresolved references remain literal; malformed relation identities
fail explicitly. Resource measurements and preregistration locks are unchanged.

| Shape | Baseline digest | Candidate digest | Digest records | Equal |
| --- | --- | --- | ---: | --- |
| Small clean | `c43e83ea82ce07e8a2af7aaa0b5896db14ffc051c612f0bf2eb098db06ec612d` | `c43e83ea82ce07e8a2af7aaa0b5896db14ffc051c612f0bf2eb098db06ec612d` | 245 | yes |
| Synthetic high-degree (1,024 callers) | `d9a51ff16f9cdca80e971ab8fc9f15b012ff65ae3089cddb7cfa5093991a0624` | `d9a51ff16f9cdca80e971ab8fc9f15b012ff65ae3089cddb7cfa5093991a0624` | 30,772 | yes |
| Synthetic high-edge (4,096 callers) | `c6d74d40c87b09b76bc55fb435fdf83daf605d34ee73248f2b8d3a222984fd9e` | `c6d74d40c87b09b76bc55fb435fdf83daf605d34ee73248f2b8d3a222984fd9e` | 122,253 | yes |

Both sides retained the existing SQLite profile: WAL journal mode, full
synchronous mode, 4,096-byte pages, clean quick-check, zero final WAL/SHM,
zero graph stages, and no persistent-size amplification beyond normal database
page-layout variance. The harness records representative indexed planner
details in the results artifact; the owning query-plan assertion
`detailed_relation_storage_statements_are_indexed_and_batch_bounded` remains
the complete proof for indexed, bounded graph reads. Transaction duration and
checkpoint frame timing are unavailable from the existing runtime, while the
real E2E proves one-generation atomicity, rollback, cancellation, and retry.
This issue adds no schema or query. The explicit-rebuild read-transfer red
cell remains machine-readable in the results artifact: the locked 134,217,728
byte medium cap was exceeded by both historical arms (149,323,652 baseline;
148,916,441 candidate), so the failure was retained rather than normalized.

## Disposition

The shared Rayon pool, parallel graph-admission candidate, and candidate-only
`scan --max-workers` surface were deleted. Current parser/summary pool creation,
graph derivation and identity admission, synchronous atomic publication, bounded
staging, cancellation, late-failure rollback, writer contention, and platform
behavior remain owned by the existing runtime boundaries. The current normal
profile adds sampled allocation footprint, explicit runtime-byte validation,
parser-worker accounting, and successful writer/cancellation/recovery proof.
It preserves the historical no-adoption decision and diagnostic failures.
Exact allocator-event counts and per-stage transaction/checkpoint frame timing
remain unavailable; no new instrumentation framework is needed to retain the
existing worker design. OpenSpec tasks 5.1–5.4 remain unchecked until independent
review and required hosted functional platform proof complete. Windows resource
accounting does not substitute for Linux/macOS execution of the real scan/watch
and publication regressions.
