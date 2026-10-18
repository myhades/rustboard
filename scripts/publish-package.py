#!/usr/bin/env python3
"""Update a release manifest in the owned tap or bucket using scoped GH_TOKEN."""
import base64, json, subprocess, sys
from pathlib import Path
path = Path(sys.argv[1])
target = sys.argv[2]
locations = {"homebrew": "homebrew-tap/contents/Casks/rustboard.rb", "scoop": "scoop-bucket/contents/bucket/rustboard.json"}
endpoint = "repos/myhades/" + locations[target]
result = subprocess.run(["gh", "api", endpoint], capture_output=True, text=True)
body = dict(message="Update Rustboard release", content=base64.b64encode(path.read_bytes()).decode(), branch="main")
if result.returncode == 0:
    existing = json.loads(result.stdout)
    if base64.b64decode(existing["content"]) == path.read_bytes():
        sys.exit(0)
    body["sha"] = existing["sha"]
elif "404" not in result.stderr:
    raise SystemExit(result.stderr)
subprocess.run(["gh", "api", "--method", "PUT", endpoint, "--input", "-"], input=json.dumps(body), text=True, check=True, stdout=subprocess.DEVNULL)
