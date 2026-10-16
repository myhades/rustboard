"""Run against a test Agent currently offering pattern files copied from Explorer."""
import json
import struct
import sys
import time

pipe_name = sys.argv[1]

def call(request):
    payload = json.dumps({'protocol': 1, 'request': request}, separators=(',', ':')).encode()
    deadline = time.monotonic() + 3
    while True:
        try:
            pipe = open(pipe_name, 'r+b', buffering=0)
            break
        except OSError:
            if time.monotonic() >= deadline:
                raise
            time.sleep(0.02)
    with pipe:
        pipe.write(struct.pack('>I', len(payload)) + payload)
        def exact(n):
            output = bytearray()
            while len(output) < n:
                chunk = pipe.read(n - len(output))
                if not chunk:
                    raise EOFError('Agent disconnected')
                output.extend(chunk)
            return output
        size = struct.unpack('>I', exact(4))[0]
        assert 0 < size <= 1048576
        return json.loads(exact(size))

offer = call({'type': 'get_offer'})['offer']
assert offer['content']['kind'] == 'files'
file = offer['content']['files'][0]
request = dict(type='read', request_id=7, offer_id=offer['offer_id'], file_id=file['file_id'], version=file['version'], offset=249, length=5)
data = call(request)
assert data['ok'] and data['response']['bytes'] == [249, 250, 0, 1, 2]
request['version'] = 'invalid-version'
assert call(request)['ok'] is False
request['length'] = 65537
assert call(request)['ok'] is False
assert call({'type': 'cancel', 'request_id': 7})['ok']
large = max(offer['content']['files'], key=lambda f: f['size'])
if large['size'] >= 65536:
    data = call(dict(type='read', request_id=8, offer_id=offer['offer_id'], file_id=large['file_id'], version=large['version'], offset=0, length=65536))
    assert data['ok'] and data['response']['bytes'] == [i % 251 for i in range(65536)]
print(json.dumps({'test': 'versioned native IPC: offer/read/range/version/cancel', 'result': 'PASS'}))
