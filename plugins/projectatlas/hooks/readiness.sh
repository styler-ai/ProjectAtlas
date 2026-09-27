#!/bin/sh
# Read-only session check. The installer, not a hook, owns runtime and MCP changes.
set -u
direct_path=$(command -v projectatlas 2>/dev/null || true)
case "$direct_path" in /*) ;; *) direct_path= ;; esac
PATH=/usr/bin:/bin:/opt/homebrew/bin:/usr/local/bin
export PATH

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
generated=
if [ -f "$host_config" ] && [ "$(wc -c < "$host_config")" -le 1048576 ]; then
  generated=$(cat "$host_config" 2>/dev/null || true)
fi
state_base=${XDG_STATE_HOME:-${HOME:-}/.local/state}
if [ -d "$state_base" ]; then
  state_base=$(CDPATH= cd -P -- "$state_base" 2>/dev/null && pwd -P) || state_base=
else
  state_base=
fi
receipt=${state_base:+$state_base/projectatlas/codex-readiness.json}
codex_config=${CODEX_HOME:-${HOME:-}/.codex}/config.toml
if [ -n "$receipt" ] && [ -f "$receipt" ] && [ ! -L "$receipt" ] &&
  [ "$(wc -c < "$receipt")" -le 65536 ] && [ -f "$codex_config" ] &&
  [ "$(wc -c < "$codex_config")" -le 1048576 ]; then
  if command -v python3 >/dev/null 2>&1; then
    registry=$(python3 -c '
import hashlib, json, os, sys
try:
    with open(sys.argv[1], encoding="utf-8") as source:
        receipt = json.load(source)
    config_path, version, root = sys.argv[2:]
    bound_config = receipt["codex_config"]
    bound_root = receipt["project_root"]
    if (isinstance(receipt.get("registry"), dict) and receipt["version"] == version and
        isinstance(bound_config, str) and os.path.realpath(bound_config) == os.path.realpath(config_path) and
        isinstance(bound_root, str) and os.path.realpath(bound_root) == os.path.realpath(root) and
        receipt["codex_config_sha256"] == hashlib.sha256(open(config_path, "rb").read()).hexdigest()):
        print(json.dumps(receipt["registry"]))
except (OSError, ValueError, TypeError, KeyError, AttributeError):
    pass
' "$receipt" "$codex_config" "$expected" "$project_root" 2>/dev/null)
  elif command -v jq >/dev/null 2>&1; then
    codex_hash=
    if command -v sha256sum >/dev/null 2>&1; then
      codex_hash=$(sha256sum "$codex_config" | awk '{print $1}')
    elif command -v shasum >/dev/null 2>&1; then
      codex_hash=$(shasum -a 256 "$codex_config" | awk '{print $1}')
    fi
    if [ -n "$codex_hash" ] &&
      same_json_path "$(cat "$receipt")" '.[0].codex_config | strings' "$codex_config"; then
      registry=$(jq -ces --arg hash "$codex_hash" --arg version "$expected" '
        select(length == 1 and .[0].version == $version and
          .[0].codex_config_sha256 == $hash and (.[0].registry | type == "object")) |
        .[0].registry
      ' "$receipt" 2>/dev/null || true)
    fi
  fi
fi
receipt_valid() {
  [ -n "$receipt" ] && [ -f "$receipt" ] && [ ! -L "$receipt" ] &&
    [ -d "$state_base/projectatlas" ] && [ ! -L "$state_base/projectatlas" ] &&
    [ -f "$db" ] && [ -f "$host_config" ] && [ -f "$codex_config" ] &&
    [ -n "$direct_path" ] && [ -f "$direct_path" ] || return 1
  [ "$(wc -c < "$receipt")" -le 65536 ] || return 1
  [ "$(wc -c < "$host_config")" -le 1048576 ] &&
    [ "$(wc -c < "$codex_config")" -le 1048576 ] || return 1
  cursor=$starting_root
  while :; do
    [ ! -f "$cursor/.codex/config.toml" ] || return 1
    [ "$cursor" = "$project_root" ] && break
    cursor=$(dirname -- "$cursor")
  done
  if command -v python3 >/dev/null 2>&1; then
    python3 - "$receipt" "$host_config" "$expected" "$project_root" "$direct_path" "$db" "$config" "$codex_config" <<'PY'
import hashlib, json, os, sys
receipt_path, generated_path, version, root, runtime, database, config, codex_config = sys.argv[1:]
def same_path(actual, wanted):
    return isinstance(actual, str) and os.path.isabs(actual) and os.path.realpath(actual) == os.path.realpath(wanted)
def digest(path):
    with open(path, "rb") as source:
        value = hashlib.sha256()
        for chunk in iter(lambda: source.read(1048576), b""):
            value.update(chunk)
        return value.hexdigest()
try:
    with open(receipt_path, encoding="utf-8") as source:
        receipt = json.load(source)
    with open(generated_path, encoding="utf-8") as source:
        generated = json.load(source)["mcpServers"]["projectatlas"]
    registry = receipt["registry"]
    transport = registry["transport"]
    args = ["--require-version", version, "--db", database]
    if config:
        args += ["--config", config]
    args += ["mcp"]
    def same_args(actual):
        return (isinstance(actual, list) and len(actual) == len(args) and
                all(same_path(value, args[index]) if index == 3 or (index == 5 and config)
                    else value == args[index] for index, value in enumerate(actual)))
    ready = (receipt["version"] == version and
             same_path(receipt["project_root"], root) and same_path(receipt["runtime"], runtime) and
             same_path(receipt["direct_cli"], runtime) and
             same_path(receipt["codex_config"], codex_config) and
             receipt["runtime_sha256"] == digest(runtime) and
             receipt["direct_cli_sha256"] == receipt["runtime_sha256"] and
             receipt["generated_sha256"] == digest(generated_path) and
             receipt["codex_config_sha256"] == digest(codex_config) and
             registry["name"] == "projectatlas" and registry["enabled"] is True and
             transport["type"] == "stdio" and same_path(transport["command"], runtime) and
             same_args(transport["args"]) and same_path(generated["command"], runtime) and
             same_args(generated["args"]) and same_path(generated["cwd"], root))
    sys.exit(0 if ready else 1)
except (OSError, ValueError, TypeError, KeyError, IndexError):
    sys.exit(1)
PY
  elif command -v jq >/dev/null 2>&1; then
    if command -v sha256sum >/dev/null 2>&1; then
      hash_file() { sha256sum "$1" | awk '{print $1}'; }
    elif command -v shasum >/dev/null 2>&1; then
      hash_file() { shasum -a 256 "$1" | awk '{print $1}'; }
    else
      return 1
    fi
    receipt_json=$(cat "$receipt") || return 1
    printf '%s\n' "$receipt_json" | jq -se --arg v "$expected" \
      --arg runtime_hash "$(hash_file "$direct_path")" \
      --arg generated_hash "$(hash_file "$host_config")" \
      --arg codex_hash "$(hash_file "$codex_config")" '
      length == 1 and (.[0] |
        .version == $v and .runtime_sha256 == $runtime_hash and
        .direct_cli_sha256 == $runtime_hash and
        .generated_sha256 == $generated_hash and .codex_config_sha256 == $codex_hash and
        .registry.name == "projectatlas" and .registry.enabled == true and
        .registry.transport.type == "stdio" and
        (.registry.transport.args | type) == "array")
    ' >/dev/null 2>&1 || return 1
    same_json_path "$receipt_json" '.[0].project_root | strings' "$project_root" &&
      same_json_path "$receipt_json" '.[0].runtime | strings' "$direct_path" &&
      same_json_path "$receipt_json" '.[0].direct_cli | strings' "$direct_path" &&
      same_json_path "$receipt_json" '.[0].codex_config | strings' "$codex_config" &&
      same_json_path "$receipt_json" '.[0].registry.transport.command | strings' "$direct_path" || return 1
    registry=$(printf '%s\n' "$receipt_json" | jq -sr '.[] | .registry') || return 1
    printf '%s\n' "$registry" | jq -se --arg v "$expected" --arg cfg "$config" '
      length == 1 and (.[0].transport.args as $args |
        ($args | type) == "array" and
        $args == (["--require-version", $v, "--db", $args[3]] +
          (if $cfg == "" then [] else ["--config", $args[5]] end) + ["mcp"]))
    ' >/dev/null 2>&1 &&
      printf '%s\n' "$generated" | jq -se --arg v "$expected" --arg cfg "$config" '
      length == 1 and (.[0].mcpServers.projectatlas.args as $args |
        ($args | type) == "array" and
        $args == (["--require-version", $v, "--db", $args[3]] +
          (if $cfg == "" then [] else ["--config", $args[5]] end) + ["mcp"]))
    ' >/dev/null 2>&1 &&
      same_json_path "$registry" '.[0].transport.args[3] | strings' "$db" &&
      same_json_path "$generated" '.[0].mcpServers.projectatlas.command | strings' "$direct_path" &&
      same_json_path "$generated" '.[0].mcpServers.projectatlas.args[3] | strings' "$db" &&
      same_json_path "$generated" '.[0].mcpServers.projectatlas.cwd | strings' "$project_root" &&
      { [ -z "$config" ] || {
        same_json_path "$registry" '.[0].transport.args[5] | strings' "$config" &&
        same_json_path "$generated" '.[0].mcpServers.projectatlas.args[5] | strings' "$config"; }; }
  else
    return 1
  fi
}
runtime_info_ok() {
  if command -v python3 >/dev/null 2>&1; then
    printf '%s\n' "$runtime" | python3 -c '
import json, os, sys
try:
    identity = json.load(sys.stdin)
    sys.exit(0 if identity.get("project") == "ProjectAtlas" and
             identity.get("version") == sys.argv[1] and
             os.path.realpath(identity["executable"]) == os.path.realpath(sys.argv[2]) else 1)
except (ValueError, TypeError, KeyError):
    sys.exit(1)
' "$expected" "$direct_path" 2>/dev/null
  else
    printf '%s\n' "$runtime" | jq -se --arg v "$expected" '
      length == 1 and (.[0] | .project == "ProjectAtlas" and .version == $v and
        (.executable | type == "string"))' >/dev/null 2>&1 &&
      same_json_path "$runtime" '.[0].executable | strings' "$direct_path"
  fi
}
if [ -n "$expected" ] && receipt_valid; then
  runtime=$("$direct_path" --format json runtime-info 2>/dev/null || true)
  if runtime_info_ok; then
    reason='project database is incompatible or bound to another root'
    set -- "$direct_path" --db "$db"
    [ -z "$config" ] || set -- "$@" --config "$config"
    set -- "$@" --format json root verify --binding-only --project-root "$project_root"
    if (cd "$project_root" && "$@" >/dev/null 2>&1); then
      reason='installer readiness receipt or host files changed; rerun the installer'
      if receipt_valid; then
        printf 'ProjectAtlas integration ready: plugin, direct CLI, generated config, and Codex MCP match %s for this project. Use the version-matched ProjectAtlas skill and repository instructions.\n' "$expected"
        exit 0
      fi
    fi
  else
    reason='runtime identity is not version-matched'
  fi
else
  reason='installer readiness receipt or host files changed; rerun the installer'
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
values = [str(value).lower() if isinstance(value, bool) else value if isinstance(value, str) and value and not any(ord(char) < 32 or 127 <= ord(char) < 160 for char in value) else "unavailable" for value in values]
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
      end | gsub("[[:cntrl:]]"; "?")
    ' 2>/dev/null || printf 'Observed %s: unavailable\n' "$1"
  else
    printf 'Observed %s: unavailable (no JSON validator)\n' "$1"
  fi
}
printf '%s\n' "$runtime" | show_identity direct_cli
if [ -z "$runtime" ]; then
  printf 'Observed resolved_direct_cli_path: %s\n' "${direct_path:-unavailable}"
fi
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
