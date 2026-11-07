#!/usr/bin/env bash
# Run the owning project-state selectors on native platform fixtures in CI or against packaged executables.
set -euo pipefail

mode="${1:-}"
if [ "$#" -gt 0 ]; then shift; fi
contract_bin=""
if [ "$#" -gt 0 ]; then
  if [ "$#" -ne 2 ] || [ "$1" != --contract-bin ]; then
    echo "usage: $0 ownership|macos-volumes [--contract-bin DIRECTORY]" >&2
    exit 2
  fi
  contract_bin="$(cd "$2" && pwd)"
fi
case "$mode" in ownership|macos-volumes) ;; *) echo "unknown native project-state mode: $mode" >&2; exit 2 ;; esac
if [ -n "$contract_bin" ]; then
  : "${PROJECTATLAS_MCP_CONTRACT_EXECUTABLE:?installed runtime selection is required}"
fi

run_contract() {
  local binary="$1" target="$2" selector="$3"
  shift 3
  if [ -z "$contract_bin" ]; then
    cargo test --locked -p projectatlas-cli "$target" "$binary" "$selector" "$@"
    return
  fi
  if [ "${1:-}" = -- ]; then shift; fi
  if [ "$target" = --bin ]; then binary=projectatlas_unit; fi
  if [ "$RUNNER_OS" = Windows ]; then binary="$binary.exe"; fi
  local executable="$contract_bin/$binary" inventory matches
  if ! inventory="$("$executable" --list)"; then
    echo "Packaged native contract inventory failed: $binary" >&2
    return 1
  fi
  matches="$(printf '%s\n' "$inventory" | awk -v selector="$selector" 'index($0, selector) == 1 && /: test$/ { count += 1 } END { print count + 0 }')"
  if [ "$matches" -eq 0 ]; then
    echo "Packaged native contract selector has no tests: $selector" >&2
    return 1
  fi
  case " $* " in
    *" --exact "*)
      matches="$(printf '%s\n' "$inventory" | awk -v expected="${selector}: test" '$0 == expected { count += 1 } END { print count + 0 }')"
      if [ "$matches" -ne 1 ]; then
        echo "Packaged native contract must contain exactly one test: $selector" >&2
        return 1
      fi ;;
  esac
  "$executable" "$selector" "$@"
}
run_unit() { run_contract projectatlas --bin "$@"; }
run_lifecycle() { run_contract e2e_lifecycle --test "$@"; }
run_worktrees() { run_contract e2e_worktrees --test "$@"; }

unit_test_executable() {
  if [ -n "$contract_bin" ]; then
    printf '%s/projectatlas_unit\n' "$contract_bin"
    return
  fi
  local target_receipt="$RUNNER_TEMP/project-state-unit-target.jsonl" executable
  cargo test --locked -p projectatlas-cli --bin projectatlas --no-run --message-format=json > "$target_receipt" || return
  executable="$(python3 - "$target_receipt" <<'PY'
import json
import sys
artifacts = [json.loads(line) for line in open(sys.argv[1], encoding="utf-8")]
binaries = {row["executable"] for row in artifacts if row.get("reason") == "compiler-artifact" and row.get("target", {}).get("name") == "projectatlas" and row.get("profile", {}).get("test") and row.get("executable")}
if len(binaries) != 1:
    raise SystemExit("expected one exact native unit-test executable")
print(next(iter(binaries)))
PY
  )" || return
  printf '%s\n' "$executable"
}

if [ "$mode" = ownership ]; then
run_managed_state_routes() {
  run_lifecycle automatic_state_admission_preserves_cli_mcp_navigation -- --exact
  run_lifecycle managed_document_writers_preserve_cli_mcp_capabilities -- --exact
  run_lifecycle root_and_metadata_validation_flow -- --exact
}
native_policy_status=0
if [ "$RUNNER_OS" = Windows ]; then
  run_unit project_state::tests::windows_encrypted_outputs_are_preserved_or_refused_before_publication -- --exact --include-ignored --nocapture || native_policy_status=$?
  run_unit runtime::tests::windows_managed_database_aliases_preserve_wal_recovery_and_reset -- --exact --nocapture || native_policy_status=$?
fi
run_unit project_state:: || native_policy_status=$?
run_unit atlas_map::tests::relative_config_roots_follow_native_private_directory_names -- --exact || native_policy_status=$?
run_unit atlas_map::tests::default_init_config_root_points_from_subdir_back_to_repo -- --exact || native_policy_status=$?
if [ "$native_policy_status" -ne 0 ]; then
  exit "$native_policy_status"
fi
# Compile and exercise the real routes before moving test temporaries
# onto the small disposable filesystem images.
if [ "$RUNNER_OS" = Windows ]; then
  export PROJECTATLAS_TEST_NATIVE_EFS=1
fi
run_managed_state_routes
if [ "$RUNNER_OS" = Windows ]; then
  PROJECTATLAS_TEST_CASE_SENSITIVE=true run_lifecycle managed_document_writers_preserve_cli_mcp_capabilities -- --exact
fi
if [ "$RUNNER_OS" = Linux ]; then
  # Build as the runner; elevate only the disposable native-label test.
  test_executable="$(unit_test_executable)"
  sudo -n env TMPDIR="$RUNNER_TEMP" "$test_executable" --exact project_state::tests::replacement_preserves_linux_security_labels_without_optional_relabel_privilege --ignored --nocapture
  # Disposable ext4 and f2fs prove both implemented Unicode profiles.
  sudo -n apt-get update
  sudo -n apt-get install --yes f2fs-tools
  for native_filesystem in ext4 f2fs; do
    image="$RUNNER_TEMP/project-state-native.$native_filesystem"
    native_root="$RUNNER_TEMP/project-state-native-$native_filesystem"
    mkdir "$native_root"
    truncate -s 512M "$image"
    if [ "$native_filesystem" = ext4 ]; then
      mkfs.ext4 -q -F -b 4096 -O casefold,verity -E encoding=utf8-12.1 "$image"
    else
      mkfs.f2fs -q -f -O extra_attr,inode_checksum,flexible_inline_xattr,casefold,verity -C utf8 "$image"
    fi
    native_mounted=false
    native_cleanup_failed=false
    cleanup_native_state() {
      if [ "$native_cleanup_failed" = true ]; then return 1; fi
      if [ "$native_mounted" = true ]; then
        if ! sudo -n umount "$native_root"; then
          native_cleanup_failed=true
          findmnt -T "$native_root" || true
          sudo -n fuser -vm "$native_root" || true
          ps -axo pid,ppid,stat,command
          return 1
        fi
        native_mounted=false
      fi
      rm -f "$image"
      rmdir "$native_root"
    }
    trap cleanup_native_state EXIT
    sudo -n mount -o loop,nodev,nosuid "$image" "$native_root"
    native_mounted=true
    sudo -n chown "$(id -u):$(id -g)" "$native_root"
    mkdir "$native_root/sensitive" "$native_root/casefold"
    chattr +F "$native_root/casefold"
    lsattr -d "$native_root/sensitive" "$native_root/casefold"
    for native_mode in sensitive casefold; do
      export TMPDIR="$native_root/$native_mode"
      export PROJECTATLAS_TEST_NATIVE_VERITY=1
      export PROJECTATLAS_TEST_CASE_SENSITIVE=true
      if [ "$native_mode" = casefold ]; then
        export PROJECTATLAS_TEST_CASE_SENSITIVE=false
      fi
      "$test_executable" project_state:: --nocapture
      "$test_executable" --exact project_state::tests::replacement_refuses_native_verity_and_late_sealing_without_leaking_stages --ignored --nocapture
      run_managed_state_routes
    done
    export TMPDIR="$RUNNER_TEMP"
    unset PROJECTATLAS_TEST_CASE_SENSITIVE
    unset PROJECTATLAS_TEST_NATIVE_VERITY
    cleanup_native_state
    native_mounted=false
    trap - EXIT
  done
elif [ "$RUNNER_OS" = macOS ]; then
  run_unit project_state::tests::private_replacement_staging_excludes_inherited_acl_handles_and_retains_foreign_entries -- --exact --include-ignored --nocapture
fi
run_unit runtime::tests::init_admits_legacy_config_before_repairing_missing_root_identity -- --exact
# The existing worktree-contract step covers the other three platforms.
if [ -n "$contract_bin" ] || [ "${PROJECTATLAS_CI_PLATFORM:-}" = "macos-x64" ]; then
  run_worktrees holistic_agent_worktree_flow_keeps_local_atlases_isolated_across_cli_watch_and_mcp -- --exact --include-ignored
fi
elif [ "$RUNNER_OS" = macOS ]; then
set -euo pipefail
original_tmpdir="${TMPDIR:-$RUNNER_TEMP}"
fixture="$(mktemp -d /private/tmp/projectatlas-case-modes.XXXXXX)"
# Native nobody ACL controls must reach this disposable mount through
# its ancestors; otherwise an unrelated 0700 temp parent masks access.
chmod 755 "$fixture"
volume="$fixture/volume"
mounted=false
nested_mounted=false
detach_failed=false
nested="$volume/.projectatlas/bridge"
attach_native_volume() {
  local target="$1" elevated="$2" image_path="$3" receipt="$4"
  python3 - "$image_path" "$target" "$receipt.owner.json" <<'PY'
import json, os, stat, sys
image, target, output = sys.argv[1:]
identity = os.lstat(image)
if not stat.S_ISREG(identity.st_mode):
    raise SystemExit("fixture image is not a direct regular file")
with open(output, "w", encoding="utf-8") as stream:
    json.dump({"path": os.path.realpath(image), "device": identity.st_dev, "inode": identity.st_ino, "mountpoint": os.path.realpath(target)}, stream)
PY
  if [ "$elevated" = true ]; then
    sudo -n hdiutil attach -plist -nobrowse -owners on -mountpoint "$target" "$image_path" > "$receipt"
  else
    hdiutil attach -plist -nobrowse -owners on -mountpoint "$target" "$image_path" > "$receipt"
  fi
}
native_volume_state() {
  local image_path="$1" receipt="$2"
  hdiutil info -plist > "$receipt.current.plist" || return
  python3 - "$image_path" "$receipt" "$receipt.current.plist" <<'PY'
import json, os, plistlib, re, sys
image, receipt, current = sys.argv[1:]
with open(receipt + ".owner.json", encoding="utf-8") as stream:
    owner = json.load(stream)
identity = os.lstat(image)
if (os.path.realpath(image), identity.st_dev, identity.st_ino) != (owner["path"], owner["device"], owner["inode"]):
    raise SystemExit("fixture image identity changed")
with open(receipt, "rb") as stream:
    attached = plistlib.load(stream)["system-entities"]
with open(current, "rb") as stream:
    images = plistlib.load(stream)["images"]
devices = [item["dev-entry"] for item in attached if "dev-entry" in item]
whole = [device for device in devices if re.fullmatch(r"/dev/disk[0-9]+", device)]
if not whole or len(set(devices)) != len(devices):
    raise SystemExit("attachment lacks an unambiguous whole device")
mounts = [owner["mountpoint"], *[item["mount-point"] for item in attached if item.get("mount-point")]]
matching = [item for item in images if os.path.realpath(item.get("image-path", "")) == owner["path"]]
if len(matching) > 1:
    raise SystemExit("fixture image attachment is ambiguous")
reported_mounts = [os.path.realpath(entity["mount-point"]) for item in images for entity in item.get("system-entities", []) if entity.get("mount-point")]
if not matching:
    if any(os.path.ismount(path) or path in reported_mounts for path in mounts):
        raise SystemExit("fixture mount persists without its image mapping")
    print("detached")
else:
    entities = matching[0]["system-entities"]
    observed = [item["dev-entry"] for item in entities if "dev-entry" in item]
    if set(observed) != set(devices):
        raise SystemExit("fixture image device mapping changed")
    owned_mounts = [os.path.realpath(item["mount-point"]) for item in entities if item.get("mount-point")]
    if owned_mounts or any(os.path.ismount(path) for path in mounts):
        if owner["mountpoint"] not in owned_mounts or reported_mounts.count(owner["mountpoint"]) != 1:
            raise SystemExit("fixture mount ownership changed")
        print("mounted")
    else:
        print("unmounted:" + whole[0])
PY
}
detach_native_volume() {
  local target="$1" elevated="$2" image_path="$3" receipt="$4"
  local state="unknown" device="" attempt admitted=false
  if state="$(native_volume_state "$image_path" "$receipt")"; then
    case "$state" in
      detached) return 0 ;;
      mounted|unmounted:*) admitted=true ;;
    esac
  fi
  # The first mountpoint operation needs the same ownership proof as
  # recovery. Successful exit alone does not prove the image detached.
  if [ "$admitted" = true ] && [ "$state" = mounted ]; then
    if [ "$elevated" = true ]; then
      if sudo -n hdiutil detach "$target"; then
        if state="$(native_volume_state "$image_path" "$receipt")"; then
          if [ "$state" = detached ]; then return 0; fi
        else admitted=false; fi
      fi
    else
      if hdiutil detach "$target"; then
        if state="$(native_volume_state "$image_path" "$receipt")"; then
          if [ "$state" = detached ]; then return 0; fi
        else admitted=false; fi
      fi
    fi
  fi
  # A non-force detach can unmount successfully and then find the image's
  # device busy. Recover only that proven partial completion, never a live mount.
  if [ "$admitted" = true ]; then
    for attempt in 1 2 3; do
      if ! state="$(native_volume_state "$image_path" "$receipt")"; then break; fi
      if [ "$state" = detached ]; then
        echo "Owned image detached after its volume unmounted: $image_path"
        return 0
      fi
      case "$state" in
        unmounted:*) device="${state#unmounted:}" ;;
        *) break ;;
      esac
      if [ "$attempt" -lt 3 ]; then sleep 1; fi
    done
  fi
  if [ "$admitted" = true ] && [ "$state" = "unmounted:$device" ] && [ -n "$device" ]; then
    # Openers can outlive the unmount. Revalidate ownership and the
    # unmounted state before EVERY bounded, non-force eject attempt.
    for attempt in 1 2 3 4 5 6 7 8 9 10; do
      if ! state="$(native_volume_state "$image_path" "$receipt")"; then break; fi
      if [ "$state" = detached ]; then return 0; fi
      case "$state" in
        unmounted:*) device="${state#unmounted:}" ;;
        *) break ;;
      esac
      echo "Finishing owned unmounted image detach through verified device: $device (attempt $attempt/10)"
      if [ "$elevated" = true ]; then
        if sudo -n hdiutil detach "$device"; then
          if [ "$(native_volume_state "$image_path" "$receipt")" = detached ]; then return 0; fi
        fi
      else
        if hdiutil detach "$device"; then
          if [ "$(native_volume_state "$image_path" "$receipt")" = detached ]; then return 0; fi
        fi
      fi
      if [ "$attempt" -lt 10 ]; then sleep 2; fi
    done
  fi
  detach_failed=true
  diskutil info "${device:-$target}" || true
  hdiutil info || true
  mount
  sudo -n lsof -nP "${device:-$target}" || true
  ps -axo pid,ppid,stat,command
  return 1
}
cleanup() {
  export TMPDIR="$original_tmpdir"
  if [ "$detach_failed" = true ]; then return 1; fi
  if [ "$nested_mounted" = true ]; then
    detach_native_volume "$nested" true "$nested_image" "$nested_receipt" || return 1
    nested_mounted=false
  fi
  if [ "$mounted" = true ]; then
    detach_native_volume "$volume" false "$image" "$volume_receipt" || return 1
    mounted=false
  fi
  rm -rf "$fixture"
}
trap cleanup EXIT
mkdir "$volume"
create_case_image() {
  local filesystem="$1"
  # APFSX is diskutil's personality; hdiutil uses this full name.
  if [ "$filesystem" = APFSX ]; then
    filesystem="Case-sensitive APFS"
  fi
  hdiutil create -size 128m -fs "$filesystem" -volname "$2" -ov "$3"
}
for filesystem in APFS APFSX HFS+ HFSX; do
  image="$fixture/$filesystem.dmg"
  create_case_image "$filesystem" ProjectAtlasCase "$image"
  volume_receipt="$fixture/$filesystem-attach.plist"
  # Track attempted attachments too: a failed attach may be partial.
  mounted=true
  attach_native_volume "$volume" false "$image" "$volume_receipt"
  mkdir -p "$volume/tmp" "$nested"
  nested_filesystem=APFSX
  if [ "$filesystem" = APFSX ] || [ "$filesystem" = HFSX ]; then
    nested_filesystem=APFS
  fi
  nested_image="$fixture/nested-$filesystem.dmg"
  create_case_image "$nested_filesystem" ProjectAtlasNested "$nested_image"
  # The runner's unprivileged mount is refused below another volume.
  # Scope elevation to this disposable image and return its root to
  # the runner before executing any product command.
  nested_receipt="$fixture/nested-$filesystem-attach.plist"
  nested_mounted=true
  attach_native_volume "$nested" true "$nested_image" "$nested_receipt"
  sudo -n chown "$(id -u):$(id -g)" "$nested"
  export PROJECTATLAS_TEST_CASE_MOUNT="$nested"
  export TMPDIR="$volume/tmp"
  export PROJECTATLAS_TEST_NATIVE_ACCESS_ROOT="$volume/tmp"
  diskutil info "$volume"
  ls -lde /private /private/tmp "$fixture" "$volume" "$volume/tmp"
  sudo -n -u nobody /bin/test -x "$volume/tmp"
  export PROJECTATLAS_TEST_CASE_SENSITIVE=false
  if [ "$filesystem" = APFSX ] || [ "$filesystem" = HFSX ]; then
    export PROJECTATLAS_TEST_CASE_SENSITIVE=true
  fi
  run_unit project_state::
  run_unit atlas_map::tests::relative_config_roots_follow_native_private_directory_names -- --exact
  run_unit project_state::tests::private_replacement_staging_excludes_inherited_acl_handles_and_retains_foreign_entries -- --exact --include-ignored --nocapture
  run_lifecycle automatic_state_admission_preserves_cli_mcp_navigation -- --exact
  run_lifecycle managed_document_writers_preserve_cli_mcp_capabilities -- --exact
  export TMPDIR="$original_tmpdir"
  unset PROJECTATLAS_TEST_NATIVE_ACCESS_ROOT
  detach_native_volume "$nested" true "$nested_image" "$nested_receipt"
  nested_mounted=false
  detach_native_volume "$volume" false "$image" "$volume_receipt"
  mounted=false
done
else
  echo "macos-volumes requires a macOS runner" >&2
  exit 2
fi
