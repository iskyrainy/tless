use std::fs;

use anyhow::{Result, bail};
use tracing::info;

use crate::file::{ValidEntity, current_timestamp, get_path, is_file_exist};

pub struct Page;

impl ValidEntity for Page {
    fn validate_and_get_path(name: &str) -> Result<std::path::PathBuf> {
        if name.trim().is_empty() {
            bail!("Name cannot be empty");
        }

        if name.len() > 100 {
            bail!("Name is too long: {0} characters (max: 100)", name.len());
        }

        let slug = Self::generate_slug(name);

        if slug.is_empty() {
            bail!("Invalid characters in name");
        }

        let file_path = get_path(&slug, "page");
        if is_file_exist(&file_path) {
            bail!("Page already exists.");
        }
        Ok(file_path)
    }
}

impl Page {
    /// Add a new page file.
    pub fn add(name: &str) -> Result<()> {
        let file_path = get_path(name, "page");
        fs::write(&file_path, Self::base_page_text(name))?;
        info!("Page '{}' created", file_path.display());
        Ok(())
    }

    fn base_page_text(name: &str) -> String {
        format!(
            "---\ntitle: {}\ndate: {}\nlayout: page.html\n---\n",
            name,
            current_timestamp()
        )
    }

    /// Remove an existing page file.
    pub fn remove(name: &str) -> Result<()> {
        let file_path = get_path(name, "page");
        if !is_file_exist(&file_path) {
            bail!("Page does not exist.");
        }
        fs::remove_file(file_path)?;
        info!("Page '{}' removed", name);
        Ok(())
    }
}
