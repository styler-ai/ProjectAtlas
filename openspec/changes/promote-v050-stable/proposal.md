# Publish ProjectAtlas 0.5.0

## Why

The accepted RC4 product has passed the complete command and installed-runtime checks. Publish the final 0.5.0 release and make the repository front page explain the product clearly to a first-time visitor.

## What Changes

- Promote the accepted RC4 product to stable 0.5.0 through the existing native release workflow, with independently verified packages, upgrade behavior, checksums, and GitHub Latest selection.
- Update version-owned package, plugin, installer, documentation, and supported workflow identities together.
- Give stable acceptance its own release graph and milestone, preserving RC publication and historical stable-repair admission.
- Rebuild the README around the persistent repository map: orientation, responsibility, connected source, graph analysis, native CLI, MCP, worktrees, documents, freshness, and token impact.
- Keep Ani and the exact sentence: "Every file not opened. Every folder not explored. Tokens saved."
- Use a clear visual hierarchy and reproducible charts whose labels distinguish observed payload sizes, modeled avoidance, and historical results. Retain links to methods and limits.
- Make marketplace/plugin support and agent-led installation prominent, including restarting the terminal and coding environment after installation.

## Capabilities

### New Capabilities

- `release-publication`: independent stable acceptance selection, preserved RC and historical-repair admission, and publication-owner lifecycle.

### Modified Capabilities

None. Existing runtime, storage, language, and token-accounting behavior remains authoritative. This change is ready for implementation under issue #602.

## Impact

Release identities and notes, the README and its versioned visual assets, supported installation guidance, and the existing OpenSpec/IssueOps mapping. No schema migration, new dependency, or product feature is introduced.

## Non-goals

Do not implement the 0.6.0 indexing plan, add npm distribution, claim universal token or latency savings, advertise unsupported languages or equivalent semantics across all parsers, copy third-party artwork or wording, reopen completed RC issues, or change unrelated working trees and databases.
