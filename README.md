# AI-Assisted Rust WinUI SwiftUI Starter

A starter for native Windows and macOS desktop apps that share one Rust core, laid out so AI coding agents can write the platform code safely.

## What it is

The app's logic lives once, in Rust (`crates/app-core`). Each platform gets a fully native UI on top of it:

- **macOS:** SwiftUI. The Rust core is compiled into a static library and reached through generated Swift bindings ([UniFFI](https://mozilla.github.io/uniffi-rs/)), packaged as the Swift package `AppCore`.
- **Windows:** WinUI 3, written in Rust with Microsoft's [`windows-reactor`](https://crates.io/crates/windows-reactor). The core is linked in directly, with no bindings layer.

Neither UI is a web view or a cross-platform toolkit. Each app looks and behaves like the platform it runs on, and the behaviour underneath is the same code on both.

Around that core sits everything a real app needs on day one: two working sample apps, sandboxing and store packaging for both platforms, progress and cancellation for long tasks, translatable messages, a release pipeline and checks for licenses, security advisories and network use.

The sample engine (local file analysis, classification, duplicate finding, image compression and plan → apply → undo file organizing) came from FylDock 1.0.0. It shows how a real engine plugs into both UIs. Replace it with your app's own logic.

## The goal

Building a native app for two platforms usually means writing the logic twice or giving up native UI. This starter takes a third route: write the logic once in Rust, and let AI coding agents do much of the per-platform UI and glue work, which is the part that otherwise doubles.

Agents are good at that work when the boundaries are clear and every mistake is caught by a check rather than by a person. So the project is built to give them exactly that:

- **One place for logic.** Behaviour lives in `app-core` and nowhere else. The FFI layer and the UIs only translate and display.
- **A fixed layout and written rules.** [`AGENTS.md`](AGENTS.md) says where things go, what is forbidden (network access, `unsafe` outside one module, engine work on the UI thread) and how to add an engine function end to end, from core to both UIs.
- **Checks at every layer.** Strict lints, Rust and Swift tests, a Windows-target check that runs from a Mac, a coverage floor, license and security audits, and a check that the committed Win32 bindings match their generator. In Claude Code, hooks format each edit and run the lints before the agent finishes.
- **Guarantees a user can trust.** Everything runs locally. The engine only touches folders the user granted, and nothing moves until the user has seen a preview, which can always be undone.

The aim is a project where you can ask an agent for a feature and get a change that compiles, passes the checks on both platforms and keeps those guarantees.

**Status:** the Rust core, the Swift package and the macOS app are built and tested on macOS. The Windows code is checked from macOS (it compiles and passes clippy for the Windows target) but has not yet been built or run on Windows; the Windows CI job will be its first real build.

## Project layout

```text
crates/
  app-core/              Rust engine: analysis, classification, dedupe,
                         compression, plan → apply → undo workflows, storage
  app-ffi/               UniFFI surface over app-core (for Swift)
  app-platform-windows/  Native Windows layer: data folders, preferences,
                         shared engine handle, edge-panel model + Win32 glue
  uniffi-bindgen/        Workspace-pinned UniFFI CLI
apple/                   Swift package "AppCore"
  Package.swift
  Sources/AppCore/       EngineService, FolderStore (security-scoped
                         bookmarks), category helpers + generated bindings
  Tests/AppCoreTests/
examples/
  windows-app/           WinUI 3 sample shell (windows-reactor) + MSIX
                         manifest template
  macos-app/             SwiftUI sample shell (XcodeGen, sandboxed)
scripts/                 build-apple.sh (xcframework + bindings), release
                         scripts, check-no-network.sh
tools/win32-bindgen/     Regenerates the raw Win32 bindings (make bindings)
l10n/                    engine-messages.json: every engine sentence, for
                         translators
deny.toml                cargo-deny: licenses, advisories, banned crates
AGENTS.md                Rules and checks for AI coding agents
                         (CLAUDE.md points here)
.claude/settings.json    Claude Code hooks: rustfmt on edit, lint on stop
```

## Quick demo

```sh
make demo
```

This runs the engine on a throwaway messy Downloads folder. It analyses the files on a background thread while watching progress, previews where each would go (`Screenshot 2026-02-01 at 09.30.12.png` → `Screenshots/2026-02-01 Screenshot.png`), applies the plan, undoes it and shows an error message in English and German. It needs no Xcode or Windows and touches nothing outside a temporary folder. The code is `crates/app-core/examples/demo.rs`.

## Sample shells

Both samples run the engine's full flow. Pick a folder and analyse it in the background, with a progress bar and Cancel. Choose a destination, preview where each file would go and untick any move. Then apply, and undo afterwards. Use them as the starting point for your own UI.

**macOS** (`examples/macos-app`): a sandboxed SwiftUI app. It keeps folder grants as security-scoped bookmarks through `FolderStore` and runs engine calls through `EngineService.run`. `project.yml` is the single source for the Xcode project, `Info.plist` and the entitlements (sandbox, user-selected read-write, app-scoped bookmarks, no network).

```sh
brew install xcodegen
make macos-app                                  # builds the engine, generates and builds the app
open examples/macos-app/StarterApp.xcodeproj    # then Run
```

**Windows** (`examples/windows-app`): a WinUI 3 app in Rust. `summary.rs` (analysis) and `organize.rs` (plan, apply, undo) call the engine and are unit-tested on every OS. `shell.rs` is the `windows-reactor` component. It runs engine work with `spawn_background` and polls progress with a short background tick, because a component's sender can't be used from another thread.

```powershell
cargo run -p windows-app
```

Unpackaged runs need the [Windows App SDK runtime](https://learn.microsoft.com/windows/apps/windows-app-sdk/downloads) 2.x installed. To ship a self-contained `.exe` instead, add [`windows-reactor-setup`](https://crates.io/crates/windows-reactor-setup) as a build dependency. Its build script downloads the runtime from NuGet. For the Microsoft Store, start from `examples/windows-app/packaging/AppxManifest.xml`, which declares the runtime as a framework dependency.

## Guarantees (enforced in app-core)

- Everything runs locally. There are no networking dependencies, no cloud AI and no code loaded at runtime.
- The engine only touches folders the shell passes in, such as folders the user granted under the macOS sandbox, plus an app-data folder.
- File changes are always planned first and never overwrite anything. Every change can be undone.

## macOS: Swift package

```sh
make apple          # Debug, host architecture
make apple-release  # Release, universal (arm64 + x86_64)
make swift-test     # Swift → UniFFI → Rust tests
```

`scripts/build-apple.sh` writes two generated files, which are not committed:

- `apple/Artifacts/AppFFI.xcframework`: the static library, the C header and the `app_ffiFFI` module.
- `apple/Sources/AppCore/Generated/AppFFI.swift`: the UniFFI bindings.

The script rebuilds the xcframework only when the Rust output changes.

To use the package in an app, add `apple/` as a local package dependency and `import AppCore`. With XcodeGen:

```yaml
packages:
  AppCore:
    path: ../ai-assisted-rust-winui-swiftui-starter/apple
targets:
  MyApp:
    dependencies:
      - package: AppCore
    preBuildScripts:
      - name: Build AppCore
        script: '"${PROJECT_DIR}/../ai-assisted-rust-winui-swiftui-starter/scripts/build-apple.sh" "${CONFIGURATION}"'
        basedOnDependencyAnalysis: false
```

The library is linked statically, so there is no extra dylib to embed, sign or notarize. The minimum OS is macOS 15. Keep `Package.swift` and `.cargo/config.toml` in sync.

| API | Purpose |
|---|---|
| `EngineService.openDefault()` | Opens the engine in the app's sandbox container. |
| `EngineService.run { ... }` | Runs engine calls off the main actor. |
| `EngineService.run(job:onProgress:) { ... }` | Runs a long call with an `EngineJob`. It reports progress on the main actor, and cancelling the Swift task cancels the job (`AppError.Cancelled`). |
| `EngineJob` | Progress (`progress().fraction`) and `cancel()` for one long call, such as `analyzeFolder(folder:recursive:job:)`. |
| `AppEngine` | The full generated engine API: `analyzeFolder`, `planWorkspace`, `planDropped`, `applyPlan`, `undoLast`, `findDuplicates`, `compressImages`, `rules`, `collections`. |
| `FolderStore` | Stores folder grants as security-scoped bookmarks: workspaces, a default destination and per-category destinations. |
| `userMessage(for:)` | Turns any error into text a user can act on, translated when the app has a translation. |
| `localized(_:)` | Shows any engine sentence (`UserTextRecord`: notices, warnings, per-file problems) in the user's language. |
| `FileCategory.storageKey`, `fileCategories()` | Stable category keys and titles. |

## Windows: Rust crate

```toml
[dependencies]
app-core = { path = "../ai-assisted-rust-winui-swiftui-starter/crates/app-core" }
app-platform-windows = { path = "../ai-assisted-rust-winui-swiftui-starter/crates/app-platform-windows" }
```

| Module | Purpose |
|---|---|
| `data_dir` | Locates `%LOCALAPPDATA%\AppStarter` (MSIX-redirected, so it is removed on uninstall), the Downloads inbox and the default destination. |
| `prefs`, `destinations` | Shell preferences saved next to the engine data, and destination fields as the user typed them, turned into engine `Destinations`. |
| `workspace::Workspace` | A thread-safe `Engine` handle that can be cloned into background jobs. |
| `app_core::Job` | Progress and cancellation for long engine calls such as `Engine::analyze_folder`. Share it in an `Arc` and poll `progress()` from the UI. |
| `edge_panel::model` | Platform-neutral state machine for the edge panel: grow, reveal, fade, then shrink. |
| `edge_panel::win32` | Windows only. Turns a window into a borderless, non-activating topmost tool window, places and shapes it, and reads display metrics. All `unsafe` code lives here. |

### Optional: edge panel

`edge_panel` is an optional building block that no sample uses yet. It turns a window into a small pill on a screen edge that springs open into a drop panel when the pointer or a file drag reaches it. It is Windows-only for now; a macOS version would be an `NSPanel` driven by the same rules.

- **`EdgeDockModel`** is a pure state machine. Feed it `EdgeEvent`s (pointer entered or exited, drag entered, left or dropped, timer elapsed). It returns the next state plus `Effect`s for the shell to perform in order: grow or shrink the window, start the hover-out timer (`HOVER_OUT_DELAY`) or the fade timer (`FADE`). The window is always grown before content appears and shrunk only after it fades, so empty window space never blocks clicks underneath.
- **`win32::Window`** finds the shell's own window by title (`Window::find_own`). It then makes it a borderless, non-activating, topmost tool window (`make_edge_panel`), places and rounds it for an `EdgePlacement` (`place`), and shows or hides it without stealing focus. `primary_display()` gives the work area and scale to place against.

To use it from a `windows-reactor` shell, open a second window for the panel and call `Window::find_own` with its title after it appears. Route pointer and drag events into the model, and carry out its effects. Use the same background-tick pattern as the sample's progress polling for the timers, keyed by the model's `generation`.

The Win32 calls come from raw bindings generated by `tools/win32-bindgen`. To call another Win32 API, add it to the tool's filter list, run `make bindings` and wrap it in `edge_panel::win32` with a `// SAFETY:` comment. CI fails if the committed bindings drift from the generator.

Everything except `edge_panel::win32` is platform-neutral and tested on macOS. The Win32 module only compiles on Windows; CI runs clippy on it on `windows-2025`. Cargo reads `.cargo/config.toml` only for builds run inside this folder. Copy its Windows `rustflags` (static CRT, Control Flow Guard, CET) into the app's own config, because the Microsoft Store certification checks them on the final `.exe`.

## Translations

Every sentence the engine shows (errors, per-file problems, notices, warnings) is a `UserText`: a stable key such as `folder-not-absolute`, its English template and named arguments. The engine never formats finished English for a shell.

- **Translators' file:** `l10n/engine-messages.json` lists every key with its English template. A test fails when it drifts from the code; `make l10n` rewrites it. Never rename or reuse a key, because translations are keyed by it.
- **macOS:** `localized(_:)` looks the key up in an `Engine` string table in the app bundle (`Engine.xcstrings` or `Engine.strings`), falls back to English and fills in the arguments. `userMessage(for:)` uses it for errors. The sample app's own UI text is localizable too. `StarterApp/Localizable.xcstrings` fills itself with every string when you build in Xcode; add languages there.
- **Windows:** `examples/windows-app/src/text.rs` loads `l10n/<language>.json` from next to the `.exe`, in the same format as the translators' file, for the user's preferred UI languages (`de-DE.json`, then `de.json`). Missing keys stay English. `scripts/package-msix.ps1` packs `examples/windows-app/l10n/` if it exists. The WinUI sample's own UI labels are still inline English.
- **Not translated:** category names. They are also the names of the folders files are filed into, so translating them would change where files go on disk.

## Checks

```sh
make lint            # rustfmt + clippy -D warnings
make test            # Rust tests
make windows-check   # clippy for the Windows target, from macOS
make swift-test      # Swift → UniFFI → Rust tests
make macos-app-test  # builds the SwiftUI sample and runs its unit tests
make audit           # licenses, security advisories, banned networking crates, no-network shell code
make coverage        # Rust line coverage, at least 80% (cargo-llvm-cov)
```

CI also checks that `bindings.rs` matches what `tools/win32-bindgen` generates, and builds the WinUI sample on `windows-2025`. Dependabot proposes dependency updates weekly, grouping the `windows-*` and `uniffi*` crates.

`make audit` runs [`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny) with `deny.toml` across the macOS and Windows targets. It allows only licenses compatible with shipping MIT apps through the stores, fails on known vulnerable or unmaintained crates and bans networking crates such as `reqwest`, `hyper`, `rustls` and `mio`, even as indirect dependencies. `scripts/check-no-network.sh` does the same for Swift code, the macOS entitlements and the MSIX capabilities.

## Working with Claude Code

`AGENTS.md` holds the rules and the recipe for adding an engine call end to end; `CLAUDE.md` points to it. `.claude/settings.json` adds two hooks for everyone who opens the project in Claude Code:

- After each edit to a `.rs` file, `rustfmt` formats it.
- Before Claude finishes, `make lint` runs. On failure the errors go back to Claude to fix, once per stop so it can't loop.

## Releasing

Push a tag such as `v1.2.0`. `.github/workflows/release.yml` builds an MSIX for x64 and for ARM64 and a universal, zipped macOS app, then attaches them to a GitHub release. Run the workflow by hand to get the builds without publishing.

Signing is optional and driven by repository secrets:

| Secret | Used for |
|---|---|
| `WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD` | Signing the MSIX for sideloading (a base64 `.pfx` whose subject matches the manifest's `Publisher`). The Microsoft Store re-signs submitted packages, so Store builds don't need it. |
| `MACOS_CERTIFICATE`, `MACOS_CERTIFICATE_PASSWORD`, `MACOS_SIGNING_IDENTITY`, `APPLE_TEAM_ID` | Signing the app with a Developer ID certificate (base64 `.p12`) for distribution outside the Mac App Store. |
| `APPLE_ID`, `APPLE_APP_PASSWORD` | Notarizing and stapling that build. |

Without them, the MSIX is unsigned and the macOS app is ad-hoc signed, which is enough for testing. Mac App Store builds go through Xcode (Product > Archive). Before submitting, replace the placeholder identity in `examples/windows-app/packaging/AppxManifest.xml` and the logos in `packaging/Assets/`, and the bundle ID in `examples/macos-app/project.yml`.

Locally: `make release-macos VERSION=1.2.0` writes `dist/AppStarter-1.2.0-macos.zip`. On Windows, build with `cargo build -p windows-app --release --target x86_64-pc-windows-msvc`, then run `scripts/package-msix.ps1`.

## License

[MIT](LICENSE)
