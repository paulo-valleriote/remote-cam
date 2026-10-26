use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use remotecam_residue::scan::{Needle, Scanner};

const SECRET: &[u8] = b"residue-test-secret-0123456789abcdef";

struct Sleeper(Child);

impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn scanner() -> Scanner {
    Scanner::new(vec![Needle::new("secret", SECRET.to_vec())])
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("remotecam-residue-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

fn sleeper_holding(value: Option<&[u8]>) -> Sleeper {
    let mut command = Command::new("sleep");
    command.arg("30").stdout(Stdio::null());
    if let Some(value) = value {
        command.env("HELD", String::from_utf8_lossy(value).into_owned());
    }
    let child = command.spawn().expect("sleep");
    std::thread::sleep(std::time::Duration::from_millis(100));
    Sleeper(child)
}

#[test]
fn process_memory_sweep_finds_a_secret_the_process_holds() {
    let process = sleeper_holding(Some(SECRET));

    let sweep = scanner()
        .sweep_process_memory(process.0.id())
        .expect("memory sweep");

    assert!(sweep.findings.contains("secret"));
    assert!(sweep.bytes_scanned > 0);
}

#[test]
fn process_memory_sweep_is_clean_for_a_process_without_the_secret() {
    let process = sleeper_holding(None);

    let sweep = scanner()
        .sweep_process_memory(process.0.id())
        .expect("memory sweep");

    assert!(sweep.findings.is_empty());
    assert!(sweep.bytes_scanned > 0);
}

#[test]
fn process_memory_sweep_of_a_missing_process_is_an_error() {
    assert!(scanner().sweep_process_memory(u32::MAX).is_err());
}

#[test]
fn tree_sweep_finds_a_secret_in_a_nested_file_and_counts_files() {
    let root = scratch("tree-hit");
    fs::create_dir_all(root.join("a/b")).expect("dirs");
    fs::write(root.join("a/b/state.bin"), [b"xx", SECRET, b"yy"].concat()).expect("file");
    fs::write(root.join("a/other"), b"nothing").expect("file");

    let sweep = scanner().sweep_tree(&root).expect("tree sweep");

    assert!(sweep.sweep.findings.contains("secret"));
    assert_eq!(sweep.files, 2);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn tree_sweep_of_an_empty_tree_finds_no_files() {
    let root = scratch("tree-empty");
    fs::create_dir_all(root.join("a/b")).expect("dirs");

    let sweep = scanner().sweep_tree(&root).expect("tree sweep");

    assert_eq!(sweep.files, 0);
    assert!(sweep.sweep.findings.is_empty());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn tree_sweep_of_a_missing_root_is_empty() {
    let sweep = scanner()
        .sweep_tree(Path::new("/nonexistent/remotecam-residue"))
        .expect("a missing root holds nothing");

    assert_eq!(sweep.files, 0);
}

#[test]
fn tree_sweep_does_not_follow_symlinks() {
    let root = scratch("tree-link");
    let outside = scratch("tree-link-outside");
    fs::write(outside.join("secret.bin"), SECRET).expect("file");
    std::os::unix::fs::symlink(&outside, root.join("link")).expect("symlink");

    let sweep = scanner().sweep_tree(&root).expect("tree sweep");

    assert!(sweep.sweep.findings.is_empty());
    fs::remove_dir_all(root).expect("cleanup");
    fs::remove_dir_all(outside).expect("cleanup");
}

#[test]
fn store_sweep_scans_known_credential_locations_under_a_home() {
    let home = scratch("home");
    fs::create_dir_all(home.join(".local/share/keyrings")).expect("dirs");
    fs::write(home.join(".local/share/keyrings/login.keyring"), SECRET).expect("file");

    let stores = scanner().sweep_credential_stores(&home);

    let hit: Vec<_> = stores
        .iter()
        .filter(|store| store.findings.contains("secret"))
        .map(|store| store.name)
        .collect();
    assert_eq!(hit, ["secret-service-keyring"]);
    assert!(stores.iter().any(|store| store.name == "netrc"));
    fs::remove_dir_all(home).expect("cleanup");
}

#[test]
fn store_sweep_of_a_clean_home_finds_nothing() {
    let home = scratch("home-clean");

    let stores = scanner().sweep_credential_stores(&home);

    assert!(stores.iter().all(|store| store.findings.is_empty()));
    fs::remove_dir_all(home).expect("cleanup");
}
