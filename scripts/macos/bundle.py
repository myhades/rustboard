import plistlib, sys
from pathlib import Path
out=Path(sys.argv[1]); app=out/'Rustboard.app'; provider=app/'Contents/PlugIns/RustboardProvider.appex'
for bundle, identifier, executable, kind in [(app,'org.rustboard.host','Rustboard','APPL'),(provider,'org.rustboard.provider','RustboardProvider','XPC!')]:
    info=dict(CFBundleIdentifier=identifier,CFBundleExecutable=executable,CFBundleName=executable,CFBundlePackageType=kind,CFBundleVersion='1',CFBundleShortVersionString='0.1.0',LSMinimumSystemVersion='13.0')
    if kind=='APPL': info['LSUIElement']=True
    else: info['NSExtension']=dict(NSExtensionPointIdentifier='com.apple.fileprovider-nonui',NSExtensionPrincipalClass='RustboardProvider.FileProviderExtension',NSExtensionFileProviderSupportsEnumeration=True)
    (bundle/'Contents/Info.plist').write_bytes(plistlib.dumps(info))
