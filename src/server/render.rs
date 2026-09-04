use std::{
    collections::{HashMap, HashSet},
    io::BufReader,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

use arc_swap::ArcSwap;
use chrono::{DateTime, NaiveDateTime, Utc};
use data_encoding::HEXUPPER;
use futures::{StreamExt, stream};
use pulldown_cmark::{Options, Parser, html};
use ring::digest::{self, SHA256};
use tera::Context;
use tokio::{
    fs::{self, File},
    io::{AsyncWriteExt, BufWriter},
};
use tracing::{error, info};

use crate::{
    file,
    server::{SITE, Site, TERA, get_layout_path, get_public_path, get_source_path},
};

/// Markdown default render options.
const DEFAULT_OPTIONS: Options = Options::all();

/// Render markdown to HTML string.
pub(crate) fn render(markdown: &str) -> String {
    let parser = Parser::new_ext(markdown, DEFAULT_OPTIONS);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}

pub(crate) async fn render_to_file(events_path: &[PathBuf]) -> std::io::Result<()> {
    let public_dir = Arc::new(get_public_path("."));

    let concurrency = num_cpus::get() + 1;
    stream::iter(events_path)
        .map(|path| {
            let public_dir = public_dir.clone();
            async move {
                let metadata = match file::parse_file(path.clone()) {
                    Ok(m) => m,
                    Err(e) => {
                        error!("Failed to parse changed post: {}", e);
                        return Err(std::io::Error::other(e.to_string()));
                    }
                };
                let file_str = match pre_hash_check(path).await? {
                    Some(s) => s,
                    None => return Ok(()),
                };

                // The file parsed successfully above, so extracting the body
                // cannot fail here; fall back to the raw text otherwise
                let md_body = frontmatter_gen::extract(&file_str)
                    .map(|(_, body)| body.to_string())
                    .unwrap_or_default();

                let md_html_str = render(&md_body);
                let file_path = public_dir.join(&metadata.title);
                let file = File::create(&file_path).await?;
                let mut writer = BufWriter::new(file);

                let mut context = Context::new();
                context.insert("content", &md_html_str);
                context.insert("markdown", &md_body);
                context.insert("title", &metadata.title);
                context.insert("date", &metadata.date);
                context.insert("site", SITE.load().as_ref());
                let layout = metadata.layout.as_deref().unwrap_or("archive.html");
                match TERA.load().render(layout, &context) {
                    Ok(rendered) => {
                        writer.write_all(rendered.as_bytes()).await?;
                        writer.flush().await?;
                        info!("Rendered {}", metadata.title);
                        Ok(())
                    }
                    Err(e) => {
                        error!("Failed to render {}: {}", metadata.title, e);
                        Err(std::io::Error::other(e))
                    }
                }
            }
        })
        .buffer_unordered(concurrency)
        .collect::<Vec<_>>()
        .await;
    dump_json().await;
    Ok(())
}

/// Render the whole site to the public dir: every post and page, the home
/// page, and the active theme's static resources.
pub async fn render_all() -> std::io::Result<()> {
    let site = SITE.load();
    remove_stale_outputs(&site).await;
    let paths = site
        .posts
        .iter()
        .chain(site.pages.iter())
        .map(|d| d.path.clone())
        .collect::<Vec<_>>();
    render_to_file(&paths).await?;
    // render_home(&site).await?;
    copy_theme_resources()?;
    Ok(())
}

/// Remove public files whose source was deleted, keeping the deployed site
/// in sync with the sources.
async fn remove_stale_outputs(site: &Site) {
    let current: HashSet<String> = site
        .posts
        .iter()
        .chain(site.pages.iter())
        .map(|m| m.path.to_string_lossy().to_string())
        .collect();
    let post_hash = POST_HASH.load();
    let stale: Vec<String> = post_hash
        .keys()
        .filter(|path| !current.contains(*path))
        .cloned()
        .collect();
    if stale.is_empty() {
        return;
    }

    let mut map = (**post_hash).clone();
    for path_str in stale {
        map.remove(&path_str);
        let Some(stem) = Path::new(&path_str).file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let out = get_public_path(stem);
        match fs::remove_file(&out).await {
            Ok(()) => info!("Removed stale output: {}", out.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => error!("Failed to remove stale output {}: {}", out.display(), e),
        }
    }
    POST_HASH.store(Arc::new(map));
}

/// Render the theme's `index.html` layout as the site home page.
async fn render_home(site: &Site) -> std::io::Result<()> {
    let tera = TERA.load();
    if !tera
        .get_template_names()
        .any(|name| name == "index.html" || name == "index")
    {
        info!("No index.html layout found, skipping home page");
        return Ok(());
    }
    let mut context = Context::new();
    // an empty content keeps `{% if content %}` blocks in the layout happy
    context.insert("content", "");
    context.insert("posts", &recent_posts(site));
    match tera.render("index.html", &context) {
        Ok(rendered) => {
            fs::write(get_public_path("index.html"), rendered).await?;
            info!("Rendered index");
        }
        Err(e) => error!("Failed to render home page: {}", e),
    }
    Ok(())
}

/// Posts from `source/post`, newest first, exposed to the home page template.
fn recent_posts(site: &Site) -> Vec<file::Metadata> {
    let post_dir = get_source_path("post");
    let mut posts: Vec<file::Metadata> = site
        .posts
        .iter()
        .filter(|m| m.path.starts_with(&post_dir))
        .cloned()
        .collect();
    posts.sort_by_key(|p| std::cmp::Reverse(date_rank(&p.date)));
    posts
}

/// Parse a frontmatter date (RFC3339 or the CLI `%Y-%m-%d %H:%M:%S` format);
/// posts without a usable date sort last.
fn date_rank(date: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(date)
        .map(|d| d.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            NaiveDateTime::parse_from_str(date, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|d| d.and_utc())
        })
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

/// Copy the active theme's `resource/` directory into `public/`.
pub(crate) fn copy_theme_resources() -> std::io::Result<()> {
    let resource_dir = get_layout_path().join("resource");
    if !resource_dir.exists() {
        return Ok(());
    }
    copy_dir_recursive(&resource_dir, &get_public_path("."))
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

static POST_HASH: LazyLock<ArcSwap<HashMap<String, String>>> = LazyLock::new(|| {
    let post_hash = get_public_path(".post_hash.json");
    // The cache file is regenerable: a missing or unreadable file just resets it
    let map = std::fs::File::open(&post_hash)
        .map(|file| serde_json::from_reader(BufReader::new(file)).unwrap_or_default())
        .unwrap_or_default();
    ArcSwap::from_pointee(map)
});

async fn pre_hash_check(path: &Path) -> std::io::Result<Option<String>> {
    let file_text = fs::read_to_string(path).await?;
    let path_str = path.to_string_lossy().to_string();
    let mut context = digest::Context::new(&SHA256);
    context.update(file_text.as_bytes());
    let hash = context.finish();
    let hash_value = HEXUPPER.encode(hash.as_ref());
    let post_hash = POST_HASH.load();

    if post_hash
        .get(&path_str)
        .is_some_and(|saved| saved == &hash_value)
    {
        return Ok(None);
    }

    let mut clone = (**post_hash).clone();
    clone.insert(path_str, hash_value);
    POST_HASH.store(Arc::new(clone));

    Ok(Some(file_text))
}

async fn dump_json() {
    let map = &**POST_HASH.load();
    let json_str = match serde_json::to_string(map) {
        Ok(str) => str,
        Err(e) => {
            info!("Failed to dump post hash values: {}", e);
            String::new()
        }
    };
    let post_hash = get_public_path(".post_hash.json");
    match fs::write(post_hash, json_str).await {
        Ok(_) => info!(".post_hash.json updated"),
        Err(e) => error!("Failed to dump post hash values: {}", e),
    }
}
