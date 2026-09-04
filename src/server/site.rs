use std::{env, fs, path::Path};

use anyhow::{Context, Result, bail};

/// Initialize a complete, deploy-ready site scaffold in the current directory.
///
/// Everything is created at the repository root: `tless.toml`, the source and
/// theme directories, plus the `.github/workflows/deploy.yml` and `.gitignore`
/// needed to publish the site to GitHub Pages.
pub fn init() -> Result<()> {
    let current_dir = env::current_dir().context("Cannot get current directory")?;
    if current_dir.join("tless.toml").exists() {
        bail!("Site already initialized in this directory");
    }

    fs::write(current_dir.join("tless.toml"), base_config_text())?;
    fs::write(current_dir.join(".gitignore"), base_gitignore_text())?;

    // Empty directories that must survive in git get a .gitkeep
    let tracked_dirs = [
        "helper",
        "plugin",
        "source/draft",
        "source/post",
        "source/page",
    ];
    for dir in tracked_dirs {
        let dir = current_dir.join(dir);
        fs::create_dir_all(&dir)?;
        fs::write(dir.join(".gitkeep"), "")?;
    }
    // Build output, ignored by .gitignore
    fs::create_dir_all(current_dir.join("public"))?;

    let workflows = current_dir.join(".github").join("workflows");
    fs::create_dir_all(&workflows)?;
    fs::write(workflows.join("deploy.yml"), base_deploy_yml_text())?;

    let layout_dir = current_dir.join("theme").join("base").join("layout");
    fs::create_dir_all(&layout_dir)?;
    let resource_dir = current_dir.join("theme").join("base").join("resource");
    fs::create_dir_all(resource_dir)?;
    write_base_theme(&layout_dir)?;
    Ok(())
}

fn write_base_theme(layout_dir: &Path) -> Result<()> {
    fs::write(layout_dir.join("index.html"), base_index_theme_text())?;
    fs::write(layout_dir.join("archive.html"), base_archive_theme_text())?;
    fs::write(layout_dir.join("category.html"), base_category_theme_text())?;
    Ok(())
}

/// Generate the `.gitignore` for a site repository.
fn base_gitignore_text() -> &'static str {
    r#"# Generated build output
public/

# Editor and OS noise
.DS_Store
*.swp
*~
"#
}

/// Generate the GitHub Pages deployment workflow.
fn base_deploy_yml_text() -> &'static str {
    r#"name: Deploy to GitHub Pages

on:
  push:
    branches: [main]
  workflow_dispatch:

permissions:
  contents: read
  pages: write
  id-token: write

concurrency:
  group: pages
  cancel-in-progress: false

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      # Point this at your fork of tless if you maintain one
      - name: Install tless
        run: cargo install --git https://github.com/iskyrainy/tless --locked

      - name: Build static site
        run: tless site -g

      - name: Upload Pages artifact
        uses: actions/upload-pages-artifact@v3
        with:
          path: public

  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - name: Deploy to GitHub Pages
        id: deployment
        uses: actions/deploy-pages@v4
"#
}

/// Generate a base configuration file content.
fn base_config_text() -> String {
    String::from(
        r#"# Tless configuration
# Update these values for your own site before publishing.
[site]
title = "My Tless Site"
description = "A fast blog powered by Tless."
author = "Your Name"
url = "http://127.0.0.1:8917"
zone = "UTC"
theme = "base"
favicon = ""
menu = [
    { name = "Home", link = "/index.html" },
    { name = "Example Post", link = "/archives/hello-tless" },
    { name = "Rust Tag", link = "/tags/rust" },
    { name = "General Category", link = "/categories/general" }
]
"#,
    )
}

fn base_style_text() -> &'static str {
    r#"
        /* PaperMod-inspired palette */
        :root {
            color-scheme: light;
            --gap: 24px;
            --content-gap: 20px;
            --nav-width: 1024px;
            --main-width: 720px;
            --header-height: 60px;
            --radius: 8px;

            --theme: #ffffff;
            --entry: #ffffff;
            --primary: #1e1e1e;
            --secondary: #6c6c6c;
            --tertiary: #d6d6d6;
            --content: #1f1f1f;
            --code-bg: #f5f5f5;
            --code-block-bg: #1c1d21;
            --border: #eeeeee;
        }

        html[data-theme="dark"] {
            color-scheme: dark;
            --theme: #1d1e20;
            --entry: #2e2e33;
            --primary: rgb(218 218 219);
            --secondary: rgb(155 156 157);
            --tertiary: rgb(65 66 68);
            --content: rgb(196 196 197);
            --code-bg: #37383e;
            --code-block-bg: #2e2e33;
            --border: #333333;
        }

        @media (prefers-color-scheme: dark) {
            html:not([data-theme="light"]) {
                color-scheme: dark;
                --theme: #1d1e20;
                --entry: #2e2e33;
                --primary: rgb(218 218 219);
                --secondary: rgb(155 156 157);
                --tertiary: rgb(65 66 68);
                --content: rgb(196 196 197);
                --code-bg: #37383e;
                --code-block-bg: #2e2e33;
                --border: #333333;
            }
        }

        * {
            box-sizing: border-box;
        }

        html {
            scroll-behavior: smooth;
        }

        body {
            margin: 0;
            background: var(--theme);
            color: var(--content);
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Oxygen, Ubuntu, Cantarell, "Fira Sans", "Droid Sans", "Helvetica Neue", sans-serif;
            font-size: 16px;
            line-height: 1.75;
            -webkit-font-smoothing: antialiased;
            overflow-wrap: break-word;
            transition: background-color 0.3s ease, color 0.3s ease;
        }

        a {
            color: var(--primary);
            text-decoration: none;
        }

        a:hover {
            text-decoration: underline;
        }

        h1, h2, h3, h4 {
            color: var(--primary);
            line-height: 1.3;
        }

        /* Header */
        .nav {
            position: sticky;
            top: 0;
            z-index: 50;
            background: var(--theme);
            border-bottom: 1px solid var(--border);
            transition: background-color 0.3s ease;
        }

        .nav-inner {
            display: flex;
            align-items: center;
            justify-content: space-between;
            gap: 12px;
            max-width: var(--nav-width);
            height: var(--header-height);
            margin: 0 auto;
            padding: 0 var(--gap);
        }

        .site-name {
            font-size: 18px;
            font-weight: 700;
            letter-spacing: 0.02em;
        }

        .site-name a {
            color: var(--primary);
        }

        .site-name a:hover {
            text-decoration: none;
        }

        .nav-links {
            display: flex;
            align-items: center;
            gap: 18px;
            font-size: 14px;
        }

        .nav-links a {
            color: var(--primary);
            white-space: nowrap;
        }

        .nav-links a:hover {
            text-decoration: underline;
            text-underline-offset: 4px;
        }

        /* Theme toggle */
        .theme-toggle {
            display: inline-flex;
            align-items: center;
            justify-content: center;
            width: 34px;
            height: 34px;
            padding: 0;
            border: none;
            border-radius: var(--radius);
            background: transparent;
            color: var(--primary);
            cursor: pointer;
        }

        .theme-toggle:hover {
            background: var(--code-bg);
        }

        .icon-sun {
            display: none;
        }

        html[data-theme="dark"] .icon-moon {
            display: none;
        }

        html[data-theme="dark"] .icon-sun {
            display: block;
        }

        /* Main column */
        .main {
            max-width: var(--main-width);
            margin: 0 auto;
            padding: var(--gap) var(--gap) 0;
        }

        .recent-title {
            margin: var(--content-gap) 0;
            font-size: 17px;
            font-weight: 600;
            letter-spacing: 0.04em;
            text-transform: uppercase;
            color: var(--secondary);
        }

        /* Entry cards (home list) */
        .post-entry {
            position: relative;
            margin-bottom: var(--gap);
            padding: var(--gap);
            background: var(--entry);
            border: 1px solid var(--border);
            border-radius: var(--radius);
            transition: transform 0.25s ease, border-color 0.25s ease;
        }

        .post-entry:hover {
            transform: translateY(-2px);
            border-color: var(--tertiary);
        }

        .entry-header h2 {
            margin: 0;
            font-size: 24px;
            line-height: 1.3;
        }

        .entry-header a:hover {
            text-decoration: underline;
            text-underline-offset: 4px;
        }

        .entry-footer {
            display: flex;
            align-items: baseline;
            flex-wrap: wrap;
            gap: 12px;
            margin-top: 10px;
            color: var(--secondary);
            font-size: 13px;
        }

        .entry-tags {
            display: flex;
            flex-wrap: wrap;
            gap: 8px;
        }

        .entry-tags span {
            color: var(--secondary);
        }

        /* Single post */
        .post-title {
            margin: var(--content-gap) 0 0;
            font-size: 40px;
            line-height: 1.2;
        }

        .post-meta {
            margin-top: 6px;
            color: var(--secondary);
            font-size: 14px;
        }

        /* Table of contents */
        details.toc {
            margin: var(--content-gap) 0;
            background: var(--code-bg);
            border: 1px solid var(--border);
            border-radius: var(--radius);
        }

        details.toc summary {
            padding: 0.3rem 1.2rem;
            border-radius: var(--radius);
            cursor: pointer;
            font-size: 14px;
            color: var(--secondary);
            user-select: none;
        }

        details.toc ul {
            list-style: none;
            margin: 0;
            padding: 0.4rem 1.2rem 0.8rem 2.2rem;
            border-top: 1px solid var(--border);
        }

        details.toc li {
            font-size: 14px;
            line-height: 1.9;
        }

        details.toc a {
            color: var(--secondary);
        }

        details.toc a:hover {
            color: var(--primary);
            text-decoration: none;
        }

        /* Post content */
        .post-content {
            margin: 30px 0;
            color: var(--content);
            font-size: 16px;
            line-height: 1.75;
        }

        .post-content > :first-child {
            margin-top: 0;
        }

        .post-content h1, .post-content h2, .post-content h3, .post-content h4 {
            margin: 2em 0 1em;
        }

        .post-content h1 {
            font-size: 1.6em;
        }

        .post-content h2 {
            font-size: 1.4em;
        }

        .post-content h3 {
            font-size: 1.2em;
        }

        .post-content p {
            margin: 1em 0;
        }

        .post-content a {
            text-decoration: underline;
            text-decoration-color: var(--tertiary);
            text-underline-offset: 0.2em;
        }

        .post-content a:hover {
            text-decoration-color: var(--secondary);
        }

        .post-content img {
            max-width: 100%;
            height: auto;
            border-radius: 4px;
        }

        .post-content blockquote {
            margin: 1.5em 0;
            padding: 0 1em;
            border-left: 2px solid var(--tertiary);
            color: var(--secondary);
        }

        .post-content hr {
            margin: 2em 0;
            border: none;
            border-top: 1px solid var(--border);
        }

        .post-content pre {
            overflow-x: auto;
            padding: 16px 20px;
            background: var(--code-block-bg);
            border-radius: var(--radius);
            color: #e8e8e8;
            font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace;
            font-size: 14px;
            line-height: 1.6;
        }

        .post-content code {
            padding: 2px 5px;
            background: var(--code-bg);
            border-radius: 4px;
            font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace;
            font-size: 0.9em;
        }

        .post-content pre code {
            padding: 0;
            background: none;
            font-size: 1em;
        }

        .post-content ul, .post-content ol {
            padding-left: 1.6em;
        }

        .post-content li {
            margin: 0.3em 0;
        }

        .post-content table {
            width: 100%;
            margin: 1.5em 0;
            border-collapse: collapse;
            font-size: 0.95em;
        }

        .post-content th, .post-content td {
            padding: 8px 12px;
            border: 1px solid var(--border);
            text-align: left;
        }

        .post-content th {
            background: var(--code-bg);
            color: var(--primary);
            font-weight: 600;
        }

        /* Helper list rows (taxonomy pages) */
        .entry-list .ul {
            list-style: none;
            margin: 0;
            padding: 0;
        }

        .entry-list .li {
            border-bottom: 1px solid var(--border);
        }

        .entry-list .a {
            display: block;
            padding: 14px 4px;
            color: var(--primary);
            font-size: 17px;
        }

        .entry-list .a:hover {
            text-decoration: underline;
            text-underline-offset: 4px;
        }

        /* Taxonomy pills */
        .taxonomy-grid {
            display: grid;
            grid-template-columns: 1fr 1fr;
            gap: var(--gap);
            margin: var(--gap) 0 calc(var(--gap) * 2);
        }

        .taxonomy-card h2 {
            margin: 0 0 12px;
            font-size: 16px;
            color: var(--secondary);
            letter-spacing: 0.04em;
            text-transform: uppercase;
        }

        .taxonomy-pills .a {
            display: inline-block;
            margin: 0 6px 8px 0;
            padding: 0 14px;
            border: 1px solid var(--border);
            border-radius: var(--radius);
            background: var(--code-bg);
            color: var(--secondary);
            font-size: 14px;
            line-height: 34px;
        }

        .taxonomy-pills .a:hover {
            background: var(--border);
            color: var(--primary);
            text-decoration: none;
        }

        .taxonomy-pills .count {
            color: var(--secondary);
            font-size: 12px;
            margin-left: 2px;
        }

        /* Pagination (rendered by the paginator helper) */
        .pagination {
            display: flex;
            align-items: center;
            flex-wrap: wrap;
            gap: 8px;
            margin: var(--gap) 0 calc(var(--gap) * 2);
        }

        .pagination-list {
            display: flex;
            gap: 6px;
            list-style: none;
            margin: 0;
            padding: 0;
        }

        .pagination-prev, .pagination-next, .pagination-link, .pagination-current {
            display: inline-flex;
            align-items: center;
            justify-content: center;
            min-width: 32px;
            height: 32px;
            padding: 0 10px;
            border: 1px solid var(--border);
            border-radius: var(--radius);
            background: var(--code-bg);
            color: var(--secondary);
            font-size: 14px;
        }

        .pagination-prev:hover, .pagination-next:hover, .pagination-link:hover {
            background: var(--border);
            color: var(--primary);
            text-decoration: none;
        }

        .pagination-current {
            border-color: var(--tertiary);
            color: var(--primary);
            font-weight: 600;
        }

        /* Footer */
        .footer {
            max-width: var(--main-width);
            margin: 0 auto;
            padding: calc(var(--gap) * 2) var(--gap) var(--gap);
            color: var(--secondary);
            font-size: 13px;
            text-align: center;
        }

        .no-posts {
            color: var(--secondary);
        }

        @media (max-width: 600px) {
            .nav-inner {
                padding: 0 16px;
            }

            .nav-links {
                gap: 12px;
                font-size: 13px;
            }

            .post-title {
                font-size: 32px;
            }

            .post-entry {
                padding: 18px;
            }

            .taxonomy-grid {
                grid-template-columns: 1fr;
            }
        }

        @media (prefers-reduced-motion: reduce) {
            html {
                scroll-behavior: auto;
            }

            .post-entry {
                transition: none;
            }

            .post-entry:hover {
                transform: none;
            }
        }
    "#
}

/// Shared `<head>`: theme pre-paint script, title and inline styles.
fn base_head(title: &str) -> String {
    format!(
        r#"<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{title}</title>
    <script>
    (() => document.documentElement.dataset.theme = localStorage.getItem('tless-theme')
        || (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'))();
    </script>
    <style>{css}</style>
</head>
"#,
        css = base_style_text(),
        title = title,
    )
}

fn base_header() -> &'static str {
    r#"<header class="nav">
    <div class="nav-inner">
        <div class="site-name"><a href="/">Tless</a></div>
        <nav class="nav-links">
            {{ link(path="/", text="Home") }}
            {{ link(path="/about", text="About") }}
            {{ link(path="/tags", text="Tags") }}
            {{ link(path="/categories", text="Categories") }}
            <button class="theme-toggle" id="theme-toggle" type="button" aria-label="Toggle theme">
                <svg class="icon-moon" viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8Z"/></svg>
                <svg class="icon-sun" viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>
            </button>
        </nav>
    </div>
</header>
"#
}

fn base_footer() -> &'static str {
    r#"<footer class="footer">
    <span>© Tless · Built with Tless</span>
</footer>
<script>
const toggleTheme = () => {
    const next = document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark';
    document.documentElement.dataset.theme = next;
    localStorage.setItem('tless-theme', next);
};
document.getElementById('theme-toggle').addEventListener('click', toggleTheme);
</script>
"#
}

fn base_index_theme_text() -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
{head}
<body>
{header}
<main class="main">
    {{% if content %}}
    <article class="post">
        <h1 class="post-title">{{{{ title | default(value="Home") }}}}</h1>
        <div class="post-content">{{{{ content | safe }}}}</div>
    </article>
    {{% else %}}
    <section class="recent-posts">
        <h2 class="recent-title">Recent Posts</h2>
        {{% for p in site.posts %}}
        <article class="post-entry">
            <header class="entry-header">
                <h2><a href="{{{{ p.title }}}}">{{{{ p.title }}}}</a></h2>
            </header>
            <footer class="entry-footer">
                {{% if p.date %}}
                <span class="entry-date"><time>{{{{ date(ts=p.date, fmt="%Y-%m-%d") }}}}</time></span>
                {{% endif %}}
                {{% if p.tags %}}
                <span class="entry-tags">{{% for t in p.tags %}}<span>#{{{{ t }}}}</span>{{% endfor %}}</span>
                {{% endif %}}
            </footer>
        </article>
        {{% else %}}
        <p class="no-posts">No posts yet. Write one with: tless blog add &lt;name&gt; &amp;&amp; tless blog publish &lt;name&gt;</p>
        {{% endfor %}}
    </section>
    {{% endif %}}
</main>
{footer}
</body>
</html>
"#,
        head = base_head(r#"{{ title | default(value="Home") }} · Tless"#),
        header = base_header(),
        footer = base_footer(),
    )
}

fn base_archive_theme_text() -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
{head}
<body>
{header}
<main class="main">
    <article class="post">
        <h1 class="post-title">{{{{ title | default(value="Article") }}}}</h1>
        <div class="post-meta">
            {{% if date %}}
            <time datetime="{{{{ date }}}}">{{{{ date(ts=date, fmt="%Y-%m-%d") }}}}</time>
            {{% endif %}}
        </div>
        <details class="toc" open>
            <summary>Table of contents</summary>
            {{{{ toc(content=markdown, max_level=3) | safe }}}}
        </details>
        <div class="post-content">{{{{ content | safe }}}}</div>
    </article>
</main>
{footer}
</body>
</html>
"#,
        head = base_head(r#"{{ title | default(value="Article") }} · Tless"#),
        header = base_header(),
        footer = base_footer(),
    )
}

fn base_category_theme_text() -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
{head}
<body>
{header}
<main class="main">
    <h1 class="post-title">{{{{ name | default(value="Taxonomy") }}}}</h1>
    <div class="post-meta">Posts in this taxonomy.</div>
    <section class="entry-list">
        {{{{ list_posts(order=-1, list=true, amount=20, show_count=false) | safe }}}}
    </section>
    <div class="pagination">
        {{{{ paginator(current=1, total=3, base="?page=") | safe }}}}
    </div>
    <div class="taxonomy-grid">
        <section class="taxonomy-card">
            <h2>Categories</h2>
            <div class="taxonomy-pills">
                {{{{ list_categories(order=-1, list=false, separator=" ", show_count=true) | safe }}}}
            </div>
        </section>
        <section class="taxonomy-card">
            <h2>Tags</h2>
            <div class="taxonomy-pills">
                {{{{ list_tags(order=-1, list=false, separator=" ", show_count=true) | safe }}}}
            </div>
        </section>
    </div>
</main>
{footer}
</body>
</html>
"#,
        head = base_head(r#"{{ name | default(value="Taxonomy") }} · Tless"#),
        header = base_header(),
        footer = base_footer(),
    )
}
