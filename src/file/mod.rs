//! Blog/page file operations: add, remove, and parse frontmatter metadata.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use crate::{
    BASE_DIR,
    server::zone,
    util::{slugify, truncate},
};

mod blog;
mod page;

pub use blog::Post;
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
    /// First paragraph of the body as plain text; filled by [parse_file].
    pub excerpt: String,
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
    Utc::now().with_timezone(&zone()).to_rfc3339()
}

/// Parse a source file into [Metadata] and its markdown body (frontmatter
/// stripped). The title falls back to the file name when not set.
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
    } else if let Some(t) = frontmatter.get("tag").and_then(|v| v.as_str()) {
        metadata.tag = Some(vec![t.to_string()]);
    }
    if let Some(category) = frontmatter.get("category").and_then(|v| v.as_array()) {
        let category_list = category
            .iter()
            .filter_map(|c| c.as_str().map(|s| s.to_string()))
            .collect();
        metadata.category = Some(category_list);
    } else if let Some(c) = frontmatter.get("category").and_then(|v| v.as_str()) {
        metadata.category = Some(vec![c.to_string()]);
    }
    metadata.excerpt = excerpt(md_body);
    Ok((metadata, md_body.to_string()))
}

/// First paragraph of a markdown body as plain text, for post listings.
/// Headings, code blocks and images are skipped; links keep their text.
fn excerpt(body: &str) -> String {
    for block in body.split("\n\n") {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }
        let mut text = String::new();
        let mut skip = false;
        let mut in_image = 0usize;
        for event in Parser::new(block) {
            match event {
                Event::Start(Tag::Heading { .. }) | Event::Start(Tag::CodeBlock(_)) => {
                    skip = true;
                    break;
                }
                Event::Start(Tag::Image { .. }) => in_image += 1,
                Event::End(TagEnd::Image) => in_image = in_image.saturating_sub(1),
                // image alt text is not part of the excerpt
                Event::Text(t) | Event::Code(t) if in_image == 0 => {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(t.trim());
                }
                _ => {}
            }
        }
        let text = text.trim();
        if !skip && !text.is_empty() {
            return truncate(text, 160);
        }
    }
    String::new()
}

/// A source entity (blog, page, ...) addressed by a user-supplied name.
pub(crate) trait ValidEntity {
    /// Resolve the entity's file path, failing when the target already exists.
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

#[cfg(test)]
mod tests {
    use super::excerpt;

    #[test]
    fn excerpt_skips_headings_and_code() {
        let body =
            "# Title\n\n## Section\n\n```rust\nfn main() {}\n```\n\nThe real first paragraph.";
        assert_eq!(excerpt(body), "The real first paragraph.");
    }

    #[test]
    fn excerpt_keeps_link_text_and_drops_images() {
        let body = "![cover](/img.png)\n\nRead the [announcement](/post/x) for details.";
        assert_eq!(excerpt(body), "Read the announcement for details.");
    }

    #[test]
    fn excerpt_truncates_long_paragraphs() {
        let body = "word ".repeat(60);
        let excerpt = excerpt(&body);
        assert!(excerpt.ends_with('…'));
        assert_eq!(excerpt.chars().count(), 161);
    }

    #[test]
    fn excerpt_is_empty_for_heading_only_bodies() {
        assert_eq!(excerpt("## Only headings\n\n### Nothing else"), "");
    }
}
