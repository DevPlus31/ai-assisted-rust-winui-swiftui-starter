# Agent guide

Instructions for AI coding agents (Claude Code, Codex, Copilot, Cursor…) working in this repo. Read `README.md` for the full picture; this file covers the rules and the checks.

## Layout and boundaries

| Path | What lives there | Rules |
|---|---|---|
| `crates/app-core` | All app logic, in pure Rust | No UI, FFI, networking or platform code. Takes only folders the shell passes in. Every file change goes through plan → apply → undo and never overwrites. |
| `crates/app-ffi` | UniFFI surface for Swift | Thin wrappers only: convert types and errors, keep logic in `app-core`. |
| `crates/app-platform-windows` | Windows platform layer, no UI | Platform-neutral except `edge_panel::win32`, the **only** place `unsafe` is allowed (each block needs a `// SAFETY:` comment). |
| `crates/app-platform-windows/src/edge_panel/win32/bindings.rs` | Generated Win32 bindings | Never edit by hand. Add the API to `tools/win32-bindgen/src/main.rs`, then `make bindings`. |
| `apple/Sources/AppCore` | Swift services over the engine | `Generated/` is written by `scripts/build-apple.sh`; never edit it. |
| `examples/windows-app` | WinUI 3 sample shell (`windows-reactor`) | Engine calls go in `summary.rs` and `organize.rs` (platform-neutral, tested); `shell.rs` holds UI state and renders. |
| `examples/macos-app` | SwiftUI sample shell (XcodeGen) | `project.yml` is the source of truth. The `.xcodeproj`, `Info.plist` and `.entitlements` are generated from it. |

## Hard rules

- **No network.** Don't add networking crates, Swift networking, or entitlements and capabilities that allow network access.
- **No `unsafe`** outside `edge_panel::win32`. The workspace denies `unsafe_code`.
- **Engine work never runs on the UI thread.** Windows: `ComponentContext::spawn_background`. macOS: `EngineService.run`.
- **Long calls take a `Job`** (`EngineJob` in Swift). The UI polls its progress and calls `cancel()`. Never call back into UI code from engine threads.
- **Files only move through plan → preview → apply.** Shells show the plan, let the user untick moves and pass the plan id with skip indexes. Undo is always offered afterwards.
- **User-facing errors** are plain sentences the user can act on. Never show debug output.
- **Every sentence the engine shows has a `TextKey`** (`crates/app-core/src/user_text.rs`) and travels as a `UserText`. Don't format English in the engine. Shells show it through `localized(_:)` (Swift) or `Translations::show` (Windows sample). After adding or changing a key, run `make l10n`; never rename or reuse a key id.
- **Keep the minimum OS in sync** (macOS 15) across `apple/Package.swift`, `.cargo/config.toml` and `examples/macos-app/project.yml`.
- Keep `.cargo/config.toml`'s Windows `rustflags` (static CRT, Control Flow Guard, CET). Store certification checks them.
- Changing the FFI surface means rebuilding the bindings (`make apple`) and updating the Swift callers in the same change.

## Recipe: add an engine call end to end

A new engine capability has to reach both shells in the same change. Work from the core outwards and run the listed check after each step.

1. **Core** (`crates/app-core`): add the method on `Engine` or a function in the right module, returning `Result<_, CoreError>` with a user-facing `Display`. Tests go next to it. If it can take more than about a second, take a `&Job` so it reports progress and can stop early with `CoreError::Cancelled`. Check: `cargo test -p app-core`.
2. **Messages and errors:** new sentences get a `TextKey` (then `make l10n`). A new `CoreError` variant also needs cases in `CoreError::user_text` and `CoreError::detail`, in `AppError` plus its `From<CoreError>` (`crates/app-ffi/src/lib.rs`), and in `userMessage(for:)` (`apple/Sources/AppCore/EngineService.swift`).
3. **FFI** (`crates/app-ffi`): add a thin `#[uniffi::export]` wrapper on `AppEngine` that converts to and from records in `records.rs`. Never pass raw paths that the engine will act on; reference engine-owned state such as plan ids and the analysed workspace instead. Add a test in `lib.rs`. Check: `cargo test -p app-ffi`.
4. **Swift bindings:** run `make apple`, then fix the Swift callers (`apple/Sources`, `apple/Tests`, `examples/macos-app`). Long calls go through `EngineService.run(job:onProgress:)`. Check: `make swift-test`.
5. **Windows:** call it through `Workspace::with` from a platform-neutral module in `examples/windows-app` (such as `summary.rs` or `organize.rs`) with tests. `shell.rs` only sends messages and runs the work in `spawn_background`. Check: `make test` and `make windows-check`.
6. **macOS app:** call it from `AnalysisModel` through `EngineService` and never on the main actor. Check: `make macos-app`.
7. **Docs:** update the API tables in `README.md`. Then run every check below.

Worked example: progress and cancellation for folder analysis touched `app-core/src/job.rs`, `engine.rs`, `analyze/mod.rs` and `error.rs`, then the FFI's `EngineJob`, `ProgressRecord` and `AppError::Cancelled`, then `EngineService.run(job:onProgress:)` in the Swift package, then both sample shells.

## Checks: run before you say you're done

```sh
make lint            # rustfmt + clippy -D warnings (pedantic)
make test            # all Rust tests
make windows-check   # clippy for x86_64-pc-windows-msvc (Win32 glue + WinUI sample), from macOS
make swift-test      # Swift → UniFFI → Rust
make macos-app-test  # builds the SwiftUI sample and runs its unit tests (needs XcodeGen)
make audit           # cargo-deny (licenses, advisories, banned networking crates) + no-network check
make coverage        # Rust line coverage ≥ 80% (needs cargo-llvm-cov)
```

In Claude Code, `.claude/settings.json` runs `rustfmt` after every edit to a `.rs` file and `make lint` before Claude stops. If lint fails, the errors come back to Claude to fix.

Nothing on macOS links a Windows binary. CI builds and tests the Windows code on `windows-2025`. If you can't run a check, say which one and why.

## Conventions

- Rust 2024 edition, toolchain pinned in `rust-toolchain.toml`. Clippy `pedantic` is on. Fix the warning rather than adding `allow`; when an `allow` is the right call, add a one-line reason.
- Put tests next to the code (`#[cfg(test)] mod tests`) and name them for the behaviour they check (`rejects_relative_paths`).
- Swift 6 language mode with strict concurrency. Use Swift Testing (`import Testing`) for new tests.
- Commits follow Conventional Commits (`feat:`, `fix:`, `docs:`…).
