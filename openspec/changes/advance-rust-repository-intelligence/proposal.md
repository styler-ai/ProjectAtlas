## Why

ProjectAtlas already provides a safe, Rust-native, atlas-first repository index and agent navigation funnel, but it does not yet provide the breadth of grammar coverage, cross-file knowledge graph, evidence-preserving incremental resolution, or ranked architecture analysis expected from a complete repository-intelligence system. This change defines a phased program to add an explicitly accepted capability set without replacing ProjectAtlas's project-local SQLite, TOON, MCP, purpose, and token-telemetry strengths.

The program is ready for phased implementation after the Phase 0 benchmark and dependency decisions are recorded. Later phases MUST remain gated on measured correctness, integrity, memory, latency, packaging, and cross-platform results rather than feature-count claims.

## What Changes

- Introduce a generated, version-locked language registry with explicit detection, parsing, symbol, semantic-resolution, and benchmarked-support tiers. The initial accepted set SHALL enumerate at least 159 runnable language/file modes mapped to at least 157 verified parser capabilities after aliases and parser reuse are normalized. Parity means every accepted entry passes its declared tier; raw counts alone never compensate for a missing or failing entry.
- Extend the current per-file symbol graph into a typed repository knowledge graph with stable identities, normalized entities and relations, source-span evidence, confidence, ambiguity, unresolved-reference, parser, resolver, and slot/epoch provenance.
- Redesign full and incremental indexing around two physical derived-data slots, atomic `(active_slot, active_epoch)` publication, conservative deletion, inbound-edge preservation, selective invalidation, bounded/cancellable work, explicit coverage evidence, and loud integrity failures.
- Add ProjectAtlas-native architecture, dependency, impact, dead-code candidate, node-simple path-trace, similarity, and relationship queries through shared Rust services, CLI commands, and typed `atlas_*` MCP tools. Graph schema and capability reporting SHALL extend the existing `atlas_settings` surface, and overview-to-slice navigation remains the default agent funnel.
- Keep deterministic lexical retrieval available in every default-core installation and make FTS5/BM25 a conditional acceleration selected only after runtime capability and equivalence checks. An optional, local-only semantic pack has an independent install/enable/build/ready/stale/fail/disable/remove lifecycle; its grammar, WASM, ANN, and model runtimes are neither linked into nor initialized by the default core.
- Add call-only, opt-in cross-repository federation whose every request supplies an explicit ordered root set. Participating databases are opened read-only, validation fails closed for the whole call, and ProjectAtlas creates no hidden global database, remembered root set, cross-project write, or background federation task.
- Add public, reproducible language-accuracy, graph-correctness, incremental-integrity, performance, memory, package-size, query-latency, and statistically evaluated agent-task benchmark gates against pinned comparison baselines and prior ProjectAtlas releases.
- Make performance an architectural property: compact typed graph arenas, numeric hot-path identities, string/path interning, bounded worker-local buffers, pre-indexed adjacency, batched staged persistence, and concurrent reads against the active slot/epoch SHALL replace generic per-object/JSON hot paths and whole-graph incremental rewrites.
- Enforce hard default-core ceilings for release-binary bytes, installed footprint, cold startup, idle MCP RSS, and no-pack scan RSS. Broad grammar/WASM hosts and ANN/model runtimes SHALL ship only in separate explicit packs and SHALL not appear in the default core dependency graph or process initialization path.
- Keep clean-host installation agent-simple: one official plugin-store installation action SHALL provision and verify the matching runtime, skill, and MCP registration without manual binary downloads, PATH editing, MCP JSON, version pins, or database-path wiring; ordinary `atlas_init` and atlas-first calls SHALL be the first project workflow.
- Permit independent, behavior-level Rust implementations of useful public algorithms and capabilities, but prohibit source-to-source translation, copied comments/tests/constants, and mirrored source module/function structure. Implementation review SHALL verify ProjectAtlas-owned structure and complete provenance.
- Preserve compatibility across every existing ProjectAtlas CLI command and MCP tool unless a delta specification explicitly changes it, including names, accepted request shapes, default semantics, output formats, exit/error behavior, project-local databases, dynamic `.gitignore` inheritance, repository-relative path validation, explicit `project_path` isolation, purpose metadata, TOON-first responses, token telemetry, and bounded MCP task progress.

### Non-Goals

- Do not use mechanical source translation as an implementation method; new Rust code must follow ProjectAtlas ownership and naming.
- Do not promise equal semantic depth for every grammar-backed language; broad syntax coverage and proven semantic coverage are separate contracts.
- Do not claim that Rust alone guarantees better performance; superiority must be demonstrated on reproducible comparative benchmarks.
- Do not implement a custom Cypher dialect when typed filters and bounded traversal services satisfy the agent workflows.
- Do not copy a Git-only polling watcher, process-global spinlock, mutable ADR subsystem, built-in 3D graph UI, or always-bundled embedding model.
- Do not execute repository code, package-manager hooks, language servers, build scripts, or downloaded grammars during normal indexing.
- Do not weaken project-root isolation, path safety, ignore behavior, or existing CLI/MCP compatibility to gain cross-repository features.
- Do not advertise a language, relationship, or benchmark tier from node counts alone.

## Capabilities

### New Capabilities

- `language-intelligence-registry`: Generated grammar registry, tiered language contracts, parser/query packs, fixtures, provenance, and capability reporting.
- `repository-knowledge-graph`: Typed repository graph entities, relations, stable identities, evidence, confidence, ambiguity, provenance, and schema evolution.
- `incremental-index-integrity`: Two-slot full publication, epoch-based freshness, correct incremental invalidation, snapshot integrity, conservative failure behavior, and coverage accounting.
- `graph-retrieval-and-analysis`: Ranked structural search, architecture summaries, graph schema, dependency/impact analysis, dead-code candidates, bounded path tracing, and query evidence.
- `local-semantic-retrieval`: Always-available lexical ranking plus optional local embeddings, hybrid ranking, a typed model lifecycle, bounded resources, and explicit capability errors when a requested semantic mode is not ready.
- `cross-repository-intelligence`: Explicit root federation, stable external identities, cross-repository relationships, isolation, and federated query behavior.
- `repository-intelligence-benchmarks`: Reproducible correctness, language, integrity, performance, memory, package, query, and agent-efficiency comparison gates.

### Modified Capabilities

None. This repository currently has no synchronized main specifications under `openspec/specs/`; the new capability specifications preserve current public behavior and define additive contracts.

## Impact

- **Core/domain:** `projectatlas-core` gains typed graph, language-capability, evidence, confidence, resolution, slot/epoch, and coverage contracts with centralized serialized values.
- **Extraction:** `projectatlas-symbols` moves from a hand-wired parser list and mostly per-file relations toward generated grammar/query packs, normalized scopes, project-wide registries, and language-specific resolvers.
- **Scanning/runtime:** `projectatlas-fs` and the shared runtime gain content-identity diff planning, dependency-driven invalidation, staged publication, cancellation, resource budgets, and conservative path/deletion handling.
- **Storage:** `projectatlas-db` gains versioned graph/slot/epoch/coverage/search/vector schemas, a checksummed migration ledger with read-only preflight, transactional migrations, corruption propagation, integrity checks, and safe snapshot import/export.
- **Queries:** `projectatlas-service` becomes the owning boundary for graph ranking, traversal, architecture, impact, similarity, lexical/semantic search, and optional federation; CLI and MCP remain thin adapters.
- **Agent surfaces:** additive CLI commands and `atlas_*` MCP tools expose typed provenance, coverage, pagination, truncation, and task-progress information while `atlas_settings` gains graph schema, accepted capability-set, backend, and pack-state reporting; the existing atlas-first funnel remains unchanged.
- **Installation:** the official plugin-store install/update/rollback/remove lifecycle gains clean-host, stale-runtime, offline, failure-recovery, and first-project E2E gates on supported platforms while preserving project-local databases and authored metadata.
- **Dependencies and releases:** grammar and optional model assets require lock manifests, license/security review, checksums, SBOM coverage, cross-platform builds, hard size/resource budgets, and feature-pack decisions. Canonical Rust crates are preferred; broad grammar/WASM, ANN, and model runtimes remain separate pack dependencies and processes rather than transitive default-core artifacts, while ProjectAtlas-owned indexing and resolution logic remains Rust.
- **Testing:** fixtures expand to a multi-language public corpus plus mutation, corruption, Unicode-path, crash-recovery, incremental-equivalence, clean-host plugin-store installation, and pinned-baseline comparison harnesses with explicit timeouts.
- **Documentation:** architecture, language support matrix, benchmark methodology/results, privacy/security model, migration/rollback guidance, and agent integration docs must be updated before any phase is advertised complete.
