use remotecam_residue::cli::{Command, DEFAULT_SEED, parse};
use remotecam_residue::probe::Mode;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn inspection_is_the_default_command() {
    assert_eq!(
        parse(&[]),
        Ok(Command::Inspect {
            seed: DEFAULT_SEED,
            out: None
        })
    );
}

#[test]
fn inspection_accepts_a_seed_and_an_output_path() {
    assert_eq!(
        parse(&args(&["--seed", "9", "--out", "evidence.json"])),
        Ok(Command::Inspect {
            seed: 9,
            out: Some("evidence.json".into())
        })
    );
}

#[test]
fn the_probe_needs_a_known_mode() {
    assert_eq!(
        parse(&args(&["probe", "--mode", "canary", "--seed", "3"])),
        Ok(Command::Probe {
            mode: Mode::Canary,
            seed: 3
        })
    );
    assert!(parse(&args(&["probe"])).is_err());
    assert!(parse(&args(&["probe", "--mode", "bogus"])).is_err());
}

#[test]
fn malformed_arguments_are_refused() {
    for bad in [
        &["--seed"][..],
        &["--seed", "x"],
        &["--out"],
        &["--mode", "idle"],
        &["probe", "--out", "x", "--mode", "idle"],
        &["--bogus"],
    ] {
        assert!(parse(&args(bad)).is_err(), "{bad:?}");
    }
}

#[test]
fn every_probe_mode_round_trips_through_its_name() {
    for mode in [Mode::Session, Mode::Idle, Mode::Canary] {
        assert_eq!(Mode::parse(mode.name()), Some(mode));
    }
}
