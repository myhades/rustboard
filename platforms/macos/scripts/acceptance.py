#!/usr/bin/env python3
"""Prepare an isolated real TLS peer. Finder Cmd+V remains a native user/UI action.
Never labels loopback as cross-machine or a byte-copy as Finder acceptance.
"""
import argparse,json,os,subprocess,time,signal
from pathlib import Path
root=Path(__file__).resolve().parents[3]
p=argparse.ArgumentParser();p.add_argument('action',choices=['prepare','status','files','text','evict','stop']);p.add_argument('--sizes',default='4096,67108864,1073741824');p.add_argument('--ttl-ms',type=int,default=600000);a=p.parse_args()
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
            try:status=control('status','--json');break
            except subprocess.CalledProcessError:time.sleep(.1)
        else:raise RuntimeError('Agent did not start')
        for name in ['command.txt','peer-pin.txt']:(out/name).unlink(missing_ok=True)
        with (out/'peer.txt').open('w') as log:
            peer=subprocess.Popen([str(root/'target/release/examples/macos_acceptance_peer'),status['fingerprint'],'45441','45442',str(out)],env=dict(env,RUSTBOARD_TEST_SIZES=a.sizes),stdout=log,stderr=log,start_new_session=True)
        for attempt in range(50):
            if (out/'peer-pin.txt').exists():break
            if peer.poll() is not None:raise RuntimeError('Peer exited')
            time.sleep(.1)
        pin=(out/'peer-pin.txt').read_text().strip()
        # The synthetic peer has no interactive enrollment UI. Configure only this
        # isolated profile offline; real-device enrollment is tested separately.
        agent.terminate();agent.wait(timeout=10)
        team=next(line.split('=',1)[1] for line in info.splitlines() if line.startswith('TeamIdentifier='))
        config=Path.home()/f'Library/Group Containers/{team}.org.rustboard.shared/acceptance/config.json'
        value=json.loads(config.read_text());value['peers']=[dict(address='127.0.0.1:45442',fingerprint=pin)]
        temporary=config.with_suffix('.test.tmp');temporary.write_text(json.dumps(value));temporary.chmod(0o600);temporary.replace(config)
        with (out/'agent.txt').open('a') as log:
            agent=subprocess.Popen([str(base/'bin/rustboardd'),'--listen','127.0.0.1:45441'],env=env,stdout=log,stderr=log,start_new_session=True)
        for attempt in range(50):
            try:control('status','--json');break
            except subprocess.CalledProcessError:time.sleep(.1)
        else:raise RuntimeError('Configured Agent did not restart')
        (out/'session.json').write_text(json.dumps({'agent_pid':agent.pid,'peer_pid':peer.pid,'profile':'acceptance','agent_fingerprint':status['fingerprint'],'peer_fingerprint':pin},indent=2))
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
elif a.action=='evict':subprocess.run([str(host),'revoke'],env=env,check=True)
elif a.action=='status':
    print(json.dumps(control('status','--json'),indent=2))
    print((out/'reads.json').read_text() if (out/'reads.json').exists() else 'No source metrics yet')
else:
    state=json.loads((out/'session.json').read_text());pid=state['agent_pid']
    expected=str(base/'bin/rustboardd')+' --listen 127.0.0.1:45441'
    actual=subprocess.check_output(['/bin/ps','-p',str(pid),'-o','command='],text=True).strip()
    if actual!=expected:raise RuntimeError('Recorded PID is not the isolated acceptance Agent; refusing stop')
    os.kill(pid,signal.SIGTERM);write_command('stop')
    subprocess.run([str(host),'remove'],env=env,check=True)
    print('Stopped acceptance profile and removed only its domain; retained evidence and identity.')
