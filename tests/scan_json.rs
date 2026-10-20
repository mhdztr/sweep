//! `scan --json` schema tests (plan §4 scan_json): exact required keys,
//! deterministic fixture values, exit code 0.
//!
//! The CLI test passes env to the child process only (no process-env
//! mutation). The tmp rule scans the REAL /tmp in the CLI case, only
//! `>= 0` is asserted there (plan §4 risk note); home-derived rules are
//! asserted exactly against the fixture.

mod common;

use serde_json::Value;
use sweep::ctx::Ctx;

const RULE_KEYS: [&str; 11] = [
    "applied",
    "bytes",
    "dirs",
    "files",
    "id",
    "needs_sudo",
    "net_of",
    "newest_age_secs",
    "reason",
    "tier",
    "title",
];

fn keys_of(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().expect("object").keys().cloned().collect();
    k.sort();
    k
}

fn assert_schema_keys(v: &Value) {
    let mut top = vec![
        "version".to_string(),
        "kind".to_string(),
        "host".to_string(),
        "rules".to_string(),
        "totals".to_string(),
        "config".to_string(),
    ];
    top.sort();
    assert_eq!(keys_of(v), top, "top-level keys");
    assert_eq!(v["version"], 1, "schema version");
    assert_eq!(v["kind"], "scan", "kind");

    // host without -v: no hostname key.
    let mut host = vec![
        "distro".to_string(),
        "family".to_string(),
        "pkgman".to_string(),
        "fstype".to_string(),
    ];
    host.sort();
    assert_eq!(keys_of(&v["host"]), host, "host keys (non-verbose)");
    assert!(
        v["host"].get("hostname").is_none(),
        "hostname omitted without -v"
    );

    assert_eq!(
        keys_of(&v["totals"]),
        ["reclaimable_bytes", "regular_bytes", "scan_ms"],
        "totals keys"
    );
    assert_eq!(
        keys_of(&v["config"]),
        ["keep_days", "preset"],
        "config keys"
    );

    let rules = v["rules"].as_array().expect("rules array");
    assert_eq!(
        rules
            .iter()
            .map(|r| r["id"].as_str().expect("id"))
            .collect::<Vec<_>>(),
        ["tmp", "user-cache", "thumbnails", "trash"],
        "rule order = registry order"
    );
    for r in rules {
        assert_eq!(keys_of(r), RULE_KEYS.to_vec(), "per-rule: exactly 11 keys");
        assert_eq!(r["tier"], "safe");
        assert!(r["applied"].is_boolean());
        assert!(r["reason"].is_null(), "applied rules have null reason");
        assert_eq!(r["needs_sudo"], false);
        assert!(r["bytes"].is_u64() && r["files"].is_u64() && r["dirs"].is_u64());
        // Key always present; null only when the rule counted nothing (e.g.
        // an empty real /tmp in the CLI case).
        assert!(
            r.get("newest_age_secs").is_some(),
            "newest_age_secs key always present"
        );
    }
}

#[test]
fn scan_report_serializes_required_keys() {
    let dir = tempfile::tempdir().expect("tempdir");
    common::scan_fixture(dir.path());
    let ctx = Ctx::for_test(dir.path().to_path_buf());
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let out = sweep::scan::run(&ctx, cancel, None);

    let report = sweep::report::ScanReport::from_scan(&ctx, &out, false);
    let v = serde_json::to_value(&report).expect("serialize");
    assert_schema_keys(&v);

    // Deterministic fixture values (plan §4).
    assert_eq!(v["host"]["distro"], "arch");
    assert_eq!(v["host"]["family"], "arch");
    assert_eq!(v["host"]["pkgman"], "pacman");
    assert_eq!(v["host"]["fstype"], "ext4");
    assert_eq!(v["totals"]["reclaimable_bytes"], 20_602);
    assert_eq!(v["totals"]["regular_bytes"], 20_602);
    assert_eq!(v["config"]["preset"], "regular");
    assert_eq!(v["config"]["keep_days"], 1);

    let tmp = &v["rules"][0];
    assert_eq!(tmp["bytes"], 3_282);
    assert_eq!(tmp["files"], 2);
    assert_eq!(tmp["dirs"], 2);
    assert_eq!(tmp["net_of"], serde_json::json!([]));

    let uc = &v["rules"][1];
    assert_eq!(uc["bytes"], 10_024, "user-cache net of thumbnails");
    assert_eq!(uc["files"], 2);
    assert_eq!(uc["dirs"], 2);
    assert_eq!(uc["net_of"], serde_json::json!(["thumbnails"]));

    assert_eq!(v["rules"][2]["bytes"], 5_120);
    assert_eq!(v["rules"][3]["bytes"], 2_176);

    // Verbose adds host.hostname and per-rule samples (plan §2.13 / D-7).
    let verbose = sweep::report::ScanReport::from_scan(&ctx, &out, true);
    let vv = serde_json::to_value(&verbose).expect("serialize");
    assert_eq!(vv["host"]["hostname"], "testhost");
    let samples = vv["rules"][0]["samples"].as_array().expect("samples");
    assert!(!samples.is_empty() && samples.len() <= 20, "≤ 20 samples");
}

#[test]
fn cli_scan_json_required_keys_and_exit_zero() {
    let dir = tempfile::tempdir().expect("tempdir");
    common::scan_fixture(dir.path());

    let out = assert_cmd::Command::cargo_bin("sweep")
        .expect("cargo_bin")
        .args(["scan", "--json"])
        .env("SWEEP_HOME", dir.path())
        .env("SWEEP_TEST_GUARD", "1")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    // Entire stdout must parse as ONE JSON document (D-3: stdout-only).
    let v: Value = serde_json::from_slice(&out).expect("stdout is one JSON doc");
    assert_schema_keys(&v);

    // tmp scans the REAL /tmp here, only >= 0 (plan §4 risk note).
    assert!(
        v["rules"][0]["bytes"].as_u64().is_some(),
        "tmp bytes is a u64 (>= 0) even against the real /tmp"
    );
    assert!(
        v["rules"][0]["newest_age_secs"].is_u64() || v["rules"][0]["newest_age_secs"].is_null()
    );
    // Home-derived rules are fixture-exact.
    assert_eq!(v["rules"][1]["bytes"], 10_024);
    assert_eq!(v["rules"][2]["bytes"], 5_120);
    assert_eq!(v["rules"][3]["bytes"], 2_176);
    assert_eq!(v["config"]["keep_days"], 1);
}
