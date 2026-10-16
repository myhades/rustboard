"""Verify Explorer outputs independently from Rust (bounded memory)."""
import hashlib
import json
import pathlib
import sys

results = []
for folder in sys.argv[1:]:
    for path in sorted(pathlib.Path(folder).glob('rustboard-*.bin')):
        size = int(path.stem.split('-')[-1])
        expected = hashlib.sha256()
        actual = hashlib.sha256()
        # Multiple of 251 keeps each chunk's pattern aligned.
        block = bytes(range(251)) * 256
        remaining = size
        while remaining:
            chunk = block[:min(len(block), remaining)]
            expected.update(chunk)
            remaining -= len(chunk)
        with path.open('rb') as stream:
            while chunk := stream.read(65536):
                actual.update(chunk)
        passed = path.stat().st_size == size and actual.digest() == expected.digest()
        results.append(dict(file=str(path), size=size, sha256=actual.hexdigest(), expected=expected.hexdigest(), result='PASS' if passed else 'FAIL'))
print(json.dumps(results, indent=2))
sys.exit(0 if results and all(r['result'] == 'PASS' for r in results) else 1)
