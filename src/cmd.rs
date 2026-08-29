use std::{env, process};

use clap::{Args, Parser, Subcommand};

use crate::{
    file::{blog, page},
    result_matcher,
    server::run,
    site,
};

/// tless command arguments
#[derive(Parser, Debug)]
#[command(
    author = "gdhvxcj <wangnan5117@gmail.com>",
    version = "0.1.0",
    about = "Build blog site.",
    long_about = "Fast and easy blog site builder."
)]
#[command(propagate_version = true)]
pub struct Command {
    #[command(subcommand)]
    pub cmd: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
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
pub struct Server {
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
pub struct Blog {
    #[command(subcommand)]
    pub cli: BlogArgs,
}

#[derive(Subcommand, Debug, Clone)]
pub enum BlogArgs {
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
    Publish {
        name: String,
    },
}

#[derive(Args, Debug)]
pub struct Page {
    #[command(subcommand)]
    pub cli: PageArgs,
}

#[derive(Subcommand, Debug, Clone)]
pub enum PageArgs {
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
pub struct Site {
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

/// Parse command line arguments and check the validity.
pub fn parse_cmd() {
    let input = Command::parse();
    match input.cmd {
        Commands::Server(server) => handle_server(server),
        Commands::Blog(blog) => handle_blog(blog),
        Commands::Page(page) => handle_page(page),
        Commands::Site(site) => handle_site(site),
    }
}

fn handle_server(server: Server) {
    let current_dir = match env::current_dir() {
        Ok(dir) => dir,
        Err(_) => {
            eprintln!("Can not get current dir");
            process::exit(1);
        }
    };
    if !current_dir.join("tless.toml").exists() {
        eprintln!("Can't find configure file in current dir.");
        process::exit(1);
    }
    // check config file
    if server.run && server.port > 1024 && server.port < 65_535 {
        result_matcher!(run::run(server.port), "Failed to start server");
    } else {
        println!(
            "Server not started. Use -r to run the server. Port must be between 1025 and 65534."
        );
        process::exit(1);
    }
}

fn handle_blog(blog: Blog) {
    match &blog.cli {
        BlogArgs::Add { name } => result_matcher!(blog::add_blog(name), "Failed to add blog"),
        BlogArgs::Remove { class, name } => {
            result_matcher!(blog::remove_blog(name, class), "Failed to remove blog")
        }
        BlogArgs::Publish { name } => {
            result_matcher!(blog::publish_blog(name), "Failed to publish blog")
        }
    }
}

fn handle_page(page: Page) {
    match &page.cli {
        PageArgs::Add { name } => result_matcher!(page::add_page(name), "Failed to add page"),
        PageArgs::Remove { name } => {
            result_matcher!(page::remove_page(name), "Failed to remove page")
        }
    }
}

fn handle_site(site: Site) {
    if site.init {
        println!("Initializing site structure...");
        result_matcher!(site::init(), "Failed to initialize site structure");
    } else if site.generate {
        println!("Generating static pages...");
    } else if site.deploy {
        println!("Deploying site to GitHub Pages...");
    } else if site.backup {
        println!("Backing up site data...");
    } else {
        eprintln!("No valid site operation specified.");
        process::exit(1);
    }
}
