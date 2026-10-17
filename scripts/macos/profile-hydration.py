#!/usr/bin/env python3
"""Isolated Core TLS -> diagnostic Unix IPC -> production Swift hydrate benchmark.

No clipboard, File Provider domain or production Agent authentication is involved.
Pass compiled Core throughput and SocketHydration binaries, vectors and a NEW output dir.
"""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import time

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("core", type=Path)
p.add_argument("swift", type=Path)
p.add_argument("vectors", type=Path)
p.add_argument("output", type=Path)
p.add_argument("--sizes", nargs="+", type=int, default=[4096, 67108864, 1073741824])
p.add_argument("--generic", action="store_true")
args = p.parse_args()
args.output.mkdir(parents=True, exist_ok=False)
hashes = {v["size"]: v["sha256"] for v in json.loads(args.vectors.read_text())["hashes"]}
for size in args.sizes:
    assert size in hashes
    with tempfile.TemporaryDirectory(prefix="rbperf-") as temp:
        sock = Path(temp) / "ipc.sock"
        dest = Path(temp) / "hydrated.bin"
        with (args.output / f"{size}-server.jsonl").open("w") as server_out, \
             (args.output / f"{size}-server-resources.txt").open("w") as server_err:
            server = subprocess.Popen(["/usr/bin/time", "-l", str(args.core.resolve()), str(size), str(sock)], stdout=server_out, stderr=server_err)
            try:
                deadline = time.monotonic() + 20
                while not sock.exists():
                    if server.poll() is not None or time.monotonic() > deadline:
                        raise RuntimeError("diagnostic server did not become ready")
                    time.sleep(0.02)
                with (args.output / f"{size}-client-resources.txt").open("w") as resources:
                    result = subprocess.run(["/usr/bin/time", "-l", str(args.swift.resolve()), str(sock), str(dest)] + (["--generic"] if args.generic else []), stdout=subprocess.PIPE, stderr=resources, text=True, timeout=300, check=True)
                report = json.loads(result.stdout)
                assert report["sha256"] == hashes[size], report
                assert dest.stat().st_size == size
                (args.output / f"{size}-client.json").write_text(result.stdout)
                assert server.wait(timeout=15) == 0
                print(result.stdout.strip(), flush=True)
            finally:
                if server.poll() is None:
                    server.terminate()
                    server.wait(timeout=35)
