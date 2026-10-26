use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::fs::FileExt;
use std::path::Path;

const CHUNK_BYTES: usize = 1 << 20;
const UNSCANNABLE_REGIONS: [&str; 3] = ["[vvar]", "[vvar_vclock]", "[vsyscall]"];

/// Where credentials usually live, relative to the home directory.
const CREDENTIAL_STORES: [(&str, &str); 8] = [
    ("secret-service-keyring", ".local/share/keyrings"),
    ("kwallet", ".local/share/kwalletd"),
    ("pass-store", ".password-store"),
    ("gnupg", ".gnupg"),
    ("ssh", ".ssh"),
    ("netrc", ".netrc"),
    ("git-credentials", ".git-credentials"),
    ("docker-credentials", ".docker/config.json"),
];

/// A byte pattern that must not survive a session, with the label reported when it does.
#[derive(Debug, Clone)]
pub struct Needle {
    label: &'static str,
    bytes: Vec<u8>,
}

impl Needle {
    pub fn new(label: &'static str, bytes: Vec<u8>) -> Self {
        Self { label, bytes }
    }

    pub fn label(&self) -> &'static str {
        self.label
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Sweep {
    pub findings: BTreeSet<&'static str>,
    pub bytes_scanned: u64,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct TreeSweep {
    pub sweep: Sweep,
    pub files: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct StoreSweep {
    pub name: &'static str,
    pub findings: BTreeSet<&'static str>,
}

pub struct Scanner {
    needles: Vec<Needle>,
}

impl Scanner {
    pub fn new(needles: Vec<Needle>) -> Self {
        Self { needles }
    }

    /// Scans every writable mapping of a live process: heap, stack and anonymous memory.
    pub fn sweep_process_memory(&self, pid: u32) -> io::Result<Sweep> {
        let maps = fs::read_to_string(format!("/proc/{pid}/maps"))?;
        let memory = File::open(format!("/proc/{pid}/mem"))?;
        let mut scan = self.stream();
        let mut buffer = vec![0_u8; CHUNK_BYTES];
        for (start, end) in writable_regions(&maps) {
            let mut address = start;
            while address < end {
                let wanted = usize::try_from(end - address)
                    .map_or(CHUNK_BYTES, |left| left.min(CHUNK_BYTES));
                match memory.read_at(&mut buffer[..wanted], address) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        scan.feed(&buffer[..read]);
                        address += read as u64;
                    }
                }
            }
        }
        Ok(scan.finish())
    }

    /// Scans every regular file below `root`; symbolic links are counted, never followed.
    pub fn sweep_tree(&self, root: &Path) -> io::Result<TreeSweep> {
        let mut scan = self.stream();
        let mut files = 0;
        let mut pending = vec![root.to_path_buf()];
        while let Some(path) = pending.pop() {
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if metadata.is_dir() {
                for entry in fs::read_dir(&path)? {
                    pending.push(entry?.path());
                }
            } else {
                files += 1;
                if metadata.is_file() {
                    let mut file = File::open(&path)?;
                    let mut chunk = vec![0_u8; CHUNK_BYTES];
                    loop {
                        let read = file.read(&mut chunk)?;
                        if read == 0 {
                            break;
                        }
                        scan.feed(&chunk[..read]);
                        // A new file must not join bytes with the previous one.
                    }
                    scan.break_stream();
                }
            }
        }
        Ok(TreeSweep {
            sweep: scan.finish(),
            files,
        })
    }

    /// Scans the credential stores under `home`; a store that does not exist holds nothing.
    pub fn sweep_credential_stores(&self, home: &Path) -> Vec<StoreSweep> {
        CREDENTIAL_STORES
            .iter()
            .map(|&(name, relative)| StoreSweep {
                name,
                findings: self
                    .sweep_tree(&home.join(relative))
                    .map(|tree| tree.sweep.findings)
                    .unwrap_or_default(),
            })
            .collect()
    }

    pub fn stream(&self) -> StreamScan<'_> {
        let longest = self
            .needles
            .iter()
            .map(|n| n.bytes.len())
            .max()
            .unwrap_or(0);
        StreamScan {
            scanner: self,
            carry: Vec::new(),
            overlap: longest.saturating_sub(1),
            sweep: Sweep::default(),
        }
    }
}

/// Scans a byte stream in chunks; a needle split across two chunks is still found.
pub struct StreamScan<'a> {
    scanner: &'a Scanner,
    carry: Vec<u8>,
    overlap: usize,
    sweep: Sweep,
}

impl StreamScan<'_> {
    pub fn feed(&mut self, chunk: &[u8]) {
        self.sweep.bytes_scanned += chunk.len() as u64;
        self.carry.extend_from_slice(chunk);
        for needle in &self.scanner.needles {
            if !self.sweep.findings.contains(needle.label) && contains(&self.carry, &needle.bytes) {
                self.sweep.findings.insert(needle.label);
            }
        }
        let keep_from = self.carry.len().saturating_sub(self.overlap);
        self.carry.drain(..keep_from);
    }

    /// Ends the current source so the next chunk cannot extend a needle from the previous one.
    pub fn break_stream(&mut self) {
        self.carry.clear();
    }

    pub fn finish(self) -> Sweep {
        self.sweep
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    let Some((&first, _)) = needle.split_first() else {
        return false;
    };
    let mut from = 0;
    while let Some(offset) = haystack[from..].iter().position(|&byte| byte == first) {
        let start = from + offset;
        if haystack[start..].starts_with(needle) {
            return true;
        }
        from = start + 1;
    }
    false
}

fn writable_regions(maps: &str) -> Vec<(u64, u64)> {
    maps.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let range = fields.next()?;
            let permissions = fields.next()?;
            let name = fields.nth(3).unwrap_or("");
            if !permissions.starts_with("rw") || UNSCANNABLE_REGIONS.contains(&name) {
                return None;
            }
            let (start, end) = range.split_once('-')?;
            Some((
                u64::from_str_radix(start, 16).ok()?,
                u64::from_str_radix(end, 16).ok()?,
            ))
        })
        .collect()
}
