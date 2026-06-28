#!/usr/bin/env sh
set -eu

repository=${PROJECTATLAS_REPOSITORY:-https://github.com/styler-ai/ProjectAtlas}
projectatlas_version=${PROJECTATLAS_VERSION:-v0.3.0}
release_base_url=${PROJECTATLAS_RELEASE_BASE_URL:-https://github.com/styler-ai/ProjectAtlas/releases/download}

if [ "${1:-}" ]; then
  project_root=$1
else
  project_root=$(pwd -P)
fi

find_projectatlas() {
  if [ -x "$HOME/.local/bin/projectatlas" ] && is_projectatlas3 "$HOME/.local/bin/projectatlas"; then
    printf '%s\n' "$HOME/.local/bin/projectatlas"
    return 0
  fi
  if [ -x "$HOME/.cargo/bin/projectatlas" ] && is_projectatlas3 "$HOME/.cargo/bin/projectatlas"; then
    printf '%s\n' "$HOME/.cargo/bin/projectatlas"
    return 0
  fi
  if command -v projectatlas >/dev/null 2>&1 && is_projectatlas3 "$(command -v projectatlas)"; then
    command -v projectatlas
    return 0
  fi
  return 1
}

is_projectatlas3() {
  candidate=$1
  help_text=$("$candidate" --help 2>/dev/null || true)
  case "$help_text" in
    *"ProjectAtlas 3 repository intelligence engine"*"mcp-config"*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
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

if command -v cargo >/dev/null 2>&1 && [ -f "$project_root/crates/projectatlas-cli/Cargo.toml" ]; then
  (cd "$project_root" && cargo install --path crates/projectatlas-cli --locked --force)
elif install_release_binary; then
  :
elif command -v cargo >/dev/null 2>&1; then
  if [ -n "$projectatlas_version" ]; then
    cargo install --git "$repository" --tag "$projectatlas_version" --package projectatlas-cli --locked --force
  else
    cargo install --git "$repository" --package projectatlas-cli --locked --force
  fi
fi

projectatlas_bin=$(find_projectatlas || true)
if [ -z "$projectatlas_bin" ]; then
  printf '%s\n' "ProjectAtlas 3 runtime was not found. Install Rust/Cargo or provide a compatible ProjectAtlas 3 release binary on PATH." >&2
  exit 1
fi

"$projectatlas_bin" --help >/dev/null

atlas_dir="$project_root/.projectatlas"
mkdir -p "$atlas_dir"
mcp_config_path="$atlas_dir/projectatlas.mcp.json"
if [ -f "$atlas_dir/config.toml" ]; then
  "$projectatlas_bin" --format json --db "$atlas_dir/projectatlas.db" --config "$atlas_dir/config.toml" mcp-config > "$mcp_config_path"
else
  "$projectatlas_bin" --format json --db "$atlas_dir/projectatlas.db" mcp-config > "$mcp_config_path"
fi

printf 'ProjectAtlas runtime installed and verified: %s\n' "$projectatlas_bin"
printf 'Project-local MCP config written: %s\n' "$mcp_config_path"
