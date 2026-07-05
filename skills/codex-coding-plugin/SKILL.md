---
name: codex-coding-plugin
description: Build, review, or fix ProjectAtlas plugin/runtime installer integration for Codex, Claude Code, and OpenCode, especially version convergence, stale ProjectAtlas cache repair, MCP config generation, skill artifacts, host smoke tests, and fake-host tests for ProjectAtlas releases.
---

# ProjectAtlas Plugin Host Integration

## Goal

Make ProjectAtlas releases converge to one verified version across the repository release tag, ProjectAtlas plugin manifests, native runtime binary, generated MCP configs, Codex marketplace/plugin cache, Codex global MCP registry, packaged ProjectAtlas skill files, Claude Code config, OpenCode config, and any old ProjectAtlas information that can survive in a host cache.

This is a repo-local ProjectAtlas skill. Do not generalize these rules into product claims for unrelated plugins. The point is to keep this repository's supported host integrations from reintroducing stale ProjectAtlas versions or paths.

## Workflow

1. Treat the ProjectAtlas release tag and plugin manifest versions as the release contract, but verify them against the native runtime with `projectatlas --format json runtime-info` before writing configs.
2. Never trust one version signal alone. Check every ProjectAtlas surface that can keep stale data:
   - `.codex-plugin/plugin.json`, `.claude-plugin/plugin.json`, and `opencode/opencode.json`;
   - Codex plugin list version and, when exposed, the reported installed source path;
   - manifest and ProjectAtlas skill artifact under the reported Codex source path;
   - runtime `runtime-info` project, major version, capabilities, executable, and version;
   - generated MCP `--require-version`, DB path, config path, and final `mcp` command;
   - Codex global MCP registry runtime, DB/config, and version guard;
   - Claude Code `.mcp.json` config consumption;
   - OpenCode `opencode.json` config consumption;
   - downstream `.github/workflows` release pins that still point at old ProjectAtlas tags.
3. Prefer official host commands for host state. Inspect cache/source files only after the host reports the source path and after confirming it is the official ProjectAtlas source.
4. Make official ProjectAtlas paths work. If an official Codex marketplace/plugin cache is stale, repair it automatically with the supported remove/add, upgrade, or marketplace replacement flow, then re-read host state. Do not stop at a stale-cache warning unless the environment is intentionally managed or repair still cannot converge.
5. Preserve user-managed environments. If the marketplace/source is not clearly the official `styler-ai/ProjectAtlas` source, do not mutate it; report a concrete user-managed skip.
6. Claude Code and OpenCode do not use the Codex marketplace/cache model in this repo. For them, verify the generated project-local config with structured JSON parsing, then smoke the real host CLI when installed.
7. Pin generated MCP configs to absolute verified runtime, DB, and config paths. Avoid PATH-only fallbacks for ProjectAtlas-installed configs.
8. Compare canonical runtime paths, including symlink resolution where available. A shim path and resolved executable path must not create a false mismatch if they refer to the same verified runtime.
9. Keep installer output concrete: say which host, version, path, config, cache, registry entry, or workflow pin was verified, repaired, skipped, or still mismatched.

## Version Checks

Use a single small helper for each host surface so the installer checks data the same way in implementation and tests. The common ProjectAtlas bug is checking one fresh surface, then installing from another stale surface: for example, `codex plugin list` reports the expected version while the reported source path still contains an old manifest or old ProjectAtlas skill.

Required guard for Codex ProjectAtlas plugin repair:

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

Required guard for ProjectAtlas generated host configs:

```text
runtime_info.version == expected
and command_path == canonical_verified_runtime
and args include "--require-version", expected, "--db", selected_db, "mcp"
and optional config path == selected_config
and host-specific fields match the supported shape
```

OpenCode host-specific fields are `type = "local"`, `enabled = true`, command array shape, and project `cwd`. Claude Code uses an `.mcp.json`-compatible `mcpServers.projectatlas` shape and must not rely on `cwd` for project binding because DB/config arguments are absolute.

## Tests

Use fake host commands for deterministic installer tests and real host CLI smoke tests when installed locally. The minimum regression matrix is:

- official marketplace, stale plugin list version -> repairs;
- official marketplace, current plugin list version but stale reported source manifest -> repairs;
- official marketplace, current plugin/source manifest but stale ProjectAtlas skill artifact -> repairs or fails before claiming convergence;
- official marketplace, current plugin and source manifest -> no-op;
- non-official marketplace -> no mutation;
- failed repair -> restores when possible and reports the mismatched field;
- generated MCP config launches the verified runtime;
- generated config uses `--require-version`, selected DB/config, and final `mcp`;
- generated config parsing uses structured JSON (`ConvertFrom-Json`, `jq`, or `python3`), not sed/regex extraction;
- POSIX path comparison resolves existing runtime symlinks before declaring a mismatch;
- OpenCode config requires `type = "local"`, `enabled = true`, command array shape, and `cwd`;
- Claude Code config requires the `.mcp.json` server shape and absolute DB/config binding.

For installer tests, assert exact command fragments for remove/add/update calls and inspect the generated config files, not just process success. For real local smoke, prefer:

```bash
opencode --version
opencode debug config
claude --version
claude mcp get projectatlas
```

Summarize only the ProjectAtlas config fields from host debug output. Host debug commands may print unrelated local environment values; do not paste full raw output into issues, PRs, or release notes.

## Release Gates

Before calling a ProjectAtlas host-plugin release stable, verify:

- ProjectAtlas plugin manifest versions match the release tag;
- runtime `runtime-info` reports the release version, ProjectAtlas identity, MCP capability, and TOON output;
- installer writes project-local Codex-compatible, Claude Code, and OpenCode MCP configs with `--require-version`;
- installer repairs official stale Codex marketplace/plugin cache, Codex ProjectAtlas skill artifact drift, and Codex global MCP registry drift when `codex` is available and not intentionally skipped;
- installer verifies Claude Code and OpenCode generated configs against the verified runtime and host-specific schema;
- local smoke covers OpenCode and Claude Code if installed on the machine;
- release tests run on Windows and POSIX paths when both installer scripts exist;
