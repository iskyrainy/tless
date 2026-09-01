//! Blog/page file operations: add, remove, and parse frontmatter metadata.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow};
use chrono::Utc;
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::BASE_DIR;

pub mod blog;
pub mod page;

/// Metadata parsed from a source file's frontmatter.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Metadata {
    pub title: String,
    pub date: String,
    pub layout: Option<String>,
    pub tags: Option<Vec<String>>,
    pub categories: Option<Vec<String>>,
    pub prva: bool,
    pub path: PathBuf,
}

impl Metadata {
    pub fn new() -> Self {
        Metadata::default()
    }
}

/// Path to a source file (`source/<class>/<name>.md`).
pub(crate) fn get_path(name: &str, class: &str) -> PathBuf {
    BASE_DIR
        .join("source")
        .join(class)
        .join(name)
        .with_extension("md")
}

/// Check whether a file exists.
pub(crate) fn is_file_exist(path: &Path) -> bool {
    path.exists()
}

/// Current timestamp formatted in the configured `[site] zone`, falling back to UTC.
pub(crate) fn current_timestamp() -> String {
    const FMT: &str = "%Y-%m-%d %H:%M:%S";
    configured_timezone()
        .map(|tz| Utc::now().with_timezone(&tz).format(FMT).to_string())
        .unwrap_or_else(|| Utc::now().format(FMT).to_string())
}

fn configured_timezone() -> Option<Tz> {
    let config_text = fs::read_to_string(BASE_DIR.join("tless.toml")).ok()?;
    let config: toml::Value = toml::from_str(&config_text).ok()?;
    let zone = config
        .get("site")
        .and_then(|site| site.get("zone"))
        .and_then(|zone| zone.as_str())
        .map(str::trim)
        .filter(|zone| !zone.is_empty())?;
    zone.parse::<Tz>().ok()
}

/// Parse the frontmatter and file name of a source file into [Metadata].
pub fn parse_file(path: PathBuf) -> Result<Metadata> {
    let mut file = fs::File::open(&path)?;
    let mut text = String::new();
    if file.read_to_string(&mut text).is_err() {
        return Err(anyhow!("Failed to read blog."));
    }
    let (frontmatter, _) = frontmatter_gen::extract(&text)?;
    let mut metadata = Metadata::new();
    metadata.title = path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string()
        .strip_suffix(".md")
        .unwrap_or_default()
        .to_string();
    metadata.path = path;
    if let Some(date) = frontmatter.get("date").and_then(|v| v.as_str()) {
        metadata.date = date.to_string();
    }
    if let Some(layout) = frontmatter.get("layout").and_then(|v| v.as_str()) {
        metadata.layout = Some(layout.to_string());
    }
    if let Some(tags) = frontmatter.get("tags").and_then(|v| v.as_array()) {
        let tag_list = tags
            .iter()
            .filter_map(|t| t.as_str().map(|s| s.to_string()))
            .collect();
        metadata.tags = Some(tag_list);
    }
    if let Some(categories) = frontmatter.get("categories").and_then(|v| v.as_array()) {
        let category_list = categories
            .iter()
            .filter_map(|c| c.as_str().map(|s| s.to_string()))
            .collect();
        metadata.categories = Some(category_list);
    }
    if let Some(prva) = frontmatter.get("prva").and_then(|v| v.as_bool()) {
        metadata.prva = prva;
    }
    Ok(metadata)
}
