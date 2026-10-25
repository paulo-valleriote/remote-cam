use std::process::ExitCode;

use remotecam_harness::{Verdict, run_from_args};

fn main() -> ExitCode {
    let report = match run_from_args(std::env::args_os().skip(1)) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("harness failed: {error}");
            return ExitCode::FAILURE;
        }
    };

    finish_report(report.verdict, report.to_json())
}

fn finish_report(verdict: Verdict, json: Result<String, serde_json::Error>) -> ExitCode {
    match json {
        Ok(line) => {
            println!("{line}");
            if verdict == Verdict::Pass {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_output_maps_serialization_and_verdict_failures_to_exit_codes() {
        assert_eq!(
            finish_report(Verdict::Pass, Ok("{}".to_owned())),
            ExitCode::SUCCESS
        );
        assert_eq!(
            finish_report(Verdict::Fail, Ok("{}".to_owned())),
            ExitCode::FAILURE
        );

        let error = serde_json::from_str::<u8>("").unwrap_err();
        assert_eq!(finish_report(Verdict::Pass, Err(error)), ExitCode::FAILURE);
    }
}
