use std::{
    collections::HashMap,
    env,
    io::BufReader,
    path::PathBuf,
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
    fs::File,
    io::{AsyncWriteExt, BufWriter},
};
use tracing::{error, info};

use crate::{
    file,
    server::{SITE, TERA},
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

pub(crate) async fn render_to_file(events_path: Vec<PathBuf>) -> std::io::Result<()> {
    let public_dir = env::current_dir()?.join("public");

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
                let (modify_flag, file_str) = pre_hash_check(&path).await?;
                if !modify_flag {
                    return Ok(());
                }
                let md_html_str = render(&file_str);
                let file_path = public_dir.join(&metadata.title);
                let file = File::create(&file_path).await?;
                let mut writer = BufWriter::new(file);

                let mut context = Context::new();
                context.insert("content", &md_html_str);
                match TERA.load().render("archive.html", &context) {
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

/// Render all posts to public dir.
pub(crate) async fn render_all() -> std::io::Result<()> {
    let public_dir = env::current_dir()?.join("public");
    let site = SITE.load();

    let concurrency = num_cpus::get() + 1;
    stream::iter(site.posts.clone())
        .map(|post| {
            let public_dir = public_dir.clone();
            async move {
                let (modify_flag, file_str) = pre_hash_check(&post.path).await?;
                if !modify_flag {
                    return Ok(());
                }
                let md_html_str = render(&file_str);
                let file_path: PathBuf = public_dir.join(&post.title);
                let file = File::create(&file_path).await?;
                let mut writer = BufWriter::new(file);

                let mut context = Context::new();
                context.insert("content", &md_html_str);

                match TERA.load().render("archive.html", &context) {
                    Ok(rendered) => {
                        writer.write_all(rendered.as_bytes()).await?;
                        writer.flush().await?;
                        info!("Rendered {}", post.title);
                        Ok(())
                    }
                    Err(e) => {
                        error!("Failed to render {}: {}", post.title, e);
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

#[derive(Debug, Deserialize, Serialize, Clone)]
pub(crate) struct HashValue {
    pub path: String,
    pub hash_v: String,
}

pub(crate) static POST_HASH: LazyLock<ArcSwap<HashMap<String, String>>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    let post_hash = env::current_dir()
        .unwrap()
        .join("public")
        .join(".post_hash.json");
    if !post_hash.exists() {
        let _ = std::fs::File::create_new(post_hash).unwrap();
    } else {
        let file = std::fs::File::open(post_hash).unwrap();
        let parsed: Vec<HashValue> =
            serde_json::from_reader(BufReader::new(file)).unwrap_or_default();
        for hash_value in parsed {
            map.insert(hash_value.path, hash_value.hash_v);
        }
    }
    ArcSwap::from_pointee(map)
});

fn get_post_hash_path() -> std::io::Result<PathBuf> {
    Ok(env::current_dir()?.join("public").join(".post_hash.json"))
}

async fn persist_post_hash(hash_map: &HashMap<String, String>) -> std::io::Result<()> {
    let hash_values: Vec<HashValue> = hash_map
        .iter()
        .map(|(path, hash_v)| HashValue {
            path: path.clone(),
            hash_v: hash_v.clone(),
        })
        .collect();
    let json = serde_json::to_vec_pretty(&hash_values).map_err(std::io::Error::other)?;
    tokio::fs::write(get_post_hash_path()?, json).await
}

pub(crate) async fn pre_hash_check(path: &PathBuf) -> std::io::Result<(bool, String)> {
    let file_text = tokio::fs::read_to_string(path).await?;
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
        return Ok((false, file_text));
    }

    let mut clone = (**post_hash).clone();
    clone.insert(path_str, hash_value);
    persist_post_hash(&clone).await?;
    POST_HASH.store(Arc::new(clone));

    Ok((true, file_text))
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
    let post_hash = env::current_dir()
        .unwrap()
        .join("public")
        .join(".post_hash.json");
    match tokio::fs::write(post_hash, json_str).await {
        Ok(_) => info!(".post_hash.json updated"),
        Err(e) => error!("Failed to dump post hash values: {}", e),
    }
}
