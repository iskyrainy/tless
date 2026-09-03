use std::{
    collections::HashMap,
    io::BufReader,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

use arc_swap::ArcSwap;
use data_encoding::HEXUPPER;
use futures::{StreamExt, stream};
use pulldown_cmark::{Options, Parser, html};
use ring::digest::{self, SHA256};
use serde::{Deserialize, Serialize};
use tera::Context;
use tokio::{
    fs::{self, File},
    io::{AsyncWriteExt, BufWriter},
};
use tracing::{error, info};

use crate::{
    file,
    server::{SITE, TERA, get_public_path},
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
                    .unwrap_or_else(|_| file_str.clone());

                let md_html_str = render(&file_str);
                let file_path = public_dir.join(&metadata.title);
                let file = File::create(&file_path).await?;
                let mut writer = BufWriter::new(file);

                let mut context = Context::new();
                context.insert("content", &md_html_str);
                context.insert("markdown", &md_body);
                context.insert("title", &metadata.title);
                context.insert("date", &metadata.date);
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

/// Render all posts and pages to public dir.
pub async fn render_all() -> std::io::Result<()> {
    let site = SITE.load();
    let mut paths = site
        .posts
        .iter()
        .map(|d| d.path.clone())
        .collect::<Vec<_>>();
    paths.append(
        &mut site
            .pages
            .iter()
            .map(|d| d.path.clone())
            .collect::<Vec<_>>(),
    );
    render_to_file(&paths).await?;
    Ok(())
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct HashValue {
    path: String,
    hash: String,
}

static POST_HASH: LazyLock<ArcSwap<HashMap<String, String>>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    let post_hash = get_public_path(".post_hash.json");
    // The cache file is regenerable: a missing or unreadable file just resets it
    if let Ok(file) = std::fs::File::open(&post_hash) {
        let parsed: Vec<HashValue> =
            serde_json::from_reader(BufReader::new(file)).unwrap_or_default();
        for hash_value in parsed {
            map.insert(hash_value.path, hash_value.hash);
        }
    }
    ArcSwap::from_pointee(map)
});

pub(crate) async fn pre_hash_check(path: &Path) -> std::io::Result<Option<String>> {
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

pub(crate) async fn dump_json() {
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
