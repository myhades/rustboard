import os,subprocess,json,time,uuid,socket,pathlib,shutil
root=pathlib.Path(__file__).resolve().parents[3]; cli=root/'target/release/rustboard.exe'; agent=root/'target/release/rustboardd.exe'
profiles=[]; agents=[]; clients=[]
def run(i,*args):
 return subprocess.run([str(cli),*args],env=profiles[i][1],capture_output=True,text=True,timeout=15)
def start_pair(i,target=None,answer='y'):
 p=subprocess.Popen([str(cli),'pair',*([target] if target else [])],env=profiles[i][1],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True);clients.append(p)
 if answer is not None:p.stdin.write(answer+'\n');p.stdin.flush()
 return p
def result(p):
 p.stdin.close();p.stdin=None
 out=p.communicate(timeout=20)[0];print(out);return p.returncode,out
def peers(i):return json.loads((profiles[i][2]/'config.json').read_text())['peers']
try:
 for i,port in enumerate([48920,48924]):
  name='cli-'+uuid.uuid4().hex[:12];env=dict(os.environ,RUSTBOARD_PROFILE=name);path=pathlib.Path(env['LOCALAPPDATA'])/'Rustboard'/name;path.mkdir();profiles.append((port,env,path));(path/'config.json').write_text(json.dumps({'identity':None,'peers':[],'listen':f'127.0.0.1:{port}','mode':'text'}));(path/'clock.json').write_text(json.dumps({'clock':156 if i==0 else 3,'watermark':None}));agents.append(subprocess.Popen([str(agent)],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL))
  for _ in range(50):
   r=run(i,'status','--json')
   if r.returncode==0:break
   time.sleep(.1)
  assert r.returncode==0,r.stderr
 # Decline must not persist on either side.
 a=start_pair(0,answer='');time.sleep(.7);b=start_pair(1,'127.0.0.1:48920');ra=result(a);rb=result(b)
 assert ra[0]!=0 and rb[0]!=0 and not peers(0) and not peers(1)
 time.sleep(.7)
 # Both confirm and save; active peers remain unchanged until restart.
 a=start_pair(0);time.sleep(.7);b=start_pair(1,'127.0.0.1:48920',answer=None);time.sleep(1.5);b.stdin.write('y\n');b.stdin.flush();ra=result(a);rb=result(b)
 assert 'Confirm on the other peer...' in ra[1] and 'Confirm on the other peer...' not in rb[1]
 assert all(code!=0 and 'Peer added.' in out and 'service commands target installed default profile only' in out for code,out in [ra,rb])
 for i in range(2):
  assert len(peers(i))==1
  assert json.loads((profiles[i][2]/'clock.json').read_text())['clock']==156
  active=json.loads(run(i,'status','--json').stdout);assert active['peers']==[] and active['pending_restart']
 # Unpair persists before restart, which is unavailable for isolated profiles.
 removed=run(0,'unpair','127.0.0.1')
 assert removed.returncode!=0 and 'service commands target installed default profile only' in removed.stderr
 assert not peers(0)
 assert run(0,'unpair','127.0.0.1').returncode!=0
 time.sleep(.7)
 # Abrupt terminal exit closes listening window through heartbeat expiry.
 a=start_pair(0);time.sleep(.7);a.terminate();a.wait(timeout=5);time.sleep(11)
 with socket.socket() as s: assert s.connect_ex(('127.0.0.1',48921))!=0
 print('PASS decline, mutual confirmation, restart-only peers, unpair, terminal-exit expiry')
 (root/'test-output/cli').mkdir(parents=True,exist_ok=True)
 (root/'test-output/cli/pairing-result.json').write_text(json.dumps({'decline_no_save':True,'mutual_confirm_save':True,'pending_restart':True,'address_unpair':True,'terminal_exit_listener_closed':True},indent=2))
finally:
 for p in clients+agents:
  if p.poll() is None:p.terminate();p.wait(timeout=10)
 for _,_,p in profiles:
  assert p.name.startswith('cli-') and p.parent==pathlib.Path(os.environ['LOCALAPPDATA'])/'Rustboard'
  shutil.rmtree(p)
