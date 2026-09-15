//! Draft and post file handling behind `tless blog`.

use std::{fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use tracing::info;

use crate::file::{ValidEntity, current_timestamp, get_path, parse_file};

pub struct Post;

impl ValidEntity for Post {
    fn validate_and_get_path(name: &str) -> Result<PathBuf> {
        let slug = Self::validate_name(name)?;
        let file_path = get_path(&slug, "draft");
        if file_path.exists() {
            bail!("Blog already exists.");
        }
        Ok(file_path)
    }
}

impl Post {
    /// Add a new draft blog file.
    pub fn add(name: &str) -> Result<()> {
        let file_path = Self::validate_and_get_path(name)?;
        fs::write(&file_path, Self::base_blog_text())
            .context(format!("Failed to create draft/: {}", file_path.display()))?;
        info!("Blog '{}' created in 'draft'", file_path.display());
        Ok(())
    }

    #[inline]
    fn base_blog_text() -> String {
        // empty arrays instead of null values: the frontmatter parser rejects
        // keys without a value
        format!(
            "---\ndate: {}\ntag: []\ncategory: []\n---\n\n# New Blog\nWrite your content here.\n",
            current_timestamp()
        )
    }

    /// Remove an existing blog file.
    pub fn remove(name: &str, class: &str) -> Result<()> {
        let slug = Self::validate_name(name)?;
        let file_path = get_path(&slug, class);
        if !file_path.exists() {
            bail!("Blog does not exist.");
        }
        fs::remove_file(&file_path)
            .context(format!("Failed to remove draft/: {}", file_path.display()))?;
        info!("Blog '{}' removed from '{}'", name, class);
        Ok(())
    }

    /// Publish a draft blog by moving it to the post class and updating its frontmatter.
    pub fn publish(name: &str) -> Result<()> {
        let slug = Self::validate_name(name)?;
        let draft_path = get_path(&slug, "draft");
        if !draft_path.exists() {
            bail!("Draft blog does not exist");
        }
        let post_path = get_path(&slug, "post");
        if post_path.exists() {
            bail!("Post blog already exists");
        }
        let (metadata, md_body) = parse_file(&draft_path)?;
        let frontmatter = format!(
            "---\ntitle: {}\ndate: {}\ntag: {}\ncategory: {}\nlayout: {}\n---\n\n",
            metadata.title,
            current_timestamp(),
            format_args!("[{}]", metadata.tag.unwrap_or_default().join(", ")),
            format_args!("[{}]", metadata.category.unwrap_or_default().join(", ")),
            metadata.layout.unwrap_or("post.html".to_string()),
        );
        let content = format!("{}{}", frontmatter, md_body);
        fs::write(&post_path, content)
            .context(format!("Failed to create post/: {}", post_path.display()))?;
        fs::remove_file(&draft_path)
            .context(format!("Failed to remove draft/: {}", draft_path.display()))?;
        info!("Blog '{}' published from 'draft' to 'post'", name);
        Ok(())
    }
}
