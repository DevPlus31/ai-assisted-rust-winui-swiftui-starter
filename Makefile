# Developer entry points (macOS host). Nothing here links a Windows binary:
# `windows-check` type-checks and lints the Windows code, and CI builds it
# on windows-2025.
.PHONY: demo test lint audit coverage l10n windows-check bindings apple apple-release swift-test macos-app macos-app-test release-macos store-check-macos clean

# Minimum Rust line coverage (`make coverage`, CI).
COVERAGE_MIN ?= 80
# Version for `make release-macos`.
VERSION ?= 1.0.0

demo: ## Two-minute engine tour in a temp folder: analyse, preview, apply, undo
	cargo run -q -p app-core --example demo

test: ## Rust unit tests (core, FFI, Windows layer + sample — all platform-neutral)
	cargo test --workspace

lint:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings

coverage: ## Rust line coverage (needs cargo-llvm-cov); fails under COVERAGE_MIN
	cargo llvm-cov --workspace --summary-only --fail-under-lines $(COVERAGE_MIN) \
		--ignore-filename-regex '(uniffi-bindgen|edge_panel/win32)/'

l10n: ## Regenerate l10n/engine-messages.json, the translators' catalog
	UPDATE_L10N=1 cargo test -p app-core translators_catalog_is_up_to_date

audit: ## Licenses, advisories, banned (networking) crates, no-network shell code
	cargo deny check
	scripts/check-no-network.sh

# Clippy for x86_64-pc-windows-msvc, including the Win32 glue and the WinUI
# sample. CARGO_FEATURE_PURE makes blake3 skip its assembly, which needs the
# MSVC assembler; real Windows builds keep the SIMD code.
windows-check:
	rustup target add x86_64-pc-windows-msvc
	CARGO_FEATURE_PURE=1 cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings

bindings: ## Regenerate the raw Win32 bindings for the edge panel
	cargo run --manifest-path tools/win32-bindgen/Cargo.toml

apple: ## Debug xcframework + Swift bindings for the host architecture
	ARCHS="$$(uname -m)" scripts/build-apple.sh Debug

apple-release: ## Universal (arm64 + x86_64) release xcframework
	scripts/build-apple.sh Release

swift-test: apple ## Swift package tests (Swift → UniFFI → Rust)
	cd apple && swift test

macos-app: apple ## Sample SwiftUI app (needs XcodeGen: brew install xcodegen)
	cd examples/macos-app && xcodegen generate
	xcodebuild -project examples/macos-app/StarterApp.xcodeproj -scheme StarterApp \
		-configuration Debug -derivedDataPath target/macos-app build

macos-app-test: macos-app ## Unit tests for the SwiftUI sample's value types
	xcodebuild test -project examples/macos-app/StarterApp.xcodeproj -scheme StarterApp \
		-configuration Debug -derivedDataPath target/macos-app

release-macos: ## Universal, zipped macOS app in dist/ (see scripts/release-macos.sh)
	scripts/release-macos.sh $(VERSION)

store-check-macos: release-macos ## Optional Mac App Store pre-flight on the release build
	python3 -I scripts/store-check-macos.py "target/macos-release/Build/Products/Release/App Starter.app"

clean:
	cargo clean
	rm -rf apple/.build apple/Artifacts apple/Sources/AppCore/Generated \
		examples/macos-app/StarterApp.xcodeproj dist
