import Foundation

/// A folder the user granted through the open panel. Access survives
/// relaunches via an app-scoped security bookmark (App Sandbox).
public struct GrantedFolder: Identifiable, Hashable, Sendable {
    public enum Role: String, Codable, Sendable {
        /// A folder the app analyses and tidies (Desktop, Downloads, …).
        case workspace
        /// Where organized files go: the default folder (`category == nil`,
        /// gets a subfolder per category) or one category's own folder.
        case destination
    }

    public let id: UUID
    public let url: URL
    public let role: Role
    /// `FileCategory.storageKey` of a category's own destination.
    public var category: String? = nil

    public var name: String { url.lastPathComponent }
    public var path: String { url.path(percentEncoded: false) }
}

/// A saved grant whose bookmark doesn't resolve right now (unplugged
/// drive, deleted folder). Kept so it works again when the drive returns,
/// and listed so the user can remove it.
public struct UnavailableFolder: Identifiable, Hashable, Sendable {
    public let id: UUID
    public let name: String
    public let role: GrantedFolder.Role
    public var category: String? = nil
}

/// Persists folder grants as security-scoped bookmarks in the app's
/// container and keeps them accessible while the app runs.
@MainActor
public final class FolderStore {
    private struct Entry: Codable {
        let id: UUID
        let role: GrantedFolder.Role
        /// Missing in files saved before per-category destinations.
        var category: String?
        var bookmark: Data
    }

    public private(set) var folders: [GrantedFolder] = []
    public private(set) var unavailable: [UnavailableFolder] = []
    private var entries: [Entry] = []
    /// URLs whose security scope we started and must stop exactly once.
    private var accessing: [UUID: URL] = [:]
    private let file: URL
    private let bookmarkOptions: URL.BookmarkCreationOptions
    private let resolveOptions: URL.BookmarkResolutionOptions

    /// - Parameter securityScoped: `false` only in tests, where URLs inside
    ///   the container don't need a security scope.
    public init(file: URL, securityScoped: Bool = true) {
        self.file = file
        bookmarkOptions = securityScoped ? [.withSecurityScope] : []
        resolveOptions = securityScoped ? [.withSecurityScope] : []
        load()
    }

    public static func defaultFile() -> URL {
        let support = (try? FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true
        )) ?? FileManager.default.temporaryDirectory
        return support.appending(path: "AppStarter/folders.json")
    }

    public var workspaces: [GrantedFolder] { folders.filter { $0.role == .workspace } }
    public var destinations: [GrantedFolder] { folders.filter { $0.role == .destination } }
    /// Default destination (holds a subfolder per category).
    public var defaultDestination: GrantedFolder? { destinations.first { $0.category == nil } }

    /// Records access to a folder the user just picked. A destination
    /// replaces the previous one for the same category (or the previous
    /// default folder), including an unavailable one.
    @discardableResult
    public func grant(_ url: URL, role: GrantedFolder.Role, category: String? = nil) throws -> GrantedFolder {
        let category = role == .destination ? category : nil
        if let existing = folders.first(where: {
            $0.url.standardizedFileURL == url.standardizedFileURL && $0.role == role && $0.category == category
        }) {
            return existing
        }
        // Create the bookmark before touching existing grants, so a failure
        // leaves the previous destination in place.
        let bookmark = try url.bookmarkData(options: bookmarkOptions, includingResourceValuesForKeys: nil, relativeTo: nil)
        let isReplaced: (GrantedFolder.Role, String?) -> Bool = { otherRole, otherCategory in
            role == .destination && otherRole == .destination && otherCategory == category
        }
        let replaced = entries.filter { isReplaced($0.role, $0.category) }
        let entry = Entry(id: UUID(), role: role, category: category, bookmark: bookmark)
        let folder = GrantedFolder(id: entry.id, url: url, role: role, category: category)
        let newEntries = entries.filter { !isReplaced($0.role, $0.category) } + [entry]
        try save(newEntries)

        replaced.forEach { stopAccessing($0.id) }
        unavailable.removeAll { isReplaced($0.role, $0.category) }
        entries = newEntries
        folders = folders.filter { !isReplaced($0.role, $0.category) } + [folder]
        startAccessing(folder.id, url)
        return folder
    }

    /// # Throws
    /// When the updated grants can't be saved (the folder stays granted).
    public func revoke(id: UUID) throws {
        let remaining = entries.filter { $0.id != id }
        try save(remaining)
        stopAccessing(id)
        entries = remaining
        folders.removeAll { $0.id == id }
        unavailable.removeAll { $0.id == id }
    }

    private func startAccessing(_ id: UUID, _ url: URL) {
        if url.startAccessingSecurityScopedResource() {
            accessing[id] = url
        }
    }

    private func stopAccessing(_ id: UUID) {
        accessing.removeValue(forKey: id)?.stopAccessingSecurityScopedResource()
    }

    private func load() {
        guard let data = try? Data(contentsOf: file) else { return }
        guard let saved = try? JSONDecoder().decode([Entry].self, from: data) else {
            // Keep the unreadable file for support instead of overwriting it.
            let aside = file.deletingPathExtension().appendingPathExtension("corrupt-\(Int(Date().timeIntervalSince1970)).json")
            try? FileManager.default.moveItem(at: file, to: aside)
            return
        }
        var refreshed = false
        for var entry in saved {
            var isStale = false
            guard let url = try? URL(resolvingBookmarkData: entry.bookmark, options: resolveOptions,
                                     relativeTo: nil, bookmarkDataIsStale: &isStale) else {
                let name = URL.resourceValues(forKeys: [.nameKey], fromBookmarkData: entry.bookmark)?.name
                unavailable.append(UnavailableFolder(id: entry.id, name: name ?? String(localized: "Unknown folder"),
                                                     role: entry.role, category: entry.category))
                entries.append(entry)
                continue
            }
            if isStale, let fresh = try? url.bookmarkData(options: bookmarkOptions, includingResourceValuesForKeys: nil, relativeTo: nil) {
                entry.bookmark = fresh
                refreshed = true
            }
            entries.append(entry)
            folders.append(GrantedFolder(id: entry.id, url: url, role: entry.role, category: entry.category))
            startAccessing(entry.id, url)
        }
        if refreshed { try? save(entries) }
    }

    private func save(_ entries: [Entry]) throws {
        try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        try JSONEncoder().encode(entries).write(to: file, options: .atomic)
    }
}
