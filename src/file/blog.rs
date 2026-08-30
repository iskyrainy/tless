use std::{env, error::Error, fs, path::PathBuf};

use chrono::Utc;
use chrono_tz::Tz;
use tracing::info;

use crate::file::{get_path, is_file_exist, parse_file};

/// Add a new blog file with default content.
/// # Arguments
/// * `name` - A reference to a `String` representing the blog name.
/// * `class` - A reference to a `String` representing the blog class (e.g., "draft", "post").
/// # Returns
/// A `Result` indicating success or failure.
/// # Examples
/// ```
/// let name = String::from("my_new_blog");
/// let class = String::from("draft");
/// match add_blog(&name, &class) {
///     Ok(_) => info!("Blog added successfully."),
///     Err(e) => error!("Failed to add blog: {}", e),
/// }
/// ```
pub fn add_blog(name: &String) -> Result<(), Box<dyn Error>> {
    let class = String::from("draft");
    let file_path = get_path(name, &class);
    if is_file_exist(&file_path) {
        return Err("Blog already exists.".into());
    }
    fs::write(&file_path, base_blog_text())?;
    info!("Blog '{}' created in 'draft'.", file_path);
    Ok(())
}

fn base_blog_text() -> String {
    format!(
        "---\ndate: {}\ntags:\ncategories:\n---\n\n# New Blog\nWrite your content here.\n",
        current_timestamp()
    )
}

/// Remove an existing blog file.
/// # Arguments
/// * `name` - A reference to a `String` representing the blog name.
/// * `class` - A reference to a `String` representing the blog class (e.g., "draft", "post").
/// # Returns
/// A `Result` indicating success or failure.
/// # Examples
/// ```
/// let name = String::from("my_old_blog");
/// let class = String::from("post");
/// match remove_blog(&name, &class) {
///     Ok(_) => info!("Blog removed successfully."),
///     Err(e) => error!("Failed to remove blog: {}", e),
/// }
/// ```
pub fn remove_blog(name: &String, class: &String) -> Result<(), Box<dyn Error>> {
    let file_path = get_path(name, class);
    if !is_file_exist(&file_path) {
        return Err("Blog does not exist.".into());
    }
    fs::remove_file(&file_path)?;
    info!("Blog '{}' removed from '{}'.", name, class);
    Ok(())
}

/// Publish a draft blog by moving it to the post class and updating its metadata.
/// # Arguments
/// * `name` - A reference to a `String` representing the blog name.
/// * `prva` - A boolean indicating if the blog should be marked as private.
/// # Returns
/// A `Result` indicating success or failure.
/// # Examples
/// ```
/// let name = String::from("my_draft_blog");
/// let prva = false;
/// match publish_blog(&name, prva) {
///     Ok(_) => info!("Blog published successfully."),
///     Err(e) => error!("Failed to publish blog: {}", e),
/// }
/// ```
pub fn publish_blog(name: &String) -> Result<(), Box<dyn Error>> {
    let draft_path = get_path(name, &String::from("draft"));
    if !is_file_exist(&draft_path) {
        return Err("Draft blog does not exist.".into());
    }
    let post_path = get_path(name, &String::from("post"));
    if is_file_exist(&post_path) {
        return Err("Post blog already exists.".into());
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

fn current_timestamp() -> String {
    let fmt = "%Y-%m-%d %H:%M:%S";
    configured_timezone()
        .map(|tz| Utc::now().with_timezone(&tz).format(fmt).to_string())
        .unwrap_or_else(|| Utc::now().format(fmt).to_string())
}

fn configured_timezone() -> Option<Tz> {
    let config_path = env::current_dir().ok()?.join("tless.toml");
    let config_text = fs::read_to_string(config_path).ok()?;
    let config: toml::Value = toml::from_str(&config_text).ok()?;
    let zone = config
        .get("site")
        .and_then(|site| site.get("zone"))
        .and_then(|zone| zone.as_str())
        .map(str::trim)
        .filter(|zone| !zone.is_empty())?;
    zone.parse::<Tz>().ok()
}
