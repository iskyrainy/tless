//! Server state: the site model, the template engine and the file watchers.

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
mod i18n;
mod render;
mod run;
mod site;
mod template;

pub use i18n::Language;
pub use i18n::translate;
pub use render::render_all;
pub use run::run;
pub use site::init;

/// Struct of global source info, including `post`, `page`.
/// # Fields
/// * `post` - List of all post metadata.
/// * `page` - List of all page metadata.
/// * `category` - Map of all categories.
/// * `tag` - Map of all tags.
/// * `config` - Config loaded from `tless.toml`.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct Site {
    pub post: Vec<Metadata>,
    pub page: Vec<Metadata>,
    pub category: HashMap<String, ClassMap>,
    pub tag: HashMap<String, ClassMap>,
    pub config: SiteConfig,
    i18n: I18nConfig,
    giscus: GiscusConfig,
}

impl Site {
    pub fn new() -> Self {
        Site {
            post: vec![],
            page: vec![],
            category: HashMap::new(),
            tag: HashMap::new(),
            config: SiteConfig::default(),
            i18n: I18nConfig::default(),
            giscus: GiscusConfig::default(),
        }
    }

    pub fn get_i18n_tl(&self) -> &Vec<String> {
        &self.i18n.target_lang
    }
}

/// A taxonomy term (category or tag) and the posts filed under it.
/// # Fields
/// * `path` - URL path of the term, e.g. `/tag/rust`.
/// * `posts` - Posts belonging to this term.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct ClassMap {
    pub path: String,
    pub posts: Vec<Metadata>,
}

/// Configuration structure for the application.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Config {
    pub site: SiteConfig,
    pub i18n: Option<I18nConfig>,
    pub giscus: Option<GiscusConfig>,
}

/// `[giscus]` configuration for post comments.
/// # Fields
/// * `repo` - GitHub repository holding the discussions, `owner/name`.
/// * `repo_id` - The repository's node id, from https://giscus.app.
/// * `category` - Discussion category the comments are filed under.
/// * `category_id` - The category's node id.
/// * `mapping` - How a page maps to a discussion, e.g. `pathname`.
/// * `lang` - Language of the giscus interface.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct GiscusConfig {
    pub repo: String,
    pub repo_id: String,
    pub category: String,
    pub category_id: String,
    pub mapping: String,
    pub lang: String,
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
}

impl SiteConfig {
    /// Timezone configured in `[site] zone`, falling back to UTC.
    #[inline]
    pub(crate) fn zone(&self) -> Tz {
        self.zone.trim().parse().unwrap_or(Tz::UTC)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct I18nConfig {
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub target_lang: Vec<String>,
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

/// Path to the configuration file (`tless.toml`).
#[inline]
fn config_path() -> PathBuf {
    BASE_DIR.join("tless.toml")
}

/// Load `tless.toml` from the working directory.
pub(crate) fn load() -> Result<Config> {
    let path = config_path();
    if !path.exists() {
        bail!("Configuration file not found at {}", path.display());
    }
    let text = fs::read_to_string(path)?;
    Ok(toml::from_str(&text)?)
}

/// Timezone of the configured site, falling back to UTC.
pub(crate) fn zone() -> Tz {
    let site = SITE.load();
    site.config.zone()
}

/// Get the path to the source dir (`./source`) in the current directory.
#[inline]
pub(crate) fn get_source_path<'a, S: Into<&'a str>>(name: S) -> PathBuf {
    BASE_DIR.join("source").join(name.into())
}

/// Path part of a site URL, e.g. `/blog` for `https://example.com/blog`.
#[inline]
pub(crate) fn extract_root_path(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    match rest.find('/') {
        Some(pos) => rest[pos..].trim_end_matches('/').to_string(),
        None => String::new(),
    }
}

/// Load [Metadata] of every source file into `SITE`, skipping unparseable files.
fn get_site() -> Site {
    let post_dir = get_source_path("post");
    let page_dir = get_source_path("page");
    let mut site = Site::new();
    (site.config, site.i18n, site.giscus) = match load() {
        Ok(config) => (
            config.site,
            config.i18n.unwrap_or_default(),
            config.giscus.unwrap_or_default(),
        ),
        Err(e) => error::fatal(format!("{e:#}")),
    };

    let class_path = |c: &str, t: &str, base_url: &str| -> String {
        format!("{}/{t}/{c}", extract_root_path(base_url))
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

/// Re-render changed sources and reload the site model.
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
/// Directory of the theme selected in `[site] theme`.
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

/// Reload templates and re-render the site when the theme changes.
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
/// Path inside the generated `public/` directory.
pub(crate) fn get_public_path<'a, S: Into<&'a str>>(name: S) -> PathBuf {
    BASE_DIR.join("public").join(name.into())
}

/// Recompile Rhai helpers when the helper directory changes.
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

/// Run all file watchers until a shutdown signal is received.
pub(crate) async fn start_watch(shutdown_tx: tokio::sync::broadcast::Sender<()>) {
    let _ = join! {
        watch_logged("source", watch_source(shutdown_tx.subscribe())),
        watch_logged("layout", watch_layout(shutdown_tx.subscribe())),
        watch_logged("helper", watch_helper(shutdown_tx.subscribe())),
    };
}
