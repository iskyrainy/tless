//! Base site files: configuration, workflows, theme layouts and static
//! assets, all embedded at compile time.

use std::{fs, path::Path};

use anyhow::Result;

const CONFIG: &str = include_str!("tless.toml");
const GITIGNORE: &str = include_str!("gitignore");
const DEPLOY: &str = include_str!("deploy.yml");
const ROBOTS: &str = include_str!("robots.txt");

const BASE: &str = include_str!("base.html");
const INDEX: &str = include_str!("index.html");
const POST: &str = include_str!("post.html");
const PAGE: &str = include_str!("page.html");
const TAG: &str = include_str!("tag.html");
const TAG_INDEX: &str = include_str!("tag-index.html");
const CATEGORY: &str = include_str!("category.html");
const CATEGORY_INDEX: &str = include_str!("category-index.html");

const STYLE: &str = include_str!("style.css");
const HIGHLIGHT: &str = include_str!("highlight.css");
/// highlight.js v11.10.0 (BSD-3-Clause), vendored so that code highlighting
/// does not depend on a CDN at runtime.
const HIGHLIGHT_JS: &str = include_str!("highlight.js");
/// Themes applied inside the giscus iframe, one per colour scheme.
const GISCUS: &str = include_str!("giscus.css");
const GISCUS_DARK: &str = include_str!("giscus-dark.css");

/// Write the base site files, theme layouts and static assets into `site_dir`.
pub(crate) fn write_base_site(site_dir: &Path) -> Result<()> {
    fs::write(site_dir.join("tless.toml"), CONFIG)?;
    fs::write(site_dir.join(".gitignore"), GITIGNORE)?;

    let workflows = site_dir.join(".github").join("workflows");
    fs::create_dir_all(&workflows)?;
    fs::write(workflows.join("deploy.yml"), DEPLOY)?;

    // `copy_robots` picks this up from the source dir when generating
    fs::write(site_dir.join("source").join("robots.txt"), ROBOTS)?;

    let layout_dir = site_dir.join("theme").join("base").join("layout");
    fs::create_dir_all(&layout_dir)?;
    let layouts = [
        ("base.html", BASE),
        ("index.html", INDEX),
        ("post.html", POST),
        ("page.html", PAGE),
        ("tag.html", TAG),
        ("tag-index.html", TAG_INDEX),
        ("category.html", CATEGORY),
        ("category-index.html", CATEGORY_INDEX),
    ];
    for (name, text) in layouts {
        fs::write(layout_dir.join(name), text)?;
    }

    let assets_dir = site_dir.join("theme").join("base").join("assets");
    fs::create_dir_all(&assets_dir)?;
    let resources = [
        ("style.css", STYLE),
        ("highlight.css", HIGHLIGHT),
        ("highlight.js", HIGHLIGHT_JS),
        ("giscus.css", GISCUS),
        ("giscus-dark.css", GISCUS_DARK),
    ];
    for (name, text) in resources {
        fs::write(assets_dir.join(name), text)?;
    }
    Ok(())
}
