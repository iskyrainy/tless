use std::fs;

use anyhow::{Result, bail};

use crate::file::{current_timestamp, get_path, is_file_exist};

/// Add a new page file.
pub fn add_page(name: &str) -> Result<()> {
    let file_path = get_path(name, "page");
    if is_file_exist(&file_path) {
        bail!("Page already exists.");
    }
    fs::write(&file_path, base_page_text(name))?;
    Ok(())
}

fn base_page_text(name: &str) -> String {
    format!(
        "---\ntitle: {}\ndate: {}\nlayout: index.html\n---\n",
        name,
        current_timestamp()
    )
}

/// Remove an existing page file.
pub fn remove_page(name: &str) -> Result<()> {
    let file_path = get_path(name, "page");
    if !is_file_exist(&file_path) {
        bail!("Page does not exist.");
    }
    fs::remove_file(file_path)?;
    Ok(())
}
