use std::{
    collections::HashMap,
    fs,
    future::Future,
    path::{self, PathBuf},
    sync::{Arc, LazyLock},
    time::Duration,
};

use anyhow::{Result, bail};
use arc_swap::ArcSwap;
use chrono_tz::Tz;
use notify::EventKind;
use notify_debouncer_full::{DebouncedEvent, new_debouncer};
use serde::{Deserialize, Serialize};
use tera::Tera;
use tokio::{join, select, sync::mpsc};
use tracing::{error, info};

use crate::{
    BASE_DIR, error,
    file::{Metadata, parse_file},
};

mod helper;
mod render;
mod run;
mod site;
mod template;

pub use render::render_all;
pub use run::run;
pub use site::init;

/// Configuration structure for the application.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Config {
    pub site: SiteConfig,
}

/// Part of `[site]` configuration details.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct SiteConfig {
    pub title: String,
    pub subtitle: String,
    pub description: String,
    pub rights: String,
    pub author: String,
    pub url: String,
    pub zone: String,
    pub theme: String,
    pub favicon: String,
    pub menu: Vec<Menu>,
    #[serde(skip)]
    inner_zone: Option<Tz>,
}

impl Site {
    pub fn get_zone(&self) -> Tz {
        self.config.inner_zone.unwrap_or_default()
    }
}

/// Menu item structure for site navigation.
/// # Fields
/// * `name` - The display name of the menu item.
/// * `link` - The URL or path the menu item points to.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Menu {
    pub name: String,
    pub link: String,
}

/// Get the path to the configuration file (`tless.toml`) in the current directory.
#[inline]
fn get_config_path() -> PathBuf {
    BASE_DIR.join("tless.toml")
}

/// Load `tless.toml` to `CONFIG`.
fn get_config_toml() -> Result<Config> {
    let config_path = get_config_path();
    if !config_path.exists() {
        bail!("Configuration file not found at {}", config_path.display());
    }
    let config_content = fs::read_to_string(config_path)?;
    Ok(toml::from_str(&config_content)?)
}

/// Struct of global source info, including `post`, `page`.
/// # Fields
/// * `post` - List of all post metadata.
/// * `page` - List of all page metadata.
/// * `category` - Map of all categories.
/// * `tag` - Map of all tags.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct Site {
    pub post: Vec<Metadata>,
    pub page: Vec<Metadata>,
    pub category: HashMap<String, ClassMap>,
    pub tag: HashMap<String, ClassMap>,
    pub config: SiteConfig,
}

impl Site {
    pub fn new() -> Self {
        Site {
            post: vec![],
            page: vec![],
            category: HashMap::new(),
            tag: HashMap::new(),
            config: SiteConfig::default(),
        }
    }
}

/// Store class info, class can be categories or tags.
/// # Fields
/// * `path` - Class url, normally as the `/self.name`.
/// * `posts` - List of posts that belong to this class.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct ClassMap {
    pub path: String,
    pub posts: Vec<Metadata>,
}

/// Get the path to the source dir (`./source`) in the current directory.
#[inline]
pub(crate) fn get_source_path<'a, S: Into<&'a str>>(name: S) -> PathBuf {
    BASE_DIR.join("source").join(name.into())
}

#[inline]
pub(crate) fn extract_root_path(url: &str) -> String {
    if url.is_empty() {
        return String::new();
    }
    if let Some(pos) = url.find("://")
        && let Some(path_pos) = url[pos + 3..].find('/')
    {
        return url[pos + 3 + path_pos..].to_string();
    }
    url.to_string()
}

/// Load [Metadata] of every source file into `SITE`, skipping unparseable files.
fn get_site() -> Site {
    let post_dir = get_source_path("post");
    let page_dir = get_source_path("page");
    let mut site = Site::new();
    site.config = match get_config_toml() {
        Ok(mut config) => {
            config.site.inner_zone = config.site.zone.trim().parse::<Tz>().ok();
            config.site
        }
        Err(e) => error::fatal(format!("{e:#}")),
    };

    let class_path = |c: &str, t: &str, base_url: &String| -> String {
        format!("{}/{}/{}", extract_root_path(base_url), t, c)
    };

    let load = |site: &mut Site, dirs: Vec<PathBuf>| {
        for dir in dirs {
            let Ok(entries) = fs::read_dir(dir) else {
                continue;
            };
            for entry in entries {
                let Ok(entry) = entry else {
                    continue;
                };
                let path = entry.path();
                if !is_source_file(&path) {
                    continue;
                }
                let (metadata, _) = match parse_file(&path) {
                    Ok(m) => m,
                    Err(e) => {
                        error!("Failed to parse source file: {}", e);
                        continue;
                    }
                };
                if path.starts_with(&page_dir) {
                    site.page.push(metadata.clone());
                } else {
                    site.post.push(metadata.clone());
                }
                if let Some(category) = metadata.category.as_ref() {
                    for c in category {
                        site.category
                            .entry(c.clone())
                            .or_insert_with(|| ClassMap {
                                path: class_path(c, "category", &site.config.url),
                                posts: vec![],
                            })
                            .posts
                            .push(metadata.clone());
                    }
                }
                if let Some(tag) = metadata.tag.as_ref() {
                    for c in tag {
                        site.tag
                            .entry(c.clone())
                            .or_insert_with(|| ClassMap {
                                path: class_path(c, "tag", &site.config.url),
                                posts: vec![],
                            })
                            .posts
                            .push(metadata.clone());
                    }
                }
            }
        }
    };

    load(&mut site, vec![post_dir, page_dir.clone()]);
    site
}

/// Only accept valid source files
fn is_source_file(path: &path::Path) -> bool {
    if !path.is_file() {
        return false;
    }
    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
        // skip temp/backup files
        if name.starts_with('.') || name.ends_with('~') || name.ends_with(".swp") {
            return false;
        }
    }
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        matches!(ext, "md" | "markdown" | "toml" | "html" | "rhai")
            && !path.to_str().unwrap_or_default().contains("/draft/")
    } else {
        false
    }
}

pub(crate) static SITE: LazyLock<ArcSwap<Site>> = LazyLock::new(|| {
    let site = get_site();
    ArcSwap::from_pointee(site)
});

async fn watch_source(mut shutdown_rx: tokio::sync::broadcast::Receiver<()>) -> Result<()> {
    // notify-debouncer-full debounce window size: 1000ms
    let (tx, mut rx) = mpsc::channel(1000);
    let mut debouncer = new_debouncer(Duration::from_millis(1000), None, move |e| {
        let _ = tx.blocking_send(e);
    })?;
    debouncer.watch(get_source_path("page"), notify::RecursiveMode::Recursive)?;
    debouncer.watch(get_source_path("post"), notify::RecursiveMode::Recursive)?;

    loop {
        select! {
            _ = shutdown_rx.recv() => {
                info!("Source watcher received shutdown signal");
                break;
            },
            Some(Ok(events)) = rx.recv() => {
                let mut changed: Vec<PathBuf> = Vec::new();
                for e in events {
                    match e.event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) => {
                            changed.extend(e.event.paths.iter().cloned());
                        }
                        _ => {}
                    };
                }
                if changed.is_empty() {
                    continue;
                }

                // refresh the site first so taxonomy pages pick up new terms
                let site = get_site();
                SITE.store(Arc::new(site));

                let posts = changed
                    .iter()
                    .filter(|p| p.starts_with(get_source_path("post")))
                    .collect::<Vec<_>>();
                if let Err(err) = render::render_post(posts).await {
                    error!("Failed to render changed file: {}", err);
                }
                let pages = changed
                    .iter()
                    .filter(|p| p.starts_with(get_source_path("page")))
                    .collect::<Vec<_>>();
                if let Err(err) = render::render_page(pages).await {
                    error!("Failed to render changed file: {}", err);
                }
                info!("Site global info reloaded.");
            }
            else => {
                info!("Source watcher channel closed");
                break;
            }
        }
    }

    Ok(())
}

#[inline]
pub(crate) fn get_layout_path() -> PathBuf {
    let dir = BASE_DIR.join("theme").join(&SITE.load().config.theme);
    if dir.exists() {
        dir
    } else {
        error::fatal(format!("Theme directory not found: {}", dir.display()))
    }
}

pub(crate) static TERA: LazyLock<ArcSwap<Tera>> = LazyLock::new(|| {
    let layout_dir = get_layout_path();
    let glob = format!("{}/layout/*.html", layout_dir.to_string_lossy());
    let mut tera = Tera::new();
    helper::register_helpers(&mut tera);
    let rhai_helpers = helper::compile_rhai_helpers(BASE_DIR.join("helper"))
        .unwrap_or_else(|e| error::fatal(format!("Failed to load helpers: {e}")));
    helper::register_rhai_helpers(&mut tera, rhai_helpers);
    tera.load_from_glob(&glob)
        .unwrap_or_else(|e| error::fatal(format!("Failed to load templates: {e}")));
    ArcSwap::from_pointee(tera)
});

async fn watch_layout(mut shutdown_rx: tokio::sync::broadcast::Receiver<()>) -> Result<()> {
    let theme_path = get_layout_path();

    // notify-debouncer-full debounce window size: 1000ms
    let (tx, mut rx) = mpsc::channel(32);
    let mut debouncer = new_debouncer(
        Duration::from_millis(1000),
        None,
        move |result: Result<Vec<DebouncedEvent>, Vec<_>>| match result {
            Ok(events) => {
                if events.iter().any(|event| {
                    matches!(
                        event.event.kind,
                        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                    )
                }) && let Err(e) = tx.try_send(())
                {
                    error!("Failed to send layout reload event: {}", e);
                }
            }
            Err(e) => {
                error!("Layout watcher error: {:?}", e);
            }
        },
    )?;
    debouncer.watch(&theme_path, notify::RecursiveMode::Recursive)?;

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                info!("Layout watcher received shutdown signal");
                break;
            }
            Some(()) = rx.recv() => {
                let tera = TERA.load();
                let mut clone = tera.as_ref().clone();
                match clone.full_reload() {
                    Ok(()) => {
                        TERA.store(Arc::new(clone));
                        if let Err(e) = render::render_all().await {
                            error!("Failed to render posts: {}", e);
                        }
                        info!("TERA reloaded.");
                    }
                    // Keep serving the previous templates on a failed reload
                    Err(e) => error!("Failed to reload templates: {}", e),
                }
            }
            else => {
                info!("Layout watcher channel closed");
                break;
            }
        }
    }

    Ok(())
}

#[inline]
pub(crate) fn get_public_path<'a, S: Into<&'a str>>(name: S) -> PathBuf {
    BASE_DIR.join("public").join(name.into())
}

async fn watch_helper(mut shutdown_rx: tokio::sync::broadcast::Receiver<()>) -> Result<()> {
    let helper_path = BASE_DIR.join("helper");

    let (tx, mut rx) = mpsc::channel(32);
    let mut debouncer = new_debouncer(
        Duration::from_millis(1000),
        None,
        move |result: Result<Vec<DebouncedEvent>, Vec<_>>| match result {
            Ok(events) => {
                if events.iter().any(|event| {
                    matches!(
                        event.event.kind,
                        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                    ) && event.event.paths.iter().any(|path| is_source_file(path))
                }) && let Err(e) = tx.try_send(())
                {
                    error!("Failed to send helper reload event: {}", e);
                }
            }
            Err(e) => {
                error!("Helper watcher error: {:?}", e);
            }
        },
    )?;
    debouncer.watch(&helper_path, notify::RecursiveMode::Recursive)?;

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                info!("Helper watcher received shutdown signal");
                break;
            }
            Some(()) = rx.recv() => {
                match helper::load_rhai_helpers(&helper_path) {
                    Ok(_) => {
                        info!("Helper reloaded.");
                    }
                    Err(e) => {
                        error!("Failed to reload helper: {}", e);
                    }
                }
            }
            else => {
                info!("Helper watcher channel closed");
                break;
            }
        }
    }

    Ok(())
}

/// Run a watcher and log its failure instead of aborting the process.
async fn watch_logged(name: &str, watch: impl Future<Output = Result<()>>) {
    if let Err(e) = watch.await {
        error!("{} watcher failed: {}", name, e);
    }
}

/// Start watching.
/// # Arguments
/// * `shutdown_tx` - Subscribe the sender to recv a shutdown signal.
pub(crate) async fn start_watch(shutdown_tx: tokio::sync::broadcast::Sender<()>) {
    let _ = join! {
        watch_logged("source", watch_source(shutdown_tx.subscribe())),
        watch_logged("layout", watch_layout(shutdown_tx.subscribe())),
        watch_logged("helper", watch_helper(shutdown_tx.subscribe())),
    };
}
