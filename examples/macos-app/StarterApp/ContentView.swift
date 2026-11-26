import AppCore
import SwiftUI

struct ContentView: View {
    @Bindable var model: AnalysisModel

    /// Which folder the open panel is choosing (one `fileImporter` per view).
    private enum PickerTarget { case workspace, destination }
    @State private var picking: PickerTarget?

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            header
            banner
            folders
            Divider()
            result
            Spacer(minLength: 0)
            if model.canUndo && !model.isBusy {
                Button("Undo Last Change") { Task { await model.undo() } }
                    .keyboardShortcut("z", modifiers: .command)
            }
        }
        .padding(28)
        .fileImporter(
            isPresented: Binding(get: { picking != nil }, set: { if !$0 { picking = nil } }),
            allowedContentTypes: [.folder]
        ) { picked in
            guard case let .success(url) = picked, let target = picking else { return } // cancelled
            switch target {
            case .workspace: Task { await model.grantAndAnalyze(url) }
            case .destination: model.chooseDestination(url)
            }
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Tidy a folder")
                .font(.largeTitle.weight(.semibold))
            Text("Everything runs on this Mac. Nothing moves until you apply, and you can undo.")
                .foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private var banner: some View {
        if let error = model.startupError {
            Label(error, systemImage: "exclamationmark.octagon.fill")
                .foregroundStyle(.red)
        } else if !model.notices.isEmpty {
            Label(model.notices.joined(separator: "\n"), systemImage: "exclamationmark.triangle.fill")
                .foregroundStyle(.orange)
        }
    }

    private var folders: some View {
        Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 8) {
            GridRow {
                Text("Folder").foregroundStyle(.secondary)
                HStack {
                    ForEach(model.workspaces) { folder in
                        Button(folder.name) { Task { await model.analyze(folder) } }
                            .contextMenu {
                                Button("Forget", role: .destructive) { model.forget(folder) }
                            }
                            .help("Analyse again. Right-click to remove the app's access.")
                    }
                    Button("Choose…") { picking = .workspace }
                }
                .disabled(!model.isReady)
            }
            GridRow {
                Text("File into").foregroundStyle(.secondary)
                HStack {
                    Text(model.destination?.name ?? "Not chosen")
                        .foregroundStyle(model.destination == nil ? .secondary : .primary)
                    Button("Choose…") { picking = .destination }
                        .disabled(model.isBusy)
                }
            }
        }
    }

    @ViewBuilder
    private var result: some View {
        switch model.status {
        case .idle:
            Text("Choose a folder to see what's in it.")
                .foregroundStyle(.secondary)
        case let .analyzing(name, progress):
            AnalyzingView(folderName: name, progress: progress, cancel: model.cancelAnalysis)
        case let .analyzed(summary, _):
            VStack(alignment: .leading, spacing: 16) {
                SummaryView(summary: summary)
                Button("Preview Changes") { Task { await model.preview() } }
                    .buttonStyle(.borderedProminent)
                    .disabled(model.destination == nil)
                if model.destination == nil {
                    Text("Choose where to file things first.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
            }
        case let .previewing(preview, _, _):
            PreviewView(preview: preview, skipped: $model.skipped) {
                Task { await model.apply() }
            } back: {
                model.backToSummary()
            }
        case let .working(text):
            ProgressView(text)
        case let .finished(outcome):
            Label(outcome.lines.joined(separator: "\n"),
                  systemImage: outcome.hasProblems ? "exclamationmark.triangle.fill" : "checkmark.circle.fill")
                .foregroundStyle(outcome.hasProblems ? .orange : .green)
        case let .failed(message):
            Label(message, systemImage: "xmark.circle.fill")
                .foregroundStyle(.red)
        }
    }
}

private struct AnalyzingView: View {
    let folderName: String
    let progress: ProgressRecord
    let cancel: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let fraction = progress.fraction {
                ProgressView(value: fraction) {
                    Text("Analysing \(folderName)… \(progress.done) of \(progress.total) files")
                }
            } else {
                ProgressView("Looking for files in \(folderName)…")
            }
            Button("Cancel", action: cancel)
                .keyboardShortcut(.cancelAction)
        }
    }
}

private struct SummaryView: View {
    let summary: FolderSummary

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(heading).font(.headline)
            Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 4) {
                ForEach(summary.categories) { count in
                    GridRow {
                        Text(count.title)
                        Text(count.files, format: .number).monospacedDigit()
                            .gridColumnAlignment(.trailing)
                        Text(Int64(count.bytes), format: .byteCount(style: .file))
                            .foregroundStyle(.secondary)
                            .gridColumnAlignment(.trailing)
                    }
                }
            }
        }
    }

    private var heading: String {
        summary.unreadable == 0
            ? String(localized: "\(summary.files) files in \(summary.folderName)")
            : String(localized: "\(summary.files) files in \(summary.folderName) (\(summary.unreadable) couldn't be read)")
    }
}
