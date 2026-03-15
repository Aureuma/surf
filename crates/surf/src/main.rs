use std::process::ExitCode;

use surf::cli;
use surf::constants::{SURF_STANDALONE_BYPASS, SURF_WRAPPER_ENV_NAME};

fn main() -> ExitCode {
    require_si_wrapper();

    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match cli::run(&args) {
        Ok(code) => ExitCode::from(code as u8),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn require_si_wrapper() {
    let wrapped = std::env::var(SURF_WRAPPER_ENV_NAME).ok();
    let bypass = std::env::var(SURF_STANDALONE_BYPASS).ok();
    if wrapped.as_deref() == Some("1") || bypass.as_deref() == Some("1") {
        return;
    }

    eprintln!("surf is managed through `si surf` and is not a standalone public CLI.");
    eprintln!("Run: si surf <command> [args]");
    eprintln!("For local development only, set {SURF_STANDALONE_BYPASS}=1.");
    std::process::exit(2);
}
