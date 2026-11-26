#!/usr/bin/env bash
# Builds the Rust engine for macOS and packages it for Swift Package Manager.
#
# Usage: scripts/build-apple.sh [Debug|Release]
#
# ARCHS selects the slices (default: "arm64 x86_64", a universal library);
# `make swift-test` passes only the host architecture.
#
# Output (consumed by apple/Package.swift; both are generated, not committed):
#   apple/Artifacts/AppFFI.xcframework                 static library + C header
#                                                      + `app_ffiFFI` Clang module
#   apple/Sources/AppCore/Generated/AppFFI.swift   UniFFI Swift bindings
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARTIFACTS="$ROOT/apple/Artifacts"
BINDINGS="$ROOT/apple/Sources/AppCore/Generated"
CONFIGURATION="${1:-${CONFIGURATION:-Release}}"
LIB_NAME="libapp_ffi"

# Xcode-launched shells have a minimal PATH.
export PATH="$HOME/.cargo/bin:$PATH"
if ! command -v rustup >/dev/null; then
  echo "error: rustup not found. Install Rust from https://rustup.rs (Homebrew's rust lacks rustup targets)." >&2
  exit 1
fi
# Pin the macOS SDK for crates that compile C (e.g. blake3's SIMD code).
unset LIBRARY_PATH
SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
export SDKROOT

case "$CONFIGURATION" in
  Debug) PROFILE="dev";     PROFILE_DIR="debug" ;;
  *)     PROFILE="release"; PROFILE_DIR="release" ;;
esac

TARGETS=()
for arch in ${ARCHS:-arm64 x86_64}; do
  case "$arch" in
    arm64)  TARGETS+=(aarch64-apple-darwin) ;;
    x86_64) TARGETS+=(x86_64-apple-darwin) ;;
    *) echo "error: unsupported architecture '$arch'" >&2; exit 1 ;;
  esac
done

cd "$ROOT"
installed="$(rustup target list --installed)"
for target in "${TARGETS[@]}"; do
  grep -qx "$target" <<<"$installed" || rustup target add "$target"
  cargo build -p app-ffi --lib --target "$target" --profile "$PROFILE"
done

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/lib" "$STAGE/include" "$STAGE/bindings"

LIBS=()
for target in "${TARGETS[@]}"; do
  LIBS+=("$ROOT/target/$target/$PROFILE_DIR/$LIB_NAME.a")
done
lipo -create "${LIBS[@]}" -output "$STAGE/lib/$LIB_NAME.a"

# Library-mode bindgen reads the metadata embedded in the compiled cdylib,
# so the Swift API always matches the Rust that was just built.
cargo run -q -p uniffi-bindgen -- generate \
  --library "$ROOT/target/${TARGETS[0]}/$PROFILE_DIR/$LIB_NAME.dylib" \
  --language swift \
  --out-dir "$STAGE/bindings"
mv "$STAGE/bindings/app_ffiFFI.h" "$STAGE/include/app_ffiFFI.h"
mv "$STAGE/bindings/app_ffiFFI.modulemap" "$STAGE/include/module.modulemap"

# Rebuild the xcframework only when the library or headers changed, so an
# unchanged engine doesn't make SwiftPM relink everything.
FRAMEWORK="$ARTIFACTS/AppFFI.xcframework"
SLICE="$(find "$FRAMEWORK" -mindepth 1 -maxdepth 1 -type d -name 'macos-*' 2>/dev/null | head -n 1 || true)"
if [[ -n "$SLICE" ]] \
  && cmp -s "$STAGE/lib/$LIB_NAME.a" "$SLICE/$LIB_NAME.a" \
  && diff -rq "$STAGE/include" "$SLICE/Headers" >/dev/null; then
  echo "AppFFI.xcframework unchanged"
else
  mkdir -p "$ARTIFACTS"
  rm -rf "$STAGE/AppFFI.xcframework"
  xcodebuild -create-xcframework \
    -library "$STAGE/lib/$LIB_NAME.a" -headers "$STAGE/include" \
    -output "$STAGE/AppFFI.xcframework" >/dev/null
  rm -rf "$FRAMEWORK"
  mv "$STAGE/AppFFI.xcframework" "$FRAMEWORK"
fi

mkdir -p "$BINDINGS"
if ! cmp -s "$STAGE/bindings/app_ffi.swift" "$BINDINGS/AppFFI.swift"; then
  mv "$STAGE/bindings/app_ffi.swift" "$BINDINGS/AppFFI.swift"
fi

echo "AppCore ready: ${TARGETS[*]} ($PROFILE_DIR)"
