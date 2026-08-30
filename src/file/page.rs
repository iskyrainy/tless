use std::{error::Error, fs};

use chrono::Utc;

use crate::file::{get_path, is_file_exist};

/// Add a new page with given name.
/// If file exists, print failed.
/// # Arguments
/// * `name` - The name of the page to be added.
/// # Returns
/// * A `Result` indicating success or failure.
/// # Examples
/// ```
/// let name = String::from("About");
/// match add_page(&name) {
///     Ok(_) => info!("Page added successfully."),
///     Err(e) => error!("Failed to add page: {}", e),
/// }
/// ```
pub fn add_page(name: &String) -> Result<(), Box<dyn Error>> {
    let class = String::from("page");
    let file_path = get_path(name, &class);
    if is_file_exist(&file_path) {
        return Err("Blog already exists.".into());
    }
    fs::write(file_path, base_page_text(name))?;
    Ok(())
}

fn base_page_text(name: &String) -> String {
    format!(
        "---\ntitle: {}\ndate: {}\nlayout: index.html\n---\n",
        name,
        Utc::now().format("%Y-%m-%d %H:%M:%S")
    )
}

/// Remove an existing page with given name.
/// If file not exists, print failed.
/// # Arguments
/// * `name` - The name of the page to be removed.
/// # Returns
/// * A `Result` indicating success or failure.
/// # Examples
/// ```
/// let name = String::from("About");
/// match remove_page(&name) {
///     Ok(_) => info!("Page removed successfully."),
///     Err(e) => error!("Failed to remove page: {}", e),
/// }
/// ```
pub fn remove_page(name: &String) -> Result<(), Box<dyn Error>> {
    let class = String::from("page");
    let file_path = get_path(name, &class);
    if !is_file_exist(&file_path) {
        return Err("Page does not exist.".into());
    }
    fs::remove_file(file_path)?;
    Ok(())
}
