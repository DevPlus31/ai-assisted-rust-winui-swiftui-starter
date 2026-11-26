import AppCore
import Foundation

/// Files of one category found in a folder.
struct CategoryCount: Identifiable, Equatable {
    let title: String
    let files: Int
    let bytes: UInt64
    var id: String { title }
}

/// The result of analysing one folder.
struct FolderSummary: Equatable {
    let folderName: String
    let files: Int
    /// Non-empty categories, largest first.
    let categories: [CategoryCount]
    /// Files the engine couldn't read.
    let unreadable: Int

    init(folderName: String, record: AnalysisRecord, categories: [CategoryInfo]) {
        let grouped = Dictionary(grouping: record.files, by: \.category)
        self.folderName = folderName
        files = record.files.count
        unreadable = record.failures.count
        // Engine order (`fileCategories()`) breaks ties; the sort is stable.
        self.categories = categories
            .compactMap { info in
                grouped[info.category].map { files in
                    CategoryCount(title: info.title, files: files.count, bytes: files.reduce(0) { $0 + $1.size })
                }
            }
            .sorted { $0.files > $1.files }
    }
}

/// A plan the user reviews before anything changes. The engine executes it
/// by id; the app can untick moves but never hands the engine paths.
struct PlanPreview: Equatable {
    struct Row: Identifiable, Equatable {
        /// Index into the plan, as `applyPlan(skip:)` expects.
        let id: Int
        let fromName: String
        let toName: String
        let toFolder: String
    }

    let planID: UInt64
    let rows: [Row]
    /// Files no rule would change.
    let unchanged: UInt64
    /// Titles of categories left in place for lack of a destination.
    let missing: [String]

    init(record: PlanRecord, categories: [CategoryInfo]) {
        planID = record.id
        rows = record.moves.enumerated().map { index, move in
            let to = URL(filePath: move.to)
            return Row(
                id: index,
                fromName: URL(filePath: move.from).lastPathComponent,
                toName: to.lastPathComponent,
                toFolder: to.deletingLastPathComponent().lastPathComponent
            )
        }
        unchanged = record.unchanged
        missing = record.missingCategories.compactMap { category in
            categories.first { $0.category == category }?.title
        }
    }
}

/// What an apply or undo did, ready to show.
struct Outcome: Equatable {
    let lines: [String]
    let hasProblems: Bool

    init(record: ApplyRecord, undone: Bool) {
        // Whole sentences, so translators never have to stitch fragments.
        var lines = [
            undone
                ? String(localized: "Put back \(record.moved) files.")
                : String(localized: "Moved \(record.moved) files."),
        ]
        if record.copied > 0 {
            lines.append(String(localized: "Copied \(record.copied) files whose folder couldn't be changed."))
        }
        if let warning = record.warning {
            lines.append(localized(warning))
        }
        lines += record.failures.map { "\(URL(filePath: $0.path).lastPathComponent): \(localized($0.message))" }
        self.lines = lines
        hasProblems = record.warning != nil || !record.failures.isEmpty
    }
}
