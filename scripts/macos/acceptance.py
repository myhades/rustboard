#!/usr/bin/env python3
"""Prepare an isolated real TLS peer. Finder Cmd+V remains a native user/UI action.
Never labels loopback as cross-machine or a byte-copy as Finder acceptance.
"""
import argparse,json,os,subprocess,time
from pathlib import Path
root=Path(__file__).resolve().parents[2]
p=argparse.ArgumentParser();p.add_argument('action',choices=['prepare','status','files','text','pause','resume','evict','stop']);p.add_argument('--sizes',default='4096,67108864,1073741824');p.add_argument('--ttl-ms',type=int,default=600000);a=p.parse_args()
if not 1<=a.ttl_ms<=600000:p.error('--ttl-ms must be 1..600000')
out=root/'test-output/macos/acceptance';out.mkdir(parents=True,exist_ok=True)
base=root/'target/macos';host=base/'Rustboard.app/Contents/MacOS/Rustboard';cli=base/'bin/rustboard'
env=dict(os.environ,RUSTBOARD_PROFILE='acceptance',RUSTBOARD_HOST=str(host))
def control(*args):return json.loads(subprocess.check_output([str(cli),*args],env=env,stderr=subprocess.PIPE))
def write_command(command):(out/'command.txt').write_text(command+'\n')
if a.action=='prepare':
    info=subprocess.run(['codesign','-dv',str(base/'bin/rustboardd')],capture_output=True,text=True,check=True).stderr
    if 'TeamIdentifier=' not in info or 'TeamIdentifier=not set' in info:raise SystemExit('Run sign.sh in your GUI terminal first')
    subprocess.run(['pluginkit','-a',str(base/'Rustboard.app/Contents/PlugIns/RustboardProvider.appex')],check=True)
    # LaunchServices registration is asynchronous after a signed bundle update.
    for attempt in range(30):
        registration=subprocess.run([str(host),'register'],env=env,capture_output=True,text=True)
        if registration.returncode==0:break
        time.sleep(.2)
    else:raise RuntimeError('File Provider registration failed: '+registration.stderr.strip())
    # The profile lock rejects a second Agent; ports are loopback-only and fixed for reproducibility.
    with (out/'agent.txt').open('w') as log:
        agent=subprocess.Popen([str(base/'bin/rustboardd'),'--listen','127.0.0.1:45441'],env=env,stdout=log,stderr=log,start_new_session=True)
    try:
        for attempt in range(50):
            if agent.poll() is not None:raise RuntimeError('Agent exited; inspect agent.txt')
            try:doctor=control('doctor');break
            except subprocess.CalledProcessError:time.sleep(.1)
        else:raise RuntimeError('Agent did not start')
        for name in ['command.txt','peer-pin.txt']:(out/name).unlink(missing_ok=True)
        with (out/'peer.txt').open('w') as log:
            peer=subprocess.Popen([str(root/'target/release/examples/macos_acceptance_peer'),doctor['fingerprint'],'45441','45442',str(out)],env=dict(env,RUSTBOARD_TEST_SIZES=a.sizes),stdout=log,stderr=log,start_new_session=True)
        for attempt in range(50):
            if (out/'peer-pin.txt').exists():break
            if peer.poll() is not None:raise RuntimeError('Peer exited')
            time.sleep(.1)
        pin=(out/'peer-pin.txt').read_text().strip()
        control('pair','127.0.0.1:45442','--fingerprint',pin)
        (out/'session.json').write_text(json.dumps({'agent_pid':agent.pid,'peer_pid':peer.pid,'profile':'acceptance','agent_fingerprint':doctor['fingerprint'],'peer_fingerprint':pin},indent=2))
        (out/'sizes.txt').write_text(a.sizes)
        (out/'ttl-ms.txt').write_text(str(a.ttl_ms))
        write_command('files')
        print('Prepared synthetic loopback; NOT cross-machine. Inspect reads.json and dataless stat before real Finder Cmd+V.')
    except BaseException:
        agent.terminate();raise
elif a.action in ['files','text']:
    if a.action=='files':
        (out/'sizes.txt').write_text(a.sizes)
        (out/'ttl-ms.txt').write_text(str(a.ttl_ms))
    write_command(a.action)
elif a.action in ['pause','resume']:print(control(a.action))
elif a.action=='evict':subprocess.run([str(host),'revoke'],env=env,check=True)
elif a.action=='status':
    print(json.dumps(control('status'),indent=2))
    print((out/'reads.json').read_text() if (out/'reads.json').exists() else 'No source metrics yet')
else:
    if (out/'peer-pin.txt').exists():control('unpair',(out/'peer-pin.txt').read_text().strip())
    control('stop');write_command('stop')
    subprocess.run([str(host),'remove'],env=env,check=True)
    print('Stopped acceptance profile and removed only its domain; retained evidence and identity.')
