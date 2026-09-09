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

## Measurement boundary

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

The exact accepted-main baseline and shared-pool candidate are retained under
the ignored project-local benchmark/build paths. Their SHA-256/size, source
revisions, and bounded build command are recorded in the input and results
artifacts. This is a performance comparison record, not a release binary claim.

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
| Small clean (7 files) | 0.592225 / 0.536433 | 0.156250 / 0.156250 | 26,996,736 / 27,648,000 | 734,311 / 737,090 | 3,628,936 / 3,628,936 | 651,264 / 651,264 |
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

The existing Rust E2E cancellation helper additionally reopens the canceled
database and compares its bounded logical SQLite snapshot. The
`scan_and_watch_preserve_atomic_publication_across_roots`
test exercises the real CLI scan/watch, timeout rollback, MCP watch, two-root
watch, same-root writer lock, retry, and that cancellation helper in one
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
there is no external huge-corpus timing, resource, or digest claim. This is a
real failed gate, not a substitute fixture or a completed task.

## Graph equivalence and SQLite profile

| Shape | Baseline digest | Candidate digest | Digest records | Equal |
| --- | --- | --- | ---: | --- |
| Small clean | `bec258fbb18c32d8868d3fe78ae4aff84f5bbc066be4ef149563d2662731cda3` | `bec258fbb18c32d8868d3fe78ae4aff84f5bbc066be4ef149563d2662731cda3` | 245 | yes |
| Synthetic high-degree (1,024 callers) | `42eb33c7ae271906c3203f834c15c8077ab8cec296d0999ed78c3677dc515719` | `42eb33c7ae271906c3203f834c15c8077ab8cec296d0999ed78c3677dc515719` | 30,772 | yes |
| Synthetic high-edge (4,096 callers) | `c72f6d5f82a6a84cc81fde3032df312ec5a7534b7b497bfa00d631d2072f5880` | `c72f6d5f82a6a84cc81fde3032df312ec5a7534b7b497bfa00d631d2072f5880` | 122,253 | yes |

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
behavior remain owned by the existing runtime boundaries. OpenSpec tasks 5.1–5.4
remain unchecked: the corrected Windows evidence and no-change decision are
recorded, but allocator events, a successful real external huge-repository
measurement, Linux/macOS measurements, transaction duration, and checkpoint
frame timing remain unavailable from this bounded existing harness run. The
external corpus gate is specifically blocked by the fail-closed scan error
above; platform evidence requires the existing hosted Linux/macOS matrix; and
allocator/transaction/checkpoint evidence require measurement sources not
exposed by this runtime/harness. Hosted/reviewer acceptance is not a local
implementation gate.
