# tles

> A fast, easy blog site builder — write Markdown, preview live, deploy to GitHub Pages.

[![Rust](https://img.shields.io/badge/rust-2024_edition-orange)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/)
[![License](https://img.shields.io/badge/license-MIT-blue)](./LICENSE)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/iskyrainy/tless/badge)](https://scorecard.dev/viewer/?uri=github.com/iskyrainy/tless)

`tles` turns a folder of Markdown files into a complete static blog. It ships a development
server with hot reload, a minimal theme with dark mode, and a scaffolded GitHub Actions
workflow, so the whole flow is `write → preview → push`.

## Functions

- **Tera templates** — [tera 2](https://github.com/Keats/tera) layouts with built-in helpers for
  TOC, pagination, Open Graph, tag/category lists and more.
- **Themes** — Design your theme.
- **Taxonomies** — tag and category pages (plus index pages) are generated automatically.
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
  robots.txt
theme/base/
  layout/                      # base.html, index.html, post.html, page.html, tag*.html, category*.html
  assets/                      # style.css, highlight.css, highlight.js
```

Names are slugified (`First Post` → `first-post.md`), and every source file is rendered to a
pretty URL: `source/post/hello.md` → `/post/hello/`.

## Configuration

```toml
[site]
title = "My Tless Site"
subtitle = "A clean, minimal theme with automatic dark mode."
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
```

`tless.toml` is read at startup and on every rebuild — restart the dev server after editing it.

## Templates

Layouts live in `theme/<theme>/layout/` and are plain Tera templates. The included base theme
extends `base.html` from each page:

| Layout | Used for |
| --- | --- |
| `base.html` | Shared shell: head, nav, footer, dark-mode toggle. |
| `index.html` | Home page (recent posts). |
| `post.html` | Single post; the default for files in `source/post/`. |
| `page.html` | Single page; the default for files in `source/page/`. |
| `tag.html` / `category.html` | One page per term with that term's posts. |
| `tag-index.html` / `category-index.html` | Overview of all terms. |

A source file can override its layout through frontmatter (`layout: post.html`). Templates
receive `title`, `date`, `content` (HTML), `markdown` (raw body), `tag`, `category` and `site`
(the full site model); taxonomy layouts additionally get `name` and `posts`.

Built-in helper functions:

| Group | Functions |
| --- | --- |
| Text & URLs | `date(ts, fmt)`, `url_for(path, relative)`, `full_url_for(path)`, `gravatar(mail)`, `number_format(value, separator)` |
| HTML tags | `css`, `js`, `link`, `image`, `mail`, `favicon`, `feed`, `meta` — each takes a `path` string or an attribute map |
| Layout | `partial(name)`, `paginator(current, total, window, base, prev_text, next_text)`, `open_graph(title, description, image, url, type)`, `toc(content, max_level)` |
| Taxonomies | `list_category`, `list_tag`, `list_post`, `list_page` with `orderby`, `order`, `amount`, `list`, `separator`, `show_count` |

```html
<article class="post-content">{{ content | safe }}</article>
<details class="toc" open>{{ toc(content=markdown, max_level=3) | safe }}</details>
{{ list_tag(orderby="count", order=-1, amount=10) | safe }}
```

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

The crate is split into a CLI (`src/cmd.rs`), file operations (`src/file/`), the dev server and
rendering pipeline (`src/server/`), and the embedded base theme (`src/server/template/`), so the
scaffold and the shipped theme are always in sync.
