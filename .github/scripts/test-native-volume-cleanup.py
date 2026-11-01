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
WORKFLOW = ROOT / ".github/workflows/ci.yml"


def main() -> None:
    """Run the actual workflow functions against isolated disk-command doubles."""
    source = WORKFLOW.read_text(encoding="utf-8")
    start = source.index("          attach_native_volume() {")
    end = source.index("          trap cleanup EXIT", start)
    helpers = textwrap.dedent(source[start:end])
    controls = {
        "ordinary": (0, 1),
        "already-detached": (0, 1),
        "mounted": (1, 1),
        "changed-device": (1, 1),
        "changed-image": (1, 1),
        "invalid-info": (1, 1),
        "mapping-changes-during-poll": (1, 1),
        "partial-unmount": (0, 2),
        "second-eject-fails": (1, 2),
        "attach-partial": (25, 2),
        "attach-empty": (25, 1),
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
                observed = entities if case == "mounted" else unmounted
                if case == "changed-device":
                    observed = [{"dev-entry": "/dev/disk9"}]
                info = {"images": [{"image-path": str(image), "system-entities": observed}]}
                if case in ("already-detached", "attach-empty"):
                    info = {"images": []}
                (directory / "fresh.plist").write_bytes(plistlib.dumps(info))
                (directory / "detached.plist").write_bytes(plistlib.dumps({"images": []}))
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
      if [ "$MODEL_CASE" = attach-empty ]; then return 25; fi
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
      if [ "$MODEL_CASE" = ordinary ]; then return 0; fi
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
if [ "$MODEL_CASE" = attach-partial ] || [ "$MODEL_CASE" = attach-empty ]; then
  if [ "$MODEL_ELEVATED" = true ]; then
    nested_mounted=true; mounted=false
    nested="$volume"; nested_image="$image"; nested_receipt="$volume_receipt"
  fi
  trap 'original=$?; if cleanup; then cleanup_code=0; else cleanup_code=$?; fi; printf "%s %s %s\n" "$original" "$detach_failed" "$cleanup_code" > "$MODEL_AUDIT/result"; exit "$original"' EXIT
  attach_native_volume "$volume" "$MODEL_ELEVATED" "$image" "$volume_receipt"
  exit 92
fi
attach_native_volume "$volume" "$MODEL_ELEVATED" "$image" "$volume_receipt"
if [ "$MODEL_CASE" = changed-image ]; then
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
                calls = (audit / "calls").read_text().splitlines()
                assert int(status) == expected_status, (case, status, result.stderr)
                assert len(calls) == expected_calls, (case, calls)
                assert calls[0] == str(mountpoint), (case, calls)
                assert len(calls) == 1 or calls[1] == "/dev/disk5", (case, calls)
                failed_cleanup = case not in ("ordinary", "already-detached", "partial-unmount", "attach-partial")
                assert latch == ("true" if failed_cleanup else "false"), (case, latch)
                assert image.exists() == (case != "attach-partial"), case
                if attach_failure:
                    assert int(report[2]) == int(failed_cleanup), (case, report)
    print(json.dumps({"native_volume_cleanup_controls": len(controls) * 2, "status": "passed"}))


if __name__ == "__main__":
    main()
