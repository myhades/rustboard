#!/usr/bin/env python3
"""Package signed artifacts; never fabricate a release URL/checksum or notarization."""
import hashlib,subprocess,tarfile
from pathlib import Path
root=Path(__file__).resolve().parents[3];out=root/'target/macos'
subprocess.run(['codesign','--verify','--deep','--strict',str(out/'Rustboard.app')],check=True)
for name in ['rustboardd','rustboard']:
    subprocess.run(['codesign','--verify','--strict',str(out/'bin'/name)],check=True)
info=subprocess.run(['codesign','-dv',str(out/'bin/rustboardd')],capture_output=True,text=True,check=True).stderr
if 'TeamIdentifier=not set' in info or 'TeamIdentifier=' not in info:raise SystemExit('Signed Apple-team artifacts required')
archive=out/'rustboard-0.1.0-prototype-macos.tar.gz'
with tarfile.open(archive,'w:gz') as tar:
    for name in ['bin','Rustboard.app']:tar.add(out/name,arcname=name)
    for name in ['install.sh','uninstall.sh','launchd.py','restart.sh']:tar.add(root/'platforms/macos/scripts'/name,arcname='platforms/macos/scripts/'+name)
    tar.add(root/'LICENSE',arcname='LICENSE')
    for name in ['INSTALLATION.md','STATUS.md','SECURITY.md']:tar.add(root/'docs'/name,arcname='docs/'+name)
hasher=hashlib.sha256()
with archive.open('rb') as stream:
    for block in iter(lambda: stream.read(1024*1024), b''):hasher.update(block)
digest=hasher.hexdigest()
archive.with_suffix(archive.suffix+'.sha256').write_text(digest+'  '+archive.name+'\n')
formula=(root/'platforms/macos/packaging/rustboard.rb.in').read_text().replace('@URL@',archive.as_uri()).replace('@SHA256@',digest)
(out/'rustboard.rb').write_text(formula)
print(archive);print('SHA256',digest)
