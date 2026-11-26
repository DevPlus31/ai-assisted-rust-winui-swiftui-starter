//! Every sentence the engine shows to people, with a stable key so shells
//! can translate it.
//!
//! The engine never hands shells finished prose only. A [`UserText`]
//! carries a [`TextKey`], the English template and named arguments.
//! Shells look the key up in their own translations (a String Catalog on
//! macOS, a resource table on Windows), fill in the arguments and fall
//! back to the English template. `l10n/engine-messages.json` lists every
//! key and template for translators; a test keeps it in sync
//! (`make l10n` rewrites it).

use std::fmt;

macro_rules! text_keys {
    ($($variant:ident => $id:literal, $template:literal;)+) => {
        /// One translatable sentence. `id` is stable: never reuse or rename
        /// one, because translations are keyed by it.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum TextKey {
            $($variant,)+
        }

        impl TextKey {
            pub const ALL: &[Self] = &[$(Self::$variant,)+];

            /// Stable key for translation tables, e.g. `preview-out-of-date`.
            pub fn id(self) -> &'static str {
                match self {
                    $(Self::$variant => $id,)+
                }
            }

            /// English text with `{name}` placeholders for the arguments.
            pub fn template(self) -> &'static str {
                match self {
                    $(Self::$variant => $template,)+
                }
            }
        }
    };
}

text_keys! {
    // Choosing folders
    FolderRequired => "folder-required", "Choose a folder first.";
    ChooseDestination => "choose-destination", "Choose where the app should file things first.";
    FolderNotAbsolute => "folder-not-absolute", "“{folder}” isn't a full folder path.";
    DataDirNotAbsolute => "data-dir-not-absolute", "The data location must be an absolute path.";
    FolderNameNotPortable => "folder-name-not-portable", "“{folder}” isn't a valid folder name on both macOS and Windows.";
    WorkspaceChanged => "workspace-changed", "This folder changed since it was analysed. Please refresh and try again.";
    FolderUnreadable => "folder-unreadable", "The folder could not be read.";
    // Rules and collections
    NameRequired => "name-required", "Every rule and collection needs a name.";
    IdsNotUnique => "ids-not-unique", "Rule and collection ids must be unique.";
    RuleMissing => "rule-missing", "That rule no longer exists.";
    // Plan, apply, undo
    PreviewOutOfDate => "preview-out-of-date", "This preview is out of date. Please review it again.";
    NothingToUndo => "nothing-to-undo", "Nothing to undo.";
    Cancelled => "cancelled", "Cancelled.";
    UndoNotSavedAfterApply => "undo-not-saved-after-apply", "Changes were made, but undo history could not be saved; undo works until the app quits.";
    UndoHistoryNotSaved => "undo-history-not-saved", "Undo history could not be saved.";
    RestoredWithNewName => "restored-with-new-name", "{count} file(s) were restored with a new name because a different file now uses the original name.";
    // App data
    SaveFailed => "save-failed", "The app could not save its settings.";
    SettingsSetAside => "settings-set-aside", "Saved settings could not be read and were set aside.";
    RulesReset => "rules-reset", "Your rules could not be read, so the defaults were restored. The old file was kept.";
    UndoHistoryCleared => "undo-history-cleared", "Undo history could not be read and was cleared.";
    // One file
    FileChangedSincePreview => "file-changed-since-preview", "The file changed after the preview, so it was left alone.";
    FileMissing => "file-missing", "The file no longer exists.";
    NoPermission => "no-permission", "The app doesn't have permission to change this file.";
    FileExists => "file-exists", "A file with that name already exists.";
    DiskFull => "disk-full", "The disk is full.";
    ContentsUnreadable => "contents-unreadable", "The file's contents could not be read.";
    FileNotProcessed => "file-not-processed", "The file could not be processed.";
    ImageUnreadable => "image-unreadable", "The image could not be read.";
    ImageNotConverted => "image-not-converted", "The image could not be converted.";
}

/// A sentence for people: a key, its arguments and the English rendering
/// (via `Display`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserText {
    pub key: TextKey,
    /// Values for the template's `{name}` placeholders.
    pub args: Vec<(&'static str, String)>,
}

impl UserText {
    pub fn new(key: TextKey) -> Self {
        Self {
            key,
            args: Vec::new(),
        }
    }

    #[must_use]
    pub fn arg(self, name: &'static str, value: impl Into<String>) -> Self {
        let mut args = self.args;
        args.push((name, value.into()));
        Self { args, ..self }
    }

    /// Fills `template` (English or a translation of this key) with the
    /// arguments. Placeholders without a value are left as they are.
    pub fn render(&self, template: &str) -> String {
        self.args
            .iter()
            .fold(template.to_owned(), |text, (name, value)| {
                text.replace(&format!("{{{name}}}"), value)
            })
    }
}

impl From<TextKey> for UserText {
    fn from(key: TextKey) -> Self {
        Self::new(key)
    }
}

impl fmt::Display for UserText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render(self.key.template()))
    }
}

/// The translators' file: every key with its English template, sorted.
pub fn catalog_json() -> String {
    let entries: std::collections::BTreeMap<&str, &str> = TextKey::ALL
        .iter()
        .map(|key| (key.id(), key.template()))
        .collect();
    let mut json = serde_json::to_string_pretty(&entries).unwrap_or_default();
    json.push('\n');
    json
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    const CATALOG: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../l10n/engine-messages.json"
    );

    #[test]
    fn renders_english_with_arguments() {
        let text = UserText::new(TextKey::FolderNotAbsolute).arg("folder", "Docs");
        assert_eq!(text.to_string(), "“Docs” isn't a full folder path.");
    }

    #[test]
    fn renders_a_translation_with_the_same_arguments() {
        let text = UserText::new(TextKey::RestoredWithNewName).arg("count", "3");
        assert_eq!(
            text.render("{count} Dateien wurden umbenannt."),
            "3 Dateien wurden umbenannt."
        );
    }

    #[test]
    fn keys_are_unique() {
        let ids: HashSet<_> = TextKey::ALL.iter().map(|key| key.id()).collect();
        assert_eq!(ids.len(), TextKey::ALL.len());
    }

    /// Fails when a key was added or changed without regenerating the
    /// translators' file. Fix: `make l10n`.
    #[test]
    fn translators_catalog_is_up_to_date() {
        let expected = catalog_json();
        if std::env::var_os("UPDATE_L10N").is_some() {
            std::fs::write(CATALOG, &expected).unwrap();
        }
        let committed = std::fs::read_to_string(CATALOG).unwrap_or_default();
        assert!(
            committed == expected,
            "l10n/engine-messages.json is out of date; run `make l10n`"
        );
    }
}
