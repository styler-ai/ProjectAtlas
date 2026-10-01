## Context

RC2 packaged plugin state can advance independently of the installed executable and Codex MCP registration. Codex marketplace installation has no ProjectAtlas-owned post-install transaction that can atomically update those external layers. Its trusted SessionStart hook and the existing installers are the supported integration surfaces. Separately, DOCX `w:sym` currently returns `UnsupportedDocxInput`, and the PDF Wasmi extractor returns a finite `ExecutionFuel` limit. CLI text-index and symbol paths translate these to scan-fatal errors, leaving no publication. PDF per-file text byte settings do not control execution fuel.

## Goals / Non-Goals

**Goals:** Make plugin/runtime/MCP mismatch explicit with an actionable version-matched repair path; keep document parsing bounded and truthful; allow usable repository evidence when one document cannot be fully extracted under an explicitly incomplete coverage contract; preserve last-valid publication and authored SQLite state; prove Windows, Linux, and macOS installed behavior before RC3 release acceptance.

**Non-Goals:** Claim that Codex's marketplace transaction installs external binaries, guess arbitrary font-specific Unicode mappings, raise/remove parser ceilings without measured valid-input evidence and retained bounds, publish partial content as complete, reinitialize a user database, or promote stable v0.5.0.

## Decisions

### Keep plugin installation and integration readiness distinct (#620)

The plugin uses its supported trusted startup/diagnostic surface and existing installer to compare version-matched plugin, direct CLI, generated config, and registered MCP identity. The installer adds a missing registry entry only after an unambiguous inventory, then writes a small user-state readiness receipt after verifying all layers. The receipt pins root, version, registered runtime, installer-owned direct CLI, and host-file hashes plus only the non-secret MCP identity fields; it never duplicates MCP environment values. On Windows, the direct CLI may be the byte-identical stable mirror while MCP stays pinned to the versioned runtime. The read-only hook validates this receipt and resolves the direct CLI before running any runtime command, without executing `codex` from project-influenced PATH. Project-local Codex config overrides or changed/missing proof yield an incomplete/actionable state and the packaged installer/verification commands. The installer remains the only mutation owner; no startup hook silently updates PATH, MCP, or project databases. An already-running host may require restart after the external state converges. Changing Codex marketplace success semantics was rejected because the plugin does not own that command or a post-install event.

### Treat document parser limits as file-local coverage, not a global work-budget increase (#624, #625)

First reproduce the PDF fuel input and DOCX symbol package in isolated fixtures. Follow validated package relationships for rendered document stories: main body, headers, footers, footnotes, endnotes, referenced comments, frames, and nested text boxes. Unreferenced glossary content is not rendered; a referenced glossary or subdocument that cannot be safely examined must leave explicit incomplete coverage, with no external fetch or path escape. Retain each examined part's identity and reject unsafe or malformed targets. For every syntactically valid rendered `w:sym` admitted within retained resource bounds, preserve exact font/code, story part, and occurrence identity through durable queryable publication, and decode Unicode only where a deterministic mapping is established; an unknown font mapping is not grounds to lose the symbol or the rest of the repository. Any package-reachable rendered story not examined, or live page-number/date field needing evaluation without an admitted literal cached result, must produce typed file-local incomplete text coverage rather than a whole-document completeness claim. Batch reachable note/comment IDs from linked stories before inflating shared item parts; retain a finite repeated-decompression-work ceiling as a typed file-local fail-safe. Measure PDF parser work and fix a demonstrated inefficiency; a finite fuel increase is allowed when representative valid-input and wall-time/memory/output/cancellation proof justify it. Accepted parser output/fact/memory/work or PDF-fuel limits after safe package admission become file-local typed incomplete coverage in both text and symbol paths. Unsafe package/ZIP input limits, malformed input, I/O, source change, cancellation, and shared deadline remain generation-fatal. The repository generation may publish only if every other admitted file is verified and every incomplete document is explicitly represented as such; queries must not infer that missing document facts or text are absent. Reuse existing skip/provenance fields where sufficient. If durable coverage cannot be represented without a schema change, make the smallest compatible migration with rollback proof, not an in-memory-only label.

### Preserve atomic generation ownership

Stage parsing before the SQLite write transaction. For PDF resource stops, a guest-to-host admission marker follows bounded object, stream, page-tree, direct-page syntax, and reachable Form syntax/resource validation. Fuel before that marker is fatal; accepted fuel, output, or configured linear-memory limits afterward record no PDF text and explicitly leave unexamined rendering semantics unknown. Stack overflow and detected malformed or unsupported text semantics stay fatal. A cancellation, source change, I/O fault, malformed package, or publication fault preserves the prior complete generation and authored state. A supported bounded-resource/unsupported-document outcome is not a fabricated successful parse. A later repaired document replaces its incomplete status through the normal incremental refresh. Existing fail-closed malformed XML, package-integrity, wrong-root, and incompatible-schema behavior stays intact.

### Keep issue boundaries independent and the release owner read-only for product fixes

#620 owns installer/plugin host convergence. #624 owns DOCX extraction and document coverage. #625 owns PDF fuel behavior and the same coverage contract; the first document issue to land establishes any shared representation, and the second refreshes from main rather than duplicating it. #492 owns only the exact RC3 package/platform acceptance after all children close. No new serial dependency is declared until one issue's implementation actually requires a shared landed baseline.

## Risks / Trade-offs

- [A startup diagnostic is unavailable when hooks are disabled/untrusted] → Document this host state and retain an explicit manual check/installer route; never claim readiness from plugin cache alone.
- [A file-local skip could be misread as complete source evidence] → Persist/query typed incomplete coverage and test search, summary, graph, health, and MCP output before publication.
- [A parser/resource fallback could hide malformed or changing input] → Accepted parser output/fact/memory/work or PDF fuel limits, unresolved DOCX symbol mappings, safely unexamined story types, and live page-number/date fields requiring evaluation on a safely admitted package become typed document-local incomplete coverage. Malformed or unsafe package/ZIP limits, I/O, source change, cancellation, and shared deadline remain generation-fatal.
- [Shared document coverage changes conflict across issues] → Land one owning boundary first; rebase and reuse it for the other, with independent causal tests.
- [Mac-only reproduction can overstate Windows impact] → Exercise the same fixtures on supported hosted/packaged platforms and label unobserved behavior as pending until readback.

## Migration Plan

1. Map the three original defects #620, #624, and #625 to exact OpenSpec task slices; add the separately accepted #627 map change; reopen #492 and reconcile the native/mapped release graph.
2. Implement one issue/PR at a time against current main, preserving the database and complete-generation contract; independently review each completed boundary.
3. After all children merge, update version-owned artifacts to `0.5.0-rc3`, run the complete installed upgrade and CLI/MCP/host/platform inventory from an exact candidate, and publish a non-Latest prerelease only with separate release authorization.

## Dependencies / Cross-Issue Impact

#620, #624, #625, and the separately accepted #627 are independent direct children of #492. A shared document-coverage representation first landed by #624 is a baseline for #625 only if that implementation chooses the same storage boundary; then #625 refreshes from accepted main. #602 stable promotion remains separate.

The separate #627 OpenSpec change owns the fileless map contract; the RC3 release owner must prove the installed map CLI/MCP behavior without assuming a TOON snapshot file.

## Open Questions

None.
