//! Binary entry point: run the CLI and map errors to process exit codes.

use std::process::ExitCode;

use tracing::error;

use tles::cmd;

fn main() -> ExitCode {
    tles::init_logging();
    match cmd::parse_cmd() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!("{e}");
            ExitCode::from(e.exit_code())
        }
    }
}
