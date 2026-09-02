use arc_swap::ArcSwap;
use chrono::{DateTime, NaiveDateTime, Utc};
use pulldown_cmark::{Event, HeadingLevel, Parser as MarkdownParser, Tag, TagEnd};
use rhai::{AST, Dynamic, Engine, Map as RhaiMap};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fmt::Write,
    fs,
    path::Path,
    sync::{Arc, LazyLock},
};
use tera::{Context, Function, Kwargs, Map, Number, State, Tera, TeraResult, Value};
use tracing::info;

use crate::server::{CONFIG, SITE, TERA};

trait CloneableFunction: Function<TeraResult<Value>> + Send + Sync {
    fn clone_box(&self) -> Box<dyn CloneableFunction>;
}

impl<T> CloneableFunction for T
where
    T: 'static + Function<TeraResult<Value>> + Send + Sync + Clone,
{
    fn clone_box(&self) -> Box<dyn CloneableFunction> {
        Box::new(self.clone())
    }
}

type HelperFunc = Box<dyn CloneableFunction>;

impl Clone for HelperFunc {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

#[derive(Default, Clone)]
pub struct Helpers {
    funcs: HashMap<String, HelperFunc>,
}

impl Helpers {
    pub fn new() -> Self {
        let mut helper = Self {
            funcs: HashMap::new(),
        };
        helper.register("date", DateHelper);
        helper.register("url_for", UrlHelper);
        helper.register("full_url_for", FullUrlHelper);
        helper.register("gravatar", GravatarHelper);
        helper.register("css", CssHelper);
        helper.register("js", JsHelper);
        helper.register("link", LinkHelper);
        helper.register("mail", MailHelper);
        helper.register("image", ImageHelper);
        helper.register("favicon", FaviconHelper);
        helper.register("feed", FeedHelper);
        helper.register("meta", MetaHelper);
        helper.register("partial", PartialHelper);
        helper.register("list_categories", CategoriesHelper);
        helper.register("list_tags", TagsHelper);
        helper.register("list_posts", PostsHelper);
        helper.register("list_pages", PagesHelper);
        helper.register("paginator", PaginatorHelper);
        helper.register("number_format", NumberFormatHelper);
        helper.register("open_graph", OpenGraphHelper);
        helper.register("toc", TocHelper);
        helper
    }

    pub fn register<F>(&mut self, name: &str, f: F)
    where
        F: Function<TeraResult<Value>> + Send + Sync + Clone + 'static,
    {
        self.funcs.insert(name.to_string(), Box::new(f));
    }

    pub fn apply_to(&self, tera: &mut Tera) {
        for (name, func) in &self.funcs {
            let f_clone = func.clone();
            tera.register_function(name, move |args, state: &State| -> TeraResult<Value> {
                f_clone.call(args, state)
            });
        }
    }
}

pub(crate) static HELPER: LazyLock<ArcSwap<Helpers>> = LazyLock::new(|| {
    let helpers = Helpers::new();
    ArcSwap::from_pointee(helpers)
});

/// Fallback amount for list helpers when no `amount` arg is given (effectively unlimited).
const DEFAULT_AMOUNT: i64 = 1 << 16;

/// Rhai engine safety limits for untrusted helper scripts.
const MAX_OPERATIONS: u64 = 1_000_000;
const MAX_EXPR_DEPTHS: (usize, usize) = (32, 64);
const MAX_CALL_LEVELS: usize = 64;

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

macro_rules! match_value_or_default {
    ($name:expr, $pat:pat => $val:ident) => {
        match $name {
            Some(arg) => match arg {
                $pat => $val,
                _ => {
                    return Err(tera::Error::msg(format!(
                        "Param type error: expect pattern `{}` but got value `{:?}`",
                        stringify!($pat),
                        arg
                    )))
                }
            },
            None => {
                return Err(tera::Error::msg(format!(
                    "Param not found: `{}`",
                    stringify!($name)
                )))
            }
        }
    };
    ($name:expr, $pat:pat => $val:ident, $default_value:expr) => {
        match $name {
            Some($pat) => $val,
            _ => $default_value,
        }
    };
    ($name:expr, $pat:pat => $val:ident, $default_value:expr, $and_then:expr) => {
        match $name {
            Some($pat) => $and_then($val),
            _ => $default_value,
        }
    };
}

#[derive(Clone)]
struct DateHelper;

impl Function<TeraResult<Value>> for DateHelper {
    fn call(&self, args: &HashMap<String, Value>) -> TeraResult<Value> {
        let ts = match args.get("ts") {
            Some(Value::Number(n)) => n.as_i64().unwrap_or_else(|| Utc::now().timestamp()),
            Some(Value::String(s)) => parse_datetime(s).unwrap_or_else(Utc::now).timestamp(),
            Some(_) => {
                return Err(tera::Error::msg(
                    "Invalid 'ts': expected an epoch number or a date string",
                ));
            }
            None => Utc::now().timestamp(),
        };

        let fmt = match_value_or_default!(args.get("fmt"), Value::String(v) => v, &String::from("%Y-%m-%d %H:%M:%S"));
        let date = DateTime::from_timestamp(ts, 0).unwrap_or_else(Utc::now);
        Ok(to_value(date.format(fmt).to_string())?)
    }
}

#[derive(Clone)]
struct TagHelper {
    tag: String,
    default_attrs: HashMap<String, String>,
    path_attr: String,
}

impl TagHelper {
    fn new(tag: &str, path_attr: &str, defaults: &[(&str, &str)]) -> Self {
        TagHelper {
            tag: tag.to_string(),
            path_attr: path_attr.to_string(),
            default_attrs: defaults
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }
}

impl Function<TeraResult<Value>> for TagHelper {
    fn call(&self, args: &HashMap<String, Value>) -> TeraResult<Value> {
        fn build_tag(
            tag: &str,
            default_attrs: &HashMap<String, String>,
            path_attr: &str,
            val: &Value,
        ) -> TeraResult<String> {
            let mut html = String::new();
            if let Some(s) = val.as_str() {
                let mut path = s.to_string();
                if !path.starts_with('/')
                    && !path.starts_with("http")
                    && !path.starts_with("mailto:")
                {
                    path = format!("/{}", path);
                }

                html.push_str(&format!("<{}", tag));
                for (k, v) in default_attrs {
                    html.push_str(&format!(r#" {}="{}""#, k, v));
                }
                html.push_str(&format!(r#" {}="{}">"#, path_attr, escape_html_attr(&path)));
                return Ok(html);
            }

            if let Some(map) = val.as_object() {
                html.push_str(&format!("<{}", tag));
                for (k, v) in default_attrs {
                    html.push_str(&format!(r#" {}="{}""#, k, v));
                }
                for (k, v) in map {
                    let v = v
                        .as_str()
                        .ok_or_else(|| tera::Error::msg(format!("Invalid type for key {}", k)))?;
                    let mut val = v.to_string();
                    if k == path_attr
                        && !v.starts_with('/')
                        && !v.starts_with("http")
                        && !v.starts_with("mailto:")
                    {
                        val = format!("/{}", v);
                    }
                    html.push_str(&format!(r#" {}="{}""#, k, escape_html_attr(&val)));
                }
                html.push('>');
                return Ok(html);
            }

            Err(tera::Error::msg("Invalid path type"))
        }

        let path = args
            .get("path")
            .ok_or_else(|| tera::Error::msg("Missing 'path'"))?;
        let html = match path {
            Value::Array(arr) => {
                let mut out = Vec::new();
                for v in arr {
                    out.push(build_tag(
                        &self.tag,
                        &self.default_attrs,
                        &self.path_attr,
                        v,
                    )?);
                }
                out.join("\n")
            }
            _ => build_tag(&self.tag, &self.default_attrs, &self.path_attr, path)?,
        };
        Ok(Value::String(html))
    }
}

macro_rules! define_tag_helper {
    ($name:ident, $tag:expr, $path_attr:expr, { $($k:expr => $v:expr),* }) => {
        #[derive(Clone)]
        struct $name;
        impl Function<TeraResult<Value>> for $name {
            fn call(&self, args: &HashMap<String, Value>) -> TeraResult<Value> {
                let helper = TagHelper::new($tag, $path_attr, &[ $(($k, $v)),* ]);
                helper.call(args)
            }
        }
    };
}

define_tag_helper!(CssHelper, "link", "href", { "rel" => "stylesheet" });
define_tag_helper!(JsHelper, "script", "src", {});
define_tag_helper!(LinkHelper, "a", "href", {});
define_tag_helper!(ImageHelper, "img", "src", {});
define_tag_helper!(MailHelper, "a", "href", {});
define_tag_helper!(FaviconHelper, "link", "href", { "rel" => "icon" });
define_tag_helper!(FeedHelper, "link", "href", { "rel" => "alternate", "type" => "application/rss+xml" });
define_tag_helper!(MetaHelper, "meta", "content", { "name" => "generator" });

macro_rules! define_url_helper {
    ($name:ident, url) => {
        #[derive(Clone)]
        struct $name;
        impl Function<TeraResult<Value>> for $name {
            fn call(&self, args: &HashMap<String, Value>) -> TeraResult<Value> {
                let site_url = &CONFIG.load().site.url;
                let path = match_value_or_default!(args.get("path"), Value::String(v) => v);
                let relative =
                    match_value_or_default!(args.get("relative"), Value::Bool(v) => v, &true);
                let res = if *relative {
                    if path.starts_with('/') {
                        format!(".{}", path)
                    } else {
                        format!("./{}", path)
                    }
                } else {
                    join_url(&super::extract_root_path(site_url), path)
                };
                Ok(Value::String(res))
            }
        }
    };

    ($name:ident, fullurl) => {
        #[derive(Clone)]
        struct $name;
        impl Function<TeraResult<Value>> for $name {
            fn call(&self, args: &HashMap<String, Value>) -> TeraResult<Value> {
                let site_url = &CONFIG.load().site.url;
                let path = match_value_or_default!(args.get("path"), Value::String(v) => v);
                Ok(Value::String(join_url(site_url, path)))
            }
        }
    };
}

define_url_helper!(UrlHelper, url);
define_url_helper!(FullUrlHelper, fullurl);

#[derive(Clone)]
struct GravatarHelper;

impl Function<TeraResult<Value>> for GravatarHelper {
    fn call(&self, args: &HashMap<String, Value>) -> TeraResult<Value> {
        let mail = match_value_or_default!(args.get("mail"), Value::String(v) => v);
        let mut hashed_email = Sha256::new();
        hashed_email.update(mail.trim());
        let hash: [u8; 32] = hashed_email.finalize().into();
        let url = format!(
            "https://www.gravatar.com/avatar/{}",
            hash.iter()
                .map(|b| format!("{:02X}", b))
                .collect::<String>()
        );
        Ok(Value::String(url))
    }
}

#[derive(Clone)]
struct PartialHelper;

impl Function<TeraResult<Value>> for PartialHelper {
    fn call(&self, args: &HashMap<String, Value>) -> TeraResult<Value> {
        let part_name =
            match_value_or_default!(args.get("name"), Value::String(v) => v, &String::from(""));
        if part_name.is_empty() {
            return Ok(Value::Null);
        }
        let tera = &TERA.load();
        let context = tera::Context::new();
        let render_str = tera.render(format!("{}.html", part_name).as_str(), &context)?;
        Ok(Value::String(render_str))
    }
}

#[derive(Clone)]
struct ListHelper {
    list: String,
}

impl Function<TeraResult<Value>> for ListHelper {
    fn call(&self, args: &HashMap<String, Value>) -> TeraResult<Value> {
        let orderby = match_value_or_default!(args.get("orderby"), Value::String(v) => v, &String::from("name"));
        let order = match_value_or_default!(args.get("order"), Value::Number(v) => v, 1, |v: &Number| v.as_i64().unwrap_or(1));
        let show_count =
            match_value_or_default!(args.get("show_count"), Value::Bool(v) => v, &true);
        let list = match_value_or_default!(args.get("list"), Value::Bool(v) => v, &true);
        let separator = match_value_or_default!(args.get("separator"), Value::String(v) => v, &String::from(","));
        let amount = match_value_or_default!(
            args.get("amount"),
            Value::Number(v) => v,
            DEFAULT_AMOUNT,
            |v: &Number| v.as_i64().unwrap_or(DEFAULT_AMOUNT)
        );
        let tag_class =
            match_value_or_default!(args.get("tag_class"), Value::Object(v) => v, &Map::new());
        let tag_class_ul = match_value_or_default!(tag_class.get("ul"), Value::String(v) => v, &String::from("ul"));
        let tag_class_li = match_value_or_default!(tag_class.get("li"), Value::String(v) => v, &String::from("li"));
        let tag_class_a =
            match_value_or_default!(tag_class.get("a"), Value::String(v) => v, &String::from("a"));
        let tag_class_count = match_value_or_default!(tag_class.get("count"), Value::String(v) => v, &String::from("count"));

        let mut res = String::new();
        let site = &SITE.load();

        let render_list = |res: &mut String, iter: Vec<(String, String, usize)>| {
            res.push_str(&format!(
                r#"<ul class="{}" itemprop="keywords">"#,
                tag_class_ul
            ));
            for (i, (name, href, count)) in iter.into_iter().enumerate() {
                if i >= amount as usize {
                    break;
                }
                res.push_str(&format!(r#"<li class="{}">"#, tag_class_li));
                res.push_str(&format!(
                    r#"<a class="{}" href="{}">{}</a>"#,
                    tag_class_a, href, name
                ));
                if *show_count && count > 0 {
                    res.push_str(&format!(
                        r#"<span class="{}">{}</span>"#,
                        tag_class_count, count
                    ));
                }
                res.push_str("</li>");
            }
            res.push_str("</ul>");
        };

        let render_inline = |res: &mut String, iter: Vec<(String, String, usize)>| {
            for (i, (name, href, count)) in iter.into_iter().enumerate() {
                if i >= amount as usize {
                    break;
                }
                res.push_str(&format!(
                    r#"<a class="{}" href="{}">{}"#,
                    tag_class_a, href, name
                ));
                if *show_count && count > 0 {
                    res.push_str(&format!(
                        r#"<span class="{}">{}</span>"#,
                        tag_class_count, count
                    ));
                }
                res.push_str(&format!("</a>{}", separator));
            }
        };

        match self.list.as_str() {
            "category" | "tag" => {
                let data = if self.list == "category" {
                    site.categories.iter()
                } else {
                    site.tags.iter()
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

                if *list {
                    render_list(&mut res, tmp);
                } else {
                    render_inline(&mut res, tmp);
                }
            }
            "post" | "page" => {
                let mut tmp = if self.list == "post" {
                    site.posts.clone()
                } else {
                    site.pages.clone()
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

                if *list {
                    render_list(&mut res, mapped);
                } else {
                    render_inline(&mut res, mapped);
                }
            }
            _ => {}
        }
        Ok(Value::String(res))
    }
}

fn make_list_helper(list: &str) -> ListHelper {
    ListHelper {
        list: list.to_string(),
    }
}

macro_rules! define_list_helper {
    ($name:ident, $list:expr) => {
        #[derive(Clone)]
        struct $name;
        impl Function<TeraResult<Value>> for $name {
            fn call(&self, args: &HashMap<String, Value>) -> TeraResult<Value> {
                let helper = make_list_helper($list);
                helper.call(args)
            }
        }
    };
}

define_list_helper!(CategoriesHelper, "category");
define_list_helper!(TagsHelper, "tag");
define_list_helper!(PostsHelper, "post");
define_list_helper!(PagesHelper, "page");

#[derive(Clone)]
struct PaginatorHelper;

impl Function<TeraResult<Value>> for PaginatorHelper {
    fn call(&self, args: &HashMap<String, Value>) -> TeraResult<Value> {
        let current = match_value_or_default!(
            args.get("current"),
            Value::Number(v) => v,
            1,
            |v: &Number| v.as_i64().unwrap_or(1).max(1)
        );
        let total = match_value_or_default!(
            args.get("total"),
            Value::Number(v) => v,
            1,
            |v: &Number| v.as_i64().unwrap_or(1).max(1)
        );
        let window = match_value_or_default!(
            args.get("window"),
            Value::Number(v) => v,
            2,
            |v: &Number| v.as_i64().unwrap_or(2).max(0)
        );
        let base = match_value_or_default!(
            args.get("base"),
            Value::String(v) => v,
            &String::from("?page=")
        );
        let prev_text = match_value_or_default!(
            args.get("prev_text"),
            Value::String(v) => v,
            &String::from("Prev")
        );
        let next_text = match_value_or_default!(
            args.get("next_text"),
            Value::String(v) => v,
            &String::from("Next")
        );

        let current = current.min(total);
        let start = (current - window).max(1);
        let end = (current + window).min(total);
        let mut html = String::from(r#"<nav class="pagination" aria-label="Pagination">"#);

        if current > 1 {
            let _ = write!(
                html,
                r#"<a class="pagination-prev" href="{}{}">{}</a>"#,
                base,
                current - 1,
                prev_text
            );
        }

        html.push_str(r#"<ol class="pagination-list">"#);
        for page in start..=end {
            if page == current {
                let _ = write!(
                    html,
                    r#"<li><span class="pagination-current" aria-current="page">{}</span></li>"#,
                    page
                );
            } else {
                let _ = write!(
                    html,
                    r#"<li><a class="pagination-link" href="{}{}">{}</a></li>"#,
                    base, page, page
                );
            }
        }
        html.push_str("</ol>");

        if current < total {
            let _ = write!(
                html,
                r#"<a class="pagination-next" href="{}{}">{}</a>"#,
                base,
                current + 1,
                next_text
            );
        }

        html.push_str("</nav>");
        Ok(Value::String(html))
    }
}

#[derive(Clone)]
struct NumberFormatHelper;

impl Function<TeraResult<Value>> for NumberFormatHelper {
    fn call(&self, args: &HashMap<String, Value>) -> TeraResult<Value> {
        let value = match args.get("value") {
            Some(Value::Number(n)) => n
                .as_i64()
                .map(|v| v.to_string())
                .or_else(|| n.as_u64().map(|v| v.to_string()))
                .or_else(|| n.as_f64().map(|v| v.to_string()))
                .unwrap_or_else(|| "0".to_string()),
            Some(Value::String(s)) => s.clone(),
            _ => return Err(tera::Error::msg("Missing 'value'")),
        };
        let separator = match_value_or_default!(
            args.get("separator"),
            Value::String(v) => v,
            &String::from(",")
        );

        let formatted = format_number_with_separator(&value, separator);
        Ok(Value::String(formatted))
    }
}

#[derive(Clone)]
struct OpenGraphHelper;

impl Function<TeraResult<Value>> for OpenGraphHelper {
    fn call(&self, args: &HashMap<String, Value>) -> TeraResult<Value> {
        let config = CONFIG.load();
        let title = match_value_or_default!(
            args.get("title"),
            Value::String(v) => v,
            &config.site.title
        );
        let description = match_value_or_default!(
            args.get("description"),
            Value::String(v) => v,
            &config.site.description
        );
        let url = match args.get("url") {
            Some(Value::String(path)) if is_absolute_url(path) => path.clone(),
            Some(Value::String(path)) => join_url(&config.site.url, path),
            _ => config.site.url.clone(),
        };
        let image = match args.get("image") {
            Some(Value::String(path)) if is_absolute_url(path) => path.clone(),
            Some(Value::String(path)) => join_url(&config.site.url, path),
            _ => String::new(),
        };
        let kind = match_value_or_default!(
            args.get("type"),
            Value::String(v) => v,
            &String::from("website")
        );

        let mut tags = vec![
            ("og:title", title.to_string()),
            ("og:description", description.to_string()),
            ("og:type", kind.to_string()),
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
                r#"<meta property="{}" content="{}">"#,
                name,
                escape_html_attr(&content)
            );
        }
        Ok(Value::String(html))
    }
}

#[derive(Clone)]
struct TocHelper;

impl Function<TeraResult<Value>> for TocHelper {
    fn call(&self, args: &HashMap<String, Value>) -> TeraResult<Value> {
        let content = match_value_or_default!(args.get("content"), Value::String(v) => v);
        let max_level = match_value_or_default!(
            args.get("max_level"),
            Value::Number(v) => v,
            6,
            |v: &Number| v.as_u64().unwrap_or(6) as usize
        );

        let mut items = Vec::new();
        let mut current_level = None;
        let mut current_text = String::new();

        for event in MarkdownParser::new(content) {
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
            return Ok(Value::String(String::new()));
        }

        let mut html = String::from(r#"<nav class="toc" aria-label="Table of contents"><ul>"#);
        for (level, text, slug) in items {
            let _ = write!(
                html,
                r##"<li class="toc-level-{}"><a href="#{}">{}</a></li>"##,
                level,
                slug,
                escape_html_text(&text)
            );
        }
        html.push_str("</ul></nav>");
        Ok(Value::String(html))
    }
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
        Some(frac) if !frac.is_empty() => format!("{}{}.{}", sign, int_formatted, frac),
        _ => format!("{}{}", sign, int_formatted),
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

#[derive(Clone)]
struct RhaiHelper {
    engine: Arc<Engine>,
    ast: Arc<AST>,
    fn_name: String,
}

impl RhaiHelper {
    fn new(engine: Arc<Engine>, ast: Arc<AST>, fn_name: impl Into<String>) -> Self {
        Self {
            engine,
            ast,
            fn_name: fn_name.into(),
        }
    }
}

fn value_to_dynamic(v: &Value) -> Dynamic {
    match v {
        Value::Null => Dynamic::UNIT,
        Value::Bool(b) => Dynamic::from_bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Dynamic::from_int(i)
            } else if let Some(f) = n.as_f64() {
                Dynamic::from_float(f)
            } else {
                Dynamic::UNIT
            }
        }
        Value::String(s) => Dynamic::from(s.clone()),
        Value::Array(a) => {
            let mut arr = Vec::with_capacity(a.len());
            for e in a {
                arr.push(value_to_dynamic(e));
            }
            Dynamic::from_array(arr)
        }
        Value::Object(o) => {
            let mut map = RhaiMap::new();
            for (k, v) in o {
                map.insert(k.into(), value_to_dynamic(v));
            }
            Dynamic::from_map(map)
        }
    }
}

impl Function<TeraResult<Value>> for RhaiHelper {
    fn call(&self, args: &HashMap<String, Value>) -> TeraResult<Value> {
        let mut scope = rhai::Scope::new();
        let mut arg_map = RhaiMap::new();
        for (k, v) in args {
            arg_map.insert(k.clone().into(), value_to_dynamic(v));
        }

        // Only accept a map named args
        scope.push("args", Dynamic::from_map(arg_map));
        let res = self
            .engine
            .call_fn::<Dynamic>(&mut scope, &self.ast, &self.fn_name, ())
            .map_err(|e| tera::Error::msg(format!("Rhai call error: {}", e)))?;
        let out = if res.is::<String>() {
            Value::String(res.cast::<String>())
        } else if res.is::<i64>() {
            let i = res.cast::<i64>();
            Value::Number(serde_json::Number::from(i))
        } else if res.is::<f64>() {
            let f = res.cast::<f64>();
            Value::Number(
                serde_json::Number::from_f64(f).unwrap_or_else(|| serde_json::Number::from(0)),
            )
        } else if res.is::<bool>() {
            Value::Bool(res.cast::<bool>())
        } else if res.is::<()>() {
            Value::Null
        } else if res.is::<rhai::ImmutableString>() {
            // also string-like
            Value::String(res.to_string())
        } else if res.is_array() || res.is_map() {
            // Serialize via JSON string as fallback
            match rhai::serde::to_dynamic(&res).and_then(|d| {
                serde_json::to_value(d).map_err(|e| {
                    Box::new(rhai::EvalAltResult::ErrorRuntime(
                        format!("{}", e).into(),
                        rhai::Position::NONE,
                    ))
                })
            }) {
                Ok(v) => v,
                Err(_) => Value::String(res.to_string()),
            }
        } else {
            Value::String(res.to_string())
        };
        Ok(out)
    }
}

pub(crate) fn load_rhai_helpers(helpers_dir: impl AsRef<Path>) -> TeraResult<()> {
    let dir = helpers_dir.as_ref();
    if !dir.exists() {
        return Ok(());
    }
    let mut engine = Engine::new();

    engine.set_max_operations(MAX_OPERATIONS);
    engine.set_max_expr_depths(MAX_EXPR_DEPTHS.0, MAX_EXPR_DEPTHS.1);
    engine.set_max_call_levels(MAX_CALL_LEVELS);
    engine.set_max_variables(1);
    engine.set_allow_looping(false);
    engine.set_optimization_level(rhai::OptimizationLevel::Simple);
    engine.disable_symbol("eval");
    engine.disable_symbol("import");
    engine.disable_symbol("use");

    let engine = Arc::new(engine);

    let mut to_add = vec![];
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("rhai") {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap()
                .to_string();
            let script = fs::read_to_string(&path)?;
            // precompile AST
            let ast = engine
                .compile(&script)
                .map_err(|e| tera::Error::msg(format!("Compile error in {}: {}", name, e)))?;
            let ast = Arc::new(ast);

            // custom helper def: fn call(args) -> string/primitive
            let helper = RhaiHelper::new(engine.clone(), ast.clone(), "call");
            to_add.push((name.clone(), helper));
            info!("Registered Rhai helper: {}", name);
        }
    }
    let helpers = HELPER.load();
    let mut h_clone = (*helpers.clone()).clone();
    for (name, helper) in to_add {
        h_clone.register(&name, helper);
    }
    HELPER.store(Arc::new(h_clone));
    Ok(())
}
