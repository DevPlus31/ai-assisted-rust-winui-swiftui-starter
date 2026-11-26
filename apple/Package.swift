// swift-tools-version: 6.0
// Run `make apple` (or scripts/build-apple.sh) first: it builds the Rust
// engine into Artifacts/AppFFI.xcframework and generates
// Sources/AppCore/Generated/AppFFI.swift.
import PackageDescription

let package = Package(
    name: "AppCore",
    platforms: [
        .macOS(.v15), // keep in sync with MACOSX_DEPLOYMENT_TARGET in .cargo/config.toml
    ],
    products: [
        .library(name: "AppCore", targets: ["AppCore"]),
    ],
    targets: [
        // The Rust engine as a static library; exposes the `app_ffiFFI` C module.
        .binaryTarget(name: "AppFFI", path: "Artifacts/AppFFI.xcframework"),
        // UniFFI bindings plus the macOS services every app shell needs.
        .target(
            name: "AppCore",
            dependencies: ["AppFFI"]
        ),
        .testTarget(
            name: "AppCoreTests",
            dependencies: ["AppCore"]
        ),
    ]
)
