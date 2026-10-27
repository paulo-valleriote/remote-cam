//! The process under inspection. It speaks a line protocol on stdio so the inspector can
//! sample its memory at known points: `LIVE`, then `ENDED` after the session is dropped.

use std::fs;
use std::io::{self, BufRead, Write};

use tokio::runtime::Builder;

use crate::cycle::open_session;

pub const LIVE: &str = "LIVE";
pub const ENDED: &str = "ENDED";
pub const IDLE: &str = "IDLE";
pub const END: &str = "END";
pub const EXIT: &str = "EXIT";
const LEAK_FILE: &str = "leaked-qr.bin";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// A complete pairing, then an orderly end.
    Session,
    /// A fresh process that never pairs: the application after a restart.
    Idle,
    /// A session that deliberately leaks, proving the inspection can fail.
    Canary,
}

impl Mode {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "session" => Some(Self::Session),
            "idle" => Some(Self::Idle),
            "canary" => Some(Self::Canary),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Idle => "idle",
            Self::Canary => "canary",
        }
    }
}

fn say(output: &mut impl Write, line: &str) -> io::Result<()> {
    writeln!(output, "{line}")?;
    output.flush()
}

fn wait_for(input: &mut impl BufRead, command: &str) -> io::Result<()> {
    let mut line = String::new();
    input.read_line(&mut line)?;
    if line.trim_end() == command {
        Ok(())
    } else {
        Err(io::Error::other("the inspector sent an unexpected command"))
    }
}

pub fn run(
    mode: Mode,
    seed: u64,
    mut input: impl BufRead,
    mut output: impl Write,
) -> io::Result<()> {
    if mode == Mode::Idle {
        say(&mut output, IDLE)?;
        return wait_for(&mut input, EXIT);
    }

    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(io::Error::other)?;
    let session = runtime
        .block_on(open_session(seed))
        .map_err(io::Error::other)?;
    if mode == Mode::Canary {
        let qr = session.material().encoded_qr().to_vec();
        fs::write(LEAK_FILE, &qr)?;
        Box::leak(qr.into_boxed_slice());
        Box::leak(Box::new(session.material().tls_fingerprint()));
    }
    say(&mut output, LIVE)?;
    wait_for(&mut input, END)?;
    drop(session);
    runtime.block_on(tokio::task::yield_now());
    remotecam_scrub::scrub_stack();
    say(&mut output, ENDED)?;
    wait_for(&mut input, EXIT)
}
