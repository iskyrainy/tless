use std::{env, fs, path::Path};

use anyhow::{Context, Result, bail};

/// Initialize the site structure in the current directory.
pub fn init() -> Result<()> {
    let current_dir = env::current_dir().context("Cannot get current directory")?;
    let current_dir = current_dir.join("blog");
    if current_dir.exists() {
        bail!("Site directory already exists.");
    }
    fs::create_dir(&current_dir)?;

    let conf_path = current_dir.join("tless.toml");
    fs::write(&conf_path, base_config_text())?;

    let dirs = vec!["source", "theme", "public", "plugin", "helper", "statistic"];
    for dir in dirs {
        fs::create_dir(current_dir.join(dir))?;
    }
    let blog_dirs = vec!["draft", "post", "page"];
    for dir in blog_dirs {
        fs::create_dir(current_dir.join("source").join(dir))?;
    }

    let theme_dir = current_dir.join("theme").join("base");
    fs::create_dir(&theme_dir)?;
    let layout_dir = theme_dir.join("layout");
    fs::create_dir(&layout_dir)?;
    let resource_dir = theme_dir.join("resource");
    fs::create_dir(&resource_dir)?;
    write_base_theme(&layout_dir)?;
    Ok(())
}

fn write_base_theme(layout_dir: &Path) -> Result<()> {
    fs::write(layout_dir.join("index.html"), base_index_theme_text())?;
    fs::write(layout_dir.join("archive.html"), base_archive_theme_text())?;
    fs::write(layout_dir.join("category.html"), base_category_theme_text())?;
    Ok(())
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
    { name = "Home", link = "/" },
    { name = "Example Post", link = "/archives/hello-tless" },
    { name = "Rust Tag", link = "/tags/rust" },
    { name = "General Category", link = "/categories/general" }
]

[auth]
ak = ""
allows = [
    "127.0.0.1"
]
"#,
    )
}

fn base_style_text() -> &'static str {
    r#"
        :root {
            color-scheme: light dark;
            --bg: #fafafa;
            --surface: #ffffff;
            --border: #e4e4e7;
            --text: #18181b;
            --muted: #71717a;
            --accent: #0d9488;
            --accent-strong: #0f766e;
            --accent-soft: rgba(13, 148, 136, 0.1);
            --on-accent: #ffffff;
            --radius: 12px;
            --max: 720px;
        }

        @media (prefers-color-scheme: dark) {
            :root {
                --bg: #101012;
                --surface: #18181b;
                --border: #27272a;
                --text: #f4f4f5;
                --muted: #a1a1aa;
                --accent: #2dd4bf;
                --accent-strong: #5eead4;
                --accent-soft: rgba(45, 212, 191, 0.12);
                --on-accent: #101012;
            }
        }

        * {
            box-sizing: border-box;
        }

        body {
            margin: 0;
            background: var(--bg);
            color: var(--text);
            font-family: ui-sans-serif, system-ui, -apple-system, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
            line-height: 1.7;
            -webkit-font-smoothing: antialiased;
            transition: background-color 0.3s ease, color 0.3s ease;
        }

        @media (prefers-reduced-motion: reduce) {
            body {
                transition: none;
            }
        }

        a {
            color: var(--accent-strong);
            text-decoration: none;
        }

        a:hover {
            text-decoration: underline;
        }

        .container {
            max-width: var(--max);
            margin: 0 auto;
            padding: 0 24px;
        }

        /* Header */
        .site-header {
            position: sticky;
            top: 0;
            z-index: 10;
            background: color-mix(in srgb, var(--bg) 85%, transparent);
            backdrop-filter: blur(10px);
            -webkit-backdrop-filter: blur(10px);
            border-bottom: 1px solid var(--border);
        }

        .header-inner {
            display: flex;
            align-items: center;
            justify-content: space-between;
            flex-wrap: wrap;
            gap: 8px 24px;
            padding-top: 14px;
            padding-bottom: 14px;
        }

        .site-logo {
            font-size: 1.1rem;
            font-weight: 700;
            letter-spacing: -0.02em;
            color: var(--text);
        }

        .site-logo:hover {
            text-decoration: none;
            opacity: 0.8;
        }

        .site-nav {
            display: flex;
            flex-wrap: wrap;
            gap: 4px 18px;
            font-size: 0.9rem;
        }

        .site-nav a {
            color: var(--muted);
            transition: color 0.2s;
        }

        .site-nav a:hover {
            color: var(--text);
            text-decoration: none;
        }

        /* Main */
        main {
            padding: 48px 0 80px;
        }

        .page-header {
            margin: 0 0 36px;
        }

        .page-title {
            margin: 0 0 8px;
            font-size: clamp(1.9rem, 4vw, 2.5rem);
            font-weight: 700;
            letter-spacing: -0.03em;
            line-height: 1.2;
        }

        .page-description {
            margin: 0;
            color: var(--muted);
            font-size: 1.02rem;
        }

        /* Cards */
        .card {
            padding: 28px;
            background: var(--surface);
            border: 1px solid var(--border);
            border-radius: var(--radius);
        }

        .section-content {
            margin-bottom: 20px;
        }

        .section-title {
            margin: 0 0 16px;
            font-size: 1rem;
            font-weight: 600;
            letter-spacing: -0.01em;
        }

        /* Post list rendered by the list_* helpers */
        .post-list-wrap .ul {
            list-style: none;
            margin: 0;
            padding: 0;
        }

        .post-list-wrap .li + .li {
            border-top: 1px solid var(--border);
        }

        .post-list-wrap .a {
            display: block;
            padding: 15px 0;
            color: var(--text);
            font-size: 1.02rem;
            font-weight: 500;
            letter-spacing: -0.01em;
        }

        .post-list-wrap .a:hover {
            color: var(--accent-strong);
            text-decoration: none;
        }

        .post-list-wrap .count {
            color: var(--muted);
            font-size: 0.8rem;
            margin-left: 4px;
        }

        /* Inline taxonomy links */
        .taxonomy-wrap .a {
            display: inline-block;
            margin: 0 6px 8px 0;
            padding: 3px 12px;
            border: 1px solid var(--border);
            border-radius: 999px;
            font-size: 0.875rem;
            color: var(--text);
            background: var(--surface);
            transition: border-color 0.2s, color 0.2s;
        }

        .taxonomy-wrap .a:hover {
            border-color: var(--accent);
            color: var(--accent-strong);
            text-decoration: none;
        }

        .taxonomy-wrap .count {
            color: var(--muted);
            font-size: 0.8rem;
            margin-left: 2px;
        }

        .taxonomy-grid {
            display: grid;
            grid-template-columns: 1fr 1fr;
            gap: 20px;
            margin-top: 20px;
        }

        /* Table of contents */
        .toc {
            margin-bottom: 28px;
            padding: 14px 18px;
            border: 1px solid var(--border);
            border-radius: var(--radius);
            background: var(--surface);
        }

        .toc summary {
            cursor: pointer;
            font-size: 0.875rem;
            font-weight: 600;
            color: var(--muted);
            user-select: none;
        }

        .toc ul {
            list-style: none;
            margin: 10px 0 0;
            padding: 0 0 0 14px;
            border-left: 1px solid var(--border);
        }

        .toc li + li {
            margin-top: 6px;
        }

        .toc a {
            font-size: 0.875rem;
            color: var(--muted);
        }

        .toc a:hover {
            color: var(--accent-strong);
            text-decoration: none;
        }

        /* Article content */
        .prose {
            font-size: 1.05rem;
        }

        .prose h1, .prose h2, .prose h3, .prose h4 {
            margin: 2.2rem 0 1rem;
            letter-spacing: -0.02em;
            line-height: 1.3;
        }

        .prose h1 {
            font-size: 1.6rem;
        }

        .prose h2 {
            font-size: 1.4rem;
        }

        .prose h3 {
            font-size: 1.2rem;
        }

        .prose p {
            margin: 0 0 1.2rem;
        }

        .prose a {
            text-decoration: underline;
            text-decoration-color: color-mix(in srgb, var(--accent) 40%, transparent);
            text-underline-offset: 3px;
        }

        .prose a:hover {
            text-decoration-color: var(--accent);
        }

        .prose img {
            max-width: 100%;
            border-radius: var(--radius);
        }

        .prose blockquote {
            margin: 1.5rem 0;
            padding: 2px 0 2px 18px;
            border-left: 3px solid var(--accent);
            color: var(--muted);
        }

        .prose pre {
            overflow-x: auto;
            padding: 16px 20px;
            border: 1px solid var(--border);
            border-radius: var(--radius);
            background: var(--surface);
            font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace;
            font-size: 0.9rem;
            line-height: 1.6;
        }

        .prose code {
            padding: 2px 6px;
            border-radius: 4px;
            background: var(--accent-soft);
            font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace;
            font-size: 0.88em;
        }

        .prose pre code {
            padding: 0;
            background: none;
            font-size: 1em;
        }

        .prose hr {
            margin: 2.5rem 0;
            border: none;
            border-top: 1px solid var(--border);
        }

        .prose ul, .prose ol {
            padding-left: 1.4rem;
        }

        .prose li {
            margin: 0.3rem 0;
        }

        .prose table {
            width: 100%;
            margin: 1.5rem 0;
            border-collapse: collapse;
            font-size: 0.95rem;
        }

        .prose th, .prose td {
            padding: 8px 12px;
            border: 1px solid var(--border);
            text-align: left;
        }

        .prose th {
            background: var(--surface);
            font-weight: 600;
        }

        /* Pagination */
        .pagination {
            display: flex;
            align-items: center;
            gap: 12px;
            margin-top: 28px;
            font-size: 0.9rem;
        }

        .pagination-list {
            display: flex;
            gap: 4px;
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
            padding: 0 8px;
            border: 1px solid var(--border);
            border-radius: 8px;
            color: var(--text);
        }

        .pagination-prev:hover, .pagination-next:hover, .pagination-link:hover {
            border-color: var(--accent);
            text-decoration: none;
        }

        .pagination-current {
            background: var(--accent);
            border-color: var(--accent);
            color: var(--on-accent);
            font-weight: 600;
        }

        /* Footer */
        .site-footer {
            padding: 24px 0 40px;
            border-top: 1px solid var(--border);
            color: var(--muted);
            font-size: 0.875rem;
        }

        .footer-inner {
            display: flex;
            flex-wrap: wrap;
            justify-content: space-between;
            gap: 8px;
        }

        .site-footer a {
            color: var(--muted);
            text-decoration: underline;
            text-underline-offset: 3px;
        }

        .site-footer a:hover {
            color: var(--text);
        }

        @media (max-width: 640px) {
            main {
                padding: 32px 0 56px;
            }

            .card {
                padding: 20px;
            }

            .taxonomy-grid {
                grid-template-columns: 1fr;
            }
        }
    "#
}

fn base_header() -> &'static str {
    r#"    <header class="site-header">
        <div class="container header-inner">
            <a class="site-logo" href="/">Tless</a>
            <nav class="site-nav">
                <a href="/">Home</a>
                <a href="/post/test">Example Post</a>
                <a href="/tags/rust">Rust</a>
                <a href="/categories/general">General</a>
            </nav>
        </div>
    </header>
"#
}

fn base_footer() -> &'static str {
    r#"    <footer class="site-footer">
        <div class="container footer-inner">
            <span>© Tless</span>
            <span>Built with Tless</span>
        </div>
    </footer>
"#
}

fn base_index_theme_text() -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{{{{ title | default(value="Home") }}}} · Tless</title>
    <style>{}</style>
</head>
<body>
{}
    <main class="container">
        <section class="page-header">
            <h1 class="page-title">{{{{ title | default(value="Home") }}}}</h1>
            <p class="page-description">A clean, minimal theme with automatic dark mode.</p>
        </section>
        {{% if content %}}
        <section class="card section-content">
            <div class="prose">{{{{ content | safe }}}}</div>
        </section>
        {{% endif %}}
        <section class="card">
            <h2 class="section-title">Recent posts</h2>
            <div class="post-list-wrap">
                {{{{ list_posts(order=-1, list=true, amount=10, show_count=false) | safe }}}}
            </div>
        </section>
    </main>
{}
</body>
</html>
"#,
        base_style_text(),
        base_header(),
        base_footer(),
    )
}

fn base_archive_theme_text() -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{{{{ title | default(value="Article") }}}} · Tless</title>
    <style>{}</style>
</head>
<body>
{}
    <main class="container">
        <article>
            <header class="page-header">
                <h1 class="page-title">{{{{ title | default(value="Article") }}}}</h1>
                {{% if date %}}
                <p class="page-description">{{{{ date }}}}</p>
                {{% endif %}}
            </header>
            <details class="toc" open>
                <summary>Table of contents</summary>
                {{{{ toc(content=content, max_level=3) | safe }}}}
            </details>
            <div class="prose">{{{{ content | safe }}}}</div>
        </article>
    </main>
{}
</body>
</html>
"#,
        base_style_text(),
        base_header(),
        base_footer(),
    )
}

fn base_category_theme_text() -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{{{{ name | default(value="Taxonomy") }}}} · Tless</title>
    <style>{}</style>
</head>
<body>
{}
    <main class="container">
        <section class="page-header">
            <h1 class="page-title">{{{{ name | default(value="Taxonomy") }}}}</h1>
            <p class="page-description">Posts in this taxonomy.</p>
        </section>
        <section class="card">
            <div class="post-list-wrap">
                {{{{ list_posts(order=-1, list=true, amount=20, show_count=false) | safe }}}}
            </div>
            <div class="pagination">
                {{{{ paginator(current=1, total=3, base="?page=") | safe }}}}
            </div>
        </section>
        <div class="taxonomy-grid">
            <section class="card">
                <h2 class="section-title">Categories</h2>
                <div class="taxonomy-wrap">
                    {{{{ list_categories(order=-1, list=false, separator=" ", show_count=true) | safe }}}}
                </div>
            </section>
            <section class="card">
                <h2 class="section-title">Tags</h2>
                <div class="taxonomy-wrap">
                    {{{{ list_tags(order=-1, list=false, separator=" ", show_count=true) | safe }}}}
                </div>
            </section>
        </div>
    </main>
{}
</body>
</html>
"#,
        base_style_text(),
        base_header(),
        base_footer(),
    )
}
