//! Executes the pinned conformance vectors and emits the canonical result
//! document consumed by `protocol/vectors/run_conformance.sh`.

use std::path::PathBuf;
use std::process::ExitCode;

use remotecam_protocol::conformance::{repository_root, run_vectors};

fn main() -> ExitCode {
    let mut vectors = repository_root().join("protocol/vectors");
    let mut output: Option<PathBuf> = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--vectors" => match arguments.next() {
                Some(value) => vectors = PathBuf::from(value),
                None => return usage(),
            },
            "--output" => match arguments.next() {
                Some(value) => output = Some(PathBuf::from(value)),
                None => return usage(),
            },
            _ => return usage(),
        }
    }

    let report = match run_vectors(&vectors) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("conformance: {error}");
            return ExitCode::FAILURE;
        }
    };
    let serialized = match serde_json::to_string_pretty(&report) {
        Ok(serialized) => serialized,
        Err(error) => {
            eprintln!("conformance: cannot serialize result: {error}");
            return ExitCode::FAILURE;
        }
    };
    match output {
        Some(path) => {
            if let Err(error) = std::fs::write(&path, format!("{serialized}\n")) {
                eprintln!("conformance: cannot write {}: {error}", path.display());
                return ExitCode::FAILURE;
            }
        }
        None => println!("{serialized}"),
    }

    let failures = report.failures();
    for failure in &failures {
        eprintln!("conformance: {failure}");
    }
    if report.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: conformance [--vectors <directory>] [--output <file>]");
    ExitCode::FAILURE
}
