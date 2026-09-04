//! Built-in Tera template functions and Rhai helper loading.

use std::{fmt::Write, fs, path::Path, sync::Arc};

use chrono::{DateTime, NaiveDateTime, Utc};
use data_encoding::HEXUPPER;
use pulldown_cmark::{Event, HeadingLevel, Parser as MarkdownParser, Tag, TagEnd};
use rhai::{AST, Dynamic, Engine, Map as RhaiMap};
use sha2::{Digest, Sha256};
use tera::{Context, Error, Kwargs, Map, State, Tera, TeraResult, Value};
use tracing::info;

use crate::server::{CONFIG, SITE, TERA, extract_root_path};

/// Register all built-in template functions on `tera`.
pub(crate) fn register_helpers(tera: &mut Tera) {
    tera.register_function("date", date_helper);
    tera.register_function("url_for", url_helper);
    tera.register_function("full_url_for", full_url_helper);
    tera.register_function("gravatar", gravatar_helper);
    tera.register_function("partial", partial_helper);
    tera.register_function("paginator", paginator_helper);
    tera.register_function("number_format", number_format_helper);
    tera.register_function("open_graph", open_graph_helper);
    tera.register_function("toc", toc_helper);
    register_tag_helpers(tera);
    register_list_helpers(tera);
}

/// Fallback amount for list helpers when no `amount` arg is given (effectively unlimited).
const DEFAULT_AMOUNT: usize = 1 << 16;

/// Parse a date string as RFC3339 or the `%Y-%m-%d %H:%M:%S` format used by the CLI.
fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|dt| dt.and_utc())
        })
}

fn is_absolute_url(path: &str) -> bool {
    path.starts_with("http://") || path.starts_with("https://")
}

/// Join a relative path onto a base URL without duplicate slashes.
fn join_url(base: &str, path: &str) -> String {
    if path.starts_with('/') {
        format!("{}{}", base.trim_end_matches('/'), path)
    } else {
        format!("{}/{}", base.trim_end_matches('/'), path)
    }
}

fn date_helper(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let ts = match kwargs.get::<Value>("ts")? {
        Some(ts) => {
            if let Some(ts) = ts.as_i64() {
                ts
            } else if let Some(ts) = ts.as_f64() {
                ts as i64
            } else if let Some(s) = ts.as_str() {
                parse_datetime(s).unwrap_or_else(Utc::now).timestamp()
            } else {
                return Err(Error::message(
                    "Invalid 'ts': expected an epoch number or a date string",
                ));
            }
        }
        None => Utc::now().timestamp(),
    };
    let fmt = kwargs
        .get::<String>("fmt")?
        .unwrap_or_else(|| "%Y-%m-%d %H:%M:%S".to_string());
    let date = DateTime::from_timestamp(ts, 0).unwrap_or_else(Utc::now);
    Ok(Value::normal_string(&date.format(&fmt).to_string()))
}

fn build_tag(
    tag: &str,
    path_attr: &str,
    attrs: &[(&str, &str)],
    val: &Value,
    text: Option<&str>,
) -> TeraResult<String> {
    let mut html = String::new();
    if let Some(s) = val.as_str() {
        let mut path = s.to_string();
        if !path.starts_with('/') && !path.starts_with("http") && !path.starts_with("mailto:") {
            path = format!("/{path}");
        }

        html.push_str(&format!("<{tag}"));
        for (k, v) in attrs {
            let _ = write!(html, r#" {k}="{v}""#);
        }
        let _ = write!(html, r#" {path_attr}="{}">"#, escape_html_attr(&path));
        if let Some(text) = text {
            let _ = write!(html, "{text}</{tag}>");
        }
        return Ok(html);
    }

    if let Some(map) = val.as_map() {
        html.push_str(&format!("<{tag}"));
        for (k, v) in attrs {
            let _ = write!(html, r#" {k}="{v}""#);
        }
        for (k, v) in map {
            let s = v.as_str().ok_or_else(|| {
                Error::message(format!(
                    "Invalid type for key {}",
                    k.as_str().unwrap_or_default()
                ))
            })?;
            let mut val = s.to_string();
            if k.as_str() == Some(path_attr)
                && !s.starts_with('/')
                && !s.starts_with("http")
                && !s.starts_with("mailto:")
            {
                val = format!("/{val}");
            }
            let _ = write!(
                html,
                r#" {}="{}""#,
                k.as_str().unwrap_or_default(),
                escape_html_attr(&val)
            );
        }
        html.push('>');
        if let Some(text) = text {
            let _ = write!(html, "{text}</{tag}>");
        }
        return Ok(html);
    }

    Err(Error::message("Invalid path type"))
}

/// Build an HTML tag (`css`, `js`, `link`, ...) for a path or a map of attributes.
fn tag_call(
    kwargs: Kwargs,
    tag: &str,
    path_attr: &str,
    attrs: &[(&str, &str)],
) -> TeraResult<Value> {
    let path = kwargs.must_get::<Value>("path")?;
    let text = kwargs.get("text")?;
    let html = match path.as_array() {
        Some(paths) => paths
            .iter()
            .map(|p| build_tag(tag, path_attr, attrs, p, text))
            .collect::<TeraResult<Vec<_>>>()?
            .join("\n"),
        None => build_tag(tag, path_attr, attrs, &path, text)?,
    };
    Ok(Value::safe_string(&html))
}

fn register_tag_helpers(tera: &mut Tera) {
    tera.register_function(
        "css",
        |kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
            tag_call(kwargs, "link", "href", &[("rel", "stylesheet")])
        },
    );
    tera.register_function(
        "js",
        |kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
            tag_call(kwargs, "script", "src", &[])
        },
    );
    tera.register_function(
        "link",
        |kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
            tag_call(kwargs, "a", "href", &[])
        },
    );
    tera.register_function(
        "image",
        |kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
            tag_call(kwargs, "img", "src", &[])
        },
    );
    tera.register_function(
        "mail",
        |kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
            tag_call(kwargs, "a", "href", &[])
        },
    );
    tera.register_function(
        "favicon",
        |kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
            tag_call(kwargs, "link", "href", &[("rel", "icon")])
        },
    );
    tera.register_function(
        "feed",
        |kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
            tag_call(
                kwargs,
                "link",
                "href",
                &[("rel", "alternate"), ("type", "application/rss+xml")],
            )
        },
    );
    tera.register_function(
        "meta",
        |kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
            tag_call(kwargs, "meta", "content", &[("name", "generator")])
        },
    );
}

fn url_helper(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let site_url = &CONFIG.load().site.url;
    let path = kwargs.must_get::<String>("path")?;
    let relative = kwargs.get::<bool>("relative")?.unwrap_or(true);
    let res = if relative {
        if path.starts_with('/') {
            format!(".{path}")
        } else {
            format!("./{path}")
        }
    } else {
        join_url(&extract_root_path(site_url), &path)
    };
    Ok(Value::normal_string(&res))
}

fn full_url_helper(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let site_url = &CONFIG.load().site.url;
    let path = kwargs.must_get::<String>("path")?;
    Ok(Value::normal_string(&join_url(site_url, &path)))
}

fn gravatar_helper(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let mail = kwargs.must_get::<String>("mail")?;
    let mut hashed_email = Sha256::new();
    hashed_email.update(mail.trim());
    let hash = HEXUPPER.encode(hashed_email.finalize().as_ref());
    let url = format!("https://www.gravatar.com/avatar/{hash}");
    Ok(Value::normal_string(&url))
}

fn partial_helper(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let name = kwargs.get::<String>("name")?.unwrap_or_default();
    if name.is_empty() {
        return Ok(Value::none());
    }
    let tera = TERA.load();
    let rendered = tera.render(&format!("{name}.html"), &Context::new())?;
    Ok(Value::safe_string(&rendered))
}

#[derive(Clone, Copy)]
enum ListKind {
    Category,
    Tag,
    Post,
    Page,
}

/// Class of a `tag_class` map entry, falling back to the default.
fn class_of(map: &Map, key: &str, default: &str) -> String {
    map.iter()
        .find(|(k, _)| k.as_str() == Some(key))
        .and_then(|(_, v)| v.as_str())
        .unwrap_or(default)
        .to_string()
}

fn list_call(kwargs: Kwargs, kind: ListKind) -> TeraResult<Value> {
    let orderby = kwargs
        .get::<String>("orderby")?
        .unwrap_or_else(|| "name".to_string());
    let order = kwargs.get::<i64>("order")?.unwrap_or(1);
    let show_count = kwargs.get::<bool>("show_count")?.unwrap_or(true);
    let list = kwargs.get::<bool>("list")?.unwrap_or(true);
    let separator = kwargs
        .get::<String>("separator")?
        .unwrap_or_else(|| ",".to_string());
    let amount = kwargs.get::<usize>("amount")?.unwrap_or(DEFAULT_AMOUNT);
    let tag_class = kwargs.get::<Map>("tag_class")?.unwrap_or_default();
    let ul_class = class_of(&tag_class, "ul", "ul");
    let li_class = class_of(&tag_class, "li", "li");
    let a_class = class_of(&tag_class, "a", "a");
    let count_class = class_of(&tag_class, "count", "count");

    let mut res = String::new();
    let site = &SITE.load();

    let render_list = |res: &mut String, iter: Vec<(String, String, usize)>| {
        res.push_str(&format!(r#"<ul class="{ul_class}" itemprop="keywords">"#));
        for (i, (name, href, count)) in iter.into_iter().enumerate() {
            if i >= amount {
                break;
            }
            res.push_str(&format!(r#"<li class="{li_class}">"#));
            res.push_str(&format!(
                r#"<a class="{a_class}" href="{href}">{}</a>"#,
                escape_html_text(&name)
            ));
            if show_count && count > 0 {
                res.push_str(&format!(r#"<span class="{count_class}">{count}</span>"#));
            }
            res.push_str("</li>");
        }
        res.push_str("</ul>");
    };

    let render_inline = |res: &mut String, iter: Vec<(String, String, usize)>| {
        for (i, (name, href, count)) in iter.into_iter().enumerate() {
            if i >= amount {
                break;
            }
            // the count badge is rendered inside the link
            res.push_str(&format!(
                r#"<a class="{a_class}" href="{href}">{}"#,
                escape_html_text(&name)
            ));
            if show_count && count > 0 {
                res.push_str(&format!(r#"<span class="{count_class}">{count}</span>"#));
            }
            res.push_str(&format!("</a>{separator}"));
        }
    };

    match kind {
        ListKind::Category | ListKind::Tag => {
            let data = match kind {
                ListKind::Category => site.categories.iter(),
                ListKind::Tag => site.tags.iter(),
                _ => unreachable!(),
            };
            let mut tmp: Vec<_> = data
                .map(|(k, v)| (k.clone(), v.path.clone(), v.posts.len()))
                .collect();
            match orderby.as_str() {
                "name" => {
                    if order == -1 {
                        tmp.sort_by(|x, y| y.0.cmp(&x.0));
                    } else {
                        tmp.sort_by(|x, y| x.0.cmp(&y.0));
                    }
                }
                "count" => {
                    if order == -1 {
                        tmp.sort_by_key(|y| std::cmp::Reverse(y.2));
                    } else {
                        tmp.sort_by_key(|x| x.2);
                    }
                }
                _ => {}
            }

            if list {
                render_list(&mut res, tmp);
            } else {
                render_inline(&mut res, tmp);
            }
        }
        ListKind::Post | ListKind::Page => {
            let mut tmp = match kind {
                ListKind::Post => site.posts.clone(),
                ListKind::Page => site.pages.clone(),
                _ => unreachable!(),
            };
            tmp.sort_by(|x, y| {
                let x_date = parse_datetime(&x.date).unwrap_or_else(Utc::now);
                let y_date = parse_datetime(&y.date).unwrap_or_else(Utc::now);
                if order == -1 {
                    y_date.cmp(&x_date)
                } else {
                    x_date.cmp(&y_date)
                }
            });
            let mapped: Vec<_> = tmp
                .into_iter()
                .map(|p| {
                    let title = p.title;
                    (title.clone(), title, 0)
                })
                .collect();

            if list {
                render_list(&mut res, mapped);
            } else {
                render_inline(&mut res, mapped);
            }
        }
    }
    Ok(Value::safe_string(&res))
}

fn register_list_helpers(tera: &mut Tera) {
    for (name, kind) in [
        ("list_categories", ListKind::Category),
        ("list_tags", ListKind::Tag),
        ("list_posts", ListKind::Post),
        ("list_pages", ListKind::Page),
    ] {
        tera.register_function(
            name,
            move |kwargs: Kwargs, _state: &State| -> TeraResult<Value> { list_call(kwargs, kind) },
        );
    }
}

fn paginator_helper(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let current = kwargs.get::<i64>("current")?.unwrap_or(1).max(1);
    let total = kwargs.get::<i64>("total")?.unwrap_or(1).max(1);
    let window = kwargs.get::<i64>("window")?.unwrap_or(2).max(0);
    let base = escape_html_attr(
        &kwargs
            .get::<String>("base")?
            .unwrap_or_else(|| "?page=".to_string()),
    );
    let prev_text = escape_html_text(
        &kwargs
            .get::<String>("prev_text")?
            .unwrap_or_else(|| "Prev".to_string()),
    );
    let next_text = escape_html_text(
        &kwargs
            .get::<String>("next_text")?
            .unwrap_or_else(|| "Next".to_string()),
    );

    let current = current.min(total);
    let start = (current - window).max(1);
    let end = (current + window).min(total);
    let mut html = String::from(r#"<nav class="pagination" aria-label="Pagination">"#);

    if current > 1 {
        let prev = current - 1;
        let _ = write!(
            html,
            r#"<a class="pagination-prev" href="{base}{prev}">{prev_text}</a>"#
        );
    }

    html.push_str(r#"<ol class="pagination-list">"#);
    for page in start..=end {
        if page == current {
            let _ = write!(
                html,
                r#"<li><span class="pagination-current" aria-current="page">{page}</span></li>"#
            );
        } else {
            let _ = write!(
                html,
                r#"<li><a class="pagination-link" href="{base}{page}">{page}</a></li>"#
            );
        }
    }
    html.push_str("</ol>");

    if current < total {
        let next = current + 1;
        let _ = write!(
            html,
            r#"<a class="pagination-next" href="{base}{next}">{next_text}</a>"#
        );
    }

    html.push_str("</nav>");
    Ok(Value::safe_string(&html))
}

fn number_format_helper(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let Some(value) = kwargs.get::<Value>("value")? else {
        return Err(Error::message("Missing 'value'"));
    };
    let value = if let Some(s) = value.as_str() {
        s.to_string()
    } else if let Some(i) = value.as_i64() {
        i.to_string()
    } else if let Some(u) = value.as_u64() {
        u.to_string()
    } else if let Some(f) = value.as_f64() {
        f.to_string()
    } else {
        return Err(Error::message(
            "Invalid 'value': expected a number or a string",
        ));
    };
    let separator = kwargs
        .get::<String>("separator")?
        .unwrap_or_else(|| ",".to_string());

    let formatted = format_number_with_separator(&value, &separator);
    Ok(Value::normal_string(&formatted))
}

fn open_graph_helper(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let config = CONFIG.load();
    let title = kwargs
        .get::<String>("title")?
        .unwrap_or_else(|| config.site.title.clone());
    let description = kwargs
        .get::<String>("description")?
        .unwrap_or_else(|| config.site.description.clone());
    let url = match kwargs.get::<String>("url")? {
        Some(path) if is_absolute_url(&path) => path,
        Some(path) => join_url(&config.site.url, &path),
        None => config.site.url.clone(),
    };
    let image = match kwargs.get::<String>("image")? {
        Some(path) if is_absolute_url(&path) => path,
        Some(path) => join_url(&config.site.url, &path),
        None => String::new(),
    };
    let kind = kwargs
        .get::<String>("type")?
        .unwrap_or_else(|| "website".to_string());

    let mut tags = vec![
        ("og:title", title),
        ("og:description", description),
        ("og:type", kind),
        ("og:url", url),
        ("og:site_name", config.site.title.clone()),
    ];
    if !image.is_empty() {
        tags.push(("og:image", image));
    }

    let mut html = String::new();
    for (name, content) in tags {
        let _ = writeln!(
            html,
            r#"<meta property="{name}" content="{}">"#,
            escape_html_attr(&content)
        );
    }
    Ok(Value::safe_string(&html))
}

fn toc_helper(kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let content = kwargs.must_get::<String>("content")?;
    let max_level = kwargs.get::<usize>("max_level")?.unwrap_or(6);

    let mut items = Vec::new();
    let mut current_level = None;
    let mut current_text = String::new();

    for event in MarkdownParser::new(&content) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                current_level = Some(level_to_usize(level));
                current_text.clear();
            }
            Event::Text(text) | Event::Code(text) => {
                if current_level.is_some() {
                    current_text.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(..)) => {
                if let Some(level) = current_level.take()
                    && level <= max_level
                    && !current_text.trim().is_empty()
                {
                    let text = current_text.trim().to_string();
                    items.push((level, text.clone(), slugify(&text)));
                }
                current_text.clear();
            }
            _ => {}
        }
    }

    if items.is_empty() {
        return Ok(Value::safe_string(""));
    }

    let mut html = String::from(r#"<nav class="toc" aria-label="Table of contents"><ul>"#);
    for (level, text, slug) in items {
        let _ = write!(
            html,
            r##"<li class="toc-level-{level}"><a href="#{slug}">{}</a></li>"##,
            escape_html_text(&text)
        );
    }
    html.push_str("</ul></nav>");
    Ok(Value::safe_string(&html))
}

fn format_number_with_separator(value: &str, separator: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return "0".to_string();
    }

    let (sign, unsigned) = value
        .strip_prefix('-')
        .map(|rest| ("-", rest))
        .unwrap_or(("", value));
    let mut split = unsigned.splitn(2, '.');
    let int_part = split.next().unwrap_or_default();
    let frac_part = split.next();

    let mut grouped_rev = String::new();
    for (i, ch) in int_part.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            grouped_rev.push_str(separator);
        }
        grouped_rev.push(ch);
    }
    let int_formatted: String = grouped_rev.chars().rev().collect();

    match frac_part {
        Some(frac) if !frac.is_empty() => format!("{sign}{int_formatted}.{frac}"),
        _ => format!("{sign}{int_formatted}"),
    }
}

fn level_to_usize(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn slugify(input: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in input.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            slug.push(lower);
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

fn escape_html_attr(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_html_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Rhai engine safety limits for untrusted helper scripts.
const MAX_OPERATIONS: u64 = 1_000_000;
const MAX_EXPR_DEPTHS: (usize, usize) = (32, 64);
const MAX_CALL_LEVELS: usize = 64;

fn value_to_dynamic(v: &Value) -> Dynamic {
    if v.is_none() {
        return Dynamic::UNIT;
    }
    if let Some(b) = v.as_bool() {
        return Dynamic::from_bool(b);
    }
    if let Some(i) = v.as_i64() {
        return Dynamic::from_int(i);
    }
    if let Some(f) = v.as_f64() {
        return Dynamic::from_float(f);
    }
    if let Some(s) = v.as_str() {
        return Dynamic::from(s.to_string());
    }
    if let Some(a) = v.as_array() {
        let mut arr = Vec::with_capacity(a.len());
        for e in a {
            arr.push(value_to_dynamic(e));
        }
        return Dynamic::from_array(arr);
    }
    if let Some(m) = v.as_map() {
        let mut map = RhaiMap::new();
        for (k, val) in m {
            if let Some(k) = k.as_str() {
                map.insert(k.into(), value_to_dynamic(val));
            }
        }
        return Dynamic::from_map(map);
    }
    Dynamic::UNIT
}

fn dynamic_to_value(res: Dynamic) -> Value {
    if res.is::<String>() {
        Value::normal_string(&res.cast::<String>())
    } else if res.is::<i64>() {
        Value::from(res.cast::<i64>())
    } else if res.is::<f64>() {
        Value::from(res.cast::<f64>())
    } else if res.is::<bool>() {
        Value::from(res.cast::<bool>())
    } else if res.is::<()>() {
        Value::none()
    } else if res.is::<rhai::ImmutableString>() {
        // also string-like
        Value::normal_string(&res.to_string())
    } else if res.is_array() || res.is_map() {
        // Serialize via JSON string as fallback
        match Value::try_from_serializable(&res) {
            Ok(v) => v,
            Err(_) => Value::normal_string(&res.to_string()),
        }
    } else {
        Value::normal_string(&res.to_string())
    }
}

fn rhai_call(kwargs: Kwargs, engine: &Engine, ast: &AST) -> TeraResult<Value> {
    let mut scope = rhai::Scope::new();
    // all template call args are passed to `fn main(args)` as one map
    let mut arg_map = RhaiMap::new();
    for (k, v) in kwargs.iter() {
        if let Some(k) = k.as_str() {
            arg_map.insert(k.into(), value_to_dynamic(v));
        }
    }

    let res = engine
        .call_fn::<Dynamic>(&mut scope, ast, "main", (arg_map,))
        .map_err(|e| Error::message(format!("Rhai call error: {e}")))?;
    Ok(dynamic_to_value(res))
}

type CompiledRhai = (String, Arc<Engine>, Arc<AST>);

/// Compile every `*.rhai` file in the helper dir. Each script must define
/// `fn main(args)`; `call` is a reserved keyword in Rhai.
pub(crate) fn compile_rhai_helpers(helpers_dir: impl AsRef<Path>) -> TeraResult<Vec<CompiledRhai>> {
    let dir = helpers_dir.as_ref();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut engine = Engine::new();

    engine.set_max_operations(MAX_OPERATIONS);
    engine.set_max_expr_depths(MAX_EXPR_DEPTHS.0, MAX_EXPR_DEPTHS.1);
    engine.set_max_call_levels(MAX_CALL_LEVELS);
    engine.set_allow_looping(false);
    engine.set_optimization_level(rhai::OptimizationLevel::Simple);
    engine.disable_symbol("eval");
    engine.disable_symbol("import");
    engine.disable_symbol("use");

    let engine = Arc::new(engine);

    let mut helpers = Vec::new();
    for entry in
        fs::read_dir(dir).map_err(|e| Error::message(format!("Failed to read helper dir: {e}")))?
    {
        let entry = entry.map_err(|e| Error::message(format!("Failed to read helper dir: {e}")))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("rhai") {
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let script = fs::read_to_string(&path)
                .map_err(|e| Error::message(format!("Failed to read {name}.rhai: {e}")))?;
            // precompile AST
            let ast = engine
                .compile(&script)
                .map_err(|e| Error::message(format!("Compile error in {name}: {e}")))?;
            helpers.push((name.to_string(), engine.clone(), Arc::new(ast)));
            info!("Registered Rhai helper: {name}");
        }
    }
    Ok(helpers)
}

/// Register compiled Rhai helpers on a template engine.
pub(crate) fn register_rhai_helpers(tera: &mut Tera, helpers: Vec<CompiledRhai>) {
    for (name, engine, ast) in helpers {
        tera.register_function(
            name,
            move |kwargs: Kwargs, _state: &State| -> TeraResult<Value> {
                rhai_call(kwargs, &engine, &ast)
            },
        );
    }
}

/// Recompile the helper dir and register new or changed helpers into the live
/// template engine. Called by the helper dir watcher.
pub(crate) fn load_rhai_helpers(helpers_dir: impl AsRef<Path>) -> TeraResult<()> {
    let helpers = compile_rhai_helpers(helpers_dir)?;
    if helpers.is_empty() {
        return Ok(());
    }
    let tera = TERA.load();
    let mut tera = tera.as_ref().clone();
    register_rhai_helpers(&mut tera, helpers);
    TERA.store(Arc::new(tera));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use tera::Context;

    fn render(template: &str) -> String {
        render_result(template).unwrap()
    }

    fn render_result(template: &str) -> TeraResult<String> {
        let mut tera = Tera::new();
        register_helpers(&mut tera);
        tera.render_str(template, &Context::new(), false)
    }

    #[test]
    fn date_helper_epoch_and_formats() {
        assert_eq!(render("{{ date(ts=0) }}"), "1970-01-01 00:00:00");
        assert_eq!(render(r#"{{ date(ts=0, fmt="%Y-%m-%d") }}"#), "1970-01-01");
        assert_eq!(
            render(r#"{{ date(ts=1788318000, fmt="%Y-%m-%d") }}"#),
            "2026-09-02"
        );
    }

    #[test]
    fn date_helper_string_inputs() {
        // RFC3339 and the %Y-%m-%d %H:%M:%S format written by the CLI
        assert_eq!(
            render(r#"{{ date(ts="2026-09-01T12:00:00Z") }}"#),
            "2026-09-01 12:00:00"
        );
        assert_eq!(
            render(r#"{{ date(ts="2026-09-01 12:00:00") }}"#),
            "2026-09-01 12:00:00"
        );
    }

    #[test]
    fn date_helper_defaults_to_now() {
        let year = Utc::now().format("%Y").to_string();
        assert_eq!(render(r#"{{ date(fmt="%Y") }}"#), year);
        // unparseable strings fall back to now as well
        assert_eq!(render(r#"{{ date(ts="garbage", fmt="%Y") }}"#), year);
    }

    #[test]
    fn date_helper_rejects_non_date_types() {
        let err = render_result(r#"{{ date(ts=true) }}"#).unwrap_err();
        assert!(err.to_string().contains("expected an epoch number"));
    }

    #[test]
    fn number_format_helper_groups_digits() {
        assert_eq!(render("{{ number_format(value=1234567) }}"), "1,234,567");
        assert_eq!(render("{{ number_format(value=-1234.5) }}"), "-1,234.5");
        assert_eq!(render("{{ number_format(value=42) }}"), "42");
        assert_eq!(render(r#"{{ number_format(value="") }}"#), "0");
        assert_eq!(
            render(r#"{{ number_format(value="9876543", separator=".") }}"#),
            "9.876.543"
        );
        // numeric strings pass through untouched
        assert_eq!(
            render(r#"{{ number_format(value="1234567") }}"#),
            "1,234,567"
        );
    }

    #[test]
    fn number_format_helper_missing_value() {
        let err = render_result("{{ number_format(separator=',') }}").unwrap_err();
        assert!(err.to_string().contains("Missing 'value'"));
    }

    #[test]
    fn gravatar_helper_sha256_url() {
        let mut hasher = Sha256::new();
        hasher.update("test@example.com");
        let expected = format!(
            "https://www.gravatar.com/avatar/{}",
            HEXUPPER.encode(hasher.finalize().as_ref())
        );
        assert_eq!(
            render(r#"{{ gravatar(mail="test@example.com") }}"#),
            expected
        );
    }

    #[test]
    fn tag_helpers_render_expected_html() {
        assert_eq!(
            render(r#"{{ css(path="/style.css") }}"#),
            r#"<link rel="stylesheet" href="/style.css">"#
        );
        assert_eq!(
            render(r#"{{ js(path="app.js") }}"#),
            r#"<script src="/app.js">"#
        );
        assert_eq!(
            render(r#"{{ favicon(path="/favicon.ico") }}"#),
            r#"<link rel="icon" href="/favicon.ico">"#
        );
        assert_eq!(
            render(r#"{{ feed(path="/rss.xml") }}"#),
            r#"<link rel="alternate" type="application/rss+xml" href="/rss.xml">"#
        );
        assert_eq!(
            render(r#"{{ meta(path="generator") }}"#),
            r#"<meta name="generator" content="/generator">"#
        );
        assert_eq!(
            render(r#"{{ link(path="/about") }}"#),
            r#"<a href="/about">"#
        );
        assert_eq!(
            render(r#"{{ link(path="/about", text="About") }}"#),
            r#"<a href="/about">About</a>"#
        );
    }

    #[test]
    fn tag_helpers_keep_absolute_and_mailto_urls() {
        assert_eq!(
            render(r#"{{ css(path="https://cdn.example.com/a.css") }}"#),
            r#"<link rel="stylesheet" href="https://cdn.example.com/a.css">"#
        );
        assert_eq!(
            render(r#"{{ mail(path="mailto:hi@example.com") }}"#),
            r#"<a href="mailto:hi@example.com">"#
        );
    }

    #[test]
    fn tag_helpers_render_arrays_as_multiple_tags() {
        assert_eq!(
            render(r#"{{ css(path=["/a.css", "/b.css"]) }}"#),
            r#"<link rel="stylesheet" href="/a.css">
<link rel="stylesheet" href="/b.css">"#
        );
    }

    #[test]
    fn tag_helpers_escape_attribute_values() {
        // quotes and angle brackets in the path must not break out of the attribute
        let html = render(r#"{{ link(path='/x" onmouseover="alert(1)') }}"#);
        assert!(html.contains(r#"href="/x&quot; onmouseover=&quot;alert(1)""#));
    }

    #[test]
    fn paginator_helper_single_page() {
        assert_eq!(
            render("{{ paginator(current=1, total=1) }}"),
            r#"<nav class="pagination" aria-label="Pagination"><ol class="pagination-list"><li><span class="pagination-current" aria-current="page">1</span></li></ol></nav>"#
        );
    }

    #[test]
    fn paginator_helper_middle_page_window() {
        assert_eq!(
            render("{{ paginator(current=2, total=5, window=1) }}"),
            r#"<nav class="pagination" aria-label="Pagination"><a class="pagination-prev" href="?page=1">Prev</a><ol class="pagination-list"><li><a class="pagination-link" href="?page=1">1</a></li><li><span class="pagination-current" aria-current="page">2</span></li><li><a class="pagination-link" href="?page=3">3</a></li></ol><a class="pagination-next" href="?page=3">Next</a></nav>"#
        );
    }

    #[test]
    fn paginator_helper_clamps_to_bounds() {
        let html = render("{{ paginator(current=0, total=3) }}");
        assert!(!html.contains("pagination-prev"));
        assert!(html.contains("pagination-current\" aria-current=\"page\">1</span>"));
        let last = render("{{ paginator(current=3, total=3) }}");
        assert!(!last.contains("pagination-next"));
        assert!(last.contains("href=\"?page=2\">Prev"));
    }

    #[test]
    fn paginator_helper_custom_text_and_base() {
        let html = render(
            r#"{{ paginator(current=2, total=2, base="/list?p=", prev_text="«", next_text="»") }}"#,
        );
        assert!(html.contains(r#"href="/list?p=1">«"#));
        assert!(!html.contains("pagination-next"));
    }

    #[test]
    fn toc_helper_extracts_headings() {
        assert_eq!(
            render(r#"{{ toc(content='# One\n\n## Two\n\n### Three') }}"#),
            r##"<nav class="toc" aria-label="Table of contents"><ul><li class="toc-level-1"><a href="#one">One</a></li><li class="toc-level-2"><a href="#two">Two</a></li><li class="toc-level-3"><a href="#three">Three</a></li></ul></nav>"##
        );
    }

    #[test]
    fn toc_helper_respects_max_level() {
        assert_eq!(
            render(r#"{{ toc(content='# One\n\n## Two', max_level=1) }}"#),
            r##"<nav class="toc" aria-label="Table of contents"><ul><li class="toc-level-1"><a href="#one">One</a></li></ul></nav>"##
        );
    }

    #[test]
    fn toc_helper_slugifies_and_strips_markup() {
        let html = render(r#"{{ toc(content='## Hello, World!\n\n### Install `tless`') }}"#);
        assert!(html.contains(r##"href="#hello-world">Hello, World!"##));
        assert!(html.contains(r##"href="#install-tless">Install tless"##));
    }

    #[test]
    fn toc_helper_returns_empty_without_headings() {
        assert_eq!(render(r#"{{ toc(content="plain text only") }}"#), "");
        // rendered HTML contains no markdown headings either
        assert_eq!(render(r#"{{ toc(content='<h2>Already HTML</h2>') }}"#), "");
    }

    #[test]
    fn toc_helper_escapes_heading_text() {
        let html = render(r#"{{ toc(content='## A & B') }}"#);
        assert!(html.contains("A &amp; B"));
    }

    #[test]
    fn parse_datetime_accepts_rfc3339_and_cli_format() {
        // both formats describe 2026-09-01T12:00:00Z
        let expected = 1788264000;
        assert_eq!(
            parse_datetime("2026-09-01T12:00:00Z").unwrap().timestamp(),
            expected
        );
        assert_eq!(
            parse_datetime("2026-09-01 12:00:00").unwrap().timestamp(),
            expected
        );
        assert!(parse_datetime("garbage").is_none());
    }

    #[test]
    fn join_url_avoids_duplicate_slashes() {
        assert_eq!(join_url("https://x.com/", "/a"), "https://x.com/a");
        assert_eq!(join_url("https://x.com", "/a"), "https://x.com/a");
        assert_eq!(join_url("https://x.com", "a"), "https://x.com/a");
        assert!(is_absolute_url("https://x.com"));
        assert!(is_absolute_url("http://x.com"));
        assert!(!is_absolute_url("/a"));
        assert!(!is_absolute_url("a"));
    }

    #[test]
    fn slugify_normalizes_into_ascii_slugs() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
        assert_eq!(slugify("a---b"), "a-b");
        assert_eq!(slugify("-x-"), "x");
        assert_eq!(slugify("café au lait"), "caf-au-lait");
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn format_number_with_separator_groups_integer_part() {
        assert_eq!(format_number_with_separator("1234567", ","), "1,234,567");
        assert_eq!(format_number_with_separator("1234567", "."), "1.234.567");
        assert_eq!(format_number_with_separator("-1234.5", ","), "-1,234.5");
        assert_eq!(format_number_with_separator("123.456", ","), "123.456");
        assert_eq!(format_number_with_separator("", ","), "0");
        assert_eq!(format_number_with_separator("42", ","), "42");
    }

    #[test]
    fn escape_helpers_encode_html_specials() {
        assert_eq!(escape_html_attr("a&b\"c<d>e"), "a&amp;b&quot;c&lt;d&gt;e");
        assert_eq!(escape_html_text("a&b<c>"), "a&amp;b&lt;c&gt;");
    }

    #[test]
    fn build_tag_map_values_are_escaped_and_prefixed() {
        let mut attrs = BTreeMap::new();
        attrs.insert("href".to_string(), Value::from("x\"y"));
        attrs.insert("title".to_string(), Value::from("hi"));
        let val = Value::from(attrs);
        let html = build_tag("a", "href", &[], &val, None).unwrap();
        // values are stored in a hash map, so check per attribute
        assert!(html.starts_with("<a "));
        assert!(html.contains(r#"href="/x&quot;y""#));
        assert!(html.contains(r#"title="hi""#));
        assert!(html.ends_with('>'));
    }

    #[test]
    fn build_tag_rejects_non_string_or_map_values() {
        assert!(build_tag("a", "href", &[], &Value::from(42), None).is_err());
    }

    #[test]
    fn dynamic_to_value_converts_primitives() {
        assert_eq!(dynamic_to_value(Dynamic::from_int(42)), Value::from(42));
        assert_eq!(dynamic_to_value(Dynamic::from_float(1.5)), Value::from(1.5));
        assert_eq!(
            dynamic_to_value(Dynamic::from_bool(true)),
            Value::from(true)
        );
        assert_eq!(
            dynamic_to_value(Dynamic::from("hello".to_string())),
            Value::normal_string("hello")
        );
        assert_eq!(dynamic_to_value(Dynamic::UNIT), Value::none());
    }

    #[test]
    fn dynamic_to_value_converts_arrays_and_maps() {
        let arr = Dynamic::from_array(vec![
            Dynamic::from_int(1),
            Dynamic::from_float(2.5),
            Dynamic::from_bool(false),
        ]);
        let value = dynamic_to_value(arr);
        assert_eq!(value.as_array().unwrap().len(), 3);
        assert_eq!(value.as_array().unwrap()[0], Value::from(1));

        let mut map = RhaiMap::new();
        map.insert("count".into(), Dynamic::from_int(3));
        let value = dynamic_to_value(Dynamic::from_map(map));
        assert_eq!(value.as_map().unwrap().len(), 1);
        assert_eq!(
            value.as_map().unwrap().get(&tera::value::Key::Str("count")),
            Some(&Value::from(3))
        );
    }
}
