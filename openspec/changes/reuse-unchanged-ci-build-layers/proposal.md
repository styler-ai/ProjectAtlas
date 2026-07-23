## Why

The optional parser-pack workflow discards reusable Cargo build state on every runner, so unchanged dependencies are rebuilt even though the ProjectAtlas candidate itself must remain freshly compiled and fully verified. Issue #341 is now in the v0.4.0 stabilization path so the remaining issue #308 iterations do not repeatedly pay that avoidable cost.

## What Changes

- Reuse only digest-addressed Cargo dependency build state across trusted workflow runs.
- Rebuild every ProjectAtlas-owned crate from the exact checked-out candidate before constructing parser-pack outputs.
- Treat restored cache bytes as untrusted, validate their bounded filesystem shape, and never cache release archives, receipts, secrets, ProjectAtlas databases, or authored workspace state.
- Add an explicit clean-construction mode that bypasses cache restore/save for final release acceptance.
- Record cold and repeated-run timings, cache disposition, and deterministic invalidation inputs for Linux and Windows.

## Capabilities

### New Capabilities

- `ci-build-layer-reuse`: Safe, measured reuse of unchanged CI dependency build layers without weakening exact-candidate or clean-release proof.

### Modified Capabilities

None.

## Impact

The change affects the optional parser-pack GitHub Actions workflow, its contained construction scripts, focused workflow-policy tests, the owning architecture documentation, and IssueOps/OpenSpec mapping. It uses the existing Cargo workspace and official GitHub Actions cache primitives; no new ProjectAtlas crate, cache service, or runtime dependency is introduced. This change is ready for implementation as part of v0.4.0 stabilization.

## Non-Goals

- Reusing ProjectAtlas-owned candidate binaries or published parser-pack outputs.
- Weakening exact-head, offline construction, native-import, license, containment, lifecycle, package, or fresh-runner verification.
- Making cache availability a correctness requirement.
- Retrofitting unrelated workflows without measured benefit.
