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

The existing `docs/benchmarks/harness/system_scale.py` was used with its
Windows Job accounting and `psutil` sampler. The checked-out runtime reports
v0.4.5 while this historical harness asks the process for v0.4.0; the runs used
a non-persistent command-line argument shim that substituted the runtime's
actual compatible version, without changing the harness or its measurement
inputs. The deterministic existing corpus generator supplied the 1,024-file
high-degree and 4,096-file large/high-edge shapes; the committed small clean
fixture supplied the small shape.

The sampler records terminal process-tree CPU and I/O bytes, sampled peak RSS
and threads, SQLite rows/profile/storage, and persistent database/WAL/SHM/stage
bytes. Allocator events are not exposed by the existing harness on this host,
so `allocations` is explicitly **unavailable** in this record; no allocator
claim is used to justify adoption. All runs used the same host and telemetry
disabled measurement environment. Both compared binaries were local debug
builds; the measurements are a comparative no-change decision, not a release
performance claim.

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

The graph digest is a deterministic digest of normalized graph entities,
relations, occurrences, coverage, resolution canonical identities, exports,
dependencies, and identity rejections. Project-instance witnesses and
storage-generated row/key identifiers are normalized or joined to their
canonical identities, so separate fixture databases can be compared without
mistaking fresh project identity for graph drift.

| Shape | Baseline digest | Candidate digest | Semantic rows | Equal |
| --- | --- | --- | ---: | --- |
| Small clean | `ffa8955ceb9ac202fd04a0a3767cc1dd389b8070d5ce932d53da4c50a5363b45` | `ffa8955ceb9ac202fd04a0a3767cc1dd389b8070d5ce932d53da4c50a5363b45` | 188 | yes |
| Medium high-degree | `ce979939f0b0a165ac8088a5ff5aa5694bea07fdf34292acdb6fada8457f883f` | `ce979939f0b0a165ac8088a5ff5aa5694bea07fdf34292acdb6fada8457f883f` | 23,592 | yes |
| Large/high-edge | `2cee13440e489f59b4cdd6450b15290039c02be82cb1e6a821cacf7db7fc70b0` | `2cee13440e489f59b4cdd6450b15290039c02be82cb1e6a821cacf7db7fc70b0` | 93,763 | yes |

Both sides retained the existing SQLite profile: WAL journal mode, full
synchronous mode, existing schema and indexes, clean quick-check, zero final
WAL, zero graph stages, and no persistent-size amplification beyond normal
database page-layout variance. The owning database query-plan assertions,
including `detailed_relation_storage_statements_are_indexed_and_batch_bounded`,
remain the proof for indexed, bounded graph reads; this issue adds no schema or
query.

## Disposition

The shared Rayon pool, parallel graph-admission candidate, and candidate-only
`scan --max-workers` surface were deleted. Current parser/summary pool creation,
graph admission, synchronous atomic publication, bounded staging, cancellation,
late-failure rollback, writer contention, and platform behavior remain owned by
the existing runtime boundaries. OpenSpec tasks 5.1–5.4 remain unchecked: the
measurement and no-change decision are recorded, but allocator events and a
real external large repository were not available from this bounded existing
harness run, and hosted/reviewer acceptance is not a local implementation gate.
