#!/usr/bin/env sh
# Purpose: Install or update the ProjectAtlas plugin runtime and POSIX MCP configs.
set -eu

repository=${PROJECTATLAS_REPOSITORY:-https://github.com/styler-ai/ProjectAtlas}
projectatlas_version=${PROJECTATLAS_VERSION:-}
release_base_url=${PROJECTATLAS_RELEASE_BASE_URL:-https://github.com/styler-ai/ProjectAtlas/releases/download}
release_binary_only=${PROJECTATLAS_RELEASE_BINARY_ONLY:-}
runtime_override=${PROJECTATLAS_RUNTIME_PATH:-}
uninstall=0
if [ "${1:-}" = "--uninstall" ]; then
  uninstall=1
  shift
fi

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
  printf '%s\n' "$HOME/.local/bin/projectatlas"
  printf '%s\n' "$HOME/.cargo/bin/projectatlas"
  printf '%s\n' "$HOME/.npm/bin/projectatlas"
  printf '%s\n' "$HOME/.npm-global/bin/projectatlas"
  printf '%s\n' "$HOME/.local/share/npm/bin/projectatlas"
}

atlas_forwarder_path() {
  runtime=$1
  printf '%s/atlas\n' "$(dirname -- "$runtime")"
}

atlas_forwarder_provenance_path() {
  forwarder=$1
  printf '%s/.atlas-forwarder.provenance\n' "$(dirname -- "$forwarder")"
}

atlas_forwarder_state_root() {
  printf '%s/projectatlas\n' "${XDG_STATE_HOME:-$HOME/.local/state}"
}

atlas_forwarder_state_path() {
  forwarder=$1
  canonical_forwarder=$(canonical_file "$forwarder") || return 1
  if command -v sha256sum >/dev/null 2>&1; then
    digest=$(printf '%s' "$canonical_forwarder" | sha256sum | awk '{print $1}')
  elif command -v shasum >/dev/null 2>&1; then
    digest=$(printf '%s' "$canonical_forwarder" | shasum -a 256 | awk '{print $1}')
  else
    printf '%s\n' "ProjectAtlas requires sha256sum or shasum for atlas forwarder installer state." >&2
    return 1
  fi
  printf '%s/atlas-forwarder-%s.state\n' "$(atlas_forwarder_state_root)" "$digest"
}

atlas_forwarder_lifecycle_lock_path=
atlas_forwarder_lifecycle_lock_root=
atlas_forwarder_lifecycle_lock_identity=
atlas_forwarder_lifecycle_lock_count=0
atlas_forwarder_lifecycle_lock_path_1=
atlas_forwarder_lifecycle_lock_root_1=
atlas_forwarder_lifecycle_lock_identity_1=
atlas_forwarder_lifecycle_lock_path_2=
atlas_forwarder_lifecycle_lock_root_2=
atlas_forwarder_lifecycle_lock_identity_2=
atlas_forwarder_lifecycle_lock_remaining_ms=30000

atlas_forwarder_lifecycle_lock_budget_ms() {
  lock_budget=${PROJECTATLAS_TEST_ATLAS_FORWARDER_LOCK_TIMEOUT_MS:-30000}
  case "$lock_budget" in
    ''|*[!0-9]*) lock_budget=30000 ;;
  esac
  printf '%s\n' "$lock_budget"
}

acquire_atlas_forwarder_flock_with_deadline() {
  lock_fd=$1
  if flock -n "$lock_fd"; then
    return 0
  fi
  while [ "$atlas_forwarder_lifecycle_lock_remaining_ms" -gt 0 ]; do
    sleep 0.025
    atlas_forwarder_lifecycle_lock_remaining_ms=$((atlas_forwarder_lifecycle_lock_remaining_ms - 25))
    if [ "$atlas_forwarder_lifecycle_lock_remaining_ms" -gt 0 ] && flock -n "$lock_fd"; then
      return 0
    fi
  done
  return 1
}

acquire_atlas_forwarder_lifecycle_lock_fd() {
  forwarder=$1
  lock_fd=$2
  case "$lock_fd" in
    7|8) ;;
    *)
      printf '%s\n' "ProjectAtlas received an unsupported atlas forwarder lifecycle lock descriptor: $lock_fd" >&2
      return 1
      ;;
  esac
  state_path=$(atlas_forwarder_state_path "$forwarder") || return 1
  lock_root=$(dirname -- "$state_path")
  mkdir -p -- "$lock_root" || {
    printf '%s\n' "ProjectAtlas could not create the atlas forwarder lifecycle lock root: $lock_root" >&2
    return 1
  }
  lock_path=${state_path%.state}.lock
  if [ ! -e "$lock_path" ] && [ ! -L "$lock_path" ]; then
    (umask 077 && set -C && : > "$lock_path") 2>/dev/null || true
  fi
  lock_parent_resolved=$(CDPATH= cd -P -- "$(dirname -- "$lock_path")" 2>/dev/null && pwd -P) || return 1
  lock_identity=$(stat -c '%d:%i' -- "$lock_path" 2>/dev/null || stat -f '%d:%i' "$lock_path" 2>/dev/null || true)
  lock_links=$(stat -c %h -- "$lock_path" 2>/dev/null || stat -f %l "$lock_path" 2>/dev/null || true)
  lock_owner_uid=$(stat -c %u -- "$lock_path" 2>/dev/null || stat -f %u "$lock_path" 2>/dev/null || true)
  if [ "$lock_parent_resolved" != "$lock_root" ] || [ -L "$lock_path" ] ||
    [ ! -f "$lock_path" ] || [ -z "$lock_identity" ] || [ "$lock_links" != 1 ] ||
    [ "$lock_owner_uid" != "$(id -u)" ]; then
    printf '%s\n' "ProjectAtlas atlas forwarder lifecycle lock is not a trusted direct file: $lock_path" >&2
    return 1
  fi

  if [ "$lock_fd" = 8 ]; then
    exec 8<> "$lock_path" || return 1
  else
    exec 7<> "$lock_path" || return 1
  fi
  lock_acquired=false
  lock_platform=$(uname -s 2>/dev/null || true)
  lock_device=${lock_identity%%:*}
  lock_inode=${lock_identity#*:}
  lock_runtime=${projectatlas_bin:-${runtime_override:-}}
  if [ -z "$lock_runtime" ] && [ "$lock_platform" = Darwin ]; then
    lock_runtime=$(find_projectatlas || true)
  fi
  if [ -n "$lock_runtime" ]; then
    lock_elapsed_ms=
    if [ "$lock_fd" = 8 ]; then
      lock_elapsed_ms=$("$lock_runtime" acquire-installer-lock "$lock_device" "$lock_inode" \
        "$atlas_forwarder_lifecycle_lock_remaining_ms" --report-elapsed <&8)
    else
      lock_elapsed_ms=$("$lock_runtime" acquire-installer-lock "$lock_device" "$lock_inode" \
        "$atlas_forwarder_lifecycle_lock_remaining_ms" --report-elapsed <&7)
    fi
    helper_result=$?
    if [ "$helper_result" -eq 0 ]; then
      case "$lock_elapsed_ms" in
        ''|*[!0-9]*) helper_result=1 ;;
        *)
          if [ "$lock_elapsed_ms" -ge "$atlas_forwarder_lifecycle_lock_remaining_ms" ]; then
            atlas_forwarder_lifecycle_lock_remaining_ms=0
          else
            atlas_forwarder_lifecycle_lock_remaining_ms=$((atlas_forwarder_lifecycle_lock_remaining_ms - lock_elapsed_ms))
          fi
          ;;
      esac
    fi
    if [ "$helper_result" -eq 0 ]; then
      if [ "$lock_platform" = Darwin ]; then
        lock_acquired=true
      elif command -v flock >/dev/null 2>&1 && acquire_atlas_forwarder_flock_with_deadline "$lock_fd"; then
        lock_acquired=true
      fi
    fi
  elif [ "$lock_platform" != Darwin ] && command -v flock >/dev/null 2>&1; then
    if acquire_atlas_forwarder_flock_with_deadline "$lock_fd"; then
      lock_acquired=true
    fi
  fi
  if [ "$lock_acquired" != true ]; then
    if [ "$lock_fd" = 8 ]; then
      exec 8>&-
    else
      exec 7>&-
    fi
    printf '%s\n' "ProjectAtlas atlas forwarder lifecycle is busy or no supported lock utility is available: $forwarder" >&2
    return 1
  fi

  current_lock_identity=$(stat -c '%d:%i' -- "$lock_path" 2>/dev/null || stat -f '%d:%i' "$lock_path" 2>/dev/null || true)
  current_lock_links=$(stat -c %h -- "$lock_path" 2>/dev/null || stat -f %l "$lock_path" 2>/dev/null || true)
  if [ -L "$lock_path" ] || [ ! -f "$lock_path" ] || [ "$current_lock_links" != 1 ] ||
    [ "$current_lock_identity" != "$lock_identity" ]; then
    if [ "$lock_fd" = 8 ]; then
      exec 8>&-
    else
      exec 7>&-
    fi
    printf '%s\n' "ProjectAtlas atlas forwarder lifecycle lock changed while acquiring: $lock_path" >&2
    return 1
  fi
  atlas_forwarder_lifecycle_lock_acquired_path=$lock_path
  atlas_forwarder_lifecycle_lock_acquired_root=$lock_root
  atlas_forwarder_lifecycle_lock_acquired_identity=$lock_identity
}

acquire_atlas_forwarder_lifecycle_lock() {
  atlas_forwarder_lifecycle_lock_remaining_ms=$(atlas_forwarder_lifecycle_lock_budget_ms)
  acquire_atlas_forwarder_lifecycle_lock_fd "$1" 8 || return 1
  atlas_forwarder_lifecycle_lock_path=$atlas_forwarder_lifecycle_lock_acquired_path
  atlas_forwarder_lifecycle_lock_root=$atlas_forwarder_lifecycle_lock_acquired_root
  atlas_forwarder_lifecycle_lock_identity=$atlas_forwarder_lifecycle_lock_acquired_identity
}

release_atlas_forwarder_lifecycle_lock_fd() {
  lock_fd=$1
  lock_path=$2
  lock_root=$3
  expected_lock_identity=$4
  if [ -z "$lock_path" ]; then
    return 0
  fi
  lock_parent=$(dirname -- "$lock_path")
  lock_parent_resolved=$(CDPATH= cd -P -- "$lock_parent" 2>/dev/null && pwd -P) || lock_parent_resolved=
  lock_identity=
  if [ -f "$lock_path" ] && [ ! -L "$lock_path" ]; then
    lock_identity=$(stat -c '%d:%i' -- "$lock_path" 2>/dev/null || stat -f '%d:%i' "$lock_path" 2>/dev/null || true)
  fi
  lock_links=$(stat -c %h -- "$lock_path" 2>/dev/null || stat -f %l "$lock_path" 2>/dev/null || true)
  lock_release_valid=true
  if [ "$lock_parent_resolved" != "$lock_root" ] ||
    [ -L "$lock_path" ] || [ ! -f "$lock_path" ] ||
    [ "$lock_identity" != "$expected_lock_identity" ] || [ "$lock_links" != 1 ]; then
    lock_release_valid=false
  fi
  if [ "$lock_fd" = 8 ]; then
    exec 8>&-
  else
    exec 7>&-
  fi
  if [ "$lock_release_valid" != true ]; then
    printf '%s\n' "ProjectAtlas atlas forwarder lifecycle lock cleanup refused/path changed: $lock_path" >&2
    return 1
  fi
}

release_atlas_forwarder_lifecycle_lock() {
  if [ -z "$atlas_forwarder_lifecycle_lock_path" ]; then
    return 0
  fi
  result=0
  release_atlas_forwarder_lifecycle_lock_fd 8 \
    "$atlas_forwarder_lifecycle_lock_path" \
    "$atlas_forwarder_lifecycle_lock_root" \
    "$atlas_forwarder_lifecycle_lock_identity" || result=$?
  atlas_forwarder_lifecycle_lock_path=
  atlas_forwarder_lifecycle_lock_root=
  atlas_forwarder_lifecycle_lock_identity=
  return "$result"
}

acquire_atlas_forwarder_lifecycle_lock_set() {
  # A write can retire one prior owned source, so acquire both identities in a
  # stable order before either forwarder lifecycle is allowed to mutate.
  destination=$1
  source=${2:-}
  atlas_forwarder_lifecycle_lock_remaining_ms=$(atlas_forwarder_lifecycle_lock_budget_ms)
  destination_key=$(canonical_file "$destination") || return 1
  source_key=
  if [ -n "$source" ]; then
    source_key=$(canonical_file "$source") || return 1
  fi
  first_forwarder=$destination
  second_forwarder=
  if [ -n "$source" ] && [ "$source_key" != "$destination_key" ]; then
    if [ "$source_key" \< "$destination_key" ]; then
      first_forwarder=$source
      second_forwarder=$destination
    else
      second_forwarder=$source
    fi
  fi
  acquire_atlas_forwarder_lifecycle_lock_fd "$first_forwarder" 8 || return 1
  first_lock_path=$atlas_forwarder_lifecycle_lock_acquired_path
  first_lock_root=$atlas_forwarder_lifecycle_lock_acquired_root
  first_lock_identity=$atlas_forwarder_lifecycle_lock_acquired_identity
  if [ -n "$second_forwarder" ]; then
    if acquire_atlas_forwarder_lifecycle_lock_fd "$second_forwarder" 7; then
      atlas_forwarder_lifecycle_lock_count=2
      atlas_forwarder_lifecycle_lock_path_1=$first_lock_path
      atlas_forwarder_lifecycle_lock_root_1=$first_lock_root
      atlas_forwarder_lifecycle_lock_identity_1=$first_lock_identity
      atlas_forwarder_lifecycle_lock_path_2=$atlas_forwarder_lifecycle_lock_acquired_path
      atlas_forwarder_lifecycle_lock_root_2=$atlas_forwarder_lifecycle_lock_acquired_root
      atlas_forwarder_lifecycle_lock_identity_2=$atlas_forwarder_lifecycle_lock_acquired_identity
    else
      second_lock_result=$?
      release_result=0
      release_atlas_forwarder_lifecycle_lock_fd 8 "$first_lock_path" "$first_lock_root" "$first_lock_identity" || release_result=$?
      if [ "$release_result" -ne 0 ]; then
        printf '%s\n' "ProjectAtlas atlas forwarder lifecycle second lock acquisition failed; partial lock release also failed." >&2
      fi
      return "$second_lock_result"
    fi
  else
    atlas_forwarder_lifecycle_lock_count=1
    atlas_forwarder_lifecycle_lock_path_1=$first_lock_path
    atlas_forwarder_lifecycle_lock_root_1=$first_lock_root
    atlas_forwarder_lifecycle_lock_identity_1=$first_lock_identity
  fi
}

release_atlas_forwarder_lifecycle_lock_set() {
  result=0
  if [ "$atlas_forwarder_lifecycle_lock_count" -eq 2 ]; then
    release_atlas_forwarder_lifecycle_lock_fd 7 \
      "$atlas_forwarder_lifecycle_lock_path_2" \
      "$atlas_forwarder_lifecycle_lock_root_2" \
      "$atlas_forwarder_lifecycle_lock_identity_2" || result=$?
  fi
  if [ "$atlas_forwarder_lifecycle_lock_count" -ge 1 ]; then
    release_atlas_forwarder_lifecycle_lock_fd 8 \
      "$atlas_forwarder_lifecycle_lock_path_1" \
      "$atlas_forwarder_lifecycle_lock_root_1" \
      "$atlas_forwarder_lifecycle_lock_identity_1" || result=$?
  fi
  atlas_forwarder_lifecycle_lock_count=0
  atlas_forwarder_lifecycle_lock_path_1=
  atlas_forwarder_lifecycle_lock_root_1=
  atlas_forwarder_lifecycle_lock_identity_1=
  atlas_forwarder_lifecycle_lock_path_2=
  atlas_forwarder_lifecycle_lock_root_2=
  atlas_forwarder_lifecycle_lock_identity_2=
  return "$result"
}

new_atlas_forwarder_capability() {
  capability=$(od -An -N32 -tx1 /dev/urandom 2>/dev/null | tr -d '[:space:]' || true)
  case "$capability" in
    [0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F]* ) ;;
    *) return 1 ;;
  esac
  [ "${#capability}" -eq 64 ] || return 1
  printf '%s\n' "$capability"
}

atlas_forwarder_state_content() {
  forwarder=$1
  verified=$2
  capability=$3
  canonical_forwarder=$(canonical_file "$forwarder") || return 1
  canonical_verified=$(canonical_file "$verified") || return 1
  printf '%s\n' '# ProjectAtlas atlas forwarder installer state v1'
  printf '%s\n' "forwarder: $canonical_forwarder"
  printf '%s\n' "runtime: $canonical_verified"
  printf '%s\n' "capability: $capability"
}

atlas_forwarder_state_capability() {
  forwarder=$1
  verified=$2
  state_path=$(atlas_forwarder_state_path "$forwarder") || return 1
  is_direct_regular_file "$state_path" || return 1
  expected_forwarder=$(canonical_file "$forwarder") || return 1
  expected_verified=$(canonical_file "$verified") || return 1
  state_forwarder=$(sed -n 's/^forwarder: //p' "$state_path" | head -n 1)
  state_verified=$(sed -n 's/^runtime: //p' "$state_path" | head -n 1)
  capability=$(sed -n 's/^capability: //p' "$state_path" | head -n 1)
  [ "$(sed -n '1p' "$state_path")" = '# ProjectAtlas atlas forwarder installer state v1' ] || return 1
  [ "$state_forwarder" = "$expected_forwarder" ] || return 1
  [ "$state_verified" = "$expected_verified" ] || return 1
  case "$capability" in
    [0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F]* ) ;;
    *) return 1 ;;
  esac
  [ "${#capability}" -eq 64 ] || return 1
  printf '%s\n' "$capability"
}

ensure_atlas_forwarder_state() {
  forwarder=$1
  verified=$2
  ATLAS_FORWARDER_STATE_PUBLISHED=0
  state_path=$(atlas_forwarder_state_path "$forwarder") || return 1
  if [ -e "$state_path" ] || [ -L "$state_path" ]; then
    atlas_forwarder_state_capability "$forwarder" "$verified" >/dev/null || {
      printf '%s\n' "ProjectAtlas atlas forwarder installer state is missing, mismatched, or linked: $state_path" >&2
      return 1
    }
    if is_managed_atlas_forwarder "$forwarder" "$verified"; then
      return 0
    fi
    if ! remove_atlas_forwarder_state "$forwarder" "$verified"; then
      printf '%s\n' "ProjectAtlas atlas forwarder installer state is retained without a managed forwarder; refusing to reuse: $state_path" >&2
      return 1
    fi
  fi
  state_root=$(atlas_forwarder_state_root)
  mkdir -p -- "$state_root" || return 1
  chmod 700 "$state_root" 2>/dev/null || true
  capability=$(new_atlas_forwarder_capability) || return 1
  temporary=$(mktemp "$state_root/.atlas-forwarder-state.XXXXXX") || return 1
  chmod 600 "$temporary" 2>/dev/null || true
  if ! atlas_forwarder_state_content "$forwarder" "$verified" "$capability" >"$temporary"; then
    rm -f -- "$temporary"
    return 1
  fi
  if ln "$temporary" "$state_path" 2>/dev/null; then
    rm -f -- "$temporary"
    ATLAS_FORWARDER_STATE_PUBLISHED=1
    return 0
  fi
  rm -f -- "$temporary"
  atlas_forwarder_state_capability "$forwarder" "$verified" >/dev/null || return 1
  return 0
}

shell_quote() {
  value=$1
  value=$(printf '%s' "$value" | sed "s/'/'\\\\''/g")
  printf "'%s'" "$value"
}

atlas_forwarder_content() {
  verified=$1
  canonical_verified=$(canonical_file "$verified") || return 1
  target_quoted=$(shell_quote "$canonical_verified") || return 1
  printf '%s\n' '# ProjectAtlas managed atlas forwarder.'
  printf '%s\n' "# target: $canonical_verified"
  printf 'exec %s "$@"\n' "$target_quoted"
}

atlas_forwarder_provenance_content() {
  forwarder=$1
  verified=$2
  canonical_forwarder=$(canonical_file "$forwarder") || return 1
  canonical_verified=$(canonical_file "$verified") || return 1
  printf '%s\n' '# ProjectAtlas atlas forwarder provenance v1'
  printf '%s\n' "forwarder: $canonical_forwarder"
  printf '%s\n' "runtime: $canonical_verified"
}

is_direct_regular_file() {
  candidate=$1
  [ -f "$candidate" ] && [ ! -L "$candidate" ] || return 1
  candidate_links=$(stat -c %h -- "$candidate" 2>/dev/null || stat -f %l "$candidate" 2>/dev/null || true)
  [ "$candidate_links" = 1 ]
}

is_atlas_forwarder_provenance() {
  provenance=$1
  forwarder=$2
  verified=$3
  atlas_forwarder_state_capability "$forwarder" "$verified" >/dev/null || return 1
  is_direct_regular_file "$provenance" || return 1
  expected_content=$(atlas_forwarder_provenance_content "$forwarder" "$verified") || return 1
  actual_content=$(cat "$provenance" 2>/dev/null || true)
  [ "$actual_content" = "$expected_content" ]
}

remove_published_atlas_forwarder_provenance() {
  provenance=$1
  forwarder=$2
  verified=$3
  if is_atlas_forwarder_provenance "$provenance" "$forwarder" "$verified"; then
    quarantine=$(new_atlas_forwarder_quarantine "$provenance") || return 1
    if ! mv -- "$provenance" "$quarantine"; then
      rm -f -- "$quarantine"
      return 1
    fi
    expected=$(atlas_forwarder_provenance_content "$forwarder" "$verified") || return 1
    actual=$(cat "$quarantine" 2>/dev/null || true)
    if [ "$actual" != "$expected" ] || [ -e "$provenance" ] || [ -L "$provenance" ]; then
      restore_atlas_forwarder_quarantine "$quarantine" "$provenance"
      return 1
    fi
    rm -f -- "$quarantine"
  fi
}

new_atlas_forwarder_quarantine() {
  file=$1
  mktemp "$(dirname -- "$file")/.atlas-forwarder-retire.XXXXXX"
}

restore_atlas_forwarder_quarantine() {
  quarantine=$1
  destination=$2
  if [ -e "$quarantine" ] || [ -L "$quarantine" ]; then
    if [ ! -e "$destination" ] && [ ! -L "$destination" ]; then
      mv -- "$quarantine" "$destination"
    fi
  fi
}

retire_atlas_forwarder_race() {
  path=$1
  race_path=$2
  content=$3
  [ -n "$race_path" ] && [ "$(canonical_file "$race_path")" = "$(canonical_file "$path")" ] || return 0
  if [ -e "$path" ] || [ -L "$path" ]; then
    return 0
  fi
  printf '%s' "$content" >"$path"
}

remove_atlas_forwarder_state() {
  forwarder=$1
  verified=$2
  state_path=$(atlas_forwarder_state_path "$forwarder") || return 1
  if [ ! -e "$state_path" ] && [ ! -L "$state_path" ]; then
    return 0
  fi
  capability=$(atlas_forwarder_state_capability "$forwarder" "$verified") || {
    printf '%s\n' "ProjectAtlas atlas forwarder installer state changed during retirement: $state_path" >&2
    return 1
  }
  quarantine=$(new_atlas_forwarder_quarantine "$state_path") || return 1
  if ! mv -- "$state_path" "$quarantine"; then
    rm -f -- "$quarantine"
    return 1
  fi
  if ! inject_atlas_forwarder_state_retirement_failure; then
    restore_atlas_forwarder_quarantine "$quarantine" "$state_path"
    return 1
  fi
  expected=$(atlas_forwarder_state_content "$forwarder" "$verified" "$capability") || return 1
  actual=$(cat "$quarantine" 2>/dev/null || true)
  if [ "$actual" != "$expected" ]; then
    restore_atlas_forwarder_quarantine "$quarantine" "$state_path"
    return 1
  fi
  rm -f -- "$quarantine"
}

managed_atlas_forwarder_target() {
  candidate=$1
  is_direct_regular_file "$candidate" || return 1
  marker_target=$(sed -n 's/^# target: //p' "$candidate" | head -n 1)
  [ -n "$marker_target" ] || return 1
  canonical_target=$(canonical_file "$marker_target") || return 1
  [ "$marker_target" = "$canonical_target" ] || return 1
  expected_forwarder=$(atlas_forwarder_path "$canonical_target") || return 1
  [ "$(canonical_file "$candidate")" = "$(canonical_file "$expected_forwarder")" ] || return 1
  is_projectatlas_runtime_contract "$canonical_target" || return 1
  provenance=$(atlas_forwarder_provenance_path "$candidate") || return 1
  is_atlas_forwarder_provenance "$provenance" "$candidate" "$canonical_target" || return 1
  expected_content=$(atlas_forwarder_content "$canonical_target") || return 1
  actual_content=$(cat "$candidate" 2>/dev/null || true)
  [ "$actual_content" = "$expected_content" ] || return 1
  printf '%s\n' "$canonical_target"
}

is_managed_atlas_forwarder() {
  candidate=$1
  verified=$2
  [ -n "$verified" ] || return 1
  managed_target=$(managed_atlas_forwarder_target "$candidate") || return 1
  [ "$managed_target" = "$(canonical_file "$verified")" ]
}

migrate_managed_atlas_forwarder_locked() {
  candidate=$1
  verified=$2
  allow_same_target=${3:-0}
  [ -n "$verified" ] || return 1
  managed_target=$(managed_atlas_forwarder_target "$candidate") || return 1
  [ "$managed_target" != "$(canonical_file "$verified")" ] || [ "$allow_same_target" -eq 1 ] || return 1
  if ! is_managed_atlas_forwarder "$candidate" "$managed_target"; then
    return 1
  fi
  provenance=$(atlas_forwarder_provenance_path "$candidate") || return 1
  forwarder_quarantine=$(new_atlas_forwarder_quarantine "$candidate") || return 1
  provenance_quarantine=$(new_atlas_forwarder_quarantine "$provenance") || {
    rm -f -- "$forwarder_quarantine"
    return 1
  }
  if ! mv -- "$candidate" "$forwarder_quarantine"; then
    rm -f -- "$forwarder_quarantine" "$provenance_quarantine"
    return 1
  fi
  retire_atlas_forwarder_race "$candidate" "${PROJECTATLAS_TEST_ATLAS_FORWARDER_RETIRE_RACE_PATH:-}" "foreign forwarder retirement race"
  if ! mv -- "$provenance" "$provenance_quarantine"; then
    restore_atlas_forwarder_quarantine "$forwarder_quarantine" "$candidate"
    rm -f -- "$provenance_quarantine"
    return 1
  fi
  retire_atlas_forwarder_race "$provenance" "${PROJECTATLAS_TEST_ATLAS_FORWARDER_PROVENANCE_RETIRE_RACE_PATH:-}" "foreign provenance retirement race"
  expected_content=$(atlas_forwarder_content "$managed_target") || return 1
  actual_content=$(cat "$forwarder_quarantine" 2>/dev/null || true)
  expected_provenance=$(atlas_forwarder_provenance_content "$candidate" "$managed_target") || return 1
  actual_provenance=$(cat "$provenance_quarantine" 2>/dev/null || true)
  if { [ "$actual_content" != "$expected_content" ] || [ "$actual_provenance" != "$expected_provenance" ]; } ||
    [ -e "$candidate" ] || [ -L "$candidate" ] || [ -e "$provenance" ] || [ -L "$provenance" ]; then
    restore_atlas_forwarder_quarantine "$provenance_quarantine" "$provenance"
    restore_atlas_forwarder_quarantine "$forwarder_quarantine" "$candidate"
    return 1
  fi
  rm -f -- "$forwarder_quarantine" "$provenance_quarantine"
  remove_atlas_forwarder_state "$candidate" "$managed_target" || return 1
  printf '%s\n' "Migrated ProjectAtlas atlas forwarder: $candidate -> $(atlas_forwarder_path "$(canonical_file "$verified")")"
}

is_owned_atlas_forwarder() {
  managed_atlas_forwarder_target "$1" >/dev/null 2>&1
}

ensure_atlas_forwarder_collision_free() {
  forwarder=$1
  verified=$2
  if [ -e "$forwarder" ] || [ -L "$forwarder" ]; then
    if ! is_managed_atlas_forwarder "$forwarder" "$verified"; then
      printf '%s\n' "ProjectAtlas atlas command collision at intended path; refusing to overwrite unmanaged or differently-owned file: $forwarder" >&2
      return 1
    fi
  fi
  existing=$(command -v atlas 2>/dev/null || true)
  if [ -n "$existing" ] && [ "$(canonical_file "$existing")" != "$(canonical_file "$forwarder")" ]; then
    if ! is_owned_atlas_forwarder "$existing"; then
      printf '%s\n' "ProjectAtlas atlas command collision on effective PATH; refusing to shadow or overwrite: $existing" >&2
      return 1
    fi
  fi
}

inject_atlas_forwarder_publication_race() {
  race_path=${PROJECTATLAS_TEST_ATLAS_FORWARDER_RACE_PATH:-}
  [ -n "$race_path" ] && [ "$(canonical_file "$race_path")" = "$(canonical_file "$1")" ] || return 0
  if [ -e "$1" ] || [ -L "$1" ]; then
    return 0
  fi
  printf '%s\n' '# foreign publication race collision' >"$1"
}

inject_atlas_forwarder_provenance_publication_race() {
  race_path=${PROJECTATLAS_TEST_ATLAS_FORWARDER_PROVENANCE_RACE_PATH:-}
  [ -n "$race_path" ] && [ "$(canonical_file "$race_path")" = "$(canonical_file "$1")" ] || return 0
  if [ -e "$1" ] || [ -L "$1" ]; then
    return 0
  fi
  printf '%s\n' '# foreign provenance publication race collision' >"$1"
}

inject_atlas_forwarder_provenance_check_race() {
  race_path=${PROJECTATLAS_TEST_ATLAS_FORWARDER_PROVENANCE_CHECK_RACE_PATH:-}
  [ -n "$race_path" ] && [ "$(canonical_file "$race_path")" = "$(canonical_file "$1")" ] || return 0
  if [ -e "$1" ] || [ -L "$1" ]; then
    return 0
  fi
  printf '%s\n' '# foreign provenance check race collision' >"$1"
}

inject_atlas_forwarder_state_retirement_failure() {
  [ -n "${PROJECTATLAS_TEST_ATLAS_FORWARDER_STATE_RETIRE_FAILURE:-}" ] || return 0
  return 1
}

pause_atlas_forwarder_after_state_publication() {
  gate=${PROJECTATLAS_TEST_ATLAS_FORWARDER_STATE_PUBLISHED_GATE:-}
  [ -n "$gate" ] || return 0
  printf '%s\n' ready >"$gate.ready"
  while [ -e "$gate" ] || [ -L "$gate" ]; do
    sleep 1
  done
}

pause_atlas_forwarder_after_lock_discovery() {
  gate=${PROJECTATLAS_TEST_ATLAS_FORWARDER_LOCK_DISCOVERY_GATE:-}
  [ -n "$gate" ] || return 0
  printf '%s\n' ready >"$gate.ready"
  while [ -e "$gate" ] || [ -L "$gate" ]; do
    sleep 1
  done
}

pause_atlas_forwarder_after_lock_acquisition() {
  gate=${PROJECTATLAS_TEST_ATLAS_FORWARDER_LOCK_ACQUIRED_GATE:-}
  [ -n "$gate" ] || return 0
  printf '%s\n' ready >"$gate.ready"
  while [ -e "$gate" ] || [ -L "$gate" ]; do
    sleep 1
  done
}

write_atlas_forwarder() {
  verified=$(canonical_file "$1") || return 1
  forwarder=$(atlas_forwarder_path "$verified")
  previous_candidate=$(command -v atlas 2>/dev/null || true)
  previous_candidate_identity=
  previous_atlas_owned_at_discovery=0
  previous_atlas_identity=
  if [ -n "$previous_candidate" ] &&
    [ "$(canonical_file "$previous_candidate")" != "$(canonical_file "$forwarder")" ]; then
    previous_candidate_identity=$(canonical_file "$previous_candidate") || return 1
    if is_owned_atlas_forwarder "$previous_candidate"; then
      previous_atlas_owned_at_discovery=1
      previous_atlas_identity=$previous_candidate_identity
    fi
  else
    previous_candidate=
  fi
  pause_atlas_forwarder_after_lock_discovery
  acquire_atlas_forwarder_lifecycle_lock_set "$forwarder" "$previous_candidate" || return 1
  pause_atlas_forwarder_after_lock_acquisition
  result=0
  previous_atlas=
  current_atlas=$(command -v atlas 2>/dev/null || true)
  if [ -n "$previous_candidate" ]; then
    if [ -n "$current_atlas" ] &&
      [ "$(canonical_file "$current_atlas")" != "$previous_candidate_identity" ]; then
      printf '%s\n' "ProjectAtlas effective atlas command changed while acquiring its lifecycle locks; refusing to publish: $current_atlas" >&2
      result=1
    elif [ -n "$current_atlas" ]; then
      current_atlas_identity=$(canonical_file "$current_atlas") || result=$?
      if [ "$result" -eq 0 ] && is_owned_atlas_forwarder "$current_atlas"; then
        previous_atlas=$current_atlas
        previous_atlas_identity=$current_atlas_identity
      elif [ "$result" -eq 0 ] && [ "$previous_atlas_owned_at_discovery" -eq 1 ]; then
        previous_atlas=$previous_candidate
      fi
    elif [ "$previous_atlas_owned_at_discovery" -eq 1 ]; then
      previous_atlas=$previous_candidate
    fi
  elif [ -n "$current_atlas" ] &&
    [ "$(canonical_file "$current_atlas")" != "$(canonical_file "$forwarder")" ]; then
    printf '%s\n' "ProjectAtlas effective atlas command appeared while acquiring its lifecycle locks; refusing to publish: $current_atlas" >&2
    result=1
  fi
  if [ "$result" -eq 0 ]; then
    write_atlas_forwarder_locked "$verified" "$previous_atlas" "$previous_atlas_identity" || result=$?
  fi
  release_result=0
  release_atlas_forwarder_lifecycle_lock_set || release_result=$?
  if [ "$release_result" -ne 0 ] && [ "$result" -eq 0 ]; then
    result=$release_result
  fi
  return "$result"
}

write_atlas_forwarder_locked() {
  verified=$1
  previous_atlas=${2:-}
  previous_atlas_identity=${3:-}
  forwarder=$(atlas_forwarder_path "$verified")
  provenance=$(atlas_forwarder_provenance_path "$forwarder")
  state_path=$(atlas_forwarder_state_path "$forwarder") || return 1
  if [ -n "$previous_atlas" ] &&
    { [ "$(canonical_file "$previous_atlas")" != "$previous_atlas_identity" ] ||
      ! is_owned_atlas_forwarder "$previous_atlas"; }; then
    printf '%s\n' "ProjectAtlas atlas forwarder migration source changed or is no longer managed: $previous_atlas" >&2
    return 1
  fi
  ensure_atlas_forwarder_collision_free "$forwarder" "$verified" || return 1
  runtime_dir=$(dirname -- "$verified")
  temporary=$(mktemp "$runtime_dir/.atlas-forwarder.XXXXXX") || {
    printf '%s\n' "ProjectAtlas could not stage the atlas forwarder beside the verified runtime: $forwarder" >&2
    return 1
  }
  temporary_provenance=$(mktemp "$runtime_dir/.atlas-forwarder-provenance.XXXXXX") || {
    rm -f "$temporary"
    printf '%s\n' "ProjectAtlas could not stage atlas forwarder provenance beside the verified runtime: $provenance" >&2
    return 1
  }
  if ! atlas_forwarder_content "$verified" >"$temporary" ||
    ! atlas_forwarder_provenance_content "$forwarder" "$verified" >"$temporary_provenance"; then
    rm -f "$temporary"
    rm -f "$temporary_provenance"
    return 1
  fi
  chmod 755 "$temporary" || {
    rm -f "$temporary"
    rm -f "$temporary_provenance"
    return 1
  }
  chmod 600 "$temporary_provenance" || {
    rm -f "$temporary" "$temporary_provenance"
    return 1
  }
  if [ -n "${PROJECTATLAS_TEST_ATLAS_FORWARDER_STAGE_FAILURE:-}" ]; then
    rm -f "$temporary" "$temporary_provenance"
    printf '%s\n' "ProjectAtlas atlas forwarder staging was intentionally failed for lifecycle proof." >&2
    return 1
  fi
  is_direct_regular_file "$temporary" && is_direct_regular_file "$temporary_provenance" || {
    rm -f "$temporary" "$temporary_provenance"
    return 1
  }
  if ! ensure_atlas_forwarder_state "$forwarder" "$verified"; then
    rm -f -- "$temporary" "$temporary_provenance"
    return 1
  fi
  state_published=$ATLAS_FORWARDER_STATE_PUBLISHED
  if [ "$state_published" -eq 1 ]; then
    pause_atlas_forwarder_after_state_publication
  fi
  provenance_published=0
  inject_atlas_forwarder_provenance_check_race "$provenance"
  if [ -e "$provenance" ] || [ -L "$provenance" ]; then
    if ! is_atlas_forwarder_provenance "$provenance" "$forwarder" "$verified"; then
      if [ "$state_published" -eq 1 ] && ! remove_atlas_forwarder_state "$forwarder" "$verified"; then
        printf '%s\n' "ProjectAtlas could not retire newly published atlas forwarder installer state: $state_path" >&2
      fi
      rm -f "$temporary" "$temporary_provenance"
      printf '%s\n' "ProjectAtlas atlas forwarder provenance collision; refusing to overwrite: $provenance" >&2
      return 1
    fi
  else
    if ! inject_atlas_forwarder_provenance_publication_race "$provenance" || ! ln "$temporary_provenance" "$provenance"; then
      if [ "$state_published" -eq 1 ]; then
        if ! remove_atlas_forwarder_state "$forwarder" "$verified"; then
          printf '%s\n' "ProjectAtlas could not retire newly published atlas forwarder installer state: $state_path" >&2
        fi
      fi
      rm -f "$temporary" "$temporary_provenance"
      printf '%s\n' "ProjectAtlas atlas forwarder provenance publication collided; refusing to overwrite: $provenance" >&2
      return 1
    fi
    rm -f "$temporary_provenance"
    provenance_published=1
  fi
  if ! ensure_atlas_forwarder_collision_free "$forwarder" "$verified"; then
    if [ "$provenance_published" -eq 1 ]; then
      if ! remove_published_atlas_forwarder_provenance "$provenance" "$forwarder" "$verified"; then
        printf '%s\n' "ProjectAtlas could not retire newly published atlas forwarder provenance: $provenance" >&2
      fi
    fi
    if [ "$state_published" -eq 1 ]; then
      if ! remove_atlas_forwarder_state "$forwarder" "$verified"; then
        printf '%s\n' "ProjectAtlas could not retire newly published atlas forwarder installer state: $state_path" >&2
      fi
    fi
    rm -f "$temporary"
    return 1
  fi
  if [ ! -e "$forwarder" ] && [ ! -L "$forwarder" ]; then
    if ! inject_atlas_forwarder_publication_race "$forwarder" || ! ln "$temporary" "$forwarder"; then
      if [ "$provenance_published" -eq 1 ]; then
        if ! remove_published_atlas_forwarder_provenance "$provenance" "$forwarder" "$verified"; then
          printf '%s\n' "ProjectAtlas could not retire newly published atlas forwarder provenance: $provenance" >&2
        fi
      fi
      if [ "$state_published" -eq 1 ]; then
        if ! remove_atlas_forwarder_state "$forwarder" "$verified"; then
          printf '%s\n' "ProjectAtlas could not retire newly published atlas forwarder installer state: $state_path" >&2
        fi
      fi
      rm -f "$temporary"
      printf '%s\n' "ProjectAtlas atlas forwarder publication collided; refusing to overwrite: $forwarder" >&2
      return 1
    fi
    rm -f "$temporary"
  else
    rm -f "$temporary"
  fi
  if ! is_managed_atlas_forwarder "$forwarder" "$verified"; then
    if [ "$provenance_published" -eq 1 ]; then
      if ! remove_published_atlas_forwarder_provenance "$provenance" "$forwarder" "$verified"; then
        printf '%s\n' "ProjectAtlas could not retire newly published atlas forwarder provenance: $provenance" >&2
      fi
    fi
    if [ "$state_published" -eq 1 ]; then
      if ! remove_atlas_forwarder_state "$forwarder" "$verified"; then
        printf '%s\n' "ProjectAtlas could not retire newly published atlas forwarder installer state: $state_path" >&2
      fi
    fi
    rm -f -- "$temporary"
    printf '%s\n' "ProjectAtlas atlas forwarder failed final ownership verification: $forwarder" >&2
    return 1
  fi
  if [ -n "$previous_atlas" ]; then
    migrate_managed_atlas_forwarder_locked "$previous_atlas" "$verified" || return 1
    hash -r 2>/dev/null || true
  fi
  printf 'ProjectAtlas atlas forwarder installed and verified: %s -> %s\n' "$forwarder" "$verified"
}

remove_atlas_forwarder() {
  forwarder=$1
  verified=$2
  if [ ! -e "$forwarder" ] && [ ! -L "$forwarder" ]; then
    return 0
  fi
  acquire_atlas_forwarder_lifecycle_lock "$forwarder" || return 1
  result=0
  remove_atlas_forwarder_locked "$forwarder" "$verified" || result=$?
  release_result=0
  release_atlas_forwarder_lifecycle_lock || release_result=$?
  if [ "$release_result" -ne 0 ] && [ "$result" -eq 0 ]; then
    result=$release_result
  fi
  return "$result"
}

remove_atlas_forwarder_locked() {
  forwarder=$1
  verified=$2
  if [ ! -e "$forwarder" ] && [ ! -L "$forwarder" ]; then
    return 0
  fi
  if ! is_managed_atlas_forwarder "$forwarder" "$verified"; then
    printf '%s\n' "ProjectAtlas atlas uninstall refused to remove an unmanaged file: $forwarder" >&2
    return 1
  fi
  migrate_managed_atlas_forwarder_locked "$forwarder" "$verified" 1 || {
    printf '%s\n' "ProjectAtlas atlas uninstall could not retire the owned forwarder safely: $forwarder" >&2
    return 1
  }
  printf 'ProjectAtlas atlas forwarder removed: %s\n' "$forwarder"
}

uninstall_atlas_forwarders() {
  if [ -n "$runtime_override" ]; then
    runtime_dir=$(CDPATH= cd -- "$(dirname -- "$runtime_override")" 2>/dev/null && pwd -P) || {
      printf '%s\n' "ProjectAtlas runtime directory was not found for atlas uninstall: $runtime_override" >&2
      return 1
    }
    forwarder="$runtime_dir/atlas"
    remove_atlas_forwarder "$forwarder" "$runtime_override" || return 1
    return 0
  fi
  for known_runtime in \
    "$HOME/.local/bin/projectatlas" \
    "$HOME/.cargo/bin/projectatlas" \
    "$HOME/.npm/bin/projectatlas" \
    "$HOME/.npm-global/bin/projectatlas" \
    "$HOME/.local/share/npm/bin/projectatlas"; do
    remove_atlas_forwarder "$(dirname -- "$known_runtime")/atlas" "$known_runtime" || return 1
  done
  projectatlas_command=$(command -v projectatlas 2>/dev/null || true)
  if [ -n "$projectatlas_command" ]; then
    remove_atlas_forwarder "$(dirname -- "$projectatlas_command")/atlas" "$projectatlas_command" || return 1
  fi
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
  command -v jq >/dev/null 2>&1 || return 1
  printf '%s\n' "$marketplaces" | jq -r '.marketplaces[]? | select(.name == "projectatlas") | .marketplaceSource.source // empty' | head -n 1
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

codex_state_snapshot_dir=
codex_state_snapshot_config_path=
codex_state_snapshot_config_existed=false
codex_state_snapshot_marketplace_root_path=
codex_state_snapshot_marketplace_root_existed=false
codex_state_snapshot_plugin_cache_path=
codex_state_snapshot_plugin_cache_existed=false
codex_state_snapshot_expected_plugin_cache_path=
codex_state_snapshot_expected_plugin_cache_existed=false
codex_state_snapshot_cache_paths_match=false
codex_state_snapshot_root=
codex_plugin_update_preserved_prior_state=false
codex_plugin_update_lock_path=
codex_plugin_update_lock_root=
codex_plugin_update_lock_identity=

acquire_codex_projectatlas_plugin_update_lock() {
  lock_root=$1
  lock_path=$lock_root/.projectatlas-plugin-update.lock
  if [ ! -e "$lock_path" ] && [ ! -L "$lock_path" ]; then
    (umask 077 && set -C && : > "$lock_path") 2>/dev/null || true
  fi
  lock_parent_resolved=$(CDPATH= cd -P -- "$(dirname -- "$lock_path")" 2>/dev/null && pwd -P) || return 1
  lock_identity=$(stat -c '%d:%i' -- "$lock_path" 2>/dev/null || stat -f '%d:%i' "$lock_path" 2>/dev/null || true)
  lock_links=$(stat -c %h -- "$lock_path" 2>/dev/null || stat -f %l "$lock_path" 2>/dev/null || true)
  lock_owner_uid=$(stat -c %u -- "$lock_path" 2>/dev/null || stat -f %u "$lock_path" 2>/dev/null || true)
  if [ "$lock_parent_resolved" != "$lock_root" ] || [ -L "$lock_path" ] ||
    [ ! -f "$lock_path" ] || [ -z "$lock_identity" ] || [ "$lock_links" != 1 ] ||
    [ "$lock_owner_uid" != "$(id -u)" ]; then
    printf "warning: Codex ProjectAtlas plugin update skipped: update lock at '%s' is not a trusted direct file.\n" "$lock_path" >&2
    return 1
  fi

  exec 9<> "$lock_path" || return 1
  lock_acquired=false
  lock_platform=$(uname -s 2>/dev/null || true)
  lock_device=${lock_identity%%:*}
  lock_inode=${lock_identity#*:}
  if [ "$lock_platform" = Darwin ]; then
    "$projectatlas_bin" acquire-installer-lock "$lock_device" "$lock_inode" <&9 && lock_acquired=true
  elif command -v flock >/dev/null 2>&1; then
    "$projectatlas_bin" acquire-installer-lock "$lock_device" "$lock_inode" <&9 &&
      flock -w 30 9 && lock_acquired=true
  fi
  if [ "$lock_acquired" != true ]; then
    exec 9>&-
    printf '%s\n' "warning: Codex ProjectAtlas plugin update skipped: another installer owns the update lock or no supported lock utility is available." >&2
    return 1
  fi

  current_lock_identity=$(stat -c '%d:%i' -- "$lock_path" 2>/dev/null || stat -f '%d:%i' "$lock_path" 2>/dev/null || true)
  current_lock_links=$(stat -c %h -- "$lock_path" 2>/dev/null || stat -f %l "$lock_path" 2>/dev/null || true)
  if [ -L "$lock_path" ] || [ ! -f "$lock_path" ] || [ "$current_lock_links" != 1 ] ||
    [ "$current_lock_identity" != "$lock_identity" ]; then
    exec 9>&-
    printf "warning: Codex ProjectAtlas plugin update skipped: update lock changed while acquiring '%s'.\n" "$lock_path" >&2
    return 1
  fi
  codex_plugin_update_lock_path=$lock_path
  codex_plugin_update_lock_root=$lock_root
  codex_plugin_update_lock_identity=$lock_identity
}

release_codex_projectatlas_plugin_update_lock() {
  if [ -z "$codex_plugin_update_lock_path" ]; then
    return 0
  fi
  lock_parent=$(dirname -- "$codex_plugin_update_lock_path")
  lock_parent_resolved=$(CDPATH= cd -P -- "$lock_parent" 2>/dev/null && pwd -P) || lock_parent_resolved=
  lock_identity=
  if [ -f "$codex_plugin_update_lock_path" ] && [ ! -L "$codex_plugin_update_lock_path" ]; then
    lock_identity=$(stat -c '%d:%i' -- "$codex_plugin_update_lock_path" 2>/dev/null || stat -f '%d:%i' "$codex_plugin_update_lock_path" 2>/dev/null || true)
  fi
  lock_links=$(stat -c %h -- "$codex_plugin_update_lock_path" 2>/dev/null || stat -f %l "$codex_plugin_update_lock_path" 2>/dev/null || true)
  lock_release_valid=true
  if [ "$lock_parent_resolved" != "$codex_plugin_update_lock_root" ] ||
    [ -L "$codex_plugin_update_lock_path" ] || [ ! -f "$codex_plugin_update_lock_path" ] ||
    [ "$lock_identity" != "$codex_plugin_update_lock_identity" ] || [ "$lock_links" != 1 ]; then
    lock_release_valid=false
  fi
  exec 9>&-
  codex_plugin_update_lock_path=
  codex_plugin_update_lock_root=
  codex_plugin_update_lock_identity=
  if [ "$lock_release_valid" != true ]; then
    printf "warning: Codex ProjectAtlas plugin update lock cleanup refused/path changed at '%s'.\n" "$lock_parent/.projectatlas-plugin-update.lock" >&2
    return 1
  fi
}

clear_codex_projectatlas_snapshot() {
  if [ -n "$codex_state_snapshot_dir" ] && [ -e "$codex_state_snapshot_dir" ]; then
    snapshot_cleanup_path=$codex_state_snapshot_dir
    snapshot_cleanup_parent=$(dirname -- "$snapshot_cleanup_path")
    snapshot_cleanup_parent_resolved=$(CDPATH= cd -P -- "$snapshot_cleanup_parent" 2>/dev/null && pwd -P) || {
      printf "warning: Codex ProjectAtlas state snapshot cleanup refused/path changed at '%s': parent path cannot be validated.\n" "$snapshot_cleanup_path" >&2
      return 1
    }
    if [ -L "$snapshot_cleanup_path" ] || [ ! -d "$snapshot_cleanup_path" ] ||
      [ "$snapshot_cleanup_parent_resolved" != "$codex_state_snapshot_root" ] ||
      [ ! -w "$snapshot_cleanup_parent_resolved" ] || [ ! -x "$snapshot_cleanup_parent_resolved" ]; then
      printf "warning: Codex ProjectAtlas state snapshot cleanup refused/path changed at '%s': path is not a direct removable snapshot.\n" "$snapshot_cleanup_path" >&2
      return 1
    fi
    if ! remove_codex_tree \
      "$snapshot_cleanup_path" "Codex recovery snapshot" "$codex_state_snapshot_root"; then
      printf "warning: Codex ProjectAtlas state snapshot cleanup failed; retained at '%s': removal did not complete.\n" "$snapshot_cleanup_path" >&2
      return 1
    fi
  fi
  codex_state_snapshot_dir=
  codex_state_snapshot_config_path=
  codex_state_snapshot_config_existed=false
  codex_state_snapshot_marketplace_root_path=
  codex_state_snapshot_marketplace_root_existed=false
  codex_state_snapshot_plugin_cache_path=
  codex_state_snapshot_plugin_cache_existed=false
  codex_state_snapshot_expected_plugin_cache_path=
  codex_state_snapshot_expected_plugin_cache_existed=false
  codex_state_snapshot_cache_paths_match=false
  codex_state_snapshot_root=
  return 0
}

validate_codex_projectatlas_snapshot_file() {
  snapshot_file_path=$1
  snapshot_file_description=$2
  snapshot_file_root=$3
  case "$snapshot_file_path/" in
    "$snapshot_file_root"/*) ;;
    *)
      printf 'warning: Codex ProjectAtlas plugin update skipped: %s is outside the Codex state root.\n' "$snapshot_file_description" >&2
      return 1
      ;;
  esac
  snapshot_file_parent=$(dirname -- "$snapshot_file_path")
  snapshot_file_parent_resolved=$(CDPATH= cd -P -- "$snapshot_file_parent" 2>/dev/null && pwd -P) || {
    printf 'warning: Codex ProjectAtlas plugin update skipped: %s ancestry cannot be validated.\n' "$snapshot_file_description" >&2
    return 1
  }
  case "$snapshot_file_parent_resolved/" in
    "$snapshot_file_root"/*) ;;
    *)
      printf 'warning: Codex ProjectAtlas plugin update skipped: %s ancestry leaves the Codex state root.\n' "$snapshot_file_description" >&2
      return 1
      ;;
  esac
  if [ ! -f "$snapshot_file_path" ] || [ -L "$snapshot_file_path" ]; then
    printf 'warning: Codex ProjectAtlas plugin update skipped: %s is not a direct file.\n' "$snapshot_file_description" >&2
    return 1
  fi
  snapshot_file_links=$(stat -c %h -- "$snapshot_file_path" 2>/dev/null || stat -f %l "$snapshot_file_path" 2>/dev/null) || {
    printf 'warning: Codex ProjectAtlas plugin update skipped: %s link count cannot be validated.\n' "$snapshot_file_description" >&2
    return 1
  }
  if [ "$snapshot_file_links" -ne 1 ]; then
    printf 'warning: Codex ProjectAtlas plugin update skipped: %s is hard linked.\n' "$snapshot_file_description" >&2
    return 1
  fi
}

validate_codex_projectatlas_destination_ancestry() {
  destination_path=$1
  destination_description=$2
  destination_root=$3
  destination_ancestor=$(dirname -- "$destination_path")
  while [ ! -d "$destination_ancestor" ]; do
    if [ -L "$destination_ancestor" ] || [ -e "$destination_ancestor" ]; then
      printf 'warning: Codex ProjectAtlas plugin update skipped: %s has a non-directory ancestor.\n' "$destination_description" >&2
      return 1
    fi
    destination_parent=$(dirname -- "$destination_ancestor")
    if [ "$destination_parent" = "$destination_ancestor" ]; then
      return 1
    fi
    destination_ancestor=$destination_parent
  done
  destination_ancestor_resolved=$(CDPATH= cd -P -- "$destination_ancestor" 2>/dev/null && pwd -P) || return 1
  case "$destination_ancestor_resolved/" in
    "$destination_root"|"$destination_root"/*) ;;
    *)
      printf 'warning: Codex ProjectAtlas plugin update skipped: %s ancestry leaves the Codex state root.\n' "$destination_description" >&2
      return 1
      ;;
  esac
  validate_codex_mounts "$destination_path" "$destination_description" "$destination_root"
}

validate_codex_mounts() {
  mount_candidate=$1
  mount_description=$2
  mount_root=$3
  mount_platform=$(uname -s 2>/dev/null || true)
  case "$mount_platform" in
    Linux)
      if ! command -v findmnt >/dev/null 2>&1 || ! command -v jq >/dev/null 2>&1; then
        printf 'warning: Codex ProjectAtlas plugin update skipped: %s mount ownership cannot be established.\n' "$mount_description" >&2
        return 1
      fi
      mount_inventory=$(findmnt --json --list --output TARGET 2>/dev/null) || {
        printf 'warning: Codex ProjectAtlas plugin update skipped: %s mount inventory cannot be read.\n' "$mount_description" >&2
        return 1
      }
      mount_targets=$(printf '%s\n' "$mount_inventory" | jq -c '
          (if type != "object" or ((.filesystems? | type) != "array") then
             error("invalid findmnt inventory")
           elif (.filesystems | length) == 0
             or any(.filesystems[];
               type != "object"
                 or ((.target? | type) != "string")
                 or (.target | startswith("/") | not)) then
             error("invalid findmnt filesystem row")
           else
             [.filesystems[].target]
           end)
        ' 2>/dev/null) || {
        printf 'warning: Codex ProjectAtlas plugin update skipped: %s mount inventory is malformed.\n' "$mount_description" >&2
        return 1
      }
      if ! printf '%s\n' "$mount_targets" | jq -e \
        --arg root "$mount_root" --arg candidate "$mount_candidate" '
          . as $targets
          | any($targets[];
              . as $target
              | $target == "/"
                or $target == $root
                or ($root | startswith($target + "/")))
            and all($targets[];
              . as $target
              | (((($target | startswith($root + "/"))
                    and (($target == $candidate)
                      or ($candidate | startswith($target + "/"))
                      or ($target | startswith($candidate + "/"))))) | not))
        ' >/dev/null 2>&1; then
        printf 'warning: Codex ProjectAtlas plugin update skipped: %s crosses or contains a mounted filesystem.\n' "$mount_description" >&2
        return 1
      fi
      ;;
    Darwin)
      if ! command -v stat >/dev/null 2>&1 || ! command -v find >/dev/null 2>&1; then
        printf 'warning: Codex ProjectAtlas plugin update skipped: %s mount ownership cannot be established.\n' "$mount_description" >&2
        return 1
      fi
      mount_root_identity=$(stat -f %d "$mount_root" 2>/dev/null) || {
        printf 'warning: Codex ProjectAtlas plugin update skipped: %s mount inventory cannot be read.\n' "$mount_description" >&2
        return 1
      }
      mount_probe=$mount_candidate
      while [ ! -e "$mount_probe" ] && [ ! -L "$mount_probe" ]; do
        mount_parent=$(dirname -- "$mount_probe")
        if [ "$mount_parent" = "$mount_probe" ]; then
          return 1
        fi
        mount_probe=$mount_parent
      done
      mount_probe_identity=$(stat -f %d "$mount_probe" 2>/dev/null) || {
        printf 'warning: Codex ProjectAtlas plugin update skipped: %s mount inventory cannot be read.\n' "$mount_description" >&2
        return 1
      }
      if [ "$mount_probe_identity" != "$mount_root_identity" ]; then
        printf 'warning: Codex ProjectAtlas plugin update skipped: %s crosses a mounted filesystem.\n' "$mount_description" >&2
        return 1
      fi
      if [ -d "$mount_candidate" ]; then
        invalid_mount=$(find -x "$mount_candidate" -type d ! -exec sh -c '
          [ "$(stat -f %d "$2" 2>/dev/null)" = "$1" ]
        ' sh "$mount_root_identity" {} \; -print -quit) || {
          printf 'warning: Codex ProjectAtlas plugin update skipped: %s mount inventory cannot be read.\n' "$mount_description" >&2
          return 1
        }
        if [ -n "$invalid_mount" ]; then
          invalid_mount_identity=$(stat -f %d "$invalid_mount" 2>/dev/null) || {
            printf 'warning: Codex ProjectAtlas plugin update skipped: %s mount inventory cannot be read.\n' "$mount_description" >&2
            return 1
          }
          if [ "$invalid_mount_identity" != "$mount_root_identity" ]; then
            printf 'warning: Codex ProjectAtlas plugin update skipped: %s contains a mounted filesystem.\n' "$mount_description" >&2
          else
            printf 'warning: Codex ProjectAtlas plugin update skipped: %s mount inventory changed while it was read.\n' "$mount_description" >&2
          fi
          return 1
        fi
      fi
      ;;
    *)
      printf 'warning: Codex ProjectAtlas plugin update skipped: %s mount ownership is unsupported on this host.\n' "$mount_description" >&2
      return 1
      ;;
  esac
}

validate_codex_projectatlas_snapshot_directory() {
  snapshot_directory_path=$1
  snapshot_directory_description=$2
  snapshot_directory_root=$3
  if [ ! -d "$snapshot_directory_path" ] || [ -L "$snapshot_directory_path" ]; then
    printf 'warning: Codex ProjectAtlas plugin update skipped: %s is not a direct directory.\n' "$snapshot_directory_description" >&2
    return 1
  fi
  snapshot_directory_resolved=$(CDPATH= cd -P -- "$snapshot_directory_path" 2>/dev/null && pwd -P) || {
    printf 'warning: Codex ProjectAtlas plugin update skipped: %s cannot be validated.\n' "$snapshot_directory_description" >&2
    return 1
  }
  case "$snapshot_directory_resolved/" in
    "$snapshot_directory_root"/*) ;;
    *)
      printf 'warning: Codex ProjectAtlas plugin update skipped: %s is outside the Codex state root.\n' "$snapshot_directory_description" >&2
      return 1
      ;;
  esac
  validate_codex_mounts \
    "$snapshot_directory_resolved" "$snapshot_directory_description" "$snapshot_directory_root" || return 1
  snapshot_directory_symlink=$(find "$snapshot_directory_resolved" -type l -print -quit) || return 1
  if [ -n "$snapshot_directory_symlink" ]; then
    printf 'warning: Codex ProjectAtlas plugin update skipped: %s contains a symbolic link.\n' "$snapshot_directory_description" >&2
    return 1
  fi
  snapshot_directory_hard_link=$(find "$snapshot_directory_resolved" -type f -links +1 -print -quit) || return 1
  if [ -n "$snapshot_directory_hard_link" ]; then
    printf 'warning: Codex ProjectAtlas plugin update skipped: %s contains a hard-linked file.\n' "$snapshot_directory_description" >&2
    return 1
  fi
}

remove_codex_tree() {
  remove_tree_path=$1
  remove_tree_description=$2
  remove_tree_root=$3
  validate_codex_projectatlas_snapshot_directory \
    "$remove_tree_path" "$remove_tree_description" "$remove_tree_root" || return 1
  if ! find "$remove_tree_path" -type d -exec sh -c '
    for remove_tree_directory do
      [ -w "$remove_tree_directory" ] && [ -x "$remove_tree_directory" ] || exit 1
    done
  ' sh {} +; then
    printf 'warning: Codex ProjectAtlas plugin update skipped: %s tree is not removable.\n' "$remove_tree_description" >&2
    return 1
  fi
  validate_codex_mounts \
    "$remove_tree_path" "$remove_tree_description" "$remove_tree_root" || return 1
  remove_tree_platform=$(uname -s 2>/dev/null || true)
  case "$remove_tree_platform" in
    Linux) find "$remove_tree_path" -xdev -depth -delete || return 1 ;;
    Darwin) find -x "$remove_tree_path" -depth -delete || return 1 ;;
    *) return 1 ;;
  esac
  [ ! -e "$remove_tree_path" ] && [ ! -L "$remove_tree_path" ]
}

stage_codex_projectatlas_snapshot() {
  installed_version=${1:-}
  reported_plugin_source_path=${2:-}
  expected_version=${3:-}
  config_path=$(codex_config_path)
  if [ -z "$config_path" ] || [ -L "$config_path" ] || { [ -e "$config_path" ] && [ ! -f "$config_path" ]; }; then
    printf '%s\n' "warning: Codex ProjectAtlas plugin update skipped: Codex config path is not safely restorable." >&2
    return 1
  fi
  config_parent=$(dirname -- "$config_path")
  codex_root=$(CDPATH= cd -P -- "$config_parent" 2>/dev/null && pwd -P) || {
    printf '%s\n' "warning: Codex ProjectAtlas plugin update skipped: existing Codex config root cannot be validated." >&2
    return 1
  }
  config_path=$codex_root/${config_path##*/}
  config_existed=false
  if [ -f "$config_path" ]; then
    config_existed=true
    config_links=$(stat -c %h -- "$config_path" 2>/dev/null || stat -f %l "$config_path" 2>/dev/null) || {
      printf '%s\n' "warning: Codex ProjectAtlas plugin update skipped: existing Codex config link count cannot be validated." >&2
      return 1
    }
    if [ "$config_links" -ne 1 ]; then
      printf '%s\n' "warning: Codex ProjectAtlas plugin update skipped: existing Codex config is hard linked." >&2
      return 1
    fi
  fi

  case "$expected_version" in
    ''|*[!0-9A-Za-z.+-]*)
      printf '%s\n' "warning: Codex ProjectAtlas plugin update skipped: expected plugin version cannot identify its Codex cache safely." >&2
      return 1
      ;;
  esac
  expected_marketplace_root=$codex_root/.tmp/marketplaces/projectatlas
  validate_codex_projectatlas_destination_ancestry "$expected_marketplace_root" "ProjectAtlas marketplace root" "$codex_root" || return 1
  marketplace_root_existed=false
  if [ -e "$expected_marketplace_root" ] || [ -L "$expected_marketplace_root" ]; then
    validate_codex_projectatlas_snapshot_directory "$expected_marketplace_root" "ProjectAtlas marketplace root" "$codex_root" || return 1
    expected_marketplace_root=$(CDPATH= cd -P -- "$expected_marketplace_root" 2>/dev/null && pwd -P) || return 1
    marketplace_root_existed=true
  fi
  expected_plugin_source_root=$expected_marketplace_root/plugins/projectatlas
  plugin_source_root=$expected_plugin_source_root
  validate_codex_projectatlas_destination_ancestry "$plugin_source_root" "installed projectatlas plugin source" "$codex_root" || return 1
  plugin_source_existed=false
  if [ -e "$plugin_source_root" ] || [ -L "$plugin_source_root" ]; then
    validate_codex_projectatlas_snapshot_directory "$plugin_source_root" "installed projectatlas plugin source" "$codex_root" || return 1
    plugin_source_root=$(CDPATH= cd -P -- "$plugin_source_root" 2>/dev/null && pwd -P) || return 1
    plugin_source_existed=true
  fi
  plugin_cache_root=
  plugin_cache_existed=false
  if [ -n "$installed_version" ]; then
    if [ -z "$reported_plugin_source_path" ] || [ ! -d "$reported_plugin_source_path" ] || [ -L "$reported_plugin_source_path" ]; then
      printf '%s\n' "warning: Codex ProjectAtlas plugin update skipped: installed projectatlas plugin source cannot be preserved safely." >&2
      return 1
    fi
    reported_plugin_source_root=$(CDPATH= cd -P -- "$reported_plugin_source_path" 2>/dev/null && pwd -P) || {
      printf '%s\n' "warning: Codex ProjectAtlas plugin update skipped: installed projectatlas plugin source cannot be validated." >&2
      return 1
    }
    if [ "$reported_plugin_source_root" != "$expected_plugin_source_root" ] || [ "$plugin_source_existed" != true ]; then
      printf '%s\n' "warning: Codex ProjectAtlas plugin update skipped: installed plugin source does not use the expected Codex marketplace layout." >&2
      return 1
    fi
    case "$installed_version" in
      ''|*[!0-9A-Za-z.+-]*)
        printf '%s\n' "warning: Codex ProjectAtlas plugin update skipped: installed plugin version cannot identify its Codex cache safely." >&2
        return 1
        ;;
    esac
    plugin_cache_path=$codex_root/plugins/cache/projectatlas/projectatlas/$installed_version
    validate_codex_projectatlas_snapshot_directory "$plugin_source_root" "installed projectatlas plugin source" "$codex_root" || return 1
    validate_codex_projectatlas_snapshot_directory "$plugin_cache_path" "installed projectatlas plugin cache" "$codex_root" || return 1
    plugin_cache_root=$(CDPATH= cd -P -- "$plugin_cache_path" 2>/dev/null && pwd -P) || return 1
    plugin_cache_existed=true
    if [ "$plugin_cache_root" != "$plugin_cache_path" ]; then
      printf '%s\n' "warning: Codex ProjectAtlas plugin update skipped: installed plugin cache does not use the expected Codex cache layout." >&2
      return 1
    fi
  fi
  expected_plugin_cache_path=$codex_root/plugins/cache/projectatlas/projectatlas/$expected_version
  validate_codex_projectatlas_destination_ancestry "$expected_plugin_cache_path" "expected projectatlas plugin cache" "$codex_root" || return 1
  expected_plugin_cache_existed=false
  cache_paths_match=false
  if [ -n "$plugin_cache_root" ] && [ "$plugin_cache_root" = "$expected_plugin_cache_path" ]; then
    cache_paths_match=true
    expected_plugin_cache_existed=true
  elif [ -e "$expected_plugin_cache_path" ] || [ -L "$expected_plugin_cache_path" ]; then
    validate_codex_projectatlas_snapshot_directory "$expected_plugin_cache_path" "expected projectatlas plugin cache" "$codex_root" || return 1
    expected_plugin_cache_path=$(CDPATH= cd -P -- "$expected_plugin_cache_path" 2>/dev/null && pwd -P) || return 1
    expected_plugin_cache_existed=true
  fi
  marketplace_manifest_path=$expected_marketplace_root/.agents/plugins/marketplace.json
  marketplace_install_record_path=$expected_marketplace_root/.codex-marketplace-install.json
  validate_codex_projectatlas_destination_ancestry "$marketplace_manifest_path" "ProjectAtlas marketplace manifest" "$codex_root" || return 1
  validate_codex_projectatlas_destination_ancestry "$marketplace_install_record_path" "ProjectAtlas marketplace install record" "$codex_root" || return 1
  marketplace_manifest_existed=false
  marketplace_install_record_existed=false
  if [ -e "$marketplace_manifest_path" ] || [ -L "$marketplace_manifest_path" ]; then
    validate_codex_projectatlas_snapshot_file "$marketplace_manifest_path" "ProjectAtlas marketplace manifest" "$codex_root" || return 1
    marketplace_manifest_existed=true
  elif [ -n "$installed_version" ]; then
    printf '%s\n' "warning: Codex ProjectAtlas plugin update skipped: ProjectAtlas marketplace manifest is missing." >&2
    return 1
  fi
  if [ -e "$marketplace_install_record_path" ] || [ -L "$marketplace_install_record_path" ]; then
    validate_codex_projectatlas_snapshot_file "$marketplace_install_record_path" "ProjectAtlas marketplace install record" "$codex_root" || return 1
    marketplace_install_record_existed=true
  elif [ -n "$installed_version" ]; then
    printf '%s\n' "warning: Codex ProjectAtlas plugin update skipped: ProjectAtlas marketplace install record is missing." >&2
    return 1
  fi

  snapshot_dir=$(mktemp -d "$codex_root/.projectatlas-plugin-state.XXXXXX") || return 1
  chmod 700 "$snapshot_dir" || {
    if ! remove_codex_tree "$snapshot_dir" "Codex recovery snapshot" "$codex_root"; then
      printf "warning: Codex ProjectAtlas state snapshot cleanup failed; retained at '%s'.\n" "$snapshot_dir" >&2
    fi
    return 1
  }
  if [ "$config_existed" = true ] && ! cp -p -- "$config_path" "$snapshot_dir/config.toml"; then
    if ! remove_codex_tree "$snapshot_dir" "Codex recovery snapshot" "$codex_root"; then
      printf "warning: Codex ProjectAtlas state snapshot cleanup failed; retained at '%s'.\n" "$snapshot_dir" >&2
    fi
    return 1
  fi
  if [ "$marketplace_root_existed" = true ] && ! cp -pPR -- "$expected_marketplace_root" "$snapshot_dir/marketplace-root"; then
    if ! remove_codex_tree "$snapshot_dir" "Codex recovery snapshot" "$codex_root"; then
      printf "warning: Codex ProjectAtlas state snapshot cleanup failed; retained at '%s'.\n" "$snapshot_dir" >&2
    fi
    return 1
  fi
  if [ "$plugin_cache_existed" = true ] && ! cp -pPR -- "$plugin_cache_root" "$snapshot_dir/plugin-cache"; then
    if ! remove_codex_tree "$snapshot_dir" "Codex recovery snapshot" "$codex_root"; then
      printf "warning: Codex ProjectAtlas state snapshot cleanup failed; retained at '%s'.\n" "$snapshot_dir" >&2
    fi
    return 1
  fi
  if [ "$expected_plugin_cache_existed" = true ] && [ "$cache_paths_match" != true ] &&
    ! cp -pPR -- "$expected_plugin_cache_path" "$snapshot_dir/expected-plugin-cache"; then
    if ! remove_codex_tree "$snapshot_dir" "Codex recovery snapshot" "$codex_root"; then
      printf "warning: Codex ProjectAtlas state snapshot cleanup failed; retained at '%s'.\n" "$snapshot_dir" >&2
    fi
    return 1
  fi
  codex_state_snapshot_dir=$snapshot_dir
  codex_state_snapshot_config_path=$config_path
  codex_state_snapshot_config_existed=$config_existed
  codex_state_snapshot_marketplace_root_path=$expected_marketplace_root
  codex_state_snapshot_marketplace_root_existed=$marketplace_root_existed
  codex_state_snapshot_plugin_cache_path=$plugin_cache_root
  codex_state_snapshot_plugin_cache_existed=$plugin_cache_existed
  codex_state_snapshot_expected_plugin_cache_path=$expected_plugin_cache_path
  codex_state_snapshot_expected_plugin_cache_existed=$expected_plugin_cache_existed
  codex_state_snapshot_cache_paths_match=$cache_paths_match
  codex_state_snapshot_root=$codex_root
}

restore_codex_projectatlas_snapshot_directory() {
  restore_directory_snapshot=$1
  restore_directory_destination=$2
  restore_directory_existed=$3
  if [ "$restore_directory_existed" = true ]; then
    validate_codex_projectatlas_snapshot_directory \
      "$restore_directory_snapshot" "Codex snapshot restore source" "$codex_state_snapshot_dir" || return 1
  fi
  case "$restore_directory_destination/" in
    "$codex_state_snapshot_root"/*) ;;
    *) return 1 ;;
  esac
  validate_codex_projectatlas_destination_ancestry \
    "$restore_directory_destination" "Codex snapshot restore destination" "$codex_state_snapshot_root" || return 1
  restore_directory_parent=$(dirname -- "$restore_directory_destination")
  mkdir -p -- "$restore_directory_parent" || return 1
  restore_directory_parent_resolved=$(CDPATH= cd -P -- "$restore_directory_parent" 2>/dev/null && pwd -P) || return 1
  case "$restore_directory_parent_resolved/" in
    "$codex_state_snapshot_root"/*) ;;
    *) return 1 ;;
  esac
  if [ -L "$restore_directory_destination" ] || { [ -e "$restore_directory_destination" ] && [ ! -d "$restore_directory_destination" ]; }; then
    return 1
  fi
  if [ -d "$restore_directory_destination" ]; then
    remove_codex_tree \
      "$restore_directory_destination" "Codex snapshot restore destination" "$codex_state_snapshot_root" || return 1
  fi
  if [ -e "$restore_directory_destination" ] || [ -L "$restore_directory_destination" ]; then
    return 1
  fi
  if [ "$restore_directory_existed" = true ]; then
    validate_codex_projectatlas_snapshot_directory \
      "$restore_directory_snapshot" "Codex snapshot restore source" "$codex_state_snapshot_dir" || return 1
    cp -pPR -- "$restore_directory_snapshot" "$restore_directory_destination" || return 1
  fi
}

restore_codex_projectatlas_snapshot_file() {
  restore_file_snapshot=$1
  restore_file_destination=$2
  restore_file_existed=$3
  if [ "$restore_file_existed" = true ]; then
    validate_codex_projectatlas_snapshot_file \
      "$restore_file_snapshot" "Codex snapshot restore source" "$codex_state_snapshot_dir" || return 1
  fi
  case "$restore_file_destination/" in
    "$codex_state_snapshot_root"/*) ;;
    *) return 1 ;;
  esac
  validate_codex_projectatlas_destination_ancestry \
    "$restore_file_destination" "Codex snapshot restore destination" "$codex_state_snapshot_root" || return 1
  restore_file_parent=$(dirname -- "$restore_file_destination")
  mkdir -p -- "$restore_file_parent" || return 1
  restore_file_parent_resolved=$(CDPATH= cd -P -- "$restore_file_parent" 2>/dev/null && pwd -P) || return 1
  case "$restore_file_parent_resolved/" in
    "$codex_state_snapshot_root"/*) ;;
    *) return 1 ;;
  esac
  if [ -L "$restore_file_destination" ] || { [ -e "$restore_file_destination" ] && [ ! -f "$restore_file_destination" ]; }; then
    return 1
  fi
  if [ "$restore_file_existed" != true ]; then
    rm -f -- "$restore_file_destination" || return 1
    if [ -e "$restore_file_destination" ] || [ -L "$restore_file_destination" ]; then
      return 1
    fi
    return 0
  fi
  restore_file_temporary=$restore_file_destination.projectatlas-restore.$$
  rm -f -- "$restore_file_temporary" || return 1
  cp -p -- "$restore_file_snapshot" "$restore_file_temporary" || return 1
  mv -f -- "$restore_file_temporary" "$restore_file_destination"
}

restore_codex_projectatlas_snapshot() {
  if [ -z "$codex_state_snapshot_dir" ]; then
    return 1
  fi
  validate_codex_projectatlas_snapshot_directory \
    "$codex_state_snapshot_dir" "Codex recovery snapshot" "$codex_state_snapshot_root" || return 1
  current_root=$(CDPATH= cd -P -- "$(dirname -- "$codex_state_snapshot_config_path")" 2>/dev/null && pwd -P) || return 1
  if [ "$current_root" != "$codex_state_snapshot_root" ]; then
    return 1
  fi
  restore_codex_projectatlas_snapshot_directory \
    "$codex_state_snapshot_dir/marketplace-root" \
    "$codex_state_snapshot_marketplace_root_path" \
    "$codex_state_snapshot_marketplace_root_existed" || return 1
  if [ -n "$codex_state_snapshot_plugin_cache_path" ]; then
    restore_codex_projectatlas_snapshot_directory \
      "$codex_state_snapshot_dir/plugin-cache" \
      "$codex_state_snapshot_plugin_cache_path" \
      "$codex_state_snapshot_plugin_cache_existed" || return 1
  fi
  if [ "$codex_state_snapshot_cache_paths_match" != true ]; then
    restore_codex_projectatlas_snapshot_directory \
      "$codex_state_snapshot_dir/expected-plugin-cache" \
      "$codex_state_snapshot_expected_plugin_cache_path" \
      "$codex_state_snapshot_expected_plugin_cache_existed" || return 1
  fi
  restore_codex_projectatlas_snapshot_file \
    "$codex_state_snapshot_dir/config.toml" \
    "$codex_state_snapshot_config_path" \
    "$codex_state_snapshot_config_existed"
}

codex_projectatlas_inventory_complete=false
codex_projectatlas_inventory_version=
codex_projectatlas_inventory_source_path=

load_codex_projectatlas_plugin_inventory() {
  codex_projectatlas_inventory_complete=false
  codex_projectatlas_inventory_version=
  codex_projectatlas_inventory_source_path=
  plugins=$("$codex_bin" plugin list --marketplace projectatlas --json 2>/dev/null) || return 1
  command -v jq >/dev/null 2>&1 || return 1
  if ! printf '%s\n' "$plugins" | jq -e '
      type == "object" and
      (.installed | type == "array") and
      all(.installed[]; type == "object")
    ' >/dev/null 2>&1; then
      return 1
    fi
    projectatlas_inventory_count=$(printf '%s\n' "$plugins" | jq -r '[
      .installed[] |
      select(.pluginId == "projectatlas@projectatlas" or
        (.name == "projectatlas" and .marketplaceName == "projectatlas"))
    ] | length') || return 1
    if [ "$projectatlas_inventory_count" -eq 0 ]; then
      codex_projectatlas_inventory_complete=true
      return 0
    fi
    if [ "$projectatlas_inventory_count" -ne 1 ] ||
      ! printf '%s\n' "$plugins" | jq -e '
        [
          .installed[] |
          select(.pluginId == "projectatlas@projectatlas" or
            (.name == "projectatlas" and .marketplaceName == "projectatlas"))
        ][0] |
        .pluginId == "projectatlas@projectatlas" and
        .name == "projectatlas" and
        .marketplaceName == "projectatlas" and
        (.version | type == "string") and
        .installed == true and
        .enabled == true and
        (.marketplaceSource | type == "object") and
        (.marketplaceSource.source | type == "string" and length > 0) and
        (.source | type == "object") and
        (.source.path | type == "string" and length > 0)
      ' >/dev/null 2>&1; then
      return 1
    fi
    codex_projectatlas_inventory_version=$(printf '%s\n' "$plugins" | jq -r '[
      .installed[] |
      select(.pluginId == "projectatlas@projectatlas" or
        (.name == "projectatlas" and .marketplaceName == "projectatlas"))
    ][0].version') || return 1
    codex_projectatlas_inventory_source_path=$(printf '%s\n' "$plugins" | jq -r '[
      .installed[] |
      select(.pluginId == "projectatlas@projectatlas" or
        (.name == "projectatlas" and .marketplaceName == "projectatlas"))
    ][0].source.path') || return 1
    codex_projectatlas_inventory_marketplace_source=$(printf '%s\n' "$plugins" | jq -r '[
      .installed[] |
      select(.pluginId == "projectatlas@projectatlas" or
        (.name == "projectatlas" and .marketplaceName == "projectatlas"))
    ][0].marketplaceSource.source') || return 1
  official_projectatlas_marketplace_source "$codex_projectatlas_inventory_marketplace_source" || return 1
  case "$codex_projectatlas_inventory_version" in
    [0-9A-Za-z]*) ;;
    *) return 1 ;;
  esac
  case "$codex_projectatlas_inventory_version" in
    *[!0-9A-Za-z.+-]*) return 1 ;;
  esac
  case "$codex_projectatlas_inventory_source_path" in
    /*) ;;
    *) return 1 ;;
  esac
  codex_projectatlas_inventory_complete=true
}

codex_projectatlas_plugin_version() {
  load_codex_projectatlas_plugin_inventory || return 0
  printf '%s\n' "$codex_projectatlas_inventory_version"
}

codex_projectatlas_plugin_source_path() {
  load_codex_projectatlas_plugin_inventory || return 0
  printf '%s\n' "$codex_projectatlas_inventory_source_path"
}

codex_projectatlas_plugin_source_manifest_version() {
  if [ "$#" -gt 0 ]; then
    plugin_source_path=$1
  else
    plugin_source_path=$(codex_projectatlas_plugin_source_path)
  fi
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
  if [ "$#" -gt 1 ]; then
    plugin_source_path=$2
  else
    plugin_source_path=$(codex_projectatlas_plugin_source_path)
  fi
  [ -n "$plugin_source_path" ] || return 1
  [ "$(codex_projectatlas_plugin_source_manifest_version "$plugin_source_path")" = "$expected_version" ]
}

codex_projectatlas_plugin_source_ready() {
  expected_version=$1
  plugin_source_path=$2
  codex_projectatlas_plugin_source_manifest_matches "$expected_version" "$plugin_source_path" &&
    cmp -s "$plugin_root/skills/projectatlas/SKILL.md" "$plugin_source_path/skills/projectatlas/SKILL.md"
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
  config_path=$(codex_config_path)
  if [ -z "$config_path" ]; then
    codex_plugin_update_preserved_prior_state=true
    printf '%s\n' "warning: Codex ProjectAtlas plugin update skipped: Codex config path cannot be resolved for update locking." >&2
    return 0
  fi
  config_parent=$(dirname -- "$config_path")
  codex_root=$(CDPATH= cd -P -- "$config_parent" 2>/dev/null && pwd -P) || {
    codex_plugin_update_preserved_prior_state=true
    printf '%s\n' "warning: Codex ProjectAtlas plugin update skipped: Codex config root cannot be validated for update locking." >&2
    return 0
  }
  acquire_codex_projectatlas_plugin_update_lock "$codex_root" || {
    codex_plugin_update_preserved_prior_state=true
    return 0
  }
  for retained_snapshot in "$codex_root"/.projectatlas-plugin-state.*; do
    if [ -e "$retained_snapshot" ] || [ -L "$retained_snapshot" ]; then
      codex_plugin_update_preserved_prior_state=true
      printf "warning: Codex ProjectAtlas plugin update skipped: retained recovery state requires inspection at '%s'.\n" "$retained_snapshot" >&2
      release_codex_projectatlas_plugin_update_lock || true
      return 0
    fi
  done
  update_result=0
  update_codex_plugin_locked || update_result=$?
  release_codex_projectatlas_plugin_update_lock || true
  return "$update_result"
}

update_codex_plugin_locked() {
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
  marketplace_source=$(codex_projectatlas_marketplace_source "$marketplaces") || {
    printf '%s\n' "Codex ProjectAtlas plugin update skipped: Codex plugin inventory requires jq for trustworthy JSON validation."
    return 0
  }
  if ! official_projectatlas_marketplace_source "$marketplace_source"; then
    printf '%s\n' "Codex ProjectAtlas plugin update skipped: projectatlas marketplace is not the official styler-ai/ProjectAtlas source."
    return 0
  fi
  previous_ref=$(codex_projectatlas_marketplace_ref)

  release_tag=v$runtime_version
  if ! load_codex_projectatlas_plugin_inventory; then
    codex_plugin_update_preserved_prior_state=true
    printf '%s\n' "warning: Codex ProjectAtlas plugin update skipped: installed plugin inventory could not be verified completely; the existing official integration was not changed." >&2
    return 0
  fi
  current_plugin_version=$codex_projectatlas_inventory_version
  current_plugin_source_path=$codex_projectatlas_inventory_source_path
  if [ "$previous_ref" = "$release_tag" ] &&
    [ "$current_plugin_version" = "$runtime_version" ] &&
    codex_projectatlas_plugin_source_ready "$runtime_version" "$current_plugin_source_path"; then
    printf 'Codex ProjectAtlas plugin marketplace already points to %s.\n' "$release_tag"
    verify_codex_projectatlas_skill_artifact
    return 0
  fi
  if [ "$previous_ref" = "$release_tag" ]; then
    if [ "$current_plugin_version" = "$runtime_version" ] &&
      ! codex_projectatlas_plugin_source_manifest_matches "$runtime_version" "$current_plugin_source_path"; then
      source_manifest_version=$(codex_projectatlas_plugin_source_manifest_version "$current_plugin_source_path")
      printf "Codex ProjectAtlas plugin source manifest version '%s' does not match %s; refreshing official projectatlas plugin cache.\n" "$source_manifest_version" "$runtime_version"
    elif [ "$current_plugin_version" = "$runtime_version" ]; then
      printf 'Codex ProjectAtlas plugin skill artifact does not match %s; refreshing official projectatlas plugin cache.\n' "$runtime_version"
    fi
    if ! stage_codex_projectatlas_snapshot "$current_plugin_version" "$current_plugin_source_path" "$runtime_version"; then
      codex_plugin_update_preserved_prior_state=true
      return 0
    fi
    update_succeeded=false
    restore_succeeded=false
    "$codex_bin" plugin remove projectatlas --marketplace projectatlas --json >/dev/null 2>&1 || true
    if "$codex_bin" plugin add projectatlas --marketplace projectatlas --json >/dev/null 2>&1; then
      if load_codex_projectatlas_plugin_inventory && [ -n "$codex_projectatlas_inventory_version" ]; then
        installed_version=$codex_projectatlas_inventory_version
        installed_source_path=$codex_projectatlas_inventory_source_path
      else
        installed_version=
        installed_source_path=
        printf '%s\n' "warning: Codex ProjectAtlas plugin update failed: installed plugin inventory could not be verified completely after refresh." >&2
      fi
      if [ -n "$installed_version" ] && [ "$installed_version" = "$runtime_version" ]; then
        if codex_projectatlas_plugin_source_manifest_matches "$runtime_version" "$installed_source_path"; then
          update_succeeded=true
          printf 'Codex ProjectAtlas plugin marketplace updated to %s.\n' "$release_tag"
          verify_codex_projectatlas_skill_artifact
        else
          source_manifest_version=$(codex_projectatlas_plugin_source_manifest_version "$installed_source_path")
          printf "warning: Codex ProjectAtlas plugin update failed: source manifest version '%s' does not match %s after refresh.\n" "$source_manifest_version" "$runtime_version" >&2
        fi
      elif [ -n "$installed_version" ]; then
        printf "warning: Codex ProjectAtlas plugin update failed: installed projectatlas plugin version '%s' does not match %s.\n" "$installed_version" "$runtime_version" >&2
      fi
    else
      printf 'warning: Codex ProjectAtlas plugin update failed: could not install projectatlas plugin at %s.\n' "$release_tag" >&2
    fi
    if [ "$update_succeeded" != true ]; then
      codex_plugin_update_preserved_prior_state=true
      if restore_codex_projectatlas_snapshot; then
        restore_succeeded=true
      else
        printf "warning: Codex ProjectAtlas plugin update failed and its preserved local state could not be restored completely; the recovery snapshot was retained at '%s'.\n" "$codex_state_snapshot_dir" >&2
      fi
    fi
    if [ "$update_succeeded" = true ] || [ "$restore_succeeded" = true ]; then
      clear_codex_projectatlas_snapshot || true
    fi
    return 0
  fi

  if ! stage_codex_projectatlas_snapshot "$current_plugin_version" "$current_plugin_source_path" "$runtime_version"; then
    codex_plugin_update_preserved_prior_state=true
    return 0
  fi
  if [ -d "$codex_state_snapshot_marketplace_root_path/.git" ] &&
    ! git -C "$codex_state_snapshot_marketplace_root_path" rev-parse --verify --quiet "refs/tags/$release_tag^{commit}" >/dev/null 2>&1 &&
    ! git -C "$codex_state_snapshot_marketplace_root_path" fetch --force --no-tags https://github.com/styler-ai/ProjectAtlas.git "refs/tags/$release_tag:refs/tags/$release_tag" >/dev/null 2>&1; then
    printf 'warning: Codex ProjectAtlas plugin update failed: could not fetch release tag %s.\n' "$release_tag" >&2
    codex_plugin_update_preserved_prior_state=true
    if restore_codex_projectatlas_snapshot; then
      clear_codex_projectatlas_snapshot || true
    else
      printf "warning: Codex ProjectAtlas release-tag fetch failed and its preserved local state could not be restored completely; the recovery snapshot was retained at '%s'.\n" "$codex_state_snapshot_dir" >&2
    fi
    return 0
  fi
  update_succeeded=false
  restore_succeeded=false
  if ! "$codex_bin" plugin marketplace remove projectatlas --json >/dev/null 2>&1; then
    printf '%s\n' "warning: Codex ProjectAtlas plugin update failed: could not remove stale projectatlas marketplace." >&2
    if restore_codex_projectatlas_snapshot; then
      restore_succeeded=true
    else
      printf "warning: Codex ProjectAtlas marketplace replacement failed and its preserved local state could not be restored completely; the recovery snapshot was retained at '%s'.\n" "$codex_state_snapshot_dir" >&2
    fi
    codex_plugin_update_preserved_prior_state=true
    if [ "$restore_succeeded" = true ]; then
      clear_codex_projectatlas_snapshot || true
    fi
    return 0
  fi
  if ! "$codex_bin" plugin marketplace add styler-ai/ProjectAtlas --ref "$release_tag" --json >/dev/null 2>&1; then
    printf 'warning: Codex ProjectAtlas plugin update failed: could not add projectatlas marketplace at %s.\n' "$release_tag" >&2
    if restore_codex_projectatlas_snapshot; then
      restore_succeeded=true
    else
      printf "warning: Codex ProjectAtlas marketplace replacement failed and its preserved local state could not be restored completely; the recovery snapshot was retained at '%s'.\n" "$codex_state_snapshot_dir" >&2
    fi
    codex_plugin_update_preserved_prior_state=true
    if [ "$restore_succeeded" = true ]; then
      clear_codex_projectatlas_snapshot || true
    fi
    return 0
  fi
  "$codex_bin" plugin remove projectatlas --marketplace projectatlas --json >/dev/null 2>&1 || true
  if "$codex_bin" plugin add projectatlas --marketplace projectatlas --json >/dev/null 2>&1; then
    if load_codex_projectatlas_plugin_inventory && [ -n "$codex_projectatlas_inventory_version" ]; then
      installed_version=$codex_projectatlas_inventory_version
      installed_source_path=$codex_projectatlas_inventory_source_path
    else
      installed_version=
      installed_source_path=
      printf '%s\n' "warning: Codex ProjectAtlas plugin update failed: installed plugin inventory could not be verified completely after refresh." >&2
    fi
    if [ -n "$installed_version" ] && [ "$installed_version" = "$runtime_version" ]; then
      if codex_projectatlas_plugin_source_manifest_matches "$runtime_version" "$installed_source_path"; then
        update_succeeded=true
        printf 'Codex ProjectAtlas plugin marketplace updated to %s.\n' "$release_tag"
        verify_codex_projectatlas_skill_artifact
      else
        source_manifest_version=$(codex_projectatlas_plugin_source_manifest_version "$installed_source_path")
        printf "warning: Codex ProjectAtlas plugin update failed: source manifest version '%s' does not match %s after refresh.\n" "$source_manifest_version" "$runtime_version" >&2
      fi
    elif [ -n "$installed_version" ]; then
      printf "warning: Codex ProjectAtlas plugin update failed: installed projectatlas plugin version '%s' does not match %s.\n" "$installed_version" "$runtime_version" >&2
    fi
  else
    printf 'warning: Codex ProjectAtlas plugin update failed: could not install projectatlas plugin at %s.\n' "$release_tag" >&2
  fi
  if [ "$update_succeeded" != true ]; then
    codex_plugin_update_preserved_prior_state=true
    if restore_codex_projectatlas_snapshot; then
      restore_succeeded=true
    else
      printf "warning: Codex ProjectAtlas plugin update failed and its preserved local state could not be restored completely; the recovery snapshot was retained at '%s'.\n" "$codex_state_snapshot_dir" >&2
    fi
  fi
  if [ "$update_succeeded" = true ] || [ "$restore_succeeded" = true ]; then
    clear_codex_projectatlas_snapshot || true
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
        *github.com/styler-ai/ProjectAtlas/releases/download/*/*)
          printf '%s\n' "$line" |
            grep -Eo 'https://github\.com/styler-ai/ProjectAtlas/releases/download/[^/[:space:]]+/' |
            sed -E 's#^.*/download/([^/]+)/$#\1#' |
            grep -E '^v[0-9][A-Za-z0-9.+-]*$' |
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

if [ -n "$runtime_override" ]; then
  runtime_override=$(canonical_file "$runtime_override") || {
    printf '%s\n' "ProjectAtlas runtime path could not be canonicalized: $runtime_override" >&2
    exit 1
  }
fi

if [ "$uninstall" -eq 1 ]; then
  uninstall_atlas_forwarders
  exit 0
fi

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

write_atlas_forwarder "$projectatlas_bin"
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
  temporary_output=$(mktemp "$atlas_dir/.projectatlas-mcp-config.XXXXXX")
  if ! "$projectatlas_bin" "$@" > "$temporary_output"; then
    rm -f "$temporary_output"
    return 1
  fi
  if ! assert_config_output_path "$output_path"; then
    rm -f "$temporary_output"
    return 1
  fi
  if ! mv -f "$temporary_output" "$output_path"; then
    rm -f "$temporary_output"
    return 1
  fi
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
