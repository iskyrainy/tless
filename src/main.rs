use std::process::ExitCode;

use tracing::error;

use tless::cmd;

fn main() -> ExitCode {
    tless::init_logging();
    match cmd::parse_cmd() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!("{e}");
            ExitCode::from(e.exit_code())
        }
    }
}
