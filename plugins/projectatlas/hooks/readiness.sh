#!/bin/sh
# Read-only session check. The installer, not a hook, owns runtime and MCP changes.
set -u

plugin_root=${PLUGIN_ROOT:-$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)}
manifest=$plugin_root/.codex-plugin/plugin.json
skill=$plugin_root/skills/projectatlas/SKILL.md
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
  { [ -e "$project_root/.git" ] || [ -d "$project_root/.projectatlas" ] ||
    [ -f "$project_root/projectatlas.toml" ]; } && break
  project_root=$(dirname -- "$project_root")
done
cat "$plugin_root/hooks/agent-instructions.txt"
printf 'Read the complete installed ProjectAtlas skill now: %s\n' "$skill"
if [ ! -f "$skill" ]; then
  printf 'ProjectAtlas integration incomplete: bundled skill is missing; reinstall the version-matched plugin before Atlas use.\n'
  exit 0
fi

if [ "$project_root" = / ] ||
  { [ ! -e "$project_root/.git" ] && [ ! -d "$project_root/.projectatlas" ] &&
    [ ! -f "$project_root/projectatlas.toml" ]; }; then
  printf 'ProjectAtlas integration incomplete: no project root was identified. Select the intended project directory before running the version-matched installer with an explicit project root.\n'
  exit 0
fi

db=$project_root/.projectatlas/projectatlas.db
host_config=$project_root/.projectatlas/projectatlas.mcp.json
config=$project_root/.projectatlas/config.toml
if [ ! -f "$config" ]; then
  config=$project_root/projectatlas.toml
  [ -f "$config" ] || config=
fi
same_json_path() {
  actual=$(printf '%s\n' "$1" | jq -srj "$2" && printf '.') || return 1
  actual=${actual%.}
  [ -n "$actual" ] && [ "$actual" -ef "$3" ]
}
runtime=
registry=
generated=$(cat "$host_config" 2>/dev/null || true)
if [ -n "$expected" ] && command -v projectatlas >/dev/null 2>&1 &&
  { command -v python3 >/dev/null 2>&1 || command -v jq >/dev/null 2>&1; }; then
  runtime=$(projectatlas --format json runtime-info 2>/dev/null || true)
  if command -v python3 >/dev/null 2>&1; then
    executable=$(printf '%s\n' "$runtime" | python3 -c '
import json, sys
try:
    identity = json.load(sys.stdin)
    if identity.get("project") == "ProjectAtlas" and identity.get("version") == sys.argv[1] and isinstance(identity.get("executable"), str):
        print(identity["executable"])
except (ValueError, AttributeError):
    pass
' "$expected" 2>/dev/null || true)
  else
    executable=$(printf '%s\n' "$runtime" | jq -r --arg v "$expected" 'select(.project == "ProjectAtlas" and .version == $v) | .executable // empty' 2>/dev/null || true)
  fi
  if [ -n "$executable" ]; then
    reason='project database or generated host config unavailable'
    if [ -f "$db" ] && [ -f "$host_config" ] && command -v codex >/dev/null 2>&1; then
      set -- projectatlas --db "$db"
      [ -z "$config" ] || set -- "$@" --config "$config"
      set -- "$@" --format json root verify
      if ! (cd "$project_root" && "$@" >/dev/null 2>&1); then
        reason='project database is incompatible or bound to another root'
      else
      registry=$(codex mcp get projectatlas --json 2>/dev/null || true)
      generated=$(cat "$host_config" 2>/dev/null || true)
      if { command -v python3 >/dev/null 2>&1 &&
        printf '%s\n' "$registry" | python3 -c '
import json, os, sys
try:
    registry = json.load(sys.stdin)
    with open(sys.argv[1], encoding="utf-8") as source:
        generated = json.load(source)["mcpServers"]["projectatlas"]
    executable, version, database, config, root = sys.argv[2:]
    args = ["--require-version", version, "--db", database]
    if config:
        args += ["--config", config]
    args += ["mcp"]
    transport = registry["transport"]
    def same_path(actual, wanted):
        return isinstance(actual, str) and os.path.realpath(actual) == os.path.realpath(wanted)
    def same_args(actual):
        return (isinstance(actual, list) and len(actual) == len(args) and
                all(same_path(value, args[index]) if index == 3 or (index == 5 and config)
                    else value == args[index] for index, value in enumerate(actual)))
    ready = (registry["enabled"] is True and transport["type"] == "stdio" and
             same_path(transport["command"], executable) and same_args(transport["args"]) and
             same_path(generated["command"], executable) and same_args(generated["args"]) and
             same_path(generated["cwd"], root))
    sys.exit(0 if ready else 1)
except (OSError, ValueError, TypeError, KeyError, IndexError):
    sys.exit(1)
' "$host_config" "$executable" "$expected" "$db" "$config" "$project_root" 2>/dev/null; } ||
        { ! command -v python3 >/dev/null 2>&1 &&
        printf '%s\n' "$registry" | jq -se --arg v "$expected" --arg cfg "$config" '
          def clean_path: if type == "string" then length > 0 and index("\u0000") == null else false end;
          length == 1 and (.[0] |
            .transport.args as $args |
            .enabled == true and .transport.type == "stdio" and
            ($args | type) == "array" and
            (.transport.command | clean_path) and ($args[3] | clean_path) and
            (if $cfg == "" then true else ($args[5] | clean_path) end) and
            $args == (["--require-version", $v, "--db", $args[3]] +
              (if $cfg == "" then [] else ["--config", $args[5]] end) + ["mcp"]))
        ' >/dev/null 2>&1 &&
        printf '%s\n' "$generated" | jq -se --arg v "$expected" --arg cfg "$config" '
          def clean_path: if type == "string" then length > 0 and index("\u0000") == null else false end;
          length == 1 and (.[0] |
            .mcpServers.projectatlas.args as $args |
            ($args | type) == "array" and
            (.mcpServers.projectatlas.command | clean_path) and
            (.mcpServers.projectatlas.cwd | clean_path) and
            ($args[3] | clean_path) and
            (if $cfg == "" then true else ($args[5] | clean_path) end) and
            $args == (["--require-version", $v, "--db", $args[3]] +
              (if $cfg == "" then [] else ["--config", $args[5]] end) + ["mcp"]))
        ' >/dev/null 2>&1 &&
        same_json_path "$registry" '.[0].transport.command | strings' "$executable" &&
        same_json_path "$registry" '.[0].transport.args[3] | strings' "$db" &&
        same_json_path "$generated" '.[0].mcpServers.projectatlas.command | strings' "$executable" &&
        same_json_path "$generated" '.[0].mcpServers.projectatlas.args[3] | strings' "$db" &&
        same_json_path "$generated" '.[0].mcpServers.projectatlas.cwd | strings' "$project_root" &&
        { [ -z "$config" ] || {
          same_json_path "$registry" '.[0].transport.args[5] | strings' "$config" &&
          same_json_path "$generated" '.[0].mcpServers.projectatlas.args[5] | strings' "$config"; }; }; }; then
        printf 'ProjectAtlas integration ready: plugin, direct CLI, generated config, and Codex MCP match %s for this project. Use the version-matched ProjectAtlas skill and repository instructions.\n' "$expected"
        exit 0
      fi
      reason='Codex MCP or generated config does not match this project/runtime'
      fi
    fi
  fi
fi

printf 'ProjectAtlas integration incomplete: %s. Plugin installation alone does not update the native runtime or MCP registry.\n' "$reason"
printf 'Expected: plugin_version=%s project_db=%s project_config=%s project_root=%s codex_mcp_enabled=true codex_mcp_transport=stdio\n' "$expected" "${db}" "${config:-unavailable}" "$project_root"
show_identity() {
  if command -v python3 >/dev/null 2>&1; then
    python3 -c '
import json, sys
label = sys.argv[1]
try:
    source = json.load(sys.stdin)
    if label == "direct_cli":
        values = (source.get("version"), source.get("executable"), None, None, None, None, None)
    elif label == "codex_mcp":
        transport = source.get("transport", {})
        args = transport.get("args", [])
        values = (args[1] if len(args) > 1 else None, transport.get("command"),
                  args[3] if len(args) > 3 else None, args[5] if len(args) > 5 else None,
                  source.get("enabled"), transport.get("type"), None)
    else:
        source = source.get("mcpServers", {}).get("projectatlas", {})
        args = source.get("args", [])
        values = (args[1] if len(args) > 1 else None, source.get("command"),
                  args[3] if len(args) > 3 else None, args[5] if len(args) > 5 else None,
                  None, None, source.get("cwd"))
except (ValueError, TypeError, KeyError, AttributeError):
    values = (None,) * 7
values = [str(value).lower() if isinstance(value, bool) else value if isinstance(value, str) and value else "unavailable" for value in values]
print("Observed %s: version=%s executable=%s db=%s config=%s enabled=%s transport=%s cwd=%s" % (label, *values))
' "$1" 2>/dev/null
  elif command -v jq >/dev/null 2>&1; then
    jq -r -s --arg label "$1" '
      (.[0] // {}) |
      if $label == "direct_cli" then
        "Observed direct_cli: version=\(.version // "unavailable") executable=\(.executable // "unavailable") db=unavailable config=unavailable enabled=unavailable transport=unavailable cwd=unavailable"
      else
        (if $label == "codex_mcp" then .transport else .mcpServers.projectatlas end) as $identity |
        "Observed \($label): version=\($identity.args[1] // "unavailable") executable=\($identity.command // "unavailable") db=\($identity.args[3] // "unavailable") config=\($identity.args[5] // "unavailable") enabled=\(if $label == "codex_mcp" then .enabled | tostring else "unavailable" end) transport=\(if $label == "codex_mcp" then .transport.type // "unavailable" else "unavailable" end) cwd=\(if $label == "generated_mcp" then $identity.cwd // "unavailable" else "unavailable" end)"
      end
    ' 2>/dev/null || printf 'Observed %s: unavailable\n' "$1"
  else
    printf 'Observed %s: unavailable (no JSON validator)\n' "$1"
  fi
}
printf '%s\n' "$runtime" | show_identity direct_cli
printf '%s\n' "$registry" | show_identity codex_mcp
printf '%s\n' "$generated" | show_identity generated_mcp
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
