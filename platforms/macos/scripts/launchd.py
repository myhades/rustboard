import plistlib,sys,os
from pathlib import Path
base,app,destination=map(Path,sys.argv[1:])
value={'Label':'org.rustboard.agent','ProgramArguments':[str(base/'bin/rustboardd')],
       'EnvironmentVariables':{'RUSTBOARD_HOST':str(app/'Contents/MacOS/Rustboard')},
       'ExitTimeOut':35,'RunAtLoad':True,'KeepAlive':True,'ThrottleInterval':5,'LimitLoadToSessionType':'Aqua',
       'ProcessType':'Interactive','Umask':63,
       'StandardOutPath':str(base/'agent.out.log'),'StandardErrorPath':str(base/'agent.err.log')}
destination.parent.mkdir(parents=True,exist_ok=True)
destination.write_bytes(plistlib.dumps(value));os.chmod(destination,0o600)
