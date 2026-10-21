"""Small GTK clipboard fixture: get MIME, or set MIME FILE [seconds]."""
import ctypes as c
import pathlib
import sys
import time
import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk, Gdk

if sys.argv[1] == "get":
    clipboard = Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD)
    value = clipboard.wait_for_contents(Gdk.Atom.intern(sys.argv[2], False))
    if value is None:
        raise SystemExit(1)
    sys.stdout.buffer.write(value.get_data())
else:
    gtk = c.CDLL("libgtk-3.so.0")
    gdk = c.CDLL("libgdk-3.so.0")
    gdk.gdk_atom_intern.argtypes = [c.c_char_p, c.c_int]
    gdk.gdk_atom_intern.restype = c.c_void_p
    gtk.gtk_clipboard_get.argtypes = [c.c_void_p]
    gtk.gtk_clipboard_get.restype = c.c_void_p
    gtk.gtk_selection_data_set.argtypes = [c.c_void_p, c.c_void_p, c.c_int, c.c_void_p, c.c_int]
    class Target(c.Structure):
        _fields_ = [("target", c.c_char_p), ("flags", c.c_uint), ("info", c.c_uint)]
    callback_type = c.CFUNCTYPE(None, c.c_void_p, c.c_void_p, c.c_uint, c.c_void_p)
    gtk.gtk_clipboard_set_with_data.argtypes = [c.c_void_p, c.POINTER(Target), c.c_uint, callback_type, c.c_void_p, c.c_void_p]
    if sys.argv[1] == "set-many":
        import json, base64
        values = [(m.encode(), base64.b64decode(b)) for m,b in json.loads(pathlib.Path(sys.argv[2]).read_text()).items()]
        seconds = float(sys.argv[3]) if len(sys.argv)>3 else 30
    else:
        values = [(sys.argv[2].encode(), pathlib.Path(sys.argv[3]).read_bytes())]
        seconds = float(sys.argv[4]) if len(sys.argv)>4 else 30
    assert 0 < len(values) <= 4 and sum(len(data) for _,data in values) <= 4*1024*1024
    buffers = [c.create_string_buffer(data) for _,data in values]
    atoms = [gdk.gdk_atom_intern(mime, 0) for mime,_ in values]
    @callback_type
    def provide(_, selection, info, user):
        gtk.gtk_selection_data_set(selection, atoms[info], 8, buffers[info], len(values[info][1]))
    clipboard = gtk.gtk_clipboard_get(gdk.gdk_atom_intern(b"CLIPBOARD", 0))
    targets = (Target * len(values))(*(Target(mime,0,i) for i,(mime,_) in enumerate(values)))
    assert gtk.gtk_clipboard_set_with_data(clipboard, targets, len(values), provide, None, None)
    print("ready", flush=True)
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        while Gtk.events_pending():
            Gtk.main_iteration_do(False)
        time.sleep(.01)
