# Design

## Context

See proposal.md for the observed macOS ARM failure. The Unix fixture selects `sha256sum` or `shasum`, canonicalizes the selected host tool, and links only its generic name in a temporary curated tool directory. The copied hook then runs with that directory on PATH. The failing macOS wrapper reports no available alternatives at the relocated path.

## Goals / Non-Goals

Keep the real selected tool executable after staging and retain every curated-path, checksum, readiness, stale-state, and non-mutation assertion. Do not change production checksum routing or claim that the synthetic directory proves real Nix-host behavior.

## Decisions

Repair the existing fixture's staging boundary. When the selected `shasum` dispatcher requires versioned siblings, stage those real companion files beside its generic link under their original names. Discover them from the selected tool's canonical host directory; ordinary standalone tools need no extra files. Confirm the precise host dependency with the native failing regression rather than assuming the correction has passed.

Keep the fixture's curated PATH and real checksum invocation. Replacing the checksum tool with a fake or falling back to the full ambient system PATH would remove the boundary the test is meant to exercise. No new framework, tool selector, or product fallback is justified.

Refresh the existing lifecycle frozen-source hash owners after the code changes. The focused command is `cargo test --locked -p projectatlas-cli --all-features --test e2e_lifecycle bundled_hook_distinguishes_ready_and_stale_mcp_without_mutation -- --exact --nocapture`. Causal native macOS proof must exercise the same real `shasum` dispatcher selected in the failing installed job; a run selecting `sha256sum` does not prove the companion repair. Preserve the original isolated host PATH for standalone installed execution, without introducing a product setting. Required affected hosted checks must also pass before accepting the fix.

## Risks / Trade-offs

- The inferred companion dependency could be incomplete: confirm it on the affected native macOS host and preserve the original failing log.
- A fake tool or ambient PATH fallback could conceal the failure: keep real tool staging and the existing curated-path assertions.
- Frozen metadata could drift: update both existing lifecycle source-hash owners and run their causal checks.
- A synthetic tool directory could overstate platform coverage: report native macOS fixture proof separately from real Nix-host acceptance.

## Migration Plan

Publish the reviewed specification before admitting implementation. Land the test-only fix with native causal proof, then let #492 freeze a fresh candidate. No installation or database migration is introduced. Reverting the correction restores the demonstrated proof failure.

## Dependencies / Cross-Issue Impact

#669 owns fixture staging and its causal proof. It has no code prerequisite on #668; delivery finishes the already-started #668 boundary first. #492 is the release owner, depends on both accepted fixes for fresh complete platform acceptance, and closes last. Architecture is unchanged; the issue records a reasoned N/A.

## Open Questions

None.
