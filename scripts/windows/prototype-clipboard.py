import ctypes as c, struct, sys, time, zlib, hashlib, json
from pathlib import Path
u=c.WinDLL('user32',use_last_error=True); k=c.WinDLL('kernel32',use_last_error=True)
u.CreateWindowExW.argtypes=[c.c_uint,c.c_wchar_p,c.c_wchar_p,c.c_uint,c.c_int,c.c_int,c.c_int,c.c_int,c.c_void_p,c.c_void_p,c.c_void_p,c.c_void_p];u.CreateWindowExW.restype=c.c_void_p
u.OpenClipboard.argtypes=[c.c_void_p];u.SetClipboardData.argtypes=[c.c_uint,c.c_void_p];u.SetClipboardData.restype=c.c_void_p
u.GetClipboardData.argtypes=[c.c_uint];u.GetClipboardData.restype=c.c_void_p
u.RegisterClipboardFormatW.argtypes=[c.c_wchar_p];u.RegisterClipboardFormatW.restype=c.c_uint
k.GlobalAlloc.argtypes=[c.c_uint,c.c_size_t];k.GlobalAlloc.restype=c.c_void_p
k.GlobalLock.argtypes=[c.c_void_p];k.GlobalLock.restype=c.c_void_p
k.GlobalUnlock.argtypes=[c.c_void_p]
u.DestroyWindow.argtypes=[c.c_void_p]
def chunk(t,b):return struct.pack('>I',len(b))+t+b+struct.pack('>I',zlib.crc32(t+b))
# Use a visible 256px fixture; a 2px image is barely visible at Preview's 100% zoom.
size=256
rows=[bytes(sum(([255,0,0,255] if x<size//2 else [0,255,0,255] for x in range(size)),[])) for _ in range(size//2)]
rows += [bytes(sum(([0,0,255,255] if x<size//2 else [255,255,255,255] for x in range(size)),[])) for _ in range(size//2)]
png=b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',size,size,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(b''.join(b'\0'+r for r in rows)))+chunk(b'IEND',b'')
pixels=bytearray(b''.join(reversed(rows)))
for i in range(0,len(pixels),4):pixels[i],pixels[i+2]=pixels[i+2],pixels[i]
dib=struct.pack('<IiiHHIIiiII',40,size,size,1,32,0,len(pixels),0,0,0,0)+pixels
mode=sys.argv[1]
if mode=='fixtures':
    out=Path(sys.argv[2]);out.mkdir(parents=True,exist_ok=True);(out/'synthetic.png').write_bytes(png);(out/'synthetic.bmp').write_bytes(b'BM'+struct.pack('<IHHI',14+len(dib),0,0,54)+dib);sys.exit()
if mode in ('png','dib','text','check-text'):
    hwnd=u.CreateWindowExW(0,'STATIC','Rustboard synthetic fixture',0,0,0,0,0,None,None,None,None)
    assert hwnd
    try:
        for _ in range(100):
            if u.OpenClipboard(hwnd):break
            time.sleep(.02)
        else:raise RuntimeError('OpenClipboard')
        try:
            if mode=='check-text':
                h=u.GetClipboardData(13);assert h;p=k.GlobalLock(h);assert c.wstring_at(p)=='Rustboard TLS Unicode 验证 🦀';k.GlobalUnlock(h)
            else:
                fmt,data={'png':(u.RegisterClipboardFormatW('PNG'),png),'dib':(8,dib),'text':(13,('Rustboard TLS Unicode 验证 🦀\0').encode('utf-16-le'))}[mode]
                h=k.GlobalAlloc(2,len(data));assert h;p=k.GlobalLock(h);c.memmove(p,data,len(data));k.GlobalUnlock(h)
                assert u.EmptyClipboard();assert u.SetClipboardData(fmt,h)
        finally:u.CloseClipboard()
    finally:u.DestroyWindow(hwnd)
    print('PASS synthetic clipboard '+mode)
