## Why

The published `v0.5.0-rc2` leaves three confirmed user-visible gaps: Codex marketplace updates can strand an older native CLI/MCP runtime (#620), and two document inputs can prevent any initial repository publication (#624, #625). The v0.5.0 release owner must therefore accept separate fixes and repeat exact installed-product proof before an RC3 candidate is eligible.

The separately requested map cleanup (#627) is also in RC3 scope: retain the core map routes while retiring only their generated legacy TOON snapshot.

## What Changes

- Give a marketplace-installed plugin a truthful, actionable runtime/MCP readiness outcome, with a supported version-matched convergence path and database preservation (#620).
- Correct DOCX font-symbol extraction/coverage so one unsupported glyph does not silently corrupt document evidence or unnecessarily prevent safe source indexing (#624).
- Correct PDF execution-fuel admission/diagnostics so the bounded parser identifies the offending file and the index remains truthful and recoverable (#625).
- Keep `projectatlas map` and `atlas_map` while replacing their legacy file write with an inline map response under the separate `retire-generated-legacy-map-file` change (#627).
- Reopen #492 as the `v0.5.0-rc3` acceptance owner, retain the three original defects and separately accepted #627 as direct children/blockers, and retain stable promotion in #602. RC3 publication is a separate release-stage action, not an effect of this planning change.

## Capabilities

### New Capabilities

- `rc3-runtime-readiness`: plugin, direct CLI, generated host config, and Codex MCP runtime identity/convergence contract.
- `rc3-document-index-continuity`: bounded DOCX/PDF extraction, explicit per-document coverage, and atomic source-index publication/retry contract.
- `rc3-release-delivery`: issue graph, exact candidate, cross-platform installed acceptance, and prerelease readback contract.

### Modified Capabilities

None. This change defines RC3 remediation requirements without changing the stable `v0.4.5` public contract.

## Impact

The existing plugin/installer/host integration, Rust document parser, CLI text-index, coverage/publication, CLI/MCP map routes, tests, platform package gates, OpenSpec issue map, and #492 release graph. No new crate, parser dependency, database reset, or stable promotion is planned. Ready for issue-scoped implementation after task mapping and issue graph synchronization.
