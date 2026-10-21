"""Opt-in X11 desktop acceptance. Uses small synthetic files and replaces the clipboard."""
import os,pathlib,subprocess,time,json,hashlib,signal,sys
root=pathlib.Path(__file__).resolve().parents[3]
import tempfile
out=pathlib.Path(tempfile.mkdtemp(prefix='rb-x11-'))
print('Artifacts:',out,flush=True)
env=dict(os.environ,DISPLAY=os.environ['DISPLAY'],XDG_RUNTIME_DIR=os.environ['XDG_RUNTIME_DIR'],DBUS_SESSION_BUS_ADDRESS=os.environ['DBUS_SESSION_BUS_ADDRESS'],XDG_CONFIG_HOME=str(out/'config'),RUSTBOARD_PROFILE='acceptance',RUSTBOARD_LOG='debug')
cli=root/'target/release/rustboard';agent=root/'target/release/rustboardd'
profile=out/'config/rustboard/acceptance';profile.mkdir(parents=True,exist_ok=True)
config=profile/'config.json'
if not config.exists():config.write_text(json.dumps(dict(listen='127.0.0.1:48920',mode='all',identity=None,peers=[])))
procs=[]
def wait(fn,seconds=15):
    deadline=time.monotonic()+seconds
    while time.monotonic()<deadline:
        try:
            value=fn()
            if value:return value
        except (FileNotFoundError,json.JSONDecodeError):pass
        time.sleep(.1)
    raise AssertionError('timeout')
def run(*args):return subprocess.run([str(cli),*args],env=env,capture_output=True,text=True)
def start():
    p=subprocess.Popen([str(agent)],env=env,stdout=open(out/'agent-output.log','a'),stderr=subprocess.STDOUT);procs.append(p)
    wait(lambda:run('status','--json').returncode==0)
    return p
def read(path):return json.loads(path.read_text())
def get(mime):return subprocess.run(['python3',str(root/'platforms/linux/scripts/clipboard-probe.py'),'get',mime],env=env,capture_output=True,timeout=10)
def publish(mime,path):
    p=subprocess.Popen(['python3',str(root/'platforms/linux/scripts/clipboard-probe.py'),'set',mime,str(path)],env=env,stdout=subprocess.PIPE,text=True);procs.append(p);assert p.stdout.readline().strip()=='ready';return p
try:
    a=start();cfg=read(config);identity=cfg['identity'];pin=hashlib.sha256(__import__('base64').b64decode(identity['certificate_der'])).hexdigest()
    a.terminate();a.wait(timeout=10)
    peerout=out/('peer-'+str(time.time_ns()));peerout.mkdir(exist_ok=True)
    peer=subprocess.Popen([str(root/'target/release/examples/clipboard_peer'),pin,'127.0.0.1:48920','127.0.0.1:48924',str(peerout)],stdout=open(out/'peer-output.log','w'),stderr=subprocess.STDOUT);procs.append(peer)
    pinfile=peerout/'peer-pin.txt';wait(pinfile.exists)
    cfg['peers']=[dict(address='127.0.0.1:48924',fingerprint=pinfile.read_text())];config.write_text(json.dumps(cfg));a=start()
    command=peerout/'command.txt';command.write_text('text')
    expected='Rustboard Linux Unicode 验证 🦀'.encode();wait(lambda:get('UTF8_STRING').stdout==expected)
    print('PASS remote text',flush=True)
    command.write_text('files');uris=wait(lambda:(r.stdout.decode().strip().splitlines() if (r:=get('text/uri-list')).returncode==0 else None))
    paths=[pathlib.Path(__import__('urllib.parse',fromlist=['unquote']).unquote(u.removeprefix('file://'))) for u in uris]
    assert read(peerout/'reads.json')==[0,0],read(peerout/'reads.json')
    assert [p.stat().st_size for p in paths]==[4096,8192]
    assert read(peerout/'reads.json')==[0,0]
    print('PASS publication and metadata zero reads',flush=True)
    echoed=out/'echoed-uris.txt';echoed.write_text('\r\n'.join(uris)+'\r\n');publish('text/uri-list',echoed)
    time.sleep(.3);assert all(p.exists() for p in paths) and read(peerout/'reads.json')==[0,0]
    print('PASS clipboard ownership handover keeps lazy files',flush=True)
    handles=[open(p,'rb',buffering=0) for p in paths]
    prefixes=[h.read(1024) for h in handles]
    command.write_text('text');wait(lambda:get('UTF8_STRING').stdout==expected)
    for h,prefix,size in zip(handles,prefixes,[4096,8192]):
        body=prefix+h.read();h.close();assert body==bytes(i%251 for i in range(size))
    wait(lambda:not paths[0].exists());print('PASS reads survive replacement, old paths cleaned',flush=True)
    text=out/'text.txt';text.write_text('Linux captured 文本 🐧');p=publish('UTF8_STRING',text)
    wait(lambda:read(peerout/'received.json')['content'].get('text')==text.read_text());print('PASS local text',flush=True)
    # GTK uses INCR for larger selections.
    text.write_text('x'*200000);p.terminate();p.wait();p=publish('UTF8_STRING',text)
    wait(lambda:read(peerout/'received.json')['content'].get('text')==text.read_text());print('PASS INCR capture',flush=True)
    source_dir=out/'source';source_dir.mkdir();(source_dir/'folder/empty').mkdir(parents=True);script=source_dir/'folder/run';script.write_bytes(b'#!/bin/sh\nexit 0\n');script.chmod(0o700);local=source_dir/'local file 中文.bin';local.write_bytes(bytes(range(256))*16)
    subprocess.Popen(['thunar',str(source_dir)],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    source_window=wait(lambda:subprocess.run(['xdotool','search','--onlyvisible','--name','^source -'],env=env,capture_output=True,text=True).stdout.strip()).splitlines()[-1]
    subprocess.run(['xdotool','windowactivate','--sync',source_window],env=env,check=True)
    def copy_source():
        subprocess.run(['xdotool','key','--clearmodifiers','ctrl+a','ctrl+c'],env=env,check=True)
        return local.as_uri().encode() in get('text/uri-list').stdout
    # A visible Thunar window can precede its directory listing becoming ready.
    wait(copy_source)
    offer=wait(lambda:(o if (o:=read(peerout/'received.json'))['content']['kind']=='files' else None))
    received=peerout/('received-'+next(f['file_id'] for f in offer['content']['files'] if f['relative_path']==local.name));wait(received.exists);assert received.read_bytes()==local.read_bytes();assert any(f['directory'] and f['relative_path']=='folder/empty' for f in offer['content']['files']);entry=next(f for f in offer['content']['files'] if f['relative_path']=='folder/run');received=peerout/('received-'+entry['file_id']);wait(received.exists);assert received.read_bytes()==script.read_bytes() and entry['executable'];print('PASS Thunar Ctrl+C folder and file capture',flush=True)
    subprocess.run(['xdotool','windowclose',source_window],env=env,check=True)
    # Native image bytes retain their representation.
    import gi
    gi.require_version('GdkPixbuf','2.0')
    from gi.repository import GdkPixbuf
    image=out/'synthetic.png';pix=GdkPixbuf.Pixbuf.new(GdkPixbuf.Colorspace.RGB,False,8,16,16);pix.fill(0x00ff00ff);pix.savev(str(image),'png',[],[])
    publish('image/png',image)
    offer=wait(lambda:(o if (o:=read(peerout/'received.json'))['content']['kind']=='image' else None))
    received=peerout/('received-'+offer['content']['image']['file_id']);wait(received.exists);assert received.read_bytes()==image.read_bytes();print('PASS native PNG capture',flush=True)
    rich=dict(kind='text',text='Rich text 中文',html='<b>Rich text 中文</b>',rtf=__import__('base64').b64encode(b'{\\rtf1 Rich text}').decode())
    (peerout/'content.json').write_text(json.dumps(rich));command.write_text('content')
    wait(lambda:get('text/html').stdout==rich['html'].encode());assert get('text/rtf').stdout==b'{\\rtf1 Rich text}'
    print('PASS native HTML and RTF publication',flush=True)
    import base64
    rich['text']='Local rich text';rich['html']='<i>Local rich text</i>'
    richfile=out/'formats.json';richfile.write_text(json.dumps({'text/plain':base64.b64encode(rich['text'].encode()).decode(),'text/html':base64.b64encode(rich['html'].encode()).decode(),'text/rtf':rich['rtf']}))
    provider=subprocess.Popen([sys.executable,str(root/'platforms/linux/scripts/clipboard-probe.py'),'set-many',str(richfile),'10'],env=env,stdout=subprocess.PIPE,text=True);procs.append(provider);assert provider.stdout.readline().strip()=='ready'
    # Give the original publication a new plain-text value to avoid echo suppression.
    wait(lambda:read(peerout/'received.json')['content'].get('html')==rich['html'])
    received=read(peerout/'received.json')['content'];assert received['text']==rich['text'] and received['rtf']==rich['rtf']
    print('PASS grouped native HTML, RTF and plain-text capture',flush=True)
    (peerout/'image.png').write_bytes(image.read_bytes());command.write_text('image')
    wait(lambda:get('image/png').stdout==(peerout/'image.png').read_bytes());assert get('text/uri-list').returncode!=0
    print('PASS native PNG publication',flush=True)
    command.write_text('directory')
    uris=wait(lambda:(r.stdout.decode().strip().splitlines() if (r:=get('text/uri-list')).returncode==0 and 'tree' in r.stdout.decode() else None))
    tree=pathlib.Path(__import__('urllib.parse',fromlist=['unquote']).unquote(uris[0].removeprefix('file://')))
    assert (tree/'empty').is_dir() and (tree/'nested/run').stat().st_size==4096
    assert read(peerout/'reads.json')==[0]
    import shutil
    shutil.copytree(tree,out/'copied-tree')
    assert (out/'copied-tree/empty').is_dir() and (out/'copied-tree/nested/run').read_bytes()==bytes(i%251 for i in range(4096))
    assert int((out/'copied-tree/nested/run').stat().st_mtime)==1700000000
    assert (out/'copied-tree/nested/run').stat().st_mode & 0o100
    print('PASS lazy directory copy, empty directory and file attributes',flush=True)
    command.write_text('files');wait(lambda:read(peerout/'published.json')['content']['kind']=='files')
    (peerout/'text.txt').write_text('z'*200000);command.write_text('text');wait(lambda:get('UTF8_STRING').stdout==b'z'*200000);print('PASS INCR publication',flush=True)
    command.write_text('files');wait(lambda:get('text/uri-list').returncode==0)
    destination=out/('thunar-'+str(time.time_ns()));destination.mkdir()
    subprocess.Popen(['thunar',str(destination)],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    window=wait(lambda:subprocess.run(['xdotool','search','--onlyvisible','--name',destination.name],env=env,capture_output=True,text=True).stdout.strip())
    window=window.splitlines()[-1]
    subprocess.run(['xdotool','windowactivate','--sync',window],env=env,check=True)
    subprocess.run(['xdotool','key','--clearmodifiers','ctrl+v'],env=env,check=True)
    for size in [4096,8192]:
        pasted=destination/f'sample-{size}.bin';wait(lambda:pasted.exists() and pasted.stat().st_size==size)
        assert pasted.read_bytes()==bytes(i%251 for i in range(size))
    print('PASS Thunar Ctrl+V and hashes',flush=True)
    subprocess.run(['xdotool','windowclose',window],env=env,check=True)
    a.terminate();a.wait(timeout=10);assert not os.path.ismount(profile/'files');assert read(config)['identity']==identity
    a=start();assert read(config)['identity']==identity;a.kill();a.wait();a=start();assert read(config)['identity']==identity
    print('PASS keyring restart and stale mount recovery',flush=True)
    a.terminate();a.wait(timeout=10)
    result=dict(passed=True,zero_preread=True,replacement=True,text=True,incr_capture=True,incr_publication=True,native_image=True,files=True,thunar_paste=True,keyring=True,crash_cleanup=True)
    report=root/'test-output/linux';report.mkdir(parents=True,exist_ok=True);(report/'desktop.json').write_text(json.dumps(result,indent=2))
finally:
    for p in reversed(procs):
        if p.poll() is None:p.terminate();p.wait(timeout=10)


    # Remove only the identity generated for this disposable test profile.
    if config.exists():
        value=json.loads(config.read_text()).get('identity')
        if value:
            import ctypes as c
            lib=c.CDLL('libsecret-1.so.0')
            lib.secret_schema_new.restype=c.c_void_p
            lib.secret_schema_new.argtypes=[c.c_char_p,c.c_int]
            lib.secret_schema_unref.argtypes=[c.c_void_p]
            lib.secret_password_clear_sync.argtypes=[c.c_void_p,c.c_void_p,c.c_void_p]
            schema=lib.secret_schema_new(b'org.rustboard.Identity',0,b'account',c.c_int(0),c.c_void_p())
            lib.secret_password_clear_sync(schema,None,None,b'account',value['private_key']['account'].encode(),c.c_void_p())
            lib.secret_schema_unref(schema)
