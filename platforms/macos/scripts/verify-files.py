#!/usr/bin/env python3
import argparse,hashlib,json
from pathlib import Path
root=Path(__file__).resolve().parents[3]
p=argparse.ArgumentParser();p.add_argument('destination',type=Path);p.add_argument('--sizes',default='4096,67108864,1073741824');a=p.parse_args()
v=json.loads((root/'tests/protocol/vectors.json').read_text());expected={h['size']:h['sha256'] for h in v['hashes']}
for size in map(int,a.sizes.split(',')):
    path=a.destination/f'rustboard-tls-{size}.bin'
    if path.stat().st_size!=size:raise SystemExit(f'FAIL size {size}')
    h=hashlib.sha256()
    with path.open('rb') as f:
        while block:=f.read(65536):h.update(block)
    if h.hexdigest()!=expected[size]:raise SystemExit(f'FAIL hash {size}')
    print(json.dumps({'status':'PASS','bytes':size,'sha256':h.hexdigest(),'note':'hash verification only; record Finder action separately'}))
