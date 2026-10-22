//! Trace writer for the pairing scenarios.
//!
//! A trace is raw evidence and stays local (it is gitignored). The evidence file
//! references it so a reviewer can follow one scenario end to end.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static TRACE_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Sets the directory traces are written to. Called once by `main`.
pub fn set_root(root: PathBuf) {
    let _ = TRACE_ROOT.set(root);
}

/// Writes one trace file, replacing any previous content.
///
/// Returns `false` when the root has not been configured or the write failed.
pub fn record(id: &str, lines: &[String]) -> bool {
    let Some(root) = TRACE_ROOT.get() else {
        return false;
    };
    write_trace(root, id, lines)
}

fn write_trace(root: &Path, id: &str, lines: &[String]) -> bool {
    if std::fs::create_dir_all(root).is_err() {
        return false;
    }
    let mut content = String::new();
    for line in lines {
        content.push_str(line);
        content.push('\n');
    }
    std::fs::write(root.join(format!("{id}.trace")), content).is_ok()
}
