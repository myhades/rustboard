#!/bin/bash
# Foreground, bounded development signing session. No password storage or ACL changes.
# Start only in the terminal where the user unlocked their keychain. Ctrl-C stops it.
set -euo pipefail
cd "$(dirname "$0")/../../.."
: "${SIGN_IDENTITY:?Set the intended Apple signing identity}"
mkdir -p target/macos/sign-session
last=''
echo 'Rustboard signing session: only platforms/macos/scripts/sign.sh, up to 30 minutes. Ctrl-C stops.'
for ((tick=0; tick<1800; tick++)); do
  request=$(cat target/macos/sign-session/request 2>/dev/null || true)
  if [[ "$request" =~ ^[a-f0-9]{32}$ ]] && [ "$request" != "$last" ]; then
    last="$request"
    set +e
    platforms/macos/scripts/sign.sh > "target/macos/sign-session/$request.log" 2>&1
    result=$?
    set -e
    printf '%s %s\n' "$request" "$result" > target/macos/sign-session/result.new
    mv target/macos/sign-session/result.new target/macos/sign-session/result
    cat "target/macos/sign-session/$request.log"
    printf 'Rustboard signing result=%s\n' "$result"
  fi
  sleep 1
done
