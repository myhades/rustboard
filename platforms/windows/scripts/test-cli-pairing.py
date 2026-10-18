import os,subprocess,json,time,uuid,socket,pathlib,shutil
root=pathlib.Path(__file__).resolve().parents[3]; cli=root/'target/release/rustboard.exe'; agent=root/'target/release/rustboardd.exe'
profiles=[]; agents=[]; clients=[]
def run(i,*args):
 return subprocess.run([str(cli),*args],env=profiles[i][1],capture_output=True,text=True,timeout=15)
def start_pair(i,target=None,answer='y'):
 p=subprocess.Popen([str(cli),'pair',*([target] if target else [])],env=profiles[i][1],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True);clients.append(p)
 p.stdin.write(answer+'\n');p.stdin.flush();return p
def result(p):
 p.stdin.close();p.stdin=None
 out=p.communicate(timeout=20)[0];print(out);return p.returncode,out
def peers(i):return json.loads((profiles[i][2]/'config.json').read_text())['peers']
try:
 for i,port in enumerate([48920,48924]):
  name='cli-'+uuid.uuid4().hex[:12];env=dict(os.environ,RUSTBOARD_PROFILE=name);path=pathlib.Path(env['LOCALAPPDATA'])/'Rustboard'/name;path.mkdir();profiles.append((port,env,path));(path/'config.json').write_text(json.dumps({'schema':2,'identity':None,'peers':[],'listen':f'127.0.0.1:{port}','mode':'text'}));agents.append(subprocess.Popen([str(agent)],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL))
  for _ in range(50):
   r=run(i,'status','--json')
   if r.returncode==0:break
   time.sleep(.1)
  assert r.returncode==0,r.stderr
 # Decline must not persist on either side.
 a=start_pair(0,answer='n');time.sleep(.7);b=start_pair(1,'127.0.0.1:48920');ra=result(a);rb=result(b)
 assert ra[0]!=0 and rb[0]!=0 and not peers(0) and not peers(1)
 time.sleep(.7)
 # Both confirm and save; active peers remain unchanged until restart.
 a=start_pair(0);time.sleep(.7);b=start_pair(1,'127.0.0.1:48920');ra=result(a);rb=result(b)
 assert ra[0]==0 and rb[0]==0
 for i in range(2):
  assert len(peers(i))==1
  active=json.loads(run(i,'status','--json').stdout);assert active['peers']==[] and active['pending_restart']
 # Address-based unpair, unknown address rejected.
 assert run(0,'unpair','127.0.0.1:48924').returncode==0
 assert not peers(0)
 assert run(0,'unpair','127.0.0.1:48924').returncode!=0
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
