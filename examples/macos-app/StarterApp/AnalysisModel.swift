import AppCore
import Foundation
import Observation

/// Drives the window: analyse (with progress and Cancel) → preview →
/// apply → undo. Every engine call runs off the main actor.
@MainActor
@Observable
final class AnalysisModel {
    enum Status: Equatable {
        case idle
        case analyzing(folderName: String, progress: ProgressRecord)
        case analyzed(FolderSummary, folder: GrantedFolder)
        case previewing(PlanPreview, summary: FolderSummary, folder: GrantedFolder)
        case working(String)
        case finished(Outcome)
        case failed(String)
    }

    private(set) var status: Status = .idle
    /// Indexes of preview rows the user unticked.
    var skipped: Set<Int> = []
    /// Set when the engine couldn't open; the UI is disabled then.
    private(set) var startupError: String?
    /// Problems the engine fixed on startup (e.g. a settings file it set aside).
    private(set) var notices: [String] = []
    private(set) var workspaces: [GrantedFolder] = []
    private(set) var destination: GrantedFolder?
    private(set) var canUndo = false

    private let engine: EngineService?
    private let store = FolderStore(file: FolderStore.defaultFile())
    private let categories = fileCategories()
    /// The running analysis; also tells a superseded one apart.
    private var job: EngineJob?

    var isBusy: Bool {
        switch status {
        case .analyzing, .working: true
        default: false
        }
    }

    var isReady: Bool { engine != nil && !isBusy }

    init() {
        do {
            let engine = try EngineService.openDefault()
            self.engine = engine
            notices = engine.engine.startupNotices().map { localized($0) }
            canUndo = engine.engine.canUndo()
        } catch {
            engine = nil
            startupError = userMessage(for: error)
        }
        refreshFolders()
    }

    // MARK: Folders

    /// Records the folder the user picked (so access survives relaunches)
    /// and analyses it.
    func grantAndAnalyze(_ url: URL) async {
        do {
            let folder = try store.grant(url, role: .workspace)
            refreshFolders()
            await analyze(folder)
        } catch {
            status = .failed(userMessage(for: error))
        }
    }

    /// An open preview was planned against the old destination (and the
    /// store has stopped accessing it), so it goes back to the summary.
    func chooseDestination(_ url: URL) {
        do {
            try store.grant(url, role: .destination)
            refreshFolders()
            backToSummary()
        } catch {
            status = .failed(userMessage(for: error))
        }
    }

    /// Leaves any summary or preview of `folder`: the app can't read it anymore.
    func forget(_ folder: GrantedFolder) {
        do {
            try store.revoke(id: folder.id)
        } catch {
            status = .failed(userMessage(for: error))
        }
        refreshFolders()
        switch status {
        case let .analyzed(_, shown) where shown.id == folder.id,
             let .previewing(_, _, shown) where shown.id == folder.id:
            status = .idle
        case .previewing where folder.role == .destination:
            backToSummary()
        default:
            break
        }
    }

    private func refreshFolders() {
        workspaces = store.workspaces
        destination = store.defaultDestination
    }

    // MARK: Analyse

    func analyze(_ folder: GrantedFolder) async {
        guard let engine, isReady else { return }
        let job = EngineJob()
        self.job = job
        status = .analyzing(folderName: folder.name, progress: ProgressRecord(done: 0, total: 0))
        let path = folder.path
        do {
            let record = try await engine.run(job: job, onProgress: { [weak self] progress in
                guard let self, self.job === job else { return }
                self.status = .analyzing(folderName: folder.name, progress: progress)
            }) { engine, job in
                try engine.analyzeFolder(folder: path, recursive: false, job: job)
            }
            guard self.job === job else { return }
            status = .analyzed(
                FolderSummary(folderName: folder.name, record: record, categories: categories),
                folder: folder
            )
        } catch AppError.Cancelled {
            if self.job === job { status = .idle }
        } catch {
            if self.job === job { status = .failed(userMessage(for: error)) }
        }
    }

    func cancelAnalysis() {
        job?.cancel()
    }

    // MARK: Preview → apply → undo

    func preview() async {
        guard let engine, case let .analyzed(summary, folder) = status, let destination else { return }
        let destinations = DestinationsRecord(defaultFolder: destination.path, categories: [])
        let path = folder.path
        do {
            let plan = try await engine.run {
                try $0.planWorkspace(folder: path, destinations: destinations, onlyPaths: [])
            }
            skipped = []
            status = .previewing(PlanPreview(record: plan, categories: categories), summary: summary, folder: folder)
        } catch {
            status = .failed(userMessage(for: error))
        }
    }

    func backToSummary() {
        guard case let .previewing(_, summary, folder) = status else { return }
        status = .analyzed(summary, folder: folder)
    }

    func apply() async {
        guard let engine, case let .previewing(preview, _, _) = status else { return }
        let skip = skipped.sorted().map(UInt32.init)
        let planID = preview.planID
        await finish(undone: false, "Moving files…") {
            try await engine.run { try $0.applyPlan(planId: planID, skip: skip) }
        }
    }

    func undo() async {
        guard let engine, isReady else { return }
        await finish(undone: true, "Putting files back…") {
            try await engine.run { try $0.undoLast() }
        }
    }

    private func finish(undone: Bool, _ working: String, _ work: () async throws -> ApplyRecord) async {
        status = .working(working)
        do {
            status = .finished(Outcome(record: try await work(), undone: undone))
        } catch {
            status = .failed(userMessage(for: error))
        }
        if let engine {
            canUndo = await engine.run { $0.canUndo() }
        }
    }
}
