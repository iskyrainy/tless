//! Command line interface: argument parsing and subcommand dispatch.

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use tracing::{info, warn};

use crate::{BASE_DIR, error::AppError, file, server};

/// Tless command arguments.
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
    /// Run the Tless server and specify the port
    Server(Server),

    /// Control post drafts and posts: add/remove/publish
    Post(Post),

    /// Control pages: add/remove
    Page(Page),

    /// Initialize a site scaffold or generate the static site
    Site(Site),
}

#[derive(Args, Debug)]
struct Server {
    /// Run the Tless server.
    ///
    /// usage:
    /// ```bash
    /// tles server -r
    /// ```
    #[clap(short, long)]
    run: bool,

    /// Port the server binds to.
    ///
    /// usage:
    /// ```bash
    /// tles server -r -p 12345
    /// ```
    #[clap(short, long, default_value_t = 8917)]
    port: u16,
}

#[derive(Args, Debug)]
struct Post {
    #[command(subcommand)]
    cli: PostArgs,
}

#[derive(Subcommand, Debug, Clone)]
enum PostArgs {
    /// Add a draft post.
    /// Fails if the file already exists.
    ///
    /// usage:
    /// ```bash
    /// # add a draft post named 'FirstBlog'
    /// tles blog add FirstBlog
    /// ```
    Add { name: String },

    /// Remove `class/name`, default class is `draft`.
    ///
    /// usage:
    /// ```bash
    /// # remove draft/FirstBlog
    /// tles blog remove FirstBlog
    ///
    /// # remove a post named 'Blog'
    /// tles blog remove -c post Blog
    /// ```
    Remove {
        #[arg(short, long, default_value = "draft")]
        class: String,

        name: String,
    },

    /// Publish a draft to post.
    /// Fails if the draft does not exist.
    ///
    /// usage:
    /// ```bash
    /// # publish draft/FirstBlog to post/FirstBlog as public post
    /// tles blog publish FirstBlog
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
    /// Fails if the page already exists.
    ///
    /// usage:
    /// ```bash
    /// # add a page named 'tags'
    /// tles page add tags
    /// ```
    Add { name: String },

    /// Remove page named `name`.
    /// Fails if the page does not exist.
    ///
    /// usage:
    /// ```bash
    /// # remove a page named 'tags'
    /// tles page remove tags
    /// ```
    Remove { name: String },
}

#[derive(Args, Debug)]
#[group(required = true, multiple = true)]
struct Site {
    /// Initialize site structure.
    ///
    /// usage:
    /// ```bash
    /// tles site -i
    /// ```
    #[clap(short, long)]
    init: bool,

    /// Generate to public/.
    ///
    /// usage:
    /// ```bash
    /// tles site -g
    /// ```
    #[clap(short, long)]
    generate: bool,

    /// Translate by LLM.
    ///
    /// usage:
    /// ```bash
    /// tles site -t
    /// ```
    #[clap(short, long)]
    translate: bool,
}

/// Parse command line arguments and run the selected subcommand.
pub fn parse_cmd() -> Result<(), AppError> {
    let input = match Command::try_parse() {
        Ok(input) => input,
        // `--help` and `--version` are not errors: print them and succeed
        Err(e)
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            println!("{e}");
            return Ok(());
        }
        Err(e) => return Err(AppError::usage(e.to_string())),
    };
    match input.cmd {
        Commands::Server(server) => handle_server(server).map_err(AppError::from),
        Commands::Post(blog) => handle_post(blog).map_err(AppError::from),
        Commands::Page(page) => handle_page(page).map_err(AppError::from),
        Commands::Site(site) => handle_site(site).map_err(AppError::from),
    }
}

fn handle_server(server: Server) -> Result<()> {
    if !BASE_DIR.join("tless.toml").exists() {
        bail!("tless.toml not found in current directory");
    }
    if server.run && (1025..=65534).contains(&server.port) {
        server::run(server.port)?;
    } else {
        bail!("Server not started. Use -r to run the server. Port must be between 1025 and 65534.");
    }
    Ok(())
}

fn handle_post(post: Post) -> Result<()> {
    match &post.cli {
        PostArgs::Add { name } => file::Post::add(name),
        PostArgs::Remove { class, name } => file::Post::remove(name, class),
        PostArgs::Publish { name } => file::Post::publish(name),
    }
}

fn handle_page(page: Page) -> Result<()> {
    match &page.cli {
        PageArgs::Add { name } => file::Page::add(name),
        PageArgs::Remove { name } => file::Page::remove(name),
    }
}

fn handle_site(site: Site) -> Result<()> {
    if !(site.init || site.generate || site.translate) {
        bail!("No valid site operation specified");
    }

    if site.init {
        info!("Initializing site structure...");
        if let Err(e) = server::init() {
            warn!("Failed to init site: {}", e);
        } else {
            info!("Initialize site structure ok");
        }
    }

    if site.generate {
        info!("Generating publish/ ...");
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("Failed to create render runtime")?;
        runtime.block_on(server::render_all())?;
        info!("Generated public/");
    }

    if site.translate {
        todo!()
    }

    Ok(())
}
