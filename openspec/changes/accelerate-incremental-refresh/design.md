# Design

## Context

See [proposal.md](proposal.md) for scope. #708 is now a 0.5.1 must-have under #715 and covers initial indexing, full reindexing, symbol/graph rebuilding and refreshes. Stable 0.5.0 is the new performance baseline. This planning change modifies no product code or release artifact.

The current CLI enters `run_single_watch_refresh_controlled`, then `refresh_index_controlled`. MCP watch-once calls the same refresh owner without using its server's `SourceObservationRegistry`. An unchanged call walks admitted paths, hashes their contents, loads stored nodes and compares the two sets. It avoids parsing and a new generation when nothing changed. A changed one-shot call can stage a full publication and perform two further exact validation scans. Continuous watching already supplies `WatchChangeSet` to an incremental path.

The one-shot cost is therefore tied to all entries and source bytes, with sorting/map construction as well. The full scan pipeline stages complete index state, revalidates publication inputs and publishes atomically. CLI and MCP symbol rebuilds share `run_symbol_build_pipeline_controlled`. These are source findings, not latency measurements. Initial/full indexing and symbol rebuilding still need their own measured phase profiles. Historical #358 timings are fixture context, not a 0.5.0 baseline.

## Goals / Non-Goals

**Goals:** improve initial/full indexing and rebuilding as well as refresh, reuse reliable changes, minimize affected work, preserve complete results and choose bounded default budgets from required work.

**Non-Goals:** a generic change-tracking framework, a new crate/database/daemon by default, Git-only discovery, metadata-only freshness, or speed claims based on unoptimized binaries or disabled normal telemetry.

## Decisions

### Settle the architecture before expanding measurements

Use one validated operation plan in the existing shared indexing owners. Reuse their current no-op, incremental and full-work states rather than adding a parallel framework. Capture root identity, policy, publication generation and trustworthy observation together, then select the required work once.

| Work mode | Architectural guarantee |
| --- | --- |
| Verified unchanged observation | Bounded identity/policy/generation checks; no tree traversal, content hashing, full-node decode, parsing or publication. |
| Known changes | Read and parse changed sources; update the actual affected data and dependency closure, without restaging unrelated project state. |
| Fresh CLI without reliable history | One exact source-discovery/comparison phase, then affected work. Compare lightweight ordered source records with the stored hash cursor; avoid loading complete stored nodes or repeatedly rediscovering the same project. |
| Required initial/full/rebuild work | Capture required source evidence once for building, share valid bytes and parse facts across projections, and publish complete required state. Wider validation is retained only where the source/writer evidence needs it. |

The source comparison is a linear merge of ordered lightweight records; use existing bounded ordering/staging facilities rather than assuming directory traversal is already sorted or allocating another full-node map. Expand the affected set once through the existing dependency/invalidation owner. Keep source reads, compatible parser results and verification stamps reusable within the operation, tied to their exact source/policy/generation.

Keep SQL, prepared/batched operations and snapshot ownership in the database crate. Use only the columns needed for comparison and bound cursor/staging memory. Preserve a consistent captured publication across reads; release readers before slow work where the existing snapshot/guard design allows it so they do not unnecessarily pin WAL checkpoints. Traverse and parse outside the published database's write transaction; retain the existing short writer-validation and atomic commit boundary. Do not add a pool, parallel writers, new indexes or tuning pragmas without an owning workload reason.

The Rust mechanism is existing concrete operation types, closed work states and RAII publication/cancellation guards, with bounded iterators/batches. A second engine, generic strategy layer or independently sized thread pool has no current consumer. The principal risk is stale evidence authorizing publication; source/policy/generation validation and causal failure/concurrency tests protect that boundary.

This is the chosen starting architecture. Code investigation and these work guarantees drive the implementation. Use a small optimized 0.5.0 reference baseline and focused before/after checks to confirm the expected benefit, not an open-ended benchmark search for an architecture. Set the cold-CLI evidence/lifetime contract before code; do not hide an unresolved history gap behind a fast MCP result. Keep broader platform validation at integrated acceptance and reuse unchanged passing proof.

### Keep one refresh and publication owner

CLI/MCP init, scan and symbol-build adapters keep their existing shared pipeline owners; one-shot and continuous watch keep the shared refresh owner. Improve their common filesystem, parsing, comparison and atomic-publication work rather than introducing separate fast implementations per adapter. Known changes describe additions, modifications, deletions and renames of any size. Include affected text, symbols, summaries, relations and configuration-dependent data; one changed file can affect many graph rows.

Reuse `WatchChangeSet`, the existing source observer, source verification and store transaction boundaries. Do not add parallel adapter implementations or a second source-of-truth index. Keep native project/worktree identity captured throughout the operation.

### Improve initial indexing and real rebuilds

Profile `run_scan_pipeline_controlled`, symbol rebuilding and their shared staging/publication path before selecting optimizations. Measure directory entries, source bytes, hashing, parser work, stored-source decoding, database statements/writes, graph invalidation, validation passes and peak memory. Use separate initial-index, full-scan and explicit symbol/graph-rebuild fixtures; verify that the required work actually ran.

Select the smallest measured improvement in the owning code: avoid redundant traversal/reading/decoding, use bounded streaming or ordered comparisons where appropriate, batch database work using existing transactions, and reuse valid source verification without losing its race protection. Inspect real SQLite behavior and query plans before proposing an index or storage change. Keep worker, memory, file and output bounds effective across the host.

Preserve selection, ignores, parser provenance, document partial coverage, source/summary/text/symbol/relation completeness and existing force/rebuild semantics. A required full rebuild cannot be replaced by an unrelated cached no-op. Exact cold/full work necessarily inspects the source evidence it needs; the target is efficient bounded work, not an unsupported constant-time promise. Initial/full/rebuild paths must meet improvement targets, not merely avoid a regression.

### Reuse trustworthy observations; handle missing history honestly

For a long-lived process, use the existing observer only while its root, policy, publication identity and event continuity remain valid. Integrating that observation with writes must fence concurrent writers and source changes; a read stamp alone is not permission to publish.

A fresh CLI process cannot infer edits made before it started from an empty event queue. Without trustworthy retained evidence it performs exact initial discovery. Start observation before this baseline where the existing backend supports it, so later source changes can be checked during the same refresh. Lost events, unavailable observation or invalid identities require exact reconciliation. No unchanged result may be inferred solely from equal size/mtime or the absence of newly observed events.

This chooses the existing observer plus exact fallback, rather than introducing an unproved persistent service or silently relaxing freshness. If measurements show that cross-invocation evidence needs an additional component, publish and review that concrete design before adding it; the issue cannot close with only a fast MCP path and an unexplained slow CLI path.

MCP one-shot refresh may admit an unchanged result through that same verified-read
boundary, with source repair disabled. Establish a missing epoch by exact comparison;
reuse an existing valid epoch without project traversal, content hashing, node decode,
parsing or publication. A detected delta transfers its operation-owned assessment to
the existing mutable refresh owner, which revalidates database identity, generation,
root and contract without repeating discovery, so repaired work is never reported as
a no-op. Caller text-policy overrides use the
normal refresh owner. Preserve worktree identity checks and the normal background task,
cooperative cancellation and exact-fallback contracts. This read-only admission grants
no write authority and does not relax mutation validation.

### Prove the standalone CLI approach before production changes

Task 2.1 settles the exact-discovery/cross-invocation evidence contract and a separate cold-CLI target using code analysis and a focused reference check. The selected initial implementation uses exact discovery plus lightweight ordered comparison and affected updates when no trusted history exists. Task 2.4 proves that CLI approach as well as the warm MCP path. A fast MCP result cannot substitute for a slow CLI result.

If exact cold discovery cannot meet that target, publish and review a concrete trustworthy cross-invocation evidence design before implementing that alternative. Cover how history is obtained and retained, root/identity validation, gaps, restart, fallback, ownership and any required process lifetime or deployment behavior. Do not silently introduce a service. The zero-traversal bound applies to a continuously observed MCP route or a CLI route that actually implements such reviewed retained evidence; it is not promised for an ordinary fresh CLI with no history.

### Apply the delta and avoid repeated whole-project work

Convert discovered or observed changes into the existing incremental path. Use source/dependency invalidation rules to expand the affected set only when necessary. Relevant compiler, ignore, parser or publication-policy changes may legitimately invalidate a wider scope.

Use source-only stored fields for comparison where feasible, and compare ordered source records in a linear pass instead of building unnecessary maps on an unchanged result. These are concrete candidates to measure, not permission to add a speculative index or cache.

The next implementation slice retains the exact discovery's ordered current nodes,
captured database instance identity, publication generation, root and contract as
operation-owned source evidence.
An admitted delta consumes that evidence: select changed nodes from it, query downstream
authored/derived state only for affected paths after the full ordered source-only
comparison, and move the complete current-node vector into the
existing graph/publication plan. Do not hash changed paths again merely to reconstruct
nodes already captured by discovery, or build a second whole-source lookup map.
Full reconciliation selected directly by that assessment also consumes its captured
inventory, rather than retaining it while discovering another complete copy. Recheck
the identity and generation at full staging admission; final exact validation still
rejects edits made after capture. An attempted incremental closure that has already
consumed its evidence may rediscover during full fallback.
Reject a changed database identity, generation, root or contract before reuse. Existing source reads
must still match the captured content hashes, and final exact publication validation
remains until an equivalent writer-safe witness is established. A delayed source edit
or concurrent publication must refuse without exposing a partial generation.

Text staging already holds hash-verified UTF-8 content. Built-in symbol parsing may
borrow that operation-owned plain-text content instead of reopening the file only when
its bytes preserve parser semantics, including BOMs and newlines; do not
clone the corpus or apply this shortcut to extracted PDF/DOCX text, skipped text,
incompatible parser policies or external parser-worker inputs. Keep bounded fallback
reads and existing final source validation. This is evidence reuse within the existing
owners, not a new cache or cross-invocation history claim.

Reuse a valid observed epoch and per-source verification across staging and publication where the existing safety contract permits it. Remove repeated exact scans only after proving their race/failure protection is provided by the retained observation and writer checks. When that proof is unavailable, keep the exact fallback. Optimize initial/full discovery separately where measurement identifies avoidable work.

### Select the adaptive budget once at admission

Automatic budget selection at the existing shared indexing work-control boundary uses reliable available estimates: required discovery, admitted file counts/bytes, known changes, affected data and parser costs. Cover initial/full/rebuild operations as well as refresh. Do not add an unbudgeted whole-tree walk just to calculate a budget. Cold operations include discovery cost; a verified warm no-op does not receive a rebuild estimate merely because the repository is large.

Keep #699's existing 1,800-second ceiling and clamp for larger representable requests. Version 0.5.1 changes only automatic admission when the caller omits a limit. If reliable counts/bytes/work estimates are unavailable, including a new cold initial scan, use the existing 1,800-second budget without a preflight walk. With reliable estimates, choose a bounded default using coefficients and a minimum fixed by task 2.1's measurements; the maximum stays 1,800 seconds.

An explicit limit up to the ceiling wins, even when it equals the former default of 1,800 seconds. Preserve zero-budget refusal, the larger-value clamp and fail-closed unrepresentable deadlines. Do not reset the clock or extend caller limits during retries. Update affected help, schemas, skill references and budget guidance with the automatic-default change before shipping it; retain the closed #699 artifacts as the historical 0.5.0 contract. Preserve cooperative checkpoints, cancellation and memory/file/output/worker bounds; do not claim a hard wall-clock guarantee.

### Measure before choosing further optimizations

Use the actual optimized installed 0.5.0 CLI/MCP as the baseline with normal telemetry. Architecture and deterministic work-count guarantees come first. Use a small representative reference set to fix practical latency/throughput and resource targets before the affected implementation; do not run exhaustive platform/fixture sweeps before a compiling behavior slice exists. Include large file counts and byte totals, dense graphs, supported parser/document coverage, initial/full/rebuild work, small and large deltas, warm/cold starts and caches, no Git, policy changes and required reconciliation in the final supported-platform acceptance. Keep private repository context out of public evidence.

Record distributions rather than one timing. Record visited entries, hashed bytes, parse candidates, decoded nodes, store statements/writes, generation changes, CPU and RSS where supported. Reuse `SourceVerificationWork` and existing harnesses; keep benchmark-only detail out of normal product output unless a real user contract needs it.

Choose the smallest causal checks for each implementation slice and a focused before/after comparison for its performance claim. Reuse existing fixtures and counters. Run the complete installed/platform matrix at release acceptance; repeat a passing check only for changed inputs, failures, an uncovered behavior or a mandatory gate. Measurement confirms the architecture; it does not replace the architectural work.

The hard work-count target for a verified unchanged observation is no project-tree traversal/content hashing/full-node decode, no parse work and no new publication. Delta processing must avoid unrelated parsing/publication. Cold discovery, initial indexing, full reindexing, symbol rebuilding and recovery each have separate targets. Report each result independently so a warm-path gain cannot conceal another slow path. Normal cases must finish within their selected budgets; injected deadlines still prove safe refusal. A larger budget alone is never a performance result. Unaffected navigation and supported upgrades must not regress beyond preregistered noise/resource limits.

## Risks / Trade-offs

- Missing or coalesced events could hide an edit. Detect continuity loss and test overflow, offline edits, restart, races, same-size/same-mtime changes and exact fallback.
- A read-oriented observer could incorrectly authorize a write. Require writer-safe root/policy/generation validation and concurrent-reader/writer failure tests.
- A small delta could still rebuild the graph or decode every stored summary. Assert affected work and row/byte counts as well as elapsed time.
- Adaptive budgets could hide slow algorithms or violate caller intent. Test the estimator separately from speed and preserve explicit-limit precedence.
- Cold exact discovery still has a cost proportional to the evidence it must inspect. Report this honestly, measure it separately and do not claim constant-time discovery without a trusted change source.

## Migration Plan

No schema change, public route removal or new dependency is required by this plan. Introduce each proven improvement in its existing shared owner with causal tests and exact fallback. If storage or a persistent component becomes necessary, review its concrete compatibility, migration and rollback design before implementation. Reuse installed-upgrade and four-platform gates before #715 accepts 0.5.1. Preserve 0.5.0 user databases and authored state.

## Dependencies / Cross-Issue Impact

#708 is a required 0.5.1 child and blocker of #715. Remove its old 0.6.0 membership and #493 blocker relationship without changing #493's remaining children. It has no implementation prerequisite on #310, #314, #388 or the language work in #563. Those 0.6.0 features consume the accepted indexing baseline later.

#699 supplies the cooperative-budget and caller-limit contract. Update affected help, schemas and default-budget guidance together without weakening explicit caller control. #358 supplies historical context only; task 2.1 establishes the optimized 0.5.0 baseline and numerical targets. The release owner implements no performance fix; this plan does not publish 0.5.1.

## Open Questions

None.

### Notification-delivery finding during platform proof

The macOS ARM64 installed release test exposed delayed callback delivery after an
immediate saved edit. Notify 8.2.0 exposes no backend delivery fence; draining the
Rust receiver and checking its callback counter cannot prove the native event queue
is empty, on macOS or other asynchronous backends. One-shot refresh currently uses
exact no-repair admission and transfers its discovery to the existing writer. The
zero-source-work warm target remains open until a causal fence is designed, reviewed
and proven; neither a sleep nor a Linux timing pass establishes that proof.
