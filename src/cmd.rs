use std::env;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use tracing::info;

use crate::{error::AppError, file, server};

/// tless command arguments
#[derive(Parser, Debug)]
#[command(
    author = "gdhvxcj <wangnan5117@gmail.com>",
    version,
    about = "Build blog site.",
    long_about = "Fast and easy blog site builder."
)]
#[command(propagate_version = true)]
struct Command {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Subcommand that run tless server and specify port
    Server(Server),

    /// Subcommand that controls blog's `add/remove/publish`
    Blog(Blog),

    /// Subcommand that controls page `add/remove/publish`
    Page(Page),

    /// Subcommand that generates static pages, deploy to github page, backup site, etc.
    Site(Site),
}

#[derive(Args, Debug)]
struct Server {
    /// Run Tless server.
    ///
    /// usage:
    /// ```bash
    /// tless server -r
    /// ```
    #[clap(short, long)]
    run: bool,

    /// Port that server binding.
    ///
    /// usage:
    /// ```bash
    /// tless server -r -p 12345
    /// ```
    #[clap(short, long, default_value_t = 8917)]
    port: u16,
}

#[derive(Args, Debug)]
struct Blog {
    #[command(subcommand)]
    cli: BlogArgs,
}

#[derive(Subcommand, Debug, Clone)]
enum BlogArgs {
    /// Add a draft blog.
    /// If file exists, print failed.
    ///
    /// usage:
    /// ```bash
    /// # add a draft blog named 'FirstBlog'
    /// tless blog add FirstBlog
    /// ```
    Add { name: String },

    /// Remove `class/name`, default class is `draft`.
    ///
    /// usage:
    /// ```bash
    /// # remove draft/FirstBlog
    /// tless blog remove FirstBlog
    ///
    /// # remove private post/Blog
    /// tless blog remove -c post -p Blog
    /// ```
    Remove {
        #[arg(short, long, default_value = "draft")]
        class: String,

        name: String,
    },

    /// Publish a draft to post.
    /// If file not exists, print failed.
    ///
    /// usage:
    /// ```bash
    /// # publish draft/FirstBlog to post/FirstBlog as public post
    /// tless blog publish FirstBlog
    /// ```
    Publish { name: String },
}

#[derive(Args, Debug)]
struct Page {
    #[command(subcommand)]
    cli: PageArgs,
}

#[derive(Subcommand, Debug, Clone)]
enum PageArgs {
    /// Add a page named `name`.
    /// If page exists, print failed.
    ///
    /// usage:
    /// ```bash
    /// # add a page named 'tags'
    /// tless page add tags
    /// ```
    Add { name: String },

    /// Remove page named `name`.
    /// If page not exists, print failed.
    ///
    /// usage:
    /// ```bash
    /// # remove a page named 'tags'
    /// tless page remove tags
    /// ```
    Remove { name: String },
}

#[derive(Args, Debug)]
#[group(required = true, multiple = false)]
struct Site {
    /// Initialize site structure.
    ///
    /// usage:
    /// ```bash
    /// tless site -i
    /// ```
    #[clap(short, long)]
    init: bool,

    /// Generate static pages.
    ///
    /// usage:
    /// ```bash
    /// tless site -g
    /// ```
    #[clap(short, long)]
    generate: bool,

    /// Deploy site to github page.
    ///
    /// usage:
    /// ```bash
    /// tless site -d
    /// ```
    #[clap(short, long)]
    deploy: bool,

    /// Backup site data to pkg.
    ///
    /// usage:
    /// ```bash
    /// tless site -b
    /// ```
    #[clap(short, long)]
    backup: bool,
}

/// Parse command line arguments and run the selected subcommand.
pub fn parse_cmd() -> Result<(), AppError> {
    let input = Command::try_parse().map_err(|e| AppError::usage(e.to_string()))?;
    match input.cmd {
        Commands::Server(server) => handle_server(server).map_err(AppError::from),
        Commands::Blog(blog) => handle_blog(blog).map_err(AppError::from),
        Commands::Page(page) => handle_page(page).map_err(AppError::from),
        Commands::Site(site) => handle_site(site).map_err(AppError::from),
    }
}

fn handle_server(server: Server) -> Result<()> {
    let current_dir = env::current_dir().context("Cannot get current directory")?;
    if !current_dir.join("tless.toml").exists() {
        bail!("tless.toml not found in current directory");
    }
    if server.run && (1025..=65534).contains(&server.port) {
        server::run(server.port).context("Failed to start server")?;
    } else {
        bail!("Server not started. Use -r to run the server. Port must be between 1025 and 65534.");
    }
    Ok(())
}

fn handle_blog(blog: Blog) -> Result<()> {
    match &blog.cli {
        BlogArgs::Add { name } => file::add_blog(name).context("Failed to add blog"),
        BlogArgs::Remove { class, name } => {
            file::remove_blog(name, class).context("Failed to remove blog")
        }
        BlogArgs::Publish { name } => file::publish_blog(name).context("Failed to publish blog"),
    }
}

fn handle_page(page: Page) -> Result<()> {
    match &page.cli {
        PageArgs::Add { name } => file::add_page(name).context("Failed to add page"),
        PageArgs::Remove { name } => file::remove_page(name).context("Failed to remove page"),
    }
}

fn handle_site(site: Site) -> Result<()> {
    if site.init {
        info!("Initializing site structure...");
        server::init().context("Failed to initialize site structure")?;
        info!("Finish site structure...");
        Ok(())
    } else if site.generate {
        info!("Generating static pages...");
        tokio::runtime::Handle::current()
            .block_on(server::render_all())
            .context("Failed to generate static pages")?;
        info!("Generated static pages");
        Ok(())
    } else if site.deploy {
        info!("Deploying site to GitHub Pages...");
        Ok(())
    } else if site.backup {
        info!("Backing up site data...");
        Ok(())
    } else {
        bail!("No valid site operation specified");
    }
}
