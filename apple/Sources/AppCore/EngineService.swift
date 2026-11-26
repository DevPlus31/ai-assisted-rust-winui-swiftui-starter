import Foundation

/// Runs Rust engine calls off the main actor so analysis, hashing and file
/// moves never block the UI. The engine serializes calls internally.
public struct EngineService: Sendable {
    public let engine: AppEngine

    public init(engine: AppEngine) {
        self.engine = engine
    }

    /// Opens the app's own data inside the sandbox container
    /// (~/Library/Containers/<bundle id>/Data/Library/Application Support/AppStarter).
    public static func openDefault() throws -> EngineService {
        let support = try FileManager.default.url(
            for: .applicationSupportDirectory,
            in: .userDomainMask,
            appropriateFor: nil,
            create: true
        )
        let directory = support.appending(path: "AppStarter", directoryHint: .isDirectory)
        return EngineService(engine: try AppEngine.open(dataDirectory: directory.path(percentEncoded: false)))
    }

    public func run<T: Sendable>(_ work: @escaping @Sendable (AppEngine) throws -> T) async throws -> T {
        let engine = engine
        return try await Task.detached(priority: .userInitiated) { try work(engine) }.value
    }

    public func run<T: Sendable>(_ work: @escaping @Sendable (AppEngine) -> T) async -> T {
        let engine = engine
        return await Task.detached(priority: .userInitiated) { work(engine) }.value
    }

    /// Runs a long engine call that takes an `EngineJob`, off the main actor.
    ///
    /// `onProgress` runs on the main actor about ten times a second while the
    /// call runs, and once more at the end. Cancelling the calling Swift task
    /// cancels `job`, so the call stops early and throws `AppError.Cancelled`.
    /// Keep `job` to cancel it from a button.
    public func run<T: Sendable>(
        job: EngineJob,
        onProgress: @escaping @MainActor (ProgressRecord) -> Void,
        _ work: @escaping @Sendable (AppEngine, EngineJob) throws -> T
    ) async throws -> T {
        let poller = Task { @MainActor in
            while !Task.isCancelled {
                onProgress(job.progress())
                try? await Task.sleep(for: Self.progressInterval)
            }
        }
        defer { poller.cancel() }
        let engine = engine
        let result = try await withTaskCancellationHandler {
            try await Task.detached(priority: .userInitiated) { try work(engine, job) }.value
        } onCancel: {
            job.cancel()
        }
        await onProgress(job.progress())
        return result
    }

    static let progressInterval = Duration.milliseconds(100)
}

extension ProgressRecord {
    /// Share of the work done (0...1), or `nil` while the engine is still
    /// counting it: show an indeterminate indicator then.
    public var fraction: Double? {
        total > 0 ? Double(min(done, total)) / Double(total) : nil
    }
}

extension FileRecord: Identifiable {
    public var id: String { path }
}

extension DuplicateGroupRecord: Identifiable {
    public var id: String { paths.joined(separator: "\n") }
}

/// Turns any error into text a user can act on, translated when the app
/// has a translation for the engine's message (see `localized(_:)`).
public func userMessage(for error: Error) -> String {
    switch error {
    case let AppError.InvalidInput(_, text),
         let AppError.NothingToUndo(_, text),
         let AppError.Storage(_, text),
         let AppError.Cancelled(_, text):
        localized(text)
    case let error as LocalizedError where error.errorDescription != nil:
        error.errorDescription ?? ""
    default:
        String(localized: "Something unexpected went wrong.")
    }
}

/// The engine's sentence in the user's language.
///
/// Looks `text.key` up in `table` (default `Engine`, i.e. `Engine.xcstrings`
/// or `Engine.strings` in the app bundle), falls back to the English
/// template, then fills in the `{name}` arguments. `l10n/engine-messages.json`
/// at the repository root lists every key and its English template.
public func localized(_ text: UserTextRecord, table: String = "Engine", bundle: Bundle = .main) -> String {
    let template = bundle.localizedString(forKey: text.key, value: text.template, table: table)
    return text.args.reduce(template) { result, arg in
        result.replacingOccurrences(of: "{\(arg.name)}", with: arg.value)
    }
}
