use remotecam_residue::scan::{Needle, Scanner};

fn scanner() -> Scanner {
    Scanner::new(vec![
        Needle::new("token", vec![0xde, 0xad, 0xbe, 0xef, 0x01, 0x02]),
        Needle::new("key", vec![0x10, 0x20, 0x30]),
    ])
}

#[test]
fn finds_a_needle_inside_one_chunk() {
    let scanner = scanner();
    let mut scan = scanner.stream();
    scan.feed(&[0, 0, 0xde, 0xad, 0xbe, 0xef, 0x01, 0x02, 9]);
    let sweep = scan.finish();

    assert_eq!(sweep.findings.into_iter().collect::<Vec<_>>(), ["token"]);
    assert_eq!(sweep.bytes_scanned, 9);
}

#[test]
fn finds_a_needle_split_across_chunks() {
    let scanner = scanner();
    let mut scan = scanner.stream();
    scan.feed(&[7, 7, 0xde, 0xad, 0xbe]);
    scan.feed(&[0xef, 0x01]);
    scan.feed(&[0x02, 7]);

    assert!(scan.finish().findings.contains("token"));
}

#[test]
fn reports_nothing_when_no_needle_is_present() {
    let scanner = scanner();
    let mut scan = scanner.stream();
    scan.feed(&[0xde, 0xad, 0xbe, 0xef, 0x01]);
    scan.feed(&[0x10, 0x20]);

    assert!(scan.finish().findings.is_empty());
}

#[test]
fn reports_each_needle_found_once() {
    let scanner = scanner();
    let mut scan = scanner.stream();
    scan.feed(&[0x10, 0x20, 0x30, 0x10, 0x20, 0x30]);

    assert_eq!(scan.finish().findings.len(), 1);
}

#[test]
fn an_empty_needle_never_matches() {
    let scanner = Scanner::new(vec![Needle::new("empty", Vec::new())]);
    let mut scan = scanner.stream();
    scan.feed(&[1, 2, 3]);

    assert!(scan.finish().findings.is_empty());
}
