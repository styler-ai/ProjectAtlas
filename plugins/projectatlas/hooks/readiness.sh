#!/bin/sh
# Read-only session check. The installer, not a hook, owns runtime and MCP changes.
set -u

plugin_root=${PLUGIN_ROOT:-$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)}
manifest=$plugin_root/.codex-plugin/plugin.json
expected=$(sed -n 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$manifest" | head -n 1)
reason='runtime unavailable, mismatched, or JSON validator unavailable'
starting_root=$(pwd -P)
project_root=$starting_root
home_root=
[ -z "${HOME:-}" ] || home_root=$(CDPATH= cd -- "$HOME" 2>/dev/null && pwd -P) || true
while :; do
  if [ "$project_root" != "$starting_root" ] && [ "$project_root" = "$home_root" ]; then
    project_root=$starting_root
    break
  fi
  [ -f "$project_root/.projectatlas/projectatlas.db" ] && break
  [ "$project_root" = / ] && break
  { [ -e "$project_root/.git" ] || [ -d "$project_root/.projectatlas" ]; } && break
  project_root=$(dirname -- "$project_root")
done
cat "$plugin_root/hooks/agent-instructions.txt"

if [ ! -e "$project_root/.git" ] && [ ! -d "$project_root/.projectatlas" ]; then
  printf 'ProjectAtlas integration incomplete: no project root was identified. Select the intended project directory before running the version-matched installer with an explicit project root.\n'
  exit 0
fi

if [ -n "$expected" ] && command -v projectatlas >/dev/null 2>&1 && command -v jq >/dev/null 2>&1; then
  runtime=$(projectatlas --format json runtime-info 2>/dev/null || true)
  executable=$(printf '%s\n' "$runtime" | jq -r 'select(.project == "ProjectAtlas") | .executable // empty' 2>/dev/null || true)
  version=$(printf '%s\n' "$runtime" | jq -r '.version // empty' 2>/dev/null || true)
  if [ "$version" = "$expected" ] && [ -n "$executable" ]; then
    reason='project database or generated host config unavailable'
    db=$project_root/.projectatlas/projectatlas.db
    host_config=$project_root/.projectatlas/projectatlas.mcp.json
    config=$project_root/.projectatlas/config.toml
    [ -f "$config" ] || config=
    if [ -f "$db" ] && [ -f "$host_config" ] && command -v codex >/dev/null 2>&1; then
      set -- projectatlas --db "$db"
      [ -z "$config" ] || set -- "$@" --config "$config"
      set -- "$@" --format json root verify
      if ! (cd "$project_root" && "$@" >/dev/null 2>&1); then
        reason='project database is incompatible or bound to another root'
      else
      registry=$(codex mcp get projectatlas --json 2>/dev/null || true)
      if printf '%s\n' "$registry" | jq -e --arg exe "$executable" --arg v "$expected" --arg db "$db" --arg cfg "$config" '
        def expected_args: ["--require-version", $v, "--db", $db] +
          (if $cfg == "" then [] else ["--config", $cfg] end) + ["mcp"];
        .enabled == true and .transport.type == "stdio" and
        .transport.command == $exe and .transport.args == expected_args
      ' >/dev/null 2>&1 &&
        jq -e --arg exe "$executable" --arg v "$expected" --arg db "$db" --arg cfg "$config" --arg root "$project_root" '
          def expected_args: ["--require-version", $v, "--db", $db] +
            (if $cfg == "" then [] else ["--config", $cfg] end) + ["mcp"];
          .mcpServers.projectatlas.command == $exe and
          .mcpServers.projectatlas.args == expected_args and
          .mcpServers.projectatlas.cwd == $root
        ' "$host_config" >/dev/null 2>&1; then
        printf 'ProjectAtlas integration ready: plugin, direct CLI, generated config, and Codex MCP match %s for this project. Use the version-matched ProjectAtlas skill and repository instructions.\n' "$expected"
        exit 0
      fi
      reason='Codex MCP or generated config does not match this project/runtime'
      fi
    fi
  fi
fi

printf 'ProjectAtlas integration incomplete: %s. Plugin installation alone does not update the native runtime or MCP registry.\n' "$reason"
shell_quote() {
  printf "'"
  printf '%s' "$1" | sed "s/'/'\\\\''/g"
  printf "'"
}
printf 'Use the version-matched ProjectAtlas skill. Do not reset a database.\nRepair command:\nPROJECTATLAS_VERSION='
shell_quote "v$expected"
printf ' bash '
shell_quote "$plugin_root/scripts/install-runtime.sh"
printf ' '
shell_quote "$project_root"
printf '\nVerification commands:\nprojectatlas --format json runtime-info\ncodex mcp get projectatlas --json\n'
