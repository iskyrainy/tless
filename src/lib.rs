use std::{env, path::PathBuf, sync::LazyLock};

use tracing_subscriber::EnvFilter;

pub mod cmd;
pub mod error;
pub(crate) mod file;
pub(crate) mod server;
pub(crate) mod site;

/// Working directory the application runs from.
pub(crate) static BASE_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    env::current_dir().unwrap_or_else(|_| error::fatal("Cannot get current directory"))
});

pub fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug")),
        )
        .with_target(true)
        .with_line_number(true)
        .compact()
        .init();
}
