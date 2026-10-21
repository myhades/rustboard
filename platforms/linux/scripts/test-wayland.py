"""Isolated Mutter/KWin Wayland acceptance; never touches the login clipboard."""
import hashlib,json,os,pathlib,subprocess,sys,tempfile,time

root=pathlib.Path(__file__).resolve().parents[3]
backend=sys.argv[sys.argv.index('--compositor')+1] if '--compositor' in sys.argv else 'mutter'
assert backend in ('mutter','kwin')
if '--session' not in sys.argv:
    out=pathlib.Path(tempfile.mkdtemp(prefix='rb-wayland-'))
    env=dict(os.environ,XDG_RUNTIME_DIR=str(out/'runtime'),XDG_CONFIG_HOME=str(out/'config'),XDG_DATA_HOME=str(out/'data'),XDG_SESSION_TYPE='wayland',WAYLAND_DISPLAY='rustboard-test',RUSTBOARD_PROFILE='acceptance',RUSTBOARD_LOG='debug')
    pathlib.Path(env['XDG_RUNTIME_DIR']).mkdir(mode=0o700)
    env.pop('DISPLAY',None)
    env.update(QT_QPA_PLATFORM='offscreen',LIBGL_ALWAYS_SOFTWARE='1')
    raise SystemExit(subprocess.call(['dbus-run-session','--',sys.executable,__file__,'--compositor',backend,'--session',str(out)],env=env))

out=pathlib.Path(sys.argv[-1]);print('Artifacts:',out,flush=True)
from gi.repository import Gio,GLib
procs=[]
def spawn(args,**kw):
    p=subprocess.Popen(args,**kw);procs.append(p);return p
def wait(fn,seconds=15):
    end=time.monotonic()+seconds
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (FileNotFoundError,json.JSONDecodeError,GLib.Error):pass
        time.sleep(.05)
    raise AssertionError('timeout')
def read(p):return json.loads(p.read_text())
def get(mime):
    return subprocess.run(['wl-paste','--no-newline','--type',mime],capture_output=True,timeout=5)
def publish(mime,body):
    p=spawn(['wl-copy','--foreground','--type',mime],stdin=subprocess.PIPE,stdout=subprocess.DEVNULL,stderr=open(out/'wl-copy.log','ab'))
    p.stdin.write(body);p.stdin.close()
    wait(lambda:get(mime).stdout==body)
    return p
cli=root/'target/release/rustboard';agent=root/'target/release/rustboardd'
profile=out/'config/rustboard/acceptance';profile.mkdir(parents=True)
config=profile/'config.json';config.write_text(json.dumps(dict(listen='127.0.0.1:48920',mode='all',identity=None,peers=[])))
def start():
    p=spawn([str(agent)],stdout=open(out/'agent.log','ab'),stderr=subprocess.STDOUT)
    wait(lambda:subprocess.run([str(cli),'status','--json'],capture_output=True).returncode==0)
    return p
def dolphin_window(directory):
    import xml.etree.ElementTree as ET
    c=Gio.bus_get_sync(Gio.BusType.SESSION,None)
    def windows():
        names=c.call_sync('org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus','ListNames',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0]
        found=set()
        def visit(bus,path):
            xml=c.call_sync(bus,path,'org.freedesktop.DBus.Introspectable','Introspect',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0]
            node=ET.fromstring(xml)
            if any(i.get('name')=='org.kde.KMainWindow' for i in node.findall('interface')):found.add((bus,path))
            for child in node.findall('node'):visit(bus,path.rstrip('/')+'/'+child.get('name'))
        for bus in names:
            if bus.startswith('org.kde.dolphin'):visit(bus,'/')
        return found
    before=windows()
    spawn(['dolphin','--new-window',str(directory)],env=dict(os.environ,QT_QPA_PLATFORM='wayland'),stdout=open(out/'dolphin.log','ab'),stderr=subprocess.STDOUT)
    bus,path=next(iter(wait(lambda:windows()-before)))
    focus=out/(directory.name+'-focus.js')
    focus.write_text('const w = workspace.windowList().find(w => w.caption.includes('+json.dumps(directory.name)+')); if (w) workspace.activeWindow = w;')
    script=c.call_sync('org.kde.KWin','/Scripting','org.kde.kwin.Scripting','loadScript',GLib.Variant('(ss)',(str(focus),directory.name)),None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0]
    c.call_sync('org.kde.KWin',f'/Scripting/Script{script}','org.kde.kwin.Script','run',None,None,Gio.DBusCallFlags.NONE,5000,None)
    actions=c.call_sync(bus,path,'org.kde.KMainWindow','actions',None,None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0]
    def action(name):
        assert name in actions,name
        enabled=c.call_sync(bus,path,'org.kde.KMainWindow','actionIsEnabled',GLib.Variant('(s)',(name,)),None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0]
        if not enabled:return False
        return c.call_sync(bus,path,'org.kde.KMainWindow','activateAction',GLib.Variant('(s)',(name,)),None,Gio.DBusCallFlags.NONE,5000,None).unpack()[0]
    return action

try:
    if backend=='mutter':
        compositor=spawn(['mutter','--headless','--wayland','--no-x11','--virtual-monitor','800x600','--wayland-display','rustboard-test'],stdout=open(out/'compositor.log','wb'),stderr=subprocess.STDOUT)
        c=Gio.bus_get_sync(Gio.BusType.SESSION,None);bus='org.gnome.Mutter.RemoteDesktop'
        def call(path,iface,method,args=None):
            return c.call_sync(bus,path,iface,method,args,None,Gio.DBusCallFlags.NONE,5000,None)
        session=wait(lambda:call('/org/gnome/Mutter/RemoteDesktop',bus,'CreateSession').unpack()[0])
        # A headless Mutter has no physical keyboard. Supply one for the wl-copy test client.
        call(session,bus+'.Session','Start')
        for pressed in [True,False]:call(session,bus+'.Session','NotifyKeyboardKeycode',GLib.Variant('(ub)',(42,pressed)))
    else:
        compositor=spawn(['kwin_wayland','--virtual','--no-lockscreen','--no-global-shortcuts','--width','800','--height','600','--socket','rustboard-test'],stdout=open(out/'compositor.log','wb'),stderr=subprocess.STDOUT)
        wait(lambda:pathlib.Path(os.environ['XDG_RUNTIME_DIR'],'rustboard-test').exists())
        protocols=subprocess.check_output(['wayland-info'],text=True,timeout=10)
        assert 'ext_data_control_manager_v1' in protocols
        (out/'protocols.txt').write_text(protocols)
    keyring=spawn(['gnome-keyring-daemon','--unlock','--foreground','--components=secrets'],stdin=subprocess.PIPE,stdout=open(out/'keyring.log','wb'),stderr=subprocess.STDOUT)
    keyring.stdin.write(b'\n');keyring.stdin.close()
    wait(lambda:b'true' in subprocess.run(['gdbus','call','--session','--dest','org.freedesktop.DBus','--object-path','/org/freedesktop/DBus','--method','org.freedesktop.DBus.NameHasOwner','org.freedesktop.secrets'],capture_output=True).stdout)
    stale=b'clipboard before Agent startup';publish('text/plain',stale)
    a=start();cfg=read(config);identity=cfg['identity'];pin=hashlib.sha256(__import__('base64').b64decode(identity['certificate_der'])).hexdigest()
    a.terminate();a.wait(timeout=10)
    peerout=out/'peer';peerout.mkdir()
    peer=spawn([str(root/'target/release/examples/clipboard_peer'),pin,'127.0.0.1:48920','127.0.0.1:48924',str(peerout)],stdout=open(out/'peer.log','wb'),stderr=subprocess.STDOUT)
    wait((peerout/'peer-pin.txt').exists)
    cfg['peers']=[dict(address='127.0.0.1:48924',fingerprint=(peerout/'peer-pin.txt').read_text())];config.write_text(json.dumps(cfg));a=start()
    time.sleep(.5);assert not (peerout/'received.json').exists()
    print('PASS startup ignores existing clipboard',flush=True)
    command=peerout/'command.txt';command.write_text('text')
    expected='Rustboard Linux Unicode 验证 🦀'.encode();wait(lambda:get('text/plain').stdout==expected)
    print('PASS remote Unicode text',flush=True)
    for body in ['Wayland 本地 🐧'.encode(),b'x'*200000]:
        publish('text/plain;charset=utf-8',body)
        wait(lambda:read(peerout/'received.json')['content'].get('text')==body.decode())
    (peerout/'text.txt').write_text('y'*200000);command.write_text('text\n');wait(lambda:get('text/plain').stdout==b'y'*200000)
    print('PASS native text capture and large pipe transfers in both directions',flush=True)
    command.write_text('files')
    uris=wait(lambda:(r.stdout.decode().strip().splitlines() if (r:=get('text/uri-list')).returncode==0 else None))
    from urllib.parse import unquote,urlsplit
    paths=[pathlib.Path(unquote(urlsplit(u).path)) for u in uris]
    assert [p.stat().st_size for p in paths]==[4096,8192]
    assert read(peerout/'reads.json')==[0,0]
    handles=[open(p,'rb',buffering=0) for p in paths];prefixes=[h.read(512) for h in handles]
    publish('text/plain',b'replacement during file reads')
    wait(lambda:read(peerout/'received.json')['content'].get('text')=='replacement during file reads')
    for h,prefix,size in zip(handles,prefixes,[4096,8192]):
        assert prefix+h.read(2*1024*1024)==bytes(i%251 for i in range(size))
        h.seek(size+1);assert h.read(2*1024*1024)==b'';h.close()
    wait(lambda:not paths[0].exists())
    print('PASS lazy files, active reads across clipboard changes and cleanup',flush=True)
    local=out/'local file 中文.bin';local.write_bytes(bytes(range(256))*16)
    publish('text/uri-list',(local.as_uri()+'\r\n').encode())
    received=wait(lambda:(v if (v:=read(peerout/'received.json'))['content'].get('files') else None))
    target=peerout/('received-'+received['content']['files'][0]['file_id']);wait(target.exists)
    assert target.read_bytes()==local.read_bytes()
    print('PASS native file capture',flush=True)
    import base64
    png=base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aXioAAAAASUVORK5CYII=')
    old=received['offer_id'];publish('image/png',png)
    received=wait(lambda:(v if (v:=read(peerout/'received.json'))['offer_id']!=old and v['content'].get('image') else None))
    target=peerout/('received-'+received['content']['image']['file_id']);wait(target.exists);assert target.read_bytes()==png
    print('PASS native PNG capture',flush=True)
    rich=dict(kind='text',text='Rich text 中文',html='<b>Rich text 中文</b>',rtf=__import__('base64').b64encode(b'{\\rtf1 Rich text}').decode())
    (peerout/'content.json').write_text(json.dumps(rich));command.write_text('content')
    wait(lambda:get('text/html').stdout==rich['html'].encode());assert get('text/rtf').stdout==b'{\\rtf1 Rich text}'
    print('PASS native HTML and RTF publication',flush=True)
    (peerout/'image.png').write_bytes(png);command.write_text('image')
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
    if backend=='kwin':
        destination=out/'dolphin-destination';destination.mkdir();paste_action=dolphin_window(destination)
        command.write_text('files\n')
        wait(lambda:get('text/uri-list').returncode==0 and read(peerout/'published.json')['offer_id'] in get('text/uri-list').stdout.decode())
        assert read(peerout/'reads.json')==[0,0]
        wait(lambda:paste_action('edit_paste'))
        for size in [4096,8192]:
            path=destination/f'sample-{size}.bin'
            wait(lambda:path.exists() and path.stat().st_size==size)
            assert path.read_bytes()==bytes(i%251 for i in range(size))
        print('PASS Dolphin Paste action and destination contents',flush=True)
    publish('text/plain',b'keep user clipboard on exit');time.sleep(.2)
    a.terminate();a.wait(timeout=10)
    assert get('text/plain').stdout==b'keep user clipboard on exit'
    assert not os.path.ismount(profile/'files')
    print('PASS exit keeps another owner and unmounts files',flush=True)
    a=start();assert read(config)['identity']==identity
    compositor.terminate();compositor.wait(timeout=10);a.wait(timeout=10)
    assert not os.path.ismount(profile/'files')
    print('PASS compositor exit stops Agent and unmounts files',flush=True)
    report=root/f'test-output/linux/wayland-{backend}.json';report.parent.mkdir(parents=True,exist_ok=True)
    report.write_text(json.dumps(dict(passed=True,artifacts=str(out),compositor=subprocess.check_output(['mutter' if backend=='mutter' else 'kwin_wayland','--version'],text=True).strip()),indent=2)+'\n')
finally:
    for p in reversed(procs):
        if p.poll() is None:
            p.terminate()
            try:p.wait(timeout=5)
            except subprocess.TimeoutExpired:p.kill();p.wait()
