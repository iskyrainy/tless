# tles

> A fast, easy blog site builder — write Markdown, preview live, deploy to GitHub Pages.

[![Rust](https://img.shields.io/badge/rust-2024_edition-orange)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/)
[![License](https://img.shields.io/badge/license-MIT-blue)](./LICENSE)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/iskyrainy/tless/badge)](https://scorecard.dev/viewer/?uri=github.com/iskyrainy/tless)

`tles` turns a folder of Markdown files into a complete static blog. It ships a development
server with hot reload, a polished theme with dark mode and LLM translations, and a scaffolded
GitHub Actions workflow.

## Functions

- **Tera templates** — [tera 2](https://github.com/Keats/tera) layouts with built-in helpers for
  TOC, pagination, Open Graph, tag/category lists and more.
- **Themes** — Design your theme.
- **Taxonomies** — tag and category pages (plus index pages) are generated automatically.
- **Translations** — `tles site -t` translates every post with an LLM into the languages listed
  in `[i18n] target_lang`, and the theme gets a language switcher for free.
- **Feeds & SEO** — `atom.xml`, `sitemap.xml` and `robots.txt` are built in.
- **Rhai helper scripts** — extend your templates with sandboxed scripts dropped into `helper/`.
- **Deploy-ready scaffold** — `tles site -i` writes the config, theme, `.gitignore` and the
  GitHub Pages workflow for you.

## Quick start

Requires [Rust](https://rustup.rs) 1.85+ (edition 2024).

```bash
# install
cargo install --git https://github.com/iskyrainy/tless

# scaffold a new site
mkdir my-blog && cd my-blog
tles site -i

# write your first post
tles blog add hello
tles blog publish hello

# preview at http://127.0.0.1:8917
tles server -r

# build the static site into public/
tles site -g
```

## Generated site

`tles site -i` produces a self-contained, deploy-ready repository:

```text
.github/workflows/deploy.yml   # GitHub Pages deployment
.gitignore
tless.toml                     # site configuration
helper/                        # Rhai helper scripts
plugin/
public/                        # build output (gitignored)
source/
  draft/                       # work-in-progress posts (never built)
  post/                        # published posts
  page/                        # standalone pages
  i18n/<lang>/                 # translations of posts, written by `tles site -t`
  robots.txt
theme/base/
  layout/                      # base.html, index.html, post.html, page.html, tag*.html, category*.html
  assets/                      # style.css, highlight.css, highlight.js
```

Names are slugified (`First Post` -> `first-post.md`), and every source file is rendered to a
pretty URL: `source/post/hello.md` -> `/post/hello/`.

## Configuration

```toml
[site]
title = "My Tless Site"
subtitle = "Notes on what I build, and how."
description = "A fast blog powered by Tless."
rights = "My Tless Site"
author = "Your Name"
url = "http://127.0.0.1:8917"
zone = "Asia/Shanghai"
theme = "base"
favicon = ""
menu = [
    { name = "Home", link = "/index.html" },
    { name = "Tags", link = "/tag" },
    { name = "Categories", link = "/category" }
]

[i18n]                         # optional; see "Translations" below
provider = "deepseek"          # deepseek | kimi | glm
api_key = "..."
model = "deepseek-flash"
target_lang = ["zh-CN", "en"]
```

`tless.toml` is read at startup and on every rebuild — restart the dev server after editing it.

## Templates

Layouts live in `theme/<theme>/layout/` and are plain Tera templates. The included base theme
extends `base.html` from each page:

| Layout | Used for |
| --- | --- |
| `base.html` | Shared shell: head, header (nav, language switcher, RSS, dark-mode toggle), footer, back-to-top. |
| `index.html` | Home feed: the newest post full width, then two per row. |
| `post.html` | Single post; the default for files in `source/post/`. |
| `page.html` | Single page; the default for files in `source/page/`. |
| `tag.html` / `category.html` | One page per term with that term's posts. |
| `tag-index.html` / `category-index.html` | Overview of all terms. |

A source file can override its layout through frontmatter (`layout: post.html`).

Every layout gets `site`, the whole site model. Templates also receive:

| Variable | On | Value |
| --- | --- | --- |
| `title` | posts, pages, terms | Post/page title; falls back to the source file name. |
| `date` | posts, pages | Frontmatter date, verbatim — feed it to `date(ts=…)`. |
| `content` | posts, pages | Rendered article HTML. |
| `markdown` | posts, pages | Raw Markdown body (what `toc()` wants). |
| `tag` / `category` | posts | Lists of terms, or null. |
| `prev_post` / `next_post` | posts | The post published earlier / later, or null. |
| `translations` | posts | Every published version of this post; see [Translations](#translations). Empty elsewhere. |
| `name` / `posts` | term pages | The term and its posts. |
| `recent_posts` | home | All posts, newest first. |

`prev_post` and `next_post` are `{ title, date, path, … }`; render a link with
`/post/{{ slugify(str=prev_post.path, is_path=true) }}/`.

Built-in helper functions:

| Group | Functions |
| --- | --- |
| Text & URLs | `date(ts, fmt)`, `url_for(path, relative)`, `full_url_for(path)`, `gravatar(mail)`, `number_format(value, separator)`, `slugify(str, is_path)` |
| HTML tags | `css`, `js`, `link`, `image`, `mail`, `favicon`, `feed`, `meta` — each takes a `path` string or an attribute map |
| Layout | `partial(name)`, `paginator(current, total, window, base, prev_text, next_text)`, `open_graph(title, description, image, url, type)`, `toc(content, min_level, max_level)` |
| Taxonomies | `list_category`, `list_tag`, `list_post`, `list_page` with `orderby`, `order`, `amount`, `list`, `separator`, `show_count`, `tag_class` |

```html
<article class="post-content">{{ content | safe }}</article>
{{ toc(content=markdown, min_level=2, max_level=3) | safe }}
{{ list_tag(orderby="count", order=-1, amount=10) | safe }}
{{ css(path=["/assets/style.css", "/assets/highlight.css"]) }}
{{ feed(path="/atom.xml", type="application/atom+xml", title=site.config.title) }}
```

Pass a map instead of a string to set any attribute — keys are written in sorted order, so the
output is reproducible:

```html
{{ link(path={"href": "/tag/" ~ t, "class": "rail-tag"}, text=t) }}
{{ js(path={"src": "/assets/highlight.js", "defer": ""}) }}
```

`slugify(str, is_path=true)` slugifies the file name of a path, which is what post URLs are
built from. `date(ts=…)` accepts an epoch number, RFC 3339 or the `%Y-%m-%d %H:%M:%S` the CLI
writes.

Headings get anchor ids while rendering, so the generated table of contents links jump to the
right spot.

### Rhai helpers

Drop a `.rhai` script into `helper/` and it becomes a template function named after the file.
Each script must define `fn main(args)` (`call` is a reserved keyword in Rhai):

```rhai
// helper/greeting.rhai → {{ greeting(name="World") }}
fn main(args) {
    "Hello, " + args["name"] + "!"
}
```

Scripts run sandboxed: no loops, no `eval`/`import`, with operation, depth and call-level limits.
Adding or editing a script reloads the dev server automatically.

## Translations

`tles site -t` reads every post in `source/post/`, asks the configured LLM to translate it, and
writes the result to `source/i18n/<lang>/<name>.md` — one file per language in
`target_lang`. A post is only re-translated when its Markdown changed, tracked by an md5 of the
source in `source/post/.post_hash.json`.

```toml
[i18n]
provider = "deepseek"          # deepseek | kimi | glm
api_key = "sk-…"
model = "deepseek-flash"
target_lang = ["zh-CN", "en"]  # BCP 47 codes
```

> [!IMPORTANT]
> Supported `target_lang` code info see more in [google language code](https://developers.google.com/workspace/admin/directory/v1/languages?hl=zh-cn).

The build (`tles site -g`) picks the translations up and publishes them next to the original:

```text
source/post/hello.md        →  public/post/hello/index.html
source/i18n/zh-CN/hello.md  →  public/post/zh-CN/hello/index.html
source/i18n/en/hello.md     →  public/post/en/hello/index.html
```

Post pages get a `translations` variable listing every version that exists, which is what the
base theme's language switcher is built from:

```json
[ { "lang": "",      "url": "/post/hello/",      "current": true  },
  { "lang": "zh-CN", "url": "/post/zh-CN/hello/", "current": false } ]
```

`lang` is empty for the original: originals have no fixed language, so nothing labels them but
their own path. `current` marks the page being rendered, so a theme can highlight it.

Code blocks, inline code and frontmatter keys pass through untouched; only `title`,
`description`, `summary` and `excerpt` are translated. Translation prompt see [SYSTEM_PROMPT](src/server/i18n.rs#L56).

## Theme

The bundled `base` theme follows the layout of the [Cloudflare blog](https://blog.cloudflare.com/).

Code blocks use the [Tokyo Night](https://github.com/folke/tokyonight.nvim) palette — Tokyo
Night Day in light mode, Tokyo Night in dark mode. The token colours are `--hl-*` custom
properties at the top of `assets/highlight.css`, so retheming highlighting means editing that
one block; the block background still comes from `--code-bg` in `style.css`.

## Deployment

The scaffolded workflow builds the site and publishes `public/` to GitHub Pages on every push
to `main`. One-time setup in your repository:

> [!IMPORTANT]
> Set **Settings → Pages → Source** to **GitHub Actions**, and update `url` in `tless.toml` to
> your final site URL so feeds, sitemap and absolute links are correct.

The workflow installs `tles` from this repository — point it at your fork if you maintain one.

## Development

```bash
cargo build --release  # build
cargo test             # unit tests
cargo clippy           # lints
cargo fmt              # formatting
```
