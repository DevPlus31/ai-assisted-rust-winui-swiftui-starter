import SwiftUI

/// The reviewable plan: one checkbox per move, then Apply or Back.
struct PreviewView: View {
    let preview: PlanPreview
    @Binding var skipped: Set<Int>
    let apply: () -> Void
    let back: () -> Void

    private var selected: Int { preview.rows.count - skipped.count }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("\(selected) of \(preview.rows.count) files will move")
                .font(.headline)
            Text(notes)
                .font(.callout)
                .foregroundStyle(.secondary)
            List(preview.rows) { row in
                Toggle(isOn: isIncluded(row.id)) {
                    HStack(spacing: 6) {
                        Text(row.fromName)
                        Image(systemName: "arrow.right").foregroundStyle(.secondary)
                        Text("\(row.toFolder)/\(row.toName)").foregroundStyle(.secondary)
                    }
                    .lineLimit(1)
                    .truncationMode(.middle)
                }
            }
            .frame(minHeight: 160)
            HStack {
                Button("Apply", action: apply)
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
                    .disabled(selected == 0)
                Button("Back", action: back)
                    .keyboardShortcut(.cancelAction)
            }
        }
    }

    private var notes: String {
        var notes = [String(localized: "\(preview.unchanged) files left as they are.")]
        if !preview.missing.isEmpty {
            let missing = preview.missing.formatted(.list(type: .and))
            notes.append(String(localized: "No destination for: \(missing)."))
        }
        return notes.joined(separator: " ")
    }

    private func isIncluded(_ index: Int) -> Binding<Bool> {
        Binding(
            get: { !skipped.contains(index) },
            set: { included in
                if included { skipped.remove(index) } else { skipped.insert(index) }
            }
        )
    }
}
