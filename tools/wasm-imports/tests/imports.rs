//! Regression tests for the WASM import guard.
//!
//! The import list is a property of the compiled artifact, so the tests
//! assemble small WAT modules rather than compiling a second contract.

use std::fs;

use wasm_imports::{check_denied, check_snapshot, scan_bytes, scan_wasm, DENY_LIST};

/// A minimal module that imports exactly one `env` function.
const UPGRADE_WAT: &str = r#"
(module
  (import "env" "l6" (func $update_current_contract_wasm (param i64) (result i64)))
  (func (export "ping") (result i32)
    i32.const 0))
"#;

/// The same module, importing a harmless host function instead.
const HARMLESS_WAT: &str = r#"
(module
  (import "env" "x0" (func $log (param i64 i64 i64)))
  (func (export "ping") (result i32)
    i32.const 0))
"#;

fn assemble(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).expect("fixture WAT must assemble")
}

#[test]
fn resolves_short_import_names_to_host_functions() {
    let scan = scan_bytes(&assemble(UPGRADE_WAT)).expect("scan");
    assert_eq!(
        scan.resolved.get("env:l6").and_then(Option::as_deref),
        Some("update_current_contract_wasm")
    );
}

#[test]
fn a_wasm_that_upgrades_itself_fails_the_denylist() {
    let scan = scan_bytes(&assemble(UPGRADE_WAT)).expect("scan");
    let err = check_denied(&scan).expect_err("upgrade import must be denied");
    assert!(
        err.contains("update_current_contract_wasm"),
        "error must name the denied function, got: {err}"
    );
    assert!(
        err.contains("entry 26"),
        "error must point at the threat-model entry, got: {err}"
    );
}

#[test]
fn a_deployer_import_is_denied_too() {
    // "l3" is `create_contract` in the vendored env.json.
    let wat = r#"
        (module
          (import "env" "l3" (func $deploy (param i64 i64) (result i64))))
    "#;
    let scan = scan_bytes(&assemble(wat)).expect("scan");
    let err = check_denied(&scan).expect_err("deployer import must be denied");
    assert!(err.contains("create_contract"), "got: {err}");
}

#[test]
fn harmless_imports_pass_the_denylist() {
    let scan = scan_bytes(&assemble(HARMLESS_WAT)).expect("scan");
    check_denied(&scan).expect("log is not on the denylist");
    assert!(scan.denied.is_empty());
}

#[test]
fn every_denylisted_name_exists_in_the_vendored_table() {
    // Catches a typo in DENY_LIST that would silently disarm that entry:
    // an entry naming a function the resolver never produces can never fire.
    let known = wasm_imports::known_host_functions();
    for entry in DENY_LIST {
        assert!(
            known.contains(entry.name),
            "DENY_LIST entry {:?} is not a host function in the vendored env.json",
            entry.name
        );
    }
}

#[test]
fn the_snapshot_is_required_and_compared() {
    let dir = tempfile::tempdir().expect("tempdir");
    let snapshot = dir.path().join("imports.snapshot");
    let scan = scan_bytes(&assemble(HARMLESS_WAT)).expect("scan");

    let err = check_snapshot(&scan, &snapshot).expect_err("missing snapshot must fail");
    assert!(err.contains("--update"), "got: {err}");

    wasm_imports::write_snapshot(&snapshot, &scan.render()).expect("write snapshot");
    check_snapshot(&scan, &snapshot).expect("unchanged snapshot must pass");

    let upgraded = scan_bytes(&assemble(UPGRADE_WAT)).expect("scan");
    let err = check_snapshot(&upgraded, &snapshot).expect_err("drift must fail");
    assert!(err.contains("differ"), "got: {err}");
}

#[test]
fn keeps_every_import_from_the_same_module() {
    // Regression: the first version keyed imports by module, so `l:_` and
    // `l:0` collided and only the snapshot's last one survived.
    let wat = r#"
        (module
          (import "env" "l_" (func $put (param i64)))
          (import "env" "l0" (func $has (param i64) (result i32))))
    "#;
    let scan = scan_bytes(&assemble(wat)).expect("scan");
    assert_eq!(scan.imports.len(), 2);
    assert_eq!(
        scan.resolved.get("env:l_").and_then(Option::as_deref),
        Some("put_contract_data")
    );
    assert_eq!(
        scan.resolved.get("env:l0").and_then(Option::as_deref),
        Some("has_contract_data")
    );
}

#[test]
fn a_missing_wasm_is_a_clear_error() {
    let err = scan_wasm(std::path::Path::new("/definitely/not/here.wasm"))
        .expect_err("missing file must fail");
    assert!(err.contains("cannot read WASM"), "got: {err}");
}

#[test]
fn the_committed_snapshot_matches_the_release_wasm() {
    // This is the guard itself, only when the release WASM has been built.
    // `stellar contract build` is a separate CI job from `cargo test`, so
    // skip loudly rather than fail a plain `cargo test` checkout.
    let wasm = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/wasm32v1-none/release/harvestlock_escrow.wasm");
    if !wasm.exists() {
        eprintln!(
            "skipping: {} is absent; run `stellar contract build` to arm this test",
            wasm.display()
        );
        return;
    }
    let scan = scan_wasm(&wasm).expect("scan release WASM");
    check_denied(&scan).expect("release WASM must not link upgrade/deploy capabilities");
    let snapshot = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("imports.snapshot");
    let committed = fs::read_to_string(&snapshot).expect("committed snapshot must exist");
    assert_eq!(
        committed,
        scan.render(),
        "the committed snapshot is stale; run the tool with --update"
    );
}
