#!/usr/bin/env sh
# Purpose: Install or update the ProjectAtlas plugin runtime and POSIX MCP configs.
set -eu

repository=${PROJECTATLAS_REPOSITORY:-https://github.com/styler-ai/ProjectAtlas}
projectatlas_version=${PROJECTATLAS_VERSION:-}
release_base_url=${PROJECTATLAS_RELEASE_BASE_URL:-https://github.com/styler-ai/ProjectAtlas/releases/download}
release_binary_only=${PROJECTATLAS_RELEASE_BINARY_ONLY:-}
runtime_override=${PROJECTATLAS_RUNTIME_PATH:-}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
plugin_root=$(cd "$script_dir/.." && pwd -P)
plugin_manifest="$plugin_root/.codex-plugin/plugin.json"
if [ -z "$projectatlas_version" ] && [ -f "$plugin_manifest" ]; then
  plugin_version=$(sed -n 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$plugin_manifest" | head -n 1)
  if [ -n "$plugin_version" ]; then
    projectatlas_version="v$plugin_version"
  fi
fi

if [ "${1:-}" ]; then
  project_root=$(cd "$1" && pwd -P)
else
  project_root=$(pwd -P)
fi
atlas_dir="$project_root/.projectatlas"
if [ -L "$atlas_dir" ] || [ -h "$atlas_dir" ]; then
  printf '%s\n' "ProjectAtlas project state directory must not be a symlink: $atlas_dir" >&2
  exit 1
fi
if [ -e "$atlas_dir" ] && [ ! -d "$atlas_dir" ]; then
  printf '%s\n' "ProjectAtlas project state path must be a directory: $atlas_dir" >&2
  exit 1
fi
if [ -d "$atlas_dir" ]; then
  atlas_dir_canonical=$(CDPATH= cd -- "$atlas_dir" && pwd -P)
  if [ "$atlas_dir_canonical" != "$atlas_dir" ]; then
    printf '%s\n' "ProjectAtlas project state directory escaped the canonical project root: $atlas_dir" >&2
    exit 1
  fi
fi
if [ -n "$runtime_override" ]; then
  runtime_dir=$(CDPATH= cd -- "$(dirname -- "$runtime_override")" && pwd -P)
  runtime_override="$runtime_dir/$(basename -- "$runtime_override")"
fi

truthy() {
  case "${1:-}" in
    1|true|TRUE|yes|YES|on|ON)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

find_projectatlas() {
  if [ -x "$HOME/.local/bin/projectatlas" ] && is_projectatlas_runtime "$HOME/.local/bin/projectatlas"; then
    printf '%s\n' "$HOME/.local/bin/projectatlas"
    return 0
  fi
  if [ -x "$HOME/.cargo/bin/projectatlas" ] && is_projectatlas_runtime "$HOME/.cargo/bin/projectatlas"; then
    printf '%s\n' "$HOME/.cargo/bin/projectatlas"
    return 0
  fi
  if command -v projectatlas >/dev/null 2>&1 && is_projectatlas_runtime "$(command -v projectatlas)"; then
    command -v projectatlas
    return 0
  fi
  return 1
}

expected_runtime_version() {
  if [ -z "$projectatlas_version" ]; then
    return 0
  fi
  printf '%s\n' "${projectatlas_version#v}"
}

is_projectatlas_runtime() {
  candidate=$1
  runtime_info=$("$candidate" --format json runtime-info 2>/dev/null || true)
  project=$(printf '%s\n' "$runtime_info" | sed -n 's/.*"project"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)
  major_version=$(printf '%s\n' "$runtime_info" | sed -n 's/.*"major_version"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p')
  runtime_version=$(printf '%s\n' "$runtime_info" | sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)
  text_format=$(printf '%s\n' "$runtime_info" | sed -n 's/.*"text_format"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)
  expected_version=$(expected_runtime_version)
  [ "$project" = "ProjectAtlas" ] &&
    [ "${major_version:-0}" -ge 3 ] 2>/dev/null &&
    printf '%s\n' "$runtime_info" | grep -q '"mcp"' &&
    [ "$text_format" = "TOON" ] &&
    { [ -z "$expected_version" ] || [ "$runtime_version" = "$expected_version" ]; }
}

is_projectatlas_runtime_contract() {
  candidate=$1
  runtime_info=$("$candidate" --format json runtime-info 2>/dev/null || true)
  project=$(printf '%s\n' "$runtime_info" | sed -n 's/.*"project"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)
  major_version=$(printf '%s\n' "$runtime_info" | sed -n 's/.*"major_version"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p')
  text_format=$(printf '%s\n' "$runtime_info" | sed -n 's/.*"text_format"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)
  [ "$project" = "ProjectAtlas" ] &&
    [ "${major_version:-0}" -ge 3 ] 2>/dev/null &&
    printf '%s\n' "$runtime_info" | grep -q '"mcp"' &&
    [ "$text_format" = "TOON" ]
}

runtime_version() {
  candidate=$1
  runtime_info=$("$candidate" --format json runtime-info 2>/dev/null || true)
  printf '%s\n' "$runtime_info" | sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1
}

known_projectatlas_shim_paths() {
  printf '%s\n' "$HOME/.cargo/bin/projectatlas"
  printf '%s\n' "$HOME/.npm/bin/projectatlas"
  printf '%s\n' "$HOME/.npm-global/bin/projectatlas"
  printf '%s\n' "$HOME/.local/share/npm/bin/projectatlas"
}

canonical_file() {
  file=$1
  if [ -e "$file" ]; then
    if command -v realpath >/dev/null 2>&1; then
      realpath "$file" 2>/dev/null && return 0
    fi
    if command -v readlink >/dev/null 2>&1; then
      resolved=$(readlink -f "$file" 2>/dev/null || true)
      if [ -n "$resolved" ]; then
        printf '%s\n' "$resolved"
        return 0
      fi
    fi
    if command -v python3 >/dev/null 2>&1; then
      python3 - "$file" <<'PY' && return 0
from pathlib import Path
import sys

print(Path(sys.argv[1]).resolve())
PY
    fi
  fi
  dir=$(CDPATH= cd -- "$(dirname -- "$file")" 2>/dev/null && pwd -P) || {
    printf '%s\n' "$file"
    return 0
  }
  printf '%s/%s\n' "$dir" "$(basename -- "$file")"
}

prepend_projectatlas_process_path() {
  runtime_dir=$(CDPATH= cd -- "$(dirname -- "$1")" 2>/dev/null && pwd -P) || return 0
  new_path=$runtime_dir
  old_ifs=$IFS
  IFS=:
  for entry in ${PATH:-}; do
    if [ -z "$entry" ]; then
      continue
    fi
    entry_dir=$(CDPATH= cd -- "$entry" 2>/dev/null && pwd -P || printf '%s\n' "$entry")
    if [ "$entry_dir" != "$runtime_dir" ]; then
      new_path=$new_path:$entry
    fi
  done
  IFS=$old_ifs
  PATH=$new_path
  export PATH
}

confirm_bare_projectatlas_resolution() {
  verified=$1
  verified_canonical=$(canonical_file "$verified")
  first=$(command -v projectatlas 2>/dev/null || true)
  if [ -z "$first" ]; then
    printf '%s\n' "warning: active process still cannot resolve bare 'projectatlas'. Generated MCP configs use the verified absolute runtime: $verified. Restart the host shell before relying on bare projectatlas." >&2
  elif [ "$(canonical_file "$first")" = "$verified_canonical" ] && is_projectatlas_runtime "$first"; then
    printf 'Active process resolves bare projectatlas to verified runtime: %s\n' "$first"
  else
    first_version=$(runtime_version "$first")
    printf '%s\n' "warning: active process still resolves bare 'projectatlas' to $first version '$first_version', not the verified runtime $verified. Generated MCP configs use the absolute runtime; restart the host shell, put $(dirname -- "$verified") first on PATH, or remove the obsolete shim before relying on bare projectatlas." >&2
  fi
}

is_known_projectatlas_shim_path() {
  candidate_canonical=$(canonical_file "$1")
  known_projectatlas_shim_paths | while IFS= read -r known_path; do
    if [ "$candidate_canonical" = "$(canonical_file "$known_path")" ]; then
      printf '%s\n' matched
      break
    fi
  done | grep -q '^matched$'
}

quarantine_stale_projectatlas_shim() {
  candidate=$1
  version=$2
  safe_version=$(printf '%s\n' "$version" | sed 's/[^A-Za-z0-9_.-]/_/g')
  if [ -z "$safe_version" ]; then
    safe_version=unknown
  fi
  quarantine_path="$candidate.projectatlas-stale-$safe_version.bak"
  if [ -e "$quarantine_path" ]; then
    quarantine_path="$quarantine_path.$(date +%Y%m%d%H%M%S)"
  fi
  if [ -e "$quarantine_path" ]; then
    quarantine_path="$quarantine_path.$$"
  fi
  if mv "$candidate" "$quarantine_path"; then
    printf '%s\n' "Quarantined stale ProjectAtlas shim: $candidate -> $quarantine_path version '$version'"
  else
    printf '%s\n' "warning: could not quarantine stale ProjectAtlas shim $candidate version '$version'." >&2
  fi
}

quarantine_known_stale_projectatlas_shims() {
  verified=$1
  expected_version=$(expected_runtime_version)
  if [ -z "$verified" ] || [ -z "$expected_version" ]; then
    return 0
  fi
  verified_canonical=$(canonical_file "$verified")
  old_ifs=$IFS
  IFS=:
  for entry in $PATH; do
    candidate=$entry/projectatlas
    if [ ! -x "$candidate" ] || [ "$(canonical_file "$candidate")" = "$verified_canonical" ]; then
      continue
    fi
    if is_known_projectatlas_shim_path "$candidate"; then
      if ! is_projectatlas_runtime_contract "$candidate"; then
        continue
      fi
      version=$(runtime_version "$candidate")
      if [ -n "$version" ] && [ "$version" != "$expected_version" ]; then
        quarantine_stale_projectatlas_shim "$candidate" "$version"
      fi
    fi
  done
  IFS=$old_ifs
  known_projectatlas_shim_paths | while IFS= read -r candidate; do
    if [ ! -x "$candidate" ] || [ "$(canonical_file "$candidate")" = "$verified_canonical" ]; then
      continue
    fi
    if ! is_projectatlas_runtime_contract "$candidate"; then
      continue
    fi
    version=$(runtime_version "$candidate")
    if [ -n "$version" ] && [ "$version" != "$expected_version" ]; then
      quarantine_stale_projectatlas_shim "$candidate" "$version"
    fi
  done
}

warn_path_shadow() {
  verified=$1
  verified_canonical=$(canonical_file "$verified")
  first=$(command -v projectatlas 2>/dev/null || true)
  if [ -z "$first" ]; then
    printf '%s\n' "warning: bare 'projectatlas' is not on PATH. Generated MCP configs use the verified absolute runtime: $verified" >&2
  elif [ "$(canonical_file "$first")" != "$verified_canonical" ]; then
    first_version=$(runtime_version "$first")
    printf '%s\n' "warning: bare 'projectatlas' resolves to $first version '$first_version', not the verified runtime $verified. Put $(dirname -- "$verified") first on PATH or remove the obsolete shim." >&2
  fi
  old_ifs=$IFS
  IFS=:
  for entry in $PATH; do
    candidate=$entry/projectatlas
    if [ ! -x "$candidate" ] || [ "$(canonical_file "$candidate")" = "$verified_canonical" ]; then
      continue
    fi
    if ! is_projectatlas_runtime "$candidate"; then
      version=$(runtime_version "$candidate")
      printf '%s\n' "warning: obsolete ProjectAtlas runtime or shim still exists on PATH: $candidate version '$version'. It was not removed automatically." >&2
    fi
  done
  IFS=$old_ifs
}

resolve_codex_command() {
  operation=$1
  codex_bin=${PROJECTATLAS_CODEX_COMMAND:-}
  if [ -z "$codex_bin" ]; then
    codex_bin=$(command -v codex 2>/dev/null || true)
  fi
  if [ -z "$codex_bin" ]; then
    printf '%s\n' "$operation skipped: codex command not found."
    return 1
  fi
  return 0
}

codex_projectatlas_marketplace_source() {
  marketplaces=$1
  if command -v jq >/dev/null 2>&1; then
    printf '%s\n' "$marketplaces" | jq -r '.marketplaces[]? | select(.name == "projectatlas") | .marketplaceSource.source // empty' | head -n 1
    return 0
  fi
  printf '%s\n' "$marketplaces" | awk '
    /"name"[[:space:]]*:[[:space:]]*"projectatlas"/ {
      line = $0
      if (line ~ /"source"[[:space:]]*:/) {
        sub(/.*"source"[[:space:]]*:[[:space:]]*"/, "", line)
        sub(/".*/, "", line)
        print line
        exit
      }
      in_projectatlas = 1
      next
    }
    in_projectatlas && /"name"[[:space:]]*:/ { in_projectatlas = 0 }
    in_projectatlas && /"source"[[:space:]]*:/ {
      line = $0
      sub(/.*"source"[[:space:]]*:[[:space:]]*"/, "", line)
      sub(/".*/, "", line)
      print line
      exit
    }
  '
}

official_projectatlas_marketplace_source() {
  source=$(printf '%s' "${1:-}" | sed 's#/*$##')
  case "$source" in
    styler-ai/ProjectAtlas | styler-ai/ProjectAtlas.git | \
      https://github.com/styler-ai/ProjectAtlas | https://github.com/styler-ai/ProjectAtlas.git | \
      git@github.com:styler-ai/ProjectAtlas | git@github.com:styler-ai/ProjectAtlas.git | \
      ssh://git@github.com/styler-ai/ProjectAtlas | ssh://git@github.com/styler-ai/ProjectAtlas.git)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

codex_config_path() {
  if [ -n "${CODEX_HOME:-}" ]; then
    printf '%s\n' "$CODEX_HOME/config.toml"
  elif [ -n "${HOME:-}" ]; then
    printf '%s\n' "$HOME/.codex/config.toml"
  fi
}

codex_projectatlas_marketplace_ref() {
  config_path=$(codex_config_path)
  if [ -z "$config_path" ] || [ ! -f "$config_path" ]; then
    return 0
  fi
  awk '
    /^[[:space:]]*\[marketplaces\.projectatlas\][[:space:]]*$/ { in_projectatlas = 1; next }
    in_projectatlas && /^[[:space:]]*\[/ { exit }
    in_projectatlas && /^[[:space:]]*ref[[:space:]]*=/ {
      line = $0
      sub(/^[^=]*=[[:space:]]*["'\'']/, "", line)
      sub(/["'\''].*/, "", line)
      print line
      exit
    }
  ' "$config_path"
}

restore_codex_projectatlas_marketplace() {
  previous_source=${1:-}
  previous_ref=${2:-}
  if [ -z "$previous_source" ]; then
    return 0
  fi
  "$codex_bin" plugin marketplace remove projectatlas --json >/dev/null 2>&1 || true
  if [ -n "$previous_ref" ]; then
    "$codex_bin" plugin marketplace add "$previous_source" --ref "$previous_ref" --json >/dev/null 2>&1 || return 0
  else
    "$codex_bin" plugin marketplace add "$previous_source" --json >/dev/null 2>&1 || return 0
  fi
  "$codex_bin" plugin add projectatlas --marketplace projectatlas --json >/dev/null 2>&1 || true
}

codex_projectatlas_plugin_version() {
  plugins=$("$codex_bin" plugin list --marketplace projectatlas --json 2>/dev/null) || return 0
  if command -v jq >/dev/null 2>&1; then
    printf '%s\n' "$plugins" | jq -r '.installed[]? | select(.pluginId == "projectatlas@projectatlas" or (.name == "projectatlas" and .marketplaceName == "projectatlas")) | .version // empty' | head -n 1
    return 0
  fi
  compact=$(printf '%s' "$plugins" | tr -d '\r\n')
  printf '%s\n' "$compact" |
    sed 's/},{/}\n{/g' |
    grep -E '"pluginId"[[:space:]]*:[[:space:]]*"projectatlas@projectatlas"|"name"[[:space:]]*:[[:space:]]*"projectatlas".*"marketplaceName"[[:space:]]*:[[:space:]]*"projectatlas"' |
    sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' |
    head -n 1
}

codex_projectatlas_plugin_source_path() {
  plugins=$("$codex_bin" plugin list --marketplace projectatlas --json 2>/dev/null) || return 0
  if command -v jq >/dev/null 2>&1; then
    printf '%s\n' "$plugins" | jq -r '.installed[]? | select(.pluginId == "projectatlas@projectatlas" or (.name == "projectatlas" and .marketplaceName == "projectatlas")) | (.source.path // .path // .root // .location // empty)' | head -n 1
    return 0
  fi
  compact=$(printf '%s' "$plugins" | tr -d '\r\n')
  printf '%s\n' "$compact" |
    sed 's/},{/}\n{/g' |
    grep -E '"pluginId"[[:space:]]*:[[:space:]]*"projectatlas@projectatlas"|"name"[[:space:]]*:[[:space:]]*"projectatlas".*"marketplaceName"[[:space:]]*:[[:space:]]*"projectatlas"' |
    sed -n 's/.*"path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' |
    head -n 1
}

codex_projectatlas_plugin_source_manifest_version() {
  plugin_source_path=$(codex_projectatlas_plugin_source_path)
  [ -n "$plugin_source_path" ] || return 0
  manifest_path=$plugin_source_path/.codex-plugin/plugin.json
  if [ ! -f "$manifest_path" ]; then
    printf '%s\n' ""
    return 0
  fi
  sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$manifest_path" | head -n 1
}

codex_projectatlas_plugin_source_manifest_matches() {
  expected_version=$1
  plugin_source_path=$(codex_projectatlas_plugin_source_path)
  [ -n "$plugin_source_path" ] || return 0
  [ "$(codex_projectatlas_plugin_source_manifest_version)" = "$expected_version" ]
}

verify_codex_projectatlas_skill_artifact() {
  runtime_version=$(expected_runtime_version)
  if [ -z "$runtime_version" ]; then
    runtime_version=$(runtime_version "$projectatlas_bin")
  fi
  if [ -z "$runtime_version" ]; then
    printf '%s\n' "Codex ProjectAtlas plugin skill verification skipped: ProjectAtlas version is unknown."
    return 0
  fi
  installed_version=$(codex_projectatlas_plugin_version)
  if [ -z "$installed_version" ]; then
    printf '%s\n' "warning: Codex ProjectAtlas plugin skill verification skipped: projectatlas plugin is not installed." >&2
    return 0
  fi
  if [ "$installed_version" != "$runtime_version" ]; then
    printf "warning: Codex ProjectAtlas plugin skill verification failed: installed projectatlas plugin version '%s' does not match %s.\n" "$installed_version" "$runtime_version" >&2
    return 0
  fi
  plugin_source_path=$(codex_projectatlas_plugin_source_path)
  if [ -z "$plugin_source_path" ]; then
    printf 'Codex ProjectAtlas plugin skill version %s is installed; Codex does not expose the active in-process ProjectAtlas skill path. Restart Codex if this session still advertises an older ProjectAtlas skill.\n' "$runtime_version"
    return 0
  fi
  manifest_path=$plugin_source_path/.codex-plugin/plugin.json
  skill_path=$plugin_source_path/skills/projectatlas/SKILL.md
  if [ ! -f "$manifest_path" ]; then
    printf 'warning: Codex ProjectAtlas plugin skill verification failed: plugin manifest was not found at %s.\n' "$manifest_path" >&2
    return 0
  fi
  if [ ! -f "$skill_path" ]; then
    printf 'warning: Codex ProjectAtlas plugin skill verification failed: ProjectAtlas skill was not found at %s.\n' "$skill_path" >&2
    return 0
  fi
  if ! grep -E '"version"[[:space:]]*:[[:space:]]*"'"$runtime_version"'"' "$manifest_path" >/dev/null; then
    printf 'warning: Codex ProjectAtlas plugin skill verification failed: manifest version does not match %s.\n' "$runtime_version" >&2
    return 0
  fi
  printf 'Codex ProjectAtlas plugin skill verified at %s for %s.\n' "$skill_path" "$runtime_version"
  printf '%s\n' "Codex does not expose the active in-process ProjectAtlas skill path; restart Codex if this session still advertises an older ProjectAtlas skill."
}

update_codex_plugin() {
  if truthy "${PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE:-}"; then
    printf '%s\n' "Codex ProjectAtlas plugin update skipped by PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE."
    return 0
  fi
  runtime_version=$(expected_runtime_version)
  if [ -z "$runtime_version" ]; then
    runtime_version=$(runtime_version "$projectatlas_bin")
  fi
  if [ -z "$runtime_version" ]; then
    printf '%s\n' "Codex ProjectAtlas plugin update skipped: ProjectAtlas version is unknown."
    return 0
  fi
  resolve_codex_command "Codex ProjectAtlas plugin update" || return 0
  marketplaces=$("$codex_bin" plugin marketplace list --json 2>&1) || {
    printf '%s\n' "Codex ProjectAtlas plugin update skipped: could not list Codex plugin marketplaces."
    return 0
  }
  if ! printf '%s\n' "$marketplaces" | grep -E '"name"[[:space:]]*:[[:space:]]*"projectatlas"' >/dev/null; then
    printf '%s\n' "Codex ProjectAtlas plugin update skipped: projectatlas marketplace is not configured."
    return 0
  fi
  marketplace_source=$(codex_projectatlas_marketplace_source "$marketplaces")
  if ! official_projectatlas_marketplace_source "$marketplace_source"; then
    printf '%s\n' "Codex ProjectAtlas plugin update skipped: projectatlas marketplace is not the official styler-ai/ProjectAtlas source."
    return 0
  fi
  previous_ref=$(codex_projectatlas_marketplace_ref)

  release_tag=v$runtime_version
  current_plugin_version=$(codex_projectatlas_plugin_version)
  if [ "$previous_ref" = "$release_tag" ] &&
    [ "$current_plugin_version" = "$runtime_version" ] &&
    codex_projectatlas_plugin_source_manifest_matches "$runtime_version"; then
    printf 'Codex ProjectAtlas plugin marketplace already points to %s.\n' "$release_tag"
    verify_codex_projectatlas_skill_artifact
    return 0
  fi
  if [ "$previous_ref" = "$release_tag" ]; then
    if [ "$current_plugin_version" = "$runtime_version" ] &&
      ! codex_projectatlas_plugin_source_manifest_matches "$runtime_version"; then
      source_manifest_version=$(codex_projectatlas_plugin_source_manifest_version)
      printf "Codex ProjectAtlas plugin source manifest version '%s' does not match %s; refreshing official projectatlas plugin cache.\n" "$source_manifest_version" "$runtime_version"
    fi
    "$codex_bin" plugin remove projectatlas --marketplace projectatlas --json >/dev/null 2>&1 || true
    if "$codex_bin" plugin add projectatlas --marketplace projectatlas --json >/dev/null 2>&1; then
      installed_version=$(codex_projectatlas_plugin_version)
      if [ "$installed_version" = "$runtime_version" ]; then
        if codex_projectatlas_plugin_source_manifest_matches "$runtime_version"; then
          printf 'Codex ProjectAtlas plugin marketplace updated to %s.\n' "$release_tag"
          verify_codex_projectatlas_skill_artifact
        else
          source_manifest_version=$(codex_projectatlas_plugin_source_manifest_version)
          printf "warning: Codex ProjectAtlas plugin update failed: source manifest version '%s' does not match %s after refresh.\n" "$source_manifest_version" "$runtime_version" >&2
          restore_codex_projectatlas_marketplace "$marketplace_source" "$previous_ref"
        fi
      else
        printf "warning: Codex ProjectAtlas plugin update failed: installed projectatlas plugin version '%s' does not match %s.\n" "$installed_version" "$runtime_version" >&2
        restore_codex_projectatlas_marketplace "$marketplace_source" "$previous_ref"
      fi
    else
      printf 'warning: Codex ProjectAtlas plugin update failed: could not install projectatlas plugin at %s.\n' "$release_tag" >&2
      restore_codex_projectatlas_marketplace "$marketplace_source" "$previous_ref"
    fi
    return 0
  fi

  if ! "$codex_bin" plugin marketplace remove projectatlas --json >/dev/null 2>&1; then
    printf '%s\n' "warning: Codex ProjectAtlas plugin update failed: could not remove stale projectatlas marketplace." >&2
    return 0
  fi
  if ! "$codex_bin" plugin marketplace add styler-ai/ProjectAtlas --ref "$release_tag" --json >/dev/null 2>&1; then
    printf 'warning: Codex ProjectAtlas plugin update failed: could not add projectatlas marketplace at %s.\n' "$release_tag" >&2
    restore_codex_projectatlas_marketplace "$marketplace_source" "$previous_ref"
    return 0
  fi
  "$codex_bin" plugin remove projectatlas --marketplace projectatlas --json >/dev/null 2>&1 || true
  if "$codex_bin" plugin add projectatlas --marketplace projectatlas --json >/dev/null 2>&1; then
    installed_version=$(codex_projectatlas_plugin_version)
    if [ "$installed_version" = "$runtime_version" ]; then
      if codex_projectatlas_plugin_source_manifest_matches "$runtime_version"; then
        printf 'Codex ProjectAtlas plugin marketplace updated to %s.\n' "$release_tag"
        verify_codex_projectatlas_skill_artifact
      else
        source_manifest_version=$(codex_projectatlas_plugin_source_manifest_version)
        printf "warning: Codex ProjectAtlas plugin update failed: source manifest version '%s' does not match %s after refresh.\n" "$source_manifest_version" "$runtime_version" >&2
        restore_codex_projectatlas_marketplace "$marketplace_source" "$previous_ref"
      fi
    else
      printf "warning: Codex ProjectAtlas plugin update failed: installed projectatlas plugin version '%s' does not match %s.\n" "$installed_version" "$runtime_version" >&2
      restore_codex_projectatlas_marketplace "$marketplace_source" "$previous_ref"
    fi
  else
    printf 'warning: Codex ProjectAtlas plugin update failed: could not install projectatlas plugin at %s.\n' "$release_tag" >&2
    restore_codex_projectatlas_marketplace "$marketplace_source" "$previous_ref"
  fi
}

update_codex_mcp_registry() {
  if truthy "${PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE:-}"; then
    printf '%s\n' "Codex MCP registry update skipped by PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE."
    return 0
  fi
  resolve_codex_command "Codex MCP registry update" || return 0
  runtime_version=$(expected_runtime_version)
  if [ -z "$runtime_version" ]; then
    runtime_version=$(runtime_version "$projectatlas_bin")
  fi
  if [ -z "$runtime_version" ]; then
    printf '%s\n' "Codex MCP registry update skipped: ProjectAtlas version is unknown."
    return 0
  fi
  existing=$("$codex_bin" mcp get projectatlas 2>&1) || {
    printf '%s\n' "Codex MCP registry update skipped: no global projectatlas MCP server is configured."
    return 0
  }
  expected_config=
  if [ -f "$project_config" ]; then
    expected_config=$project_config
  elif [ -f "$flat_config" ]; then
    expected_config=$flat_config
  fi
  if printf '%s\n' "$existing" | grep -F "$projectatlas_bin" >/dev/null &&
    printf '%s\n' "$existing" | grep -F "$runtime_version" >/dev/null &&
    printf '%s\n' "$existing" | grep -F "$atlas_dir/projectatlas.db" >/dev/null &&
    { [ -z "$expected_config" ] || printf '%s\n' "$existing" | grep -F "$expected_config" >/dev/null; }; then
    printf 'Codex MCP registry already points to ProjectAtlas %s for %s.\n' "$runtime_version" "$atlas_dir/projectatlas.db"
    return 0
  fi
  if ! "$codex_bin" mcp remove projectatlas >/dev/null 2>&1; then
    printf '%s\n' "warning: Codex MCP registry update failed: could not remove stale global projectatlas server." >&2
    return 0
  fi
  set -- mcp add projectatlas -- "$projectatlas_bin" --require-version "$runtime_version" --db "$atlas_dir/projectatlas.db"
  if [ -n "$expected_config" ]; then
    set -- "$@" --config "$expected_config"
  fi
  set -- "$@" mcp
  if "$codex_bin" "$@" >/dev/null 2>&1; then
    printf 'Codex MCP registry updated to ProjectAtlas runtime %s with database %s.\n' "$projectatlas_bin" "$atlas_dir/projectatlas.db"
  else
    printf '%s\n' "warning: Codex MCP registry update failed: could not add verified global projectatlas server." >&2
  fi
}

report_projectatlas_workflow_pins() {
  runtime_version=$(expected_runtime_version)
  if [ -z "$runtime_version" ]; then
    runtime_version=$(runtime_version "$projectatlas_bin")
  fi
  if [ -z "$runtime_version" ]; then
    return 0
  fi
  workflow_dir=$project_root/.github/workflows
  if [ ! -d "$workflow_dir" ]; then
    return 0
  fi
  release_tag=v$runtime_version
  find "$workflow_dir" -type f \( -name '*.yml' -o -name '*.yaml' \) | while IFS= read -r workflow_file; do
    line_number=0
    while IFS= read -r line || [ -n "$line" ]; do
      line_number=$((line_number + 1))
      case "$line" in
        *github.com/styler-ai/ProjectAtlas/releases/download/v*)
          printf '%s\n' "$line" |
            grep -Eo 'v[0-9]+\.[0-9]+\.[0-9]+' |
            sort -u |
            while IFS= read -r found_tag; do
              if [ "$found_tag" != "$release_tag" ]; then
                relative_path=${workflow_file#"$project_root"/}
                printf 'warning: Stale ProjectAtlas workflow release pin in %s:%s uses %s; expected %s.\n' "$relative_path" "$line_number" "$found_tag" "$release_tag" >&2
              fi
            done
          ;;
      esac
    done < "$workflow_file"
  done
}

download_release_file() {
  url=$1
  output=$2
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$output"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$url" -O "$output"
  else
    return 1
  fi
}

archive_sha256() {
  archive=$1
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$archive" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$archive" | awk '{print $1}'
  else
    return 1
  fi
}

verify_release_checksum() {
  archive=$1
  asset=$2
  checksums=$3
  expected=$(awk -v asset="$asset" '$2 == asset || $2 == "./" asset { print tolower($1); exit }' "$checksums")
  if [ -z "$expected" ]; then
    printf '%s\n' "SHA256SUMS did not contain an entry for $asset" >&2
    return 1
  fi
  actual=$(archive_sha256 "$archive") || {
    printf '%s\n' "Could not calculate SHA-256 for $asset" >&2
    return 1
  }
  if [ "$actual" != "$expected" ]; then
    printf '%s\n' "Checksum mismatch for $asset: expected $expected, found $actual" >&2
    return 1
  fi
}

install_release_binary() {
  if [ -z "$projectatlas_version" ]; then
    return 1
  fi
  os=$(uname -s)
  arch=$(uname -m)
  case "$os:$arch" in
    Linux:x86_64|Linux:amd64)
      suffix=x86_64-unknown-linux-gnu
      ;;
    Darwin:x86_64|Darwin:amd64)
      suffix=x86_64-apple-darwin
      ;;
    Darwin:arm64|Darwin:aarch64)
      suffix=aarch64-apple-darwin
      ;;
    *)
      return 1
      ;;
  esac
  asset="projectatlas-$projectatlas_version-$suffix.tar.gz"
  url="$release_base_url/$projectatlas_version/$asset"
  tmp_dir=$(mktemp -d)
  archive="$tmp_dir/$asset"
  checksums="$tmp_dir/SHA256SUMS"
  if ! download_release_file "$url" "$archive"; then
    rm -rf "$tmp_dir"
    return 1
  fi
  if ! download_release_file "$release_base_url/$projectatlas_version/SHA256SUMS" "$checksums"; then
    rm -rf "$tmp_dir"
    return 1
  fi
  if ! verify_release_checksum "$archive" "$asset" "$checksums"; then
    rm -rf "$tmp_dir"
    return 1
  fi
  tar -xzf "$archive" -C "$tmp_dir" || {
    rm -rf "$tmp_dir"
    return 1
  }
  mkdir -p "$HOME/.local/bin"
  cp "$tmp_dir/projectatlas/projectatlas" "$HOME/.local/bin/projectatlas" || {
    rm -rf "$tmp_dir"
    return 1
  }
  chmod +x "$HOME/.local/bin/projectatlas"
  rm -rf "$tmp_dir"
}

if [ -n "$runtime_override" ]; then
  if ! is_projectatlas_runtime "$runtime_override"; then
    printf '%s\n' "Provided ProjectAtlas runtime does not satisfy the ProjectAtlas runtime/version contract: $runtime_override" >&2
    exit 1
  fi
  projectatlas_bin=$runtime_override
else
  installed_bin=
  if truthy "$release_binary_only"; then
    install_release_binary || {
      printf '%s\n' "ProjectAtlas release-binary install was required but failed for $projectatlas_version." >&2
      exit 1
    }
    installed_bin="$HOME/.local/bin/projectatlas"
  elif install_release_binary; then
    installed_bin="$HOME/.local/bin/projectatlas"
  elif command -v cargo >/dev/null 2>&1; then
    if [ -n "$projectatlas_version" ]; then
      cargo install --git "$repository" --tag "$projectatlas_version" projectatlas-cli --locked --force
    else
      cargo install --git "$repository" projectatlas-cli --locked --force
    fi
  fi

  if [ -n "$installed_bin" ]; then
    projectatlas_bin=$installed_bin
  else
    projectatlas_bin=$(find_projectatlas || true)
  fi
  if [ -z "$projectatlas_bin" ]; then
    printf '%s\n' "A ProjectAtlas runtime matching $projectatlas_version was not found. Install Rust/Cargo or provide the matching ProjectAtlas release binary on PATH." >&2
    exit 1
  fi
  if ! is_projectatlas_runtime "$projectatlas_bin"; then
    printf '%s\n' "Installed ProjectAtlas runtime did not satisfy the ProjectAtlas runtime/version contract: $projectatlas_bin" >&2
    exit 1
  fi
fi

prepend_projectatlas_process_path "$projectatlas_bin"
"$projectatlas_bin" --format json runtime-info >/dev/null
confirm_bare_projectatlas_resolution "$projectatlas_bin"
quarantine_known_stale_projectatlas_shims "$projectatlas_bin"
warn_path_shadow "$projectatlas_bin"

if [ -L "$atlas_dir" ] || [ -h "$atlas_dir" ]; then
  printf '%s\n' "ProjectAtlas project state directory must not be a symlink: $atlas_dir" >&2
  exit 1
fi
if [ -e "$atlas_dir" ] && [ ! -d "$atlas_dir" ]; then
  printf '%s\n' "ProjectAtlas project state path must be a directory: $atlas_dir" >&2
  exit 1
fi
mkdir -p "$atlas_dir"
atlas_dir_canonical=$(CDPATH= cd -- "$atlas_dir" && pwd -P)
if [ "$atlas_dir_canonical" != "$atlas_dir" ]; then
  printf '%s\n' "ProjectAtlas project state directory escaped the canonical project root: $atlas_dir" >&2
  exit 1
fi
mcp_config_path="$atlas_dir/projectatlas.mcp.json"
claude_mcp_config_path="$atlas_dir/projectatlas.claude.mcp.json"
opencode_config_path="$atlas_dir/projectatlas.opencode.json"
flat_config="$project_root/projectatlas.toml"
project_config="$atlas_dir/config.toml"

assert_config_output_path() {
  output_path=$1
  if [ -L "$output_path" ] || [ -h "$output_path" ]; then
    printf '%s\n' "ProjectAtlas MCP config output must not be a symlink: $output_path" >&2
    return 1
  fi
  if [ -e "$output_path" ] && [ ! -f "$output_path" ]; then
    printf '%s\n' "ProjectAtlas MCP config output must be a regular file: $output_path" >&2
    return 1
  fi
}

write_mcp_config() {
  output_path=$1
  harness=${2:-}
  if [ -f "$project_config" ]; then
    set -- --format json --db "$atlas_dir/projectatlas.db" --config "$project_config" mcp-config
  elif [ -f "$flat_config" ]; then
    set -- --format json --db "$atlas_dir/projectatlas.db" --config "$flat_config" mcp-config
  else
    set -- --format json --db "$atlas_dir/projectatlas.db" mcp-config
  fi
  if [ -n "$harness" ]; then
    set -- "$@" --harness "$harness"
  fi
  assert_config_output_path "$output_path"
  "$projectatlas_bin" "$@" > "$output_path"
}

canonical_path() {
  candidate=$1
  if [ -d "$candidate" ]; then
    CDPATH= cd -- "$candidate" 2>/dev/null && pwd -P && return 0
  fi
  canonical_file "$candidate"
}

require_same_path() {
  actual=$1
  expected=$2
  label=$3
  if [ -z "$actual" ]; then
    printf '%s\n' "$label is missing." >&2
    return 1
  fi
  case "$actual" in
    /*) ;;
    *)
      printf '%s\n' "$label path is not absolute: $actual" >&2
      return 1
      ;;
  esac
  actual_canonical=$(canonical_path "$actual")
  expected_canonical=$(canonical_path "$expected")
  if [ "$actual_canonical" != "$expected_canonical" ]; then
    printf '%s\n' "$label path mismatch: expected $expected, found $actual" >&2
    return 1
  fi
}

require_json_parser() {
  if command -v jq >/dev/null 2>&1 || command -v python3 >/dev/null 2>&1; then
    return 0
  fi
  printf '%s\n' "ProjectAtlas generated MCP config verification requires jq or python3 for JSON parsing." >&2
  return 1
}

generated_claude_command() {
  config_path=$1
  if command -v jq >/dev/null 2>&1; then
    jq -r '.mcpServers.projectatlas.command // empty' "$config_path"
    return 0
  fi
  require_json_parser || return 1
  python3 - "$config_path" <<'PY'
import json
import sys
with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
print(payload.get("mcpServers", {}).get("projectatlas", {}).get("command", ""))
PY
}

generated_claude_args() {
  config_path=$1
  if command -v jq >/dev/null 2>&1; then
    jq -r '.mcpServers.projectatlas.args[]? // empty' "$config_path"
    return 0
  fi
  require_json_parser || return 1
  python3 - "$config_path" <<'PY'
import json
import sys
with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
for arg in payload.get("mcpServers", {}).get("projectatlas", {}).get("args", []):
    print(arg)
PY
}

generated_claude_has_cwd() {
  config_path=$1
  if command -v jq >/dev/null 2>&1; then
    jq -e '.mcpServers.projectatlas | has("cwd")' "$config_path" >/dev/null
    return $?
  fi
  require_json_parser || return 1
  python3 - "$config_path" <<'PY'
import json
import sys
with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
server = payload.get("mcpServers", {}).get("projectatlas", {})
sys.exit(0 if "cwd" in server else 1)
PY
}

generated_opencode_string() {
  config_path=$1
  key=$2
  if command -v jq >/dev/null 2>&1; then
    jq -r ".mcp.projectatlas.$key // empty" "$config_path"
    return 0
  fi
  require_json_parser || return 1
  python3 - "$config_path" "$key" <<'PY'
import json
import sys
with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
value = payload.get("mcp", {}).get("projectatlas", {}).get(sys.argv[2], "")
print(value if isinstance(value, str) else "")
PY
}

generated_opencode_enabled() {
  config_path=$1
  if command -v jq >/dev/null 2>&1; then
    jq -r '.mcp.projectatlas.enabled // empty' "$config_path"
    return 0
  fi
  require_json_parser || return 1
  python3 - "$config_path" <<'PY'
import json
import sys
with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
value = payload.get("mcp", {}).get("projectatlas", {}).get("enabled", "")
if isinstance(value, bool):
    print("true" if value else "false")
PY
}

generated_opencode_command_array() {
  config_path=$1
  if command -v jq >/dev/null 2>&1; then
    jq -r '.mcp.projectatlas.command[]? // empty' "$config_path"
    return 0
  fi
  require_json_parser || return 1
  python3 - "$config_path" <<'PY'
import json
import sys
with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
for arg in payload.get("mcp", {}).get("projectatlas", {}).get("command", []):
    print(arg)
PY
}

effective_config_path() {
  if [ -f "$project_config" ]; then
    printf '%s\n' "$project_config"
  elif [ -f "$flat_config" ]; then
    printf '%s\n' "$flat_config"
  fi
}

require_arg_value() {
  args_text=$1
  name=$2
  expected=$3
  label=$4
  path_value=${5:-}
  seen_name=0
  while IFS= read -r arg; do
    if [ "$seen_name" -eq 1 ]; then
      if [ "$path_value" = path ]; then
        require_same_path "$arg" "$expected" "$label" || return 1
      elif [ "$arg" != "$expected" ]; then
        printf '%s\n' "$label mismatch: expected $expected, found $arg" >&2
        return 1
      fi
      return 0
    fi
    if [ "$arg" = "$name" ]; then
      seen_name=1
    fi
  done <<EOF
$args_text
EOF
  printf '%s\n' "$label argument $name is missing." >&2
  return 1
}

verify_generated_mcp_config() {
  config_path=$1
  harness=$2
  runtime_version=$(expected_runtime_version)
  if [ -z "$runtime_version" ]; then
    runtime_version=$(runtime_version "$projectatlas_bin")
  fi
  if [ -z "$runtime_version" ]; then
    printf '%s\n' "$harness ProjectAtlas generated MCP config cannot be verified because the runtime version is unknown." >&2
    return 1
  fi
  [ -f "$config_path" ] || {
    printf '%s\n' "$harness ProjectAtlas generated MCP config was not written: $config_path" >&2
    return 1
  }
  case "$harness" in
    "Claude Code")
      command_path=$(generated_claude_command "$config_path") || return 1
      require_same_path "$command_path" "$projectatlas_bin" "Claude Code command" || return 1
      args_text=$(generated_claude_args "$config_path") || return 1
      if generated_claude_has_cwd "$config_path"; then
        printf '%s\n' "Claude Code generated MCP config must not rely on cwd." >&2
        return 1
      fi
      ;;
    "OpenCode")
      server_type=$(generated_opencode_string "$config_path" type) || return 1
      [ "$server_type" = local ] || {
        printf '%s\n' "OpenCode generated MCP config type mismatch." >&2
        return 1
      }
      server_enabled=$(generated_opencode_enabled "$config_path") || return 1
      [ "$server_enabled" = true ] || {
        printf '%s\n' "OpenCode generated MCP config must set enabled=true." >&2
        return 1
      }
      server_cwd=$(generated_opencode_string "$config_path" cwd) || return 1
      require_same_path "$server_cwd" "$project_root" "OpenCode cwd" || return 1
      command_array=$(generated_opencode_command_array "$config_path") || return 1
      command_path=$(printf '%s\n' "$command_array" | sed -n '1p')
      require_same_path "$command_path" "$projectatlas_bin" "OpenCode command" || return 1
      args_text=$(printf '%s\n' "$command_array" | sed -n '2,$p')
      ;;
    *)
      printf '%s\n' "Unsupported generated MCP config harness: $harness" >&2
      return 1
      ;;
  esac
  require_arg_value "$args_text" --require-version "$runtime_version" "$harness --require-version" || return 1
  require_arg_value "$args_text" --db "$atlas_dir/projectatlas.db" "$harness --db" path || return 1
  expected_config=$(effective_config_path)
  if [ -n "$expected_config" ]; then
    require_arg_value "$args_text" --config "$expected_config" "$harness --config" path || return 1
  fi
  last_arg=$(printf '%s\n' "$args_text" | sed '/^$/d' | tail -n 1)
  [ "$last_arg" = mcp ] || {
    printf '%s\n' "$harness generated MCP config does not end with mcp." >&2
    return 1
  }
  printf '%s\n' "$harness ProjectAtlas generated MCP config verified for runtime $projectatlas_bin and database $atlas_dir/projectatlas.db."
}

write_mcp_config "$mcp_config_path"
write_mcp_config "$claude_mcp_config_path" claude-code
write_mcp_config "$opencode_config_path" opencode
verify_generated_mcp_config "$claude_mcp_config_path" "Claude Code"
verify_generated_mcp_config "$opencode_config_path" "OpenCode"
update_codex_plugin
update_codex_mcp_registry
report_projectatlas_workflow_pins

printf 'ProjectAtlas runtime installed and verified: %s\n' "$projectatlas_bin"
printf 'ProjectAtlas update preserved project state under %s; use reset-index --apply for explicit state cleanup.\n' "$atlas_dir"
printf 'Project-local MCP config written: %s\n' "$mcp_config_path"
printf 'Project-local Claude Code MCP config written: %s\n' "$claude_mcp_config_path"
printf 'Project-local OpenCode MCP config written: %s\n' "$opencode_config_path"
printf '%s\n' "Claude Code ProjectAtlas integration verified through generated MCP config; restart Claude Code if an older session cached previous instructions."
printf '%s\n' "OpenCode ProjectAtlas integration verified through generated MCP config; restart OpenCode if an older session cached previous instructions."
