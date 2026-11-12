#!/bin/bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This release script requires macOS."
  exit 1
fi

# Use a stable Apple Development identity so macOS can recognize JadX across
# rebuilds. Pass only the certificate hash to avoid exposing account details.
signing_identity=$(
  /usr/bin/security find-identity -v -p codesigning 2>/dev/null |
    /usr/bin/awk -F '"' '/Apple Development:/ { split($1, identity, " "); print identity[2]; exit }'
)

if [[ -z "$signing_identity" ]]; then
  echo "No valid Apple Development signing identity was found in the login keychain."
  echo "Create one in Xcode > Settings > Accounts > Manage Certificates, then retry."
  exit 1
fi

export APPLE_SIGNING_IDENTITY="$signing_identity"
exec ./node_modules/.bin/tauri build "$@"
