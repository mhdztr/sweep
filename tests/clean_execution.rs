//! M2 execution tests: safe candidate deletion, dry-run safety, and reports.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use sweep::ctx::Ctx;
use sweep::exec::{self, ExitCode, Plan, PlanItem};
use sweep::rules::Tier;

fn plan(paths: Vec<PathBuf>, dry_run: bool) -> Plan {
    Plan {
        selected: vec![PlanItem {
            id: "tmp",
            needs_sudo: false,
            tier: Tier::Safe,
            paths,
            purge_ack: false,
        }],
        gate_ack: BTreeMap::new(),
        keep_days: Some(1),
        dry_run,
        trash: false,
        schema: sweep::SCHEMA_VERSION,
    }
}

fn fixture_old_file(home: &Path) -> PathBuf {
    common::scan_fixture(home);
    home.join(".fake-tmp/old.bin")
}

#[test]
fn execution_removes_candidates_and_writes_history() {
    let dir = tempfile::tempdir().expect("tempdir");
    let old = fixture_old_file(dir.path());
    let newer = dir.path().join(".fake-tmp/newer.bin");
    let ctx = Ctx::for_test(dir.path().to_path_buf());

    let report = exec::execute(&ctx, &plan(vec![old.clone()], false)).expect("execute");

    assert_eq!(report.exit_hint, ExitCode::Ok);
    assert_eq!(report.freed_total, 1_234);
    assert!(!old.exists(), "selected candidate must be removed");
    assert!(newer.exists(), "unselected candidate must survive");

    let state = sweep::state::state_dir(dir.path());
    let history = state.join("history.jsonl");
    assert!(history.is_file());
    let content = std::fs::read_to_string(history).expect("history");
    assert!(content.contains("\"freed_total\":1234"), "{content}");
    assert_eq!(
        std::fs::read_dir(state.join("logs")).expect("logs").count(),
        1
    );
}

#[test]
fn dry_run_preserves_files_and_state() {
    let dir = tempfile::tempdir().expect("tempdir");
    let old = fixture_old_file(dir.path());
    let before = common::manifest(dir.path());
    let ctx = Ctx::for_test(dir.path().to_path_buf());

    let report = exec::execute(&ctx, &plan(vec![old.clone()], true)).expect("dry-run");

    assert_eq!(report.exit_hint, ExitCode::Ok);
    assert!(old.exists());
    assert_eq!(before, common::manifest(dir.path()));
    assert!(!sweep::state::state_dir(dir.path()).exists());
}

#[test]
fn execution_refuses_protected_and_never_touch_paths_without_following_symlinks() {
    let dir = tempfile::tempdir().expect("tempdir");
    common::scan_fixture(dir.path());
    let outside = tempfile::tempdir().expect("outside");
    let outside_target = outside.path().join("target.txt");
    std::fs::write(&outside_target, b"outside").expect("outside file");
    let link = dir.path().join(".fake-tmp/link");
    std::os::unix::fs::symlink(&outside_target, &link).expect("symlink");
    let protected = dir.path().join(".fake-tmp/systemd-private-x/run/file");
    let user_data = dir.path().join("Documents/secret.txt");
    std::fs::create_dir_all(user_data.parent().expect("parent")).expect("mkdir");
    std::fs::write(&user_data, b"user data").expect("user data");
    let downloads = dir.path().join("Downloads/keep.iso");
    std::fs::create_dir_all(downloads.parent().expect("parent")).expect("mkdir");
    std::fs::write(&downloads, b"download").expect("download");
    let traversal = dir.path().join(".cache/../Documents/secret.txt");
    let ctx = Ctx::for_test(dir.path().to_path_buf());

    let report = exec::execute(
        &ctx,
        &plan(
            vec![
                protected,
                link.clone(),
                user_data.clone(),
                downloads.clone(),
                traversal,
            ],
            false,
        ),
    )
    .expect("execute");

    assert!(
        report.items[0]
            .skipped
            .iter()
            .any(|reason| reason.contains("protected"))
    );
    assert!(
        report.items[0]
            .errors
            .iter()
            .any(|error| error.contains("outside allowed prefix"))
    );
    assert!(
        report.items[0]
            .errors
            .iter()
            .any(|error| error.contains("non-normalized path"))
    );
    assert!(!link.exists(), "the link itself may be unlinked");
    assert!(outside_target.exists(), "the symlink target must survive");
    assert!(user_data.exists(), "never-touch user data must survive");
    assert!(
        downloads.exists(),
        "Downloads requires the destructive gate"
    );
}

#[test]
fn one_rule_error_does_not_stop_later_rules() {
    let dir = tempfile::tempdir().expect("tempdir");
    let old = fixture_old_file(dir.path());
    let ctx = Ctx::for_test(dir.path().to_path_buf());
    let mut execution_plan = plan(vec![PathBuf::from("/")], false);
    execution_plan.selected.push(PlanItem {
        id: "tmp",
        needs_sudo: false,
        tier: Tier::Safe,
        paths: vec![old.clone()],
        purge_ack: false,
    });

    let report = exec::execute(&ctx, &execution_plan).expect("execute");

    assert_eq!(report.exit_hint, ExitCode::ExecErr);
    assert!(
        report.items[0]
            .errors
            .iter()
            .any(|error| error.contains("/"))
    );
    assert!(!old.exists(), "later rules still execute after an error");
}

#[test]
fn plan_validation_rejects_destructive_and_sudo_items() {
    let mut destructive = plan(Vec::new(), true);
    destructive.selected[0].tier = Tier::Destructive;
    assert!(exec::execute(&Ctx::for_test(PathBuf::from("/tmp")), &destructive).is_err());

    let mut sudo = plan(Vec::new(), true);
    sudo.selected[0].needs_sudo = true;
    assert!(exec::execute(&Ctx::for_test(PathBuf::from("/tmp")), &sudo).is_err());
}

#[test]
fn scripted_safe_clean_runs_without_a_tty() {
    let sandbox = common::sandbox();
    common::scan_fixture(sandbox.path());

    Command::cargo_bin("sweep")
        .expect("binary")
        .args([
            "clean",
            "--only",
            "user-cache,thumbnails,trash",
            "--preset",
            "safe",
            "--yes",
            "--no-color",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("freed"));

    assert!(!sandbox.path().join(".cache/app-cache/big.bin").exists());
    assert!(!sandbox.path().join(".cache/thumbnails/f1.png").exists());
    assert!(
        !sandbox
            .path()
            .join(".local/share/Trash/files/trashed.dat")
            .exists()
    );
    assert!(
        sweep::state::state_dir(sandbox.path())
            .join("history.jsonl")
            .is_file()
    );
}

#[test]
fn scripted_dry_run_does_not_mutate_fixture_or_state() {
    let sandbox = common::sandbox();
    common::scan_fixture(sandbox.path());
    let before = common::manifest(sandbox.path());

    Command::cargo_bin("sweep")
        .expect("binary")
        .args([
            "clean",
            "--only",
            "user-cache,thumbnails,trash",
            "--preset",
            "safe",
            "--yes",
            "--dry-run",
            "--no-color",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("dry-run"));

    assert_eq!(before, common::manifest(sandbox.path()));
    assert!(!sweep::state::state_dir(sandbox.path()).exists());
}

#[test]
fn trash_flag_is_refused() {
    let _sandbox = common::sandbox();
    Command::cargo_bin("sweep")
        .expect("binary")
        .args(["clean", "--trash", "--yes", "--preset", "safe"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not implemented"));
}

#[test]
fn json_flag_is_refused() {
    let _sandbox = common::sandbox();
    Command::cargo_bin("sweep")
        .expect("binary")
        .args(["clean", "--json", "--yes", "--preset", "safe"])
        .assert()
        .failure()
        .code(1);
}
