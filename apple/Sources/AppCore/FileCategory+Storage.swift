import Foundation

extension FileCategory {
    /// Stable key for persisting a category (e.g. in `GrantedFolder.category`).
    public var storageKey: String { String(describing: self) }
}

extension CategoryInfo: Identifiable {
    public var id: String { category.storageKey }
}
