use std::process::ExitCode;

use remotecam_harness::{Verdict, run};

fn main() -> ExitCode {
    let report = run();

    match report.to_json() {
        Ok(line) => {
            println!("{line}");
            if report.verdict == Verdict::Pass {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("harness report serialisation failed: {error}");
            ExitCode::FAILURE
        }
    }
}
