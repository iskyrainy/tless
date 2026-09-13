//! Page file handling behind `tless page`.

use std::{fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use tracing::info;

use crate::file::{ValidEntity, current_timestamp, get_path};

pub struct Page;

impl ValidEntity for Page {
    fn validate_and_get_path(name: &str) -> Result<PathBuf> {
        let slug = Self::validate_name(name)?;
        let file_path = get_path(&slug, "page");
        if file_path.exists() {
            bail!("Page already exists.");
        }
        Ok(file_path)
    }
}

impl Page {
    /// Add a new page file.
    pub fn add(name: &str) -> Result<()> {
        let file_path = Self::validate_and_get_path(name)?;
        fs::write(&file_path, Self::base_page_text(name))
            .context(format!("Failed to write new page: {}", file_path.display()))?;
        info!("Page '{}' created", file_path.display());
        Ok(())
    }

    #[inline]
    fn base_page_text(name: &str) -> String {
        format!(
            "---\ntitle: {}\ndate: {}\nlayout: page.html\n---\n",
            name,
            current_timestamp()
        )
    }

    /// Remove an existing page file.
    pub fn remove(name: &str) -> Result<()> {
        let slug = Self::validate_name(name)?;
        let file_path = get_path(&slug, "page");
        if !file_path.exists() {
            bail!("Page does not exist.");
        }
        fs::remove_file(&file_path)
            .context(format!("Failed to remove page/: {}", file_path.display()))?;
        info!("Page '{}' removed", name);
        Ok(())
    }
}
