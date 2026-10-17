#!/usr/bin/env python3
"""Observe test-only Finder destination availability and process resources.

Start before the user pastes. Hashing occurs after the availability timestamp;
report callback-to-ready using the separate File Provider log, not monitor uptime.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("destination", type=Path)
p.add_argument("output", type=Path)
p.add_argument("--agent-pid", type=int, required=True)
args = p.parse_args()
args.output.mkdir(parents=True, exist_ok=False)
expected = {v["size"]: v["sha256"] for v in json.loads((Path(__file__).resolve().parents[2] / "tests/protocol/vectors.json").read_text())["hashes"]}
samples = []
result = None
for _ in range(3000):
    now = time.time()
    raw = subprocess.check_output(["ps", "-axo", "pid,rss,time,comm"], text=True)
    rows = [line.strip() for line in raw.splitlines() if line.strip().startswith(str(args.agent_pid) + " ") or "RustboardProvider.appex/Contents/MacOS/RustboardProvider" in line]
    files = list(args.destination.glob("*.bin"))
    sizes = [dict(name=f.name, size=f.stat().st_size) for f in files]
    samples.append(dict(time=now, processes=rows, files=sizes))
    if sorted(f["size"] for f in sizes) == sorted(expected):
        # Do not call preallocation or an open Finder destination "ready".
        opened = subprocess.run(["/usr/sbin/lsof", "-F", "p", "--", *map(str, files)], capture_output=True, text=True)
        if opened.returncode != 1 or opened.stdout.strip():
            time.sleep(0.1)
            continue
        ready = time.time()
        hashes = []
        for f in files:
            h = hashlib.sha256()
            with f.open("rb") as source:
                while block := source.read(1048576):
                    h.update(block)
            hashes.append(dict(name=f.name, size=f.stat().st_size, sha256=h.hexdigest(), match=h.hexdigest() == expected[f.stat().st_size]))
        if all(f["match"] for f in hashes):
            result = dict(all_files_observed_ready=ready, hashes_completed=time.time(), files=hashes)
            break
    time.sleep(0.1)
(args.output / "samples.json").write_text(json.dumps(samples))
(args.output / "result.json").write_text(json.dumps(result, indent=2))
print(json.dumps(result), flush=True)
if result is None:
    raise SystemExit("Finder copy did not finish")
