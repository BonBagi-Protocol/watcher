//! Golden runs of the testnet example (examples/testnet/): the exact text, JSON and receipt a
//! member sees, plus exit codes. CI also runs the first ones inside the Docker image.
//!
//! After an intended output change: `UPDATE_GOLDEN=1 cargo test --test examples`, then review
//! the diff of examples/testnet/expected/.

use std::path::{Path, PathBuf};

use bonbagi_watcher::{args::Env, main_with};

const E: &str = "examples/testnet";
/// 2026-10-27T05:40:00Z: receipts carry a timestamp, pinned here so they are reproducible.
const EPOCH: &str = "1793079600";

fn env() -> Env {
    Env {
        source_date_epoch: Some(EPOCH.into()),
        ..Env::default()
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn run(cmd: &str) -> (i32, String, String) {
    // Paths in the commands are relative to the repository root, as in the README and Docker.
    std::env::set_current_dir(root()).unwrap();
    let args: Vec<String> = cmd.split_whitespace().map(String::from).collect();
    main_with(&args, &env())
}

fn golden(name: &str, actual: &str) {
    let path = root().join(E).join("expected").join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n");
    assert_eq!(actual, want, "output differs from {}", path.display());
}

fn charter() -> String {
    format!("--charter {E}/charter.json --snapshot {E}/snapshot.json")
}

#[test]
fn every_payment_to_member_one() {
    let (code, out, err) = run(&format!("check {} --member 1", charter()));
    assert_eq!(
        (code, err.as_str()),
        (1, ""),
        "a swapped payment is a mismatch"
    );
    golden("all.txt", &out);
}

#[test]
fn one_order_paid_exactly() {
    let cmd = format!(
        "check {} --key-file {E}/dex-key.TEST-ONLY.txt --order ord-k7m2qx9p --price 0.003",
        charter()
    );
    let (code, out, _) = run(&cmd);
    assert_eq!(code, 0);
    golden("order-paid.txt", &out);
    let (code, out, _) = run(&format!("{cmd} --json"));
    assert_eq!(code, 0);
    golden("order-paid.json", &out);
}

#[test]
fn the_receipt_is_reproducible_and_verifies() {
    let dir = std::env::temp_dir().join("bonbagi-watcher-golden");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("receipt.json");
    let cmd = format!(
        "check {} --key-file {E}/dex-key.TEST-ONLY.txt --order ord-k7m2qx9p --price 0.003 --receipt {}",
        charter(),
        file.display()
    );
    let (code, out, _) = run(&cmd);
    assert_eq!(code, 0);
    assert!(out.contains("Receipt  ") && out.contains("signed with key 8a88e3dd…6f5c"));
    golden("receipt.json", &std::fs::read_to_string(&file).unwrap());

    let (code, out, _) = run(&format!(
        "verify-receipt {E}/expected/receipt.json --charter {E}/charter.json"
    ));
    assert_eq!(code, 0);
    golden("verify-receipt.txt", &out);
}

#[test]
fn a_changed_receipt_is_invalid() {
    let text = std::fs::read_to_string(root().join(E).join("expected/receipt.json")).unwrap();
    let bad = std::env::temp_dir().join("bonbagi-watcher-receipt-changed.json");
    std::fs::write(&bad, text.replace("\"180000\"", "\"190000\"")).unwrap();
    let (code, out, _) = run(&format!(
        "verify-receipt {} --charter {E}/charter.json",
        bad.display()
    ));
    assert_eq!(code, 1);
    assert!(out.contains("FAIL  signature") && out.contains("INVALID"));
}

#[test]
fn a_pending_order_from_a_pay_link() {
    let link = std::fs::read_to_string(root().join(E).join("paylink.txt")).unwrap();
    let (code, out, _) = run(&format!(
        "check --link {} --snapshot {E}/snapshot.json --member Dex --order ord-pnd3w8kq",
        link.trim()
    ));
    assert_eq!(code, 1);
    golden("link-pending.txt", &out);
}

#[test]
fn a_swapped_output_is_a_mismatch() {
    let (code, out, _) = run(&format!(
        "check {} --member 1 --order ord-swapped1",
        charter()
    ));
    assert_eq!(code, 1);
    golden("swapped.txt", &out);
}

#[test]
fn an_order_that_never_arrived() {
    let (code, out, _) = run(&format!(
        "check {} --member 1 --order ord-nothere1",
        charter()
    ));
    assert_eq!(code, 1);
    assert!(out.contains("=> NOT FOUND  No output for order ord-nothere1"));
}

#[test]
fn input_errors_exit_2() {
    let unsigned = std::env::temp_dir().join("bonbagi-watcher-unsigned.json");
    let text = std::fs::read_to_string(root().join(E).join("charter.json")).unwrap();
    std::fs::write(
        &unsigned,
        text.replace("\"bps\": 6000", "\"bps\": 5000")
            .replace("\"bps\": 4000", "\"bps\": 5000"),
    )
    .unwrap();
    let (code, _, err) = run(&format!(
        "check --charter {} --snapshot {E}/snapshot.json --member 1",
        unsigned.display()
    ));
    assert_eq!(code, 2);
    assert!(
        err.contains("not signed by every member (missing or invalid: Dex, Hans)"),
        "{err}"
    );

    let (code, _, err) = run(&format!("check {} --member 3", charter()));
    assert_eq!(code, 2);
    assert!(err.contains("members 1 to 2"));

    // Hans's key is not Dex: a key and a member number that disagree are refused.
    let hans = std::env::temp_dir().join("bonbagi-watcher-hans-key.txt");
    std::fs::write(&hans, "02".repeat(32)).unwrap();
    let (code, _, err) = run(&format!(
        "check {} --member 1 --key-file {}",
        charter(),
        hans.display()
    ));
    assert_eq!(code, 2);
    assert!(err.contains("the key file is member 2"));

    let (code, _, err) = run(&format!(
        "check {} --member 1 --order ord-k7m2qx9p --price 0.0000001",
        charter()
    ));
    assert_eq!(code, 2);
    assert!(err.contains("cannot be split"), "{err}");
}

#[test]
fn every_example_file_is_where_the_readme_says() {
    for f in [
        "charter.json",
        "snapshot.json",
        "paylink.txt",
        "dex-key.TEST-ONLY.txt",
    ] {
        assert!(Path::new(&root().join(E).join(f)).is_file(), "{f}");
    }
}
