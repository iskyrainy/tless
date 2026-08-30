/// Macro to handle Result types with customizable error and success messages or handlers.
/// # Arguments
/// * `$result` - The Result to be matched.
/// * `$err_msg` - The error message to display on failure.
/// * `$suc_msg` - (Optional) The success message to display on success.
/// * `err_handler` - (Optional) A custom error handler function.
/// * `ok_handler` - (Optional) A custom success handler function.
/// # Examples
/// ```
/// let result: Result<(), &str> = Err("An error occurred");
/// result_matcher!(result, "Failed to execute", "Executed successfully");
/// ```
#[macro_export]
macro_rules! result_matcher {
    ($result:expr, err_handler = $err_handler:expr) => {
        match $result {
            Err(err) => $err_handler(err),
            Ok(suc) => suc,
        }
    };
    ($result:expr, err_handler = $err_handler:expr, ok_handler = $ok_handler:expr) => {
        match $result {
            Err(err) => $err_handler(err),
            Ok(mut suc) => {
                $ok_handler(&mut suc);
                suc
            }
        }
    };
    ($result:expr, $err_msg:expr) => {
        match $result {
            Err(err) => {
                tracing::error!("{}: {}", $err_msg, err);
                std::process::exit(1);
            }
            Ok(suc) => suc,
        }
    };
    ($result:expr, $err_msg:expr, $suc_msg:expr) => {
        match $result {
            Err(err) => {
                tracing::error!("{}: {}", $err_msg, err);
                std::process::exit(1);
            }
            Ok(suc) => {
                tracing::info!("{}", $suc_msg);
                suc
            }
        }
    };
}
