#!/usr/bin/env python3
"""Bounded process RSS sampling; resource evidence only, never an acceptance verdict."""
import argparse, json, subprocess, time
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('output',type=Path);p.add_argument('--seconds',type=int,default=600);a=p.parse_args()
peaks={};start=time.monotonic()
while time.monotonic()-start < a.seconds:
    for line in subprocess.check_output(['ps','-axo','rss=,comm='],text=True).splitlines():
        fields=line.strip().split(maxsplit=1)
        if len(fields)!=2:continue
        name=Path(fields[1]).name
        if name in ['rustboardd','RustboardProvider','macos_acceptance_peer']:peaks[name]=max(peaks.get(name,0),int(fields[0])*1024)
    a.output.write_text(json.dumps({'elapsed_seconds':round(time.monotonic()-start,1),'peak_rss_bytes':peaks}))
    time.sleep(1)
