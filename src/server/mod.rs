use std::{
    collections::HashMap,
    env, fs,
    path::{self, PathBuf},
    sync::{Arc, LazyLock},
    time::Duration,
};

use anyhow::{Result, anyhow};
use arc_swap::ArcSwap;
use notify::EventKind;
use notify_debouncer_full::{DebouncedEvent, new_debouncer};
use serde::{Deserialize, Serialize};
use tera::Tera;
use tokio::{join, select, sync::mpsc};
use tracing::{error, info};

use crate::{
    file::{Metadata, parse_file},
    result_matcher,
};

pub mod helper;
pub mod render;
pub mod run;

pub(crate) static BASE_DIR: LazyLock<PathBuf> = LazyLock::new(|| env::current_dir().unwrap());

/// Configuration structure for the application.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Config {
    pub site: SiteConfig,
}

/// Part of `[site]` configuration details.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct SiteConfig {
    pub title: String,
    pub description: String,
    pub author: String,
    pub url: String,
    pub zone: String,
    pub theme: String,
    pub favicon: String,
    pub menu: Vec<Menu>,
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
fn get_config_path() -> PathBuf {
    BASE_DIR.join("tless.toml")
}

/// Load `tless.toml` to `CONFIG`.
fn get_config_toml() -> Result<Config> {
    let config_path = get_config_path();
    if !config_path.exists() {
        return Err(anyhow!(
            "Configuration file not found at {}",
            config_path.display()
        ));
    }
    let config_content = fs::read_to_string(config_path)?;
    Ok(toml::from_str(&config_content)?)
}

/// Global static configuration accessible throughout the application.
pub(crate) static CONFIG: LazyLock<ArcSwap<Config>> = LazyLock::new(|| {
    if let Ok(config) = get_config_toml() {
        ArcSwap::from_pointee(config)
    } else {
        panic!("Failed to load config")
    }
});

enum ConfigWatchEvent {
    Rewatch,
    Reload,
}

/// Watch the configuration file for changes and update the global `CONFIG` accordingly.
/// # Arguments
/// * `shutdown_rx` - A receiver to listen for shutdown signals.
async fn watch_config(mut shutdown_rx: tokio::sync::broadcast::Receiver<()>) -> Result<()> {
    let config_path = get_config_path();
    let (tx, mut rx) = mpsc::channel(32);
    let mut debouncer = new_debouncer(
        Duration::from_millis(1000),
        None,
        move |result: Result<Vec<DebouncedEvent>, Vec<_>>| match result {
            Ok(events) => {
                let mut reload = false;
                let mut rewatch = false;
                for event in events {
                    match event.event.kind {
                        EventKind::Modify(_) => {
                            reload = true;
                        }
                        EventKind::Create(_) => {
                            reload = true;
                            rewatch = true;
                        }
                        EventKind::Remove(_) => {
                            rewatch = true;
                        }
                        _ => {}
                    }
                }

                let event = if rewatch {
                    Some(ConfigWatchEvent::Rewatch)
                } else if reload {
                    Some(ConfigWatchEvent::Reload)
                } else {
                    None
                };
                if let Some(event) = event
                    && let Err(e) = tx.try_send(event)
                {
                    error!("Failed to send config event: {}", e);
                }
            }
            Err(e) => {
                error!("Config watcher error: {:?}", e);
            }
        },
    )?;

    debouncer.watch(&config_path, notify::RecursiveMode::NonRecursive)?;

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                info!("Config watcher received shutdown signal");
                break;
            }
            Some(event) = rx.recv() => {
                let config = match get_config_toml() {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to load config: {}", e);
                        continue;
                    }
                };
                CONFIG.store(Arc::new(config));
                if let ConfigWatchEvent::Rewatch = event
                    && config_path.exists() && let Err(e) = debouncer.watch(
                        &config_path,
                        notify::RecursiveMode::NonRecursive,
                    ) {
                        error!("Config file rewatch error: {}", e);
                    }
            }
            else => {
                info!("Config watcher channel closed");
                break;
            }
        }
    }

    Ok(())
}

/// Struct of global source info, including `post`, `page`.
/// # Fields
/// * `posts` - List of all post metadata.
/// * `pages` - List of all page metadata.
/// * `categories` - Map of all categories.
/// * `tags` - Map of all tags.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct Site {
    pub posts: Vec<Metadata>,
    pub pages: Vec<Metadata>,
    pub categories: HashMap<String, ClassMap>,
    pub tags: HashMap<String, ClassMap>,
}

impl Site {
    pub fn new() -> Self {
        Site {
            posts: vec![],
            pages: vec![],
            categories: HashMap::new(),
            tags: HashMap::new(),
        }
    }
}

/// Store class info, class can be categories or tags.
/// # Fields
/// * `name` - Class name.
/// * `path` - Class url, normally as the `/self.name`.
/// * `posts` - List of posts that belong to this class.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct ClassMap {
    pub path: String,
    pub posts: Vec<Metadata>,
}

impl ClassMap {
    pub fn new() -> Self {
        ClassMap {
            path: String::new(),
            posts: vec![],
        }
    }
}

/// Get the path to the source dir (`./source`) in the current directory.
pub(crate) fn get_source_path<'a, S: Into<&'a str>>(name: S) -> PathBuf {
    BASE_DIR.join("source").join(name.into())
}

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

/// Load files' [Metadata] of `./source` into `SITE`.
fn get_site() -> Site {
    let post_dir = get_source_path("post");
    let page_dir = get_source_path("page");
    let site = Site::new();

    let class_path = |c: &String, t: &'static str| -> String {
        let config = CONFIG.load();
        format!(
            "{}/{}/{}",
            extract_root_path(config.site.url.as_str()),
            t,
            c
        )
    };

    let load = |mut site: Site, dirs: Vec<PathBuf>| -> Site {
        dirs.iter().for_each(|dir| {
            if let Ok(dir) = fs::read_dir(dir) {
                for entry in dir {
                    let entry = match entry {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    let path = entry.path();
                    if !is_source_file(&path) {
                        continue;
                    }
                    let metadata =
                        result_matcher!(parse_file(PathBuf::from(&path)), "Failed to parse file");
                    site.posts.push(metadata.clone());
                    if let Some(categories) = metadata.categories.as_ref() {
                        for c in categories {
                            if let Some(map) = site.categories.get_mut(c) {
                                map.posts.push(metadata.clone());
                            } else {
                                let mut new_map = ClassMap::new();
                                new_map.path = class_path(c, "categories");
                                new_map.posts.push(metadata.clone());
                                site.categories.insert(c.to_string(), new_map);
                            }
                        }
                    }
                    if let Some(tags) = metadata.tags.as_ref() {
                        for c in tags {
                            if let Some(map) = site.tags.get_mut(c) {
                                map.posts.push(metadata.clone());
                            } else {
                                let mut new_map = ClassMap::new();
                                new_map.path = class_path(c, "tags");
                                new_map.posts.push(metadata.clone());
                                site.tags.insert(c.to_string(), new_map);
                            }
                        }
                    }
                }
            }
        });
        site
    };

    load(site, vec![post_dir, page_dir])
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
    // notify-debouncer-mini debounce window size: 1000ms
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
                let mut update_flag = false;
                for e in events {
                    let event = &e.event;
                    match event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) => {
                            result_matcher!(
                                render::render_to_file(&event.paths).await,
                                "Failed to render changed markdown to file"
                            );
                            update_flag = true;
                        }
                        _ => {}
                    };
                }

                if update_flag {
                    let site = get_site();
                    SITE.store(Arc::new(site));
                    info!("Site global info reloaded.");
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

pub(crate) fn get_layout_path() -> PathBuf {
    let dir = BASE_DIR.join("theme").join(&CONFIG.load().site.theme);
    if dir.exists() {
        dir
    } else {
        error!("Failed to init Tera");
        std::process::exit(1)
    }
}

pub(crate) static TERA: LazyLock<ArcSwap<Tera>> = LazyLock::new(|| {
    let layout_dir = get_layout_path();
    let tera = result_matcher!(
        Tera::new(&format!("{}/layout/*.html", layout_dir.to_string_lossy())),
        err_handler = |e| {
            error!("Parsing error(s): {}", e);
            std::process::exit(1)
        },
        ok_handler = |tera| {
            helper::Helpers::new().apply_to(tera);
        }
    );
    ArcSwap::from_pointee(tera)
});

async fn watch_layout(mut shutdown_rx: tokio::sync::broadcast::Receiver<()>) -> Result<()> {
    let theme_path = get_layout_path();

    // notify-debouncer-mini debounce window size: 1000ms
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
                result_matcher!(clone.full_reload(), "Failed to reload templates");
                TERA.store(Arc::new(clone));
                result_matcher!(render::render_all().await, "Failed to render posts");
                info!("TERA reloaded.");
            }
            else => {
                info!("Layout watcher channel closed");
                break;
            }
        }
    }

    Ok(())
}

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

/// Start watching.
/// # Arguments
/// * `shutdown_tx` - Subscribe the sender to recv a shutdown signal.
pub(crate) async fn start_watch(shutdown_tx: tokio::sync::broadcast::Sender<()>) {
    let _ = join! {
        watch_config(shutdown_tx.subscribe()),
        watch_source(shutdown_tx.subscribe()),
        watch_layout(shutdown_tx.subscribe()),
        watch_helper(shutdown_tx.subscribe()),
    };
}
