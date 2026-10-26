use std::io::{self, Write};
use std::process::ExitCode;

use remotecam_residue::cli::{self, Command, USAGE};
use remotecam_residue::{inspect, probe};

#[global_allocator]
static ALLOCATOR: remotecam_scrub::ZeroizingAllocator = remotecam_scrub::ZeroizingAllocator;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match cli::parse(&args) {
        Ok(Command::Probe { mode, seed }) => {
            match probe::run(mode, seed, io::stdin().lock(), io::stdout().lock()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => fail(&error),
            }
        }
        Ok(Command::Inspect { seed, out }) => match inspect_and_print(seed, out.as_deref()) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::from(1),
            Err(error) => fail(&error),
        },
        Err(message) => {
            eprintln!("{message}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn inspect_and_print(seed: u64, out: Option<&std::path::Path>) -> io::Result<bool> {
    let options = inspect::Options {
        seed,
        home: inspect::home_from_environment().into(),
        probe_executable: std::env::current_exe()?,
    };
    let report = inspect::run(&options)?;
    let mut line = serde_json::to_vec(&report).map_err(io::Error::other)?;
    line.push(b'\n');
    if let Some(path) = out {
        std::fs::write(path, &line)?;
    }
    io::stdout().write_all(&line)?;
    Ok(report.summary.passed)
}

fn fail(error: &io::Error) -> ExitCode {
    eprintln!("residue inspection failed: {error}");
    ExitCode::from(3)
}
