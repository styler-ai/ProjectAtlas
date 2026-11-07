#!/usr/bin/env python3
"""Prove native image construction and cleanup preserve owned fixture state."""

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
if [ "${PROJECTATLAS_MCP_DIAGNOSTIC_DIR:-}" != "$RUNNER_TEMP" ]; then exit 24; fi
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
                                         "CARGO_BIN_EXE_projectatlas": "unselected-ambient-runtime",
                                         "PROJECTATLAS_MCP_DIAGNOSTIC_DIR": "unselected-fixture-volume"})
            assert result.returncode == expected, (target, platform, behavior, result.stdout, result.stderr)
            arguments = directory / "arguments"
            assert arguments.exists() == (behavior in ("valid", "test-failure")), behavior
            if arguments.exists():
                assert arguments.read_text().splitlines() == ["owning_contract", "--exact", "--nocapture"]
    return len(cases)


def verify_image_construction() -> int:
    """Exercise actual creation/cleanup functions without disk devices or retries of tests."""
    source = CONTRACTS.read_text(encoding="utf-8")
    helpers = source[source.index("attach_native_volume() {"):source.index("trap cleanup EXIT")]
    creator = source[source.index("create_case_image() {"):source.index("for filesystem in APFS APFSX HFS+ HFSX;")]
    # Expected create status, attempts, cleanup status, and retained fixture.
    cases = {
        "success": (0, 1, 0, False),
        "busy-once": (0, 2, 0, False),
        "busy-nested": (0, 2, 0, False),
        "busy-limit": (1, 3, 1, True),
        "other-error": (7, 1, 1, True),
        "busy-other-status": (7, 1, 1, True),
        "near-busy-message": (1, 1, 1, True),
        "existing-output": (1, 0, 1, True),
        "symlink-output": (1, 0, 1, True),
        "dangling-output": (1, 0, 1, True),
        "existing-next-output": (1, 1, 1, True),
        "busy-mapped": (1, 1, 1, True),
        "busy-older-mapped": (1, 2, 1, True),
        "busy-info-error": (1, 1, 1, True),
        "busy-malformed-info": (1, 1, 1, True),
        "busy-unknown-info": (1, 1, 1, True),
        "busy-relative-info": (1, 1, 1, True),
        "cleanup-mapped": (0, 0, 1, True),
        "cleanup-unrelated": (0, 0, 0, False),
        "cleanup-info-error": (0, 0, 17, True),
        "cleanup-malformed-info": (0, 0, 1, True),
        "failed-create-owned-mount": (7, 1, 1, True),
    }
    for case, expected in cases.items():
        with tempfile.TemporaryDirectory(prefix="native-image-create-") as temporary:
            root = Path(temporary)
            fixture, audit = root / "fixture", root / "audit"
            fixture.mkdir()
            audit.mkdir()
            image = fixture / "case image.dmg"
            info = {"images": []}
            if case in ("busy-mapped", "busy-older-mapped", "cleanup-mapped"):
                info = {"images": [{"image-path": str(image)}]}
            elif case == "busy-nested":
                info = {"images": [{"image-path": str(fixture / "outer.dmg")}]}
            elif case == "cleanup-unrelated":
                info = {"images": [{"image-path": str(root / "fixture-other" / "outside.dmg")}]}
            elif case == "busy-unknown-info":
                info = {"images": [{}]}
            elif case == "busy-relative-info":
                info = {"images": [{"image-path": "unresolved.dmg"}]}
            (audit / "info.plist").write_bytes(plistlib.dumps(info))
            (audit / "empty.plist").write_bytes(plistlib.dumps({"images": []}))
            outside = audit / "outside"
            outside.write_bytes(b"outside remains unchanged")
            if case == "existing-output":
                image.write_bytes(b"existing output remains unchanged")
            if case == "symlink-output":
                image.symlink_to(outside)
            if case == "dangling-output":
                image.symlink_to(audit / "missing-outside")
            script = r'''
set -euo pipefail
hdiutil() {
  case "$1" in
    create)
      local count=0 output="${!#}"
      if [ -f "$MODEL_AUDIT/count" ]; then count="$(cat "$MODEL_AUDIT/count")"; fi
      count=$((count+1)); echo "$count" > "$MODEL_AUDIT/count"
      printf '%s\0' "$@" > "$MODEL_AUDIT/args-$count"
      printf 'partial attempt %s\n' "$count" > "$output"
      if [ "$MODEL_CASE" = success ] || { [ "$count" -eq 2 ] && { [ "$MODEL_CASE" = busy-once ] || [ "$MODEL_CASE" = busy-nested ]; }; }; then
        echo "created: $output"; return 0
      fi
      if [ "$MODEL_CASE" = existing-next-output ]; then
        echo existing > "$MODEL_ROOT/case image.attempt-2.dmg"
      fi
      if [ "$MODEL_CASE" = other-error ] || [ "$MODEL_CASE" = failed-create-owned-mount ]; then
        echo 'hdiutil: create failed - Input/output error' >&2; return 7
      fi
      if [ "$MODEL_CASE" = near-busy-message ]; then
        echo 'hdiutil: create failed - Resource busy (different error)' >&2; return 1
      fi
      echo 'hdiutil: create failed - Resource busy' >&2
      if [ "$MODEL_CASE" = busy-other-status ]; then return 7; fi
      return 1 ;;
    info)
      if [ "$MODEL_CASE" = busy-info-error ] || [ "$MODEL_CASE" = cleanup-info-error ]; then return 17; fi
      if [ "$MODEL_CASE" = busy-malformed-info ] || [ "$MODEL_CASE" = cleanup-malformed-info ]; then echo invalid-plist; return; fi
      if [ "$MODEL_CASE" = busy-older-mapped ] && [ "$(cat "$MODEL_AUDIT/count")" -lt 2 ]; then
        cat "$MODEL_AUDIT/empty.plist"
      else cat "$MODEL_AUDIT/info.plist"; fi ;;
    *) return 91 ;;
  esac
}
sleep() { printf '%s\n' "$*" >> "$MODEL_AUDIT/sleeps"; }
'''
            script += helpers + creator
            script += r'''
fixture="$MODEL_ROOT"
original_tmpdir="$MODEL_AUDIT"
mounted=false; nested_mounted=false; detach_failed=false
case "$MODEL_CASE" in
  cleanup-*) result=0; selected="" ;;
  *)
    if selected="$(create_case_image APFSX 'case volume' "$fixture/case image.dmg")"; then
      result=0
      test -f "$selected"
    else result=$?; fi ;;
esac
printf '%s\n' "$result" > "$MODEL_AUDIT/status"
printf '%s\n' "$selected" > "$MODEL_AUDIT/selected"
find "$fixture" -name '*.dmg' -type f -print > "$MODEL_AUDIT/attempt-images"
if [ "$MODEL_CASE" = busy-nested ]; then cp "$MODEL_AUDIT/empty.plist" "$MODEL_AUDIT/info.plist"; fi
if [ "$MODEL_CASE" = failed-create-owned-mount ]; then
  mounted=true; volume="$fixture/mount"; image="$fixture/outer.dmg"; volume_receipt="$fixture/outer.plist"
  detach_native_volume() { echo owned-detach >> "$MODEL_AUDIT/detaches"; }
fi
if cleanup; then cleanup_status=0; else cleanup_status=$?; fi
printf '%s\n' "$cleanup_status" > "$MODEL_AUDIT/cleanup-status"
'''
            result = subprocess.run(["bash", "-c", script], capture_output=True, text=True, timeout=20,
                                    env={**os.environ, "MODEL_ROOT": str(fixture), "MODEL_AUDIT": str(audit), "MODEL_CASE": case})
            assert result.returncode == 0, (case, result.stdout, result.stderr)
            count = int((audit / "count").read_text()) if (audit / "count").exists() else 0
            observed = (int((audit / "status").read_text()), count,
                        int((audit / "cleanup-status").read_text()), fixture.exists())
            assert observed == expected, (case, observed, result.stderr)
            destinations = []
            for attempt in range(1, count + 1):
                arguments = (audit / f"args-{attempt}").read_bytes().decode().split("\0")[:-1]
                assert "-ov" not in arguments and "-force" not in arguments, (case, arguments)
                assert arguments[arguments.index("-fs") + 1] == "Case-sensitive APFS", arguments
                destinations.append(arguments[-1])
            assert len(set(destinations)) == count, (case, destinations)
            if expected[0] == 0 and count:
                assert (audit / "selected").read_text().strip() == destinations[-1], case
                assert len((audit / "attempt-images").read_text().splitlines()) == count, case
            if case == "busy-limit":
                assert (audit / "sleeps").read_text().splitlines() == ["2", "2"], case
            if case == "failed-create-owned-mount":
                assert (audit / "detaches").read_text().splitlines() == ["owned-detach"], case
            if case == "existing-output":
                assert image.read_bytes() == b"existing output remains unchanged"
            assert outside.read_bytes() == b"outside remains unchanged", case
            assert not (audit / "missing-outside").exists(), case
    return len(cases)


def main() -> None:
    """Run the actual workflow functions against isolated disk-command doubles."""
    routing_controls = verify_contract_dispatch()
    creation_controls = verify_image_construction()
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
                      "native_image_construction_controls": creation_controls,
                      "packaged_contract_routing_controls": routing_controls, "status": "passed"}))


if __name__ == "__main__":
    main()
