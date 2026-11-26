use std::path::PathBuf;

/// Folder name under the per-user local app data directory.
const APP_FOLDER: &str = "AppStarter";

/// Where the app stores its own settings and undo history.
///
/// Inside an MSIX package, writes under `%LOCALAPPDATA%` are transparently
/// redirected to the package's private `LocalCache` folder, which Windows
/// removes on uninstall — the behaviour the Microsoft Store expects.
pub fn resolve() -> Option<PathBuf> {
    under(
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        APP_FOLDER,
    )
}

/// The folder the app offers to tidy first (`%USERPROFILE%\Downloads`).
pub fn default_inbox() -> Option<PathBuf> {
    under(user_profile(), "Downloads")
}

/// Where category folders are created (`%USERPROFILE%\Documents\AppStarter`).
pub fn default_destination() -> Option<PathBuf> {
    under(user_profile(), "Documents").map(|docs| docs.join(APP_FOLDER))
}

fn user_profile() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

fn under(base: Option<PathBuf>, child: &str) -> Option<PathBuf> {
    base.filter(|path| path.is_absolute())
        .map(|path| path.join(child))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_child_to_absolute_bases_only() {
        let base = std::env::temp_dir();
        assert_eq!(
            under(Some(base.clone()), APP_FOLDER),
            Some(base.join(APP_FOLDER))
        );
        assert_eq!(under(None, APP_FOLDER), None);
        assert_eq!(under(Some(PathBuf::from("relative")), APP_FOLDER), None);
    }
}
