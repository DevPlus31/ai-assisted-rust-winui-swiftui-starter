#!/usr/bin/env bash
# Fails when shell code or packaging could reach the network. The Rust side
# is covered by the `[bans]` list in deny.toml (`cargo deny check bans`).
#
# Usage: scripts/check-no-network.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
status=0

fail() {
  echo "error: $1" >&2
  status=1
}

# Swift networking APIs in hand-written code (generated bindings excluded).
swift_hits="$(grep -rnE '\bURLSession\b|^import Network\b|\bNWConnection\b|\bNWListener\b|\bCFNetwork\b|\bWKWebView\b|\bSocketPort\b' \
  apple/Sources examples/macos-app --include='*.swift' --exclude-dir=Generated || true)"
[[ -z "$swift_hits" ]] || fail "networking API in Swift code:"$'\n'"$swift_hits"

# macOS entitlements that allow network access.
entitlement_hits="$(grep -rnE 'com\.apple\.security\.network\.(client|server)' examples/macos-app apple || true)"
[[ -z "$entitlement_hits" ]] || fail "network entitlement:"$'\n'"$entitlement_hits"

# MSIX capabilities that allow network access.
capability_hits="$(grep -rnE 'Name="(internetClient|internetClientServer|privateNetworkClientServer)"' examples/windows-app || true)"
[[ -z "$capability_hits" ]] || fail "network capability in the MSIX manifest:"$'\n'"$capability_hits"

if [[ $status -eq 0 ]]; then
  echo "no-network check passed"
fi
exit $status
