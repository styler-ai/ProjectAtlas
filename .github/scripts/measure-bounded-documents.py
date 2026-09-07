#!/usr/bin/env python3
"""Measure the fixed PDF/DOCX scan case using native process resource counters."""

import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys
import tempfile
import time
import zipfile


MIB = 1024 * 1024
LIMITS = {"elapsed_seconds": 120, "cpu_seconds": 90, "peak_rss_bytes": 768 * MIB,
          "output_bytes": MIB, "database_bytes": 64 * MIB, "io_operations": 2_000_000}


def pdf_bytes(pages=16):
    objects = [b"<< /Type /Catalog /Pages 2 0 R >>", b"", b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"]
    kids = []
    for page in range(pages):
        number = len(objects) + 1
        kids.append(f"{number} 0 R")
        objects.append(f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {number+1} 0 R /Resources << /Font << /F1 3 0 R >> >> >>".encode())
        text = f"BT /F1 12 Tf 72 720 Td (Document resource marker page {page}) Tj ET\n".encode()
        objects.append(f"<< /Length {len(text)} >>\nstream\n".encode() + text + b"endstream")
    objects[1] = f"<< /Type /Pages /Kids [{' '.join(kids)}] /Count {pages} >>".encode()
    result = bytearray(b"%PDF-1.4\n")
    offsets = []
    for index, value in enumerate(objects, 1):
        offsets.append(len(result))
        result.extend(f"{index} 0 obj\n".encode() + value + b"\nendobj\n")
    xref = len(result)
    result.extend(f"xref\n0 {len(objects)+1}\n0000000000 65535 f \n".encode())
    for offset in offsets:
        result.extend(f"{offset:010} 00000 n \n".encode())
    result.extend(f"trailer\n<< /Size {len(objects)+1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
    return result


def fixture(root):
    docs = root / "docs"
    docs.mkdir(parents=True)
    # The fixture lives under the checkout; give it its own repository boundary.
    subprocess.run(["git", "init", "--quiet", str(root)], check=True, timeout=10)
    atlas = root / ".projectatlas"
    atlas.mkdir()
    (atlas / "config.toml").write_text('[project]\nroot = "."\n')
    pdf = pdf_bytes()
    xml = ('<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>'
           + ''.join(f"<w:p><w:r><w:t>Document resource marker paragraph {i}</w:t></w:r></w:p>" for i in range(64))
           + '</w:body></w:document>')
    for index in range(64):
        (docs / f"guide-{index:02}.pdf").write_bytes(pdf)
        with zipfile.ZipFile(docs / f"guide-{index:02}.docx", "w", compression=zipfile.ZIP_DEFLATED) as archive:
            archive.writestr("word/document.xml", xml)
    return sum(path.stat().st_size for path in docs.iterdir())


class WindowsCounters:
    """Retain the owned process handle so counters remain readable after exit."""

    def __init__(self, pid):
        from ctypes import wintypes
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.psapi = ctypes.WinDLL("psapi", use_last_error=True)
        self.kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        self.kernel.OpenProcess.restype = wintypes.HANDLE
        self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        self.kernel.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4

        class Memory(ctypes.Structure):
            _fields_ = [("cb", wintypes.DWORD), ("faults", wintypes.DWORD)] + [
                (name, ctypes.c_size_t) for name in ("peak", "working", "peak_paged", "paged", "peak_nonpaged", "nonpaged", "pagefile", "peak_pagefile")]

        class Io(ctypes.Structure):
            _fields_ = [(name, ctypes.c_ulonglong) for name in ("reads", "writes", "other", "read_bytes", "write_bytes", "other_bytes")]

        self.memory_type, self.io_type, self.time_type = Memory, Io, wintypes.FILETIME
        self.psapi.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(Memory), wintypes.DWORD]
        self.kernel.GetProcessIoCounters.argtypes = [wintypes.HANDLE, ctypes.POINTER(Io)]
        self.handle = self.kernel.OpenProcess(0x0410, False, pid)
        if not self.handle:
            raise ctypes.WinError(ctypes.get_last_error())

    def read(self):
        values = [self.time_type() for _ in range(4)]
        memory, io = self.memory_type(), self.io_type()
        memory.cb = ctypes.sizeof(memory)
        for ok in (self.kernel.GetProcessTimes(self.handle, *(ctypes.byref(value) for value in values)),
                   self.psapi.GetProcessMemoryInfo(self.handle, ctypes.byref(memory), memory.cb),
                   self.kernel.GetProcessIoCounters(self.handle, ctypes.byref(io))):
            if not ok:
                raise ctypes.WinError(ctypes.get_last_error())
        cpu = sum((value.dwHighDateTime << 32) + value.dwLowDateTime for value in values[2:]) / 10_000_000
        return {"cpu_seconds": cpu, "peak_rss_bytes": memory.peak,
                "io_operations": io.reads + io.writes, "io_read_bytes": io.read_bytes,
                "io_write_bytes": io.write_bytes, "io_counter_kind": "windows_process_read_write"}

    def close(self):
        self.kernel.CloseHandle(self.handle)


def measure(binary, root):
    stdout, stderr = root / "scan.stdout", root / "scan.stderr"
    started = time.monotonic()
    with stdout.open("wb") as out, stderr.open("wb") as err:
        process = subprocess.Popen([str(binary), "--format", "json", "--config", str(root / ".projectatlas/config.toml"), "--db", str(root / ".projectatlas/projectatlas.db"), "scan", "."], cwd=root, stdout=out, stderr=err)
        counters = None
        try:
            if os.name == "nt":
                counters = WindowsCounters(process.pid)
            while True:
                if counters:
                    done = process.poll() is not None
                else:
                    pid, status, usage = os.wait4(process.pid, os.WNOHANG)
                    done = pid != 0
                    if done:
                        process.returncode = os.waitstatus_to_exitcode(status)
                if done:
                    break
                if time.monotonic() - started > LIMITS["elapsed_seconds"] or stdout.stat().st_size + stderr.stat().st_size > LIMITS["output_bytes"]:
                    raise RuntimeError("document scan exceeded wall time or captured-output bound")
                time.sleep(0.025)
            if counters:
                result = counters.read()
            else:
                result = {"cpu_seconds": usage.ru_utime + usage.ru_stime,
                          "peak_rss_bytes": usage.ru_maxrss * (1 if sys.platform == "darwin" else 1024),
                          "io_operations": usage.ru_inblock + usage.ru_oublock,
                          "io_input_blocks": usage.ru_inblock, "io_output_blocks": usage.ru_oublock,
                          "io_counter_kind": "posix_rusage_filesystem_blocks"}
            if process.returncode:
                raise RuntimeError(f"document scan exited {process.returncode}: {stderr.read_text(errors='replace')[:2000]}")
        finally:
            if process.returncode is None:
                process.kill()
                process.wait(timeout=10)
            if counters:
                counters.close()
    result.update(elapsed_seconds=time.monotonic() - started,
                  output_bytes=stdout.stat().st_size + stderr.stat().st_size,
                  database_bytes=sum(path.stat().st_size for path in (root / ".projectatlas").glob("projectatlas.db*")))
    scan = json.loads(stdout.read_text())
    if scan["overview"]["files"] != 128 or scan["text_index"]["indexed"] != 128:
        raise RuntimeError("resource fixture did not index exactly its 128 PDF/DOCX files")
    for name, maximum in LIMITS.items():
        if result[name] > maximum:
            raise RuntimeError(f"{name} exceeded {maximum}: {json.dumps(result, sort_keys=True)}")
    if result["peak_rss_bytes"] <= 0 or result["cpu_seconds"] <= 0 or result["database_bytes"] <= 0:
        raise RuntimeError("native process or database counters were missing")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    scratch = Path(__file__).resolve().parents[2] / ".tmp"
    scratch.mkdir(exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="document-resources-", dir=scratch))
    runs = []
    for index in range(3):
        root = work / f"run-{index}"
        input_bytes = fixture(root)
        # Keep process output outside the scanned document tree.
        (root / ".gitignore").write_text(".projectatlas/\n.gitignore\nscan.stdout\nscan.stderr\n")
        row = measure(binary, root)
        row["input_bytes"] = input_bytes
        runs.append(row)
    with binary.open("rb") as stream:
        binary_sha256 = hashlib.file_digest(stream, "sha256").hexdigest()
    report = {"platform": sys.platform, "binary_sha256": binary_sha256,
              "fixture": {"pdf_files": 64, "pages_per_pdf": 16, "docx_files": 64, "paragraphs_per_docx": 64},
              "limits": LIMITS, "runs": runs,
              "medians": {key: statistics.median(row[key] for row in runs) for key in LIMITS},
              "io_note": "Native counter semantics differ by platform; no cross-platform byte-equivalence claim."}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report["medians"], sort_keys=True))


if __name__ == "__main__":
    main()
