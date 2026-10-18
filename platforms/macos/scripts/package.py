#!/usr/bin/env python3
"""Package signed artifacts; never fabricate a release URL/checksum or notarization."""
import argparse,hashlib,subprocess,tarfile,tomllib
args=argparse.ArgumentParser()
args.add_argument("--release-url", help="HTTPS URL of this exact archive after upload")
options=args.parse_args()
if options.release_url and not options.release_url.startswith("https://"):raise SystemExit("Release URL must be HTTPS")
from pathlib import Path
root=Path(__file__).resolve().parents[3];out=root/'target/macos'
subprocess.run(['codesign','--verify','--deep','--strict',str(out/'Rustboard.app')],check=True)
for name in ['rustboardd','rustboard']:
    subprocess.run(['codesign','--verify','--strict',str(out/'bin'/name)],check=True)
info=subprocess.run(['codesign','-dv',str(out/'bin/rustboardd')],capture_output=True,text=True,check=True).stderr
if 'TeamIdentifier=not set' in info or 'TeamIdentifier=' not in info:raise SystemExit('Signed Apple-team artifacts required')
arch=subprocess.check_output(['lipo','-archs',str(out/'bin/rustboardd')],text=True).strip()
if arch not in ('arm64','x86_64'):raise SystemExit('Expected a single supported architecture')
version=tomllib.loads((root/'Cargo.toml').read_text())['package']['version']
archive=out/f'rustboard-{version}-macos-{arch}.tar.gz'
def executable_scripts(info):
    if info.name.endswith('.sh'):info.mode=0o755
    return info
with tarfile.open(archive,'w:gz') as tar:
    for name in ['bin','Rustboard.app']:tar.add(out/name,arcname=name)
    for name in ['install.sh','uninstall.sh','launchd.py','restart.sh','path.py']:tar.add(root/'platforms/macos/scripts'/name,arcname='platforms/macos/scripts/'+name,filter=executable_scripts)
    tar.add(root/'LICENSE',arcname='LICENSE')
    for name in ['INSTALLATION.md','STATUS.md','SECURITY.md']:tar.add(root/'docs'/name,arcname='docs/'+name)
hasher=hashlib.sha256()
with archive.open('rb') as stream:
    for block in iter(lambda: stream.read(1024*1024), b''):hasher.update(block)
digest=hasher.hexdigest()
archive.with_suffix(archive.suffix+'.sha256').write_text(digest+'  '+archive.name+'\n')
formula=(root/'platforms/macos/packaging/rustboard.rb.in').read_text().replace('@VERSION@',version).replace('@URL@',options.release_url or archive.as_uri()).replace('@SHA256@',digest).replace('@ARCH@','arm64' if arch=='arm64' else 'intel')
tap=out/'homebrew-tap/Casks';tap.mkdir(parents=True,exist_ok=True)
(tap/'rustboard.rb').write_text(formula)
print(archive);print('SHA256',digest)
