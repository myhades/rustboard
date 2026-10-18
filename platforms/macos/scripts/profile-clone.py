#!/usr/bin/env python3
"""Time APFS clonefile primitives independently of TLS, Finder and full-file hashes."""
import ctypes
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import time

vectors = json.loads(Path(sys.argv[1]).read_text())
libc = ctypes.CDLL("/usr/lib/libSystem.B.dylib", use_errno=True)
clone = libc.clonefile
clone.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_int]
clone.restype = ctypes.c_int
block = bytes(range(251)) * 256
with tempfile.TemporaryDirectory(prefix="rustboard-clone-") as folder:
    for item in vectors["hashes"]:
        size = item["size"]
        source = Path(folder) / f"source-{size}"
        snapshot = Path(folder) / f"snapshot-{size}"
        delivered = Path(folder) / f"delivered-{size}"
        # Fully materialize input outside the clone timer, never use a sparse file.
        with source.open("xb") as stream:
            remaining = size
            while remaining:
                chunk = block[:min(remaining, len(block))]
                stream.write(chunk)
                remaining -= len(chunk)
            stream.flush()
            os.fsync(stream.fileno())
        times = []
        for src, dest in [(source, snapshot), (snapshot, delivered)]:
            start = time.perf_counter()
            result = clone(os.fsencode(src), os.fsencode(dest), 0)
            seconds = time.perf_counter() - start
            if result != 0:
                raise OSError(ctypes.get_errno(), "clonefile failed; no copy fallback")
            times.append(seconds)
        # Source mutation must not change either clone; verification is not timed.
        with source.open("r+b") as stream:
            stream.write(b"changed")
        for path in [snapshot, delivered]:
            digest = hashlib.sha256()
            with path.open("rb") as stream:
                while chunk := stream.read(65536):
                    digest.update(chunk)
            assert path.stat().st_size == size and digest.hexdigest() == item["sha256"]
        print(json.dumps({"bytes": size, "snapshot_clone_seconds": times[0], "delivery_clone_seconds": times[1], "cow_hashes_verified": True,
                          "scope": "clonefile primitives, not File Provider delivery; no hash/fsync in clone timer"}), flush=True)
