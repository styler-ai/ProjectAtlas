# Proposal

## Why

An agent inspecting source relations must be able to find the specification that actually documents that source and each endpoint's accepted purpose. Generic Markdown extraction, a supported `documented_by` view, or a passing synthetic fixture does not prove that actual OpenSpec files are correctly connected and visible in the normal agent workflow.

## What Changes

- Verify representative actual OpenSpec/source pairs through the existing canonical `documents` fact and its inbound `documented_by` view, including file-first traversal, exact provenance and multiple applicable specifications.
- Make the supported source-dependency plus specification navigation recipe explicit, including classification, direction, filters, bounds and continuations. Preserve omitted-selection legacy behavior; require no new endpoint or broad default change merely for this verification.
- Verify that source and document purposes are projected from their own authoritative owners at query time, with correct approval/provenance and truthful unavailable or non-local states.
- Distinguish missing authored references, unsupported syntax, stale indexes, partial coverage, parser/resolver defects, filtering and purpose projection. Fix confirmed defects at their existing owners, including equivalent affected variants.
- Reconcile real repository references, focused skill guidance, architecture and causal full/incremental/platform proof. If the existing implementation is correct, retain it and add only missing references, guidance or proof.

## Capabilities

### New Capabilities

- `specification-source-traceability`: observable, purpose-aware, bidirectional specification/source navigation and its end-to-end acceptance contract using the existing documentation graph.

### Modified Capabilities

None. Existing classification, canonical document relations, purpose ownership, bounds, root isolation and publication contracts remain constraints. Their implementation is reused rather than duplicated.

## Impact

Markdown extraction in `projectatlas-symbols`; document normalization/resolution/publication in the CLI runtime; normalized graph and purpose reads in `projectatlas-db`; bounded relation composition in `projectatlas-service`; CLI JSON/TOON and MCP; actual OpenSpec references, tests and focused shipped graph-navigation guidance. No new dependency, crate, schema, database, persisted inverse edge or inference service is planned.

## Release Scope

Issue #751 targets **v0.5.1-rc2**, not RC1. This packet specifies the work completely but does not authorize implementation or publication during RC1 acceptance. Keep #751 outside the current RC1 milestone/blocker graph. Task 1.2 activates it only after the published RC1 baseline has been verified and the next-candidate scope of the existing v0.5.1 release owner #715 is reopened and reconciled. That activation is an explicit prerequisite, not an unresolved functional design question.

The user's later explicit RC2 direction applies to this specification/source relation visibility and purpose-integration topic, including confirmed defects found while verifying it. It is a topic-specific exception to the general RC1 self-hosting rule; unrelated defects discovered in stable 0.5.0 retain that rule. Do not silently expand this exception or reclassify #751 as an RC1 blocker.

## Non-Goals

- Inferring implementation ownership or conformance from prose, embeddings, matching names, file age or purpose text.
- Treating every documentation mention as a normative specification or forcing one specification per source file.
- Replacing canonical `documents` storage, adding a second inverse graph, or copying separately authored purposes into graph rows.
- Resetting an atlas, merging sibling state, narrowing supported local paths, or hiding missing/partial/stale evidence.
- Expanding this issue into general post-compaction or dead-code-review skill design; that separate requested guidance is owned by RC1 release/skill work.
