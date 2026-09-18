//! Site rendering: markdown to HTML, layout rendering, and every generated
//! output (pages, taxonomies, feed and sitemap).

use std::{
    cmp::Reverse,
    collections::HashMap,
    fmt::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local};
use chrono_tz::Tz;
use futures::{StreamExt, stream};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};
use tera::Context as TeraContext;
use tokio::{
    fs::{self, File},
    io::AsyncWriteExt,
};
use tracing::info;

use crate::{
    file::{Metadata, parse_file},
    server::{
        ClassMap, SITE, Site, TERA, extract_root_path, get_layout_path, get_public_path,
        get_source_path,
    },
    util::{get_cpu, slugify},
};

/// Markdown default render options.
const DEFAULT_OPTIONS: Options = Options::all();

/// Render markdown to HTML string, adding anchor ids to headings that match
/// the slugs produced by the `toc` helper.
#[inline]
fn render(markdown: &str) -> String {
    let events: Vec<Event> = Parser::new_ext(markdown, DEFAULT_OPTIONS).collect();
    let mut html_output = String::new();
    html::push_html(&mut html_output, add_heading_ids(events).into_iter());
    html_output
}

/// pulldown-cmark does not emit heading anchors; inject ids so that `toc`
/// links resolve. The heading text is collected the same way the `toc`
/// helper does, keeping both sides on the same slug.
fn add_heading_ids(events: Vec<Event<'_>>) -> Vec<Event<'_>> {
    let mut out = Vec::with_capacity(events.len());
    let mut i = 0;
    while i < events.len() {
        let Event::Start(Tag::Heading { level, .. }) = &events[i] else {
            out.push(events[i].clone());
            i += 1;
            continue;
        };
        let level = *level;
        let mut text = String::new();
        let mut end = i + 1;
        while end < events.len() {
            match &events[end] {
                Event::End(TagEnd::Heading(_)) => break,
                Event::Text(t) | Event::Code(t) => text.push_str(t),
                _ => {}
            }
            end += 1;
        }
        let slug = slugify(text.trim());
        out.push(Event::Start(Tag::Heading {
            level,
            id: (!slug.is_empty()).then(|| slug.into()),
            classes: Vec::new(),
            attrs: Vec::new(),
        }));
        out.extend(events[i + 1..=end.min(events.len() - 1)].iter().cloned());
        i = end + 1;
    }
    out
}


/// Render one page per term (`category.html` / `tag.html`) for the terms of
/// `metadata`, skipping terms that already have a page in this build.
async fn render_terms(
    terms: Option<&[String]>,
    classes: &HashMap<String, ClassMap>,
    dir: &str,
    layout: &str,
) -> Result<()> {
    let Some(terms) = terms else {
        return Ok(());
    };
    let out_root = Arc::new(get_public_path(dir));
    stream::iter(
        terms
            .iter()
            .filter(|term| !out_root.join(term.as_str()).exists()),
    )
    .map(|term| {
        let out_root = out_root.clone();
        async move {
            let dst_dir = out_root.join(term);
            fs::create_dir_all(&dst_dir).await?;
            let posts = classes
                .get(term)
                .map(|class| class.posts.clone())
                .unwrap_or_default();
            let mut context = TeraContext::new();
            context.insert("site", SITE.load().as_ref());
            context.insert("name", term);
            context.insert("title", term);
            context.insert("posts", &posts);
            match TERA.load().render(layout, &context) {
                Ok(rendered) => {
                    let mut file =
                        File::create(dst_dir.join("index.html"))
                            .await
                            .context(format!(
                                "Failed to create index.html in {}",
                                dst_dir.display()
                            ))?;
                    file.write_all_buf(&mut rendered.as_bytes())
                        .await
                        .context(format!(
                            "Failed to write index.html in {}",
                            dst_dir.display()
                        ))?;
                    file.flush().await.context(format!(
                        "Failed to flush index.html in {}",
                        dst_dir.display()
                    ))?;
                    Ok(())
                }
                Err(e) => bail!("Failed to render {dir} {term}: {e}"),
            }
        }
    })
    .buffer_unordered(get_cpu())
    .collect::<Vec<_>>()
    .await
    .into_iter()
    .collect::<Result<()>>()
}

#[inline]
/// Render the category and tag pages of one post.
async fn render_file_class(metadata: &Metadata) -> Result<()> {
    let site = SITE.load();
    render_terms(
        metadata.category.as_deref(),
        &site.category,
        "category",
        "category.html",
    )
    .await?;
    render_terms(metadata.tag.as_deref(), &site.tag, "tag", "tag.html").await?;
    Ok(())
}

enum RenderType {
    Post,
    Page,
}

#[inline]
/// Render one source file through its layout (frontmatter `layout`, else the
/// render-type default) into `dst`. Posts additionally emit taxonomy pages.
async fn render_file(src: &Path, dst: &Path, rt: RenderType) -> Result<()> {
    let (metadata, md_body) = parse_file(src)?;
    let md_html_str = render(&md_body);
    let mut context = TeraContext::new();
    context.insert("content", &md_html_str);
    context.insert("markdown", &md_body);
    context.insert("title", &metadata.title);
    context.insert("date", &metadata.date);
    context.insert("tag", &metadata.tag);
    context.insert("category", &metadata.category);
    context.insert("site", SITE.load().as_ref());
    let layout = metadata.layout.as_deref().unwrap_or(match rt {
        RenderType::Post => "post.html",
        RenderType::Page => "page.html",
    });
    match TERA.load().render(layout, &context) {
        Ok(rendered) => {
            let mut file = File::create(dst)
                .await
                .context(format!("Failed to create: {}", dst.display()))?;
            file.write_all_buf(&mut rendered.as_bytes())
                .await
                .context(format!("Failed to write: {}", dst.display()))?;
            file.flush()
                .await
                .context(format!("Failed to flush: {}", dst.display()))?;
        }
        Err(e) => {
            bail!("Failed to render {}: {}", metadata.title, e);
        }
    };
    if let RenderType::Post = rt {
        render_file_class(&metadata).await?;
    }
    Ok(())
}

/// Render posts to `public/post/<name>/index.html`.
pub(crate) async fn render_post(paths: Vec<&PathBuf>) -> Result<()> {
    let pub_dir = Arc::new(get_public_path("."));
    stream::iter(paths)
        .map(|path| {
            let pub_dir = pub_dir.clone();
            async move {
                if let Some(name) = path.file_stem() {
                    let name = name.to_string_lossy().to_string();
                    let dst_dir = pub_dir.join("post").join(&name);
                    fs::create_dir_all(&dst_dir)
                        .await
                        .context(format!("Failed to create public/post/{name}/"))?;
                    let dst_file = dst_dir.join("index.html");
                    render_file(path, &dst_file, RenderType::Post).await?;
                }
                Ok(())
            }
        })
        .buffer_unordered(get_cpu())
        .collect::<Vec<Result<()>>>()
        .await
        .into_iter()
        .collect::<Result<()>>()
}

/// Render pages to `public/<name>/index.html`.
pub(crate) async fn render_page(paths: Vec<&PathBuf>) -> Result<()> {
    let pub_dir = Arc::new(get_public_path("."));
    stream::iter(paths)
        .map(|path| {
            let pub_dir = pub_dir.clone();
            async move {
                if let Some(name) = path.file_stem() {
                    let name = name.to_string_lossy().to_string();
                    let dst_dir = pub_dir.join(&name);
                    fs::create_dir_all(&dst_dir)
                        .await
                        .context(format!("Failed to create public/page/{name}/"))?;
                    let dst_file = dst_dir.join("index.html");
                    render_file(path, &dst_file, RenderType::Page).await?;
                }
                Ok(())
            }
        })
        .buffer_unordered(get_cpu())
        .collect::<Vec<Result<()>>>()
        .await
        .into_iter()
        .collect::<Result<()>>()
}

/// Render the tag and category index pages.
async fn render_class() -> Result<()> {
    let mut context = TeraContext::new();
    context.insert("site", SITE.load().as_ref());
    context.insert("title", "Categories");
    let category_dir = get_public_path(".").join("category");
    fs::create_dir_all(&category_dir)
        .await
        .context("Failed to create public/category/")?;
    match TERA.load().render("category-index.html", &context) {
        Ok(rendered) => {
            let mut file = File::create(category_dir.join("index.html"))
                .await
                .context("Failed to create public/category/index.html")?;
            file.write_all_buf(&mut rendered.as_bytes())
                .await
                .context("Failed to write public/category/index.html")?;
            file.flush()
                .await
                .context("Failed to flush public/category/index.html")?;
        }
        Err(e) => {
            bail!("Failed to render category dir: {}", e);
        }
    };
    context.insert("title", "Tags");
    let tag_dir = get_public_path(".").join("tag");
    fs::create_dir_all(&tag_dir)
        .await
        .context("Failed to create public/tag/")?;
    match TERA.load().render("tag-index.html", &context) {
        Ok(rendered) => {
            let mut file = File::create(tag_dir.join("index.html"))
                .await
                .context("Failed to create public/tag/index.html")?;
            file.write_all_buf(&mut rendered.as_bytes())
                .await
                .context("Failed to write public/tag/index.html")?;
            file.flush()
                .await
                .context("Failed to flush public/tag/index.html")?;
        }
        Err(e) => {
            bail!("Failed to render tag dir: {}", e);
        }
    };
    Ok(())
}

#[inline]
/// Copy `source/robots.txt` into the build output when present.
async fn copy_robots() -> Result<()> {
    let src = get_source_path(".").join("robots.txt");
    if src.exists() {
        let dst = get_public_path(".").join("robots.txt");
        fs::copy(&src, &dst).await.context(format!(
            "Failed to copy from {} to {}",
            src.display(),
            dst.display()
        ))?;
    }
    Ok(())
}

#[inline]
/// Escape text for XML element and attribute content.
fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[inline]
/// Truncate to `max_chars` characters, appending `…` when shortened.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    let mut out: String = s.chars().take(max_chars).collect();
    if s.chars().count() > max_chars {
        out.push('…');
    }
    out
}

/// Build the Atom feed for all posts.
async fn gen_atom_str() -> String {
    let site = SITE.load();
    let mut xml = String::with_capacity(409600);
    let root_path = extract_root_path(&site.config.url);
    let root_esc = escape_xml(&root_path);

    // Feed
    let _ = writeln!(xml, r#"<?xml version="1.0" encoding="utf-8"?>"#);
    let _ = writeln!(xml, r#"<feed xmlns="http://www.w3.org/2005/Atom">"#);
    let _ = writeln!(
        xml,
        "  <author><name>{}</name></author>",
        escape_xml(&site.config.author)
    );
    let _ = writeln!(xml, "  <generator>Tless</generator>");
    let _ = writeln!(xml, "  <id>{root_esc}/atom.xml</id>");
    let _ = writeln!(xml, r#"  <link href="{root_esc}" rel="alternate"/>"#);
    let _ = writeln!(xml, r#"  <link href="{root_esc}/atom.xml" rel="self"/>"#);
    let _ = writeln!(
        xml,
        "  <rights>{}</rights>",
        escape_xml(&site.config.rights)
    );
    let _ = writeln!(
        xml,
        "  <subtitle>{}</subtitle>",
        escape_xml(&site.config.subtitle)
    );
    let _ = writeln!(xml, "  <title>{}</title>", escape_xml(&site.config.title));
    let _ = writeln!(xml, "  <updated>{}</updated>", Local::now().to_rfc3339());

    // Entries
    for post in &site.post {
        let Some(name) = post
            .path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
        else {
            continue;
        };
        let name_esc = escape_xml(&name);

        let _ = writeln!(xml, "  <entry>");
        if let Some(cates) = &post.category {
            for c in cates {
                let _ = writeln!(xml, "    <category>{}</category>", escape_xml(c));
            }
        }
        let content = fs::read_to_string(get_public_path("post").join(&name).join("index.html"))
            .await
            .unwrap_or_default();

        let _ = writeln!(
            xml,
            "    <content type=\"html\">{}</content>",
            escape_xml(&content)
        );

        let _ = writeln!(xml, "    <id>{root_esc}/post/{name_esc}</id>");
        let _ = writeln!(
            xml,
            r#"    <link href="{root_esc}/post/{name_esc}" rel="alternate"/>"#
        );
        let _ = writeln!(
            xml,
            r#"    <link href="{root_esc}/post/{name_esc}" rel="self"/>"#
        );

        let summary = truncate_chars(&content, 200);
        let _ = writeln!(
            xml,
            "    <summary type=\"html\">{}</summary>",
            escape_xml(&summary)
        );

        let _ = writeln!(xml, "    <published>{}</published>", post.date);
        let _ = writeln!(xml, "    <title>{}</title>", escape_xml(&post.title));
        if let Ok(m) = fs::metadata(&post.path).await
            && let Ok(updated) = m.modified()
        {
            let updated: DateTime<Local> = DateTime::from(updated);
            let _ = writeln!(xml, "    <updated>{}</updated>", updated.to_rfc3339());
        }
        let _ = writeln!(xml, "  </entry>");
    }
    let _ = writeln!(xml, "</feed>");

    xml
}

/// Write `public/atom.xml`.
async fn gen_atom() -> Result<()> {
    let dst = get_public_path("atom.xml");
    let atom_str = gen_atom_str().await;
    fs::write(dst, atom_str)
        .await
        .context("Failed to write public/atom.xml")?;
    Ok(())
}

/// Build the sitemap for all posts.
async fn gen_sitemap_str() -> String {
    let site = SITE.load();
    let mut xml = String::with_capacity(40960);
    let root_path = extract_root_path(&site.config.url);
    let root_esc = escape_xml(&root_path);

    let _ = writeln!(xml, r#"<?xml version="1.0" encoding="utf-8"?>"#);
    let _ = writeln!(
        xml,
        r#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">"#
    );
    for post in &site.post {
        let Some(name) = post
            .path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
        else {
            continue;
        };
        let name_esc = escape_xml(&name);
        let _ = writeln!(xml, "  <url>");
        let _ = writeln!(xml, r#"  <loc>{root_esc}/post/{name_esc}</loc>"#);
        if let Ok(m) = fs::metadata(&post.path).await
            && let Ok(updated) = m.modified()
        {
            let updated: DateTime<Local> = DateTime::from(updated);
            let _ = writeln!(xml, r#"  <lastmod>{}</lastmod>"#, updated);
        }
        let _ = writeln!(xml, "  </url>");
    }
    let _ = writeln!(xml, "</urlset>");

    xml
}

/// Write `public/sitemap.xml`.
async fn gen_sitemap() -> Result<()> {
    let dst = get_public_path("sitemap.xml");
    let sitemap_str = gen_sitemap_str().await;
    fs::write(dst, sitemap_str)
        .await
        .context("Failed to write public/sitemap.xml")?;
    Ok(())
}

/// Render the whole site into `public/`: posts, pages, taxonomies, the home
/// page, theme assets, feed and sitemap.
pub async fn render_all() -> Result<()> {
    let site = SITE.load();
    // start from a clean build directory
    remove_stale_outputs().await?;
    copy_theme_resources()?;
    copy_robots().await?;

    render_home(&site).await?;
    render_class().await?;
    render_post(site.post.iter().map(|d| &d.path).collect::<Vec<_>>()).await?;
    render_page(site.page.iter().map(|d| &d.path).collect::<Vec<_>>()).await?;

    gen_atom().await?;
    gen_sitemap().await?;
    Ok(())
}

/// Wipe the previous build output so every run starts clean.
async fn remove_stale_outputs() -> Result<()> {
    let target = crate::BASE_DIR.join("public");
    if target.exists() {
        fs::remove_dir_all(&target)
            .await
            .context("Failed to remove public/")?;
        fs::create_dir_all(&target)
            .await
            .context("Failed to create public/")?;
    }
    Ok(())
}

/// Render the theme's `index.html` layout as the site home page.
async fn render_home(site: &Site) -> Result<()> {
    let tera = TERA.load();
    if !tera
        .get_template_names()
        .any(|name| name == "index.html" || name == "index.md")
    {
        info!("Skipping home page");
        return Ok(());
    }
    let mut context = TeraContext::new();
    // empty values keep `{% if content %}` / `{% if title %}` blocks happy
    context.insert("content", "");
    context.insert("title", "");
    context.insert("recent_posts", &recent_posts(site));
    context.insert("site", site);
    match tera.render("index.html", &context) {
        Ok(rendered) => {
            fs::write(get_public_path("index.html"), rendered)
                .await
                .context("Failed to write public/home/index.html")?;
            info!("Render public/home/index.html");
        }
        Err(e) => bail!("Failed to render public/home/index.html: {}", e),
    }
    Ok(())
}

/// Posts from `source/post`, newest first, exposed to the home page template.
fn recent_posts(site: &Site) -> Vec<Metadata> {
    let post_dir = get_source_path("post");
    let mut posts = site
        .post
        .iter()
        .filter(|m| m.path.starts_with(&post_dir))
        .cloned()
        .collect::<Vec<_>>();
    posts.sort_by_key(|p| Reverse(date_rank(&p.date)));
    posts
}

/// Parse a frontmatter date (RFC3339 format);
/// posts without a usable date sort last.
fn date_rank(date: &str) -> DateTime<Tz> {
    let tz = SITE.load().config.zone();
    DateTime::parse_from_rfc3339(date)
        .map(|d| d.with_timezone(&tz))
        .ok()
        .unwrap_or(DateTime::<Tz>::MIN_UTC.with_timezone(&tz))
}

/// Copy the active theme's `assets/` directory into `public/assets/`.
fn copy_theme_resources() -> Result<()> {
    let resource_dir = get_layout_path().join("assets");
    if !resource_dir.exists() {
        return Ok(());
    }
    copy_dir_recursive(&resource_dir, &get_public_path("assets"))
}

/// Recursively copy the `src` tree into `dst`.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).context(format!("Failed to create dir: {}", dst.display()))?;
    for entry in std::fs::read_dir(src).context(format!("Failed to read dir: {}", src.display()))? {
        let entry = entry.context("Failed to get theme entry")?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).context(format!(
                "Failed to copy from {} to {}",
                from.display(),
                to.display()
            ))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_adds_heading_anchors_matching_toc_slugs() {
        let html = render("# Hello, World!\n\n## Section Two\n\n### Install `tless`");
        assert!(html.contains(r#"<h1 id="hello-world">"#));
        assert!(html.contains(r#"<h2 id="section-two">"#));
        assert!(html.contains(r#"<h3 id="install-tless">"#));
        assert_eq!(slugify("Hello, World!"), "hello-world");
    }

    #[test]
    fn render_omits_ids_for_symbol_only_headings() {
        // smart punctuation turns `---` into an em dash, leaving no usable slug
        let html = render("## ---\n");
        assert!(html.contains("<h2>"));
        assert!(!html.contains("<h2 id="));
    }
}
