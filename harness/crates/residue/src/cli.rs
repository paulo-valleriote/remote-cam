use std::path::PathBuf;

use crate::probe::Mode;

pub const DEFAULT_SEED: u64 = 0;
pub const USAGE: &str = "usage: remotecam-residue [--seed N] [--out PATH]";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Inspect { seed: u64, out: Option<PathBuf> },
    Probe { mode: Mode, seed: u64 },
}

pub fn parse(args: &[String]) -> Result<Command, String> {
    let mut seed = DEFAULT_SEED;
    let mut out = None;
    let mut mode = None;
    let (probing, rest) = match args.split_first() {
        Some((first, rest)) if first == "probe" => (true, rest),
        _ => (false, args),
    };
    let mut rest = rest.iter();
    while let Some(flag) = rest.next() {
        let mut value = || rest.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--seed" => {
                seed = value()?
                    .parse()
                    .map_err(|_| "--seed must be an unsigned integer".to_owned())?;
            }
            "--out" if !probing => out = Some(PathBuf::from(value()?)),
            "--mode" if probing => {
                mode = Some(Mode::parse(value()?).ok_or_else(|| "unknown probe mode".to_owned())?);
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if probing {
        let mode = mode.ok_or_else(|| "probe needs --mode".to_owned())?;
        Ok(Command::Probe { mode, seed })
    } else {
        Ok(Command::Inspect { seed, out })
    }
}
