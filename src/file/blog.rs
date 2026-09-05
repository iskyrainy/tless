use std::{fs, path::PathBuf};

use anyhow::{Result, bail};
use tracing::info;

use crate::file::{ValidEntity, current_timestamp, get_path, is_file_exist, parse_file};

pub struct Blog;

impl ValidEntity for Blog {
    fn validate_and_get_path(name: &str) -> Result<PathBuf> {
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

        let file_path = get_path(&slug, "draft");
        if is_file_exist(&file_path) {
            bail!("Page already exists.");
        }
        Ok(file_path)
    }
}

impl Blog {
    /// Add a new draft blog file.
    pub fn add(name: &str) -> Result<()> {
        let file_path = Self::validate_and_get_path(name)?;
        fs::write(&file_path, Self::base_blog_text())?;
        info!("Blog '{}' created in 'draft'", file_path.display());
        Ok(())
    }

    #[inline]
    fn base_blog_text() -> String {
        // empty arrays instead of null values: the frontmatter parser rejects
        // keys without a value
        format!(
            "---\ndate: {}\ntags: []\ncategories: []\n---\n\n# New Blog\nWrite your content here.\n",
            current_timestamp()
        )
    }

    /// Remove an existing blog file.
    pub fn remove(name: &str, class: &str) -> Result<()> {
        let file_path = get_path(name, class);
        if !is_file_exist(&file_path) {
            bail!("Blog does not exist.");
        }
        fs::remove_file(&file_path)?;
        info!("Blog '{}' removed from '{}'", name, class);
        Ok(())
    }

    /// Publish a draft blog by moving it to the post class and updating its frontmatter.
    pub fn publish(name: &str) -> Result<()> {
        let draft_path = get_path(name, "draft");
        if !is_file_exist(&draft_path) {
            bail!("Draft blog does not exist");
        }
        let post_path = get_path(name, "post");
        if is_file_exist(&post_path) {
            bail!("Post blog already exists");
        }
        let metadata = parse_file(&draft_path)?;
        let frontmatter = format!(
            "---\ntitle: {}\ndate: {}\ntags: {}\ncategories: {}\nlayout: {}\n---\n\n",
            metadata.title,
            current_timestamp(),
            format_args!("[{}]", metadata.tags.unwrap_or_default().join(", ")),
            format_args!("[{}]", metadata.categories.unwrap_or_default().join(", ")),
            metadata.layout.unwrap_or("archive.html".to_string()),
        );
        let file_str = fs::read_to_string(&draft_path)?;
        let content = format!("{}{}", frontmatter, file_str);
        fs::write(&post_path, content)?;
        fs::remove_file(&draft_path)?;
        info!("Blog '{}' published from 'draft' to 'post'", name);
        Ok(())
    }
}
