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
    mime = sys.argv[2].encode()
    data = pathlib.Path(sys.argv[3]).read_bytes()
    assert len(data) <= 262144
    buffer = c.create_string_buffer(data)
    atom = gdk.gdk_atom_intern(mime, 0)
    @callback_type
    def provide(_, selection, info, user):
        gtk.gtk_selection_data_set(selection, atom, 8, buffer, len(data))
    clipboard = gtk.gtk_clipboard_get(gdk.gdk_atom_intern(b"CLIPBOARD", 0))
    target = Target(mime, 0, 0)
    assert gtk.gtk_clipboard_set_with_data(clipboard, c.byref(target), 1, provide, None, None)
    print("ready", flush=True)
    deadline = time.monotonic() + (float(sys.argv[4]) if len(sys.argv) > 4 else 30)
    while time.monotonic() < deadline:
        while Gtk.events_pending():
            Gtk.main_iteration_do(False)
        time.sleep(.01)
