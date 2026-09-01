# #358 graph-construction resource measurement

## Decision

The existing separate-pool indexing lifecycle is retained. The candidate that
reused one Rayon pool for parsing, graph-identity admission, and structural
summaries was removed after comparison because it did not produce a material
full-envelope win. No schema, dependency, or production resource boundary was
changed by this decision.

The frozen remediation threshold was a reduction of at least 10% in both cold
scan wall time and CPU time for every representative shape, with no more than a
5% regression in any measured resource envelope and no correctness, generation,
cancellation, contention, or cleanup regression. A faster single incremental
rebuild was not sufficient to offset a cold-scan regression.

## Measurement boundary

The existing `docs/benchmarks/harness/system_scale.py` is the measurement
owner. It now accepts `--required-version` and `--caller-files`, so the reviewed
v0.4.5 runtime and both generated corpus sizes are selected by the harness
itself; no command-line shim is required. The exact six invocations, runtime
identities, source revisions, corpus inputs, environment lock, threshold, and
path placeholders are recorded in
[`v050-358-resource-measurement-input.json`](v050-358-resource-measurement-input.json).
The bounded machine-readable operands are retained in
[`v050-358-resource-measurement-results.json`](v050-358-resource-measurement-results.json);
the `shapes`, `concurrency`, `contention`, and `cancellation` objects are enough
to recompute the table, digest equality, threshold, and failed cells without
local paths or process IDs. The deterministic existing corpus generator
supplied the 1,024-file high-degree and 4,096-file large/high-edge shapes; the
committed small clean fixture supplied the small shape.

The historical candidate executable is not retained in the worktree after the
replay; its recorded SHA-256/size identify the measured artifact, and the
candidate source revision plus the bounded build command are retained for
reconstruction. This is a performance comparison record, not a release binary
claim.

The sampler records terminal process-tree CPU and I/O bytes, sampled peak RSS
and threads, SQLite rows/profile/storage, and persistent database/WAL/SHM/stage
bytes. Allocator events, transaction duration, checkpoint timing, planner text,
an external huge repository, and Linux/macOS measurements are explicitly typed
**unavailable** in the results artifact; no unavailable field is used to
justify adoption. All runs used the same host and telemetry-disabled
measurement environment. Both compared binaries were local debug builds; the
measurements are a comparative no-change decision, not a release performance
claim.

The graph digest is emitted by the existing harness as a canonical JSON digest
of logical graph entities, relations, occurrences, coverage, resolution keys,
exports, dependencies, and identity rejections. Project-instance witnesses,
storage-generated row/key identifiers, and rejection generation numbers are
excluded or joined to canonical identities, allowing independent fixture
databases to compare graph content rather than allocation order.

## Cold and incremental comparison

Values are baseline/candidate pairs. CPU is process-tree CPU seconds, RSS is
peak bytes, and I/O is terminal process-tree transfer bytes. Every database
ended with zero WAL and zero graph-stage directories.

| Shape | Cold wall (s) | Cold CPU (s) | Cold RSS (bytes) | Cold read (bytes) | Cold write (bytes) | Persistent DB (bytes) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Small clean (7 files) | 0.592225 / 0.536433 | 0.156250 / 0.156250 | 26,996,736 / 27,648,000 | 734,311 / 737,090 | 3,628,936 / 3,628,936 | 651,264 / 651,264 |
| Medium high-degree (1,028 files) | 3.637465 / 4.638718 | 4.031250 / 4.718750 | 58,327,040 / 59,867,136 | 100,928,028 / 102,858,911 | 451,222,284 / 449,906,744 | 19,124,224 / 19,156,992 |
| Large/high-edge (4,099 files) | 18.345450 / 20.815886 | 20.187500 / 21.953125 | 143,839,232 / 145,047,552 | 901,575,354 / 901,737,343 | 2,184,124,648 / 2,185,144,596 | 72,511,488 / 72,392,704 |

| Shape | Unchanged watch wall (s) | Unchanged watch CPU (s) | Narrow watch wall (s) | Narrow watch CPU (s) | Explicit rebuild wall (s) | Explicit rebuild CPU (s) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Small clean | 0.312957 / 0.397349 | 0.171875 / 0.203125 | not run | not run | not run | not run |
| Medium high-degree | 0.376240 / 0.873522 | 0.359375 / 0.468750 | 1.249288 / 1.250055 | 0.281250 / 0.281250 | 9.055662 / 4.331094 | 6.921875 / 4.593750 |
| Large/high-edge | 0.614500 / 0.610234 | 1.000000 / 1.390625 | 1.749800 / 1.693783 | 1.484375 / 1.390625 | 19.491716 / 20.096981 | 22.531250 / 22.968750 |

The candidate therefore regressed the medium cold scan by 27.53% wall and
17.05% CPU, and the large cold scan by 13.47% wall and 8.75% CPU. The small
wall reduction was paired with higher RSS and slower unchanged watch. The
medium rebuild improvement did not satisfy the all-shape threshold, while the
large rebuild also regressed.

## Concurrency, cancellation, and writer behavior

The existing concurrent-isolation and contention harness paths were run for
both binaries with two roots and eight configured workers per process from a
16-worker host budget. Both binaries preserved cross-root/same-root correctness,
worker-budget, and concurrent RSS limits; the values below are conservative sums
of per-process peaks. Both runs missed the existing medium rebuild read-transfer
cap (145,837,576 baseline; 149,158,913 candidate); the failed cell is retained
and is part of the no-adoption decision.

| Scenario | Baseline | Candidate |
| --- | --- | --- |
| Two-root peak RSS / CPU / read / write | 116,973,568 / 7.953125 / 291,939,388 / 849,669,592 | 118,800,384 / 7.406250 / 292,262,660 / 846,775,016 |
| Same-root peak RSS / CPU / read / write | 116,224,000 / 5.109375 / 147,743,126 / 425,288,960 | 118,333,440 / 5.421875 / 146,772,422 / 424,215,272 |
| Two-root and same-root publication result | passed; own-root identity and generation preserved | passed; own-root identity and generation preserved |
| Writer-lock failure / retry | 1.460155s, `database is locked`; generation unchanged, retry +1 | 1.525907s, `database is locked`; generation unchanged, retry +1 |
| Cooperative MCP cancellation | 0.088352s; terminal `canceled`, generation unchanged | 0.092811s; terminal `canceled`, generation unchanged |

The existing Rust E2E cancellation helper additionally reopens the canceled
database and compares its bounded logical SQLite snapshot. The new
`resource_measurement_baseline_pipeline_preserves_atomic_graph_publication`
test exercises the real CLI scan/watch, timeout rollback, MCP watch, two-root
watch, same-root writer lock, retry, and that cancellation helper in one
bounded fixture.

## Graph equivalence and SQLite profile

| Shape | Baseline digest | Candidate digest | Digest records | Equal |
| --- | --- | --- | ---: | --- |
| Small clean | `bec258fbb18c32d8868d3fe78ae4aff84f5bbc066be4ef149563d2662731cda3` | `bec258fbb18c32d8868d3fe78ae4aff84f5bbc066be4ef149563d2662731cda3` | 245 | yes |
| Medium high-degree | `42eb33c7ae271906c3203f834c15c8077ab8cec296d0999ed78c3677dc515719` | `42eb33c7ae271906c3203f834c15c8077ab8cec296d0999ed78c3677dc515719` | 30,772 | yes |
| Large/high-edge | `c72f6d5f82a6a84cc81fde3032df312ec5a7534b7b497bfa00d631d2072f5880` | `c72f6d5f82a6a84cc81fde3032df312ec5a7534b7b497bfa00d631d2072f5880` | 122,253 | yes |

Both sides retained the existing SQLite profile: WAL journal mode, full
synchronous mode, 4,096-byte pages, clean quick-check, zero final WAL/SHM,
zero graph stages, and no persistent-size amplification beyond normal database
page-layout variance. Transaction duration, checkpoint timing, and planner
text are explicitly unavailable from this harness; the owning query-plan
assertion `detailed_relation_storage_statements_are_indexed_and_batch_bounded`
remains the available proof for indexed, bounded graph reads. This issue adds
no schema or query.

## Disposition

The shared Rayon pool, parallel graph-admission candidate, and candidate-only
`scan --max-workers` surface were deleted. Current parser/summary pool creation,
graph admission, synchronous atomic publication, bounded staging, cancellation,
late-failure rollback, writer contention, and platform behavior remain owned by
the existing runtime boundaries. OpenSpec tasks 5.1–5.4 remain unchecked: the
measurement and no-change decision are recorded, but allocator events and a
real external large repository were not available from this bounded existing
harness run, and hosted/reviewer acceptance is not a local implementation gate.
