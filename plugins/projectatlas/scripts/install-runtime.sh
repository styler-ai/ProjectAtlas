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

update_codex_mcp_registry() {
  if truthy "${PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE:-}"; then
    printf '%s\n' "Codex MCP registry update skipped by PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE."
    return 0
  fi
  codex_bin=${PROJECTATLAS_CODEX_COMMAND:-}
  if [ -z "$codex_bin" ]; then
    codex_bin=$(command -v codex 2>/dev/null || true)
  fi
  if [ -z "$codex_bin" ]; then
    printf '%s\n' "Codex MCP registry update skipped: codex command not found."
    return 0
  fi
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
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$archive" || {
      rm -rf "$tmp_dir"
      return 1
    }
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$url" -O "$archive" || {
      rm -rf "$tmp_dir"
      return 1
    }
  else
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
  elif command -v cargo >/dev/null 2>&1 && [ -f "$project_root/crates/projectatlas-cli/Cargo.toml" ]; then
    (cd "$project_root" && cargo install --path crates/projectatlas-cli --locked --force)
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

atlas_dir="$project_root/.projectatlas"
mkdir -p "$atlas_dir"
mcp_config_path="$atlas_dir/projectatlas.mcp.json"
claude_mcp_config_path="$atlas_dir/projectatlas.claude.mcp.json"
opencode_config_path="$atlas_dir/projectatlas.opencode.json"
flat_config="$project_root/projectatlas.toml"
project_config="$atlas_dir/config.toml"

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
  "$projectatlas_bin" "$@" > "$output_path"
}

write_mcp_config "$mcp_config_path"
write_mcp_config "$claude_mcp_config_path" claude-code
write_mcp_config "$opencode_config_path" opencode
update_codex_mcp_registry

printf 'ProjectAtlas runtime installed and verified: %s\n' "$projectatlas_bin"
printf 'ProjectAtlas update preserved project state under %s; use reset-index --apply for explicit state cleanup.\n' "$atlas_dir"
printf 'Project-local MCP config written: %s\n' "$mcp_config_path"
printf 'Project-local Claude Code MCP config written: %s\n' "$claude_mcp_config_path"
printf 'Project-local OpenCode MCP config written: %s\n' "$opencode_config_path"
