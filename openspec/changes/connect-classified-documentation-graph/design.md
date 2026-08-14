## Context

The current language registry owns deterministic filename/extension/content detection and parser/structural-summary ownership. Markdown and MDX structural summaries already use `pulldown-cmark`, while the symbol engine deliberately reports Markdown symbol extraction unavailable. The normalized graph stores canonical entity and relation facts with typed resolution and reverse traversal, but `ExtendedRelationKind` has no document relation. SQLite schema 16 owns the active atlas through one append-only migration inventory. File/search/summary/purpose/relation/analysis adapters expose bounded results but have no content-role selection.

#430 separately owns worktree lifecycle: an immutable exact-tag stable-main seed, a private ignored writable atlas in each checkout, exact-root rebinding, two-sided branch refresh, and explicit per-call `project_path` for concurrent hosts. #440 must add semantic derived state inside that lifecycle; it must not add another shared database or let two worktrees write one graph.

## Goals / Non-Goals

**Goals:**

- Make source, documentation, configuration/data text, other text, and opaque files visibly distinct everywhere agents navigate.
- Create only parser-proven explicit document facts with exact selectors and typed uncertainty.
- Preserve legacy unfiltered candidates, ranking, relation families, purposes, and source parsing.
- Reuse one stored `documents` fact for outbound and inbound navigation.
- Publish classifications, headings, and relations atomically and invalidate the exact affected closure.
- Hydrate stable-main document facts through #430 while keeping each checkout's branch graph private and current.
- Upgrade v0.4.4 ordinary and linked-worktree users without configuration or database surgery.
- Keep extraction, SQLite access, traversal, memory, and output bounded at intended repository scale.

**Non-Goals:**

- Semantic/embedding similarity, inferred documentation ownership, or LLM-generated links.
- A general prose knowledge graph, full MDX/JSX/RST parser, external language server, or new parser framework.
- Treating documentation as runtime truth, configuration/data as source by implication, or purpose text as classification authority.
- Indexing ignored, secret-bearing, vendor/cache, oversized, or otherwise ineligible content.
- Guessing generated status from filenames; existing ignore/admission evidence remains authoritative.
- Storing a second inverse relation, sharing one writable atlas across worktrees, or merging branch databases.
- Rewriting source/documents, repairing links, or changing Git/worktree state.

## Decisions

### Keep one closed content-classification authority

Add `ContentClassification` with exactly `Source`, `Documentation`, `ConfigurationData`, `OtherText`, and `Opaque`. Known language rows gain one registry-owned classification. Markdown/MDX are documentation; declared program/template/style languages are source; declared structured configuration/data families use configuration/data. Otherwise eligible valid UTF-8 is other text and invalid/binary content is opaque. Ignored or otherwise ineligible files do not receive a published row. No `Generated` variant is invented because ProjectAtlas has no authoritative generated-file classifier beyond existing ignore/admission evidence.

Purpose remains authored responsibility. Classification is derived file role and survives in output independently of missing, suggested, stale, or approved purpose text.

Add caller-visible selection values `source`, `documentation`, and `both`, plus an internal `UnspecifiedLegacy` state when the request omits the field. `both` means source plus documentation. Configuration/data and other text remain visible through the omitted legacy-compatible path and retain explicit classifications. Opaque files remain discoverable metadata when already admitted but never become searchable text.

### Move reusable Markdown structure extraction to the symbol owner

Use the existing workspace-pinned `pulldown-cmark` parser and its offset iterator in `projectatlas-symbols`. The extractor returns bounded headings and explicit link/path candidates with parser provenance, exact byte/line ranges, completeness, and limit states. Add a `Markdown` symbol-parser owner and `Heading` symbol kind. The existing structural summary reuses the same heading facts rather than maintaining a competing heading parser.

Markdown and the Markdown portion of MDX are supported. Raw JSX/HTML, runtime expressions, templated destinations, and RST structure remain unsupported/partial rather than guessed. Headings inside code fences are ignored by the parser. Image targets do not create document relations. Reference links resolve only through the parser's explicit destination. Backticked paths are candidates only when the complete code span is one normalized repository-relative file selector; ordinary identifiers, calls, shell fragments, URLs, fragments alone, and prose are not candidates.

Per-file limits bound admitted bytes, headings, links/path candidates, label bytes, and retained evidence. Hitting a limit records partial coverage and truncation; it does not publish fabricated completeness.

### Resolve exact local identities and store one document relation

Add `Documents` to the existing extended graph family. A validated link is sourced from its nearest enclosing heading entity when one exists and from the Markdown file otherwise; its validated indexed file/heading target is the relation target. Exact link occurrences retain their typed source span, while heading symbols retain one-based lines plus parser-supplied UTF-8 byte and Unicode-column selectors. `documented_by` is only the inbound adapter label/view of the same stored fact. Same-file links are navigation self-references and are suppressed. Duplicate canonical source-target facts collapse while links under distinct headings remain distinct; cycles remain ordinary bounded graph cycles.

Resolution starts from the selected document's normalized repository directory, strips an optional query/fragment for file identity while retaining a valid heading fragment as target evidence, normalizes separators without accepting drive/UNC paths, rejects root/symlink escape, and resolves only indexed exact-case identities under the selected root. Case-fold-only collisions never choose a winner. Directory targets, external schemes, anchors without a file, dynamic/template destinations, unsupported encodings, and non-static inline code remain unsupported.

Unresolved reasons are closed and content-free: `missing`, `ignored`, `outside_root`, `case_conflict`, `unsupported`, and `no_static_target`. Resolution uses the same effective root scan policy for existing and nonexistent targets, including `.gitignore`, `.ignore`, Git excludes, and stricter Atlas exclusions. One bounded target-state memo is revalidated before publication so symlink/type/policy changes cannot race an affected generation into stale success. Evidence retains only bounded repository-relative selectors/digests, never absolute host paths or ignored/private content. Exact existing graph resolution/completeness/provenance types remain the common envelope.

### Preserve legacy queries while making classification explicit

Every affected row gains an additive classification for file endpoints. When content selection is omitted, files/search/purpose candidates and ordering remain byte-for-byte compatible except for additive fields, and new document relations do not enter an unfiltered legacy relation traversal unless `documents` or a content selection is explicitly requested.

With explicit selection, the shared service predicate filters file candidates, traversal anchors, and expansion frontiers before adapter truncation. An explicitly requested `documents` traversal may return the cross-class target needed for doc-to-source or source-to-doc navigation even when the frontier selection names one side; the endpoint classifications remain visible. This avoids suppressing the purpose of the relation while preventing unrelated frontier expansion.

CLI and MCP accept the same closed strings and errors. Existing callers that omit the field retain current defaults. Exact next calls include the addressed root, file/heading selector, direction, relation, selection, bounds, and freshness/completeness needed to continue safely.

### Add one active-atlas classification table and reuse graph storage

#440 owns the append-only active-atlas schema 16-to-17 transition. #430's new continuity database has independent schema ownership and must not consume this version. Add one constrained `file_content_classifications` table keyed to admitted file nodes, with the five closed values and an index supporting classification-plus-path candidate access. Rebuild the derived symbol table once to add nullable, all-or-none byte/column selector columns; rebuild the disposable graph projection once to add the constrained unresolved-document reason. Existing graph entity/relation and symbol tables then store heading and document facts; no second document table or inverse edge table is added.

Classification writes are prepared/batched and participate in the same transaction as node/symbol/graph publication. Full publication, incremental refresh, deletion, rollback, repair, and derived cleanup cannot expose a generation whose classification rows and document facts disagree. Migration classifies current rows deterministically from persisted language plus bounded current-source admission during the normal refresh; it never rewrites authored purposes. A v0.4.4 database is backed up/preflighted and migrated through the existing inventory; newer/incompatible, corrupt, busy, or interrupted state follows existing typed refusal/recovery.

Query-plan assertions cover classification/path selection and inbound `documents` traversal. Services batch endpoint classifications with result pages and do not issue one query per result.

### Compose with #430 seed and per-worktree ownership

#440 owns the live schema-17 classification, heading, document, completeness, and unresolved facts. #430 transports their portable subset through the derived-state allowlist. The clean-main sealer includes them only after a complete #440 generation and excludes all local identities, telemetry, sessions, tasks, absolute paths, caches, writable sidecars, and ephemeral exact-source byte/column selectors as already specified. Stable heading identity and canonical document facts remain sufficient for portable navigation; each checkout republishes exact selectors from its own current bytes.

Hydration copies/reflinks the immutable exact-tag seed into one staged private active database, rebinds that checkout, then #440's ordinary incremental closure adds/changes/removes classifications, headings, and document relations for both sides of the branch diff. The seed is never opened writable. Existing compatible local databases win over hydration and migrate locally. Missing/offline/incompatible seeds use the ordinary local migration/init/full-build path.

Two worktrees may therefore start with identical stable-main document facts and immediately diverge after branch-local refresh. Explicit per-call `project_path` captures the correct root/database/generation before traversal; no classification, unresolved selector, relation, purpose, or next call may come from a sibling checkout.

### Keep adapter and guidance changes additive

Files, search, file summary, purpose queue/review/set responses, symbol/relation pages, graph analysis, settings/capability reports, CLI JSON/TOON, and MCP schemas expose the same classification contract where a file endpoint appears. Purpose mutation does not accept a classification override. The shipped ProjectAtlas skill teaches agents to choose source, documentation, or both, treat documentation as guidance rather than source truth, and follow explicit `documents`/inbound `documented_by` before exact slices.

### Bound intended-scale work

Classification is constant work per admitted file after existing detection. Markdown extraction is linear in admitted bytes and capped before retained facts grow. Resolution uses normalized maps/sets and indexed SQLite keys rather than pairwise file scans. Incremental refresh invalidates changed documents plus the existing reverse affected-source closure; deletion/rename removes old heading/relation keys in the publication transaction. Traversal remains bounded by existing rows/depth/bytes/deadline/cancellation budgets. Representative measurements compare clean/full and one-document incremental runs for CPU, wall time, allocations/RSS, SQLite statements/lock time, WAL/write amplification, persistent bytes, seed size, and output bytes.

## Risks / Trade-offs

- **Classification drifts across adapters** -> Keep one core enum/registry projection and batch the stored value into every file-bearing response.
- **Markdown syntax creates false links** -> Accept only parser-emitted destinations and whole-span repository-path code selectors; retain typed unsupported evidence instead of guessing.
- **Inverse relations disagree** -> Store only canonical outbound `documents`; derive `documented_by` through inbound traversal.
- **A filter changes legacy ranking** -> Represent omission separately and freeze exact unfiltered candidate/order fixtures.
- **Case, symlink, or root handling leaks paths** -> Resolve through selected-root indexed identities, reject ambiguous/escaping targets, and return bounded repository-relative reasons only.
- **A changed document leaves stale reverse edges** -> Publish classification, symbols, and affected relation closure in one generation transaction with delete/rename/crash tests.
- **Seed facts contaminate a branch worktree** -> Refresh both sides of the seed/current diff into a private rebound database before reporting a complete generation.
- **Schema 17 conflicts with #430** -> #440 owns the active-atlas migration; #430 uses its separate continuity schema and consumes schema 17 only through its portable allowlist.
- **Large docs or link fan-out inflate cost** -> Enforce per-file and per-request caps, indexed access, batching, cancellation, and representative profiles.

## Migration Plan

1. Land #430's lifecycle contract and keep its active-atlas seed allowlist ready to admit #440's schema-17 derived facts; do not publish the RC seed yet.
2. Land core classification/document contracts and the SQLite 16-to-17 migration with real write/read/rollback/query-plan coverage before services or adapters depend on it.
3. Land the shared Markdown extractor, exact resolver, and atomic full/incremental graph publication.
4. Add compatibility-preserving service selection and CLI/MCP projections, then shipped skill/user guidance.
5. Run ordinary-checkout and linked-worktree E2E: migrate v0.4.4, seal stable-main seed, hydrate two private databases, diverge documentation/source, refresh, and prove isolated bidirectional traversal.
6. Seal and publish the final v0.4.5-rc1 seed only from the complete merged main generation. Rollback before publication uses the existing database backup/recovery path; a failed candidate never replaces a valid local atlas or stable seed.

## Dependencies / Cross-Issue Impact

#440 depends on #430 for immutable seed transport, per-checkout writable database lifecycle, exact-root rebinding, branch-difference refresh ownership, explicit `project_path` routing, `.gitignore`, and v0.4.4 upgrade orchestration. #430 depends on #440 to define schema-17 classification/heading/document facts that its final v0.4.5-rc1 portable allowlist, complete-generation proof, hydration equivalence, skill, and worktree E2E must include. #448 prevents either issue from remaining in `v0.4.5-00` until these mapped contracts and final architecture tasks are ready.

The ownership boundary is strict: #430 moves and activates verified derived state; #440 defines and refreshes the classified documentation semantics inside each selected active atlas. Neither issue may introduce a shared writable graph, merge branch databases, or duplicate the other's root/selection logic.

## Open Questions

None.
