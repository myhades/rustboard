#!/usr/bin/env python3
"""Bounded per-user launchd smoke test; no persistent login item installation."""
import json,os,plistlib,shutil,signal,subprocess,time
from pathlib import Path
root=Path(__file__).resolve().parents[2]
# Caller can select a preserved signed build explicitly; report that choice in evidence.
source=Path(os.environ.get('RUSTBOARD_SIGNED_TEST_BUILD',str(root/'target/macos')))
base=Path.home()/'Library/Application Support/Rustboard/launchd-test'
base.mkdir(parents=True,exist_ok=True);os.chmod(base,0o700)
(base/'bin').mkdir(exist_ok=True)
for name in ['rustboard','rustboardd']:
    src=source/'bin'/name if (source/'bin'/name).exists() else source/name
    shutil.copy2(src,base/'bin'/name)
# Use the signed source host so this test does not register another extension copy.
label='org.rustboard.agent-test';target=f'gui/{os.getuid()}/{label}'
plist=root/'target/macos/launchd-test.plist'
values={'Label':label,'ProgramArguments':[str(base/'bin/rustboardd')],
        'EnvironmentVariables':{'RUSTBOARD_PROFILE':'launchtest','RUSTBOARD_HOST':str(source/'Rustboard.app/Contents/MacOS/Rustboard')},
        'RunAtLoad':True,'KeepAlive':True,'ThrottleInterval':1,'LimitLoadToSessionType':'Aqua','Umask':63,
        'StandardErrorPath':str(base/'stderr.txt'),'StandardOutPath':str(base/'stdout.txt')}
plist.write_bytes(plistlib.dumps(values))
env=dict(os.environ,RUSTBOARD_PROFILE='launchtest')
def doctor():
    p=subprocess.run([str(base/'bin/rustboard'),'status','--json'],capture_output=True,text=True,env=env,timeout=3)
    return json.loads(p.stdout) if p.returncode==0 else None
def wait_service():
    for _ in range(50):
        value=doctor()
        if value and value.get('ok'):return value
        time.sleep(.1)
    raise RuntimeError('launchd IPC not available; inspect dedicated stderr.txt')
subprocess.run(['launchctl','bootstrap',f'gui/{os.getuid()}',str(plist)],check=True)
try:
    first=wait_service()
    before=subprocess.check_output(['launchctl','print',target],text=True)
    subprocess.run(['launchctl','kill','SIGKILL',target],check=True)
    time.sleep(1.2)
    second=wait_service()
    after=subprocess.check_output(['launchctl','print',target],text=True)
    def pid(text):return next(line.strip() for line in text.splitlines() if line.strip().startswith('pid ='))
    assert pid(before)!=pid(after)
    assert first['fingerprint']==second['fingerprint']
    print(json.dumps({'status':'PASS','build':str(source),'before':pid(before),'after':pid(after),'ipc':'signed CLI status before/after','identity_persisted':True,'scope':'same-login process recovery; NOT File Provider fetch, login, or machine reboot'},indent=2))
finally:
    subprocess.run(['launchctl','bootout',target],check=True)
