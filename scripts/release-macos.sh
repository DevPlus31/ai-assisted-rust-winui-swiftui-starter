#!/usr/bin/env bash
# Builds the sample macOS app for release (universal arm64 + x86_64) and
# zips it into dist/.
#
# Usage: scripts/release-macos.sh [version]      e.g. 1.2.0 (default 1.0.0)
#
# Without signing secrets the app is ad-hoc signed: fine for testing, not
# for distribution. For a Developer ID build outside the Mac App Store, set:
#   MACOS_SIGNING_IDENTITY  e.g. "Developer ID Application: Name (TEAMID)",
#                           already in the keychain
#   APPLE_TEAM_ID
# and to notarize as well:
#   APPLE_ID, APPLE_APP_PASSWORD (an app-specific password)
# Mac App Store builds go through Xcode (Product > Archive) instead.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:-1.0.0}"
DIST="$ROOT/dist"
DERIVED="$ROOT/target/macos-release"
APP="$DERIVED/Build/Products/Release/App Starter.app"
ZIP="$DIST/AppStarter-$VERSION-macos.zip"

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: version must look like 1.2.3, got '$VERSION'" >&2
  exit 1
fi

cd "$ROOT"
# SwiftPM resolves the binary target before Xcode's pre-build script runs,
# so the universal xcframework must exist first.
scripts/build-apple.sh Release
(cd examples/macos-app && xcodegen generate)

signing=(CODE_SIGN_IDENTITY=-)
if [[ -n "${MACOS_SIGNING_IDENTITY:-}" ]]; then
  signing=(
    CODE_SIGN_IDENTITY="$MACOS_SIGNING_IDENTITY"
    DEVELOPMENT_TEAM="${APPLE_TEAM_ID:?APPLE_TEAM_ID is required for signing}"
    OTHER_CODE_SIGN_FLAGS=--timestamp
  )
fi

xcodebuild -project examples/macos-app/StarterApp.xcodeproj -scheme StarterApp \
  -configuration Release -derivedDataPath "$DERIVED" \
  ARCHS="arm64 x86_64" ONLY_ACTIVE_ARCH=NO \
  MARKETING_VERSION="$VERSION" "${signing[@]}" \
  build

mkdir -p "$DIST"
rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"

if [[ -n "${MACOS_SIGNING_IDENTITY:-}" && -n "${APPLE_ID:-}" ]]; then
  xcrun notarytool submit "$ZIP" --wait \
    --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
    --password "${APPLE_APP_PASSWORD:?APPLE_APP_PASSWORD is required to notarize}"
  xcrun stapler staple "$APP"
  rm -f "$ZIP"
  ditto -c -k --keepParent "$APP" "$ZIP" # zip again with the stapled ticket
fi

lipo -archs "$APP/Contents/MacOS/App Starter"
echo "Built $ZIP"
