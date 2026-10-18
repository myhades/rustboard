#!/bin/bash
# Hosted-runner-only ephemeral keychain. Never print certificate/private-key data.
set -euo pipefail
: "${RUNNER_TEMP:?}"
: "${MACOS_CERTIFICATE_P12_BASE64:?Configure the signing certificate secret}"
: "${MACOS_CERTIFICATE_PASSWORD:?Configure the certificate password secret}"
: "${MACOS_SIGN_IDENTITY:?Configure the certificate SHA-1 fingerprint secret}"
keychain="$RUNNER_TEMP/rustboard-signing.keychain-db"
p12="$RUNNER_TEMP/rustboard-signing.p12"
password=$(openssl rand -hex 32)
echo "::add-mask::$password"
cleanup() {
  security delete-keychain "$keychain" >/dev/null 2>&1 || true
  rm -f "$p12"
}
trap cleanup EXIT
umask 077
printf '%s' "$MACOS_CERTIFICATE_P12_BASE64" | base64 --decode > "$p12"
security create-keychain -p "$password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$password" "$keychain"
security import "$p12" -P "$MACOS_CERTIFICATE_PASSWORD" -k "$keychain" -t cert -f pkcs12 -T /usr/bin/codesign >/dev/null
security set-key-partition-list -S apple-tool:,apple:,codesign: -k "$password" "$keychain" >/dev/null
security list-keychains -d user -s "$keychain" "$HOME/Library/Keychains/login.keychain-db"
export SIGN_IDENTITY="$MACOS_SIGN_IDENTITY"
bash platforms/macos/scripts/sign.sh
# Verify launch of the signed CLI on a fresh runner. This is NOT GUI/File Provider acceptance.
target/macos/bin/rustboard --help
