//! Blog/page file operations: add, remove, and parse frontmatter metadata.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::{BASE_DIR, server::SITE};

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

impl Metadata {
    pub fn new() -> Self {
        Metadata::default()
    }
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

/// Check whether a file exists.
#[inline]
pub(crate) fn is_file_exist(path: &Path) -> bool {
    path.exists()
}

/// Current timestamp formatted in the configured `[site] zone`, falling back to UTC.
#[inline]
pub(crate) fn current_timestamp() -> String {
    let site = SITE.load();
    Utc::now().with_timezone(&site.get_zone()).to_rfc3339()
}

/// Parse the frontmatter and file name of a source file into [Metadata].
pub fn parse_file(path: &PathBuf) -> Result<(Metadata, String)> {
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
    let mut metadata = Metadata::new();
    metadata.path = path.clone();
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

    #[inline]
    fn slugify(input: &str) -> String {
        let mut slug = String::new();
        let mut prev_dash = false;
        for ch in input.chars() {
            let lower = ch.to_ascii_lowercase();
            if lower.is_ascii_alphanumeric() {
                slug.push(lower);
                prev_dash = false;
            } else if !prev_dash && !slug.is_empty() {
                slug.push('-');
                prev_dash = true;
            }
        }
        slug.trim_matches('-').to_string()
    }
}
