use std::{fs, path::PathBuf};

use anyhow::{Result, bail};
use tracing::info;

use crate::file::{current_timestamp, get_path, is_file_exist, parse_file};

/// Add a new draft blog file.
pub fn add_blog(name: &str) -> Result<()> {
    let file_path = get_path(name, "draft");
    if is_file_exist(&file_path) {
        bail!("Blog already exists.");
    }
    fs::write(&file_path, base_blog_text())?;
    info!("Blog '{}' created in 'draft'.", file_path.display());
    Ok(())
}

fn base_blog_text() -> String {
    format!(
        "---\ndate: {}\ntags:\ncategories:\n---\n\n# New Blog\nWrite your content here.\n",
        current_timestamp()
    )
}

/// Remove an existing blog file.
pub fn remove_blog(name: &str, class: &str) -> Result<()> {
    let file_path = get_path(name, class);
    if !is_file_exist(&file_path) {
        bail!("Blog does not exist.");
    }
    fs::remove_file(&file_path)?;
    info!("Blog '{}' removed from '{}'.", name, class);
    Ok(())
}

/// Publish a draft blog by moving it to the post class and updating its frontmatter.
pub fn publish_blog(name: &str) -> Result<()> {
    let draft_path = get_path(name, "draft");
    if !is_file_exist(&draft_path) {
        bail!("Draft blog does not exist.");
    }
    let post_path = get_path(name, "post");
    if is_file_exist(&post_path) {
        bail!("Post blog already exists.");
    }
    let metadata = parse_file(PathBuf::from(&draft_path))?;
    let frontmatter = format!(
        "---\ntitle: {}\ndate: {}\ntags: {}\ncategories: {}\n---\n\n",
        metadata.title,
        current_timestamp(),
        format_args!("[{}]", metadata.tags.unwrap_or_default().join(", ")),
        format_args!("[{}]", metadata.categories.unwrap_or_default().join(", ")),
    );
    let file_str = fs::read_to_string(&draft_path)?;
    let content = format!("{}{}", frontmatter, file_str);
    fs::write(&post_path, content)?;
    fs::remove_file(&draft_path)?;
    info!("Blog '{}' published from 'draft' to 'post'.", name);
    Ok(())
}
