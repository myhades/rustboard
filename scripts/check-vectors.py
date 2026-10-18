"""Portable independent protocol and content test vectors; Python 3 standard library only."""
import argparse
import hashlib
import json
import pathlib
import struct

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--full-hashes', action='store_true', help='also hash the 64 MiB and 1 GiB synthetic vectors')
args = parser.parse_args()
root = pathlib.Path(__file__).resolve().parent.parent
vectors = json.loads((root / 'tests/protocol/vectors.json').read_text(encoding='utf-8'))
payload = json.dumps(vectors['offer'], ensure_ascii=False, separators=(',', ':')).encode()
frame = struct.pack('>I', len(payload)) + payload
assert struct.unpack('>I', frame[:4])[0] == len(frame) - 4
assert json.loads(frame[4:]) == vectors['offer']
assert vectors['data']['bytes'] == [(vectors['read']['offset'] + i) % 251 for i in range(vectors['read']['length'])]
block = bytes(range(251)) * 256
for vector in vectors['hashes']:
    if not args.full_hashes and vector['size'] > 4096:
        continue
    h = hashlib.sha256()
    left = vector['size']
    while left:
        chunk = block[:min(left, len(block))]
        h.update(chunk)
        left -= len(chunk)
    assert h.hexdigest() == vector['sha256']
print('PASS: metadata frame, Unicode JSON, offset data, ' + ('all SHA-256 vectors' if args.full_hashes else '4 KiB SHA-256'))
