//! Replays the pinned handshake scenarios and emits the canonical result
//! document consumed by `protocol/vectors/run_conformance.sh`.

use std::process::ExitCode;

use remotecam_session_core::conformance::run_cli;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match run_cli(&arguments) {
        Ok(Some(document)) => {
            println!("{document}");
            ExitCode::SUCCESS
        }
        Ok(None) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("handshake-conformance: {message}");
            ExitCode::FAILURE
        }
    }
}
