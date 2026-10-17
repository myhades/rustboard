#!/usr/bin/env python3
"""Package signed artifacts; never fabricate a release URL/checksum or notarization."""
import hashlib,subprocess,tarfile
from pathlib import Path
root=Path(__file__).resolve().parents[2];out=root/'target/macos'
subprocess.run(['codesign','--verify','--deep','--strict',str(out/'Rustboard.app')],check=True)
info=subprocess.run(['codesign','-dv',str(out/'bin/rustboardd')],capture_output=True,text=True,check=True).stderr
if 'TeamIdentifier=not set' in info or 'TeamIdentifier=' not in info:raise SystemExit('Signed Apple-team artifacts required')
archive=out/'rustboard-0.1.0-macos.tar.gz'
with tarfile.open(archive,'w:gz') as tar:
    for name in ['bin','Rustboard.app']:tar.add(out/name,arcname=name)
digest=hashlib.sha256(archive.read_bytes()).hexdigest()
formula=(root/'packaging/macos/rustboard.rb.in').read_text().replace('@URL@',archive.as_uri()).replace('@SHA256@',digest)
(out/'rustboard.rb').write_text(formula)
print(archive);print('SHA256',digest)
