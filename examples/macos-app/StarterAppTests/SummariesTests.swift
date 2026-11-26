import AppCore
import Foundation
import Testing

/// What the window shows, built from engine records (no engine needed).
struct SummariesTests {
    private let categories = fileCategories()

    private func file(_ name: String, _ category: FileCategory, size: UInt64) -> FileRecord {
        FileRecord(
            path: "/inbox/\(name)", name: name, size: size, category: category,
            categoryLabel: "", confidence: 1, reason: "", date: nil,
            suggestedName: name, width: nil, height: nil
        )
    }

    private func text(_ key: String, _ template: String, _ args: [TextArgRecord] = []) -> UserTextRecord {
        UserTextRecord(key: key, template: template, args: args, text: template)
    }

    @Test func groupsFilesByCategoryLargestFirst() {
        let record = AnalysisRecord(
            files: [
                file("a.mp3", .audio, size: 10),
                file("b.mp3", .audio, size: 5),
                file("c.zip", .archive, size: 7),
            ],
            failures: [FailureRecord(path: "/inbox/d", message: text("file-missing", "The file no longer exists."))]
        )

        let summary = FolderSummary(folderName: "inbox", record: record, categories: categories)

        #expect(summary.files == 3)
        #expect(summary.unreadable == 1)
        #expect(summary.categories.map(\.files) == [2, 1])
        #expect(summary.categories.first?.bytes == 15)
        #expect(summary.categories.first?.title == categories.first { $0.category == .audio }?.title)
    }

    @Test func previewRowsKeepThePlanIndexAndNameMissingCategories() {
        let plan = PlanRecord(
            id: 7,
            moves: [
                MoveRecord(from: "/inbox/a.mp3", to: "/Organized/Audio/a.mp3", ruleName: "File by category"),
                MoveRecord(from: "/inbox/scan.pdf", to: "/Organized/Invoices/2026-01-02 Invoice.pdf", ruleName: "File by category"),
            ],
            unchanged: 3,
            missingCategories: [.video]
        )

        let preview = PlanPreview(record: plan, categories: categories)

        #expect(preview.planID == 7)
        #expect(preview.rows.map(\.id) == [0, 1], "ids are the indexes applyPlan(skip:) expects")
        #expect(preview.rows[1].fromName == "scan.pdf")
        #expect(preview.rows[1].toName == "2026-01-02 Invoice.pdf")
        #expect(preview.rows[1].toFolder == "Invoices")
        #expect(preview.unchanged == 3)
        #expect(preview.missing == [categories.first { $0.category == .video }?.title])
    }

    @Test func outcomeListsCopiesWarningsAndFailuresInEnglishWithoutTranslations() {
        let record = ApplyRecord(
            moved: 2,
            copied: 1,
            failures: [FailureRecord(path: "/inbox/locked.pdf", message: text("no-permission", "The app doesn't have permission to change this file."))],
            warning: text("restored-with-new-name", "{count} file(s) were restored with a new name.", [TextArgRecord(name: "count", value: "1")])
        )

        let outcome = Outcome(record: record, undone: true)

        #expect(outcome.hasProblems)
        #expect(outcome.lines == [
            "Put back 2 files.",
            "Copied 1 files whose folder couldn't be changed.",
            "1 file(s) were restored with a new name.",
            "locked.pdf: The app doesn't have permission to change this file.",
        ])
    }

    @Test func aCleanApplyHasNoProblems() {
        let outcome = Outcome(record: ApplyRecord(moved: 4, copied: 0, failures: [], warning: nil), undone: false)
        #expect(!outcome.hasProblems)
        #expect(outcome.lines == ["Moved 4 files."])
    }
}
