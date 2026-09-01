//! Unified error handling: one error type carrying a process exit code, and a
//! single exit path for startup failures that cannot return a `Result`.
//!
//! Exit codes: `0` = success, `1` = runtime failure, `2` = usage error (the
//! same convention clap uses for invalid CLI arguments).

use std::fmt;
use std::process;

const EXIT_FAILURE: u8 = 1;
const EXIT_USAGE: u8 = 2;

/// Application error with an associated process exit code.
#[derive(Debug)]
pub struct AppError {
    code: u8,
    message: String,
}

impl AppError {
    /// Create a runtime failure error (exit code 1).
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            code: EXIT_FAILURE,
            message: message.into(),
        }
    }

    /// Create a usage error for invalid CLI arguments (exit code 2).
    pub fn usage(message: impl Into<String>) -> Self {
        let message = message.into();
        // clap prefixes its rendered errors with "error: "; drop it because
        // the message is logged under the ERROR level already
        let message = message
            .strip_prefix("error: ")
            .unwrap_or(&message)
            .to_string();
        Self {
            code: EXIT_USAGE,
            message,
        }
    }

    /// Process exit code this error should terminate with.
    pub fn exit_code(&self) -> u8 {
        self.code
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        // `{:#}` renders the full error chain with all context
        Self::new(format!("{err:#}"))
    }
}

/// Log an unrecoverable startup failure and terminate the process.
/// The only place in the codebase allowed to call `process::exit`.
pub fn fatal(message: impl fmt::Display) -> ! {
    tracing::error!("{message}");
    process::exit(EXIT_FAILURE as i32);
}
