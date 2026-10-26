//! The inspector: one method applied identically after a session ends and after a restart.

use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use crate::cycle::derive_needles;
use crate::probe::{END, ENDED, EXIT, IDLE, LIVE, Mode};
use crate::report::{Canary, Expectation, Inspection, Report, Scope, ScopeResult};
use crate::scan::Scanner;

const TICKET_BASIS: &str = "remotecam-pairing::listener::tests::server_config_issues_no_tickets_and_keeps_no_resumable_session";

pub struct Options {
    pub seed: u64,
    /// The real home directory, whose credential stores are swept.
    pub home: PathBuf,
    pub probe_executable: PathBuf,
}

/// A probe process confined to a throwaway sandbox for every directory it could write to.
struct Probe {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Probe {
    fn spawn(options: &Options, mode: Mode, sandbox: &Sandbox) -> io::Result<Self> {
        let mut child = Command::new(&options.probe_executable)
            .args([
                "probe",
                "--mode",
                mode.name(),
                "--seed",
                &options.seed.to_string(),
            ])
            .env_clear()
            .env("HOME", sandbox.path("home"))
            .env("XDG_CONFIG_HOME", sandbox.path("config"))
            .env("XDG_DATA_HOME", sandbox.path("data"))
            .env("XDG_STATE_HOME", sandbox.path("state"))
            .env("XDG_CACHE_HOME", sandbox.path("cache"))
            .env("TMPDIR", sandbox.path("tmp"))
            .current_dir(sandbox.path("home"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("no probe stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("no probe stdout"))?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn expect(&mut self, line: &str) -> io::Result<()> {
        let mut received = String::new();
        self.stdout.read_line(&mut received)?;
        if received.trim_end() == line {
            Ok(())
        } else {
            Err(io::Error::other(format!("the probe did not report {line}")))
        }
    }

    fn send(&mut self, command: &str) -> io::Result<()> {
        writeln!(self.stdin, "{command}")?;
        self.stdin.flush()
    }

    fn exit(mut self) -> io::Result<()> {
        self.send(EXIT)?;
        let status = self.child.wait()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other("the probe exited with an error"))
        }
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    const DIRECTORIES: [&'static str; 6] = ["home", "config", "data", "state", "cache", "tmp"];

    fn create(label: &str) -> io::Result<Self> {
        let root =
            std::env::temp_dir().join(format!("remotecam-residue-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for name in Self::DIRECTORIES {
            fs::create_dir_all(root.join(name))?;
        }
        Ok(Self { root })
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn memory_scope(scanner: &Scanner, probe: Option<&Probe>) -> io::Result<ScopeResult> {
    let sweep = probe
        .map(|probe| scanner.sweep_process_memory(probe.pid()))
        .transpose()?;
    let mut scope = ScopeResult::new(Scope::ProcessMemory, "scan", sweep.is_some());
    if let Some(sweep) = sweep {
        scope.unreadable_regions = Some(sweep.unreadable_regions);
        scope.findings = sweep.findings.into_iter().collect();
    }
    Ok(scope)
}

fn state_scope(scanner: &Scanner, sandbox: &Sandbox) -> io::Result<ScopeResult> {
    let tree = scanner.sweep_tree(&sandbox.root)?;
    let mut scope = ScopeResult::new(Scope::StateDirectory, "scan", true);
    scope.files = Some(tree.files);
    scope.findings = tree.sweep.findings.into_iter().collect();
    Ok(scope)
}

/// The real home cannot be written by the sandboxed probe, so a clean sweep there only means
/// something if the same sweep finds a secret planted in a scratch home (the control).
fn stores_scope(scanner: &Scanner, home: &Path) -> io::Result<ScopeResult> {
    let stores = scanner.sweep_credential_stores(home);
    let mut scope = ScopeResult::new(Scope::CredentialStores, "scan", true);
    scope.findings = stores
        .iter()
        .flat_map(|store| store.findings.iter().copied())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    scope.stores = stores.iter().map(|store| store.name).collect();
    scope.unreadable_stores = Some(stores.iter().filter(|store| store.unreadable).count() as u64);
    scope.control_detected = Some(credential_control(scanner)?);
    Ok(scope)
}

fn credential_control(scanner: &Scanner) -> io::Result<bool> {
    let scratch = Sandbox::create("control")?;
    let home = scratch.path("home");
    fs::write(home.join(".netrc"), scanner.sample().unwrap_or_default())?;
    Ok(scanner
        .sweep_credential_stores(&home)
        .iter()
        .any(|store| !store.findings.is_empty()))
}

/// TLS tickets are never issued, so there is nothing to scan for at run time. The invariant
/// is asserted by a unit test of the pairing crate, which this report names as its basis.
fn ticket_scope() -> ScopeResult {
    let mut scope = ScopeResult::new(Scope::TlsTicketState, "unit_test", false);
    scope.basis = Some(TICKET_BASIS);
    scope
}

fn inspect(
    case: &'static str,
    expectation: Expectation,
    scanner: &Scanner,
    options: &Options,
    sandbox: &Sandbox,
    probe: Option<&Probe>,
) -> io::Result<Inspection> {
    Ok(Inspection::judge(
        case,
        expectation,
        vec![
            memory_scope(scanner, probe)?,
            state_scope(scanner, sandbox)?,
            stores_scope(scanner, &options.home)?,
            ticket_scope(),
        ],
    ))
}

fn run_canary(scanner: &Scanner, options: &Options) -> io::Result<Canary> {
    let sandbox = Sandbox::create("canary")?;
    let mut probe = Probe::spawn(options, Mode::Canary, &sandbox)?;
    probe.expect(LIVE)?;
    probe.send(END)?;
    probe.expect(ENDED)?;
    let memory = scanner.sweep_process_memory(probe.pid())?;
    let state = scanner.sweep_tree(&sandbox.root)?;
    probe.exit()?;
    Ok(Canary {
        memory_detected: !memory.findings.is_empty(),
        state_detected: !state.sweep.findings.is_empty(),
    })
}

pub fn run(options: &Options) -> io::Result<Report> {
    let needles = derive_needles(options.seed).map_err(io::Error::other)?;
    let labels = needles.iter().map(|needle| needle.label()).collect();
    let scanner = Scanner::new(needles);
    let mut inspections = Vec::with_capacity(4);

    let sandbox = Sandbox::create("session")?;
    let mut probe = Probe::spawn(options, Mode::Session, &sandbox)?;
    probe.expect(LIVE)?;
    inspections.push(inspect(
        "live-control",
        Expectation::Found,
        &scanner,
        options,
        &sandbox,
        Some(&probe),
    )?);
    probe.send(END)?;
    probe.expect(ENDED)?;
    inspections.push(inspect(
        "session-end",
        Expectation::Clean,
        &scanner,
        options,
        &sandbox,
        Some(&probe),
    )?);
    probe.exit()?;
    inspections.push(inspect(
        "process-exit",
        Expectation::Clean,
        &scanner,
        options,
        &sandbox,
        None,
    )?);

    let mut restarted = Probe::spawn(options, Mode::Idle, &sandbox)?;
    restarted.expect(IDLE)?;
    inspections.push(inspect(
        "restart",
        Expectation::Clean,
        &scanner,
        options,
        &sandbox,
        Some(&restarted),
    )?);
    restarted.exit()?;

    let canary = run_canary(&scanner, options)?;
    Ok(Report::new(options.seed, labels, inspections, canary))
}

/// The environment variable that locates the real home directory.
pub fn home_from_environment() -> OsString {
    std::env::var_os("HOME").unwrap_or_default()
}
