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
  major_version=$(printf '%s\n' "$runtime_info" | sed -n 's/.*"major_version"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p')
  runtime_version=$(printf '%s\n' "$runtime_info" | sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)
  expected_version=$(expected_runtime_version)
  case "$runtime_info" in
    *'"project": "ProjectAtlas"'*'"mcp"'*'"text_format": "TOON"'*)
      [ "${major_version:-0}" -ge 3 ] 2>/dev/null &&
        { [ -z "$expected_version" ] || [ "$runtime_version" = "$expected_version" ]; }
      ;;
    *)
      return 1
      ;;
  esac
}

runtime_version() {
  candidate=$1
  runtime_info=$("$candidate" --format json runtime-info 2>/dev/null || true)
  printf '%s\n' "$runtime_info" | sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1
}

canonical_file() {
  file=$1
  dir=$(CDPATH= cd -- "$(dirname -- "$file")" 2>/dev/null && pwd -P) || {
    printf '%s\n' "$file"
    return 0
  }
  printf '%s/%s\n' "$dir" "$(basename -- "$file")"
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

"$projectatlas_bin" --format json runtime-info >/dev/null
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

printf 'ProjectAtlas runtime installed and verified: %s\n' "$projectatlas_bin"
printf 'ProjectAtlas update preserved project state under %s; use reset-index --apply for explicit state cleanup.\n' "$atlas_dir"
printf 'Project-local MCP config written: %s\n' "$mcp_config_path"
printf 'Project-local Claude Code MCP config written: %s\n' "$claude_mcp_config_path"
printf 'Project-local OpenCode MCP config written: %s\n' "$opencode_config_path"
