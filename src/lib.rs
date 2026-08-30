use tracing_subscriber::EnvFilter;

pub mod cmd;
pub mod file;
pub mod macros;
pub mod server;
pub mod site;

pub fn init_logging() {
    // tracing init
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug")),
        )
        .with_target(true)
        .with_line_number(true)
        .compact()
        .init();
}
