import Foundation
import Testing
import AppCore

@MainActor
final class FolderStoreTests {
    private let root = FileManager.default.temporaryDirectory
        .appending(path: "FolderStoreTests-\(UUID().uuidString)", directoryHint: .isDirectory)
    private var file: URL { root.appending(path: "grants.json") }

    init() throws {
        for name in ["Downloads", "Organized", "Photos"] {
            try FileManager.default.createDirectory(at: folder(name), withIntermediateDirectories: true)
        }
    }

    deinit {
        try? FileManager.default.removeItem(at: root)
    }

    private func folder(_ name: String) -> URL {
        root.appending(path: name, directoryHint: .isDirectory)
    }

    private func makeStore() -> FolderStore {
        FolderStore(file: file, securityScoped: false)
    }

    @Test func grantsPersistAcrossLaunches() throws {
        let store = makeStore()
        try store.grant(folder("Downloads"), role: .workspace)
        try store.grant(folder("Organized"), role: .destination)
        try store.grant(folder("Photos"), role: .destination, category: FileCategory.photo.storageKey)

        let reloaded = makeStore()

        #expect(reloaded.workspaces.map(\.name) == ["Downloads"])
        #expect(reloaded.defaultDestination?.name == "Organized")
        #expect(reloaded.destinations.first { $0.category == "photo" }?.name == "Photos")
        #expect(reloaded.unavailable.isEmpty)
    }

    @Test func aNewDestinationReplacesTheOneForTheSameCategory() throws {
        let store = makeStore()
        try store.grant(folder("Organized"), role: .destination)
        try store.grant(folder("Photos"), role: .destination) // new default folder
        try store.grant(folder("Organized"), role: .destination, category: "invoice")
        try store.grant(folder("Photos"), role: .destination, category: "invoice")

        #expect(store.destinations.count == 2)
        #expect(store.defaultDestination?.name == "Photos")
        #expect(store.destinations.first { $0.category == "invoice" }?.name == "Photos")
    }

    @Test func grantingTheSameFolderTwiceKeepsOneEntry() throws {
        let store = makeStore()
        let first = try store.grant(folder("Downloads"), role: .workspace)
        let second = try store.grant(folder("Downloads"), role: .workspace)

        #expect(first.id == second.id)
        #expect(store.folders.count == 1)
    }

    @Test func workspacesNeverCarryACategory() throws {
        let granted = try makeStore().grant(folder("Downloads"), role: .workspace, category: "photo")
        #expect(granted.category == nil)
    }

    @Test func revokedGrantsStayGone() throws {
        let store = makeStore()
        let granted = try store.grant(folder("Downloads"), role: .workspace)

        try store.revoke(id: granted.id)

        #expect(store.folders.isEmpty)
        #expect(makeStore().folders.isEmpty)
    }

    @Test func aCorruptFileIsSetAsideNotOverwritten() throws {
        try Data("{broken".utf8).write(to: file)

        let store = makeStore()

        #expect(store.folders.isEmpty)
        let names = try FileManager.default.contentsOfDirectory(atPath: root.path(percentEncoded: false))
        #expect(names.contains { $0.hasPrefix("grants.corrupt-") })
    }
}
