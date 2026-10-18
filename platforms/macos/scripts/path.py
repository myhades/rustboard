#!/usr/bin/env python3
"""Manage only Rustboard's marked shell stanza and symlink."""
import os, sys
from pathlib import Path

home = Path.home()
block = '# >>> Rustboard PATH >>>\nexport PATH="$HOME/.local/bin:$PATH"\n# <<< Rustboard PATH <<<\n'
action = sys.argv[1]
if action not in ('add', 'remove'):
    raise SystemExit('Expected add or remove')
for name in ('rustboard',):
    link = home / '.local/bin' / name
    target = home / 'Library/Application Support/Rustboard/bin' / name
    if action == 'add':
        link.parent.mkdir(parents=True, exist_ok=True)
        if link.is_symlink() and os.readlink(link) == str(target):
            pass
        elif link.exists() or link.is_symlink():
            raise SystemExit('Existing user-bin entry belongs to another installation: ' + str(link))
        else:
            link.symlink_to(target)
    elif link.is_symlink() and os.readlink(link) == str(target):
        link.unlink()
for name in ('.zprofile', '.bash_profile'):
    path = home / name
    data = path.read_text() if path.exists() else ''
    if action == 'add':
        if '# >>> Rustboard PATH >>>' not in data:
            path.write_text(data + ('\n' if data and not data.endswith('\n') else '') + block)
    elif block in data:
        path.write_text(data.replace(block, ''))
