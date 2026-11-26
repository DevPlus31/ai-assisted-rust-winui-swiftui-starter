//! Engine messages in the user's language.
//!
//! A translation file has the same shape as `l10n/engine-messages.json`
//! (key → template, with the same `{name}` placeholders). Ship them in an
//! `l10n` folder next to the app, named by language (`de-DE.json` or
//! `de.json`); [`Translations::for_languages`] picks the best match for the
//! user's UI languages. Anything missing falls back to English.

use std::collections::HashMap;
use std::path::Path;

use app_core::UserText;

#[derive(Debug, Clone, Default)]
pub struct Translations {
    templates: HashMap<String, String>,
}

impl Translations {
    /// No translations: every message is shown in English.
    pub fn english() -> Self {
        Self::default()
    }

    /// Reads a translation file.
    ///
    /// # Errors
    /// When the text isn't a JSON object of strings.
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json)
            .map(|templates| Self { templates })
            .map_err(|e| format!("invalid translation file: {e}"))
    }

    /// The first translation file in `dir` matching `languages` (most
    /// preferred first): the full tag (`de-DE.json`), then the language
    /// alone (`de.json`). English when none exists or none is valid.
    pub fn for_languages(dir: &Path, languages: &[String]) -> Self {
        languages
            .iter()
            .flat_map(|tag| {
                let base = tag.split('-').next().unwrap_or(tag);
                [tag.as_str(), base]
            })
            .find_map(|name| {
                let json = std::fs::read_to_string(dir.join(format!("{name}.json"))).ok()?;
                Self::from_json(&json).ok()
            })
            .unwrap_or_default()
    }

    /// `text` in this language, or in English when it has no translation.
    pub fn show(&self, text: &UserText) -> String {
        let template = self
            .templates
            .get(text.key.id())
            .map_or(text.key.template(), String::as_str);
        text.render(template)
    }
}

#[cfg(test)]
mod tests {
    use app_core::TextKey;

    use super::*;

    #[test]
    fn falls_back_to_english() {
        let text = UserText::new(TextKey::FolderNotAbsolute).arg("folder", "Docs");
        assert_eq!(
            Translations::english().show(&text),
            "“Docs” isn't a full folder path."
        );
    }

    #[test]
    fn uses_a_translation_with_its_arguments() {
        let german = Translations::from_json(
            r#"{"folder-not-absolute": "„{folder}“ ist kein vollständiger Ordnerpfad."}"#,
        )
        .unwrap();
        let text = UserText::new(TextKey::FolderNotAbsolute).arg("folder", "Docs");
        assert_eq!(
            german.show(&text),
            "„Docs“ ist kein vollständiger Ordnerpfad."
        );
        assert_eq!(
            german.show(&TextKey::NothingToUndo.into()),
            "Nothing to undo.",
            "untranslated keys stay English"
        );
    }

    #[test]
    fn the_english_catalog_is_a_valid_translation_file() {
        let catalog = include_str!("../../../l10n/engine-messages.json");
        assert!(Translations::from_json(catalog).is_ok());
    }

    #[test]
    fn picks_the_best_file_for_the_user_languages() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("de.json"),
            r#"{"nothing-to-undo": "Nichts rückgängig zu machen."}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("fr.json"), "not json").unwrap();
        let undo = UserText::from(TextKey::NothingToUndo);

        let languages = |tags: &[&str]| tags.iter().map(ToString::to_string).collect::<Vec<_>>();
        let german = Translations::for_languages(dir.path(), &languages(&["de-AT", "en-US"]));
        assert_eq!(german.show(&undo), "Nichts rückgängig zu machen.");

        let broken_french = Translations::for_languages(dir.path(), &languages(&["fr-FR"]));
        assert_eq!(broken_french.show(&undo), "Nothing to undo.");
    }

    #[test]
    fn rejects_a_malformed_file() {
        assert!(Translations::from_json("[1, 2]").is_err());
    }
}
