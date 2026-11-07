#!/usr/bin/env python3
"""Prove native image cleanup refuses uncertain state before a recovery eject."""

from __future__ import annotations

import json
import os
from pathlib import Path
import plistlib
import subprocess
import tempfile
import textwrap

ROOT = Path(__file__).resolve().parents[2]
CONTRACTS = ROOT / ".github/scripts/native-project-state-contracts.sh"


def verify_contract_dispatch() -> int:
    """Reject failed inventories/builds even when a caller captures shell failure."""
    source = CONTRACTS.read_text(encoding="utf-8")
    functions = source[source.index("run_contract() {"):source.index('\nif [ "$mode" = ownership ]; then')]
    cases = [
        ("unit", "Linux", "valid", 0),
        ("lifecycle", "Linux", "valid", 0),
        ("worktrees", "Linux", "valid", 0),
        ("unit", "Windows", "valid", 0),
        ("lifecycle", "Windows", "valid", 0),
        ("unit", "Linux", "missing", 1),
        ("unit", "Linux", "duplicate", 1),
        ("unit", "Linux", "inventory-failure", 1),
        ("unit", "Linux", "test-failure", 9),
        ("unit", "Linux", "compile-failure", 7),
    ]
    for target, platform, behavior, expected in cases:
        with tempfile.TemporaryDirectory(prefix="native-contract-routing-") as temporary:
            directory = Path(temporary)
            binary = {"unit": "projectatlas_unit", "lifecycle": "e2e_lifecycle", "worktrees": "e2e_worktrees"}[target]
            if platform == "Windows":
                binary += ".exe"
            executable = directory / binary
            executable.write_text(r'''#!/usr/bin/env bash
set -euo pipefail
if [ "${CARGO_BIN_EXE_projectatlas:-}" != "$PROJECTATLAS_MCP_CONTRACT_EXECUTABLE" ]; then exit 23; fi
if [ "${1:-}" = --list ]; then
  if [ "$MODEL_BEHAVIOR" = missing ]; then exit 0; fi
  printf 'owning_contract: test\n'
  if [ "$MODEL_BEHAVIOR" = duplicate ]; then printf 'owning_contract: test\n'; fi
  if [ "$MODEL_BEHAVIOR" = inventory-failure ]; then exit 7; fi
  exit 0
fi
printf '%s\n' "$@" > "$MODEL_ROOT/arguments"
if [ "$MODEL_BEHAVIOR" = test-failure ]; then exit 9; fi
''', encoding="utf-8")
            executable.chmod(0o755)
            script = 'set -euo pipefail\n' + functions + r'''
contract_bin="$MODEL_ROOT"
if [ "$MODEL_BEHAVIOR" = compile-failure ]; then
  contract_bin=""
  cargo() { printf '{"reason":"compiler-artifact","target":{"name":"projectatlas"},"profile":{"test":true},"executable":"stale"}\n'; return 7; }
  if value="$(unit_test_executable)"; then result=0; else result=$?; fi
else
  result=0
  run_'''+target+r''' owning_contract -- --exact --nocapture || result=$?
fi
exit "$result"
'''
            result = subprocess.run(["bash", "-c", script], text=True, capture_output=True, timeout=10,
                                    env={**os.environ, "MODEL_ROOT": str(directory), "RUNNER_TEMP": str(directory),
                                         "RUNNER_OS": platform, "MODEL_BEHAVIOR": behavior,
                                         "PROJECTATLAS_MCP_CONTRACT_EXECUTABLE": str(directory / "installed runtime"),
                                         "CARGO_BIN_EXE_projectatlas": "unselected-ambient-runtime"})
            assert result.returncode == expected, (target, platform, behavior, result.stdout, result.stderr)
            arguments = directory / "arguments"
            assert arguments.exists() == (behavior in ("valid", "test-failure")), behavior
            if arguments.exists():
                assert arguments.read_text().splitlines() == ["owning_contract", "--exact", "--nocapture"]
    return len(cases)


def main() -> None:
    """Run the actual workflow functions against isolated disk-command doubles."""
    routing_controls = verify_contract_dispatch()
    source = CONTRACTS.read_text(encoding="utf-8")
    start = source.index("attach_native_volume() {")
    end = source.index("trap cleanup EXIT", start)
    helpers = textwrap.dedent(source[start:end])
    controls = {
        "ordinary": (0, 1),
        "already-detached": (0, 0),
        "mounted": (1, 1),
        "foreign-mount": (1, 0),
        "changed-image-first-success": (1, 0),
        "successful-detach-unconfirmed": (1, 1),
        "changed-device": (1, 0),
        "changed-image": (1, 0),
        "invalid-info": (1, 0),
        "mapping-changes-during-poll": (1, 1),
        "partial-unmount": (0, 2),
        "second-eject-fails": (1, 11),
        "busy-then-quiescent": (0, 4),
        "mapping-changes-after-recovery": (1, 2),
        "mount-appears-after-recovery": (1, 2),
        "image-changes-after-recovery": (1, 2),
        "attach-partial": (25, 1),
        "attach-empty": (25, 0),
        "attach-empty-foreign": (25, 0),
    }
    for elevated in ("false", "true"):
        for case, (expected_status, expected_calls) in controls.items():
            with tempfile.TemporaryDirectory(prefix="native-volume-control-") as temporary:
                directory = Path(temporary) / "fixture"
                directory.mkdir()
                audit = Path(temporary) / "audit"
                audit.mkdir()
                image = directory / "owned.dmg"
                image.write_bytes(b"owned disposable image")
                mountpoint = directory / "mount"
                mountpoint.mkdir()
                entities = [
                    {"dev-entry": "/dev/disk5", "content-hint": "GUID_partition_scheme"},
                    {"dev-entry": "/dev/disk5s1"},
                    {"dev-entry": "/dev/disk6"},
                    {"dev-entry": "/dev/disk6s1", "mount-point": str(mountpoint)},
                ]
                if case == "attach-partial":
                    entities[-1].pop("mount-point")
                (directory / "attachment-input.plist").write_bytes(
                    plistlib.dumps({"system-entities": entities})
                )
                unmounted = [{"dev-entry": entry["dev-entry"]} for entry in entities]
                observed = entities if case in ("ordinary", "mounted", "partial-unmount", "second-eject-fails", "busy-then-quiescent", "mapping-changes-during-poll", "mapping-changes-after-recovery", "mount-appears-after-recovery", "image-changes-after-recovery", "attach-partial", "successful-detach-unconfirmed", "changed-image-first-success") else unmounted
                if case == "changed-device":
                    observed = [{"dev-entry": "/dev/disk9"}]
                info = {"images": [{"image-path": str(image), "system-entities": observed}]}
                if case in ("already-detached", "attach-empty"):
                    info = {"images": []}
                if case in ("foreign-mount", "attach-empty-foreign"):
                    info = {"images": [{"image-path": str(directory / "foreign.dmg"), "system-entities": [{"dev-entry": "/dev/disk9", "mount-point": str(mountpoint)}]}]}
                (directory / "fresh.plist").write_bytes(plistlib.dumps(info))
                (directory / "unmounted.plist").write_bytes(plistlib.dumps({"images": [{"image-path": str(image), "system-entities": unmounted}]}))
                (directory / "detached.plist").write_bytes(plistlib.dumps({"images": []}))
                (directory / "mounted.plist").write_bytes(plistlib.dumps({"images": [{
                    "image-path": str(image), "system-entities": entities
                }]}))
                (directory / "changed.plist").write_bytes(plistlib.dumps({"images": [{
                    "image-path": str(image), "system-entities": [{"dev-entry": "/dev/disk9"}]
                }]}))
                if case == "invalid-info":
                    (directory / "fresh.plist").write_bytes(b"not a plist")
                script = r'''
set -euo pipefail
hdiutil() {
  case "$1" in
    attach)
      if [ "$MODEL_CASE" = attach-empty ] || [ "$MODEL_CASE" = attach-empty-foreign ]; then return 25; fi
      cat "$MODEL_ROOT/attachment-input.plist"
      if [ "$MODEL_CASE" = attach-partial ]; then return 25; fi ;;
    info)
      if [ "${2:-}" = -plist ]; then
        local count=0
        if [ -f "$MODEL_AUDIT/info-count" ]; then count="$(cat "$MODEL_AUDIT/info-count")"; fi
        count=$((count+1)); echo "$count" > "$MODEL_AUDIT/info-count"
        if [ "$MODEL_CASE" = mapping-changes-during-poll ] && [ "$count" -ge 2 ]; then
          cp "$MODEL_ROOT/changed.plist" "$MODEL_ROOT/fresh.plist"
        fi
        cat "$MODEL_ROOT/fresh.plist"
      fi ;;
    detach)
      echo "$2" >> "$MODEL_AUDIT/calls"
      if [ "$#" -ne 2 ]; then return 93; fi
      if [ "$MODEL_CASE" = ordinary ] || [ "$MODEL_CASE" = changed-image-first-success ]; then
        cp "$MODEL_ROOT/detached.plist" "$MODEL_ROOT/fresh.plist"
        return 0
      fi
      if [ "$MODEL_CASE" = successful-detach-unconfirmed ]; then return 0; fi
      if [ "$2" = "$MODEL_ROOT/mount" ] && [ "$MODEL_CASE" != mounted ]; then
        cp "$MODEL_ROOT/unmounted.plist" "$MODEL_ROOT/fresh.plist"
      fi
      if [ "$2" = /dev/disk5 ]; then
        case "$MODEL_CASE" in
          busy-then-quiescent)
            if [ "$(wc -l < "$MODEL_AUDIT/calls")" -ge 4 ]; then
              cp "$MODEL_ROOT/detached.plist" "$MODEL_ROOT/fresh.plist"
              return 0
            fi ;;
          mapping-changes-after-recovery)
            cp "$MODEL_ROOT/changed.plist" "$MODEL_ROOT/fresh.plist" ;;
          mount-appears-after-recovery)
            cp "$MODEL_ROOT/mounted.plist" "$MODEL_ROOT/fresh.plist" ;;
          image-changes-after-recovery)
            echo replacement > "$MODEL_ROOT/replacement"
            mv "$MODEL_ROOT/replacement" "$MODEL_ROOT/owned.dmg" ;;
        esac
      fi
      if [ "$2" = /dev/disk5 ] && { [ "$MODEL_CASE" = partial-unmount ] || [ "$MODEL_CASE" = attach-partial ]; }; then
        cp "$MODEL_ROOT/detached.plist" "$MODEL_ROOT/fresh.plist"
        return 0
      fi
      return 1 ;;
    *) return 90 ;;
  esac
}
sudo() { if [ "$1" = -n ]; then shift; fi; "$@"; }
sleep() { :; }
diskutil() { :; }
lsof() { :; }
mount() { :; }
ps() { :; }
'''
                script += helpers
                script += r'''
fixture="$MODEL_ROOT"
original_tmpdir="$MODEL_ROOT"
volume="$MODEL_ROOT/mount"
image="$MODEL_ROOT/owned.dmg"
volume_receipt="$MODEL_ROOT/receipt.plist"
mounted=true
nested_mounted=false
detach_failed=false
if [ "$MODEL_CASE" = attach-partial ] || [ "$MODEL_CASE" = attach-empty ] || [ "$MODEL_CASE" = attach-empty-foreign ]; then
  if [ "$MODEL_ELEVATED" = true ]; then
    nested_mounted=true; mounted=false
    nested="$volume"; nested_image="$image"; nested_receipt="$volume_receipt"
  fi
  trap 'original=$?; if cleanup; then cleanup_code=0; else cleanup_code=$?; fi; printf "%s %s %s\n" "$original" "$detach_failed" "$cleanup_code" > "$MODEL_AUDIT/result"; exit "$original"' EXIT
  attach_native_volume "$volume" "$MODEL_ELEVATED" "$image" "$volume_receipt"
  exit 92
fi
attach_native_volume "$volume" "$MODEL_ELEVATED" "$image" "$volume_receipt"
if [ "$MODEL_CASE" = changed-image ] || [ "$MODEL_CASE" = changed-image-first-success ]; then
  echo replacement > "$MODEL_ROOT/replacement"
  mv "$MODEL_ROOT/replacement" "$image"
fi
if detach_native_volume "$volume" "$MODEL_ELEVATED" "$image" "$volume_receipt"; then
  result=0
else
  result=$?
  # The real EXIT cleanup must respect its latch and never issue another eject.
  if cleanup; then exit 91; fi
  test -d "$fixture"
fi
printf '%s %s\n' "$result" "$detach_failed" > "$MODEL_AUDIT/result"
'''
                result = subprocess.run(
                    ["bash", "-c", script], text=True, capture_output=True, timeout=20,
                    env={**os.environ, "MODEL_ROOT": str(directory), "MODEL_CASE": case,
                         "MODEL_ELEVATED": elevated, "MODEL_AUDIT": str(audit)},
                )
                attach_failure = case.startswith("attach-")
                assert result.returncode == (25 if attach_failure else 0), (case, result.stdout, result.stderr)
                report = (audit / "result").read_text().split()
                status, latch = report[:2]
                calls = (audit / "calls").read_text().splitlines() if (audit / "calls").exists() else []
                assert int(status) == expected_status, (case, status, result.stderr)
                assert len(calls) == expected_calls, (case, calls)
                assert not calls or calls[0] == ("/dev/disk5" if case == "attach-partial" else str(mountpoint)), (case, calls)
                assert len(calls) <= 1 or calls[1] == "/dev/disk5", (case, calls)
                assert all(call == "/dev/disk5" for call in calls[1:]), (case, calls)
                failed_cleanup = case not in ("ordinary", "already-detached", "partial-unmount", "attach-partial", "busy-then-quiescent")
                assert latch == ("true" if failed_cleanup else "false"), (case, latch)
                assert image.exists() == (case != "attach-partial"), case
                if attach_failure:
                    assert int(report[2]) == int(failed_cleanup), (case, report)
    print(json.dumps({"native_volume_cleanup_controls": len(controls) * 2,
                      "packaged_contract_routing_controls": routing_controls, "status": "passed"}))


if __name__ == "__main__":
    main()
