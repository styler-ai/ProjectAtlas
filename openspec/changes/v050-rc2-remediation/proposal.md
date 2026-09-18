## Why

The published `v0.5.0-rc1` audit found six reproducible defects across plugin startup guidance, classified discovery, package documentation, the Windows `atlas` entrypoint, and optional-parser-pack lifecycle. They prevent RC1 from satisfying its release promise, so the existing v0.5.0 release owner must reopen, accept the fixes, and publish a new non-Latest prerelease only after a complete installed-product RC2 readback.

## What Changes

- Add trusted, bounded ProjectAtlas plugin lifecycle guidance for supported Codex session starts without mutating a project or duplicating the full skill (#604).
- Admit only real indexed files to classified CLI/MCP discovery; synthetic companion paths never make a request fail (#605).
- Make the packaged setup guide version-matched and teach the installed `atlas` route, including the inherited-PATH restart boundary (#606).
- Accept the published versioned optional-parser archive name while retaining platform, digest, content, and containment checks (#607).
- Replace the Windows `atlas.cmd` argument-loss boundary with a native argument-preserving route and prove JSON-bearing CLI parity (#608).
- Diagnose and repair the Windows containment-admission failure from the final parser-pack asset without bypassing isolation, then preserve safe retry and no-partial-selection behavior (#609).
- Reopen #492 and the `v0.5.0-00` release graph, then freeze, prove, publish, and independently read back `v0.5.0-rc2` as a non-draft prerelease while `v0.4.5` remains Latest.

## Capabilities

### New Capabilities

- `plugin-lifecycle-instructions`: trusted, version-matched ProjectAtlas session guidance through the supported Codex plugin hook contract.
- `classified-file-discovery`: real-file-only classified file selection with CLI/MCP parity.
- `installed-cli-and-package-guidance`: version-matched packaged instructions and an argument-preserving Windows `atlas` route.
- `optional-parser-pack-admission`: published-asset filename compatibility and bounded Windows containment admission.
- `rc2-release-delivery`: reopened v0.5.0 remediation hierarchy and complete `v0.5.0-rc2` installed-product publication proof.

### Modified Capabilities

None. This change corrects additive/reliability behavior without changing the supported stable `v0.4.5` contract or promoting `v0.5.0` to Latest.

## Impact

- `plugins/projectatlas` packaging and its version-matched skill, hook, installer, and host proof.
- CLI/MCP file discovery, Windows launcher installation, release README/package assembly, optional-parser lifecycle/supervisor, and their focused E2E coverage.
- `openspec/issue-map.json`, the reopened `v0.5.0-00` native issue graph, RC2 release notes, checksums/assets, and four-platform installed-product proof.

## Non-Goals

- No stable `v0.5.0` promotion, Latest change, new parser provider, containment bypass, database migration, new crate, or generic launcher framework.
- No project initialization, scan, repository-instruction replacement, or automatic trust grant from the plugin hook.
- No release of a package whose exact RC2 assets have not passed the resolved lifecycle and installed-route checks.
