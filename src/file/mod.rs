//! Blog/page file operations: add, remove, and parse frontmatter metadata.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::{BASE_DIR, config, util::slugify};

mod blog;
mod page;

pub use blog::Blog;
pub use page::Page;

/// Metadata parsed from a source file's frontmatter.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Metadata {
    pub title: String,
    pub date: String,
    pub layout: Option<String>,
    pub tag: Option<Vec<String>>,
    pub category: Option<Vec<String>>,
    pub path: PathBuf,
}

/// Path to a source file (`source/<class>/<name>.md`).
#[inline]
pub(crate) fn get_path(name: &str, class: &str) -> PathBuf {
    BASE_DIR
        .join("source")
        .join(class)
        .join(name)
        .with_extension("md")
}

/// Current timestamp formatted in the configured `[site] zone`, falling back to UTC.
#[inline]
pub(crate) fn current_timestamp() -> String {
    Utc::now().with_timezone(&config::zone()).to_rfc3339()
}

/// Parse the frontmatter and file name of a source file into [Metadata].
pub fn parse_file(path: &Path) -> Result<(Metadata, String)> {
    let mut file =
        fs::File::open(path).context(format!("Failed to open file: {}", path.display()))?;
    let mut text = String::new();
    if file.read_to_string(&mut text).is_err() {
        bail!("Failed to read blog.");
    }
    let (frontmatter, md_body) = frontmatter_gen::extract(&text).context(format!(
        "Failed to extract file frontmatter: {}",
        path.display()
    ))?;
    let mut metadata = Metadata {
        path: path.to_path_buf(),
        ..Default::default()
    };
    if let Some(title) = frontmatter.get("title").and_then(|v| v.as_str()) {
        metadata.title = title.to_string();
    } else {
        metadata.title = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
    }
    if let Some(date) = frontmatter.get("date").and_then(|v| v.as_str()) {
        metadata.date = date.to_string();
    }
    if let Some(layout) = frontmatter.get("layout").and_then(|v| v.as_str()) {
        metadata.layout = Some(layout.to_string());
    }
    if let Some(tags) = frontmatter.get("tag").and_then(|v| v.as_array()) {
        let tag_list = tags
            .iter()
            .filter_map(|t| t.as_str().map(|s| s.to_string()))
            .collect();
        metadata.tag = Some(tag_list);
    }
    if let Some(category) = frontmatter.get("category").and_then(|v| v.as_array()) {
        let category_list = category
            .iter()
            .filter_map(|c| c.as_str().map(|s| s.to_string()))
            .collect();
        metadata.category = Some(category_list);
    }
    Ok((metadata, md_body.to_string()))
}

pub(crate) trait ValidEntity {
    fn validate_and_get_path(name: &str) -> Result<PathBuf>;

    /// Reject empty or oversized names and return their slug.
    fn validate_name(name: &str) -> Result<String> {
        if name.trim().is_empty() {
            bail!("Name cannot be empty");
        }
        if name.len() > 100 {
            bail!("Name is too long: {0} characters (max: 100)", name.len());
        }
        let slug = slugify(name);
        if slug.is_empty() {
            bail!("Invalid characters in name");
        }
        Ok(slug)
    }
}
