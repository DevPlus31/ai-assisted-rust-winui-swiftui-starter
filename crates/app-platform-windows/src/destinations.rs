//! The destination folders as the user typed them (one default folder plus
//! one per category), saved as-is and turned into engine [`Destinations`]
//! when organizing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use app_core::analyze::Category;
use app_core::workflow::Destinations;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DestinationFields {
    /// Gets a subfolder per category that has no folder of its own.
    #[serde(default)]
    pub default_folder: String,
    #[serde(default)]
    pub by_category: BTreeMap<Category, String>,
}

impl DestinationFields {
    pub fn with_default(default_folder: String) -> Self {
        Self {
            default_folder,
            ..Self::default()
        }
    }

    /// Text of the field for `category` (`None` = the default folder).
    pub fn get(&self, category: Option<Category>) -> &str {
        match category {
            None => &self.default_folder,
            Some(category) => self.by_category.get(&category).map_or("", String::as_str),
        }
    }

    #[must_use]
    pub fn set(&self, category: Option<Category>, text: String) -> Self {
        let mut next = self.clone();
        match category {
            None => next.default_folder = text,
            Some(category) if text.trim().is_empty() => {
                next.by_category.remove(&category);
            }
            Some(category) => {
                next.by_category.insert(category, text);
            }
        }
        next
    }

    /// Blank fields are ignored.
    ///
    /// # Errors
    /// A user-facing message for a field that isn't a full path, or when
    /// every field is blank.
    pub fn parse(&self) -> Result<Destinations, String> {
        let default_root = absolute(&self.default_folder, "Default folder")?;
        let by_category = self
            .by_category
            .iter()
            .filter_map(|(category, text)| {
                absolute(text, category.folder())
                    .transpose()
                    .map(|folder| folder.map(|f| (*category, f)))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let destinations = Destinations {
            default_root,
            by_category,
        };
        if destinations.is_empty() {
            return Err("Choose where the app should file things first (Destinations).".into());
        }
        Ok(destinations)
    }

    /// Short description for the Edge Dock; empty when nothing is set.
    pub fn summary(&self) -> String {
        let own = self
            .by_category
            .values()
            .filter(|t| !t.trim().is_empty())
            .count();
        let default_name = Path::new(self.default_folder.trim())
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        match (own, default_name) {
            (0, None) => String::new(),
            (0, Some(name)) => format!("Into {name}"),
            (n, None) => format!("Sorted into {n} folder{}", plural(n)),
            (n, Some(name)) => format!("{n} own folder{}, the rest into {name}", plural(n)),
        }
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// `Ok(None)` for a blank field.
fn absolute(text: &str, field: &str) -> Result<Option<PathBuf>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let path = PathBuf::from(text);
    if path.is_absolute() {
        Ok(Some(path))
    } else {
        Err(format!("{field}: enter a full folder path."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        std::env::temp_dir()
    }

    #[test]
    fn category_fields_parse_into_destinations() {
        let pictures = root().join("Pictures");
        let fields = DestinationFields::default()
            .set(Some(Category::Photo), pictures.display().to_string())
            .set(Some(Category::Video), "  ".into());

        let destinations = fields.parse().unwrap();

        assert_eq!(destinations.default_root, None);
        assert_eq!(
            destinations.category_folder(Category::Photo),
            Some(pictures)
        );
        assert_eq!(fields.get(Some(Category::Video)), "");
        assert_eq!(fields.summary(), "Sorted into 1 folder");
    }

    #[test]
    fn blank_or_relative_fields_explain_what_to_do() {
        assert!(DestinationFields::default().parse().is_err());
        let relative = DestinationFields::default().set(Some(Category::Invoice), "Bills".into());
        assert_eq!(
            relative.parse().unwrap_err(),
            "Invoices: enter a full folder path."
        );
    }

    #[test]
    fn summary_names_the_default_folder() {
        let fields =
            DestinationFields::with_default(root().join("Organized").display().to_string());
        assert_eq!(fields.summary(), "Into Organized");
        let fields = fields.set(Some(Category::Photo), root().display().to_string());
        assert_eq!(fields.summary(), "1 own folder, the rest into Organized");
        assert_eq!(DestinationFields::default().summary(), "");
    }
}
