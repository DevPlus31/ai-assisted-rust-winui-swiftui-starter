//! Where organized files go: a folder of the user's choice per category
//! (Invoices → `~/Documents/Finance`, Photos → `~/Pictures`, …), plus an
//! optional default folder. Categories without their own folder get a
//! subfolder named after them inside the default folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::analyze::Category;
use crate::error::CoreError;
use crate::user_text::{TextKey, UserText};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Destinations {
    /// Holds `<Category folder>` subfolders for categories without their own.
    #[serde(default)]
    pub default_root: Option<PathBuf>,
    /// Files of these categories go straight into the given folder.
    #[serde(default)]
    pub by_category: BTreeMap<Category, PathBuf>,
}

impl Destinations {
    /// Everything into `<root>/<Category folder>`.
    pub fn single(root: impl Into<PathBuf>) -> Self {
        Self {
            default_root: Some(root.into()),
            by_category: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with(self, category: Category, folder: impl Into<PathBuf>) -> Self {
        let mut by_category = self.by_category;
        by_category.insert(category, folder.into());
        Self {
            by_category,
            ..self
        }
    }

    pub fn is_empty(&self) -> bool {
        self.default_root.is_none() && self.by_category.is_empty()
    }

    /// The folder a file of `category` is filed into, if any.
    pub fn category_folder(&self, category: Category) -> Option<PathBuf> {
        self.by_category.get(&category).cloned().or_else(|| {
            self.default_root
                .as_ref()
                .map(|root| root.join(category.folder()))
        })
    }

    /// Base for rules with a fixed relative folder: the default folder,
    /// else the category's own folder.
    pub fn base_for(&self, category: Category) -> Option<&Path> {
        self.default_root
            .as_deref()
            .or_else(|| self.by_category.get(&category).map(PathBuf::as_path))
    }

    /// # Errors
    /// [`CoreError::InvalidInput`] when nothing is set or a folder isn't an
    /// absolute path.
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.is_empty() {
            return Err(CoreError::invalid(TextKey::ChooseDestination));
        }
        let relative = self
            .default_root
            .iter()
            .chain(self.by_category.values())
            .find(|folder| !folder.is_absolute());
        match relative {
            Some(folder) => Err(CoreError::InvalidInput(
                UserText::new(TextKey::FolderNotAbsolute)
                    .arg("folder", folder.display().to_string()),
            )),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_folders_win_over_the_default_folder() {
        let destinations = Destinations::single("/org").with(Category::Photo, "/pictures");

        assert_eq!(
            destinations.category_folder(Category::Photo),
            Some(PathBuf::from("/pictures"))
        );
        assert_eq!(
            destinations.category_folder(Category::Invoice),
            Some(PathBuf::from("/org/Invoices"))
        );
        assert_eq!(
            destinations.base_for(Category::Photo),
            Some(Path::new("/org"))
        );
    }

    #[test]
    fn without_a_default_only_chosen_categories_have_a_folder() {
        let destinations = Destinations::default().with(Category::Invoice, "/finance");

        assert_eq!(destinations.category_folder(Category::Video), None);
        assert_eq!(
            destinations.base_for(Category::Invoice),
            Some(Path::new("/finance"))
        );
        assert_eq!(destinations.base_for(Category::Video), None);
    }

    #[test]
    fn empty_or_relative_destinations_are_rejected() {
        assert!(Destinations::default().validate().is_err());
        assert!(Destinations::single("Organized").validate().is_err());
        assert!(
            Destinations::default()
                .with(Category::Photo, "Pictures")
                .validate()
                .is_err()
        );
        assert!(Destinations::single("/org").validate().is_ok());
    }

    #[test]
    fn round_trips_through_json() {
        let destinations = Destinations::single("/org").with(Category::BankStatement, "/bank");
        let json = serde_json::to_string(&destinations).unwrap();
        assert_eq!(
            serde_json::from_str::<Destinations>(&json).unwrap(),
            destinations
        );
        assert_eq!(
            serde_json::from_str::<Destinations>("{}").unwrap(),
            Destinations::default()
        );
    }
}
