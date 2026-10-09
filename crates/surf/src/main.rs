use std::process::ExitCode;

use surf::cli;

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match cli::run(&args) {
        Ok(code) => ExitCode::from(code as u8),
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}
