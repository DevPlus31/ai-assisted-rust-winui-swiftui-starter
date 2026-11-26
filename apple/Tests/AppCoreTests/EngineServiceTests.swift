import Foundation
import Testing
import AppCore

/// Swift → UniFFI → Rust, end to end, in temp folders.
final class EngineServiceTests {
    private let root = FileManager.default.temporaryDirectory
        .appending(path: "AppCoreTests-\(UUID().uuidString)", directoryHint: .isDirectory)
    private var inbox: URL { root.appending(path: "Downloads", directoryHint: .isDirectory) }
    private var organized: URL { root.appending(path: "Organized", directoryHint: .isDirectory) }

    init() throws {
        try FileManager.default.createDirectory(at: inbox, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: organized, withIntermediateDirectories: true)
    }

    deinit {
        try? FileManager.default.removeItem(at: root)
    }

    private func makeService() throws -> EngineService {
        let data = root.appending(path: "data", directoryHint: .isDirectory)
        return EngineService(engine: try AppEngine.open(dataDirectory: data.path(percentEncoded: false)))
    }

    private func write(_ name: String, _ text: String) throws {
        try Data(text.utf8).write(to: inbox.appending(path: name))
    }

    @Test func analyzesPlansAppliesAndUndoes() async throws {
        try write("scan_7.txt", "Invoice number 7. Bill to: Ana. Amount due 2026-02-03")
        let service = try makeService()
        let folder = inbox.path(percentEncoded: false)
        let destinations = DestinationsRecord(defaultFolder: organized.path(percentEncoded: false), categories: [])

        let analysis = try await service.run { try $0.analyzeFolder(folder: folder, recursive: false, job: EngineJob()) }
        #expect(analysis.files.map(\.category) == [.invoice])

        let plan = try await service.run { try $0.planWorkspace(folder: folder, destinations: destinations, onlyPaths: []) }
        #expect(plan.moves.count == 1)
        #expect(plan.moves.first?.to.hasPrefix(self.organized.path(percentEncoded: false)) == true)

        let applied = try await service.run { try $0.applyPlan(planId: plan.id, skip: []) }
        #expect(applied.moved == 1)
        #expect(await service.run { $0.canUndo() })

        let undone = try await service.run { try $0.undoLast() }
        #expect(undone.moved == 1)
        #expect(FileManager.default.fileExists(atPath: inbox.appending(path: "scan_7.txt").path(percentEncoded: false)))
    }

    @Test @MainActor func longCallsReportProgressAndEndWithTheFullCount() async throws {
        try write("a.txt", "one")
        try write("b.txt", "two")
        let service = try makeService()
        let folder = inbox.path(percentEncoded: false)
        let seen = ProgressLog()

        let analysis = try await service.run(job: EngineJob(), onProgress: seen.append) {
            try $0.analyzeFolder(folder: folder, recursive: false, job: $1)
        }

        #expect(analysis.files.count == 2)
        #expect(seen.last == ProgressRecord(done: 2, total: 2))
        #expect(seen.last?.fraction == 1)
    }

    @Test func aCancelledJobThrowsCancelledWithAMessage() async throws {
        try write("a.txt", "one")
        let service = try makeService()
        let folder = inbox.path(percentEncoded: false)
        let job = EngineJob()
        job.cancel()

        do {
            _ = try await service.run(job: job, onProgress: { _ in }) {
                try $0.analyzeFolder(folder: folder, recursive: false, job: $1)
            }
            Issue.record("a cancelled job should throw")
        } catch let error as AppError {
            guard case let .Cancelled(_, text) = error else {
                Issue.record("expected Cancelled, got \(error)")
                return
            }
            #expect(text.key == "cancelled")
            #expect(userMessage(for: error) == "Cancelled.")
        }
    }

    @Test func cancellingTheSwiftTaskCancelsTheJob() async throws {
        let service = try makeService()
        let job = EngineJob()
        // Copied out first so the task captures only Sendable values (not
        // this test instance, which `Self` inside the closure would).
        let cancelledText = Self.cancelledText
        let task = Task {
            try await service.run(job: job, onProgress: { _ in }) { _, job in
                // Stand-in for a long engine call that checks its job.
                while !job.isCancelled() { Thread.sleep(forTimeInterval: 0.01) }
                throw AppError.Cancelled(message: "Cancelled.", text: cancelledText)
            }
        }

        task.cancel()

        await #expect(throws: AppError.self) { try await task.value }
        #expect(job.isCancelled())
    }

    private static let cancelledText = UserTextRecord(key: "cancelled", template: "Cancelled.", args: [], text: "Cancelled.")

    @Test func engineMessagesFallBackToEnglishAndFillInArguments() async throws {
        let service = try makeService()
        let destinations = DestinationsRecord(defaultFolder: "relative/path", categories: [])

        do {
            _ = try await service.run { try $0.planDropped(paths: [], destinations: destinations) }
            Issue.record("a relative destination should be rejected")
        } catch let AppError.InvalidInput(message, text) {
            #expect(text.key == "folder-not-absolute")
            #expect(text.args == [TextArgRecord(name: "folder", value: "relative/path")])
            #expect(localized(text) == message)
            #expect(message == "“relative/path” isn't a full folder path.")
        }
    }

    @Test func categoriesWithoutADestinationAreReported() async throws {
        try write("song.mp3", "ID3")
        let service = try makeService()
        let song = inbox.appending(path: "song.mp3").path(percentEncoded: false)
        let destinations = DestinationsRecord(
            defaultFolder: nil,
            categories: [CategoryFolder(category: .invoice, folder: organized.path(percentEncoded: false))]
        )

        let plan = try await service.run { try $0.planDropped(paths: [song], destinations: destinations) }

        #expect(plan.moves.isEmpty)
        #expect(plan.missingCategories == [.audio])
    }

    @Test func engineErrorsBecomeReadableMessages() async throws {
        let service = try makeService()
        let empty = DestinationsRecord(defaultFolder: nil, categories: [])

        await #expect(throws: AppError.self) {
            try await service.run { try $0.planDropped(paths: [], destinations: empty) }
        }
        do {
            _ = try await service.run { try $0.undoLast() }
            Issue.record("undo without history should throw")
        } catch {
            #expect(!userMessage(for: error).isEmpty)
        }
    }

    @Test func everyCategoryHasAStableKeyAndTitle() {
        let categories = fileCategories()
        #expect(categories.count == Set(categories.map(\.id)).count)
        #expect(categories.allSatisfy { !$0.title.isEmpty })
        #expect(FileCategory.bankStatement.storageKey == "bankStatement")
    }
}

/// Collects progress updates on the main actor.
@MainActor
private final class ProgressLog {
    private(set) var updates: [ProgressRecord] = []
    var last: ProgressRecord? { updates.last }
    func append(_ progress: ProgressRecord) { updates.append(progress) }
}
