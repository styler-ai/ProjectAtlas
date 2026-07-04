---
name: codex-coding-plugin
description: Build, review, or fix stable Codex plugins, especially plugin runtime installers, marketplace update flows, MCP config generation, skill artifacts, version convergence, stale cache repair, and fake-host tests for Codex plugin releases.
---

# Codex Coding Plugin

## Goal

Make Codex plugins converge to one verified version across the plugin manifest, installed plugin metadata, runtime binary, generated MCP config, global MCP registry, skill files, and host cache.

## Workflow

1. Treat the plugin manifest version as the release contract, but verify it against the runtime with `runtime-info` before writing configs.
2. Prefer official host commands for plugin and marketplace state. Inspect host cache files only after the host reports a source path and only after confirming the source is the official plugin.
3. Never trust one version signal alone. Compare at least:
   - plugin list version,
   - reported plugin source manifest version,
   - runtime `runtime-info` version,
   - generated MCP `--require-version`,
   - global MCP registry runtime and database path when the host exposes one.
4. If an official marketplace cache is stale, repair it automatically with the host's remove/add or upgrade flow, then re-read the host state.
5. Fail only after repair was attempted and the verified state still mismatches. The normal official path should work, not just report drift.
6. Preserve user-managed environments. If the marketplace source is not clearly the official plugin source, do not mutate it.
7. Pin generated MCP configs to absolute verified runtime, DB, and config paths. Avoid PATH-only fallbacks for installed plugin configs.
8. Keep installer output concrete: say which version/path was verified, repaired, skipped, or still mismatched.

## Version Checks

Use a single small helper per host to read the host-reported plugin object. From that object, derive the source path and source manifest version. The common bug is checking `codex plugin list` version, then running installer code from a stale source path.

Required guard for Codex plugin repair:

```text
if plugin_list.version == expected
and marketplace_ref == expected_tag
and reported_source_manifest.version == expected:
    no-op
else if official ProjectAtlas marketplace:
    remove/add or upgrade the plugin
    re-read plugin list and source manifest
    require both to match expected
else:
    skip mutation with clear user-managed message
```

## Tests

Use fake host commands instead of real Codex state. The minimum regression matrix is:

- official marketplace, stale plugin list version -> repairs;
- official marketplace, current plugin list version but stale reported source manifest -> repairs;
- official marketplace, current plugin and source manifest -> no-op;
- non-official marketplace -> no mutation;
- failed repair -> restores when possible and reports the mismatched field;
- generated MCP config launches the verified runtime.

For installer tests, assert exact command fragments for remove/add/update calls and inspect the generated config files, not just process success.

## Release Gates

Before calling a Codex plugin release stable, verify:

- plugin manifest version matches release tag;
- runtime `runtime-info` reports the release version, ProjectAtlas identity, MCP capability, and TOON output;
- installer writes project-local MCP config with `--require-version`;
- installer repairs or reports global MCP registry drift;
- installer verifies the skill artifact path/version when Codex exposes it;
- release tests run on Windows and POSIX paths when both installer scripts exist.
